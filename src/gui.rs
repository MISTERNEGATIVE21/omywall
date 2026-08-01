use eframe::egui;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use crate::config::Config;
use crate::ipc::{send_ipc_request, DaemonStatus, IpcRequest, IpcResponse};
use crate::logger::get_log_path;

pub fn run_gui(config: Config) -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("OMYWALL Wallpaper Engine v3.5")
            .with_inner_size([1240.0, 820.0])
            .with_min_inner_size([940.0, 640.0]),
        ..Default::default()
    };

    eframe::run_native(
        "OMYWALL Wallpaper Engine v3.5",
        options,
        Box::new(|_cc| Ok(Box::new(OmarchyGuiApp::new(config)))),
    )
}

#[derive(PartialEq, Clone, Copy)]
enum CategoryFilter {
    All,
    Videos,
    WebWidgets,
    StaticImages,
    SteamWorkshop,
}

#[derive(PartialEq, Clone, Copy)]
enum ViewMode {
    Grid,
    List,
}

struct OmarchyGuiApp {
    config: Config,
    status: Arc<Mutex<Option<DaemonStatus>>>,
    workspace_mappings: Arc<Mutex<HashMap<String, String>>>,
    wallpapers: Vec<PathBuf>,
    selected_wallpaper: Option<PathBuf>,
    selected_workspace: String,
    search_filter: String,
    category_filter: CategoryFilter,
    view_mode: ViewMode,
    web_url_input: String,
    new_web_title: String,
    new_web_category: String,
    volume_slider: i64,
    opacity_slider: f32,
    autostart_enabled: bool,
    status_message: String,
    slideshow_interval_secs: u64,
    slideshow_shuffle: bool,
    show_doctor: bool,
    show_logs: bool,
    texture_cache: HashMap<PathBuf, egui::TextureHandle>,
    last_poll_instant: std::time::Instant,
}

#[derive(Clone)]
struct ToolStatus {
    name: String,
    description: String,
    installed: bool,
}

fn check_tool_installed(cmd: &str) -> bool {
    if Command::new("which").arg(cmd).output().map(|o| o.status.success()).unwrap_or(false) {
        return true;
    }
    if Command::new("sh").args(["-c", &format!("command -v {}", cmd)]).output().map(|o| o.status.success()).unwrap_or(false) {
        return true;
    }

    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/home/user"));
    let candidates = [
        home.join(".local/bin").join(cmd),
        home.join(".cargo/bin").join(cmd),
        PathBuf::from("/usr/bin").join(cmd),
        PathBuf::from("/usr/local/bin").join(cmd),
        PathBuf::from("/bin").join(cmd),
    ];

    for c in &candidates {
        if c.exists() {
            return true;
        }
    }

    false
}

fn check_installed_tools() -> Vec<ToolStatus> {
    let tools = vec![
        ("mpvpaper", "mpvpaper", "Primary Wayland video wallpaper renderer (wlr-layer-shell)"),
        ("mpv", "mpv", "Hardware-accelerated video player engine"),
        ("ffmpeg", "ffmpeg", "Video thumbnail generator & media processor"),
        ("electron", "electron", "Desktop web streams & widget overlay engine"),
        ("jq", "jq", "JSON processor for IPC & Hyprland events"),
        ("notify-send", "notify-send", "Desktop notification system"),
        ("hyprctl", "hyprctl", "Hyprland compositor controller"),
    ];

    tools
        .into_iter()
        .map(|(name, cmd, desc)| {
            let installed = check_tool_installed(cmd);
            ToolStatus {
                name: name.to_string(),
                description: desc.to_string(),
                installed,
            }
        })
        .collect()
}

fn run_installer_script() -> String {
    let script_path = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("scripts")
        .join("install_deps.sh");

    let raw_cmd = if script_path.exists() {
        format!("bash {}", script_path.display())
    } else {
        "curl -sSL https://raw.githubusercontent.com/Omarchy/Omarchy-Wall/main/scripts/install_deps.sh | bash".to_string()
    };

    let bash_cmd = format!("{}; echo ''; echo 'Done. Press Enter to close...'; read _", raw_cmd);

    // 1. Try $TERMINAL environment variable if set
    if let Ok(term) = std::env::var("TERMINAL") {
        let trimmed = term.trim();
        if !trimmed.is_empty() {
            let res = if trimmed == "foot" {
                Command::new(trimmed).args(["bash", "-c", &bash_cmd]).spawn()
            } else {
                Command::new(trimmed).args(["-e", "bash", "-c", &bash_cmd]).spawn()
            };
            if res.is_ok() {
                return format!("Launched dependency installer script in $TERMINAL ({})", trimmed);
            }
        }
    }

    // 2. Try known terminal emulators with correct individual arguments
    let candidates: &[(&str, &[&str])] = &[
        ("ghostty", &["-e", "bash", "-c", &bash_cmd]),
        ("kitty", &["-e", "bash", "-c", &bash_cmd]),
        ("alacritty", &["-e", "bash", "-c", &bash_cmd]),
        ("foot", &["bash", "-c", &bash_cmd]),
        ("wezterm", &["start", "--", "bash", "-c", &bash_cmd]),
        ("konsole", &["-e", "bash", "-c", &bash_cmd]),
        ("gnome-terminal", &["--", "bash", "-c", &bash_cmd]),
        ("xfce4-terminal", &["-x", "bash", "-c", &bash_cmd]),
        ("x-terminal-emulator", &["-e", "bash", "-c", &bash_cmd]),
        ("xterm", &["-e", "bash", "-c", &bash_cmd]),
    ];

    for &(term, args) in candidates {
        if Command::new(term).args(args).spawn().is_ok() {
            return format!("Launched dependency installer script in {}", term);
        }
    }

    // 3. Fallback shell command
    let shell_fallback = format!(
        "ghostty -e bash -c '{0}' || kitty -e bash -c '{0}' || alacritty -e bash -c '{0}' || foot bash -c '{0}' || konsole -e bash -c '{0}' || gnome-terminal -- bash -c '{0}' || x-terminal-emulator -e bash -c '{0}'",
        bash_cmd.replace('\'', "'\\''")
    );

    match Command::new("sh").args(["-c", &shell_fallback]).spawn() {
        Ok(_) => "Launched dependency installer script via shell fallback".to_string(),
        Err(e) => format!("Failed to launch terminal for installer: {}", e),
    }
}

fn md5_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 5381;
    for &b in bytes {
        hash = ((hash << 5).wrapping_add(hash)).wrapping_add(b as u64);
    }
    hash
}

fn get_thumbnail_path(video_path: &Path) -> Option<PathBuf> {
    let cache_dir = PathBuf::from("/tmp/omarchy_thumbs");
    let _ = std::fs::create_dir_all(&cache_dir);

    let ext = video_path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp") {
        return Some(video_path.to_path_buf());
    }

    let stem = video_path.file_stem().and_then(|s| s.to_str()).unwrap_or("thumb");
    let hash = format!("{:x}", md5_hash(video_path.to_string_lossy().as_bytes()));
    let thumb_file = cache_dir.join(format!("{}_{}.jpg", stem, &hash[..8]));

    if thumb_file.exists() {
        return Some(thumb_file);
    }

    let input_str = video_path.to_string_lossy().to_string();
    let thumb_str = thumb_file.to_string_lossy().to_string();

    std::thread::spawn(move || {
        let _ = Command::new("ffmpeg")
            .args(["-ss", "00:00:01", "-i", &input_str, "-vframes", "1", "-s", "320x180", "-y", &thumb_str])
            .output();
    });

    None
}

fn get_web_thumbnail_path(target: &str) -> Option<PathBuf> {
    let path = Path::new(target);
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();

    if matches!(ext.as_str(), "mp4" | "mkv" | "webm" | "avi" | "mov" | "gif" | "png" | "jpg" | "jpeg" | "webp") {
        return get_thumbnail_path(path);
    }

    let cache_dir = PathBuf::from("/tmp/omarchy_thumbs");
    let _ = std::fs::create_dir_all(&cache_dir);

    let hash = format!("{:x}", md5_hash(target.as_bytes()));
    let thumb_file = cache_dir.join(format!("web_{}.jpg", &hash[..8]));

    if thumb_file.exists() {
        return Some(thumb_file);
    }

    let thumb_str = thumb_file.to_string_lossy().to_string();
    let target_url = target.to_string();

    std::thread::spawn(move || {
        let res = Command::new("chromium")
            .args([
                "--headless",
                "--disable-gpu",
                &format!("--screenshot={}", thumb_str),
                "--window-size=1280,720",
                &target_url,
            ])
            .output();
        if res.is_err() || !Path::new(&thumb_str).exists() {
            let title_text = Path::new(&target_url).file_name().and_then(|n| n.to_str()).unwrap_or("Web Stream");
            let svg_content = format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"320\" height=\"180\" viewBox=\"0 0 320 180\">\
  <rect width=\"320\" height=\"180\" fill=\"#141824\"/>\
  <rect x=\"10\" y=\"10\" width=\"300\" height=\"160\" rx=\"10\" fill=\"#1c2333\" stroke=\"#00f0ff\" stroke-width=\"2\"/>\
  <text x=\"160\" y=\"80\" fill=\"#00f0ff\" font-family=\"sans-serif\" font-size=\"18\" font-weight=\"bold\" text-anchor=\"middle\">🌐 WEB WALLPAPER</text>\
  <text x=\"160\" y=\"115\" fill=\"#00ff9d\" font-family=\"sans-serif\" font-size=\"11\" text-anchor=\"middle\">{}</text>\
</svg>",
                title_text
            );
            let svg_file = cache_dir.join(format!("web_{}.svg", &hash[..8]));
            let _ = std::fs::write(&svg_file, svg_content);
            let _ = Command::new("ffmpeg")
                .args(["-i", &svg_file.to_string_lossy(), "-y", &thumb_str])
                .output();
        }
    });

    None
}

fn load_egui_texture(ctx: &egui::Context, path: &Path) -> Option<egui::TextureHandle> {
    let img = image::open(path).ok()?;
    let rgba = img.to_rgba8();
    let pixels = rgba.as_raw();
    let color_img = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], pixels);
    let name = path.to_string_lossy().to_string();
    Some(ctx.load_texture(name, color_img, egui::TextureOptions::LINEAR))
}

fn get_file_size_str(path: &Path) -> String {
    if let Ok(meta) = std::fs::metadata(path) {
        let bytes = meta.len();
        if bytes > 1024 * 1024 {
            format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
        } else if bytes > 1024 {
            format!("{:.1} KB", bytes as f64 / 1024.0)
        } else {
            format!("{} B", bytes)
        }
    } else {
        "Unknown size".into()
    }
}

impl OmarchyGuiApp {
    fn new(config: Config) -> Self {
        let wallpapers = Self::scan_wallpapers(&config.wallpaper_dir);
        let selected_wallpaper = wallpapers.first().cloned();
        let status = Arc::new(Mutex::new(None));
        let workspace_mappings = Arc::new(Mutex::new(config.workspace_wallpapers.clone()));
        let autostart_enabled = Config::is_autostart_enabled();

        let app = Self {
            config: config.clone(),
            status,
            workspace_mappings,
            wallpapers,
            selected_wallpaper,
            selected_workspace: "1".to_string(),
            search_filter: String::new(),
            category_filter: CategoryFilter::All,
            view_mode: ViewMode::Grid,
            web_url_input: "https://".to_string(),
            new_web_title: String::new(),
            new_web_category: "Web Animation".to_string(),
            volume_slider: config.volume,
            opacity_slider: config.opacity,
            autostart_enabled,
            status_message: "Ready".to_string(),
            slideshow_interval_secs: config.slideshow_interval,
            slideshow_shuffle: config.slideshow_shuffle,
            show_doctor: false,
            show_logs: false,
            texture_cache: HashMap::new(),
            last_poll_instant: std::time::Instant::now(),
        };

        app.fetch_or_spawn_daemon();
        app.fetch_workspace_mappings();
        app
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

    fn poll_daemon_status(&self) {
        let socket = self.config.socket_path.clone();
        let status_arc = self.status.clone();
        let mappings_arc = self.workspace_mappings.clone();

        tokio::spawn(async move {
            if let Ok(IpcResponse::Status(st)) = send_ipc_request(&socket, &IpcRequest::GetStatus).await {
                if let Ok(mut guard) = status_arc.lock() {
                    *guard = Some(st);
                }
            } else if let Ok(mut guard) = status_arc.lock() {
                *guard = None;
            }

            if let Ok(IpcResponse::WorkspaceMappings { mappings, .. }) =
                send_ipc_request(&socket, &IpcRequest::GetWorkspaceMappings).await
            {
                if let Ok(mut guard) = mappings_arc.lock() {
                    *guard = mappings;
                }
            }
        });
    }

    fn fetch_or_spawn_daemon(&self) {
        let socket = self.config.socket_path.clone();
        let status_arc = self.status.clone();

        tokio::spawn(async move {
            let res = send_ipc_request(&socket, &IpcRequest::GetStatus).await;
            if let Ok(IpcResponse::Status(st)) = res {
                if let Ok(mut guard) = status_arc.lock() {
                    *guard = Some(st);
                }
            } else {
                let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("omarchy-wall"));
                let _ = Command::new(exe).arg("daemon").spawn();
            }
        });
    }

    fn fetch_workspace_mappings(&self) {
        let socket = self.config.socket_path.clone();
        let mappings_arc = self.workspace_mappings.clone();

        tokio::spawn(async move {
            if let Ok(IpcResponse::WorkspaceMappings { mappings, .. }) =
                send_ipc_request(&socket, &IpcRequest::GetWorkspaceMappings).await
            {
                if let Ok(mut guard) = mappings_arc.lock() {
                    *guard = mappings;
                }
            }
        });
    }

    fn send_request(&self, req: IpcRequest) {
        let socket = self.config.socket_path.clone();
        let status_arc = self.status.clone();
        let mappings_arc = self.workspace_mappings.clone();

        tokio::spawn(async move {
            let resp = send_ipc_request(&socket, &req).await;
            if let Ok(IpcResponse::Status(st)) = send_ipc_request(&socket, &IpcRequest::GetStatus).await {
                if let Ok(mut guard) = status_arc.lock() {
                    *guard = Some(st);
                }
            }
            if let Ok(IpcResponse::WorkspaceMappings { mappings, .. }) =
                send_ipc_request(&socket, &IpcRequest::GetWorkspaceMappings).await
            {
                if let Ok(mut guard) = mappings_arc.lock() {
                    *guard = mappings;
                }
            }
            if let Ok(IpcResponse::Err { message }) = resp {
                eprintln!("{}", message);
            }
        });
    }
}

impl eframe::App for OmarchyGuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.last_poll_instant.elapsed() > std::time::Duration::from_millis(800) {
            self.last_poll_instant = std::time::Instant::now();
            self.poll_daemon_status();
        }

        let mut style = (*ctx.style()).clone();
        style.visuals.dark_mode = true;
        style.visuals.override_text_color = Some(egui::Color32::from_rgb(235, 240, 250));
        style.visuals.panel_fill = egui::Color32::from_rgb(11, 13, 20);
        style.visuals.window_fill = egui::Color32::from_rgb(18, 22, 34);
        style.visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(22, 27, 42);
        ctx.set_style(style);

        let mut pending_action: Option<IpcRequest> = None;
        let mut pending_msg: Option<String> = None;

        let current_status = self.status.lock().ok().and_then(|s| s.clone());
        let current_wall = current_status.as_ref().and_then(|s| s.current_wallpaper.clone());
        let active_ws = current_status.as_ref().and_then(|s| s.active_workspace.clone());
        let current_mode = current_status.as_ref().map(|st| st.mode.clone()).unwrap_or_else(|| "workspace".into());
        let is_paused = current_status.as_ref().map(|st| st.is_paused).unwrap_or(false);
        let mappings = self.workspace_mappings.lock().ok().map(|m| m.clone()).unwrap_or_default();

        // 1. TOP HEADER PANEL
        egui::TopBottomPanel::top("top_header").show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.heading(
                    egui::RichText::new("🌌 OMYWALL WALLPAPER ENGINE")
                        .color(egui::Color32::from_rgb(0, 240, 255))
                        .size(19.0)
                        .strong(),
                );
                ui.label(egui::RichText::new("v3.5").color(egui::Color32::from_rgb(160, 100, 255)).small().strong());

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // 1-CLICK AUTOSTART TOGGLE BUTTON
                    let autostart_btn = if self.autostart_enabled {
                        egui::RichText::new("🚀 AUTOSTART ON").color(egui::Color32::from_rgb(0, 255, 160)).strong().small()
                    } else {
                        egui::RichText::new("🚀 AUTOSTART OFF").color(egui::Color32::from_rgb(150, 165, 190)).small()
                    };

                    if ui.button(autostart_btn).clicked() {
                        let new_state = !self.autostart_enabled;
                        if Config::set_autostart(new_state).is_ok() {
                            self.autostart_enabled = new_state;
                            pending_msg = Some(format!("Autostart on boot {}", if new_state { "ENABLED 🟢" } else { "DISABLED 🔴" }));
                        }
                    }

                    let is_online = current_status.is_some();
                    if is_online {
                        let mode_label = if current_mode == "monitor" || current_mode == "screen" {
                            "🖥 ACTIVE SCREEN MODE"
                        } else {
                            "🗔 WORKSPACE MODE"
                        };
                        if ui.button(egui::RichText::new(mode_label).color(egui::Color32::from_rgb(255, 190, 50)).strong().small()).clicked() {
                            pending_action = Some(IpcRequest::ToggleMode);
                            pending_msg = Some("Toggled wallpaper mode".into());
                        }

                        egui::Frame::none()
                            .fill(egui::Color32::from_rgb(10, 45, 25))
                            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(0, 255, 140)))
                            .rounding(6.0)
                            .inner_margin(egui::Margin::symmetric(10.0, 4.0))
                            .show(ui, |ui| {
                                ui.label(egui::RichText::new("🟢 DAEMON ONLINE").color(egui::Color32::from_rgb(0, 255, 150)).small().strong());
                            });
                    } else {
                        if ui.button(egui::RichText::new("▶ Start Daemon").color(egui::Color32::from_rgb(0, 240, 255)).small().strong()).clicked() {
                            let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("omarchy-wall"));
                            let _ = Command::new(exe).arg("daemon").spawn();
                            self.fetch_or_spawn_daemon();
                        }

                        egui::Frame::none()
                            .fill(egui::Color32::from_rgb(45, 15, 15))
                            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(240, 70, 70)))
                            .rounding(6.0)
                            .inner_margin(egui::Margin::symmetric(10.0, 4.0))
                            .show(ui, |ui| {
                                ui.label(egui::RichText::new("🔴 DAEMON OFFLINE").color(egui::Color32::from_rgb(255, 100, 100)).small().strong());
                            });
                    }

                    if ui.selectable_label(self.show_doctor, "🛠 System Doctor").clicked() {
                        self.show_doctor = !self.show_doctor;
                    }
                    if ui.selectable_label(self.show_logs, "📜 System Logs").clicked() {
                        self.show_logs = !self.show_logs;
                    }
                });
            });

            ui.add_space(6.0);
            ui.separator();
            ui.add_space(6.0);

            // 2. WORKSPACE QUICK MATRIX BAR (1..10) - Uniform Spacing & Hover Tooltips Fix
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
                ui.label(egui::RichText::new("WORKSPACES:").color(egui::Color32::from_rgb(140, 160, 190)).strong().small());
                ui.add_space(4.0);

                for ws_id in 1..=10 {
                    let ws_str = ws_id.to_string();
                    let is_live_focused = active_ws.as_ref() == Some(&ws_str);
                    let is_selected_tab = self.selected_workspace == ws_str;
                    let mapped_wall = mappings.get(&ws_str).cloned().unwrap_or_default();
                    let has_mapping = !mapped_wall.is_empty();

                    let mapped_filename = if has_mapping {
                        Path::new(&mapped_wall)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(&mapped_wall)
                            .to_string()
                    } else {
                        "Unassigned / Desktop Default".to_string()
                    };

                    let (btn_bg, stroke_color, text_color, badge_symbol) = if is_live_focused {
                        (
                            egui::Color32::from_rgb(10, 55, 35),
                            egui::Stroke::new(1.5, egui::Color32::from_rgb(0, 255, 160)),
                            egui::Color32::from_rgb(0, 255, 160),
                            "🟢",
                        )
                    } else if is_selected_tab {
                        (
                            egui::Color32::from_rgb(30, 45, 75),
                            egui::Stroke::new(1.5, egui::Color32::from_rgb(0, 220, 255)),
                            egui::Color32::from_rgb(0, 220, 255),
                            "📌",
                        )
                    } else if has_mapping {
                        (
                            egui::Color32::from_rgb(20, 28, 44),
                            egui::Stroke::new(1.0, egui::Color32::from_rgb(140, 90, 240)),
                            egui::Color32::from_rgb(200, 215, 245),
                            "🗔",
                        )
                    } else {
                        (
                            egui::Color32::from_rgb(16, 19, 28),
                            egui::Stroke::new(1.0, egui::Color32::from_rgb(35, 42, 60)),
                            egui::Color32::from_rgb(120, 135, 155),
                            "",
                        )
                    };

                    let label_text = if badge_symbol.is_empty() {
                        format!("WS {}", ws_str)
                    } else {
                        format!("{} {}", badge_symbol, ws_str)
                    };

                    let response = egui::Frame::none()
                        .fill(btn_bg)
                        .stroke(stroke_color)
                        .rounding(6.0)
                        .inner_margin(egui::Margin::symmetric(10.0, 5.0))
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new(&label_text).color(text_color).strong().size(12.0));
                        })
                        .response
                        .on_hover_ui(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(format!("Workspace {}", ws_str)).strong().size(13.0));
                                if is_live_focused {
                                    ui.label(egui::RichText::new("🟢 ACTIVE FOCUS").color(egui::Color32::from_rgb(0, 255, 160)).small().strong());
                                }
                            });
                            ui.separator();
                            ui.label(egui::RichText::new(format!("Assigned: {}", mapped_filename)).color(egui::Color32::from_rgb(180, 195, 220)).small());
                            ui.label(egui::RichText::new("Click to switch desktop workspace").color(egui::Color32::from_rgb(120, 135, 155)).small());
                        });

                    if response.interact(egui::Sense::click()).clicked() {
                        self.selected_workspace = ws_str.clone();
                        pending_action = Some(IpcRequest::SwitchWorkspace { workspace: ws_str });
                    }
                }
            });
            ui.add_space(6.0);
        });

        // 3. BOTTOM STATUS BAR
        egui::TopBottomPanel::bottom("bottom_status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&self.status_message).color(egui::Color32::from_rgb(150, 165, 185)).small());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(ref wall) = current_wall {
                        let filename = Path::new(wall).file_name().and_then(|n| n.to_str()).unwrap_or(wall);
                        ui.label(egui::RichText::new(format!("Playing: {}", filename)).color(egui::Color32::from_rgb(0, 230, 140)).small().strong());
                    } else {
                        ui.label(egui::RichText::new("Idle / Desktop Default Wallpaper").color(egui::Color32::from_rgb(255, 180, 50)).small());
                    }
                });
            });
        });

        // 4. FIXED-WIDTH RIGHT CONTROL INSPECTOR PANEL (340px GLASS PANEL)
        egui::SidePanel::right("control_panel")
            .resizable(false)
            .exact_width(340.0)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.heading("⚙ Control Inspector");
                ui.add_space(8.0);

                let selected_opt = self.selected_wallpaper.clone();
                if let Some(ref selected) = selected_opt {
                    let filename = selected.file_name().and_then(|n| n.to_str()).unwrap_or("Selected");
                    let path_str = selected.to_string_lossy().to_string();
                    let size_str = get_file_size_str(selected);

                    ui.group(|ui| {
                        ui.vertical_centered(|ui| {
                            // Render Hero Thumbnail Preview (Supports Video, Images & Web Wallpapers)
                            if let Some(thumb_path) = get_web_thumbnail_path(&path_str) {
                                if !self.texture_cache.contains_key(&thumb_path) {
                                    if let Some(tex) = load_egui_texture(ctx, &thumb_path) {
                                        self.texture_cache.insert(thumb_path.clone(), tex);
                                    }
                                }
                                if let Some(tex) = self.texture_cache.get(&thumb_path) {
                                    ui.add(egui::Image::new(tex).max_size(egui::vec2(290.0, 155.0)).rounding(6.0));
                                    ui.add_space(6.0);
                                }
                            }

                            ui.label(egui::RichText::new(filename).color(egui::Color32::from_rgb(0, 225, 255)).strong().size(15.0));
                            ui.label(egui::RichText::new(format!("Size: {}  •  Format: {}", size_str, selected.extension().and_then(|e| e.to_str()).unwrap_or("").to_uppercase())).color(egui::Color32::from_rgb(140, 155, 175)).small());

                            ui.add_space(8.0);
                            if ui.button(egui::RichText::new("▶ Apply to Desktop Background").color(egui::Color32::from_rgb(0, 255, 150)).strong()).clicked() {
                                pending_action = Some(IpcRequest::SetWallpaper { path: path_str.clone() });
                                pending_msg = Some(format!("Applied wallpaper: {}", filename));
                            }
                        });
                    });

                    ui.add_space(10.0);
                    ui.label(egui::RichText::new("1-Click Workspace Matrix:").strong().small());
                    ui.horizontal_wrapped(|ui| {
                        for ws_idx in 1..=10 {
                            let ws_str = ws_idx.to_string();
                            let is_mapped_to_this_ws = mappings.get(&ws_str) == Some(&path_str);
                            let btn_text = format!("WS {}", ws_str);

                            let btn_color = if is_mapped_to_this_ws {
                                egui::Color32::from_rgb(0, 240, 140)
                            } else {
                                egui::Color32::from_rgb(180, 195, 215)
                            };

                            if ui.button(egui::RichText::new(&btn_text).color(btn_color).strong()).clicked() {
                                if is_mapped_to_this_ws {
                                    pending_action = Some(IpcRequest::SetWorkspaceWallpaper { workspace: ws_str.clone(), path: "".into() });
                                    pending_msg = Some(format!("Cleared Workspace {} mapping", ws_str));
                                } else {
                                    pending_action = Some(IpcRequest::SetWorkspaceWallpaper { workspace: ws_str.clone(), path: path_str.clone() });
                                    pending_msg = Some(format!("Mapped {} to Workspace {}", filename, ws_str));
                                }
                            }
                        }
                    });
                } else {
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgb(18, 22, 34))
                        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(35, 45, 65)))
                        .rounding(6.0)
                        .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new("🖼 Gallery Selection").strong().color(egui::Color32::from_rgb(0, 220, 255)));
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new("Select a wallpaper from the gallery grid to inspect file specs, apply instantly, or bind to workspaces.").color(egui::Color32::from_rgb(140, 155, 180)).small());
                        });
                }

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);

                // Web Engine URL Loader Section
                ui.label(egui::RichText::new("🌐 Web URL & Widget Loader").strong());
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.web_url_input).hint_text("https://... or file://..."));
                    if ui.button("▶ Load Web").clicked() {
                        let url = self.web_url_input.clone();
                        pending_action = Some(IpcRequest::SetWallpaper { path: url.clone() });
                        pending_msg = Some(format!("Loaded web wallpaper: {}", url));
                    }
                });

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);

                ui.label(egui::RichText::new("Playback & Engine Controls").strong());
                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    if ui.button(if is_paused { "▶ Resume" } else { "⏸ Pause" }).clicked() {
                        pending_action = Some(IpcRequest::TogglePause);
                    }
                    if ui.button("⏹ Stop / Clear").clicked() {
                        pending_action = Some(IpcRequest::StopWallpaper);
                        pending_msg = Some("Stopped active wallpaper playback".into());
                    }
                });

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label("Volume:");
                    if ui.add(egui::Slider::new(&mut self.volume_slider, 0..=100).suffix("%")).changed() {
                        pending_action = Some(IpcRequest::SetVolume { volume: self.volume_slider });
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("Opacity:");
                    if ui.add(egui::Slider::new(&mut self.opacity_slider, 0.0..=1.0)).changed() {
                        pending_action = Some(IpcRequest::SetOpacity { opacity: self.opacity_slider });
                    }
                });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label("Hardware Dec:");
                    let mut hwdec = current_status.as_ref().map(|s| s.hwdec.clone()).unwrap_or_else(|| "auto".to_string());
                    egui::ComboBox::from_id_salt("hwdec_combo")
                        .selected_text(&hwdec)
                        .show_ui(ui, |ui| {
                            if ui.selectable_value(&mut hwdec, "auto".to_string(), "auto (GPU)").clicked() {
                                pending_action = Some(IpcRequest::SetHwdec { hwdec: "auto".into() });
                            }
                            if ui.selectable_value(&mut hwdec, "vaapi".to_string(), "vaapi (Intel/AMD)").clicked() {
                                pending_action = Some(IpcRequest::SetHwdec { hwdec: "vaapi".into() });
                            }
                            if ui.selectable_value(&mut hwdec, "nvdec".to_string(), "nvdec (NVIDIA)").clicked() {
                                pending_action = Some(IpcRequest::SetHwdec { hwdec: "nvdec".into() });
                            }
                            if ui.selectable_value(&mut hwdec, "no".to_string(), "no (Software)").clicked() {
                                pending_action = Some(IpcRequest::SetHwdec { hwdec: "no".into() });
                            }
                        });
                });

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);

                ui.label(egui::RichText::new("Automated Slideshow").strong());
                let slideshow_active = current_status.as_ref().map(|s| s.slideshow_active).unwrap_or(false);

                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.slideshow_shuffle, "Shuffle");
                    ui.add(egui::Slider::new(&mut self.slideshow_interval_secs, 10..=3600).prefix("Interval: ").suffix("s"));
                });

                ui.add_space(4.0);
                if slideshow_active {
                    if ui.button("⏹ Stop Slideshow").clicked() {
                        pending_action = Some(IpcRequest::StopSlideshow);
                        pending_msg = Some("Stopped automated slideshow".into());
                    }
                } else {
                    if ui.button("▶ Start Slideshow").clicked() {
                        pending_action = Some(IpcRequest::StartSlideshow {
                            interval_secs: self.slideshow_interval_secs,
                            shuffle: self.slideshow_shuffle,
                        });
                        pending_msg = Some(format!("Started slideshow ({}s)", self.slideshow_interval_secs));
                    }
                }
            });

        // 5. CENTRAL PANEL: WALLPAPER GALLERY GRID & SAVED WEB WALLPAPER BOOKMARKS
        egui::CentralPanel::default().show(ctx, |ui| {
            // Optional Drawer for System Doctor
            if self.show_doctor {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.heading("🛠 OMYWALL System Doctor");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("❌ Close Doctor").clicked() {
                                self.show_doctor = false;
                            }
                            if ui.button(egui::RichText::new("⚡ Run Terminal Installer Script").color(egui::Color32::from_rgb(0, 240, 255)).strong().small()).clicked() {
                                pending_msg = Some(run_installer_script());
                            }
                        });
                    });
                    ui.add_space(6.0);

                    let tools = check_installed_tools();

                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);
                        for tool in &tools {
                            let (bg_color, stroke_color, badge_text, badge_color) = if tool.installed {
                                (
                                    egui::Color32::from_rgb(10, 36, 24),
                                    egui::Stroke::new(1.0, egui::Color32::from_rgb(0, 220, 130)),
                                    "🟢 INSTALLED",
                                    egui::Color32::from_rgb(0, 240, 140),
                                )
                            } else {
                                (
                                    egui::Color32::from_rgb(36, 15, 20),
                                    egui::Stroke::new(1.0, egui::Color32::from_rgb(240, 70, 70)),
                                    "🔴 MISSING",
                                    egui::Color32::from_rgb(255, 100, 100),
                                )
                            };

                            egui::Frame::none()
                                .fill(bg_color)
                                .stroke(stroke_color)
                                .rounding(6.0)
                                .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(&tool.name).strong().size(12.0));
                                        ui.label(egui::RichText::new(badge_text).color(badge_color).small().strong());
                                    });
                                })
                                .response
                                .on_hover_ui(|ui| {
                                    ui.label(egui::RichText::new(&tool.name).strong().size(13.0));
                                    ui.separator();
                                    ui.label(egui::RichText::new(&tool.description).color(egui::Color32::from_rgb(180, 195, 215)).small());
                                });
                        }
                    });
                });
                ui.add_space(8.0);
            }

            // Optional Drawer for System Logs
            if self.show_logs {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.heading("📜 Diagnostic Logs");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("❌ Close").clicked() {
                                self.show_logs = false;
                            }
                        });
                    });
                    let log_path = get_log_path();
                    let content = if log_path.exists() {
                        std::fs::read_to_string(&log_path).unwrap_or_default()
                    } else {
                        "No logs found.".into()
                    };
                    egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                        ui.add(egui::TextEdit::multiline(&mut content.as_str()).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY));
                    });
                });
                ui.add_space(10.0);
            }

            // Main Gallery Controls & Format Filter Pills
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    // Category Filter Pills
                    if ui.selectable_label(self.category_filter == CategoryFilter::All, format!("All ({})", self.wallpapers.len())).clicked() {
                        self.category_filter = CategoryFilter::All;
                    }
                    if ui.selectable_label(self.category_filter == CategoryFilter::Videos, "🎥 Videos").clicked() {
                        self.category_filter = CategoryFilter::Videos;
                    }
                    if ui.selectable_label(self.category_filter == CategoryFilter::WebWidgets, "🌐 Web Widgets & Saved Animations").clicked() {
                        self.category_filter = CategoryFilter::WebWidgets;
                    }
                    if ui.selectable_label(self.category_filter == CategoryFilter::StaticImages, "🖼 Static Images").clicked() {
                        self.category_filter = CategoryFilter::StaticImages;
                    }
                    if ui.selectable_label(self.category_filter == CategoryFilter::SteamWorkshop, "🎮 Steam Workshop").clicked() {
                        self.category_filter = CategoryFilter::SteamWorkshop;
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.selectable_label(self.view_mode == ViewMode::List, "📋 List View").clicked() {
                            self.view_mode = ViewMode::List;
                        }
                        if ui.selectable_label(self.view_mode == ViewMode::Grid, "🖼 Grid View").clicked() {
                            self.view_mode = ViewMode::Grid;
                        }

                        if ui.button("📁 Folder").clicked() {
                            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                                self.config.wallpaper_dir = folder;
                                let _ = self.config.save();
                                self.wallpapers = Self::scan_wallpapers(&self.config.wallpaper_dir);
                                pending_msg = Some(format!("Wallpaper directory set to {}", self.config.wallpaper_dir.display()));
                            }
                        }
                        if ui.button("🔄 Rescan").clicked() {
                            self.wallpapers = Self::scan_wallpapers(&self.config.wallpaper_dir);
                            pending_msg = Some(format!("Rescanned wallpapers (found {} files)", self.wallpapers.len()));
                        }
                    });
                });

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("🔍");
                    ui.add(egui::TextEdit::singleline(&mut self.search_filter).hint_text("Search wallpaper name or format..."));
                });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                // SPECIAL TAB: WEB WIDGETS & SAVED ANIMATED WEBSITES LIBRARY
                if self.category_filter == CategoryFilter::WebWidgets {
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.heading("🌐 Animated Web & HTML Widgets");
                            ui.label(egui::RichText::new("● Live HTML5 / JS").color(egui::Color32::from_rgb(0, 240, 255)).small().strong())
                                .on_hover_text("Click any bookmark to launch interactive canvas, web stream, or HTML desktop widget");
                        });
                        ui.add_space(6.0);

                        egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                            let bookmarks = self.config.saved_web_wallpapers.clone();
                            for bm in &bookmarks {
                                ui.group(|ui| {
                                    ui.horizontal(|ui| {
                                        let target_url = if bm.url.starts_with("assets/") {
                                            std::env::current_dir()
                                                .unwrap_or_default()
                                                .join(&bm.url)
                                                .to_string_lossy()
                                                .to_string()
                                        } else {
                                            bm.url.clone()
                                        };

                                        // Render Web Thumbnail Preview Image
                                        if let Some(thumb_path) = get_web_thumbnail_path(&target_url) {
                                            if !self.texture_cache.contains_key(&thumb_path) {
                                                if let Some(tex) = load_egui_texture(ctx, &thumb_path) {
                                                    self.texture_cache.insert(thumb_path.clone(), tex);
                                                }
                                            }
                                            if let Some(tex) = self.texture_cache.get(&thumb_path) {
                                                ui.add(egui::Image::new(tex).max_size(egui::vec2(70.0, 40.0)).rounding(4.0));
                                            }
                                        }

                                        ui.vertical(|ui| {
                                            ui.label(egui::RichText::new(&bm.title).color(egui::Color32::from_rgb(0, 225, 255)).strong().size(14.0));
                                            ui.label(egui::RichText::new(&bm.url).color(egui::Color32::from_rgb(140, 155, 175)).small());
                                        });

                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            if !bm.is_demo {
                                                if ui.button("🗑").clicked() {
                                                    self.config.remove_web_bookmark(&bm.url);
                                                }
                                            }
                                            if ui.button(egui::RichText::new("▶ Launch Web Wallpaper").color(egui::Color32::from_rgb(0, 255, 150)).strong()).clicked() {
                                                pending_action = Some(IpcRequest::SetWallpaper { path: target_url.clone() });
                                                pending_msg = Some(format!("Launched web wallpaper: {}", bm.title));
                                            }
                                        });
                                    });
                                });
                                ui.add_space(4.0);
                            }
                        });

                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(8.0);

                        // FORM TO SAVE CUSTOM ANIMATED WEBSITE / HTML WALLPAPER
                        ui.label(egui::RichText::new("➕ Save Custom Animated Website or HTML File").strong());
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut self.new_web_title).hint_text("Title (e.g. Matrix Canvas)"));
                            ui.add(egui::TextEdit::singleline(&mut self.web_url_input).hint_text("URL or Path (https://...)"));
                            if ui.button("💾 Save Website").clicked() {
                                if !self.new_web_title.trim().is_empty() && !self.web_url_input.trim().is_empty() {
                                    self.config.add_web_bookmark(
                                        self.new_web_title.trim().to_string(),
                                        self.web_url_input.trim().to_string(),
                                        self.new_web_category.clone(),
                                    );
                                    pending_msg = Some(format!("Saved web wallpaper: {}", self.new_web_title));
                                    self.new_web_title.clear();
                                }
                            }
                        });
                    });
                    ui.add_space(10.0);
                }

                // GENERAL WALLPAPER GALLERY SCROLL AREA
                if self.wallpapers.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(40.0);
                        ui.label(egui::RichText::new("📂 No Video or Image Wallpapers Found").color(egui::Color32::from_rgb(240, 160, 50)).strong());
                        ui.label(format!("Folder: {}", self.config.wallpaper_dir.display()));
                        ui.add_space(10.0);
                        if ui.button("➕ Open Video File...").clicked() {
                            if let Some(file) = rfd::FileDialog::new()
                                .add_filter("Videos & GIFs", &["mkv", "mp4", "webm", "avi", "mov", "gif", "m4v", "flv"])
                                .pick_file()
                            {
                                let path_str = file.to_string_lossy().to_string();
                                pending_action = Some(IpcRequest::SetWallpaper { path: path_str });
                                pending_msg = Some(format!("Applied wallpaper: {}", file.display()));
                            }
                        }
                    });
                } else {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        let wallpapers_clone = self.wallpapers.clone();

                        let filtered: Vec<&PathBuf> = wallpapers_clone
                            .iter()
                            .filter(|path| {
                                let path_str = path.to_string_lossy().to_lowercase();
                                if !self.search_filter.is_empty() && !path_str.contains(&self.search_filter.to_lowercase()) {
                                    return false;
                                }

                                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
                                match self.category_filter {
                                    CategoryFilter::All => true,
                                    CategoryFilter::Videos => matches!(ext.as_str(), "mp4" | "mkv" | "webm" | "avi" | "mov" | "gif" | "flv" | "m4v" | "wmv"),
                                    CategoryFilter::WebWidgets => matches!(ext.as_str(), "html" | "htm" | "js"),
                                    CategoryFilter::StaticImages => matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp"),
                                    CategoryFilter::SteamWorkshop => path_str.contains("431960") || path_str.contains("workshop"),
                                }
                            })
                            .collect();

                        for path in filtered {
                            let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("Unknown");
                            let path_str = path.to_string_lossy().to_string();
                            let is_active = current_wall.as_ref().map(|curr| Path::new(curr) == path).unwrap_or(false);
                            let is_selected = self.selected_wallpaper.as_ref() == Some(path);

                            let mut mapped_ws = Vec::new();
                            for (ws_key, ws_target) in &mappings {
                                if ws_target == &path_str {
                                    mapped_ws.push(ws_key.clone());
                                }
                            }
                            mapped_ws.sort();

                            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_uppercase();
                            let card_bg = if is_selected {
                                egui::Color32::from_rgb(30, 48, 75)
                            } else if is_active {
                                egui::Color32::from_rgb(12, 40, 28)
                            } else {
                                egui::Color32::from_rgb(22, 27, 42)
                            };

                            let card_stroke = if is_active {
                                egui::Stroke::new(1.8, egui::Color32::from_rgb(0, 255, 150))
                            } else if is_selected {
                                egui::Stroke::new(1.8, egui::Color32::from_rgb(0, 225, 255))
                            } else {
                                egui::Stroke::new(1.0, egui::Color32::from_rgb(35, 45, 65))
                            };

                            let card_resp = egui::Frame::none()
                                .fill(card_bg)
                                .stroke(card_stroke)
                                .rounding(6.0)
                                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        if let Some(thumb_path) = get_web_thumbnail_path(&path_str) {
                                            if !self.texture_cache.contains_key(&thumb_path) {
                                                if let Some(tex) = load_egui_texture(ctx, &thumb_path) {
                                                    self.texture_cache.insert(thumb_path.clone(), tex);
                                                }
                                            }
                                            if let Some(tex) = self.texture_cache.get(&thumb_path) {
                                                ui.add(egui::Image::new(tex).max_size(egui::vec2(60.0, 36.0)).rounding(4.0));
                                            }
                                        }

                                        let badge_color = match ext.as_str() {
                                            "MKV" => egui::Color32::from_rgb(255, 120, 0),
                                            "MP4" => egui::Color32::from_rgb(0, 180, 240),
                                            "GIF" => egui::Color32::from_rgb(220, 100, 255),
                                            "HTML" | "JS" => egui::Color32::from_rgb(255, 200, 0),
                                            _ => egui::Color32::from_rgb(150, 165, 185),
                                        };
                                        ui.label(egui::RichText::new(format!("[{}]", ext)).color(badge_color).strong().small());

                                        if is_active {
                                            ui.label(egui::RichText::new(filename).color(egui::Color32::from_rgb(0, 255, 150)).strong());
                                            ui.label(egui::RichText::new("● LIVE").color(egui::Color32::from_rgb(255, 190, 50)).small().strong());
                                        } else {
                                            ui.label(filename);
                                        }

                                        if !mapped_ws.is_empty() {
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                ui.label(egui::RichText::new(format!("WS: {}", mapped_ws.join(","))).color(egui::Color32::from_rgb(180, 110, 255)).small().strong());
                                            });
                                        }
                                    });
                                })
                                .response;

                            if card_resp.interact(egui::Sense::click()).clicked() {
                                self.selected_wallpaper = Some(path.clone());
                            }
                            ui.add_space(4.0);
                        }
                    });
                }
            });
        });

        if let Some(act) = pending_action {
            self.send_request(act);
        }
        if let Some(msg) = pending_msg {
            self.status_message = msg;
        }

        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }
}
