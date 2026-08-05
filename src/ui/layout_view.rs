use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use crate::{
    layout::{auto_arrange, bounding_box, canvas_scale, canvas_to_world, move_selected, snap_position, world_to_canvas},
    monitor::Monitor,
    ui::{AppState, DragState, StatusLevel, Tab},
};

pub fn handle_key(event: KeyEvent, state: &mut AppState) {
    let shift = event.modifiers.contains(KeyModifiers::SHIFT);
    let step = if shift { 10 } else { 1 };
    let count = state.monitors.len();

    match event.code {
        KeyCode::Char('h') | KeyCode::Left => {
            state.push_undo();
            move_selected(&state.layout, &mut state.monitors, -step, 0);
            state.dirty = true;
        }
        KeyCode::Char('l') | KeyCode::Right => {
            state.push_undo();
            move_selected(&state.layout, &mut state.monitors, step, 0);
            state.dirty = true;
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.push_undo();
            move_selected(&state.layout, &mut state.monitors, 0, -step);
            state.dirty = true;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            state.push_undo();
            move_selected(&state.layout, &mut state.monitors, 0, step);
            state.dirty = true;
        }
        KeyCode::Tab | KeyCode::Char('n') => state.layout.next(count),
        KeyCode::BackTab | KeyCode::Char('p') => state.layout.prev(count),
        KeyCode::Char('[') => state.layout.zoom = (state.layout.zoom - 0.1).max(0.1),
        KeyCode::Char(']') => state.layout.zoom = (state.layout.zoom + 0.1).min(5.0),
        KeyCode::Char('0') => {
            state.push_undo();
            auto_arrange(&mut state.monitors);
            state.dirty = true;
        }
        KeyCode::Enter => {
            state.config.sync_from_monitor(state.layout.selected, &state.monitors);
            state.tab = Tab::Config;
        }
        _ => {}
    }
}

pub fn handle_mouse(event: MouseEvent, state: &mut AppState) {
    let col = event.column;
    let row = event.row;

    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            let canvas = canvas_area(state.terminal_size);
            if let Some(idx) = monitor_at(col, row, canvas, state) {
                let (min_x, min_y, _, _) = bounding_box(&state.monitors);
                let scale = canvas_scale_for(canvas, state);
                let (wx, wy) = canvas_to_world(col, row, scale, min_x, min_y, canvas.x + 1, canvas.y + 1);

                // Push undo at drag start, not on every move
                state.push_undo();
                state.layout.selected = idx;
                state.drag_state = Some(DragState {
                    monitor_idx: idx,
                    origin_x: state.monitors[idx].x,
                    origin_y: state.monitors[idx].y,
                    click_world_x: wx,
                    click_world_y: wy,
                });
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            if let Some(ref drag) = state.drag_state {
                let canvas = canvas_area(state.terminal_size);
                let (min_x, min_y, _, _) = bounding_box(&state.monitors);
                let scale = canvas_scale_for(canvas, state);
                let (wx, wy) = canvas_to_world(col, row, scale, min_x, min_y, canvas.x + 1, canvas.y + 1);

                let idx = drag.monitor_idx;
                let new_x = drag.origin_x + (wx - drag.click_world_x);
                let new_y = drag.origin_y + (wy - drag.click_world_y);
                let (sx, sy) = snap_position(idx, new_x, new_y, &state.monitors, state.layout.snap_threshold);
                state.monitors[idx].x = sx;
                state.monitors[idx].y = sy;
                state.dirty = true;
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            state.drag_state = None;
        }
        MouseEventKind::ScrollUp => {
            let canvas = canvas_area(state.terminal_size);
            if in_canvas(col, row, canvas) {
                state.layout.zoom = (state.layout.zoom + 0.1).min(5.0);
            } else {
                state.layout.prev(state.monitors.len());
            }
        }
        MouseEventKind::ScrollDown => {
            let canvas = canvas_area(state.terminal_size);
            if in_canvas(col, row, canvas) {
                state.layout.zoom = (state.layout.zoom - 0.1).max(0.1);
            } else {
                state.layout.next(state.monitors.len());
            }
        }
        _ => {}
    }
}

pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    if state.monitors.is_empty() {
        let msg = Paragraph::new("No monitors detected. Is Hyprland running?")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(msg, area);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    render_canvas(f, chunks[0], state);
    render_readout(f, chunks[1], state);
}

fn render_canvas(f: &mut Frame, area: Rect, state: &AppState) {
    let monitors = &state.monitors;
    let (min_x, min_y, _, _) = bounding_box(monitors);
    let scale = canvas_scale_for(area, state);

    // Pre-compute which monitors are overlapping (shown with red borders)
    let overlapping = overlapping_monitors(monitors);

    let selected = state.layout.selected;
    let draw_order: Vec<usize> = (0..monitors.len())
        .filter(|&i| i != selected)
        .chain(std::iter::once(selected))
        .collect();

    for i in draw_order {
        let m = &monitors[i];
        let (cx, cy) = world_to_canvas(m.x, m.y, scale, min_x, min_y, area.x + 1, area.y + 1);
        let cw = ((m.world_width() as f32 * scale) as u16).max(4);
        let ch = ((m.world_height() as f32 * scale * 0.5) as u16).max(2);

        let cw = cw.min(area.width.saturating_sub(cx.saturating_sub(area.x)));
        let ch = ch.min(area.height.saturating_sub(cy.saturating_sub(area.y)));
        if cw == 0 || ch == 0 {
            continue;
        }

        let rect = Rect { x: cx, y: cy, width: cw, height: ch };

        let is_selected = i == selected;
        let is_dragging = state.drag_state.as_ref().map(|d| d.monitor_idx == i).unwrap_or(false);
        let is_overlapping = overlapping[i];

        let border_style = if is_dragging {
            Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)
        } else if is_overlapping {
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
        } else if is_selected {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Blue)
        };

        let label = format!(" {} ", m.name);
        let mode_str = format!(" {}@{:.0}Hz ", m.active_mode.width, m.active_mode.refresh);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(if is_selected || is_dragging || is_overlapping {
                BorderType::Thick
            } else {
                BorderType::Rounded
            })
            .border_style(border_style)
            .title(Span::styled(&label, border_style))
            .title_bottom(Span::styled(&mode_str, Style::default().fg(Color::DarkGray)));

        f.render_widget(block, rect);
    }
}

fn render_readout(f: &mut Frame, area: Rect, state: &AppState) {
    if state.monitors.is_empty() {
        return;
    }
    let idx = state.layout.selected.min(state.monitors.len() - 1);
    let m = &state.monitors[idx];

    let mirror_info = m.mirror_of.as_ref()
        .map(|src| format!("  mirror:{}", src))
        .unwrap_or_default();

    let overlap_warn = if overlapping_monitors(&state.monitors)[idx] {
        "  ⚠ OVERLAP"
    } else {
        ""
    };

    let drag_hint = if state.drag_state.is_some() { "  [dragging]" } else { "" };

    let text = format!(
        " {}  x:{}  y:{}  {}x{}@{:.0}Hz  scale:{:.2}{}{}{}",
        m.name, m.x, m.y,
        m.active_mode.width, m.active_mode.height, m.active_mode.refresh,
        m.scale,
        mirror_info, overlap_warn, drag_hint,
    );
    f.render_widget(
        Paragraph::new(text).style(Style::default().fg(Color::Cyan)),
        area,
    );
}

/// Returns a bool per monitor: true if it overlaps any other monitor.
fn overlapping_monitors(monitors: &[Monitor]) -> Vec<bool> {
    let mut flags = vec![false; monitors.len()];
    for i in 0..monitors.len() {
        for j in (i + 1)..monitors.len() {
            let a = &monitors[i];
            let b = &monitors[j];
            if a.x < b.right_edge() && a.right_edge() > b.x
                && a.y < b.bottom_edge() && a.bottom_edge() > b.y
            {
                flags[i] = true;
                flags[j] = true;
            }
        }
    }
    flags
}

/// Canvas rect from terminal size.
/// Tab bar = y 0-1 (height 2), bottom bar = last row (height 1), readout = 1 row inside content.
pub fn canvas_area(terminal_size: (u16, u16)) -> Rect {
    let (tw, th) = terminal_size;
    Rect {
        x: 0,
        y: 2,
        width: tw,
        height: th.saturating_sub(4),
    }
}

fn in_canvas(col: u16, row: u16, canvas: Rect) -> bool {
    col > canvas.x
        && col < canvas.x + canvas.width.saturating_sub(1)
        && row > canvas.y
        && row < canvas.y + canvas.height.saturating_sub(1)
}

fn canvas_scale_for(area: Rect, state: &AppState) -> f32 {
    let w = area.width.saturating_sub(2);
    let h = area.height.saturating_sub(2);
    canvas_scale(&state.monitors, w, h, state.layout.zoom)
}

/// Hit-test: which monitor (if any) is at canvas position (col, row)?
fn monitor_at(col: u16, row: u16, canvas: Rect, state: &AppState) -> Option<usize> {
    if state.monitors.is_empty() {
        return None;
    }
    let monitors = &state.monitors;
    let (min_x, min_y, _, _) = bounding_box(monitors);
    let scale = canvas_scale_for(canvas, state);
    let pad_x = canvas.x + 1;
    let pad_y = canvas.y + 1;

    // Check selected first (renders on top)
    let selected = state.layout.selected;
    let order: Vec<usize> = std::iter::once(selected)
        .chain((0..monitors.len()).filter(|&i| i != selected))
        .collect();

    for i in order {
        let m = &monitors[i];
        let (cx, cy) = world_to_canvas(m.x, m.y, scale, min_x, min_y, pad_x, pad_y);
        let cw = ((m.world_width() as f32 * scale) as u16).max(4);
        let ch = ((m.world_height() as f32 * scale * 0.5) as u16).max(2);
        if col >= cx && col < cx + cw && row >= cy && row < cy + ch {
            return Some(i);
        }
    }
    None
}

pub fn trigger_save(state: &mut AppState) {
    use crate::monitor::format_hypr_line;
    use std::path::PathBuf;

    let path: PathBuf = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()))
        .join("hypr/monitors.conf");

    let is_new = !path.exists();
    let mut lines = vec!["# Generated by breadmon — do not edit by hand".to_owned()];
    for m in &state.monitors {
        lines.push(format_hypr_line(m));
    }
    let content = lines.join("\n") + "\n";

    match std::fs::write(&path, &content) {
        Ok(()) => {
            if is_new {
                state.set_status(
                    format!("Saved. Add: source = {} to hyprland.conf", path.display()),
                    StatusLevel::Success,
                );
            } else {
                state.set_status(format!("Saved to {}", path.display()), StatusLevel::Success);
            }
            state.dirty = false;
        }
        Err(e) => state.set_status(format!("Save failed: {}", e), StatusLevel::Error),
    }
}
