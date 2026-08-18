#[cfg(feature = "jemalloc")]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

mod config;
mod display;
mod electron_preview;
mod engine;
mod gui;
mod iced_gui;
mod ipc;
mod logger;
mod lwe;
mod steam_scanner;
mod steam_workshop;
mod tui;
mod web_engine;
mod web_layer;
mod webkit_render;
pub mod video_render;
pub mod servo_render;
pub mod widgets_bridge;


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
    Gui {
        /// Launch minimized in background / system tray
        #[arg(short, long)]
        minimize: bool,
    },
    /// Set a local video/GIF/HTML file as wallpaper (alias: s)
    #[command(alias = "s")]
    Set {
        /// File path to video, GIF, or HTML file
        path: PathBuf,
    },
    /// Set a Steam Wallpaper Engine wallpaper directory as wallpaper
    SetSteam {
        /// Folder path to Steam Wallpaper Engine item (contains project.json)
        path: PathBuf,
        /// Target screen / monitor output name (e.g. eDP-1, HDMI-A-1)
        #[arg(short, long)]
        screen: Option<String>,
    },
    /// Detect connected displays and multi-monitor geometry
    DetectDisplays,
    /// Scan and list installed Steam Wallpaper Engine workshop items
    SteamList,
    /// View detailed metadata and properties of a Steam Wallpaper
    SteamInfo {
        /// Steam Wallpaper Workshop ID or folder name
        id: String,
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
    /// Hide / pause wallpaper rendering engine & GUI for hypridle / system lock (alias: h, hide-wallpaper)
    #[command(alias = "h", alias = "hide-wallpaper")]
    Hide,
    /// Show / resume wallpaper rendering engine & GUI after hypridle / system unlock (alias: unhide, unminimize)
    #[command(alias = "unhide", alias = "unminimize")]
    Show,
    /// Toggle wallpaper visibility / hide state (alias: th, toggle-visibility)
    #[command(alias = "th", alias = "toggle-visibility")]
    ToggleHide,
    /// Minimize GUI window & pause background playback for hypridle or waybar (alias: m, min)
    #[command(alias = "m", alias = "min")]
    Minimize,
    /// Output JSON status for Waybar custom module (alias: w)
    #[command(alias = "w")]
    Waybar,
    /// Print recommended hypridle.conf configuration snippet
    HypridleConfig,
    /// Print recommended waybar config.jsonc & style.css configuration snippets
    WaybarConfig,
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
        #[arg(long, default_value = "top_right")]
        position: String,
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
    /// Configure hardware acceleration video decoder (nvdec, cuda, vaapi, vulkan, auto, no)
    SetHwdec {
        decoder: String,
    },
    /// Stop background daemon (alias: k)
    #[command(alias = "k")]
    Stop,
    /// Internal: render a web URL as a background layer-shell surface
    #[command(hide = true)]
    WebLayer {
        /// URL or local path to render as wallpaper
        url: String,
        #[arg(long)]
        widget: bool,
        #[arg(long, default_value = "top_right")]
        position: String,
        #[arg(long, default_value_t = 480)]
        width: i32,
        #[arg(long, default_value_t = 560)]
        height: i32,
        #[arg(long)]
        monitor: Option<String>,
    },
}

struct SlideshowState {
    active: bool,
    interval_secs: u64,
    shuffle: bool,
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to build tokio runtime")
        .block_on(future)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
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
            block_on(run_daemon(&mut cfg))?;
        }
        Some(Commands::Gui { minimize }) => {
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
            gui::run_gui(cfg, minimize)?;
        }
        None => {
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
            gui::run_gui(cfg, false)?;
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
            block_on(tui::run_tui(&cfg))?;
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
            block_on(send_ipc_cmd(&cfg.socket_path, req));
        }
        Some(Commands::SetSteam { path, screen }) => {
            let abs_path = fs::canonicalize(&path).unwrap_or(path);
            let req = IpcRequest::SetSteamWallpaper {
                path: abs_path.to_string_lossy().to_string(),
                screen,
                overrides: None,
            };
            block_on(send_ipc_cmd(&cfg.socket_path, req));
        }
        Some(Commands::DetectDisplays) => {
            let displays = display::detect_displays();
            println!("\x1b[1;36mDetected Displays / Monitors ({} connected):\x1b[0m", displays.len());
            for d in displays {
                println!("  • \x1b[1;33m{}\x1b[0m: {} @ {}Hz (pos: {},{}){}", d.name, d.resolution, d.refresh_rate, d.x, d.y, if d.primary { " [PRIMARY]" } else { "" });
            }
        }
        Some(Commands::SteamList) => {
            let wallpapers = steam_scanner::scan_steam_wallpapers();
            println!("\x1b[1;36mFound {} Steam Wallpaper Engine Workshop Item(s):\x1b[0m", wallpapers.len());
            for w in wallpapers {
                println!("  • [\x1b[1;32m{}\x1b[0m] \x1b[1;37m{}\x1b[0m (type: {}, author: {})", w.id, w.title, w.wallpaper_type.as_str(), w.author);
            }
        }
        Some(Commands::SteamInfo { id }) => {
            let wallpapers = steam_scanner::scan_steam_wallpapers();
            if let Some(w) = wallpapers.iter().find(|x| x.id == id || x.workshop_id == id) {
                println!("\x1b[1;36mSteam Wallpaper Details:\x1b[0m");
                println!("  ID:          {}", w.id);
                println!("  Title:       {}", w.title);
                println!("  Author:      {}", w.author);
                println!("  Type:        {}", w.wallpaper_type.as_str());
                println!("  Path:        {}", w.path.display());
                println!("  Thumbnail:   {:?}", w.thumbnail);
                println!("  Properties:  {} custom property definition(s)", w.properties.len());
                for prop in &w.properties {
                    println!("    - {} ({}, type: {})", prop.label, prop.key, prop.prop_type);
                }
            } else {
                eprintln!("Steam Wallpaper with ID '{}' not found.", id);
            }
        }
        Some(Commands::SetUrl { url }) => {
            let req = IpcRequest::SetUrl { url };
            block_on(send_ipc_cmd(&cfg.socket_path, req));
        }
        Some(Commands::Clear) => {
            block_on(send_ipc_cmd(&cfg.socket_path, IpcRequest::StopWallpaper));
        }
        Some(Commands::SetMonitor { monitor, path }) => {
            let req = IpcRequest::SetMonitorWallpaper { monitor, path };
            block_on(send_ipc_cmd(&cfg.socket_path, req));
        }
        Some(Commands::SetOpacity { opacity }) => {
            let req = IpcRequest::SetOpacity { opacity };
            block_on(send_ipc_cmd(&cfg.socket_path, req));
        }
        Some(Commands::SetWidget { url, disable, position }) => {
            let req = IpcRequest::SetWidget {
                url,
                enabled: !disable,
                position: Some(position),
            };
            block_on(send_ipc_cmd(&cfg.socket_path, req));
        }
        Some(Commands::Pause) => {
            block_on(send_ipc_cmd(&cfg.socket_path, IpcRequest::Pause));
        }
        Some(Commands::Resume) => {
            block_on(send_ipc_cmd(&cfg.socket_path, IpcRequest::Resume));
        }
        Some(Commands::Toggle) => {
            block_on(send_ipc_cmd(&cfg.socket_path, IpcRequest::TogglePause));
        }
        Some(Commands::Hide) => {
            block_on(send_ipc_cmd(&cfg.socket_path, IpcRequest::Hide));
        }
        Some(Commands::Show) => {
            block_on(send_ipc_cmd(&cfg.socket_path, IpcRequest::Show));
        }
        Some(Commands::ToggleHide) => {
            block_on(send_ipc_cmd(&cfg.socket_path, IpcRequest::ToggleHide));
        }
        Some(Commands::Minimize) => {
            block_on(send_ipc_cmd(&cfg.socket_path, IpcRequest::Minimize));
        }
        Some(Commands::Waybar) => {
            match block_on(send_ipc_request(&cfg.socket_path, &IpcRequest::GetWaybarStatus)) {
                Ok(IpcResponse::WaybarStatus { json }) => println!("{}", json),
                Ok(_) => {
                    println!("{{\"text\":\"🌌 Off\",\"alt\":\"off\",\"tooltip\":\"OMYWALL Daemon status error\",\"class\":\"off\",\"percentage\":0}}");
                }
                Err(_) => {
                    println!("{{\"text\":\"🌌 Off\",\"alt\":\"off\",\"tooltip\":\"OMYWALL Wallpaper Daemon is inactive\",\"class\":\"off\",\"percentage\":0}}");
                }
            }
        }
        Some(Commands::HypridleConfig) => {
            println!("\x1b[1;36m┌────────────────────────────────────────────────────────┐\x1b[0m");
            println!("\x1b[1;36m│          🔒 OMYWALL HYPRIDLE CONFIGURATION             │\x1b[0m");
            println!("\x1b[1;36m└────────────────────────────────────────────────────────┘\x1b[0m\n");
            println!("Add the following listener blocks to \x1b[1;33m~/.config/hypr/hypridle.conf\x1b[0m:\n");
            println!("\x1b[1;32m# Automatically hide & pause OMYWALL wallpaper to save CPU/GPU when idle:");
            println!("listener {{");
            println!("    timeout = 300                                # 5 minutes idle");
            println!("    on-timeout = omywall hide                   # Hide & pause wallpaper engine");
            println!("    on-resume = omywall show                    # Restore & resume wallpaper engine");
            println!("}}\n");
            println!("# Turn off screens / lock after 10 minutes idle:");
            println!("listener {{");
            println!("    timeout = 600                                # 10 minutes idle");
            println!("    on-timeout = hyprlock                       # Launch hyprlock screensaver");
            println!("}}\x1b[0m");
        }
        Some(Commands::WaybarConfig) => {
            println!("\x1b[1;36m┌────────────────────────────────────────────────────────┐\x1b[0m");
            println!("\x1b[1;36m│          📊 OMYWALL WAYBAR MODULE CONFIGURATION        │\x1b[0m");
            println!("\x1b[1;36m└────────────────────────────────────────────────────────┘\x1b[0m\n");
            println!("Add the custom module to \x1b[1;33m~/.config/waybar/config.jsonc\x1b[0m:\n");
            println!("\x1b[1;32m\"custom/omywall\": {{");
            println!("    \"format\": \"{{}}\",");
            println!("    \"return-type\": \"json\",");
            println!("    \"exec\": \"omywall waybar\",");
            println!("    \"interval\": 2,");
            println!("    \"on-click\": \"omywall toggle\",");
            println!("    \"on-click-right\": \"omywall toggle-hide\",");
            println!("    \"on-click-middle\": \"omywall minimize\",");
            println!("    \"on-scroll-up\": \"omywall next\",");
            println!("    \"on-scroll-down\": \"omywall prev\"");
            println!("}}\x1b[0m\n");
            println!("Add the CSS styling to \x1b[1;33m~/.config/waybar/style.css\x1b[0m:\n");
            println!("\x1b[1;32m#custom-omywall {{");
            println!("    padding: 0 10px;");
            println!("    color: #00f0ff;");
            println!("}}");
            println!("#custom-omywall.paused {{ color: #ffaa00; }}");
            println!("#custom-omywall.hidden {{ color: #777777; }}");
            println!("#custom-omywall.stopped {{ color: #ff4444; }}\x1b[0m");
        }
        Some(Commands::Next) => {
            block_on(send_ipc_cmd(&cfg.socket_path, IpcRequest::NextWallpaper));
        }
        Some(Commands::Prev) => {
            block_on(send_ipc_cmd(&cfg.socket_path, IpcRequest::PrevWallpaper));
        }
        Some(Commands::Slideshow { interval, shuffle }) => {
            let req = IpcRequest::StartSlideshow {
                interval_secs: interval,
                shuffle,
            };
            block_on(send_ipc_cmd(&cfg.socket_path, req));
        }
        Some(Commands::StopSlideshow) => {
            block_on(send_ipc_cmd(&cfg.socket_path, IpcRequest::StopSlideshow));
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
            block_on(send_ipc_cmd(&cfg.socket_path, IpcRequest::CycleLiveWallpaper));
        }
        Some(Commands::Status) => {
            match block_on(send_ipc_request(&cfg.socket_path, &IpcRequest::GetStatus)) {
                Ok(IpcResponse::Status(st)) => {
                    let metrics = crate::config::get_system_metrics();
                    println!("\x1b[1;36m┌────────────────────────────────────────────────────────┐\x1b[0m");
                    println!("\x1b[1;36m│          🌌 OMYWALL WALLPAPER ENGINE STATUS           │\x1b[0m");
                    println!("\x1b[1;36m└────────────────────────────────────────────────────────┘\x1b[0m");
                    println!("  \x1b[1;33m● Current Wallpaper:\x1b[0m     \x1b[1;32m{}\x1b[0m", st.current_wallpaper.unwrap_or_else(|| "None Selected".into()));
                    println!("  \x1b[1;33m● Active Monitor:\x1b[0m        \x1b[1;37m{}\x1b[0m", st.active_monitor.unwrap_or_else(|| "All / Primary".into()));
                    println!("  \x1b[1;33m● Playback State:\x1b[0m        \x1b[1;35m{}\x1b[0m", if st.is_paused { "Paused ⏸" } else { "Playing ▶" });
                    println!("  \x1b[1;33m● Hardware Acceleration:\x1b[0m \x1b[1;36m{}\x1b[0m (Screen {})", st.hwdec, st.screen_id);
                    println!("  \x1b[1;33m● Real-time CPU Usage:\x1b[0m   \x1b[1;36m{:.1}%\x1b[0m", metrics.cpu_usage);
                    println!("  \x1b[1;33m● Real-time RAM Usage:\x1b[0m   \x1b[1;36m{} MB / {} MB\x1b[0m", metrics.ram_used_mb, metrics.ram_total_mb);
                    println!("  \x1b[1;33m● Real-time GPU Usage:\x1b[0m   \x1b[1;32m{:.1}%\x1b[0m ({}, VRAM: {} MB)", metrics.gpu_usage, metrics.gpu_name, metrics.vram_used_mb);
                    println!("  \x1b[1;33m● Catalog Wallpapers:\x1b[0m     {} available files", st.total_wallpapers);
                }
                Ok(other) => println!("Response: {:?}", other),
                Err(e) => {
                    eprintln!("\x1b[1;31mDaemon Status Error:\x1b[0m {}", e);
                    std::process::exit(1);
                }
            }
        }
        Some(Commands::SetHwdec { decoder }) => {
            let mut c = Config::load();
            c.hwdec = decoder.clone();
            let _ = c.save();
            block_on(send_ipc_cmd(&cfg.socket_path, IpcRequest::SetHwdec { hwdec: decoder.clone() }));
            println!("✅ Hardware acceleration video decoder set to: {}", decoder);
        }
        Some(Commands::Stop) => {
            block_on(send_ipc_cmd(&cfg.socket_path, IpcRequest::QuitDaemon));
        }
        Some(Commands::WebLayer { url, widget, position, width, height, monitor }) => {
            crate::web_layer::run_with_options(&url, widget, &position, width, height, monitor.as_deref())?;
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

    let file = match fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&lock_path) {
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
    widgets_bridge::start_telemetry_loop();


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
                IpcRequest::GetDisplays => {
                    let displays = display::detect_displays();
                    IpcResponse::Displays { displays }
                }
                IpcRequest::QuerySteamWallpapers => {
                    let wallpapers = steam_scanner::scan_steam_wallpapers();
                    IpcResponse::SteamWallpapers { wallpapers }
                }
                IpcRequest::SetSteamWallpaper { path, screen, overrides } => {
                    let path_buf = PathBuf::from(&path);
                    let res = {
                        let eng = engine.lock().unwrap();
                        eng.set_steam_wallpaper(&path_buf, screen.as_deref(), overrides.as_ref())
                    };
                    match res {
                        Ok(_) => {
                            let mut cfg_guard = config_arc.lock().unwrap();
                            cfg_guard.default_wallpaper = Some(path_buf);
                            let _ = cfg_guard.save();
                            log_info(&format!("IPC: Steam Wallpaper set to {}", path));
                            IpcResponse::Ok {
                                message: format!("Steam Wallpaper set to {}", path),
                            }
                        }
                        Err(e) => {
                            log_error(&format!("IPC SetSteam Error: {}", e));
                            IpcResponse::Err { message: e }
                        }
                    }
                }
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
                IpcRequest::Hide => {
                    let eng = engine.lock().unwrap();
                    match eng.hide() {
                        Ok(_) => {
                            let _ = std::process::Command::new("hyprctl").args(["dispatch", "minimize"]).output();
                            IpcResponse::Ok { message: "Wallpaper and GUI hidden / paused".into() }
                        }
                        Err(e) => IpcResponse::Err { message: e },
                    }
                }
                IpcRequest::Show => {
                    let eng = engine.lock().unwrap();
                    match eng.show() {
                        Ok(_) => IpcResponse::Ok { message: "Wallpaper restored / resumed".into() },
                        Err(e) => IpcResponse::Err { message: e },
                    }
                }
                IpcRequest::ToggleHide => {
                    let eng = engine.lock().unwrap();
                    match eng.toggle_hide() {
                        Ok(hidden) => {
                            if hidden {
                                let _ = std::process::Command::new("hyprctl").args(["dispatch", "minimize"]).output();
                            }
                            IpcResponse::Ok {
                                message: format!("Wallpaper visibility set to: {}", if hidden { "Hidden" } else { "Visible" }),
                            }
                        }
                        Err(e) => IpcResponse::Err { message: e },
                    }
                }
                IpcRequest::Minimize => {
                    let eng = engine.lock().unwrap();
                    let _ = eng.hide();
                    let _ = std::process::Command::new("hyprctl").args(["dispatch", "movetoworkspacesilent", "special:omywall,title:OMYWALL"]).output();
                    let _ = std::process::Command::new("hyprctl").args(["dispatch", "minimize"]).output();
                    let _ = std::process::Command::new("swaymsg").args(["[title=\"OMYWALL\"]", "move", "scratchpad"]).output();
                    let _ = std::process::Command::new("xdotool").args(["search", "--class", "omywall", "windowminimize"]).output();
                    IpcResponse::Ok { message: "Minimized GUI window and paused wallpaper engine".into() }
                }
                IpcRequest::GetWaybarStatus => {
                    let eng = engine.lock().unwrap();
                    let wall_opt = eng.current_wallpaper();
                    let is_paused = eng.is_paused().unwrap_or(false);
                    let is_hidden = eng.is_hidden();
                    let is_stopped = eng.is_user_stopped();
                    let vol = eng.volume();
                    let is_muted = eng.is_muted();
                    let hwdec = eng.hwdec();
                    let fps = eng.target_fps();

                    let (status_str, alt_str, class_str, icon) = if is_hidden {
                        ("Hidden (Idle)", "hidden", "hidden", "🙈")
                    } else if is_stopped {
                        ("Stopped", "stopped", "stopped", "⏹️")
                    } else if is_paused {
                        ("Paused", "paused", "paused", "⏸️")
                    } else {
                        ("Playing", "playing", "playing", "🌌")
                    };

                    let title = wall_opt
                        .as_ref()
                        .and_then(|p| std::path::Path::new(p).file_name())
                        .and_then(|n| n.to_str())
                        .unwrap_or("No Active Wallpaper");

                    let text = format!("{} {}", icon, title);
                    let tooltip = format!(
                        "OMYWALL Wallpaper Engine\nStatus: {}\nWallpaper: {}\nDecoder: {}\nFPS: {}\nVolume: {}%{}",
                        status_str,
                        title,
                        hwdec,
                        fps,
                        vol,
                        if is_muted { " [MUTED]" } else { "" }
                    );

                    let waybar_json = serde_json::json!({
                        "text": text,
                        "alt": alt_str,
                        "tooltip": tooltip,
                        "class": class_str,
                        "percentage": vol,
                    }).to_string();

                    IpcResponse::WaybarStatus { json: waybar_json }
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
                IpcRequest::SetWidget { url, enabled, position } => {
                    let eng = engine.lock().unwrap();
                    let mut cfg_guard = config_arc.lock().unwrap();
                    cfg_guard.enable_widgets = enabled;
                    cfg_guard.widget_url = if url.trim().is_empty() { None } else { Some(url.clone()) };
                    if let Some(ref pos) = position {
                        cfg_guard.widget_position = pos.clone();
                    }
                    let pos_str = position.unwrap_or_else(|| cfg_guard.widget_position.clone());
                    let _ = cfg_guard.save();
                    match eng.set_widget_with_position(&url, enabled, &pos_str) {
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
                        is_hidden: eng.is_hidden(),
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
                        widget_position: Some(cfg_guard.widget_position.clone()),
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
        if def.exists() {
            let _ = engine.set_wallpaper(def);
            return;
        }
    }

    let bunny_candidates = [
        PathBuf::from("assets/wallpapers/bunny.mp4"),
        dirs::home_dir().unwrap_or_default().join(".local/share/omywall/assets/wallpapers/bunny.mp4"),
        dirs::home_dir().unwrap_or_default().join(".local/share/omywall/wallpapers/bunny.mp4"),
    ];
    for b in &bunny_candidates {
        if b.exists() {
            let _ = engine.set_wallpaper(b);
            return;
        }
    }

    let catalog = scan_wallpapers(&config.wallpaper_dir);
    if let Some(first) = catalog.first() {
        let _ = engine.set_wallpaper(first);
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
