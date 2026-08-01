mod config;
mod engine;
mod gui;
mod ipc;
mod logger;
mod tui;
mod web_engine;

use clap::{Parser, Subcommand};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;

use config::Config;
use engine::WallpaperEngine;
use ipc::{send_ipc_request, DaemonStatus, IpcRequest, IpcResponse};
use logger::{get_log_path, init_logging, log_error, log_info};

#[derive(Parser)]
#[command(name = "omywall")]
#[command(about = "OMYWALL - Universal Hardware-Accelerated Video, Stream & Desktop Wallpaper Engine", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start background wallpaper engine daemon (alias: d)
    #[command(alias = "d")]
    Daemon,
    /// Launch interactive Ratatui Terminal UI (alias: t)
    #[command(alias = "t")]
    Tui,
    /// Launch egui Desktop Settings & Catalog GUI (alias: g)
    #[command(alias = "g")]
    Gui,
    /// Set a local video/GIF/HTML file as wallpaper (alias: s)
    #[command(alias = "s")]
    Set {
        /// File path to video, GIF, or HTML file
        path: PathBuf,
    },
    /// Stream a Web video URL or Web/JS site (alias: u)
    #[command(alias = "u")]
    SetUrl {
        /// Web URL or YouTube link
        url: String,
    },
    /// Stop active wallpaper playback / clear screen (alias: c)
    #[command(alias = "c")]
    Clear,
    /// Pause wallpaper playback
    Pause,
    /// Resume wallpaper playback (alias: r)
    #[command(alias = "r")]
    Resume,
    /// Toggle pause / play (alias: p, tog)
    #[command(alias = "p", alias = "tog")]
    Toggle,
    /// Switch to next wallpaper (alias: n)
    #[command(alias = "n")]
    Next,
    /// Switch to previous wallpaper (alias: b)
    #[command(alias = "b")]
    Prev,
    /// Start automated slideshow rotation
    Slideshow {
        #[arg(short, long, default_value_t = 300)]
        interval: u64,
        #[arg(short, long)]
        shuffle: bool,
    },
    /// Stop automated wallpaper slideshow
    StopSlideshow,
    /// Assign wallpaper to a specific monitor (alias: mon)
    #[command(alias = "mon")]
    SetMonitor {
        monitor: String,
        path: String,
    },
    /// Set wallpaper opacity / transparency (0.0 to 1.0)
    SetOpacity {
        opacity: f32,
    },
    /// Configure desktop web widget overlay
    SetWidget {
        url: String,
        #[arg(short, long)]
        disable: bool,
    },
    /// Manage autostart on system boot (alias: auto)
    #[command(alias = "auto")]
    Autostart {
        #[arg(long)]
        enable: bool,
        #[arg(long)]
        disable: bool,
    },
    /// Cycle through live wallpapers sequentially & toggle (alias: toggle-live, live)
    #[command(alias = "toggle-live", alias = "live")]
    Cycle,
    /// View application logs (alias: l)
    #[command(alias = "l")]
    Logs,
    /// Show current daemon status (alias: st)
    #[command(alias = "st")]
    Status,
    /// Stop background daemon (alias: k)
    #[command(alias = "k")]
    Stop,
}

struct SlideshowState {
    active: bool,
    interval_secs: u64,
    shuffle: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Ignore SIGCHLD signals so spawned child processes exit cleanly without signaling MPV/Rust process
    unsafe {
        libc::signal(libc::SIGCHLD, libc::SIG_IGN);
    }

    let cli = Cli::parse();
    let mut cfg = Config::load();

    match cli.command {
        Some(Commands::Daemon) => {
            init_logging();
            let _lock = match acquire_instance_lock("omywall-daemon") {
                Some(f) => f,
                None => {
                    let msg = "⚠️ OMYWALL Wallpaper Engine Daemon is already running.";
                    println!("{}", msg);
                    let _ = std::process::Command::new("notify-send")
                        .args(["-u", "normal", "-a", "OMYWALL", "OMYWALL Wallpaper Engine", "Daemon process is already running!"])
                        .output();
                    return Ok(());
                }
            };
            Box::leak(Box::new(_lock));
            run_daemon(&mut cfg).await?;
        }
        Some(Commands::Gui) | None => {
            let _lock = match acquire_instance_lock("omywall-gui") {
                Some(f) => f,
                None => {
                    let msg = "⚠️ OMYWALL Wallpaper Engine GUI process is already running!";
                    println!("{}", msg);
                    let _ = std::process::Command::new("notify-send")
                        .args(["-u", "normal", "-a", "OMYWALL", "OMYWALL Wallpaper Engine", "Process is already running! Focusing existing window..."])
                        .output();

                    // Attempt to focus existing GUI window across Hyprland, Sway, wmctrl, xdotool
                    let _ = std::process::Command::new("hyprctl").args(["dispatch", "focuswindow", "OMYWALL Wallpaper Engine"]).output();
                    let _ = std::process::Command::new("swaymsg").args(["[title=\"OMYWALL Wallpaper Engine.*\"] focus"]).output();
                    let _ = std::process::Command::new("wmctrl").args(["-a", "OMYWALL Wallpaper Engine"]).output();
                    return Ok(());
                }
            };
            Box::leak(Box::new(_lock));
            gui::run_gui(cfg)?;
        }
        Some(Commands::Tui) => {
            let _lock = match acquire_instance_lock("omywall-tui") {
                Some(f) => f,
                None => {
                    let msg = "⚠️ OMYWALL Wallpaper Engine Terminal UI is already running.";
                    println!("{}", msg);
                    let _ = std::process::Command::new("notify-send")
                        .args(["-u", "normal", "-a", "OMYWALL", "OMYWALL Wallpaper Engine", "Terminal UI process is already running!"])
                        .output();
                    return Ok(());
                }
            };
            Box::leak(Box::new(_lock));
            tui::run_tui(&cfg).await?;
        }
        Some(Commands::Logs) => {
            let log_path = get_log_path();
            if log_path.exists() {
                println!("--- OMYWALL Log ({}) ---", log_path.display());
                let content = fs::read_to_string(&log_path).unwrap_or_default();
                let lines: Vec<&str> = content.lines().collect();
                let start = lines.len().saturating_sub(50);
                for line in &lines[start..] {
                    println!("{}", line);
                }
            } else {
                println!("No log file found at {}", log_path.display());
            }
        }
        Some(Commands::Set { path }) => {
            let abs_path = fs::canonicalize(&path).unwrap_or(path);
            let req = IpcRequest::SetWallpaper {
                path: abs_path.to_string_lossy().to_string(),
            };
            send_ipc_cmd(&cfg.socket_path, req).await;
        }
        Some(Commands::SetUrl { url }) => {
            let req = IpcRequest::SetUrl { url };
            send_ipc_cmd(&cfg.socket_path, req).await;
        }
        Some(Commands::Clear) => {
            send_ipc_cmd(&cfg.socket_path, IpcRequest::StopWallpaper).await;
        }
        Some(Commands::SetMonitor { monitor, path }) => {
            let req = IpcRequest::SetMonitorWallpaper { monitor, path };
            send_ipc_cmd(&cfg.socket_path, req).await;
        }
        Some(Commands::SetOpacity { opacity }) => {
            let req = IpcRequest::SetOpacity { opacity };
            send_ipc_cmd(&cfg.socket_path, req).await;
        }
        Some(Commands::SetWidget { url, disable }) => {
            let req = IpcRequest::SetWidget { url, enabled: !disable };
            send_ipc_cmd(&cfg.socket_path, req).await;
        }
        Some(Commands::Pause) => {
            send_ipc_cmd(&cfg.socket_path, IpcRequest::Pause).await;
        }
        Some(Commands::Resume) => {
            send_ipc_cmd(&cfg.socket_path, IpcRequest::Resume).await;
        }
        Some(Commands::Toggle) => {
            send_ipc_cmd(&cfg.socket_path, IpcRequest::TogglePause).await;
        }
        Some(Commands::Next) => {
            send_ipc_cmd(&cfg.socket_path, IpcRequest::NextWallpaper).await;
        }
        Some(Commands::Prev) => {
            send_ipc_cmd(&cfg.socket_path, IpcRequest::PrevWallpaper).await;
        }
        Some(Commands::Slideshow { interval, shuffle }) => {
            let req = IpcRequest::StartSlideshow {
                interval_secs: interval,
                shuffle,
            };
            send_ipc_cmd(&cfg.socket_path, req).await;
        }
        Some(Commands::StopSlideshow) => {
            send_ipc_cmd(&cfg.socket_path, IpcRequest::StopSlideshow).await;
        }
        Some(Commands::Autostart { enable, disable }) => {
            if enable {
                match Config::set_autostart(true) {
                    Ok(_) => println!("✅ Autostart on boot enabled (~/.config/autostart/omywall.desktop)"),
                    Err(e) => eprintln!("Error enabling autostart: {}", e),
                }
            } else if disable {
                match Config::set_autostart(false) {
                    Ok(_) => println!("✅ Autostart on boot disabled"),
                    Err(e) => eprintln!("Error disabling autostart: {}", e),
                }
            } else {
                let status = if Config::is_autostart_enabled() { "ENABLED 🟢" } else { "DISABLED 🔴" };
                println!("OMYWALL Wallpaper Engine Autostart Status: {}", status);
            }
        }
        Some(Commands::Cycle) => {
            send_ipc_cmd(&cfg.socket_path, IpcRequest::CycleLiveWallpaper).await;
        }
        Some(Commands::Status) => {
            match send_ipc_request(&cfg.socket_path, &IpcRequest::GetStatus).await {
                Ok(IpcResponse::Status(st)) => {
                    println!("--- OMYWALL Wallpaper Engine Status ---");
                    println!("Current Wallpaper: {}", st.current_wallpaper.unwrap_or_else(|| "None Selected".into()));
                    println!("Active Monitor:    {}", st.active_monitor.unwrap_or_else(|| "None".into()));
                    println!("Playback:          {}", if st.is_paused { "Paused ⏸" } else { "Playing ▶" });
                    println!("Slideshow Mode:    {}", if st.slideshow_active { format!("Active (Interval: {}s)", st.slideshow_interval) } else { "Disabled".into() });
                    println!("Hardware Dec:      {} (Screen {})", st.hwdec, st.screen_id);
                    println!("Volume:            {}% ({})", st.volume, if st.is_muted { "Muted" } else { "Unmuted" });
                    println!("Wallpapers Found:  {}", st.total_wallpapers);
                }
                Ok(other) => println!("Response: {:?}", other),
                Err(e) => {
                    eprintln!("Daemon Status Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Some(Commands::Stop) => {
            send_ipc_cmd(&cfg.socket_path, IpcRequest::QuitDaemon).await;
        }
    }

    Ok(())
}

async fn send_ipc_cmd(socket_path: &Path, req: IpcRequest) {
    match send_ipc_request(socket_path, &req).await {
        Ok(IpcResponse::Ok { message }) => println!("Success: {}", message),
        Ok(IpcResponse::Err { message }) => {
            eprintln!("Error: {}", message);
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Daemon Error: {}", e);
            std::process::exit(1);
        }
        _ => {}
    }
}

fn acquire_instance_lock(name: &str) -> Option<std::fs::File> {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let lock_path = PathBuf::from(runtime_dir).join(format!("{}.lock", name));

    let file = match fs::OpenOptions::new().write(true).create(true).open(&lock_path) {
        Ok(f) => f,
        Err(_) => return None,
    };

    use std::os::unix::io::AsRawFd;
    let fd = file.as_raw_fd();
    let res = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
    if res != 0 {
        return None;
    }

    Some(file)
}

async fn run_daemon(cfg: &mut Config) -> Result<(), Box<dyn std::error::Error>> {
    log_info("Starting OMYWALL Wallpaper Engine Daemon...");

    if !cfg.wallpaper_dir.exists() {
        let _ = fs::create_dir_all(&cfg.wallpaper_dir);
    }

    let config_arc = Arc::new(Mutex::new(cfg.clone()));

    let engine = Arc::new(Mutex::new(WallpaperEngine::new(
        &cfg.hwdec,
        cfg.gpu_device.clone(),
        cfg.target_fps,
        cfg.volume,
        cfg.mute,
        cfg.window_id,
        cfg.screen_id,
    )?));

    let wallpaper_files = Arc::new(Mutex::new(scan_wallpapers(&cfg.wallpaper_dir)));
    let active_monitor = Arc::new(Mutex::new(None::<String>));
    let cycle_index = Arc::new(Mutex::new(None::<usize>));

    let slideshow_state = Arc::new(Mutex::new(SlideshowState {
        active: false,
        interval_secs: cfg.slideshow_interval,
        shuffle: cfg.slideshow_shuffle,
    }));

    // Auto-slideshow background loop
    {
        let engine_clone = engine.clone();
        let files_clone = wallpaper_files.clone();
        let slideshow_clone = slideshow_state.clone();
        let wall_dir = cfg.wallpaper_dir.clone();

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let (active, interval) = {
                    let st = slideshow_clone.lock().unwrap();
                    (st.active, st.interval_secs)
                };

                if active {
                    tokio::time::sleep(Duration::from_secs(interval.saturating_sub(1))).await;
                    let is_active = slideshow_clone.lock().unwrap().active;
                    if is_active {
                        let files = {
                            let mut guard = files_clone.lock().unwrap();
                            *guard = scan_wallpapers(&wall_dir);
                            guard.clone()
                        };

                        if !files.is_empty() {
                            let eng = engine_clone.lock().unwrap();
                            let current = eng.current_wallpaper();

                            let idx = current
                                .as_ref()
                                .and_then(|curr| files.iter().position(|f| f.to_string_lossy() == *curr))
                                .map(|i| (i + 1) % files.len())
                                .unwrap_or(0);

                            let _ = eng.set_wallpaper(&files[idx]);
                        }
                    }
                }
            }
        });
    }

    if cfg.socket_path.exists() {
        let _ = fs::remove_file(&cfg.socket_path);
    }
    if let Some(parent) = cfg.socket_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let listener = UnixListener::bind(&cfg.socket_path)?;
    log_info(&format!("OMYWALL Wallpaper Engine listening on socket: {}", cfg.socket_path.display()));

    let initial_mon = std::process::Command::new("hyprctl")
        .arg("monitors")
        .output()
        .ok()
        .and_then(|out| {
            let stdout = String::from_utf8_lossy(&out.stdout);
            stdout.lines().next().and_then(|line| {
                if line.starts_with("Monitor ") {
                    line.split_whitespace().nth(1).map(|s| s.to_string())
                } else {
                    None
                }
            })
        });

    if let Some(ref mon) = initial_mon {
        let mut active_mon_guard = active_monitor.lock().unwrap();
        *active_mon_guard = Some(mon.clone());
    }

    {
        let cfg_guard = config_arc.lock().unwrap();
        let eng = engine.lock().unwrap();
        apply_active_wallpaper(&cfg_guard, &eng, initial_mon.as_deref());
    }

    loop {
        let (mut stream, _) = match listener.accept().await {
            Ok(s) => s,
            Err(e) => {
                log_error(&format!("Socket accept error: {}", e));
                continue;
            }
        };

        let engine = engine.clone();
        let wallpaper_files = wallpaper_files.clone();
        let slideshow_state = slideshow_state.clone();
        let active_mon_arc = active_monitor.clone();
        let cycle_idx_arc = cycle_index.clone();
        let config_arc = config_arc.clone();
        let wall_dir = cfg.wallpaper_dir.clone();

        tokio::spawn(async move {
            let mut len_buf = [0u8; 4];
            if stream.read_exact(&mut len_buf).await.is_err() {
                return;
            }
            let req_len = u32::from_be_bytes(len_buf) as usize;
            let mut req_buf = vec![0u8; req_len];
            if stream.read_exact(&mut req_buf).await.is_err() {
                return;
            }

            let req: IpcRequest = match serde_json::from_slice(&req_buf) {
                Ok(r) => r,
                Err(e) => {
                    let err_resp = IpcResponse::Err {
                        message: format!("Invalid Request: {}", e),
                    };
                    let _ = send_response(&mut stream, &err_resp).await;
                    return;
                }
            };

            let mut quit_signal = false;
            let response = match req {
                IpcRequest::SetWallpaper { path } => {
                    let path_buf = PathBuf::from(&path);
                    let res = {
                        let eng = engine.lock().unwrap();
                        eng.set_wallpaper(&path_buf)
                    };
                    match res {
                        Ok(_) => {
                            let mut cfg_guard = config_arc.lock().unwrap();
                            cfg_guard.default_wallpaper = Some(path_buf);
                            let _ = cfg_guard.save();
                            log_info(&format!("IPC: Wallpaper set to {}", path));
                            IpcResponse::Ok {
                                message: format!("Wallpaper set to {}", path),
                            }
                        }
                        Err(e) => {
                            log_error(&format!("IPC Set Error: {}", e));
                            IpcResponse::Err { message: e }
                        }
                    }
                }
                IpcRequest::SetUrl { url } => {
                    let res = {
                        let eng = engine.lock().unwrap();
                        eng.set_url(&url)
                    };
                    match res {
                        Ok(_) => {
                            log_info(&format!("IPC: Streaming URL wallpaper {}", url));
                            IpcResponse::Ok {
                                message: format!("Streaming URL wallpaper: {}", url),
                            }
                        }
                        Err(e) => {
                            log_error(&format!("IPC SetUrl Error: {}", e));
                            IpcResponse::Err { message: e }
                        }
                    }
                }
                IpcRequest::StopWallpaper => {
                    let eng = engine.lock().unwrap();
                    match eng.stop_wallpaper() {
                        Ok(_) => {
                            log_info("IPC: Wallpaper playback stopped");
                            IpcResponse::Ok {
                                message: "Wallpaper stopped".into(),
                            }
                        }
                        Err(e) => IpcResponse::Err { message: e },
                    }
                }
                IpcRequest::SwitchMonitor { monitor } => {
                    {
                        let mut active_guard = active_mon_arc.lock().unwrap();
                        *active_guard = Some(monitor.clone());
                    }
                    let cfg_guard = config_arc.lock().unwrap();
                    let eng = engine.lock().unwrap();
                    apply_active_wallpaper(&cfg_guard, &eng, Some(&monitor));
                    IpcResponse::Ok {
                        message: format!("Switched to Monitor {}", monitor),
                    }
                }
                IpcRequest::CycleLiveWallpaper => {
                    let files = {
                        let mut guard = wallpaper_files.lock().unwrap();
                        *guard = scan_wallpapers(&wall_dir);
                        guard.clone()
                    };

                    if files.is_empty() {
                        IpcResponse::Err {
                            message: "No live wallpapers found in wallpaper directory".into(),
                        }
                    } else {
                        let mut cycle_guard = cycle_idx_arc.lock().unwrap();
                        let next_idx = match *cycle_guard {
                            None => 0,
                            Some(idx) => idx + 1,
                        };

                        if next_idx >= files.len() {
                            *cycle_guard = None;
                            let eng = engine.lock().unwrap();
                            let _ = eng.stop_wallpaper();
                            log_info("IPC: Live wallpaper cycle completed. Restored default background.");
                            IpcResponse::Ok {
                                message: "Live wallpaper cycle completed. Restored default background.".into(),
                            }
                        } else {
                            *cycle_guard = Some(next_idx);
                            let next_file = &files[next_idx];
                            let eng = engine.lock().unwrap();
                            let _ = eng.set_wallpaper(next_file);
                            log_info(&format!("IPC: Cycled to live wallpaper [{}/{}] '{}'", next_idx + 1, files.len(), next_file.display()));
                            IpcResponse::Ok {
                                message: format!("Cycled live wallpaper [{}/{}]: {}", next_idx + 1, files.len(), next_file.file_name().and_then(|n| n.to_str()).unwrap_or("")),
                            }
                        }
                    }
                }
                IpcRequest::Pause => {
                    let eng = engine.lock().unwrap();
                    match eng.pause() {
                        Ok(_) => IpcResponse::Ok { message: "Playback paused".into() },
                        Err(e) => IpcResponse::Err { message: e },
                    }
                }
                IpcRequest::Resume => {
                    let eng = engine.lock().unwrap();
                    match eng.resume() {
                        Ok(_) => IpcResponse::Ok { message: "Playback resumed".into() },
                        Err(e) => IpcResponse::Err { message: e },
                    }
                }
                IpcRequest::TogglePause => {
                    let eng = engine.lock().unwrap();
                    match eng.toggle_pause() {
                        Ok(paused) => IpcResponse::Ok {
                            message: format!("Playback state: {}", if paused { "Paused" } else { "Playing" }),
                        },
                        Err(e) => IpcResponse::Err { message: e },
                    }
                }
                IpcRequest::NextWallpaper => {
                    let files = {
                        let mut guard = wallpaper_files.lock().unwrap();
                        *guard = scan_wallpapers(&wall_dir);
                        guard.clone()
                    };

                    let eng = engine.lock().unwrap();
                    let current = eng.current_wallpaper();

                    if files.is_empty() {
                        IpcResponse::Err {
                            message: "Wallpaper Exception: No wallpapers found in directory".into(),
                        }
                    } else {
                        let idx = current
                            .as_ref()
                            .and_then(|curr| files.iter().position(|f| f.to_string_lossy() == *curr))
                            .map(|i| (i + 1) % files.len())
                            .unwrap_or(0);

                        let next_file = &files[idx];
                        match eng.set_wallpaper(next_file) {
                            Ok(_) => IpcResponse::Ok {
                                message: format!("Switched to {}", next_file.display()),
                            },
                            Err(e) => IpcResponse::Err { message: e },
                        }
                    }
                }
                IpcRequest::PrevWallpaper => {
                    let files = {
                        let mut guard = wallpaper_files.lock().unwrap();
                        *guard = scan_wallpapers(&wall_dir);
                        guard.clone()
                    };

                    let eng = engine.lock().unwrap();
                    let current = eng.current_wallpaper();

                    if files.is_empty() {
                        IpcResponse::Err {
                            message: "Wallpaper Exception: No wallpapers found in directory".into(),
                        }
                    } else {
                        let idx = current
                            .as_ref()
                            .and_then(|curr| files.iter().position(|f| f.to_string_lossy() == *curr))
                            .map(|i| if i == 0 { files.len() - 1 } else { i - 1 })
                            .unwrap_or(0);

                        let prev_file = &files[idx];
                        match eng.set_wallpaper(prev_file) {
                            Ok(_) => IpcResponse::Ok {
                                message: format!("Switched to {}", prev_file.display()),
                            },
                            Err(e) => IpcResponse::Err { message: e },
                        }
                    }
                }
                IpcRequest::StartSlideshow { interval_secs, shuffle } => {
                    let mut st = slideshow_state.lock().unwrap();
                    st.active = true;
                    st.interval_secs = interval_secs;
                    st.shuffle = shuffle;
                    log_info(&format!("IPC: Slideshow started with {}s interval", interval_secs));
                    IpcResponse::Ok {
                        message: format!("Slideshow started with {}s interval", interval_secs),
                    }
                }
                IpcRequest::StopSlideshow => {
                    let mut st = slideshow_state.lock().unwrap();
                    st.active = false;
                    log_info("IPC: Slideshow stopped");
                    IpcResponse::Ok {
                        message: "Slideshow stopped".into(),
                    }
                }
                IpcRequest::SetVolume { volume } => {
                    let mut eng = engine.lock().unwrap();
                    match eng.set_volume(volume) {
                        Ok(_) => IpcResponse::Ok { message: format!("Volume set to {}%", volume) },
                        Err(e) => IpcResponse::Err { message: e },
                    }
                }
                IpcRequest::SetMute { mute } => {
                    let mut eng = engine.lock().unwrap();
                    match eng.set_mute(mute) {
                        Ok(_) => IpcResponse::Ok {
                            message: format!("Mute state set to {}", mute),
                        },
                        Err(e) => IpcResponse::Err { message: e },
                    }
                }
                IpcRequest::SetHwdec { hwdec } => {
                    let mut cfg_guard = config_arc.lock().unwrap();
                    cfg_guard.hwdec = hwdec.clone();
                    let _ = cfg_guard.save();
                    let mut eng = engine.lock().unwrap();
                    match eng.set_hwdec(&hwdec) {
                        Ok(_) => IpcResponse::Ok {
                            message: format!("Hardware decoder set to {}", hwdec),
                        },
                        Err(e) => IpcResponse::Err { message: e },
                    }
                }
                IpcRequest::SetGpuDevice { gpu_device } => {
                    let mut cfg_guard = config_arc.lock().unwrap();
                    cfg_guard.gpu_device = gpu_device.clone();
                    let _ = cfg_guard.save();
                    let mut eng = engine.lock().unwrap();
                    match eng.set_gpu_device(gpu_device.clone()) {
                        Ok(_) => IpcResponse::Ok {
                            message: format!("GPU device target set to {:?}", gpu_device),
                        },
                        Err(e) => IpcResponse::Err { message: e },
                    }
                }
                IpcRequest::SetTargetFps { fps } => {
                    let mut cfg_guard = config_arc.lock().unwrap();
                    cfg_guard.target_fps = fps;
                    let _ = cfg_guard.save();
                    let mut eng = engine.lock().unwrap();
                    match eng.set_target_fps(fps) {
                        Ok(_) => IpcResponse::Ok {
                            message: format!("Target rendering FPS set to {} FPS", fps),
                        },
                        Err(e) => IpcResponse::Err { message: e },
                    }
                }
                IpcRequest::SetScreen { screen_id } => {
                    let mut eng = engine.lock().unwrap();
                    match eng.set_screen(screen_id) {
                        Ok(_) => IpcResponse::Ok {
                            message: format!("Target screen set to Screen {}", screen_id),
                        },
                        Err(e) => IpcResponse::Err { message: e },
                    }
                }
                IpcRequest::SetMonitorWallpaper { monitor, path } => {
                    let mut cfg_guard = config_arc.lock().unwrap();
                    if path.trim().is_empty() {
                        cfg_guard.monitor_wallpapers.remove(&monitor);
                        let _ = cfg_guard.save();
                        let mon_guard = active_mon_arc.lock().unwrap();
                        let eng = engine.lock().unwrap();
                        apply_active_wallpaper(&cfg_guard, &eng, mon_guard.as_deref());
                        log_info(&format!("IPC: Cleared Monitor {} wallpaper mapping", monitor));
                        IpcResponse::Ok {
                            message: format!("Cleared Monitor {} mapping", monitor),
                        }
                    } else {
                        cfg_guard.monitor_wallpapers.insert(monitor.clone(), path.clone());
                        let _ = cfg_guard.save();
                        let mon_guard = active_mon_arc.lock().unwrap();
                        let eng = engine.lock().unwrap();
                        apply_active_wallpaper(&cfg_guard, &eng, mon_guard.as_deref());
                        log_info(&format!("IPC: Mapped Monitor {} to '{}'", monitor, path));
                        IpcResponse::Ok {
                            message: format!("Monitor {} mapped to '{}'", monitor, path),
                        }
                    }
                }
                IpcRequest::GetMonitorMappings => {
                    let cfg_guard = config_arc.lock().unwrap();
                    IpcResponse::MonitorMappings {
                        mappings: cfg_guard.monitor_wallpapers.clone(),
                    }
                }
                IpcRequest::SetOpacity { opacity } => {
                    let eng = engine.lock().unwrap();
                    let mut cfg_guard = config_arc.lock().unwrap();
                    cfg_guard.opacity = opacity;
                    let _ = cfg_guard.save();
                    match eng.set_opacity(opacity) {
                        Ok(_) => IpcResponse::Ok {
                            message: format!("Opacity set to {:.0}%", opacity * 100.0),
                        },
                        Err(e) => IpcResponse::Err { message: e },
                    }
                }
                IpcRequest::SetWidget { url, enabled } => {
                    let eng = engine.lock().unwrap();
                    let mut cfg_guard = config_arc.lock().unwrap();
                    cfg_guard.enable_widgets = enabled;
                    cfg_guard.widget_url = if url.trim().is_empty() { None } else { Some(url.clone()) };
                    let _ = cfg_guard.save();
                    match eng.set_widget(&url, enabled) {
                        Ok(_) => IpcResponse::Ok {
                            message: format!("Desktop widget state: {}", if enabled { "Enabled" } else { "Disabled" }),
                        },
                        Err(e) => IpcResponse::Err { message: e },
                    }
                }
                IpcRequest::GetStatus => {
                    let eng = engine.lock().unwrap();
                    let files = wallpaper_files.lock().unwrap();
                    let st = slideshow_state.lock().unwrap();
                    let mon_guard = active_mon_arc.lock().unwrap();
                    let (w_enabled, w_url) = eng.get_widget_info();
                    let cfg_guard = config_arc.lock().unwrap();

                    let status = DaemonStatus {
                        current_wallpaper: eng.current_wallpaper(),
                        active_monitor: mon_guard.clone(),
                        is_paused: eng.is_paused().unwrap_or(false),
                        volume: eng.volume(),
                        is_muted: eng.is_muted(),
                        hwdec: eng.hwdec(),
                        gpu_device: eng.gpu_device(),
                        target_fps: eng.target_fps(),
                        screen_id: eng.screen_id(),
                        slideshow_active: st.active,
                        slideshow_interval: st.interval_secs,
                        slideshow_shuffle: st.shuffle,
                        opacity: eng.get_opacity(),
                        widget_enabled: w_enabled,
                        widget_url: w_url,
                        monitor_wallpapers: cfg_guard.monitor_wallpapers.clone(),
                        total_wallpapers: files.len(),
                    };
                    IpcResponse::Status(status)
                }
                IpcRequest::ListWallpapers => {
                    let mut guard = wallpaper_files.lock().unwrap();
                    *guard = scan_wallpapers(&wall_dir);
                    let eng = engine.lock().unwrap();
                    IpcResponse::WallpaperList {
                        files: guard.iter().map(|p| p.to_string_lossy().to_string()).collect(),
                        current: eng.current_wallpaper(),
                    }
                }
                IpcRequest::QuitDaemon => {
                    quit_signal = true;
                    log_info("IPC: Daemon shutting down...");
                    IpcResponse::Ok { message: "Daemon shutting down...".into() }
                }
            };

            let _ = send_response(&mut stream, &response).await;
            if quit_signal {
                std::process::exit(0);
            }
        });
    }
}

fn apply_active_wallpaper(
    config: &Config,
    engine: &WallpaperEngine,
    active_mon: Option<&str>,
) {
    if engine.is_user_stopped() {
        return;
    }

    let mut target_path: Option<String> = None;
    if let Some(mon) = active_mon {
        if let Some(target) = config.get_monitor_wallpaper(mon) {
            target_path = Some(target.clone());
        }
    }

    if let Some(path_str) = target_path {
        if !path_str.trim().is_empty() {
            if path_str.starts_with("http://") || path_str.starts_with("https://") {
                let _ = engine.set_url(&path_str);
            } else {
                let _ = engine.set_wallpaper(Path::new(&path_str));
            }
            return;
        }
    }

    if let Some(ref def) = config.default_wallpaper {
        let _ = engine.set_wallpaper(def);
    }
}

async fn send_response(stream: &mut tokio::net::UnixStream, resp: &IpcResponse) -> Result<(), String> {
    let json_bytes = serde_json::to_vec(resp).map_err(|e| e.to_string())?;
    let len = json_bytes.len() as u32;
    stream.write_all(&len.to_be_bytes()).await.map_err(|e| e.to_string())?;
    stream.write_all(&json_bytes).await.map_err(|e| e.to_string())?;
    stream.flush().await.map_err(|e| e.to_string())?;
    Ok(())
}

fn scan_wallpapers(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let valid_exts = ["mkv", "mp4", "webm", "avi", "mov", "gif", "html", "htm", "js", "m4v", "flv", "wmv", "png", "jpg", "jpeg", "webp"];

    fn walk_dir(d: &Path, depth: usize, files: &mut Vec<PathBuf>, valid_exts: &[&str]) {
        if depth > 4 {
            return;
        }
        if let Ok(entries) = fs::read_dir(d) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                        if valid_exts.contains(&ext.to_lowercase().as_str()) {
                            let canon = fs::canonicalize(&path).unwrap_or(path);
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
        let candidate_dirs = [
            home.join(".config").join("omarchy").join("themes"),
            home.join(".config").join("omarchy").join("current"),
            home.join(".local").join("share").join("wallpapers"),
            home.join("Pictures").join("Wallpapers"),
            home.join("Pictures"),
            home.join("Videos"),
            home.join(".local").join("share").join("Steam").join("steamapps").join("workshop").join("content").join("431960"),
            home.join(".steam").join("steam").join("steamapps").join("workshop").join("content").join("431960"),
            home.join(".steam").join("root").join("steamapps").join("workshop").join("content").join("431960"),
            PathBuf::from("/usr/share/backgrounds"),
        ];

        for c_dir in &candidate_dirs {
            if c_dir.exists() {
                walk_dir(c_dir, 0, &mut files, &valid_exts);
            }
        }
    }

    files.sort();
    files
}
