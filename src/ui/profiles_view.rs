use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::{
    profile,
    ui::{AppState, StatusLevel},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileField {
    List,
    NameInput,
    SaveBtn,
}

pub struct ProfilesState {
    pub profiles: Vec<String>,
    pub selected_idx: usize,
    pub new_name: String,
    pub focused: ProfileField,
    pub delete_confirm: Option<(String, Instant)>,
}

impl ProfilesState {
    pub fn new(profiles: Vec<String>) -> Self {
        Self {
            profiles,
            selected_idx: 0,
            new_name: String::new(),
            focused: ProfileField::List,
            delete_confirm: None,
        }
    }

    pub fn refresh(&mut self) {
        self.profiles = profile::list().unwrap_or_default();
        if self.selected_idx >= self.profiles.len() && !self.profiles.is_empty() {
            self.selected_idx = self.profiles.len() - 1;
        }
    }

    fn tick_delete_confirm(&mut self) {
        if let Some((_, born)) = &self.delete_confirm {
            if born.elapsed().as_secs() >= 3 {
                self.delete_confirm = None;
            }
        }
    }
}

pub fn handle_key(event: KeyEvent, state: &mut AppState) {
    state.profiles.tick_delete_confirm();

    match event.code {
        KeyCode::Tab => {
            state.profiles.focused = match state.profiles.focused {
                ProfileField::List => ProfileField::NameInput,
                ProfileField::NameInput => ProfileField::SaveBtn,
                ProfileField::SaveBtn => ProfileField::List,
            };
        }
        KeyCode::BackTab => {
            state.profiles.focused = match state.profiles.focused {
                ProfileField::List => ProfileField::SaveBtn,
                ProfileField::NameInput => ProfileField::List,
                ProfileField::SaveBtn => ProfileField::NameInput,
            };
        }
        KeyCode::Esc => {
            state.profiles.new_name.clear();
            state.profiles.delete_confirm = None;
            state.profiles.focused = ProfileField::List;
        }
        _ => match state.profiles.focused {
            ProfileField::List => handle_list_key(event, state),
            ProfileField::NameInput => handle_name_key(event, state),
            ProfileField::SaveBtn => {
                if event.code == KeyCode::Enter {
                    do_save(state);
                }
            }
        },
    }
}

fn handle_list_key(event: KeyEvent, state: &mut AppState) {
    let count = state.profiles.profiles.len();

    match event.code {
        KeyCode::Char('j') | KeyCode::Down => {
            if count > 0 {
                state.profiles.selected_idx = (state.profiles.selected_idx + 1) % count;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if count > 0 {
                state.profiles.selected_idx = state
                    .profiles
                    .selected_idx
                    .checked_sub(1)
                    .unwrap_or(count - 1);
            }
        }
        KeyCode::Enter => {
            if count > 0 {
                state.push_undo();
                do_load(state);
            }
        }
        KeyCode::Char('d') => {
            if count == 0 {
                return;
            }
            let name = state.profiles.profiles[state.profiles.selected_idx].clone();
            if let Some((pending, _)) = &state.profiles.delete_confirm {
                if *pending == name {
                    // Second press: confirm delete
                    match profile::delete(&name) {
                        Ok(()) => {
                            state.profiles.refresh();
                            state.set_status(format!("Deleted profile '{}'", name), StatusLevel::Success);
                        }
                        Err(e) => {
                            state.set_status(format!("Delete failed: {}", e), StatusLevel::Error);
                        }
                    }
                    state.profiles.delete_confirm = None;
                    return;
                }
            }
            state.profiles.delete_confirm = Some((name.clone(), Instant::now()));
            state.set_status(
                format!("Press d again to delete '{}' (3s to cancel)", name),
                StatusLevel::Info,
            );
        }
        _ => {}
    }
}

pub fn handle_mouse(event: MouseEvent, state: &mut AppState) {
    let row = event.row;
    let col = event.column;
    let (tw, th) = state.terminal_size;

    // Layout in profiles view:
    // Content area: y=2..th-2
    // List: y=2, height = content_h - 3 (save row is Length(3))
    // Save row: y = th - 4, height 3
    let content_h = th.saturating_sub(3); // th - tab(2) - bottom(1)
    let save_row_y = 2 + content_h.saturating_sub(3);
    // List block: border at y=2, items start at y=3
    let list_items_start = 3u16;

    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if row >= save_row_y {
                // Clicked in save row
                let save_btn_x = tw.saturating_sub(16);
                if col >= save_btn_x {
                    // Save button
                    state.profiles.focused = ProfileField::SaveBtn;
                    do_save(state);
                } else {
                    // Name input
                    state.profiles.focused = ProfileField::NameInput;
                }
            } else if row >= list_items_start {
                let item_idx = (row - list_items_start) as usize;
                if item_idx < state.profiles.profiles.len() {
                    state.profiles.focused = ProfileField::List;
                    state.profiles.selected_idx = item_idx;
                }
            }
        }
        MouseEventKind::Down(MouseButton::Right) => {
            // Right-click on a profile item = load it
            if row >= list_items_start && row < save_row_y {
                let item_idx = (row - list_items_start) as usize;
                if item_idx < state.profiles.profiles.len() {
                    state.profiles.selected_idx = item_idx;
                    state.push_undo();
                    do_load(state);
                }
            }
        }
        MouseEventKind::ScrollUp => {
            let count = state.profiles.profiles.len();
            if count > 0 {
                state.profiles.selected_idx =
                    state.profiles.selected_idx.checked_sub(1).unwrap_or(count - 1);
                state.profiles.focused = ProfileField::List;
            }
        }
        MouseEventKind::ScrollDown => {
            let count = state.profiles.profiles.len();
            if count > 0 {
                state.profiles.selected_idx = (state.profiles.selected_idx + 1) % count;
                state.profiles.focused = ProfileField::List;
            }
        }
        _ => {}
    }
}

fn handle_name_key(event: KeyEvent, state: &mut AppState) {
    match event.code {
        KeyCode::Char(c) => {
            // Only allow alphanumeric, dash, underscore
            if c.is_alphanumeric() || c == '-' || c == '_' {
                state.profiles.new_name.push(c);
            }
        }
        KeyCode::Backspace => {
            state.profiles.new_name.pop();
        }
        KeyCode::Enter => {
            do_save(state);
        }
        _ => {}
    }
}

fn do_save(state: &mut AppState) {
    let name = state.profiles.new_name.trim().to_owned();
    if name.is_empty() {
        state.set_status("Enter a profile name first.", StatusLevel::Info);
        return;
    }
    let profile = profile::from_monitors(&name, &state.monitors);
    match profile::save(&profile) {
        Ok(()) => {
            state.profiles.new_name.clear();
            state.profiles.refresh();
            state.set_status(format!("Saved profile '{}'", name), StatusLevel::Success);
        }
        Err(e) => {
            state.set_status(format!("Save failed: {}", e), StatusLevel::Error);
        }
    }
}

fn do_load(state: &mut AppState) {
    let name = state.profiles.profiles[state.profiles.selected_idx].clone();
    match profile::load(&name) {
        Ok(p) => {
            profile::apply_to_monitors(&p, &mut state.monitors);
            state.dirty = true;
            state.set_status(
                format!("Loaded profile '{}'. Press [a] to apply.", name),
                StatusLevel::Success,
            );
        }
        Err(e) => {
            state.set_status(format!("Load failed: {}", e), StatusLevel::Error);
        }
    }
}

pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),    // profile list
            Constraint::Length(3), // name input + save button
        ])
        .split(area);

    render_list(f, chunks[0], state);
    render_save_row(f, chunks[1], state);
}

fn render_list(f: &mut Frame, area: Rect, state: &AppState) {
    let profiles = &state.profiles.profiles;
    let is_focused = state.profiles.focused == ProfileField::List;

    let items: Vec<ListItem> = if profiles.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "  No profiles saved yet.",
            Style::default().fg(Color::DarkGray),
        )))]
    } else {
        profiles
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let is_selected = i == state.profiles.selected_idx;
                let pending_delete = state
                    .profiles
                    .delete_confirm
                    .as_ref()
                    .map(|(n, _)| n == name)
                    .unwrap_or(false);

                let style = if is_selected && is_focused {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                        .bg(Color::DarkGray)
                } else if is_selected {
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                let label = if pending_delete {
                    format!("  {}  [d again to confirm delete]", name)
                } else {
                    format!("  {}", name)
                };

                ListItem::new(Line::from(Span::styled(label, style)))
            })
            .collect()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(if is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        })
        .title(" Profiles  (Enter=load  d=delete) ");

    let list = List::new(items).block(block);
    let mut list_state = ListState::default();
    if !profiles.is_empty() {
        list_state.select(Some(state.profiles.selected_idx));
    }
    f.render_stateful_widget(list, area, &mut list_state);
}

fn render_save_row(f: &mut Frame, area: Rect, state: &AppState) {
    let name_focused = state.profiles.focused == ProfileField::NameInput;
    let save_focused = state.profiles.focused == ProfileField::SaveBtn;

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(16)])
        .split(area);

    // Name input
    let input_display = if name_focused {
        format!(" Profile name: {}|", state.profiles.new_name)
    } else {
        format!(" Profile name: {}", state.profiles.new_name)
    };
    let input_style = if name_focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    f.render_widget(
        Paragraph::new(input_display)
            .style(input_style)
            .block(Block::default().borders(Borders::ALL).border_style(input_style)),
        chunks[0],
    );

    // Save button
    let save_style = if save_focused {
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    f.render_widget(
        Paragraph::new(" [ Save ] ")
            .style(save_style)
            .block(Block::default().borders(Borders::ALL).border_style(save_style)),
        chunks[1],
    );
}
