use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Terminal,
};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::config::Config;
use crate::ipc::{send_ipc_request, DaemonStatus, IpcRequest, IpcResponse};

enum InputMode {
    Normal,
    Search,
    SetWorkspace,
    SetMonitor,
    SetUrl,
}

pub async fn run_tui(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let status_resp = send_ipc_request(&config.socket_path, &IpcRequest::GetStatus).await;
    let initial_status = match status_resp {
        Ok(IpcResponse::Status(status)) => Some(status),
        _ => None,
    };

    let mut wallpapers = scan_wallpapers(&config.wallpaper_dir);

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = main_tui_loop(&mut terminal, config, &mut wallpapers, initial_status).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("TUI Error: {}", err);
    }

    Ok(())
}

fn scan_wallpapers(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let valid_exts = ["mkv", "mp4", "webm", "avi", "mov", "gif", "html", "htm", "js", "m4v", "flv", "wmv", "png", "jpg", "jpeg", "webp"];

    fn walk_dir(d: &Path, depth: usize, files: &mut Vec<PathBuf>, valid_exts: &[&str]) {
        if depth > 4 {
            return;
        }
        if let Ok(entries) = std::fs::read_dir(d) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                        if valid_exts.contains(&ext.to_lowercase().as_str()) {
                            let canon = std::fs::canonicalize(&path).unwrap_or(path);
                            if !files.contains(&canon) {
                                files.push(canon);
                            }
                        }
                    }
                } else if path.is_dir() {
                    walk_dir(&path, depth + 1, files, valid_exts);
                }
            }
        }
    }

    walk_dir(dir, 0, &mut files, &valid_exts);

    if let Some(home) = dirs::home_dir() {
        let omarchy_themes = home.join(".config").join("omarchy").join("themes");
        if omarchy_themes.exists() {
            walk_dir(&omarchy_themes, 0, &mut files, &valid_exts);
        }
        let omarchy_current = home.join(".config").join("omarchy").join("current");
        if omarchy_current.exists() {
            walk_dir(&omarchy_current, 0, &mut files, &valid_exts);
        }
        let local_wallpapers = home.join(".local").join("share").join("wallpapers");
        if local_wallpapers.exists() {
            walk_dir(&local_wallpapers, 0, &mut files, &valid_exts);
        }
    }

    files.sort();
    files
}

async fn main_tui_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    config: &Config,
    wallpapers: &mut Vec<PathBuf>,
    mut daemon_status: Option<DaemonStatus>,
) -> Result<(), String> {
    let mut list_state = ListState::default();
    if !wallpapers.is_empty() {
        list_state.select(Some(0));
    }

    let mut search_query = String::new();
    let mut url_query = String::new();
    let mut ws_query = String::new();
    let mut mode = InputMode::Normal;
    let mut status_msg = String::from("Connected to OMYWALL Wallpaper Engine");
    let mut ws_mappings: HashMap<String, String> = HashMap::new();

    // Fetch initial workspace mappings
    if let Ok(IpcResponse::WorkspaceMappings { mappings, .. }) =
        send_ipc_request(&config.socket_path, &IpcRequest::GetWorkspaceMappings).await
    {
        ws_mappings = mappings;
    }

    loop {
        let filtered_wallpapers: Vec<&PathBuf> = wallpapers
            .iter()
            .filter(|p| {
                if search_query.is_empty() {
                    true
                } else {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .map(|s| s.to_lowercase().contains(&search_query.to_lowercase()))
                        .unwrap_or(false)
                }
            })
            .collect();

        let current_wall_str = daemon_status
            .as_ref()
            .and_then(|s| s.current_wallpaper.clone());

        terminal
            .draw(|f| {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .margin(1)
                    .constraints(
                        [
                            Constraint::Length(3), // Banner
                            Constraint::Min(10),  // Main body
                            Constraint::Length(3), // Footer
                        ]
                        .as_ref(),
                    )
                    .split(f.area());

                // Title Banner
                let mode_str = match mode {
                    InputMode::Normal => "NORMAL MODE",
                    InputMode::Search => "SEARCH FILTER MODE",
                    InputMode::SetWorkspace => "MAP WORKSPACE MODE",
                    InputMode::SetMonitor => "MAP MONITOR MODE",
                    InputMode::SetUrl => "WEB STREAM URL MODE",
                };

                let title = Paragraph::new(Line::from(vec![
                    Span::styled(
                        " OMYWALL WALLPAPER ENGINE ",
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(" ─ [{}] ", mode_str),
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        "Video, GIF & Workspace Wallpapers",
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Cyan)),
                );
                f.render_widget(title, chunks[0]);

                let body_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(55), Constraint::Percentage(45)].as_ref())
                    .split(chunks[1]);

                // Left Panel: Wallpaper List with Workspace Badges
                let items: Vec<ListItem> = filtered_wallpapers
                    .iter()
                    .enumerate()
                    .map(|(idx, path)| {
                        let filename = path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("Unknown");

                        let path_str = path.to_string_lossy().to_string();

                        let is_active = current_wall_str
                            .as_ref()
                            .map(|curr| Path::new(curr) == *path)
                            .unwrap_or(false);

                        let mut mapped_ws = Vec::new();
                        for (ws_id, target) in &ws_mappings {
                            if target == &path_str {
                                mapped_ws.push(ws_id.clone());
                            }
                        }
                        mapped_ws.sort();

                        let ws_badge = if !mapped_ws.is_empty() {
                            format!(" [WS {}]", mapped_ws.join(","))
                        } else {
                            "".to_string()
                        };

                        let active_indicator = if is_active { " [ACTIVE ▶]" } else { "" };

                        let style = if is_active {
                            Style::default()
                                .fg(Color::Green)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::White)
                        };

                        ListItem::new(Line::from(vec![
                            Span::styled(format!("{:2}. ", idx + 1), Style::default().fg(Color::DarkGray)),
                            Span::styled(filename, style),
                            Span::styled(ws_badge, Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                            Span::styled(active_indicator, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                        ]))
                    })
                    .collect();

                let list_title = match mode {
                    InputMode::Search => format!(" Wallpapers (Search: {}) ", search_query),
                    InputMode::SetWorkspace => format!(" Select Wallpaper -> Enter WS Number (1-10): {} ", ws_query),
                    InputMode::SetMonitor => format!(" Select Wallpaper -> Enter Monitor Name (eDP-1/HDMI-A-1/0): {} ", ws_query),
                    InputMode::SetUrl => format!(" Enter Stream Web URL: {} ", url_query),
                    _ => format!(" Wallpapers ({}) ", filtered_wallpapers.len()),
                };

                let list_block = Block::default()
                    .title(list_title)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(match mode {
                        InputMode::Search => Color::Yellow,
                        InputMode::SetWorkspace => Color::Magenta,
                        InputMode::SetMonitor => Color::LightCyan,
                        InputMode::SetUrl => Color::Cyan,
                        InputMode::Normal => Color::Blue,
                    }));

                let list = List::new(items)
                    .block(list_block)
                    .highlight_style(
                        Style::default()
                            .bg(Color::Rgb(40, 44, 52))
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )
                    .highlight_symbol("▶ ");

                f.render_stateful_widget(list, body_chunks[0], &mut list_state);

                // Right Panel Split (Status Card + Workspaces Mapping Card)
                let right_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Percentage(55), Constraint::Percentage(45)].as_ref())
                    .split(body_chunks[1]);

                // Status Card
                let status_text = if let Some(ref st) = daemon_status {
                    let curr_name = st.current_wallpaper.as_ref().map(|p| {
                        if p.starts_with("http://") || p.starts_with("https://") {
                            p.clone()
                        } else {
                            Path::new(p).file_name().and_then(|n| n.to_str()).unwrap_or(p).to_string()
                        }
                    }).unwrap_or_else(|| "None Selected".to_string());

                    vec![
                        Line::from(vec![
                            Span::styled("Active Mode: ", Style::default().fg(Color::DarkGray)),
                            Span::styled(st.mode.to_uppercase(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                        ]),
                        Line::from(vec![
                            Span::styled("Active Wallpaper: ", Style::default().fg(Color::DarkGray)),
                            Span::styled(curr_name, Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                        ]),
                        Line::from(vec![
                            Span::styled("Focused Workspace: ", Style::default().fg(Color::DarkGray)),
                            Span::styled(
                                st.active_workspace.clone().unwrap_or_else(|| "None".to_string()),
                                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(" | Mon: ", Style::default().fg(Color::DarkGray)),
                            Span::styled(
                                st.active_monitor.clone().unwrap_or_else(|| "None".to_string()),
                                Style::default().fg(Color::LightCyan),
                            ),
                        ]),
                        Line::from(vec![
                            Span::styled("Playback State: ", Style::default().fg(Color::DarkGray)),
                            Span::styled(
                                if st.is_paused { "PAUSED ⏸" } else { "PLAYING ▶" },
                                Style::default().fg(if st.is_paused { Color::Yellow } else { Color::Green }),
                            ),
                        ]),
                        Line::from(vec![
                            Span::styled("HW Accel / Screen: ", Style::default().fg(Color::DarkGray)),
                            Span::styled(format!("{} (Screen {})", st.hwdec, st.screen_id), Style::default().fg(Color::LightBlue)),
                        ]),
                    ]
                } else {
                    vec![Line::from(Span::styled(
                        "Daemon Offline",
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    ))]
                };

                let status_block = Paragraph::new(status_text)
                    .block(Block::default().title(" System Status ").borders(Borders::ALL).border_style(Style::default().fg(Color::Magenta)))
                    .wrap(Wrap { trim: true });

                f.render_widget(status_block, right_chunks[0]);

                // Help & Bindings Legend
                let help_text = vec![
                    Line::from(vec![Span::styled("Enter ", Style::default().fg(Color::Yellow)), Span::raw("▶ Apply selected wallpaper")]),
                    Line::from(vec![Span::styled("t / Tab", Style::default().fg(Color::Yellow)), Span::raw("▶ Toggle Mode (Workspace <-> Screen)")]),
                    Line::from(vec![Span::styled("c     ", Style::default().fg(Color::Yellow)), Span::raw("▶ Cycle live wallpapers sequentially")]),
                    Line::from(vec![Span::styled("w     ", Style::default().fg(Color::Yellow)), Span::raw("▶ Map wallpaper to Workspace (1-10)")]),
                    Line::from(vec![Span::styled("M     ", Style::default().fg(Color::Yellow)), Span::raw("▶ Map wallpaper to Monitor (eDP-1, etc.)")]),
                    Line::from(vec![Span::styled("[ / ] ", Style::default().fg(Color::Yellow)), Span::raw("▶ Opacity Down / Up")]),
                    Line::from(vec![Span::styled("p     ", Style::default().fg(Color::Yellow)), Span::raw("▶ Toggle Pause / Resume")]),
                    Line::from(vec![Span::styled("s     ", Style::default().fg(Color::Yellow)), Span::raw("▶ Toggle Auto-Slideshow")]),
                    Line::from(vec![Span::styled("q/Esc ", Style::default().fg(Color::Yellow)), Span::raw("▶ Quit TUI")]),
                ];

                let help_block = Paragraph::new(help_text)
                    .block(Block::default().title(" Terminal Controls ").borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray)));

                f.render_widget(help_block, right_chunks[1]);

                let footer = Paragraph::new(Span::styled(&status_msg, Style::default().fg(Color::LightCyan)))
                    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray)));
                f.render_widget(footer, chunks[2]);
            })
            .map_err(|e| e.to_string())?;

        if event::poll(Duration::from_millis(150)).map_err(|e| e.to_string())? {
            if let Event::Key(key) = event::read().map_err(|e| e.to_string())? {
                match mode {
                    InputMode::Search => match key.code {
                        KeyCode::Esc | KeyCode::Enter => {
                            mode = InputMode::Normal;
                        }
                        KeyCode::Backspace => {
                            search_query.pop();
                        }
                        KeyCode::Char(c) => {
                            search_query.push(c);
                        }
                        _ => {}
                    },
                    InputMode::SetWorkspace => match key.code {
                        KeyCode::Esc => {
                            mode = InputMode::Normal;
                            ws_query.clear();
                        }
                        KeyCode::Enter => {
                            if !ws_query.trim().is_empty() {
                                if let Some(idx) = list_state.selected() {
                                    if let Some(selected_path) = filtered_wallpapers.get(idx) {
                                        let ws = ws_query.trim().to_string();
                                        let path_str = selected_path.to_string_lossy().to_string();
                                        let req = IpcRequest::SetWorkspaceWallpaper {
                                            workspace: ws.clone(),
                                            path: path_str.clone(),
                                        };
                                        if let Ok(IpcResponse::Ok { message }) =
                                            send_ipc_request(&config.socket_path, &req).await
                                        {
                                            status_msg = message;
                                            ws_mappings.insert(ws, path_str);
                                        }
                                    }
                                }
                            }
                            mode = InputMode::Normal;
                            ws_query.clear();
                        }
                        KeyCode::Backspace => {
                            ws_query.pop();
                        }
                        KeyCode::Char(c) => {
                            if c.is_ascii_digit() || c.is_alphanumeric() {
                                ws_query.push(c);
                            }
                        }
                        _ => {}
                    },
                    InputMode::SetUrl => match key.code {
                        KeyCode::Esc => {
                            mode = InputMode::Normal;
                            url_query.clear();
                        }
                        KeyCode::Enter => {
                            if !url_query.trim().is_empty() {
                                let req = IpcRequest::SetUrl {
                                    url: url_query.trim().to_string(),
                                };
                                if let Ok(IpcResponse::Ok { message }) =
                                    send_ipc_request(&config.socket_path, &req).await
                                {
                                    status_msg = message;
                                }
                            }
                            mode = InputMode::Normal;
                            url_query.clear();
                        }
                        KeyCode::Backspace => {
                            url_query.pop();
                        }
                        KeyCode::Char(c) => {
                            url_query.push(c);
                        }
                        _ => {}
                    },
                    InputMode::SetMonitor => match key.code {
                        KeyCode::Esc => {
                            mode = InputMode::Normal;
                            ws_query.clear();
                        }
                        KeyCode::Enter => {
                            if !ws_query.trim().is_empty() {
                                if let Some(idx) = list_state.selected() {
                                    if let Some(selected_path) = filtered_wallpapers.get(idx) {
                                        let mon = ws_query.trim().to_string();
                                        let path_str = selected_path.to_string_lossy().to_string();
                                        let req = IpcRequest::SetMonitorWallpaper {
                                            monitor: mon.clone(),
                                            path: path_str.clone(),
                                        };
                                        if let Ok(IpcResponse::Ok { message }) =
                                            send_ipc_request(&config.socket_path, &req).await
                                        {
                                            status_msg = message;
                                        }
                                    }
                                }
                            }
                            mode = InputMode::Normal;
                            ws_query.clear();
                        }
                        KeyCode::Backspace => {
                            ws_query.pop();
                        }
                        KeyCode::Char(c) => {
                            ws_query.push(c);
                        }
                        _ => {}
                    },
                    InputMode::Normal => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            break;
                        }
                        KeyCode::Char('/') => {
                            mode = InputMode::Search;
                        }
                        KeyCode::Char('w') => {
                            mode = InputMode::SetWorkspace;
                        }
                        KeyCode::Char('M') => {
                            mode = InputMode::SetMonitor;
                        }
                        KeyCode::Char('[') => {
                            let curr_op = daemon_status.as_ref().map(|s| s.opacity).unwrap_or(1.0);
                            let new_op = (curr_op - 0.05).max(0.0);
                            let req = IpcRequest::SetOpacity { opacity: new_op };
                            let _ = send_ipc_request(&config.socket_path, &req).await;
                        }
                        KeyCode::Char(']') => {
                            let curr_op = daemon_status.as_ref().map(|s| s.opacity).unwrap_or(1.0);
                            let new_op = (curr_op + 0.05).min(1.0);
                            let req = IpcRequest::SetOpacity { opacity: new_op };
                            let _ = send_ipc_request(&config.socket_path, &req).await;
                        }
                        KeyCode::Char('u') => {
                            mode = InputMode::SetUrl;
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if !filtered_wallpapers.is_empty() {
                                let idx = list_state.selected().unwrap_or(0);
                                let next = (idx + 1) % filtered_wallpapers.len();
                                list_state.select(Some(next));
                            }
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            if !filtered_wallpapers.is_empty() {
                                let idx = list_state.selected().unwrap_or(0);
                                let prev = if idx == 0 {
                                    filtered_wallpapers.len() - 1
                                } else {
                                    idx - 1
                                };
                                list_state.select(Some(prev));
                            }
                        }
                        KeyCode::Enter => {
                            if let Some(idx) = list_state.selected() {
                                if let Some(selected_path) = filtered_wallpapers.get(idx) {
                                    let req = IpcRequest::SetWallpaper {
                                        path: selected_path.to_string_lossy().to_string(),
                                    };
                                    match send_ipc_request(&config.socket_path, &req).await {
                                        Ok(IpcResponse::Ok { message }) => {
                                            status_msg = format!("Applied wallpaper: {}", message);
                                        }
                                        Ok(IpcResponse::Err { message }) => {
                                            status_msg = format!("Daemon error: {}", message);
                                        }
                                        Err(e) => {
                                            status_msg = format!("IPC error: {}", e);
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        KeyCode::Char('p') => {
                            let req = IpcRequest::TogglePause;
                            let _ = send_ipc_request(&config.socket_path, &req).await;
                        }
                        KeyCode::Char('s') => {
                            let is_active = daemon_status.as_ref().map(|s| s.slideshow_active).unwrap_or(false);
                            let req = if is_active {
                                IpcRequest::StopSlideshow
                            } else {
                                IpcRequest::StartSlideshow {
                                    interval_secs: config.slideshow_interval,
                                    shuffle: config.slideshow_shuffle,
                                }
                            };
                            let _ = send_ipc_request(&config.socket_path, &req).await;
                        }
                        KeyCode::Char('t') | KeyCode::Tab => {
                            let req = IpcRequest::ToggleMode;
                            let _ = send_ipc_request(&config.socket_path, &req).await;
                        }
                        KeyCode::Char('c') => {
                            let req = IpcRequest::CycleLiveWallpaper;
                            let _ = send_ipc_request(&config.socket_path, &req).await;
                        }
                        KeyCode::Char('m') => {
                            let is_muted = daemon_status.as_ref().map(|s| s.is_muted).unwrap_or(true);
                            let req = IpcRequest::SetMute { mute: !is_muted };
                            let _ = send_ipc_request(&config.socket_path, &req).await;
                        }
                        KeyCode::Char('+') | KeyCode::Char('=') => {
                            let vol = daemon_status.as_ref().map(|s| s.volume).unwrap_or(0);
                            let req = IpcRequest::SetVolume { volume: vol + 5 };
                            let _ = send_ipc_request(&config.socket_path, &req).await;
                        }
                        KeyCode::Char('-') | KeyCode::Char('_') => {
                            let vol = daemon_status.as_ref().map(|s| s.volume).unwrap_or(0);
                            let req = IpcRequest::SetVolume { volume: vol - 5 };
                            let _ = send_ipc_request(&config.socket_path, &req).await;
                        }
                        KeyCode::Char('n') => {
                            let req = IpcRequest::NextWallpaper;
                            let _ = send_ipc_request(&config.socket_path, &req).await;
                        }
                        KeyCode::Char('b') => {
                            let req = IpcRequest::PrevWallpaper;
                            let _ = send_ipc_request(&config.socket_path, &req).await;
                        }
                        KeyCode::Char('r') => {
                            *wallpapers = scan_wallpapers(&config.wallpaper_dir);
                            status_msg = format!("Rescanned {}. Found {} wallpapers.", config.wallpaper_dir.display(), wallpapers.len());
                        }
                        _ => {}
                    },
                }
            }
        }

        if let Ok(IpcResponse::Status(st)) = send_ipc_request(&config.socket_path, &IpcRequest::GetStatus).await {
            daemon_status = Some(st);
        }
        if let Ok(IpcResponse::WorkspaceMappings { mappings, .. }) =
            send_ipc_request(&config.socket_path, &IpcRequest::GetWorkspaceMappings).await
        {
            ws_mappings = mappings;
        }
    }

    Ok(())
}
