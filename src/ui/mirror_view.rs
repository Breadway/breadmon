use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::{
    mirror::{find_mirror_modes, refresh_match_label, MirrorResult},
    ui::{AppState, StatusLevel},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirrorField {
    Source,
    Target,
    Compute,
    Apply,
    Cancel,
}

#[derive(Debug, Default)]
pub struct MirrorState {
    pub source_idx: usize,
    pub target_idx: usize,
    pub result: Option<MirrorResult>,
    pub focused: usize, // index into FIELDS
}

const FIELDS: &[MirrorField] = &[
    MirrorField::Source,
    MirrorField::Target,
    MirrorField::Compute,
    MirrorField::Apply,
    MirrorField::Cancel,
];

impl MirrorState {
    fn current_field(&self) -> MirrorField {
        FIELDS[self.focused.min(FIELDS.len() - 1)]
    }

    fn next_field(&mut self) {
        // Skip Apply/Cancel if no result yet
        let mut next = (self.focused + 1) % FIELDS.len();
        if self.result.is_none() && (FIELDS[next] == MirrorField::Apply || FIELDS[next] == MirrorField::Cancel) {
            next = 0;
        }
        self.focused = next;
    }

    fn prev_field(&mut self) {
        let len = FIELDS.len();
        let mut prev = self.focused.checked_sub(1).unwrap_or(len - 1);
        if self.result.is_none() && (FIELDS[prev] == MirrorField::Apply || FIELDS[prev] == MirrorField::Cancel) {
            prev = FIELDS.iter().position(|&f| f == MirrorField::Compute).unwrap_or(2);
        }
        self.focused = prev;
    }

    /// Ensure source and target are different
    pub fn fix_indices(&mut self, count: usize) {
        if count < 2 {
            return;
        }
        if self.source_idx == self.target_idx {
            self.target_idx = (self.source_idx + 1) % count;
        }
    }
}

pub fn handle_key(event: KeyEvent, state: &mut AppState) {
    let count = state.monitors.len();
    if count < 2 {
        return;
    }

    match event.code {
        KeyCode::Tab | KeyCode::Char('j') | KeyCode::Down => {
            state.mirror.next_field();
        }
        KeyCode::BackTab | KeyCode::Char('k') | KeyCode::Up => {
            state.mirror.prev_field();
        }
        KeyCode::Char('h') | KeyCode::Left => match state.mirror.current_field() {
            MirrorField::Source => {
                state.mirror.source_idx = state.mirror.source_idx.checked_sub(1).unwrap_or(count - 1);
                state.mirror.fix_indices(count);
                state.mirror.result = None;
            }
            MirrorField::Target => {
                state.mirror.target_idx = state.mirror.target_idx.checked_sub(1).unwrap_or(count - 1);
                state.mirror.fix_indices(count);
                state.mirror.result = None;
            }
            _ => {}
        },
        KeyCode::Char('l') | KeyCode::Right => match state.mirror.current_field() {
            MirrorField::Source => {
                state.mirror.source_idx = (state.mirror.source_idx + 1) % count;
                state.mirror.fix_indices(count);
                state.mirror.result = None;
            }
            MirrorField::Target => {
                state.mirror.target_idx = (state.mirror.target_idx + 1) % count;
                state.mirror.fix_indices(count);
                state.mirror.result = None;
            }
            _ => {}
        },
        KeyCode::Enter => match state.mirror.current_field() {
            MirrorField::Compute => {
                let src = &state.monitors[state.mirror.source_idx];
                let tgt = &state.monitors[state.mirror.target_idx];
                match find_mirror_modes(src, tgt) {
                    Some(result) => {
                        state.mirror.result = Some(result);
                        // Move focus to Apply
                        state.mirror.focused = FIELDS.iter().position(|&f| f == MirrorField::Apply).unwrap_or(3);
                    }
                    None => {
                        state.set_status(
                            "No compatible modes found between these monitors.",
                            StatusLevel::Error,
                        );
                    }
                }
            }
            MirrorField::Apply => {
                if let Some(result) = state.mirror.result.clone() {
                    state.push_undo();
                    let src_name = state.monitors[state.mirror.source_idx].name.clone();
                    let tgt_idx = state.mirror.target_idx;

                    state.monitors[tgt_idx].active_mode = result.mirror_mode.clone();
                    state.monitors[tgt_idx].mirror_of = Some(src_name.clone());

                    state.mark_dirty();
                    state.mirror.result = None;
                    state.mirror.focused = 0;
                    state.set_status(
                        format!(
                            "Mirror set: {} → {} at {}",
                            src_name,
                            state.monitors[tgt_idx].name,
                            result.mirror_mode
                        ),
                        StatusLevel::Success,
                    );
                }
            }
            MirrorField::Cancel => {
                state.mirror.result = None;
                state.mirror.focused = 0;
            }
            _ => {}
        },
        KeyCode::Esc => {
            state.mirror.result = None;
            state.mirror.focused = 0;
        }
        _ => {}
    }
}

pub fn handle_mouse(event: MouseEvent, state: &mut AppState) {
    let count = state.monitors.len();
    if count < 2 {
        return;
    }

    let row = event.row;
    // Content area starts at y=2.
    // Pickers chunk: y=2..5 (height 4).  Source line at y=2, target at y=4.
    // Compute button: y=6.
    // Result panel: y=7+ (border at y=7, buttons at ~y=13 for a typical terminal).

    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            match row {
                2 | 3 => {
                    // Source picker area
                    let f_idx = FIELDS.iter().position(|&f| f == MirrorField::Source).unwrap_or(0);
                    state.mirror.focused = f_idx;
                }
                4 | 5 => {
                    // Target picker area
                    let f_idx = FIELDS.iter().position(|&f| f == MirrorField::Target).unwrap_or(1);
                    state.mirror.focused = f_idx;
                }
                6 => {
                    // Compute button
                    let f_idx = FIELDS.iter().position(|&f| f == MirrorField::Compute).unwrap_or(2);
                    state.mirror.focused = f_idx;
                    // Also activate it
                    let src = &state.monitors[state.mirror.source_idx];
                    let tgt = &state.monitors[state.mirror.target_idx];
                    match crate::mirror::find_mirror_modes(src, tgt) {
                        Some(result) => {
                            state.mirror.result = Some(result);
                            state.mirror.focused = FIELDS.iter().position(|&f| f == MirrorField::Apply).unwrap_or(3);
                        }
                        None => {
                            state.set_status(
                                "No compatible modes found between these monitors.",
                                crate::ui::StatusLevel::Error,
                            );
                        }
                    }
                }
                r if r >= 7
                    // Result panel: Apply is on the line with buttons.
                    // Rough column check: col < 20 = Apply, col >= 20 = Cancel
                    && state.mirror.result.is_some() =>
                {
                    let col = event.column;
                    if col < 20 {
                        // Activate Apply
                        state.mirror.focused = FIELDS.iter().position(|&f| f == MirrorField::Apply).unwrap_or(3);
                        if let Some(result) = state.mirror.result.clone() {
                            state.push_undo();
                            let src_name = state.monitors[state.mirror.source_idx].name.clone();
                            let tgt_idx = state.mirror.target_idx;
                            state.monitors[tgt_idx].active_mode = result.mirror_mode.clone();
                            state.monitors[tgt_idx].mirror_of = Some(src_name.clone());
                            state.mark_dirty();
                            state.mirror.result = None;
                            state.mirror.focused = 0;
                            state.set_status(
                                format!("Mirror set: {} → {} at {}", src_name, state.monitors[tgt_idx].name, result.mirror_mode),
                                crate::ui::StatusLevel::Success,
                            );
                        }
                    } else {
                        // Cancel
                        state.mirror.result = None;
                        state.mirror.focused = 0;
                    }
                }
                _ => {}
            }
        }
        MouseEventKind::ScrollUp => {
            // Scroll in source/target pickers to cycle monitors
            match state.mirror.current_field() {
                MirrorField::Source => {
                    state.mirror.source_idx = state.mirror.source_idx.checked_sub(1).unwrap_or(count - 1);
                    state.mirror.fix_indices(count);
                    state.mirror.result = None;
                }
                MirrorField::Target => {
                    state.mirror.target_idx = state.mirror.target_idx.checked_sub(1).unwrap_or(count - 1);
                    state.mirror.fix_indices(count);
                    state.mirror.result = None;
                }
                _ => {}
            }
        }
        MouseEventKind::ScrollDown => {
            match state.mirror.current_field() {
                MirrorField::Source => {
                    state.mirror.source_idx = (state.mirror.source_idx + 1) % count;
                    state.mirror.fix_indices(count);
                    state.mirror.result = None;
                }
                MirrorField::Target => {
                    state.mirror.target_idx = (state.mirror.target_idx + 1) % count;
                    state.mirror.fix_indices(count);
                    state.mirror.result = None;
                }
                _ => {}
            }
        }
        _ => {}
    }
}

pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    let count = state.monitors.len();

    if count < 2 {
        f.render_widget(
            Paragraph::new("Need at least 2 monitors for mirror setup.")
                .style(Style::default().fg(Color::DarkGray)),
            area,
        );
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // pickers
            Constraint::Length(1), // compute button
            Constraint::Min(0),    // result / hint
        ])
        .split(area);

    render_pickers(f, chunks[0], state, count);
    render_compute_btn(f, chunks[1], state);
    if let Some(ref result) = state.mirror.result {
        render_result(f, chunks[2], state, result);
    } else {
        let hint = Paragraph::new("  Press Enter on [Compute] to find the best matching modes.")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(hint, chunks[2]);
    }
}

fn render_pickers(f: &mut Frame, area: Rect, state: &AppState, count: usize) {
    let src_name = state
        .monitors
        .get(state.mirror.source_idx.min(count - 1))
        .map(|m| m.name.as_str())
        .unwrap_or("?");
    let tgt_name = state
        .monitors
        .get(state.mirror.target_idx.min(count - 1))
        .map(|m| m.name.as_str())
        .unwrap_or("?");

    let focused = state.mirror.current_field();

    let src_style = if focused == MirrorField::Source {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let tgt_style = if focused == MirrorField::Target {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let lines = vec![
        Line::from(vec![
            Span::raw("  Source  "),
            Span::styled(format!("[ {} ]", src_name), src_style),
            Span::raw("  (mirror this output)"),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::raw("  Target  "),
            Span::styled(format!("[ {} ]", tgt_name), tgt_style),
            Span::raw("  (the mirroring output)"),
        ]),
    ];

    f.render_widget(Paragraph::new(lines), area);
}

fn render_compute_btn(f: &mut Frame, area: Rect, state: &AppState) {
    let focused = state.mirror.current_field() == MirrorField::Compute;
    let style = if focused {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    f.render_widget(
        Paragraph::new("  [ Compute Best Match ]").style(style),
        area,
    );
}

fn render_result(f: &mut Frame, area: Rect, state: &AppState, result: &MirrorResult) {
    let ar = result.ar_ratio;
    let ar_label = if result.ar_exact {
        format!("{}:{} (exact)", ar.0, ar.1)
    } else {
        format!("{}:{} (approx)", ar.0, ar.1)
    };

    let refresh_label = refresh_match_label(result);

    let focused = state.mirror.current_field();
    let apply_style = if focused == MirrorField::Apply {
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let cancel_style = if focused == MirrorField::Cancel {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let lines = vec![
        Line::raw(""),
        Line::from(Span::styled(
            format!("  Source mode:  {}  (AR {})", result.source_mode, ar_label),
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            format!("  Mirror mode:  {}", result.mirror_mode),
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            format!("  Refresh:      {:.2} Hz  ({})", result.refresh, refresh_label),
            Style::default().fg(Color::White),
        )),
        Line::raw(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("[ Apply Mirror ]", apply_style),
            Span::raw("   "),
            Span::styled("[ Cancel ]", cancel_style),
        ]),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" Result ");

    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(Paragraph::new(lines), inner);
}
