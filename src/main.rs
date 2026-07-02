mod layout;
mod mirror;
mod monitor;
mod profile;
mod ui;

use std::io;

use anyhow::Result;
use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyEventKind,
        MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::{
    sync::mpsc,
    time::{interval, Duration},
};

use ui::{AppState, StatusLevel};

fn hyprland_socket2_path() -> Option<String> {
    let instance = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run/user/1000".into());
    Some(format!("{}/hypr/{}/.socket2.sock", runtime, instance))
}

#[derive(Debug)]
enum AppEvent {
    Key(crossterm::event::KeyEvent),
    Mouse(crossterm::event::MouseEvent),
    Resize(u16, u16),
    Tick,
    MonitorChange,
}

#[tokio::main]
async fn main() -> Result<()> {
    let monitors = monitor::load_monitors().await.unwrap_or_else(|e| {
        eprintln!("Warning: could not load monitors: {}", e);
        vec![]
    });

    // Terminal setup
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run(&mut terminal, monitors).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    result
}

async fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    monitors: Vec<monitor::Monitor>,
) -> Result<()> {
    let size = terminal.size()?;
    let mut state = AppState::new(monitors, (size.width, size.height));

    let (tx, mut rx) = mpsc::unbounded_channel::<AppEvent>();

    // Event reader task
    let tx_events = tx.clone();
    tokio::spawn(async move {
        let mut stream = EventStream::new();
        let mut tick = interval(Duration::from_secs(5));
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    let _ = tx_events.send(AppEvent::Tick);
                }
                maybe_event = stream.next() => {
                    match maybe_event {
                        Some(Ok(Event::Key(key))) => {
                            let _ = tx_events.send(AppEvent::Key(key));
                        }
                        Some(Ok(Event::Mouse(mouse))) => {
                            let _ = tx_events.send(AppEvent::Mouse(mouse));
                        }
                        Some(Ok(Event::Resize(w, h))) => {
                            let _ = tx_events.send(AppEvent::Resize(w, h));
                        }
                        None => break,
                        _ => {}
                    }
                }
            }
        }
    });

    // Hyprland socket hotplug listener
    let tx_hotplug = tx.clone();
    tokio::spawn(async move {
        if let Some(sig) = hyprland_socket2_path() {
            if let Ok(mut stream) = tokio::net::UnixStream::connect(&sig).await {
                use tokio::io::AsyncBufReadExt;
                let reader = tokio::io::BufReader::new(&mut stream);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if line.starts_with("monitoradded")
                        || line.starts_with("monitorremoved")
                        || line.starts_with("monitorscale")
                    {
                        let _ = tx_hotplug.send(AppEvent::MonitorChange);
                    }
                }
            }
        }
    });

    loop {
        state.tick_status();
        terminal.draw(|f| ui::render(f, &state))?;

        let event = rx.recv().await;

        match event {
            None => break,
            Some(AppEvent::Tick) => {
                if let Ok(monitors) = monitor::load_monitors().await {
                    if !state.dirty {
                        state.monitors = monitors;
                        state.layout.clamp_selected(state.monitors.len());
                    }
                }
            }
            Some(AppEvent::MonitorChange) => {
                if let Ok(monitors) = monitor::load_monitors().await {
                    state.monitors = monitors;
                    state.layout.clamp_selected(state.monitors.len());
                    state.set_status("Monitor configuration changed.", StatusLevel::Info);
                }
            }
            Some(AppEvent::Resize(w, h)) => {
                state.terminal_size = (w, h);
            }
            Some(AppEvent::Mouse(mouse)) => {
                if mouse.kind == MouseEventKind::Moved {
                    continue;
                }
                ui::handle_mouse(mouse, &mut state);
            }
            Some(AppEvent::Key(key)) => {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                match key.code {
                    crossterm::event::KeyCode::Char('a') => {
                        state.pending_apply = true;
                        state.set_status("Applying...", StatusLevel::Info);
                    }
                    crossterm::event::KeyCode::Char('s') => {
                        ui::layout_view::trigger_save(&mut state);
                    }
                    crossterm::event::KeyCode::Char('r') => {
                        match monitor::load_monitors().await {
                            Ok(monitors) => {
                                state.monitors = monitors;
                                state.layout.clamp_selected(state.monitors.len());
                                state.dirty = false;
                                state.set_status("Monitors refreshed.", StatusLevel::Success);
                            }
                            Err(e) => {
                                state.set_status(
                                    format!("Refresh failed: {}", e),
                                    StatusLevel::Error,
                                );
                            }
                        }
                    }
                    _ => {
                        if !ui::handle_key(key, &mut state) {
                            break;
                        }
                    }
                }
            }
        }

        if state.pending_apply {
            state.pending_apply = false;
            match monitor::apply_monitors(&state.monitors).await {
                Ok(()) => {
                    state.set_status("Applied.", StatusLevel::Success);
                }
                Err(e) => {
                    state.set_status(format!("Apply failed: {}", e), StatusLevel::Error);
                }
            }
        }
    }

    Ok(())
}
