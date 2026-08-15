pub mod config_view;
pub mod layout_view;
pub mod mirror_view;
pub mod profiles_view;

use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph, Tabs},
    Frame,
};

use crate::{layout::LayoutState, monitor::Monitor};

use config_view::ConfigState;
use mirror_view::MirrorState;
use profiles_view::ProfilesState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Layout,
    Config,
    Mirror,
    Profiles,
}

impl Tab {
    pub fn index(self) -> usize {
        match self {
            Self::Layout => 0,
            Self::Config => 1,
            Self::Mirror => 2,
            Self::Profiles => 3,
        }
    }

    fn from_col(col: u16) -> Option<Self> {
        // Tab titles: " 1 Layout "(10), " 2 Config "(10), " 3 Mirror "(10), " 4 Profiles "(12)
        // Each separated by a 1-char divider │
        const WIDTHS: [u16; 4] = [10, 10, 10, 12];
        let mut x = 0u16;
        for (i, &w) in WIDTHS.iter().enumerate() {
            if col >= x && col < x + w {
                return Some(match i {
                    0 => Self::Layout,
                    1 => Self::Config,
                    2 => Self::Mirror,
                    3 => Self::Profiles,
                    _ => unreachable!(),
                });
            }
            x += w + 1;
        }
        None
    }

    fn hints(self) -> &'static str {
        match self {
            Self::Layout =>
                " [hjkl/drag]move  [Tab]next  [Shift+hjkl]×10  [0]arrange  [Ctrl+Z]undo  [a]apply  [s]save",
            Self::Config =>
                " [jk]field  [hl]value  [,.]scale  [Enter]apply  [Esc]cancel  [\\[\\]]monitor  [Ctrl+Z]undo",
            Self::Mirror =>
                " [Tab]focus  [hl]cycle  [Enter]confirm  [Ctrl+Z]undo",
            Self::Profiles =>
                " [jk]nav  [Enter]load  [d×2]delete  [Tab]name  [Ctrl+Z]undo",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusLevel {
    Info,
    Success,
    Error,
}

pub struct StatusMsg {
    pub text: String,
    pub level: StatusLevel,
    pub born: Instant,
}

/// Drag state for moving monitors in the layout canvas.
pub struct DragState {
    pub monitor_idx: usize,
    pub origin_x: i32,
    pub origin_y: i32,
    pub click_world_x: i32,
    pub click_world_y: i32,
}

pub struct AppState {
    pub monitors: Vec<Monitor>,
    pub tab: Tab,
    pub layout: LayoutState,
    pub config: ConfigState,
    pub mirror: MirrorState,
    pub profiles: ProfilesState,
    pub status: Option<StatusMsg>,
    pub dirty: bool,
    pub quit_confirm: bool,
    pub drag_state: Option<DragState>,
    pub terminal_size: (u16, u16),
    /// Set to true by any handler that wants `main.rs` to run `apply_monitors`.
    pub pending_apply: bool,
    /// Named snapshot last loaded or saved this session. Cleared when the
    /// in-memory layout is edited, so `bread.mon.applied` can report it
    /// honestly (or `null` for an ad-hoc layout).
    pub active_profile: Option<String>,
    /// Snapshots for Ctrl+Z undo (up to 20 deep).
    pub undo_stack: Vec<Vec<Monitor>>,
}

impl AppState {
    pub fn new(monitors: Vec<Monitor>, terminal_size: (u16, u16)) -> Self {
        let profiles_list = crate::profile::list().unwrap_or_default();
        Self {
            monitors,
            tab: Tab::Layout,
            layout: LayoutState::default(),
            config: ConfigState::default(),
            mirror: MirrorState::default(),
            profiles: ProfilesState::new(profiles_list),
            status: None,
            dirty: false,
            quit_confirm: false,
            drag_state: None,
            terminal_size,
            pending_apply: false,
            active_profile: None,
            undo_stack: Vec::new(),
        }
    }

    pub fn set_status(&mut self, text: impl Into<String>, level: StatusLevel) {
        self.status = Some(StatusMsg {
            text: text.into(),
            level,
            born: Instant::now(),
        });
    }

    /// Mark the in-memory layout as edited. Also forgets `active_profile`
    /// — a mutated layout is no longer the named snapshot that was loaded
    /// or saved.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
        self.active_profile = None;
    }

    pub fn tick_status(&mut self) {
        if let Some(s) = &self.status {
            if s.born.elapsed().as_secs() >= 3 {
                self.status = None;
            }
        }
    }

    pub fn switch_tab(&mut self, tab: Tab) {
        self.tab = tab;
        if tab == Tab::Config {
            self.config
                .sync_from_monitor(self.layout.selected, &self.monitors);
        }
    }

    /// Save a monitor snapshot for undo (max 20 entries).
    pub fn push_undo(&mut self) {
        self.undo_stack.push(self.monitors.clone());
        if self.undo_stack.len() > 20 {
            self.undo_stack.remove(0);
        }
    }

    /// Restore the most recent undo snapshot.
    pub fn undo(&mut self) {
        if let Some(snapshot) = self.undo_stack.pop() {
            self.monitors = snapshot;
            self.mark_dirty();
            self.layout.clamp_selected(self.monitors.len());
            // Re-sync config view to the restored state
            let idx = self.layout.selected;
            self.config.sync_from_monitor(idx, &self.monitors);
            self.set_status("Undone.", StatusLevel::Info);
        } else {
            self.set_status("Nothing to undo.", StatusLevel::Info);
        }
    }
}

/// Returns true if the app should continue running, false to quit.
pub fn handle_key(event: KeyEvent, state: &mut AppState) -> bool {
    // Quit
    if event.code == KeyCode::Char('q')
        || (event.code == KeyCode::Char('c') && event.modifiers.contains(KeyModifiers::CONTROL))
    {
        if state.dirty && !state.quit_confirm {
            state.set_status("Unsaved changes. Press q again to quit.", StatusLevel::Info);
            state.quit_confirm = true;
            return true;
        }
        return false;
    }
    state.quit_confirm = false;

    // Global: Ctrl+Z undo
    if event.code == KeyCode::Char('z') && event.modifiers.contains(KeyModifiers::CONTROL) {
        state.undo();
        return true;
    }

    // Global tab switching
    match event.code {
        KeyCode::Char('1') | KeyCode::F(1) => {
            state.switch_tab(Tab::Layout);
            return true;
        }
        KeyCode::Char('2') | KeyCode::F(2) => {
            state.switch_tab(Tab::Config);
            return true;
        }
        KeyCode::Char('3') | KeyCode::F(3) => {
            state.switch_tab(Tab::Mirror);
            return true;
        }
        KeyCode::Char('4') | KeyCode::F(4) => {
            state.switch_tab(Tab::Profiles);
            return true;
        }
        _ => {}
    }

    match state.tab {
        Tab::Layout => layout_view::handle_key(event, state),
        Tab::Config => config_view::handle_key(event, state),
        Tab::Mirror => mirror_view::handle_key(event, state),
        Tab::Profiles => profiles_view::handle_key(event, state),
    }

    true
}

pub fn handle_mouse(event: MouseEvent, state: &mut AppState) {
    let col = event.column;
    let row = event.row;
    let (_, th) = state.terminal_size;

    // Tab bar: rows 0-1
    if row < 2 {
        if event.kind == MouseEventKind::Down(MouseButton::Left) {
            if let Some(tab) = Tab::from_col(col) {
                state.switch_tab(tab);
            }
        }
        return;
    }

    // Bottom bar: last row
    if row >= th.saturating_sub(1) {
        return;
    }

    match state.tab {
        Tab::Layout => layout_view::handle_mouse(event, state),
        Tab::Config => config_view::handle_mouse(event, state),
        Tab::Mirror => mirror_view::handle_mouse(event, state),
        Tab::Profiles => profiles_view::handle_mouse(event, state),
    }
}

pub fn render(f: &mut Frame, state: &AppState) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    render_tabs(f, chunks[0], state);

    match state.tab {
        Tab::Layout => layout_view::render(f, chunks[1], state),
        Tab::Config => config_view::render(f, chunks[1], state),
        Tab::Mirror => mirror_view::render(f, chunks[1], state),
        Tab::Profiles => profiles_view::render(f, chunks[1], state),
    }

    render_bottom_bar(f, chunks[2], state);
}

fn render_tabs(f: &mut Frame, area: Rect, state: &AppState) {
    let titles: Vec<Line> = vec![
        Line::from(Span::raw(" 1 Layout ")),
        Line::from(Span::raw(" 2 Config ")),
        Line::from(Span::raw(" 3 Mirror ")),
        Line::from(Span::raw(" 4 Profiles ")),
    ];
    let tabs = Tabs::new(titles)
        .block(Block::default())
        .select(state.tab.index())
        .style(Style::default().fg(Color::DarkGray))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .divider(Span::raw("│"));

    f.render_widget(tabs, area);
}

fn render_bottom_bar(f: &mut Frame, area: Rect, state: &AppState) {
    let dirty_marker = if state.dirty { " [modified]" } else { "" };
    let hints = format!("{}{}  [q]uit", state.tab.hints(), dirty_marker);

    let (status_text, status_style) = if let Some(msg) = &state.status {
        let color = match msg.level {
            StatusLevel::Info => Color::Cyan,
            StatusLevel::Success => Color::Green,
            StatusLevel::Error => Color::Red,
        };
        (format!("  {} ", msg.text), Style::default().fg(color))
    } else {
        (String::new(), Style::default())
    };

    let status_width = status_text.len() as u16;
    let bar_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(status_width)])
        .split(area);

    f.render_widget(
        Paragraph::new(hints).style(Style::default().fg(Color::DarkGray)),
        bar_chunks[0],
    );
    f.render_widget(
        Paragraph::new(status_text).style(status_style),
        bar_chunks[1],
    );
}
