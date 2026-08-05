use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::{
    monitor::{Monitor, Transform},
    ui::{AppState, StatusLevel},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigField {
    Resolution,
    Refresh,
    Scale,
    Transform,
    Vrr,
    Dpms,
    MirrorOf,
}

impl ConfigField {
    const ALL: &'static [ConfigField] = &[
        Self::Resolution,
        Self::Refresh,
        Self::Scale,
        Self::Transform,
        Self::Vrr,
        Self::Dpms,
        Self::MirrorOf,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Resolution => "Resolution",
            Self::Refresh => "Refresh",
            Self::Scale => "Scale",
            Self::Transform => "Transform",
            Self::Vrr => "VRR",
            Self::Dpms => "DPMS",
            Self::MirrorOf => "Mirror of",
        }
    }
}

#[derive(Debug, Default)]
pub struct ConfigState {
    pub monitor_idx: usize,
    pub focused: usize, // index into ConfigField::ALL
    // Dropdown indices
    pub res_idx: usize,
    pub refresh_idx: usize,
    pub transform_idx: usize,
    pub mirror_idx: usize, // 0 = none, 1.. = monitor names
    // Scale editing
    pub scale_str: String,
    pub scale_editing: bool,
    // Cached lists
    pub resolutions: Vec<(u32, u32)>,
    pub refreshes: Vec<f64>,
    pub mirror_options: Vec<String>, // "(none)", then monitor names
}

impl ConfigState {
    pub fn sync_from_monitor(&mut self, idx: usize, monitors: &[Monitor]) {
        if monitors.is_empty() {
            return;
        }
        let idx = idx.min(monitors.len() - 1);
        self.monitor_idx = idx;
        let m = &monitors[idx];

        self.resolutions = m.unique_resolutions();
        // Find current resolution index
        self.res_idx = self
            .resolutions
            .iter()
            .position(|&(w, h)| w == m.active_mode.width && h == m.active_mode.height)
            .unwrap_or(0);

        self.update_refreshes(m);

        self.transform_idx = m.transform.as_u8() as usize;
        self.scale_str = format!("{:.2}", m.scale);
        self.scale_editing = false;

        // Mirror options
        self.mirror_options = std::iter::once("(none)".to_owned())
            .chain(
                monitors
                    .iter()
                    .filter(|other| other.name != m.name)
                    .map(|other| other.name.clone()),
            )
            .collect();

        self.mirror_idx = m
            .mirror_of
            .as_deref()
            .and_then(|src| self.mirror_options.iter().position(|o| o == src))
            .unwrap_or(0);
    }

    fn update_refreshes(&mut self, m: &Monitor) {
        if let Some(&(w, h)) = self.resolutions.get(self.res_idx) {
            self.refreshes = m.refreshes_for(w, h);
            self.refresh_idx = self
                .refreshes
                .iter()
                .position(|&r| (r - m.active_mode.refresh).abs() < 0.01)
                .unwrap_or(0);
        }
    }

    fn current_field(&self) -> ConfigField {
        ConfigField::ALL[self.focused.min(ConfigField::ALL.len() - 1)]
    }

    fn next_field(&mut self) {
        self.focused = (self.focused + 1) % ConfigField::ALL.len();
    }

    fn prev_field(&mut self) {
        self.focused = self.focused.checked_sub(1).unwrap_or(ConfigField::ALL.len() - 1);
    }
}

pub fn handle_key(event: KeyEvent, state: &mut AppState) {
    let cfg = &mut state.config;

    match event.code {
        KeyCode::Char('j') | KeyCode::Down => {
            cfg.scale_editing = false;
            cfg.next_field();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            cfg.scale_editing = false;
            cfg.prev_field();
        }
        KeyCode::Tab => {
            cfg.scale_editing = false;
            cfg.next_field();
        }
        KeyCode::BackTab => {
            cfg.scale_editing = false;
            cfg.prev_field();
        }
        // Navigate between monitors
        KeyCode::Char('[') => {
            let count = state.monitors.len();
            if count > 0 {
                let new_idx = state.config.monitor_idx.checked_sub(1).unwrap_or(count - 1);
                state.layout.selected = new_idx;
                state.config.sync_from_monitor(new_idx, &state.monitors);
            }
        }
        KeyCode::Char(']') => {
            let count = state.monitors.len();
            if count > 0 {
                let new_idx = (state.config.monitor_idx + 1) % count;
                state.layout.selected = new_idx;
                state.config.sync_from_monitor(new_idx, &state.monitors);
            }
        }
        KeyCode::Char('a') => {
            apply_current(state);
        }
        KeyCode::Char('s') => {
            crate::ui::layout_view::trigger_save(state);
        }
        KeyCode::Esc => {
            state.config.scale_editing = false;
            // Re-sync from live monitor to discard pending edits
            let idx = state.config.monitor_idx;
            state.config.sync_from_monitor(idx, &state.monitors);
        }
        KeyCode::Enter => {
            if state.config.current_field() == ConfigField::Scale {
                commit_scale(state);
            }
            apply_current(state);
        }
        _ => {
            state.push_undo();
            handle_field_key(event, state);
        }
    }
}

pub fn handle_mouse(event: MouseEvent, state: &mut AppState) {
    if state.monitors.is_empty() {
        return;
    }
    let row = event.row;
    // Content area starts at y=2 (after tab bar).
    // Config view: header (Length(2)) at y=2-3, list block starts at y=4.
    // List border top at y=4, items at y=5+.
    let items_start_row = 5u16;

    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if row >= items_start_row {
                let field_idx = (row - items_start_row) as usize;
                if field_idx < ConfigField::ALL.len() {
                    state.config.focused = field_idx;
                }
            }
        }
        MouseEventKind::ScrollUp => {
            state.push_undo();
            let fake_right = KeyEvent::new(KeyCode::Right, crossterm::event::KeyModifiers::NONE);
            handle_field_key(fake_right, state);
        }
        MouseEventKind::ScrollDown => {
            state.push_undo();
            let fake_left = KeyEvent::new(KeyCode::Left, crossterm::event::KeyModifiers::NONE);
            handle_field_key(fake_left, state);
        }
        _ => {}
    }
}

fn handle_field_key(event: KeyEvent, state: &mut AppState) {
    let monitors_len = state.monitors.len();
    if monitors_len == 0 {
        return;
    }
    let idx = state.config.monitor_idx.min(monitors_len - 1);

    match state.config.current_field() {
        ConfigField::Resolution => match event.code {
            KeyCode::Char('h') | KeyCode::Left => {
                if state.config.res_idx > 0 {
                    state.config.res_idx -= 1;
                    let m = &state.monitors[idx];
                    state.config.update_refreshes(m);
                    sync_mode_to_monitor(state, idx);
                    state.dirty = true;
                }
            }
            KeyCode::Char('l') | KeyCode::Right
                if state.config.res_idx + 1 < state.config.resolutions.len() =>
            {
                state.config.res_idx += 1;
                let m = &state.monitors[idx];
                state.config.update_refreshes(m);
                sync_mode_to_monitor(state, idx);
                state.dirty = true;
            }
            _ => {}
        },
        ConfigField::Refresh => match event.code {
            KeyCode::Char('h') | KeyCode::Left => {
                if state.config.refresh_idx > 0 {
                    state.config.refresh_idx -= 1;
                    sync_mode_to_monitor(state, idx);
                    state.dirty = true;
                }
            }
            KeyCode::Char('l') | KeyCode::Right
                if state.config.refresh_idx + 1 < state.config.refreshes.len() =>
            {
                state.config.refresh_idx += 1;
                sync_mode_to_monitor(state, idx);
                state.dirty = true;
            }
            _ => {}
        },
        ConfigField::Scale => match event.code {
            KeyCode::Char(',') => {
                let s = state.monitors[idx].scale - 0.1;
                state.monitors[idx].scale = (s * 100.0).round() / 100.0;
                state.monitors[idx].scale = state.monitors[idx].scale.max(0.1);
                state.config.scale_str = format!("{:.2}", state.monitors[idx].scale);
                state.dirty = true;
            }
            KeyCode::Char('.') => {
                let s = state.monitors[idx].scale + 0.1;
                state.monitors[idx].scale = (s * 100.0).round() / 100.0;
                state.monitors[idx].scale = state.monitors[idx].scale.min(10.0);
                state.config.scale_str = format!("{:.2}", state.monitors[idx].scale);
                state.dirty = true;
            }
            KeyCode::Char(c) if c.is_ascii_digit() || c == '.' => {
                state.config.scale_editing = true;
                state.config.scale_str.push(c);
            }
            KeyCode::Backspace => {
                state.config.scale_str.pop();
            }
            _ => {}
        },
        ConfigField::Transform => match event.code {
            KeyCode::Char('h') | KeyCode::Left => {
                let all = Transform::all();
                state.config.transform_idx = state
                    .config
                    .transform_idx
                    .checked_sub(1)
                    .unwrap_or(all.len() - 1);
                state.monitors[idx].transform = all[state.config.transform_idx];
                state.dirty = true;
            }
            KeyCode::Char('l') | KeyCode::Right => {
                let all = Transform::all();
                state.config.transform_idx = (state.config.transform_idx + 1) % all.len();
                state.monitors[idx].transform = all[state.config.transform_idx];
                state.dirty = true;
            }
            _ => {}
        },
        ConfigField::Vrr => match event.code {
            KeyCode::Char('h') | KeyCode::Left | KeyCode::Char('l') | KeyCode::Right | KeyCode::Char(' ') => {
                state.monitors[idx].vrr = !state.monitors[idx].vrr;
                state.dirty = true;
            }
            _ => {}
        },
        ConfigField::Dpms => match event.code {
            KeyCode::Char('h') | KeyCode::Left | KeyCode::Char('l') | KeyCode::Right | KeyCode::Char(' ') => {
                state.monitors[idx].dpms = !state.monitors[idx].dpms;
                state.dirty = true;
            }
            _ => {}
        },
        ConfigField::MirrorOf => match event.code {
            KeyCode::Char('h') | KeyCode::Left => {
                if state.config.mirror_idx > 0 {
                    state.config.mirror_idx -= 1;
                    sync_mirror_to_monitor(state, idx);
                    state.dirty = true;
                }
            }
            KeyCode::Char('l') | KeyCode::Right
                if state.config.mirror_idx + 1 < state.config.mirror_options.len() =>
            {
                state.config.mirror_idx += 1;
                sync_mirror_to_monitor(state, idx);
                state.dirty = true;
            }
            _ => {}
        },
    }
}

fn sync_mode_to_monitor(state: &mut AppState, idx: usize) {
    if let Some(&(w, h)) = state.config.resolutions.get(state.config.res_idx) {
        if let Some(&r) = state.config.refreshes.get(state.config.refresh_idx) {
            state.monitors[idx].active_mode.width = w;
            state.monitors[idx].active_mode.height = h;
            state.monitors[idx].active_mode.refresh = r;
        }
    }
}

fn sync_mirror_to_monitor(state: &mut AppState, idx: usize) {
    let chosen = state.config.mirror_options.get(state.config.mirror_idx).cloned();
    state.monitors[idx].mirror_of = match chosen.as_deref() {
        Some("(none)") | None => None,
        Some(s) => Some(s.to_owned()),
    };
}

fn commit_scale(state: &mut AppState) {
    let idx = state.config.monitor_idx.min(state.monitors.len().saturating_sub(1));
    if let Ok(v) = state.config.scale_str.parse::<f64>() {
        state.monitors[idx].scale = v.clamp(0.1, 10.0);
        state.config.scale_str = format!("{:.2}", state.monitors[idx].scale);
        state.dirty = true;
    }
    state.config.scale_editing = false;
}

fn apply_current(state: &mut AppState) {
    state.pending_apply = true;
    state.set_status("Applying...", StatusLevel::Info);
    state.dirty = false;
}

pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    if state.monitors.is_empty() {
        f.render_widget(
            Paragraph::new("No monitors detected.").style(Style::default().fg(Color::DarkGray)),
            area,
        );
        return;
    }

    let idx = state.config.monitor_idx.min(state.monitors.len() - 1);
    let m = &state.monitors[idx];

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(0)])
        .split(area);

    // Monitor header with PPI hint
    let ppi_hint = match (m.ppi(), m.suggested_scale()) {
        (Some(ppi), Some(scale)) => format!("  │  {:.0} PPI  (suggested scale: {:.2})", ppi, scale),
        _ => String::new(),
    };
    let header = format!(" {} — {}{}", m.name, m.description, ppi_hint);
    f.render_widget(
        Paragraph::new(header).style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        chunks[0],
    );

    let form_area = chunks[1];
    let row_height = 1u16;
    let fields = ConfigField::ALL;

    let items: Vec<ListItem> = fields
        .iter()
        .enumerate()
        .map(|(i, &field)| {
            let is_focused = i == state.config.focused;
            let value = field_value(field, state, m);
            let label = format!("  {:12}  {}", field.label(), value);
            let style = if is_focused {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
                    .bg(Color::DarkGray)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(Span::styled(label, style)))
        })
        .collect();

    let _ = row_height; // used implicitly via ListItem heights
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Config "),
    );

    let mut list_state = ListState::default();
    list_state.select(Some(state.config.focused));
    f.render_stateful_widget(list, form_area, &mut list_state);
}

fn field_value(field: ConfigField, state: &AppState, m: &Monitor) -> String {
    match field {
        ConfigField::Resolution => {
            if let Some(&(w, h)) = state.config.resolutions.get(state.config.res_idx) {
                format!("{}x{}  (h/l to change)", w, h)
            } else {
                format!("{}x{}", m.active_mode.width, m.active_mode.height)
            }
        }
        ConfigField::Refresh => {
            if let Some(&r) = state.config.refreshes.get(state.config.refresh_idx) {
                format!("{:.2} Hz  (h/l to change)", r)
            } else {
                format!("{:.2} Hz", m.active_mode.refresh)
            }
        }
        ConfigField::Scale => {
            if state.config.scale_editing {
                format!("{}|  (Enter to commit, ,/. for ±0.1)", state.config.scale_str)
            } else {
                format!("{}  (,/. for ±0.1)", state.config.scale_str)
            }
        }
        ConfigField::Transform => Transform::all()
            [state.config.transform_idx.min(Transform::all().len() - 1)]
            .label()
            .to_owned(),
        ConfigField::Vrr => {
            if m.vrr { "ON".to_owned() } else { "OFF".to_owned() }
        }
        ConfigField::Dpms => {
            if m.dpms { "ON".to_owned() } else { "OFF".to_owned() }
        }
        ConfigField::MirrorOf => state
            .config
            .mirror_options
            .get(state.config.mirror_idx)
            .cloned()
            .unwrap_or_else(|| "(none)".to_owned()),
    }
}
