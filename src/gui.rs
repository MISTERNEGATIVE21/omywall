use eframe::egui;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use crate::config::Config;
use crate::ipc::{send_ipc_request, DaemonStatus, IpcRequest, IpcResponse};
use crate::logger::get_log_path;

static PENDING_THUMBS: Mutex<Option<HashSet<String>>> = Mutex::new(None);
static FAILED_THUMBS: Mutex<Option<HashSet<String>>> = Mutex::new(None);
static DECODED_IMAGES: Mutex<Option<HashMap<PathBuf, egui::ColorImage>>> = Mutex::new(None);
static PENDING_DECODES: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);
static UPDATED_THUMBS: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);

fn notify_thumb_updated(path: PathBuf) {
    if let Ok(mut guard) = UPDATED_THUMBS.lock() {
        let set = guard.get_or_insert_with(HashSet::new);
        set.insert(path);
    }
}

fn is_thumb_pending(key: &str) -> bool {
    if let Ok(guard) = PENDING_THUMBS.lock() {
        if let Some(ref set) = *guard {
            return set.contains(key);
        }
    }
    false
}

fn set_thumb_pending(key: &str, pending: bool) {
    if let Ok(mut guard) = PENDING_THUMBS.lock() {
        let set = guard.get_or_insert_with(HashSet::new);
        if pending {
            set.insert(key.to_string());
        } else {
            set.remove(key);
        }
    }
}

fn request_background_image_decode(ctx: egui::Context, path: PathBuf) {
    {
        if let Ok(mut pending) = PENDING_DECODES.lock() {
            let set = pending.get_or_insert_with(HashSet::new);
            if set.contains(&path) {
                return;
            }
            set.insert(path.clone());
        }
    }

    std::thread::spawn(move || {
        if let Ok(img) = image::open(&path) {
            let rgba = img.to_rgba8();
            let pixels = rgba.as_raw().clone();
            let color_img = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], &pixels);

            if let Ok(mut decoded) = DECODED_IMAGES.lock() {
                let map = decoded.get_or_insert_with(HashMap::new);
                map.insert(path.clone(), color_img);
            }
            ctx.request_repaint();
        }

        if let Ok(mut pending) = PENDING_DECODES.lock() {
            if let Some(set) = pending.as_mut() {
                set.remove(&path);
            }
        }
    });
}

fn load_window_icon() -> Option<egui::IconData> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/home/user"));
    let candidates = [
        PathBuf::from("assets/omywall.svg"),
        PathBuf::from("assets/omywall.png"),
        home.join(".local/share/omywall/assets/omywall.svg"),
        home.join(".local/share/icons/hicolor/scalable/apps/omywall.svg"),
        home.join(".local/share/icons/hicolor/512x512/apps/omywall.png"),
    ];

    for path in &candidates {
        if path.exists() {
            if let Ok(img) = image::open(path) {
                let rgba = img.to_rgba8();
                let width = rgba.width();
                let height = rgba.height();
                let pixels = rgba.into_raw();
                return Some(egui::IconData {
                    rgba: pixels,
                    width,
                    height,
                });
            }
        }
    }
    None
}

pub fn run_gui(config: Config) -> Result<(), eframe::Error> {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("OMYWALL Wallpaper Engine v3.5")
        .with_app_id("omywall")
        .with_inner_size([1240.0, 820.0])
        .with_min_inner_size([940.0, 640.0]);

    if let Some(icon) = load_window_icon() {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "OMYWALL Wallpaper Engine v3.5",
        options,
        Box::new(|_cc| Ok(Box::new(OmywallGuiApp::new(config)))),
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

struct OmywallGuiApp {
    config: Config,
    status: Arc<Mutex<Option<DaemonStatus>>>,
    wallpapers: Vec<PathBuf>,
    selected_wallpaper: Option<PathBuf>,
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
    #[allow(dead_code)]
    slideshow_interval_secs: u64,
    #[allow(dead_code)]
    slideshow_shuffle: bool,
    show_doctor: bool,
    show_logs: bool,
    show_hyprlock: bool,
    show_gpu_settings: bool,
    show_inspector: bool,
    textures_loaded_this_frame: usize,
    texture_cache: HashMap<PathBuf, Option<egui::TextureHandle>>,
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

fn get_current_time_str() -> (String, String) {
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        let time_str = format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec);
        let months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
        let days = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        let m_idx = (tm.tm_mon as usize).min(11);
        let d_idx = (tm.tm_wday as usize).min(6);
        let date_str = format!("{}, {} {} {:04}", days[d_idx], months[m_idx], tm.tm_mday, tm.tm_year + 1900);
        (time_str, date_str)
    }
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
        ("hyprlock", "hyprlock", "Wayland GPU-accelerated screen locker & screensaver"),
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
    let cwd_script = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("scripts")
        .join("install_deps.sh");

    let exe_script = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|parent| parent.join("scripts").join("install_deps.sh")));

    let script_path = if cwd_script.exists() {
        Some(cwd_script)
    } else if let Some(ref ep) = exe_script {
        if ep.exists() {
            Some(ep.clone())
        } else {
            None
        }
    } else {
        None
    };

    let raw_cmd = if let Some(sp) = script_path {
        format!("bash {}", sp.display())
    } else {
        "curl -sSL https://raw.githubusercontent.com/MISTERNEGATIVE21/Omywall/main/scripts/install_deps.sh | bash".to_string()
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

    // 2. Try known terminal emulators
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

pub fn md5_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 5381;
    for &b in bytes {
        hash = ((hash << 5).wrapping_add(hash)).wrapping_add(b as u64);
    }
    hash
}

fn generate_video_fallback_image(_title_text: &str, ext_str: &str, target_path: &Path) {
    let width = 320u32;
    let height = 180u32;
    let mut imgbuf = image::RgbImage::new(width, height);

    for (x, y, pixel) in imgbuf.enumerate_pixels_mut() {
        let factor = (x + y) as f32 / (width + height) as f32;
        let r = (13.0 * (1.0 - factor) + 22.0 * factor) as u8;
        let g = (17.0 * (1.0 - factor) + 28.0 * factor) as u8;
        let b = (26.0 * (1.0 - factor) + 43.0 * factor) as u8;
        *pixel = image::Rgb([r, g, b]);
    }

    let (br, bg, bb) = match ext_str.to_uppercase().as_str() {
        "MKV" => (255, 120, 0),
        "MP4" => (0, 180, 240),
        "GIF" => (220, 100, 255),
        _ => (168, 85, 247),
    };

    for x in 8..=311 {
        imgbuf.put_pixel(x, 8, image::Rgb([br, bg, bb]));
        imgbuf.put_pixel(x, 9, image::Rgb([br, bg, bb]));
        imgbuf.put_pixel(x, 170, image::Rgb([br, bg, bb]));
        imgbuf.put_pixel(x, 171, image::Rgb([br, bg, bb]));
    }
    for y in 8..=171 {
        imgbuf.put_pixel(8, y, image::Rgb([br, bg, bb]));
        imgbuf.put_pixel(9, y, image::Rgb([br, bg, bb]));
        imgbuf.put_pixel(310, y, image::Rgb([br, bg, bb]));
        imgbuf.put_pixel(311, y, image::Rgb([br, bg, bb]));
    }

    for y in 70..=110 {
        let max_x = 145 + ((y as i32 - 70) * 35 / 40);
        for x in 145..=max_x.min(185) {
            if x >= 0 && x < 320 && y < 180 {
                imgbuf.put_pixel(x as u32, y as u32, image::Rgb([br, bg, bb]));
            }
        }
    }

    let _ = imgbuf.save(target_path);
}

fn get_thumbnail_path(video_path: &Path) -> Option<PathBuf> {
    let cache_dir = PathBuf::from("/tmp/omywall_thumbs");
    let _ = std::fs::create_dir_all(&cache_dir);

    let ext = video_path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp") {
        return Some(video_path.to_path_buf());
    }

    let key = video_path.to_string_lossy().to_string();
    let stem = video_path.file_stem().and_then(|s| s.to_str()).unwrap_or("thumb");
    let hash = format!("{:x}", md5_hash(key.as_bytes()));
    let thumb_file = cache_dir.join(format!("{}_{}.jpg", stem, &hash[..8]));

    if thumb_file.exists() {
        return Some(thumb_file);
    }

    let title = video_path.file_name().and_then(|n| n.to_str()).unwrap_or("Video");
    generate_video_fallback_image(title, &ext, &thumb_file);

    if !is_thumb_pending(&key) {
        set_thumb_pending(&key, true);
        let input_str = key.clone();
        let thumb_str = thumb_file.to_string_lossy().to_string();
        let ext_str = ext.clone();

        std::thread::spawn(move || {
            let mut res = Command::new("ffmpeg")
                .args(["-ss", "00:00:00.500", "-i", &input_str, "-vframes", "1", "-s", "320x180", "-y", &thumb_str])
                .output();

            if res.is_err() || !Path::new(&thumb_str).exists() {
                res = Command::new("ffmpeg")
                    .args(["-i", &input_str, "-vframes", "1", "-s", "320x180", "-y", &thumb_str])
                    .output();
            }

            if res.is_err() || !Path::new(&thumb_str).exists() {
                let _ = Command::new("mpv")
                    .args(["--no-audio", "--frames=1", &format!("--o={}", thumb_str), &input_str])
                    .output();
            }

            if !Path::new(&thumb_str).exists() {
                let title = Path::new(&input_str).file_name().and_then(|n| n.to_str()).unwrap_or("Video");
                generate_video_fallback_image(title, &ext_str, Path::new(&thumb_str));
            }

            set_thumb_pending(&input_str, false);
            notify_thumb_updated(PathBuf::from(&thumb_str));
        });
    }

    Some(thumb_file)
}

fn generate_web_fallback_image(target_path: &Path) {
    let width = 320u32;
    let height = 180u32;
    let mut imgbuf = image::RgbImage::new(width, height);

    for (x, y, pixel) in imgbuf.enumerate_pixels_mut() {
        let factor = (x + y) as f32 / (width + height) as f32;
        let r = (18.0 * (1.0 - factor) + 10.0 * factor) as u8;
        let g = (23.0 * (1.0 - factor) + 16.0 * factor) as u8;
        let b = (38.0 * (1.0 - factor) + 28.0 * factor) as u8;
        *pixel = image::Rgb([r, g, b]);
    }

    for x in 8..=311 {
        imgbuf.put_pixel(x, 8, image::Rgb([0, 240, 255]));
        imgbuf.put_pixel(x, 9, image::Rgb([0, 240, 255]));
        imgbuf.put_pixel(x, 170, image::Rgb([0, 240, 255]));
        imgbuf.put_pixel(x, 171, image::Rgb([0, 240, 255]));
    }
    for y in 8..=171 {
        imgbuf.put_pixel(8, y, image::Rgb([0, 240, 255]));
        imgbuf.put_pixel(9, y, image::Rgb([0, 240, 255]));
        imgbuf.put_pixel(310, y, image::Rgb([0, 240, 255]));
        imgbuf.put_pixel(311, y, image::Rgb([0, 240, 255]));
    }

    let _ = imgbuf.save(target_path);
}

fn get_web_thumbnail_path(target: &str) -> Option<PathBuf> {
    let resolved = crate::config::resolve_asset_path(target);
    let path = Path::new(&resolved);
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();

    if matches!(ext.as_str(), "mp4" | "mkv" | "webm" | "avi" | "mov" | "gif" | "png" | "jpg" | "jpeg" | "webp") {
        return get_thumbnail_path(path);
    }

    let cache_dir = PathBuf::from("/tmp/omywall_thumbs");
    let _ = std::fs::create_dir_all(&cache_dir);

    let key = resolved.clone();
    let hash = format!("{:x}", md5_hash(key.as_bytes()));
    let thumb_file = cache_dir.join(format!("web_{}.jpg", &hash[..8]));

    if !thumb_file.exists() {
        generate_web_fallback_image(&thumb_file);
    }

    Some(thumb_file)
}

#[allow(dead_code)]
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

impl OmywallGuiApp {
    pub fn new(config: Config) -> Self {
        let wallpapers = Self::scan_wallpapers(&config.wallpaper_dir);
        let selected_wallpaper = wallpapers.first().cloned();
        let status = Arc::new(Mutex::new(None));
        let autostart_enabled = Config::is_autostart_enabled();

        let app = Self {
            config: config.clone(),
            status,
            wallpapers,
            selected_wallpaper,
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
            show_hyprlock: false,
            show_gpu_settings: false,
            show_inspector: true,
            textures_loaded_this_frame: 0,
            texture_cache: HashMap::new(),
            last_poll_instant: std::time::Instant::now(),
        };

        app.fetch_or_spawn_daemon();
        app
    }

    fn scan_wallpapers(dir: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let mut seen = HashSet::new();
        let valid_exts = ["mkv", "mp4", "webm", "avi", "mov", "gif", "html", "htm", "js", "m4v", "flv", "wmv", "png", "jpg", "jpeg", "webp"];

        fn walk_dir(d: &Path, depth: usize, files: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, valid_exts: &[&str]) {
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
                                if seen.insert(canon.clone()) {
                                    files.push(canon);
                                }
                            }
                        }
                    } else if path.is_dir() {
                        walk_dir(&path, depth + 1, files, seen, valid_exts);
                    }
                }
            }
        }

        walk_dir(dir, 0, &mut files, &mut seen, &valid_exts);

        if let Some(home) = dirs::home_dir() {
            let candidate_dirs = [
                home.join(".local").join("share").join("omywall"),
                home.join(".local").join("share").join("omywall").join("assets"),
                home.join(".config").join("omywall").join("themes"),
                home.join(".config").join("omywall").join("current"),
                home.join(".local").join("share").join("wallpapers"),
                home.join("Pictures").join("Wallpapers"),
                home.join("Pictures"),
                home.join("Videos"),
                home.join(".local").join("share").join("Steam").join("steamapps").join("workshop").join("content").join("431960"),
                home.join(".steam").join("steam").join("steamapps").join("workshop").join("content").join("431960"),
                home.join(".steam").join("root").join("steamapps").join("workshop").join("content").join("431960"),
                PathBuf::from("/usr/share/omywall"),
                PathBuf::from("/usr/share/omywall/assets"),
                PathBuf::from("/usr/local/share/omywall/assets"),
                PathBuf::from("/usr/share/backgrounds"),
                std::env::current_dir().unwrap_or_default().join("assets"),
            ];

            for c_dir in &candidate_dirs {
                if c_dir.exists() {
                    walk_dir(c_dir, 0, &mut files, &mut seen, &valid_exts);
                }
            }
        }

        files.sort();
        files
    }

    fn poll_daemon_status(&self) {
        let socket = self.config.socket_path.clone();
        let status_arc = self.status.clone();

        std::thread::spawn(move || {
            if let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() {
                rt.block_on(async {
                    if let Ok(IpcResponse::Status(st)) = send_ipc_request(&socket, &IpcRequest::GetStatus).await {
                        if let Ok(mut guard) = status_arc.lock() {
                            *guard = Some(st);
                        }
                    } else if let Ok(mut guard) = status_arc.lock() {
                        *guard = None;
                    }
                });
            }
        });
    }

    fn fetch_or_spawn_daemon(&self) {
        let socket = self.config.socket_path.clone();
        let status_arc = self.status.clone();

        std::thread::spawn(move || {
            if let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() {
                rt.block_on(async {
                    let res = send_ipc_request(&socket, &IpcRequest::GetStatus).await;
                    if let Ok(IpcResponse::Status(st)) = res {
                        if let Ok(mut guard) = status_arc.lock() {
                            *guard = Some(st);
                        }
                    } else {
                        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("omywall"));
                        let _ = Command::new(exe).arg("daemon").spawn();
                    }
                });
            }
        });
    }

    fn send_request(&self, req: IpcRequest) {
        let socket = self.config.socket_path.clone();
        let status_arc = self.status.clone();

        std::thread::spawn(move || {
            if let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() {
                rt.block_on(async {
                    let resp = send_ipc_request(&socket, &req).await;
                    if let Ok(IpcResponse::Status(st)) = send_ipc_request(&socket, &IpcRequest::GetStatus).await {
                        if let Ok(mut guard) = status_arc.lock() {
                            *guard = Some(st);
                        }
                    }
                    if let Ok(IpcResponse::Err { message }) = resp {
                        eprintln!("{}", message);
                    }
                });
            }
        });
    }

    fn get_cached_texture(&mut self, ctx: &egui::Context, thumb_path: &Path) -> Option<&egui::TextureHandle> {
        if let Ok(mut guard) = UPDATED_THUMBS.lock() {
            if let Some(set) = guard.as_mut() {
                if set.remove(thumb_path) {
                    self.texture_cache.remove(thumb_path);
                }
            }
        }

        if self.texture_cache.len() > 500 {
            self.texture_cache.clear();
        }

        if self.texture_cache.contains_key(thumb_path) {
            return self.texture_cache.get(thumb_path).and_then(|t| t.as_ref());
        }

        let mut decoded_img = None;
        if let Ok(mut decoded) = DECODED_IMAGES.lock() {
            if let Some(map) = decoded.as_mut() {
                decoded_img = map.remove(thumb_path);
            }
        }

        if let Some(color_img) = decoded_img {
            let name = thumb_path.to_string_lossy().to_string();
            let tex = ctx.load_texture(name, color_img, egui::TextureOptions::LINEAR);
            self.texture_cache.insert(thumb_path.to_path_buf(), Some(tex));
            return self.texture_cache.get(thumb_path).and_then(|t| t.as_ref());
        }

        request_background_image_decode(ctx.clone(), thumb_path.to_path_buf());
        ctx.request_repaint_after(std::time::Duration::from_millis(30));
        None
    }
}

fn minimize_gui_window(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));

    std::thread::spawn(|| {
        let _ = Command::new("hyprctl")
            .args(["dispatch", "movetoworkspacesilent", "special:omywall,title:OMYWALL"])
            .output();
        let _ = Command::new("hyprctl")
            .args(["dispatch", "minimize"])
            .output();
        let _ = Command::new("swaymsg")
            .args(["[title=\"OMYWALL\"]", "move", "scratchpad"])
            .output();
        let _ = Command::new("xdotool")
            .args(["search", "--class", "omywall", "windowminimize"])
            .output();
        let _ = Command::new("wmctrl")
            .args(["-r", "OMYWALL", "-b", "add,hidden"])
            .output();
    });
}

impl eframe::App for OmywallGuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.textures_loaded_this_frame = 0;
        if self.last_poll_instant.elapsed() > std::time::Duration::from_millis(2500) {
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
        let is_paused = current_status.as_ref().map(|st| st.is_paused).unwrap_or(false);

        if self.selected_wallpaper.is_none() {
            if let Some(ref wall) = current_wall {
                self.selected_wallpaper = Some(PathBuf::from(wall));
            }
        }

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
                            let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("omywall"));
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

                    if ui.button(egui::RichText::new("📥 Minimize to Tray").color(egui::Color32::from_rgb(0, 240, 255)).strong().small()).clicked() {
                        minimize_gui_window(ctx);
                        pending_msg = Some("Minimized OMYWALL window to system tray".into());
                    }
                    if ui.selectable_label(self.show_inspector, "🔍 Media Inspector").clicked() {
                        self.show_inspector = !self.show_inspector;
                    }
                    if ui.selectable_label(self.show_hyprlock, "🔒 Screensaver").clicked() {
                        self.show_hyprlock = !self.show_hyprlock;
                    }
                    if ui.selectable_label(self.show_gpu_settings, "🎮 GPU Hardware").clicked() {
                        self.show_gpu_settings = !self.show_gpu_settings;
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

        if self.show_gpu_settings {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.heading(
                            egui::RichText::new("🎮 GPU Hardware Acceleration & Graphics Selector")
                                .color(egui::Color32::from_rgb(0, 240, 255))
                                .strong(),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("❌ Close").clicked() {
                                self.show_gpu_settings = false;
                            }
                        });
                    });
                    ui.add_space(6.0);

                    let gpus = crate::config::detect_system_gpus();

                    ui.label(egui::RichText::new("🔍 Detected System Graphics Processing Units (GPUs):").strong().color(egui::Color32::from_rgb(255, 190, 50)));
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(10.0, 6.0);
                        for gpu in &gpus {
                            let (badge_color, border_color) = match gpu.vendor.as_str() {
                                "NVIDIA" => (egui::Color32::from_rgb(10, 40, 20), egui::Color32::from_rgb(118, 185, 0)),
                                "AMD" => (egui::Color32::from_rgb(40, 10, 15), egui::Color32::from_rgb(237, 28, 36)),
                                "Intel" => (egui::Color32::from_rgb(10, 25, 45), egui::Color32::from_rgb(0, 199, 255)),
                                _ => (egui::Color32::from_rgb(25, 28, 40), egui::Color32::from_rgb(140, 155, 180)),
                            };

                            egui::Frame::none()
                                .fill(badge_color)
                                .stroke(egui::Stroke::new(1.2, border_color))
                                .rounding(6.0)
                                .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(&gpu.vendor).strong().color(border_color));
                                        ui.label(egui::RichText::new(&gpu.name).strong().size(12.0));
                                        if let Some(ref dev) = gpu.device_path {
                                            ui.label(egui::RichText::new(format!("[{}]", dev)).color(egui::Color32::from_rgb(180, 195, 215)).small());
                                        }
                                    });
                                });
                        }
                    });

                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(8.0);

                    ui.columns(2, |cols| {
                        cols[0].vertical(|ui| {
                            ui.label(egui::RichText::new("⚡ Hardware Video Decoder Driver:").strong().color(egui::Color32::from_rgb(0, 240, 255)));
                            ui.add_space(4.0);
                            let curr_hwdec = self.config.hwdec.clone();
                            for &(mode_id, mode_label, desc) in &[
                                ("auto", "⚡ Auto-Detect GPU (Recommended)", "Automatic hardware acceleration detection"),
                                ("nvdec", "💚 NVIDIA NVDEC", "NVIDIA NVDEC Hardware Video Decoder"),
                                ("cuda", "⚡ NVIDIA CUDA Acceleration", "NVIDIA CUDA Hardware Video Acceleration"),
                                ("vaapi", "🔷 VA-API (Intel / AMD GPU)", "Linux VA-API Hardware Video Acceleration"),
                                ("vulkan", "🌋 Vulkan Video", "Modern Vulkan Hardware Video Decoder"),
                                ("no", "⚙️ CPU (Software Only)", "Software video decoding using CPU cores"),
                            ] {
                                if ui.radio(curr_hwdec == mode_id, mode_label).on_hover_text(desc).clicked() {
                                    self.config.hwdec = mode_id.to_string();
                                    let _ = self.config.save();
                                    pending_action = Some(IpcRequest::SetHwdec { hwdec: mode_id.to_string() });
                                    pending_msg = Some(format!("Hardware acceleration decoder set to {}", mode_label));
                                }
                            }
                        });

                        cols[1].vertical(|ui| {
                            ui.label(egui::RichText::new("🎯 Target GPU Render Node:").strong().color(egui::Color32::from_rgb(0, 255, 160)));
                            ui.add_space(4.0);
                            let curr_dev = self.config.gpu_device.clone();

                            if ui.radio(curr_dev.is_none(), "Auto-Select GPU (Default)").clicked() {
                                self.config.gpu_device = None;
                                let _ = self.config.save();
                                pending_action = Some(IpcRequest::SetGpuDevice { gpu_device: None });
                                pending_msg = Some("GPU target set to Auto-Select".into());
                            }

                            for gpu in &gpus {
                                if let Some(ref dev) = gpu.device_path {
                                    let is_sel = curr_dev.as_ref() == Some(dev);
                                    if ui.radio(is_sel, format!("{} ({})", gpu.vendor, dev)).clicked() {
                                        self.config.gpu_device = Some(dev.clone());
                                        let _ = self.config.save();
                                        pending_action = Some(IpcRequest::SetGpuDevice { gpu_device: Some(dev.clone()) });
                                        pending_msg = Some(format!("GPU target set to {}", dev));
                                    }
                                }
                            }
                        });
                    });

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);

                    ui.label(egui::RichText::new("🎞️ Target Frame Rate (FPS) Limit & Refresh Rate Control:").strong().color(egui::Color32::from_rgb(255, 180, 50)));
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Quick Presets:").small().color(egui::Color32::from_rgb(170, 185, 205)));
                        for &fps in &[30, 60, 90, 120, 144, 240] {
                            let label = format!("{} FPS", fps);
                            let is_curr = self.config.target_fps == fps;
                            if ui.selectable_label(is_curr, label).clicked() {
                                self.config.target_fps = fps;
                                let _ = self.config.save();
                                pending_action = Some(IpcRequest::SetTargetFps { fps });
                                pending_msg = Some(format!("Target rendering FPS set to {} FPS", fps));
                            }
                        }
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Custom FPS Limit:").small());
                        let mut fps_val = self.config.target_fps;
                        if ui.add(egui::Slider::new(&mut fps_val, 15..=240).suffix(" FPS")).changed() {
                            self.config.target_fps = fps_val;
                            let _ = self.config.save();
                            pending_action = Some(IpcRequest::SetTargetFps { fps: fps_val });
                            pending_msg = Some(format!("Target rendering FPS set to {} FPS", fps_val));
                        }
                    });
                });
            });
        }



        if self.show_inspector {
            egui::SidePanel::right("control_inspector_panel")
                .resizable(true)
                .default_width(310.0)
                .min_width(260.0)
                .max_width(450.0)
                .show(ctx, |ui| {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.heading(
                            egui::RichText::new("🔍 Media Control Inspector")
                                .color(egui::Color32::from_rgb(0, 240, 255))
                                .strong(),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("❌").clicked() {
                                self.show_inspector = false;
                            }
                        });
                    });
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(6.0);

                    if let Some(ref sel_path) = self.selected_wallpaper.clone() {
                        let filename = sel_path.file_name().and_then(|n| n.to_str()).unwrap_or("Unknown");
                        let path_str = sel_path.to_string_lossy().to_string();
                        let ext = sel_path.extension().and_then(|e| e.to_str()).unwrap_or("").to_uppercase();
                        let is_active = current_wall.as_ref().map(|curr| Path::new(curr) == sel_path).unwrap_or(false);

                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("🖥 Live Inspection Preview").strong().color(egui::Color32::from_rgb(255, 190, 50)));
                            if is_active {
                                ui.label(egui::RichText::new("● LIVE NOW").color(egui::Color32::from_rgb(0, 255, 150)).small().strong());
                            } else {
                                ui.label(egui::RichText::new("⏹ INACTIVE").color(egui::Color32::from_rgb(140, 155, 180)).small());
                            }
                        });
                        ui.add_space(4.0);

                        egui::Frame::none()
                            .fill(egui::Color32::from_rgb(14, 18, 28))
                            .stroke(egui::Stroke::new(1.5, egui::Color32::from_rgb(0, 225, 255)))
                            .rounding(8.0)
                            .inner_margin(egui::Margin::same(6.0))
                            .show(ui, |ui| {
                                ui.vertical_centered(|ui| {
                                    if let Some(thumb_path) = get_web_thumbnail_path(&path_str) {
                                        if let Some(tex) = self.get_cached_texture(ctx, &thumb_path) {
                                            ui.add(egui::Image::new(tex).max_size(egui::vec2(280.0, 150.0)).rounding(6.0));
                                        } else {
                                            ui.set_height(140.0);
                                            ui.centered_and_justified(|ui| {
                                                ui.label(egui::RichText::new("⏳ Rendering Preview...").color(egui::Color32::from_rgb(0, 200, 255)).small());
                                            });
                                        }
                                    } else {
                                        ui.set_height(140.0);
                                        ui.centered_and_justified(|ui| {
                                            ui.label(egui::RichText::new("⏳ Generating Preview...").color(egui::Color32::from_rgb(0, 200, 255)).small());
                                        });
                                    }
                                });
                            });

                        ui.add_space(8.0);
                        ui.group(|ui| {
                            ui.label(egui::RichText::new("📋 Media Attributes").strong().small());
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("Title:").strong().small());
                                ui.label(egui::RichText::new(filename).color(egui::Color32::from_rgb(0, 240, 255)).small().strong());
                            });
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("Format:").strong().small());
                                ui.label(egui::RichText::new(&ext).color(egui::Color32::from_rgb(0, 255, 160)).small().strong());
                                ui.label(egui::RichText::new("Size:").strong().small());
                                ui.label(egui::RichText::new(get_file_size_str(sel_path)).color(egui::Color32::from_rgb(180, 195, 215)).small());
                            });
                            ui.label(egui::RichText::new(format!("Path: {}", path_str)).color(egui::Color32::from_rgb(130, 145, 170)).small());
                        });

                        ui.add_space(8.0);
                        ui.group(|ui| {
                            ui.label(egui::RichText::new("🎛 Active Playback Controls").strong().small());
                            ui.add_space(4.0);

                            ui.horizontal(|ui| {
                                if ui.button(egui::RichText::new("▶ Apply to Current").color(egui::Color32::from_rgb(0, 255, 150)).strong()).clicked() {
                                    pending_action = Some(IpcRequest::SetWallpaper { path: path_str.clone() });
                                    pending_msg = Some(format!("Applied wallpaper: {}", filename));
                                }
                                let pause_label = if is_paused { "▶ Resume" } else { "⏸ Pause" };
                                if ui.button(pause_label).clicked() {
                                    pending_action = Some(IpcRequest::TogglePause);
                                    pending_msg = Some("Toggled playback pause state".into());
                                }
                                if ui.button(egui::RichText::new("🛑 Stop").color(egui::Color32::from_rgb(255, 90, 90))).clicked() {
                                    pending_action = Some(IpcRequest::StopWallpaper);
                                    pending_msg = Some("Stopped active wallpaper engine".into());
                                }
                            });

                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.label("🔊 Vol:");
                                if ui.add(egui::Slider::new(&mut self.volume_slider, 0..=100).suffix("%")).changed() {
                                    pending_action = Some(IpcRequest::SetVolume { volume: self.volume_slider });
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("✨ Opacity:");
                                if ui.add(egui::Slider::new(&mut self.opacity_slider, 0.0..=1.0)).changed() {
                                    pending_action = Some(IpcRequest::SetOpacity { opacity: self.opacity_slider });
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("🎞 Target FPS:");
                                let mut fps_val = self.config.target_fps;
                                if ui.add(egui::Slider::new(&mut fps_val, 15..=240).suffix(" FPS")).changed() {
                                    self.config.target_fps = fps_val;
                                    let _ = self.config.save();
                                    pending_action = Some(IpcRequest::SetTargetFps { fps: fps_val });
                                    pending_msg = Some(format!("Target rendering FPS set to {} FPS", fps_val));
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("⚡ Decoder:");
                                let mut selected_hwdec = self.config.hwdec.clone();
                                egui::ComboBox::from_id_salt("hwdec_selector_inspector")
                                    .selected_text(match selected_hwdec.as_str() {
                                        "nvdec" => "💚 NVIDIA NVDEC",
                                        "nvdec-copy" => "💚 NVIDIA NVDEC Copy",
                                        "cuda" => "⚡ NVIDIA CUDA",
                                        "cuda-copy" => "⚡ NVIDIA CUDA Copy",
                                        "vaapi" => "🔷 VA-API (Intel/AMD)",
                                        "vaapi-copy" => "🔷 VA-API Copy",
                                        "vulkan" => "🌋 Vulkan Video",
                                        "vdpau" => "🎞 VDPAU (Legacy)",
                                        "no" => "⚙️ CPU Software Only",
                                        _ => "⚡ Auto-Detect GPU",
                                    })
                                    .show_ui(ui, |ui| {
                                        for &(val, label) in &[
                                            ("auto", "⚡ Auto-Detect GPU"),
                                            ("nvdec", "💚 NVIDIA NVDEC"),
                                            ("nvdec-copy", "💚 NVIDIA NVDEC Copy"),
                                            ("cuda", "⚡ NVIDIA CUDA Transcoder"),
                                            ("cuda-copy", "⚡ NVIDIA CUDA Copy"),
                                            ("vaapi", "🔷 VA-API (Intel/AMD)"),
                                            ("vaapi-copy", "🔷 VA-API Copy"),
                                            ("vulkan", "🌋 Vulkan Video"),
                                            ("vdpau", "🎞 VDPAU Legacy"),
                                            ("no", "⚙️ CPU Software Only"),
                                        ] {
                                            if ui.selectable_value(&mut selected_hwdec, val.to_string(), label).clicked() {
                                                self.config.hwdec = val.to_string();
                                                let _ = self.config.save();
                                                pending_action = Some(IpcRequest::SetHwdec { hwdec: val.to_string() });
                                                pending_msg = Some(format!("Hardware decoder set to {}", label));
                                            }
                                        }
                                    });
                            });
                        });
                    } else {
                        ui.vertical_centered(|ui| {
                            ui.add_space(40.0);
                            ui.label(egui::RichText::new("👆 Select Any Wallpaper").color(egui::Color32::from_rgb(0, 240, 255)).strong());
                            ui.label(egui::RichText::new("Click card in gallery to inspect parameters").color(egui::Color32::from_rgb(140, 160, 190)).small());
                        });
                    }
                });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
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

            if self.show_hyprlock {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.heading(
                            egui::RichText::new("🔒 Hyprlock Screensaver Configurator & Live Preview")
                                .color(egui::Color32::from_rgb(0, 240, 255))
                                .strong(),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("❌ Close").clicked() {
                                self.show_hyprlock = false;
                            }
                            if ui.button(egui::RichText::new("⚡ Test Screensaver Now").color(egui::Color32::from_rgb(0, 255, 160)).strong().small()).clicked() {
                                let _ = self.config.save_hyprlock_conf(current_wall.as_deref());
                                let _ = Command::new("hyprlock").spawn();
                                pending_msg = Some("Launched Hyprlock screensaver test".into());
                            }
                            if ui.button(egui::RichText::new("💾 Save hyprlock.conf").color(egui::Color32::from_rgb(0, 220, 255)).strong().small()).clicked() {
                                match self.config.save_hyprlock_conf(current_wall.as_deref()) {
                                    Ok(path) => {
                                        let _ = self.config.save();
                                        pending_msg = Some(format!("Saved screensaver config to {}", path.display()));
                                    }
                                    Err(e) => {
                                        pending_msg = Some(format!("Error saving hyprlock.conf: {}", e));
                                    }
                                }
                            }
                        });
                    });
                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(6.0);

                    ui.columns(2, |cols| {
                        // COLUMN 1: CONFIGURATION CONTROLS
                        cols[0].vertical(|ui| {
                            ui.label(egui::RichText::new("⚙ Screensaver Settings").strong().color(egui::Color32::from_rgb(255, 190, 50)));
                            ui.add_space(6.0);

                            ui.group(|ui| {
                                ui.label(egui::RichText::new("🖼 Background Source:").strong().small());
                                ui.horizontal(|ui| {
                                    let is_auto = self.config.hyprlock.background_path.is_empty();
                                    if ui.radio(is_auto, "Active Desktop Wallpaper").clicked() {
                                        self.config.hyprlock.background_path.clear();
                                    }
                                    if ui.radio(!is_auto, "Custom Image").clicked() {
                                        if let Some(file) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "webp"]).pick_file() {
                                            self.config.hyprlock.background_path = file.to_string_lossy().to_string();
                                        }
                                    }
                                });
                                if !self.config.hyprlock.background_path.is_empty() {
                                    ui.label(egui::RichText::new(format!("File: {}", self.config.hyprlock.background_path)).color(egui::Color32::from_rgb(140, 155, 175)).small());
                                }
                            });

                            ui.add_space(6.0);
                            ui.group(|ui| {
                                ui.label(egui::RichText::new("✨ Glass Blur Effects:").strong().small());
                                ui.add(egui::Slider::new(&mut self.config.hyprlock.blur_passes, 0..=6).prefix("Passes: "));
                                ui.add(egui::Slider::new(&mut self.config.hyprlock.blur_size, 0..=16).prefix("Blur Size: ").suffix("px"));
                            });

                            ui.add_space(6.0);
                            ui.group(|ui| {
                                ui.label(egui::RichText::new("⏰ Digital Clock & Typography:").strong().small());
                                ui.add(egui::Slider::new(&mut self.config.hyprlock.clock_size, 40..=120).prefix("Font Size: ").suffix("pt"));
                                ui.horizontal_wrapped(|ui| {
                                    ui.label("Color: ");
                                    for &(color_name, hex) in &[("Cyan", "#00f0ff"), ("Emerald", "#00ff9d"), ("Purple", "#b040ff"), ("Gold", "#ffc107"), ("White", "#ffffff"), ("Pink", "#ff007f")] {
                                        if ui.selectable_label(self.config.hyprlock.clock_color == hex, color_name).clicked() {
                                            self.config.hyprlock.clock_color = hex.to_string();
                                        }
                                    }
                                });
                            });

                            ui.add_space(6.0);
                            ui.group(|ui| {
                                ui.label(egui::RichText::new("💬 Welcome Message & Accents:").strong().small());
                                ui.horizontal(|ui| {
                                    ui.label("Text:");
                                    ui.add(egui::TextEdit::singleline(&mut self.config.hyprlock.welcome_message).hint_text("Welcome back, $USER!"));
                                });
                                ui.horizontal_wrapped(|ui| {
                                    ui.label("Ring Accent:");
                                    for &(color_name, hex) in &[("Cyan", "#00f0ff"), ("Purple", "#b040ff"), ("Emerald", "#00ff9d"), ("Red", "#ff4444"), ("Gold", "#ffc107")] {
                                        if ui.selectable_label(self.config.hyprlock.input_field_ring == hex, color_name).clicked() {
                                            self.config.hyprlock.input_field_ring = hex.to_string();
                                        }
                                    }
                                });
                            });
                        });

                        // COLUMN 2: LIVE INTERACTIVE PREVIEW
                        cols[1].vertical(|ui| {
                            ui.label(egui::RichText::new("🖥 Live Screensaver Mock-up Preview").strong().color(egui::Color32::from_rgb(0, 240, 255)));
                            ui.add_space(6.0);

                            let (time_str, date_str) = get_current_time_str();
                            let user_name = std::env::var("USER").unwrap_or_else(|_| "User".to_string());
                            let welcome_str = self.config.hyprlock.welcome_message.replace("$USER", &user_name);

                            let preview_bg = if self.config.hyprlock.blur_passes > 0 {
                                egui::Color32::from_rgb(10, 14, 24)
                            } else {
                                egui::Color32::from_rgb(18, 24, 38)
                            };

                            let ring_color = match self.config.hyprlock.input_field_ring.as_str() {
                                "#00f0ff" => egui::Color32::from_rgb(0, 240, 255),
                                "#b040ff" => egui::Color32::from_rgb(176, 64, 255),
                                "#00ff9d" => egui::Color32::from_rgb(0, 255, 157),
                                "#ff4444" => egui::Color32::from_rgb(255, 68, 68),
                                "#ffc107" => egui::Color32::from_rgb(255, 193, 7),
                                _ => egui::Color32::from_rgb(0, 240, 255),
                            };

                            let clock_col = match self.config.hyprlock.clock_color.as_str() {
                                "#00f0ff" => egui::Color32::from_rgb(0, 240, 255),
                                "#00ff9d" => egui::Color32::from_rgb(0, 255, 157),
                                "#b040ff" => egui::Color32::from_rgb(176, 64, 255),
                                "#ffc107" => egui::Color32::from_rgb(255, 193, 7),
                                "#ff007f" => egui::Color32::from_rgb(255, 0, 127),
                                _ => egui::Color32::from_rgb(255, 255, 255),
                            };

                            egui::Frame::none()
                                .fill(preview_bg)
                                .stroke(egui::Stroke::new(2.0, ring_color))
                                .rounding(12.0)
                                .inner_margin(egui::Margin::same(16.0))
                                .show(ui, |ui| {
                                    ui.set_height(260.0);
                                    ui.vertical_centered(|ui| {
                                        ui.add_space(15.0);

                                        // Render live clock text
                                        let font_sz = (self.config.hyprlock.clock_size as f32 * 0.45).clamp(24.0, 52.0);
                                        ui.label(egui::RichText::new(&time_str).color(clock_col).size(font_sz).strong());

                                        ui.add_space(4.0);
                                        ui.label(egui::RichText::new(&date_str).color(egui::Color32::from_rgb(200, 215, 235)).size(13.0));

                                        ui.add_space(10.0);
                                        ui.label(egui::RichText::new(&welcome_str).color(egui::Color32::from_rgb(240, 245, 255)).size(14.0).strong());

                                        ui.add_space(16.0);
                                        // Render password field box mock-up
                                        egui::Frame::none()
                                            .fill(egui::Color32::from_rgb(11, 13, 20))
                                            .stroke(egui::Stroke::new(2.0, ring_color))
                                            .rounding(20.0)
                                            .inner_margin(egui::Margin::symmetric(24.0, 8.0))
                                            .show(ui, |ui| {
                                                ui.label(egui::RichText::new("••••••••").color(egui::Color32::from_rgb(180, 195, 215)).size(14.0));
                                            });
                                    });
                                });
                        });
                    });
                });
                ui.add_space(10.0);
            }

            ui.group(|ui| {
                ui.horizontal(|ui| {
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

                if self.category_filter == CategoryFilter::WebWidgets {
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.heading(
                                egui::RichText::new("🌐 Interactive 3D Web & HTML5 Wallpapers")
                                    .color(egui::Color32::from_rgb(0, 240, 255))
                                    .strong(),
                            );
                            ui.label(egui::RichText::new("● Native WebGL Canvas Engine").color(egui::Color32::from_rgb(0, 255, 160)).small().strong())
                                .on_hover_text("Native GTK Layer Shell + WebKit2 wlr-layer-shell desktop background rendering");
                        });
                        ui.add_space(6.0);

                        let bookmarks = self.config.saved_web_wallpapers.clone();
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(12.0, 12.0);
                            for bm in &bookmarks {
                                let target_url = crate::config::resolve_asset_path(&bm.url);
                                let is_active = current_wall.as_ref().map(|curr| curr == &target_url).unwrap_or(false);
                                let is_selected = self.selected_wallpaper.as_ref().map(|p| p.to_string_lossy() == target_url).unwrap_or(false);

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

                                let frame_res = egui::Frame::none()
                                    .fill(card_bg)
                                    .stroke(card_stroke)
                                    .rounding(10.0)
                                    .inner_margin(egui::Margin::same(8.0))
                                    .show(ui, |ui| {
                                        ui.set_width(220.0);
                                        ui.set_height(175.0);
                                        ui.vertical_centered(|ui| {
                                            if let Some(thumb_path) = get_web_thumbnail_path(&target_url) {
                                                if let Some(tex) = self.get_cached_texture(ctx, &thumb_path) {
                                                    ui.add(egui::Image::new(tex).max_size(egui::vec2(204.0, 100.0)).rounding(6.0));
                                                } else {
                                                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                                                    egui::Frame::none()
                                                        .fill(egui::Color32::from_rgb(14, 18, 28))
                                                        .rounding(6.0)
                                                        .show(ui, |ui| {
                                                            ui.set_width(204.0);
                                                            ui.set_height(100.0);
                                                        });
                                                }
                                            } else {
                                                ctx.request_repaint_after(std::time::Duration::from_millis(100));
                                            }
                                            ui.add_space(4.0);
                                            ui.horizontal(|ui| {
                                                ui.label(egui::RichText::new(format!("[{}]", bm.category)).color(egui::Color32::from_rgb(0, 240, 255)).strong().small());
                                                ui.add(egui::Label::new(egui::RichText::new(&bm.title).strong().small()).truncate());
                                            });
                                            ui.add_space(4.0);
                                            ui.horizontal(|ui| {
                                                if ui.button(egui::RichText::new("▶ Launch").color(egui::Color32::from_rgb(0, 255, 150)).strong().small()).clicked() {
                                                    self.selected_wallpaper = Some(PathBuf::from(&target_url));
                                                    self.show_inspector = true;
                                                    pending_action = Some(IpcRequest::SetWallpaper { path: target_url.clone() });
                                                    pending_msg = Some(format!("Launched 3D web wallpaper: {}", bm.title));
                                                }
                                                if ui.button(egui::RichText::new("👁 Preview").color(egui::Color32::from_rgb(0, 220, 255)).small()).clicked() {
                                                    self.selected_wallpaper = Some(PathBuf::from(&target_url));
                                                    self.show_inspector = true;
                                                }
                                                if !bm.is_demo {
                                                    if ui.button("🗑").clicked() {
                                                        self.config.remove_web_bookmark(&bm.url);
                                                    }
                                                }
                                            });
                                        });
                                    });

                                let interact = frame_res.response.interact(egui::Sense::click());
                                if interact.double_clicked() {
                                    self.selected_wallpaper = Some(PathBuf::from(&target_url));
                                    self.show_inspector = true;
                                    pending_action = Some(IpcRequest::SetWallpaper { path: target_url.clone() });
                                    pending_msg = Some(format!("Launched 3D web wallpaper: {}", bm.title));
                                } else if interact.clicked() {
                                    self.selected_wallpaper = Some(PathBuf::from(&target_url));
                                    self.show_inspector = true;
                                }
                            }
                        });

                        ui.add_space(12.0);
                        ui.separator();
                        ui.add_space(8.0);

                        ui.label(egui::RichText::new("➕ Save Custom Animated Website or HTML File").strong().color(egui::Color32::from_rgb(255, 190, 50)));
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut self.new_web_title).hint_text("Title (e.g. Matrix Canvas)"));
                            ui.add(egui::TextEdit::singleline(&mut self.web_url_input).hint_text("URL or Path (https://...)"));
                            if ui.button(egui::RichText::new("💾 Save Website").color(egui::Color32::from_rgb(0, 240, 255)).strong()).clicked() {
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

                        if self.view_mode == ViewMode::Grid {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
                                for path in filtered {
                                    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("Unknown");
                                    let path_str = path.to_string_lossy().to_string();
                                    let is_active = current_wall.as_ref().map(|curr| Path::new(curr) == path).unwrap_or(false);
                                    let is_selected = self.selected_wallpaper.as_ref() == Some(path);

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

                                    let badge_color = match ext.as_str() {
                                        "MKV" => egui::Color32::from_rgb(255, 120, 0),
                                        "MP4" => egui::Color32::from_rgb(0, 180, 240),
                                        "GIF" => egui::Color32::from_rgb(220, 100, 255),
                                        "HTML" | "JS" => egui::Color32::from_rgb(255, 200, 0),
                                        _ => egui::Color32::from_rgb(150, 165, 185),
                                    };

                                    let frame_res = egui::Frame::none()
                                        .fill(card_bg)
                                        .stroke(card_stroke)
                                        .rounding(8.0)
                                        .inner_margin(egui::Margin::same(8.0))
                                        .show(ui, |ui| {
                                            ui.set_width(210.0);
                                            ui.set_height(160.0);
                                            ui.vertical_centered(|ui| {
                                                if let Some(thumb_path) = get_web_thumbnail_path(&path_str) {
                                                    if let Some(tex) = self.get_cached_texture(ctx, &thumb_path) {
                                                        ui.add(egui::Image::new(tex).max_size(egui::vec2(194.0, 95.0)).rounding(4.0));
                                                    } else {
                                                        ctx.request_repaint_after(std::time::Duration::from_millis(150));
                                                        egui::Frame::none()
                                                            .fill(egui::Color32::from_rgb(14, 18, 28))
                                                            .rounding(4.0)
                                                            .show(ui, |ui| {
                                                                ui.set_width(194.0);
                                                                ui.set_height(95.0);
                                                            });
                                                    }
                                                } else {
                                                    ctx.request_repaint_after(std::time::Duration::from_millis(150));
                                                    egui::Frame::none()
                                                        .fill(egui::Color32::from_rgb(14, 18, 28))
                                                        .rounding(4.0)
                                                        .show(ui, |ui| {
                                                            ui.set_width(194.0);
                                                            ui.set_height(95.0);
                                                        });
                                                }
                                                ui.add_space(4.0);
                                                ui.horizontal(|ui| {
                                                    ui.label(egui::RichText::new(format!("[{}]", ext)).color(badge_color).strong().small());
                                                    ui.add(egui::Label::new(egui::RichText::new(filename).strong().small()).truncate());
                                                });
                                                ui.add_space(4.0);
                                                ui.horizontal(|ui| {
                                                    if ui.button(egui::RichText::new("▶ Apply").color(egui::Color32::from_rgb(0, 255, 150)).strong().small()).clicked() {
                                                        self.selected_wallpaper = Some(path.clone());
                                                        self.show_inspector = true;
                                                        pending_action = Some(IpcRequest::SetWallpaper { path: path_str.clone() });
                                                        pending_msg = Some(format!("Applied wallpaper: {}", filename));
                                                    }
                                                    if ui.button(egui::RichText::new("👁 Preview").color(egui::Color32::from_rgb(0, 220, 255)).small()).clicked() {
                                                        self.selected_wallpaper = Some(path.clone());
                                                        self.show_inspector = true;
                                                    }
                                                    if is_active {
                                                        ui.label(egui::RichText::new("● LIVE").color(egui::Color32::from_rgb(255, 190, 50)).small().strong());
                                                    }
                                                });
                                            });
                                        });

                                    let card_interact = frame_res.response.interact(egui::Sense::click());
                                    if card_interact.double_clicked() {
                                        self.selected_wallpaper = Some(path.clone());
                                        self.show_inspector = true;
                                        pending_action = Some(IpcRequest::SetWallpaper { path: path_str.clone() });
                                        pending_msg = Some(format!("Applied wallpaper: {}", filename));
                                    } else if card_interact.clicked() {
                                        self.selected_wallpaper = Some(path.clone());
                                        self.show_inspector = true;
                                    }
                                }
                            });
                        } else {
                            for path in filtered {
                                let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("Unknown");
                                let path_str = path.to_string_lossy().to_string();
                                let is_active = current_wall.as_ref().map(|curr| Path::new(curr) == path).unwrap_or(false);
                                let is_selected = self.selected_wallpaper.as_ref() == Some(path);

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

                                let badge_color = match ext.as_str() {
                                    "MKV" => egui::Color32::from_rgb(255, 120, 0),
                                    "MP4" => egui::Color32::from_rgb(0, 180, 240),
                                    "GIF" => egui::Color32::from_rgb(220, 100, 255),
                                    "HTML" | "JS" => egui::Color32::from_rgb(255, 200, 0),
                                    _ => egui::Color32::from_rgb(150, 165, 185),
                                };

                                let card_resp = egui::Frame::none()
                                    .fill(card_bg)
                                    .stroke(card_stroke)
                                    .rounding(6.0)
                                    .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            if let Some(thumb_path) = get_web_thumbnail_path(&path_str) {
                                                if let Some(tex) = self.get_cached_texture(ctx, &thumb_path) {
                                                    ui.add(egui::Image::new(tex).max_size(egui::vec2(60.0, 36.0)).rounding(4.0));
                                                } else {
                                                    ctx.request_repaint_after(std::time::Duration::from_millis(150));
                                                }
                                            } else {
                                                ctx.request_repaint_after(std::time::Duration::from_millis(150));
                                            }

                                            ui.label(egui::RichText::new(format!("[{}]", ext)).color(badge_color).strong().small());

                                            if is_active {
                                                ui.label(egui::RichText::new(filename).color(egui::Color32::from_rgb(0, 255, 150)).strong());
                                                ui.label(egui::RichText::new("● LIVE").color(egui::Color32::from_rgb(255, 190, 50)).small().strong());
                                            } else {
                                                ui.label(filename);
                                            }

                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                if ui.button(egui::RichText::new("▶ Apply").color(egui::Color32::from_rgb(0, 255, 150)).strong().small()).clicked() {
                                                    self.selected_wallpaper = Some(path.clone());
                                                    self.show_inspector = true;
                                                    pending_action = Some(IpcRequest::SetWallpaper { path: path_str.clone() });
                                                    pending_msg = Some(format!("Applied wallpaper: {}", filename));
                                                }
                                                if ui.button(egui::RichText::new("👁 Preview").color(egui::Color32::from_rgb(0, 220, 255)).small()).clicked() {
                                                    self.selected_wallpaper = Some(path.clone());
                                                    self.show_inspector = true;
                                                }
                                            });
                                        });
                                    })
                                    .response;

                                let interact = card_resp.interact(egui::Sense::click());
                                if interact.double_clicked() {
                                    self.selected_wallpaper = Some(path.clone());
                                    self.show_inspector = true;
                                    pending_action = Some(IpcRequest::SetWallpaper { path: path_str.clone() });
                                    pending_msg = Some(format!("Applied wallpaper: {}", filename));
                                } else if interact.clicked() {
                                    self.selected_wallpaper = Some(path.clone());
                                    self.show_inspector = true;
                                }
                                ui.add_space(4.0);
                            }
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
            });

        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }
}
