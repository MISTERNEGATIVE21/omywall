use eframe::egui;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use crate::config::Config;
use crate::ipc::{send_ipc_request, DaemonStatus, IpcRequest, IpcResponse};
use crate::logger::get_log_path;

static PENDING_THUMBS: Mutex<Option<HashSet<String>>> = Mutex::new(None);
static DECODED_IMAGES: Mutex<Option<HashMap<PathBuf, egui::ColorImage>>> = Mutex::new(None);
static PENDING_DECODES: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);
static UPDATED_THUMBS: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);
static WORKSHOP_BROWSE_RESULT: Mutex<Option<Result<Vec<crate::steam_workshop::WorkshopItem>, String>>> = Mutex::new(None);
static WORKSHOP_DL_RESULT: Mutex<Option<Result<PathBuf, String>>> = Mutex::new(None);
static STEAM_SCAN_RESULT: Mutex<Option<Vec<crate::steam_scanner::SteamWallpaper>>> = Mutex::new(None);
static LWE_PROPS_RESULT: Mutex<Option<Result<Vec<crate::lwe::WallpaperProperty>, String>>> = Mutex::new(None);
static DISPLAY_SCAN_RESULT: Mutex<Option<Vec<crate::display::DisplayInfo>>> = Mutex::new(None);
static GIF_ANIM_CACHE: Mutex<Option<HashMap<PathBuf, Option<Arc<GifAnim>>>>> = Mutex::new(None);
static PENDING_GIF_DECODES: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);

struct GifAnim {
    frames: Vec<egui::TextureHandle>,
    frame_delay_ms: u64,
}

pub fn notify_thumb_updated(path: PathBuf) {
    if let Ok(mut guard) = DECODED_IMAGES.lock() {
        if let Some(ref mut map) = *guard {
            map.remove(&path);
        }
    }
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
        let path_str = path.to_string_lossy();
        let is_remote = path_str.starts_with("http://") || path_str.starts_with("https://");

        let loaded = if is_remote {
            let cache_dir = PathBuf::from("/tmp/omywall_thumbs");
            let _ = std::fs::create_dir_all(&cache_dir);
            let hash = format!("{:x}", md5_hash(path_str.as_bytes()));
            let cached_file = cache_dir.join(format!("remote_{}.jpg", &hash[..12]));

            if cached_file.exists() {
                image::open(&cached_file)
            } else if let Ok(client) = reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(15)).build() {
                if let Ok(resp) = client.get(path_str.as_ref()).send() {
                    if let Ok(bytes) = resp.bytes() {
                        if !bytes.is_empty() {
                            let _ = std::fs::write(&cached_file, &bytes);
                            image::load_from_memory(&bytes)
                        } else {
                            Err(image::ImageError::IoError(std::io::Error::new(std::io::ErrorKind::NotFound, "empty response")))
                        }
                    } else {
                        Err(image::ImageError::IoError(std::io::Error::new(std::io::ErrorKind::NotFound, "read bytes failed")))
                    }
                } else {
                    Err(image::ImageError::IoError(std::io::Error::new(std::io::ErrorKind::NotFound, "http get failed")))
                }
            } else {
                Err(image::ImageError::IoError(std::io::Error::new(std::io::ErrorKind::NotFound, "client init failed")))
            }
        } else {
            image::open(&path)
        };

        let is_ok = loaded.is_ok();
        let is_temp_thumb = path_str.starts_with("/tmp/omywall_thumbs") || path_str.starts_with("/tmp/omywall_workshop_thumbs");

        let color_img = match loaded {
            Ok(img) => {
                let resized = img.thumbnail(384, 216);
                let rgba = resized.to_rgba8();
                let pixels = rgba.as_raw().clone();
                egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], &pixels)
            }
            Err(_) => {
                let width = 320u32;
                let height = 180u32;
                let mut imgbuf = image::RgbImage::new(width, height);
                for (x, y, pixel) in imgbuf.enumerate_pixels_mut() {
                    let factor = (x + y) as f32 / (width + height) as f32;
                    let r = (15.0 * (1.0 - factor) + 8.0 * factor) as u8;
                    let g = (20.0 * (1.0 - factor) + 12.0 * factor) as u8;
                    *pixel = image::Rgb([r, g, (35.0 * (1.0 - factor) + 25.0 * factor) as u8]);
                }
                for x in 4..=315 {
                    imgbuf.put_pixel(x, 4, image::Rgb([0, 240, 255]));
                    imgbuf.put_pixel(x, 175, image::Rgb([0, 240, 255]));
                }
                for y in 4..=175 {
                    imgbuf.put_pixel(4, y, image::Rgb([0, 240, 255]));
                    imgbuf.put_pixel(315, y, image::Rgb([0, 240, 255]));
                }
                let rgba: image::RgbaImage = image::DynamicImage::ImageRgb8(imgbuf).to_rgba8();
                let pixels = rgba.as_raw().clone();
                egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &pixels)
            }
        };

        if is_ok || !is_temp_thumb {
            if let Ok(mut decoded) = DECODED_IMAGES.lock() {
                let map = decoded.get_or_insert_with(HashMap::new);
                if map.len() > 120 {
                    map.clear();
                }
                map.insert(path.clone(), color_img);
            }
        }
        ctx.request_repaint();

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

pub fn run_gui(config: Config, start_minimized: bool) -> Result<(), eframe::Error> {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("OMYWALL Wallpaper Engine v4.5")
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
        "OMYWALL Wallpaper Engine v4.5",
        options,
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            let _ = GLOBAL_EGUI_CTX.set(cc.egui_ctx.clone());
            Ok(Box::new(OmywallGuiApp::new(config, start_minimized)))
        }),
    )
}

static GLOBAL_EGUI_CTX: std::sync::OnceLock<egui::Context> = std::sync::OnceLock::new();

pub fn global_egui_ctx() -> Option<&'static egui::Context> {
    GLOBAL_EGUI_CTX.get()
}

#[derive(PartialEq, Clone, Copy, Debug)]
enum AppTab {
    Installed,
    SteamWorkshop,
    Displays,
    Settings,
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
    Carousel,
    List,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ThemeScheme {
    DarkGlass,
    SteamAmber,
    HardLightCyber,
    OledPitchBlack,
}

impl Default for ThemeScheme {
    fn default() -> Self {
        ThemeScheme::DarkGlass
    }
}

impl ThemeScheme {
    pub fn name(&self) -> &'static str {
        match self {
            ThemeScheme::DarkGlass => "🌌 Dark Glass",
            ThemeScheme::SteamAmber => "🔥 Steam Amber",
            ThemeScheme::HardLightCyber => "⚡ Cyber Light",
            ThemeScheme::OledPitchBlack => "🖤 OLED Black",
        }
    }

    pub fn apply(&self, ctx: &egui::Context) {
        let mut style = (*ctx.style()).clone();
        style.visuals.dark_mode = true;

        match self {
            ThemeScheme::DarkGlass => {
                style.visuals.override_text_color = Some(egui::Color32::from_rgb(235, 242, 255));
                style.visuals.panel_fill = egui::Color32::from_rgba_unmultiplied(10, 12, 20, 240);
                style.visuals.window_fill = egui::Color32::from_rgba_unmultiplied(17, 21, 35, 245);
                style.visuals.window_rounding = egui::Rounding::same(14.0);
                style.visuals.window_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(0, 240, 255, 60));

                style.visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgba_unmultiplied(22, 27, 44, 200);
                style.visuals.widgets.noninteractive.rounding = egui::Rounding::same(10.0);
                style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 20));

                style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgba_unmultiplied(26, 32, 52, 210);
                style.visuals.widgets.inactive.rounding = egui::Rounding::same(10.0);
                style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 30));

                style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgba_unmultiplied(35, 45, 75, 240);
                style.visuals.widgets.hovered.rounding = egui::Rounding::same(10.0);
                style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.5, egui::Color32::from_rgb(0, 240, 255));

                style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(0, 200, 220);
                style.visuals.widgets.active.rounding = egui::Rounding::same(10.0);
                style.visuals.widgets.active.bg_stroke = egui::Stroke::new(2.0, egui::Color32::from_rgb(0, 255, 160));

                style.visuals.selection.bg_fill = egui::Color32::from_rgb(0, 150, 220);
            }
            ThemeScheme::SteamAmber => {
                style.visuals.override_text_color = Some(egui::Color32::from_rgb(240, 246, 255));
                style.visuals.panel_fill = egui::Color32::from_rgba_unmultiplied(11, 20, 29, 245);
                style.visuals.window_fill = egui::Color32::from_rgba_unmultiplied(18, 30, 44, 250);
                style.visuals.window_rounding = egui::Rounding::same(12.0);
                style.visuals.window_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(102, 192, 244));

                style.visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgba_unmultiplied(23, 38, 54, 220);
                style.visuals.widgets.noninteractive.rounding = egui::Rounding::same(8.0);
                style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(45, 70, 95));

                style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgba_unmultiplied(27, 44, 63, 230);
                style.visuals.widgets.inactive.rounding = egui::Rounding::same(8.0);
                style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(60, 90, 120));

                style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(42, 68, 96);
                style.visuals.widgets.hovered.rounding = egui::Rounding::same(8.0);
                style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.5, egui::Color32::from_rgb(255, 153, 0));

                style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(255, 153, 0);
                style.visuals.widgets.active.rounding = egui::Rounding::same(8.0);
                style.visuals.widgets.active.bg_stroke = egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 204, 0));

                style.visuals.selection.bg_fill = egui::Color32::from_rgb(102, 192, 244);
            }
            ThemeScheme::HardLightCyber => {
                style.visuals.override_text_color = Some(egui::Color32::from_rgb(250, 250, 255));
                style.visuals.panel_fill = egui::Color32::from_rgb(8, 8, 14);
                style.visuals.window_fill = egui::Color32::from_rgb(15, 15, 24);
                style.visuals.window_rounding = egui::Rounding::same(10.0);
                style.visuals.window_stroke = egui::Stroke::new(1.5, egui::Color32::from_rgb(255, 0, 85));

                style.visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(20, 20, 32);
                style.visuals.widgets.noninteractive.rounding = egui::Rounding::same(6.0);
                style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(45, 45, 70));

                style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(25, 25, 40);
                style.visuals.widgets.inactive.rounding = egui::Rounding::same(6.0);
                style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(60, 60, 90));

                style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(40, 30, 60);
                style.visuals.widgets.hovered.rounding = egui::Rounding::same(6.0);
                style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.5, egui::Color32::from_rgb(0, 255, 157));

                style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(0, 255, 157);
                style.visuals.widgets.active.rounding = egui::Rounding::same(6.0);
                style.visuals.widgets.active.bg_stroke = egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 0, 150));

                style.visuals.selection.bg_fill = egui::Color32::from_rgb(255, 0, 85);
            }
            ThemeScheme::OledPitchBlack => {
                style.visuals.override_text_color = Some(egui::Color32::from_rgb(255, 255, 255));
                style.visuals.panel_fill = egui::Color32::from_rgb(0, 0, 0);
                style.visuals.window_fill = egui::Color32::from_rgb(10, 10, 10);
                style.visuals.window_rounding = egui::Rounding::same(12.0);
                style.visuals.window_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(30, 30, 30));

                style.visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(14, 14, 14);
                style.visuals.widgets.noninteractive.rounding = egui::Rounding::same(8.0);
                style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(28, 28, 28));

                style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(18, 18, 18);
                style.visuals.widgets.inactive.rounding = egui::Rounding::same(8.0);
                style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(40, 40, 40));

                style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(30, 30, 30);
                style.visuals.widgets.hovered.rounding = egui::Rounding::same(8.0);
                style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.5, egui::Color32::from_rgb(0, 240, 255));

                style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(0, 240, 255);
                style.visuals.widgets.active.rounding = egui::Rounding::same(8.0);
                style.visuals.widgets.active.bg_stroke = egui::Stroke::new(2.0, egui::Color32::from_rgb(0, 255, 160));

                style.visuals.selection.bg_fill = egui::Color32::from_rgb(0, 240, 255);
            }
        }

        ctx.set_style(style);
    }
}

struct OmywallGuiApp {
    config: Config,
    status: Arc<Mutex<Option<DaemonStatus>>>,
    active_tab: AppTab,
    theme_scheme: ThemeScheme,
    wallpapers: Vec<PathBuf>,
    selected_wallpaper: Option<PathBuf>,
    steam_wallpapers: Vec<crate::steam_scanner::SteamWallpaper>,
    selected_steam_wallpaper: Option<crate::steam_scanner::SteamWallpaper>,
    displays: Vec<crate::display::DisplayInfo>,
    selected_screen: Option<String>,
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
    textures_loaded_this_frame: usize,
    texture_cache: HashMap<PathBuf, Option<egui::TextureHandle>>,
    last_poll_instant: std::time::Instant,
    card_hover: HashMap<PathBuf, bool>,

    // Steam Workshop browse state
    workshop_items: Vec<crate::steam_workshop::WorkshopItem>,
    workshop_selected: Option<crate::steam_workshop::WorkshopItem>,
    workshop_page: u32,
    workshop_sort: String,
    workshop_days: i64,
    workshop_loading: bool,
    workshop_status: String,
    workshop_downloading: Option<String>,

    // linux-wallpaperengine tuning state
    tuning_wall: Option<crate::steam_scanner::SteamWallpaper>,
    tuning_overrides: crate::config::WallpaperOverrides,
    lwe_props: Vec<crate::lwe::WallpaperProperty>,
    lwe_prop_values: HashMap<String, String>,
    lwe_props_busy: bool,
    lwe_props_status: String,

    // Real-time System Telemetry
    system_metrics: crate::config::SystemMetrics,
    last_metrics_poll: std::time::Instant,

    // Floating Picture-in-Picture (PiP) Live Preview
    pip_active: bool,
    pip_target: Option<PathBuf>,

    // Minimization & Window Pinning State
    start_minimized: bool,
    minimized_on_launch_done: bool,
    is_pinned_on_top: bool,
}

#[derive(Clone)]
struct ToolStatus {
    name: String,
    description: String,
    installed: bool,
}

fn check_tool_installed(cmd: &str) -> bool {
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(cmd);
            if candidate.exists() {
                return true;
            }
        }
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

fn fmt_f64(v: f64) -> String {
    let rounded = (v * 100000.0).round() / 100000.0;
    format!("{}", rounded)
}

fn parse_color_value(value: &str) -> [f32; 4] {
    let mut rgba = [1.0_f32; 4];
    let mut idx = 0usize;
    for part in value.split(|c: char| c == ',' || c.is_whitespace()) {
        if part.is_empty() || idx >= 4 {
            continue;
        }
        if let Ok(v) = part.parse::<f32>() {
            rgba[idx] = v.clamp(0.0, 1.0);
            idx += 1;
        }
    }
    rgba
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

fn get_thumbnail_path(ctx: Option<egui::Context>, video_path: &Path) -> Option<PathBuf> {
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
        let ctx = ctx.clone();

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
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
    }

    Some(thumb_file)
}

fn get_gif_preview_path(ctx: Option<egui::Context>, video_path: &Path) -> Option<PathBuf> {
    let cache_dir = PathBuf::from("/tmp/omywall_thumbs");
    let _ = std::fs::create_dir_all(&cache_dir);

    let ext = video_path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if !matches!(ext.as_str(), "mp4" | "mkv" | "webm" | "avi" | "mov" | "gif" | "flv" | "m4v" | "wmv") {
        return None;
    }

    let key = video_path.to_string_lossy().to_string();
    let stem = video_path.file_stem().and_then(|s| s.to_str()).unwrap_or("clip");
    let hash = format!("{:x}", md5_hash(key.as_bytes()));
    let gif_file = cache_dir.join(format!("{}_{}.gif", stem, &hash[..8]));

    if gif_file.exists() {
        return Some(gif_file);
    }

    let gif_key = format!("{}::gif", key);
    if !is_thumb_pending(&gif_key) {
        set_thumb_pending(&gif_key, true);
        let input_str = key.clone();
        let out_str = gif_file.to_string_lossy().to_string();
        let part_str = format!("{}.part", out_str);
        let ctx = ctx.clone();

        std::thread::spawn(move || {
            let vf = "fps=12,scale=320:180:flags=lanczos,split[s0][s1];[s0]palettegen=max_colors=128[p];[s1][p]paletteuse=dither=bayer:bayer_scale=3";
            let ok = Command::new("ffmpeg")
                .args(["-y", "-hide_banner", "-loglevel", "error", "-t", "4", "-i", &input_str, "-vf", vf, "-f", "gif", &part_str])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);

            if ok && Path::new(&part_str).exists() {
                let _ = std::fs::rename(&part_str, &out_str);
            } else {
                let _ = std::fs::remove_file(&part_str);
            }

            set_thumb_pending(&format!("{}::gif", input_str), false);
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
    }

    Some(gif_file)
}

fn decode_gif_frames(path: &Path) -> Vec<egui::ColorImage> {
    let mut out = Vec::new();
    let Ok(file) = std::fs::File::open(path) else {
        return out;
    };
    let Ok(decoder) = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(file)) else {
        return out;
    };
    for frame in image::AnimationDecoder::into_frames(decoder) {
        if out.len() >= 120 {
            break;
        }
        let Ok(frame) = frame else {
            break;
        };
        let rgba = frame.into_buffer();
        let pixels = rgba.as_raw().to_vec();
        out.push(egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], &pixels));
    }
    out
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

pub fn get_web_thumbnail_path(ctx: Option<egui::Context>, target: &str) -> Option<PathBuf> {
    let resolved = crate::config::resolve_asset_path(target);
    let path = Path::new(&resolved);
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();

    if matches!(ext.as_str(), "mp4" | "mkv" | "webm" | "avi" | "mov" | "gif" | "png" | "jpg" | "jpeg" | "webp") {
        return get_thumbnail_path(ctx, path);
    }

    let cache_dir = PathBuf::from("/tmp/omywall_thumbs");
    let _ = std::fs::create_dir_all(&cache_dir);

    let key = resolved.clone();
    let hash = format!("{:x}", md5_hash(key.as_bytes()));
    let thumb_file = cache_dir.join(format!("web_{}.jpg", &hash[..8]));

    if !thumb_file.exists() {
        generate_web_fallback_image(&thumb_file);
        notify_thumb_updated(thumb_file.clone());
        request_web_thumbnail_render(ctx, &resolved, &thumb_file);
    }

    Some(thumb_file)
}

fn request_web_thumbnail_render(ctx: Option<egui::Context>, url: &str, out_file: &Path) {
    if let Some(renderer) = crate::webkit_render::global_renderer() {
        renderer.render_thumbnail(url, out_file);
        return;
    }

    let py_runner = PathBuf::from("/tmp/omywall_web_thumb.py");
    let code = r#"import sys, os
import gi
gi.require_version('Gtk', '3.0')
gi.require_version('WebKit2', '4.1')
from gi.repository import Gtk, WebKit2, GLib

url = sys.argv[1]
out = sys.argv[2]

if not (url.startswith('http://') or url.startswith('https://') or url.startswith('file://') or url.startswith('data:')):
    url = 'file://' + os.path.abspath(url)

win = Gtk.Window(Gtk.WindowType.TOPLEVEL)
win.set_default_size(640, 360)
win.set_decorated(False)
win.set_opacity(0.0)
win.move(-10000, -10000)

webview = WebKit2.WebView()
webview.connect('load-failed', lambda *args: True)
settings = webview.get_settings()
settings.set_enable_webgl(True)
settings.set_enable_media_stream(True)
settings.set_enable_mediasource(True)
settings.set_media_playback_requires_user_gesture(False)
settings.set_allow_file_access_from_file_urls(True)
webview.load_uri(url)
win.add(webview)
win.show_all()

def done(webview, res):
    try:
        surface = webview.get_snapshot_finish(res)
        if surface is not None:
            surface.write_to_png(out)
    except Exception as e:
        sys.stderr.write(str(e) + "\n")
    Gtk.main_quit()
    return True

def snap():
    webview.get_snapshot(WebKit2.SnapshotRegion.FULL_DOCUMENT, WebKit2.SnapshotOptions.NONE, None, done)
    return False

def on_load(webview, event):
    if event == WebKit2.LoadEvent.FINISHED:
        GLib.timeout_add(600, snap)
    return True

webview.connect('load-changed', on_load)
GLib.timeout_add(9000, Gtk.main_quit)
Gtk.main()
"#;
    let _ = std::fs::write(&py_runner, code);

    let out_str = out_file.to_string_lossy().to_string();
    let url_owned = url.to_string();
    let ctx_clone = ctx.clone();
    std::thread::spawn(move || {
        let _ = Command::new("python3")
            .args([py_runner.to_string_lossy().as_ref(), &url_owned, &out_str])
            .env("WEBKIT_FORCE_COMPOSITING_MODE", "1")
            .env("__NV_PRIME_RENDER_OFFLOAD", "1")
            .env("__GLX_VENDOR_LIBRARY_NAME", "nvidia")
            .output();
        if Path::new(&out_str).exists() {
            notify_thumb_updated(PathBuf::from(&out_str));
            if let Some(c) = ctx_clone {
                c.request_repaint();
            }
        }
    });
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

impl OmywallGuiApp {
    pub fn new(config: Config, start_minimized: bool) -> Self {
        let wallpapers = Self::scan_wallpapers(&config.wallpaper_dir);
        let selected_wallpaper = wallpapers.first().cloned();
        let steam_wallpapers = Vec::new();
        let selected_steam_wallpaper = None;
        let displays = Vec::new();
        let selected_screen = None;
        let status = Arc::new(Mutex::new(None));
        let autostart_enabled = Config::is_autostart_enabled();

        let app = Self {
            config: config.clone(),
            status,
            active_tab: AppTab::Installed,
            theme_scheme: ThemeScheme::DarkGlass,
            wallpapers,
            selected_wallpaper,
            steam_wallpapers,
            selected_steam_wallpaper,
            displays,
            selected_screen,
            search_filter: String::new(),
            category_filter: CategoryFilter::All,
            view_mode: ViewMode::Carousel,
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
            textures_loaded_this_frame: 0,
            texture_cache: HashMap::new(),
            last_poll_instant: std::time::Instant::now(),
            card_hover: HashMap::new(),
            workshop_items: Vec::new(),
            workshop_selected: None,
            workshop_page: 1,
            workshop_sort: "trend".to_string(),
            workshop_days: 7,
            workshop_loading: false,
            workshop_status: String::new(),
            workshop_downloading: None,
            tuning_wall: None,
            tuning_overrides: crate::config::WallpaperOverrides::default(),
            lwe_props: Vec::new(),
            lwe_prop_values: HashMap::new(),
            lwe_props_busy: false,
            lwe_props_status: String::new(),
            system_metrics: crate::config::get_system_metrics(),
            last_metrics_poll: std::time::Instant::now(),
            pip_active: false,
            pip_target: None,
            start_minimized,
            minimized_on_launch_done: false,
            is_pinned_on_top: false,
        };

        app.fetch_or_spawn_daemon();
        app.spawn_steam_scan();
        app.spawn_display_scan();
        app
    }

    fn scan_wallpapers(dir: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let mut seen = HashSet::new();
        let valid_exts = ["mkv", "mp4", "webm", "avi", "mov", "gif", "html", "htm", "js", "pkg", "m4v", "flv", "wmv", "png", "jpg", "jpeg", "webp"];

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

    fn spawn_steam_scan(&self) {
        std::thread::spawn(|| {
            let result = crate::steam_scanner::scan_steam_wallpapers();
            if let Ok(mut g) = STEAM_SCAN_RESULT.lock() {
                *g = Some(result);
            }
        });
    }

    fn spawn_display_scan(&self) {
        std::thread::spawn(|| {
            let result = crate::display::detect_displays();
            if let Ok(mut g) = DISPLAY_SCAN_RESULT.lock() {
                *g = Some(result);
            }
        });
    }

    fn drain_background_scans(&mut self) {
        if let Ok(mut g) = STEAM_SCAN_RESULT.lock() {
            if let Some(result) = g.take() {
                self.steam_wallpapers = result;
            }
        }
        if let Ok(mut g) = DISPLAY_SCAN_RESULT.lock() {
            if let Some(result) = g.take() {
                if self.selected_screen.is_none() {
                    self.selected_screen = result.first().map(|d| d.name.clone());
                }
                self.displays = result;
            }
        }
    }

    fn begin_lwe_props_query(&mut self, ctx: &egui::Context, path: PathBuf) {
        self.lwe_props_busy = true;
        self.lwe_props_status = "Querying wallpaper properties…".to_string();
        if let Ok(mut g) = LWE_PROPS_RESULT.lock() {
            *g = None;
        }
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let res = crate::lwe::list_properties(&path);
            if let Ok(mut g) = LWE_PROPS_RESULT.lock() {
                *g = Some(res);
            }
            ctx2.request_repaint();
        });
    }

    fn poll_lwe_props(&mut self) {
        if !self.lwe_props_busy {
            return;
        }
        let result = LWE_PROPS_RESULT.lock().ok().and_then(|mut g| g.take());
        if let Some(result) = result {
            self.lwe_props_busy = false;
            match result {
                Ok(props) => {
                    for p in &props {
                        self.lwe_prop_values
                            .entry(p.name.clone())
                            .or_insert_with(|| p.value.clone());
                    }
                    self.lwe_props = props;
                    self.lwe_props_status = format!("Loaded {} tunable properties", self.lwe_props.len());
                }
                Err(e) => {
                    self.lwe_props_status = format!("Property query failed: {}", e);
                }
            }
        }
    }

    fn build_steam_overrides(&self, wall_path: &Path) -> crate::config::WallpaperOverrides {
        let tuning_this = self.tuning_wall.as_ref().map(|t| t.path == wall_path).unwrap_or(false);
        let mut ov = if tuning_this {
            self.tuning_overrides.clone()
        } else {
            let key = wall_path.to_string_lossy().to_string();
            self.config.wallpaper_overrides.get(&key).cloned().unwrap_or_default()
        };
        if tuning_this {
            ov.custom_properties = self.lwe_prop_values.clone();
        }
        ov
    }

    fn ui_steam_tuning_panel(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        pending_action: &mut Option<IpcRequest>,
        pending_msg: &mut Option<String>,
    ) {
        let Some(tun) = self.tuning_wall.clone() else {
            return;
        };
        let tun_path = tun.path.clone();
        let tun_title = tun.title.clone();

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("⚙ Tuning").strong().color(egui::Color32::from_rgb(0, 240, 255)).size(15.0));
            ui.label(egui::RichText::new(&tun_title).strong().size(14.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(egui::RichText::new("✖ Close").small()).clicked() {
                    self.tuning_wall = None;
                    self.lwe_props.clear();
                    self.lwe_prop_values.clear();
                    self.lwe_props_status.clear();
                }
            });
        });
        ui.add_space(4.0);

        egui::CollapsingHeader::new("Basic Scene Options")
            .default_open(true)
            .show(ui, |ui| {
                egui::Grid::new("lwe_basic_grid")
                    .num_columns(2)
                    .spacing(egui::vec2(16.0, 8.0))
                    .show(ui, |ui| {
                        ui.label("FPS Limit");
                        let mut fps = self.tuning_overrides.fps.unwrap_or(60);
                        if ui.add(egui::Slider::new(&mut fps, 1..=240).suffix(" FPS")).changed() {
                            self.tuning_overrides.fps = Some(fps);
                        }
                        ui.end_row();

                        ui.label("Volume");
                        let mut vol = self.tuning_overrides.volume.unwrap_or(self.volume_slider);
                        if ui.add(egui::Slider::new(&mut vol, 0..=150).suffix("%")).changed() {
                            self.tuning_overrides.volume = Some(vol);
                        }
                        ui.end_row();

                        ui.label("Scaling");
                        let mut scaling = self.tuning_overrides.scaling.clone().unwrap_or_else(|| "default".to_string());
                        egui::ComboBox::from_id_salt("lwe_scaling")
                            .selected_text(&scaling)
                            .show_ui(ui, |ui| {
                                for s in ["default", "stretch", "fit", "fill"] {
                                    ui.selectable_value(&mut scaling, s.to_string(), s);
                                }
                            });
                        if scaling != "default" {
                            self.tuning_overrides.scaling = Some(scaling);
                        } else {
                            self.tuning_overrides.scaling = None;
                        }
                        ui.end_row();

                        ui.label("Clamp");
                        let mut clamp = self.tuning_overrides.clamp.clone().unwrap_or_else(|| "default".to_string());
                        egui::ComboBox::from_id_salt("lwe_clamp")
                            .selected_text(&clamp)
                            .show_ui(ui, |ui| {
                                for s in ["default", "clamp", "border", "repeat"] {
                                    ui.selectable_value(&mut clamp, s.to_string(), s);
                                }
                            });
                        if clamp != "default" {
                            self.tuning_overrides.clamp = Some(clamp);
                        } else {
                            self.tuning_overrides.clamp = None;
                        }
                        ui.end_row();

                        ui.label("Layer");
                        let mut layer = self.tuning_overrides.layer.clone().unwrap_or_else(|| "bottom".to_string());
                        egui::ComboBox::from_id_salt("lwe_layer")
                            .selected_text(&layer)
                            .show_ui(ui, |ui| {
                                for s in ["background", "bottom", "top", "overlay"] {
                                    ui.selectable_value(&mut layer, s.to_string(), s);
                                }
                            });
                        self.tuning_overrides.layer = Some(layer);
                        ui.end_row();

                        ui.label("Screenshot (for pywal)");
                        let mut shot = self.tuning_overrides.screenshot.clone().unwrap_or_default();
                        if ui.text_edit_singleline(&mut shot).changed() {
                            if shot.trim().is_empty() {
                                self.tuning_overrides.screenshot = None;
                            } else {
                                self.tuning_overrides.screenshot = Some(shot.trim().to_string());
                            }
                        }
                        ui.end_row();
                    });
            });

        egui::CollapsingHeader::new("Audio & Performance")
            .default_open(false)
            .show(ui, |ui| {
                egui::Grid::new("lwe_audio_grid")
                    .num_columns(2)
                    .spacing(egui::vec2(16.0, 8.0))
                    .show(ui, |ui| {
                        let mut silent = self.tuning_overrides.silent.unwrap_or(self.config.mute);
                        if ui.checkbox(&mut silent, "Mute all wallpaper sound").changed() {
                            self.tuning_overrides.silent = Some(silent);
                        }
                        ui.end_row();

                        let mut noauto = self.tuning_overrides.no_automute.unwrap_or(false);
                        if ui.checkbox(&mut noauto, "No auto-mute (keep sound while other apps play)").changed() {
                            self.tuning_overrides.no_automute = Some(noauto);
                        }
                        ui.end_row();

                        let mut noap = self.tuning_overrides.no_audio_processing.unwrap_or(false);
                        if ui.checkbox(&mut noap, "Disable audio-reactive processing").changed() {
                            self.tuning_overrides.no_audio_processing = Some(noap);
                        }
                        ui.end_row();

                        let mut nofp = self.tuning_overrides.no_fullscreen_pause.unwrap_or(false);
                        if ui.checkbox(&mut nofp, "Never pause when apps go fullscreen").changed() {
                            self.tuning_overrides.no_fullscreen_pause = Some(nofp);
                        }
                        ui.end_row();

                        let mut fsoa = self.tuning_overrides.fullscreen_pause_only_active.unwrap_or(false);
                        if ui.checkbox(&mut fsoa, "Pause only when the fullscreen window is active").changed() {
                            self.tuning_overrides.fullscreen_pause_only_active = Some(fsoa);
                        }
                        ui.end_row();

                        let mut dm = self.tuning_overrides.disable_mouse.unwrap_or(false);
                        if ui.checkbox(&mut dm, "Disable mouse interaction").changed() {
                            self.tuning_overrides.disable_mouse = Some(dm);
                        }
                        ui.end_row();

                        let mut dp = self.tuning_overrides.disable_parallax.unwrap_or(false);
                        if ui.checkbox(&mut dp, "Disable parallax effect").changed() {
                            self.tuning_overrides.disable_parallax = Some(dp);
                        }
                        ui.end_row();

                        let mut dparts = self.tuning_overrides.disable_particles.unwrap_or(false);
                        if ui.checkbox(&mut dparts, "Disable particles").changed() {
                            self.tuning_overrides.disable_particles = Some(dparts);
                        }
                        ui.end_row();
                    });
            });

        egui::CollapsingHeader::new("Tunable Wallpaper Properties")
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("🔄 Query Properties").clicked() {
                        self.begin_lwe_props_query(ctx, tun_path.clone());
                    }
                    if !self.lwe_props_status.is_empty() {
                        ui.label(egui::RichText::new(&self.lwe_props_status).color(egui::Color32::from_rgb(140, 160, 190)).small());
                    }
                });
                ui.add_space(6.0);

                if self.lwe_props_busy {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Querying property definitions…");
                    });
                } else if !self.lwe_props.is_empty() {
                    let props = self.lwe_props.clone();
                    for prop in &props {
                        ui.horizontal(|ui| {
                            ui.set_min_width(260.0);
                            match prop.prop_type {
                                crate::lwe::PropertyType::Boolean => {
                                    let mut b = prop.value == "1" || prop.value.eq_ignore_ascii_case("true");
                                    if let Some(cur) = self.lwe_prop_values.get(&prop.name) {
                                        b = cur == "1" || cur.eq_ignore_ascii_case("true");
                                    }
                                    if ui.checkbox(&mut b, &prop.name).changed() {
                                        self.lwe_prop_values.insert(
                                            prop.name.clone(),
                                            if b { "1".to_string() } else { "0".to_string() },
                                        );
                                    }
                                }
                                crate::lwe::PropertyType::Slider => {
                                    let min = prop.min.unwrap_or(0.0);
                                    let max = prop.max.unwrap_or(100.0);
                                    let step = prop.step.unwrap_or(1.0);
                                    let mut val = prop.value.parse::<f64>().unwrap_or(min);
                                    if let Some(cur) = self.lwe_prop_values.get(&prop.name) {
                                        if let Ok(v) = cur.parse::<f64>() {
                                            val = v;
                                        }
                                    }
                                    ui.add(
                                        egui::Slider::new(&mut val, min..=max)
                                            .step_by(step)
                                            .text(&prop.name),
                                    );
                                    let stored = fmt_f64(val);
                                    if self.lwe_prop_values.get(&prop.name).map(|v| v != &stored).unwrap_or(true) {
                                        self.lwe_prop_values.insert(prop.name.clone(), stored);
                                    }
                                }
                                crate::lwe::PropertyType::Combolist => {
                                    let cur = self.lwe_prop_values.get(&prop.name).cloned().unwrap_or_else(|| prop.value.clone());
                                    let selected = prop.options.iter().position(|(_, stored)| stored == &cur).unwrap_or(0);
                                    let mut sel = selected;
                                    egui::ComboBox::from_id_salt(format!("lwe_prop_{}", prop.name))
                                        .selected_text(prop.options.get(selected).map(|(d, _)| d.as_str()).unwrap_or("…"))
                                        .show_ui(ui, |ui| {
                                            for (i, (label, _)) in prop.options.iter().enumerate() {
                                                ui.selectable_value(&mut sel, i, label.as_str());
                                            }
                                        });
                                    if sel != selected {
                                        if let Some((_, stored)) = prop.options.get(sel) {
                                            self.lwe_prop_values.insert(prop.name.clone(), stored.clone());
                                        }
                                    }
                                }
                                crate::lwe::PropertyType::Color => {
                                    let mut rgba = parse_color_value(
                                        self.lwe_prop_values.get(&prop.name).cloned().unwrap_or_else(|| prop.value.clone()).as_str(),
                                    );
                                    if ui.color_edit_button_rgba_unmultiplied(&mut rgba).changed() {
                                        self.lwe_prop_values.insert(
                                            prop.name.clone(),
                                            format!("{:.6}, {:.6}, {:.6}, {:.6}", rgba[0], rgba[1], rgba[2], rgba[3]),
                                        );
                                    }
                                    ui.label(&prop.name);
                                }
                                _ => {
                                    let mut text =
                                        self.lwe_prop_values.get(&prop.name).cloned().unwrap_or_else(|| prop.value.clone());
                                    if ui.text_edit_singleline(&mut text).changed() {
                                        self.lwe_prop_values.insert(prop.name.clone(), text);
                                    }
                                    ui.label(&prop.name);
                                }
                            }
                        });
                        if !prop.description.is_empty() {
                            ui.label(egui::RichText::new(&prop.description).color(egui::Color32::from_rgb(110, 125, 150)).small());
                        }
                        ui.add_space(4.0);
                    }
                } else {
                    ui.label(egui::RichText::new("Click “Query Properties” to load this wallpaper's tunable controls.").color(egui::Color32::from_rgb(140, 160, 190)).small());
                }
            });

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let save_btn = egui::Button::new(egui::RichText::new("💾 Save & Apply").color(egui::Color32::from_rgb(0, 12, 24)).strong())
                .fill(egui::Color32::from_rgb(0, 255, 150))
                .rounding(6.0);
            if ui.add(save_btn).clicked() {
                self.tuning_overrides.custom_properties = self.lwe_prop_values.clone();
                let key = tun_path.to_string_lossy().to_string();
                self.config.wallpaper_overrides.insert(key, self.tuning_overrides.clone());
                let _ = self.config.save();
                let overrides = self.build_steam_overrides(&tun_path);
                *pending_action = Some(IpcRequest::SetSteamWallpaper {
                    path: tun_path.to_string_lossy().to_string(),
                    screen: self.selected_screen.clone(),
                    overrides: Some(overrides),
                });
                *pending_msg = Some(format!("Saved overrides and applied Steam Wallpaper: {}", tun_title));
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

        if self.texture_cache.get(thumb_path).and_then(|t| t.as_ref()).is_some() {
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

    fn get_gif_animation(&self, ctx: &egui::Context, gif_path: &Path) -> Option<Arc<GifAnim>> {
        {
            if let Ok(guard) = GIF_ANIM_CACHE.lock() {
                if let Some(map) = guard.as_ref() {
                    if let Some(v) = map.get(gif_path) {
                        return v.clone();
                    }
                }
            }
        }

        {
            let mut guard = match PENDING_GIF_DECODES.lock() {
                Ok(g) => g,
                Err(_) => return None,
            };
            let set = guard.get_or_insert_with(HashSet::new);
            if set.contains(gif_path) {
                return None;
            }
            set.insert(gif_path.to_path_buf());
        }

        let path = gif_path.to_path_buf();
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let frames = decode_gif_frames(&path);
            let anim = if frames.is_empty() {
                None
            } else {
                let mut texs = Vec::with_capacity(frames.len());
                for (i, color_img) in frames.iter().enumerate() {
                    let name = format!("gif_anim_{}_{}", path.to_string_lossy(), i);
                    texs.push(ctx2.load_texture(name, color_img.clone(), egui::TextureOptions::LINEAR));
                }
                Some(Arc::new(GifAnim { frames: texs, frame_delay_ms: 83 }))
            };

            if let Ok(mut guard) = GIF_ANIM_CACHE.lock() {
                let map = guard.get_or_insert_with(HashMap::new);
                if map.len() > 16 {
                    map.clear();
                }
                map.insert(path.clone(), anim);
            }
            if let Ok(mut guard) = PENDING_GIF_DECODES.lock() {
                if let Some(set) = guard.as_mut() {
                    set.remove(&path);
                }
            }
            ctx2.request_repaint();
        });

        None
    }

    fn draw_asset_thumb(&mut self, ctx: &egui::Context, ui: &mut egui::Ui, path: &Path, hovered: bool, size: egui::Vec2) {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        let is_video = matches!(ext.as_str(), "mp4" | "mkv" | "webm" | "avi" | "mov" | "gif" | "flv" | "m4v" | "wmv");

        let is_selected = self.selected_wallpaper.as_ref() == Some(&path.to_path_buf());
        if (hovered || is_selected) && is_video {
            if let Some(gif_path) = get_gif_preview_path(Some(ctx.clone()), path) {
                if gif_path.exists() {
                    if let Some(anim) = self.get_gif_animation(ctx, &gif_path) {
                        let time = ui.input(|i| i.time);
                        let idx = ((time * 1000.0) / anim.frame_delay_ms as f64) as usize % anim.frames.len();
                        ui.add(egui::Image::new(&anim.frames[idx]).fit_to_exact_size(size).rounding(4.0));
                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(anim.frame_delay_ms.max(50)));
                        return;
                    }
                }
            }
            if let Some(video_thumb) = get_thumbnail_path(Some(ctx.clone()), path) {
                if let Some(tex) = self.get_cached_texture(ctx, &video_thumb) {
                    ui.add(egui::Image::new(tex).max_size(size).rounding(4.0));
                    return;
                }
            }
        }

        let path_str = path.to_string_lossy();
        if path_str.starts_with("http://") || path_str.starts_with("https://") || matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp") {
            if let Some(tex) = self.get_cached_texture(ctx, path) {
                ui.add(egui::Image::new(tex).max_size(size).rounding(4.0));
                return;
            }
        }

        if let Some(thumb_path) = get_web_thumbnail_path(Some(ctx.clone()), &path_str) {
            if let Some(tex) = self.get_cached_texture(ctx, &thumb_path) {
                ui.add(egui::Image::new(tex).max_size(size).rounding(4.0));
                return;
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        }

        egui::Frame::none()
            .fill(egui::Color32::from_rgb(14, 18, 28))
            .rounding(4.0)
            .show(ui, |ui| {
                ui.set_width(size.x);
                ui.set_height(size.y);
            });
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

fn toggle_always_on_top(ctx: &egui::Context, on_top: bool) {
    let level = if on_top { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal };
    ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
    std::thread::spawn(move || {
        if on_top {
            let _ = Command::new("hyprctl").args(["dispatch", "setfloating", "title:^(OMYWALL.*)$"]).output();
            let _ = Command::new("hyprctl").args(["dispatch", "pin", "title:^(OMYWALL.*)$"]).output();
        } else {
            let _ = Command::new("hyprctl").args(["dispatch", "unpin", "title:^(OMYWALL.*)$"]).output();
        }
    });
}

impl eframe::App for OmywallGuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.start_minimized && !self.minimized_on_launch_done {
            self.minimized_on_launch_done = true;
            minimize_gui_window(ctx);
        }

        self.textures_loaded_this_frame = 0;
        if self.last_poll_instant.elapsed() > std::time::Duration::from_millis(2500) {
            self.last_poll_instant = std::time::Instant::now();
            self.poll_daemon_status();
        }
        if self.last_metrics_poll.elapsed() > std::time::Duration::from_millis(1500) {
            self.last_metrics_poll = std::time::Instant::now();
            self.system_metrics = crate::config::get_system_metrics();
        }

        self.theme_scheme.apply(ctx);

        self.drain_background_scans();
        self.poll_lwe_props();

        let mut pending_action: Option<IpcRequest> = None;
        let mut pending_msg: Option<String> = None;

        if self.pip_active {
            if let Some(ref path) = self.pip_target.clone() {
                let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("Preview");
                let mut is_open = true;
                let mut user_closed = false;
                egui::Window::new(format!("📺 Live PiP Preview: {}", filename))
                    .open(&mut is_open)
                    .default_size(egui::vec2(380.0, 240.0))
                    .resizable(true)
                    .show(ctx, |ui| {
                        ui.vertical_centered(|ui| {
                            self.draw_asset_thumb(ctx, ui, path, true, egui::vec2(350.0, 175.0));
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                if ui.button(egui::RichText::new("▶ Apply Wallpaper").color(egui::Color32::from_rgb(0, 255, 150)).strong().small()).clicked() {
                                    pending_action = Some(IpcRequest::SetWallpaper { path: path.to_string_lossy().to_string() });
                                    pending_msg = Some(format!("Applied wallpaper: {}", filename));
                                }
                                if ui.button(egui::RichText::new("❌ Close").color(egui::Color32::from_rgb(255, 100, 100)).small()).clicked() {
                                    user_closed = true;
                                }
                            });
                        });
                    });
                if user_closed || !is_open {
                    self.pip_active = false;
                }
            }
        }

        let current_status = self.status.lock().ok().and_then(|s| s.clone());
        let current_wall = current_status.as_ref().and_then(|s| s.current_wallpaper.clone());
        let _is_paused = current_status.as_ref().map(|st| st.is_paused).unwrap_or(false);

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
                ui.label(egui::RichText::new("v4.5").color(egui::Color32::from_rgb(160, 100, 255)).small().strong());

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
                    let pin_text = if self.is_pinned_on_top { "📌 Pinned On Top" } else { "📌 Pin on Top" };
                    if ui.selectable_label(self.is_pinned_on_top, pin_text).on_hover_text("Pin/Float window on top (Cloudflare WARP client style) on Hyprland/Sway").clicked() {
                        self.is_pinned_on_top = !self.is_pinned_on_top;
                        toggle_always_on_top(ctx, self.is_pinned_on_top);
                        pending_msg = Some(if self.is_pinned_on_top { "Pinned OMYWALL on top" } else { "Unpinned OMYWALL" }.into());
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

                    egui::ComboBox::from_id_salt("theme_selector")
                        .selected_text(self.theme_scheme.name())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.theme_scheme, ThemeScheme::DarkGlass, ThemeScheme::DarkGlass.name());
                            ui.selectable_value(&mut self.theme_scheme, ThemeScheme::SteamAmber, ThemeScheme::SteamAmber.name());
                            ui.selectable_value(&mut self.theme_scheme, ThemeScheme::HardLightCyber, ThemeScheme::HardLightCyber.name());
                            ui.selectable_value(&mut self.theme_scheme, ThemeScheme::OledPitchBlack, ThemeScheme::OledPitchBlack.name());
                        });
                });
            });
            ui.add_space(6.0);
        });

        // 2. LEFT SIDEBAR NAVIGATION (Matching jagrat7/linux-wallpaper-engine side-bar.tsx)
        egui::SidePanel::left("left_sidebar_nav")
            .resizable(false)
            .exact_width(170.0)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.vertical_centered(|ui| {
                    ui.heading(
                        egui::RichText::new("🌌 OMYWALL")
                            .color(egui::Color32::from_rgb(0, 240, 255))
                            .size(17.0)
                            .strong(),
                    );
                    ui.label(egui::RichText::new("Wallpaper Engine").color(egui::Color32::from_rgb(140, 155, 180)).small());
                });
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(10.0);

                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;

                    let tab_btn = |ui: &mut egui::Ui, is_sel: bool, text: &str| {
                        ui.add_sized(
                            [ui.available_width(), 34.0],
                            egui::SelectableLabel::new(is_sel, egui::RichText::new(text).size(13.0).strong()),
                        )
                    };

                    if tab_btn(ui, self.active_tab == AppTab::Installed, "📥 Installed").clicked() {
                        self.active_tab = AppTab::Installed;
                    }
                    if tab_btn(ui, self.active_tab == AppTab::SteamWorkshop, "🛠 Workshop").clicked() {
                        self.active_tab = AppTab::SteamWorkshop;
                        self.spawn_steam_scan();
                    }
                    if tab_btn(ui, self.active_tab == AppTab::Displays, "📺 Displays").clicked() {
                        self.active_tab = AppTab::Displays;
                        self.spawn_display_scan();
                    }
                    if tab_btn(ui, self.active_tab == AppTab::Settings, "⚙ Settings").clicked() {
                        self.active_tab = AppTab::Settings;
                    }
                });

                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("v4.5").color(egui::Color32::from_rgb(120, 135, 160)).small().strong());
                    ui.label(egui::RichText::new("linux-wallpaperengine").color(egui::Color32::from_rgb(90, 105, 130)).small());
                });
            });

        // 3. BOTTOM STATUS BAR (Matching jagrat7/linux-wallpaper-engine bottom-status-bar.tsx)
        egui::TopBottomPanel::bottom("bottom_status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let disp_name = self.displays.first().map(|d| d.name.as_str()).unwrap_or("eDP-1");
                ui.label(egui::RichText::new(format!("📺 {}", disp_name)).color(egui::Color32::from_rgb(0, 240, 255)).small().strong());

                ui.separator();

                if let Some(ref wall) = current_wall {
                    let filename = Path::new(wall).file_name().and_then(|n| n.to_str()).unwrap_or(wall);
                    ui.label(egui::RichText::new("🟢").size(10.0));
                    ui.label(egui::RichText::new(filename).color(egui::Color32::from_rgb(0, 255, 150)).small().strong());
                } else {
                    ui.label(egui::RichText::new("⚪").size(10.0));
                    ui.label(egui::RichText::new("No active wallpaper").color(egui::Color32::from_rgb(140, 155, 180)).small());
                }

                ui.separator();
                let metrics = &self.system_metrics;
                let ram_pct = if metrics.ram_total_mb > 0 { (metrics.ram_used_mb as f32 / metrics.ram_total_mb as f32) * 100.0 } else { 0.0 };
                let ram_used_gb = metrics.ram_used_mb as f32 / 1024.0;
                let ram_total_gb = metrics.ram_total_mb as f32 / 1024.0;
                ui.label(egui::RichText::new(format!("💻 CPU: {:.1}%", metrics.cpu_usage)).color(egui::Color32::from_rgb(0, 240, 255)).small().strong());
                ui.separator();
                ui.label(egui::RichText::new(format!("🧠 RAM: {:.1}/{:.1}GB ({:.0}%)", ram_used_gb, ram_total_gb, ram_pct)).color(egui::Color32::from_rgb(0, 255, 160)).small().strong());
                ui.separator();
                ui.label(egui::RichText::new(format!("🎮 GPU: {:.0}%", metrics.gpu_usage)).color(egui::Color32::from_rgb(255, 190, 50)).small().strong());

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(egui::RichText::new("⏹ Stop").color(egui::Color32::from_rgb(255, 90, 90)).small().strong()).clicked() {
                        pending_action = Some(IpcRequest::StopWallpaper);
                        pending_msg = Some("Stopped wallpaper playback".into());
                    }

                    let mute_label = if self.config.mute { "🔇 Unmute" } else { "🔊 Mute" };
                    if ui.button(egui::RichText::new(mute_label).color(egui::Color32::from_rgb(0, 240, 255)).small()).clicked() {
                        let new_mute = !self.config.mute;
                        self.config.mute = new_mute;
                        let _ = self.config.save();
                        pending_action = Some(IpcRequest::SetMute { mute: new_mute });
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
                            let available_options = crate::config::get_available_hwdec_options();
                            for &(mode_id, mode_label, desc) in &available_options {
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




        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                match self.active_tab {
                    AppTab::Displays => {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.heading(
                                    egui::RichText::new("📺 Connected Monitors & Display Assignment")
                                        .color(egui::Color32::from_rgb(0, 240, 255))
                                        .strong(),
                                );
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button("🔄 Rescan Displays").clicked() {
                                        self.spawn_display_scan();
                                        pending_msg = Some("Scanning displays…".into());
                                    }
                                });
                            });
                            ui.add_space(8.0);
                            ui.separator();
                            ui.add_space(8.0);

                            ui.label(egui::RichText::new(format!("Detected {} Active Display Output(s):", self.displays.len())).strong().color(egui::Color32::from_rgb(255, 190, 50)));
                            ui.add_space(6.0);

                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(12.0, 10.0);
                                for disp in &self.displays {
                                    let is_selected = self.selected_screen.as_ref() == Some(&disp.name);
                                    let bg_color = if is_selected { egui::Color32::from_rgb(15, 35, 55) } else { egui::Color32::from_rgb(20, 24, 38) };
                                    let stroke_color = if is_selected { egui::Color32::from_rgb(0, 240, 255) } else { egui::Color32::from_rgb(50, 60, 85) };

                                    egui::Frame::none()
                                        .fill(bg_color)
                                        .stroke(egui::Stroke::new(1.5, stroke_color))
                                        .rounding(10.0)
                                        .inner_margin(egui::Margin::same(12.0))
                                        .show(ui, |ui| {
                                            ui.set_width(260.0);
                                            ui.vertical(|ui| {
                                                ui.horizontal(|ui| {
                                                    ui.label(egui::RichText::new(&disp.name).strong().size(15.0).color(egui::Color32::from_rgb(0, 240, 255)));
                                                    if disp.primary {
                                                        ui.label(egui::RichText::new("PRIMARY").color(egui::Color32::from_rgb(0, 255, 160)).small().strong());
                                                    }
                                                });
                                                ui.add_space(4.0);
                                                ui.label(egui::RichText::new(format!("Resolution: {}", disp.resolution)).small());
                                                ui.label(egui::RichText::new(format!("Refresh Rate: {} Hz", disp.refresh_rate)).small());
                                                ui.label(egui::RichText::new(format!("Position: ({}, {})", disp.x, disp.y)).color(egui::Color32::from_rgb(140, 155, 180)).small());

                                                ui.add_space(8.0);
                                                if ui.button(egui::RichText::new(if is_selected { "✓ Selected Target" } else { "Select Display" }).strong().small()).clicked() {
                                                    self.selected_screen = Some(disp.name.clone());
                                                }
                                            });
                                        });
                                }
                            });
                        });
                    }
                    AppTab::SteamWorkshop => {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.heading(
                                    egui::RichText::new("🛠 Steam Wallpaper Engine Workshop Catalog")
                                        .color(egui::Color32::from_rgb(0, 240, 255))
                                        .strong(),
                                );
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button("🔄 Rescan Steam Libraries").clicked() {
                                        self.spawn_steam_scan();
                                        pending_msg = Some("Scanning Steam libraries…".into());
                                    }
                                });
                            });
                            ui.add_space(8.0);
                            ui.separator();
                            ui.add_space(8.0);

                            if self.workshop_items.is_empty() && !self.workshop_loading {
                                self.workshop_loading = true;
                                self.workshop_status = "Loading Workshop catalog...".to_string();
                                let page = self.workshop_page;
                                let sort = self.workshop_sort.clone();
                                let days = self.workshop_days;
                                let ctx2 = ctx.clone();
                                std::thread::spawn(move || {
                                    let res = crate::steam_workshop::browse_workshop(page, &sort, days);
                                    if let Ok(mut g) = WORKSHOP_BROWSE_RESULT.lock() {
                                        *g = Some(res);
                                    }
                                    ctx2.request_repaint();
                                });
                            }

                            // Browse controls
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("Browse:").strong().color(egui::Color32::from_rgb(255, 190, 50)));
                                egui::ComboBox::from_id_salt("ws_sort_combo")
                                    .selected_text(&self.workshop_sort)
                                    .show_ui(ui, |ui| {
                                        for &(val, label) in &[("trend", "🔥 Trending"), ("top_rated", "⭐ Top Rated"), ("most_subscribed", "📥 Most Subscribed"), ("newest", "🆕 Newest")] {
                                            ui.selectable_value(&mut self.workshop_sort, val.to_string(), label);
                                        }
                                    });
                                egui::ComboBox::from_id_salt("ws_days_combo")
                                    .selected_text(format!("{} days", self.workshop_days))
                                    .show_ui(ui, |ui| {
                                        for d in [7i64, 30, 90, 365] {
                                            ui.selectable_value(&mut self.workshop_days, d, format!("{} days", d));
                                        }
                                    });
                                if ui.button("◀ Page").clicked() && self.workshop_page > 1 {
                                    self.workshop_page -= 1;
                                    self.workshop_loading = true;
                                    self.workshop_status = format!("Loading page {}...", self.workshop_page);
                                    let page = self.workshop_page;
                                    let sort = self.workshop_sort.clone();
                                    let days = self.workshop_days;
                                    let ctx2 = ctx.clone();
                                    std::thread::spawn(move || {
                                        let res = crate::steam_workshop::browse_workshop(page, &sort, days);
                                        if let Ok(mut g) = WORKSHOP_BROWSE_RESULT.lock() {
                                            *g = Some(res);
                                        }
                                        ctx2.request_repaint();
                                    });
                                }
                                ui.label(egui::RichText::new(format!("Page {}", self.workshop_page)).strong());
                                if ui.button("Page ▶").clicked() {
                                    self.workshop_page += 1;
                                    self.workshop_loading = true;
                                    self.workshop_status = format!("Loading page {}...", self.workshop_page);
                                    let page = self.workshop_page;
                                    let sort = self.workshop_sort.clone();
                                    let days = self.workshop_days;
                                    let ctx2 = ctx.clone();
                                    std::thread::spawn(move || {
                                        let res = crate::steam_workshop::browse_workshop(page, &sort, days);
                                        if let Ok(mut g) = WORKSHOP_BROWSE_RESULT.lock() {
                                            *g = Some(res);
                                        }
                                        ctx2.request_repaint();
                                    });
                                }
                                let browse_btn = egui::Button::new(egui::RichText::new("🔍 Search Workshop").strong())
                                    .fill(egui::Color32::from_rgb(0, 120, 255))
                                    .rounding(6.0);
                                if ui.add(browse_btn).clicked() {
                                    self.workshop_loading = true;
                                    self.workshop_status = "Loading Workshop catalog...".to_string();
                                    let page = self.workshop_page;
                                    let sort = self.workshop_sort.clone();
                                    let days = self.workshop_days;
                                    let ctx2 = ctx.clone();
                                    std::thread::spawn(move || {
                                        let res = crate::steam_workshop::browse_workshop(page, &sort, days);
                                        if let Ok(mut g) = WORKSHOP_BROWSE_RESULT.lock() {
                                            *g = Some(res);
                                        }
                                        ctx2.request_repaint();
                                    });
                                }
                                if self.workshop_loading {
                                    ui.label(egui::RichText::new("⏳ Loading...").color(egui::Color32::from_rgb(255, 190, 50)).small());
                                }
                            });

                            if !crate::steam_workshop::steamcmd_available() {
                                ui.add_space(4.0);
                                egui::Frame::none()
                                    .fill(egui::Color32::from_rgb(45, 25, 10))
                                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(255, 150, 0)))
                                    .rounding(6.0)
                                    .inner_margin(egui::Margin::same(8.0))
                                    .show(ui, |ui| {
                                        ui.label(egui::RichText::new("⚠ steamcmd not installed — anonymous downloads are unavailable. Install steamcmd (e.g. `sudo pacman -S steamcmd`) or use “Add to Library” to subscribe in the Steam client.").color(egui::Color32::from_rgb(255, 200, 120)).small());
                                    });
                            }

                            ui.add_space(6.0);

                            // Drain background browse result
                            if let Ok(mut g) = WORKSHOP_BROWSE_RESULT.lock() {
                                if let Some(res) = g.take() {
                                    self.workshop_loading = false;
                                    match res {
                                        Ok(items) => {
                                            self.workshop_items = items;
                                            self.workshop_status = format!("Found {} items", self.workshop_items.len());
                                        }
                                        Err(e) => {
                                            self.workshop_status = format!("Error: {}", e);
                                        }
                                    }
                                }
                            }

                            // Drain background download result
                            if let Ok(mut g) = WORKSHOP_DL_RESULT.lock() {
                                if let Some(res) = g.take() {
                                    self.workshop_downloading = None;
                                    match res {
                                        Ok(_path) => {
                                            self.workshop_status = "✅ Downloaded! Refresh Installed or rescan to use it.".to_string();
                                            self.spawn_steam_scan();
                                            pending_msg = Some("Workshop item downloaded successfully".into());
                                        }
                                        Err(e) => {
                                            self.workshop_status = format!("Download failed: {}", e);
                                        }
                                    }
                                }
                            }

                            if !self.workshop_status.is_empty() {
                                ui.label(egui::RichText::new(&self.workshop_status).color(egui::Color32::from_rgb(140, 200, 255)).small());
                                ui.add_space(4.0);
                            }

                            // Workshop browse result cards
                            if !self.workshop_items.is_empty() {
                                ui.label(egui::RichText::new("📦 Workshop Catalog:").strong().color(egui::Color32::from_rgb(255, 190, 50)));
                                ui.add_space(6.0);
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing = egui::vec2(12.0, 10.0);
                                    let items = self.workshop_items.clone();
                                    for item in &items {
                                        let is_sel = self.workshop_selected.as_ref().map(|s| s.id == item.id).unwrap_or(false);
                                        let is_dl = crate::steam_workshop::is_downloaded(&item.id);
                                        let is_downloading = self.workshop_downloading.as_ref() == Some(&item.id);
                                        let bg_color = if is_sel { egui::Color32::from_rgb(15, 35, 55) } else { egui::Color32::from_rgb(20, 24, 38) };
                                        let stroke_color = if is_sel { egui::Color32::from_rgb(0, 240, 255) } else { egui::Color32::from_rgb(50, 60, 85) };

                                        let thumb_cache = crate::steam_workshop::cached_preview_path(item);
                                        let has_thumb = thumb_cache.as_ref().map(|p| p.exists()).unwrap_or(false);
                                        if !has_thumb {
                                            crate::steam_workshop::request_preview_image(item);
                                        }

                                        let item_clone = item.clone();
                                        let sel_id = item.id.clone();
                                        let _ = egui::Frame::none()
                                            .fill(bg_color)
                                            .stroke(egui::Stroke::new(1.5, stroke_color))
                                            .rounding(10.0)
                                            .inner_margin(egui::Margin::same(10.0))
                                            .show(ui, |ui| {
                                                ui.set_width(230.0);
                                                ui.vertical(|ui| {
                                                    let mut rendered = false;
                                                    if has_thumb {
                                                        if let Some(p) = thumb_cache.clone() {
                                                            if let Some(tex) = self.get_cached_texture(ctx, &p) {
                                                                ui.add(egui::Image::new(tex).max_size(egui::vec2(210.0, 120.0)).rounding(6.0));
                                                                rendered = true;
                                                            }
                                                        }
                                                    }
                                                    if !rendered {
                                                        if let Some(ref url) = item.preview_url {
                                                            let p = PathBuf::from(url);
                                                            if let Some(tex) = self.get_cached_texture(ctx, &p) {
                                                                ui.add(egui::Image::new(tex).max_size(egui::vec2(210.0, 120.0)).rounding(6.0));
                                                                rendered = true;
                                                            }
                                                        }
                                                    }
                                                    if !rendered {
                                                        egui::Frame::none()
                                                            .fill(egui::Color32::from_rgb(14, 18, 28))
                                                            .rounding(6.0)
                                                            .show(ui, |ui| {
                                                                ui.set_width(210.0);
                                                                ui.set_height(120.0);
                                                                ui.centered_and_justified(|ui| {
                                                                    ui.label(egui::RichText::new("🎞").size(22.0).color(egui::Color32::from_rgb(0, 200, 255)));
                                                                });
                                                            });
                                                        ctx.request_repaint_after(std::time::Duration::from_millis(200));
                                                    }

                                                    ui.add_space(4.0);
                                                    ui.add(egui::Label::new(egui::RichText::new(&item.title).strong().size(13.0).color(egui::Color32::from_rgb(0, 240, 255))).truncate());
                                                    ui.label(egui::RichText::new(format!("By {}", item.author)).color(egui::Color32::from_rgb(140, 155, 180)).small());
                                                    ui.label(egui::RichText::new(format!("📥 {} subs  👁 {} views", item.subscriptions, item.views)).color(egui::Color32::from_rgb(180, 195, 215)).small());
                                                    if item.file_size > 0 {
                                                        ui.label(egui::RichText::new(format!("💾 {}", crate::steam_workshop::get_file_size_str(item.file_size))).color(egui::Color32::from_rgb(180, 195, 215)).small());
                                                    }
                                                    if let Some(tag) = item.tags.first() {
                                                        ui.label(egui::RichText::new(format!("🏷 {}", tag)).color(egui::Color32::from_rgb(0, 255, 160)).small());
                                                    }

                                                    ui.add_space(6.0);
                                                    ui.horizontal(|ui| {
                                                        if is_downloading {
                                                            ui.label(egui::RichText::new("⏳ Downloading...").color(egui::Color32::from_rgb(255, 190, 50)).small());
                                                        } else if is_dl {
                                                            let apply_btn = egui::Button::new(egui::RichText::new("▶ Apply").color(egui::Color32::from_rgb(0, 12, 24)).strong().small())
                                                                .fill(egui::Color32::from_rgb(0, 255, 150))
                                                                .rounding(6.0);
                                                            if ui.add(apply_btn).clicked() {
                                                                if let Some(dir) = crate::steam_workshop::downloaded_item_path(&item_clone.id) {
                                                                    pending_action = Some(IpcRequest::SetWallpaper { path: dir.to_string_lossy().to_string() });
                                                                    pending_msg = Some(format!("Applied Steam Workshop item: {}", item_clone.title));
                                                                }
                                                            }
                                                        } else {
                                                            let dl_btn = egui::Button::new(egui::RichText::new("⬇ Download").color(egui::Color32::from_rgb(0, 12, 24)).strong().small())
                                                                .fill(egui::Color32::from_rgb(0, 200, 255))
                                                                .rounding(6.0);
                                                            if ui.add(dl_btn).clicked() {
                                                                self.workshop_downloading = Some(item_clone.id.clone());
                                                                self.workshop_status = format!("Downloading {}...", item_clone.title);
                                                                let id = item_clone.id.clone();
                                                                let ctx2 = ctx.clone();
                                                                std::thread::spawn(move || {
                                                                    let res = crate::steam_workshop::download_workshop_item(&id);
                                                                    if let Ok(mut g) = WORKSHOP_DL_RESULT.lock() {
                                                                        *g = Some(res);
                                                                    }
                                                                    ctx2.request_repaint();
                                                                });
                                                            }
                                                        }
                                                        if ui.button(egui::RichText::new("📖 Add to Library").small()).clicked() {
                                                            crate::steam_workshop::open_in_browser(&sel_id);
                                                            pending_msg = Some(format!("Opened {} in browser — click Subscribe in Steam", item_clone.title));
                                                        }
                                                    });
                                                });
                                            });
                                    }
                                });
                            }

                            ui.add_space(10.0);
                            ui.separator();
                            ui.add_space(8.0);

                            if self.steam_wallpapers.is_empty() {
                                ui.vertical_centered(|ui| {
                                    ui.add_space(40.0);
                                    ui.label(egui::RichText::new("No Steam Wallpaper Engine items found.").color(egui::Color32::from_rgb(255, 180, 50)).strong());
                                    ui.label(egui::RichText::new("Ensure Steam is installed and Workshop wallpapers for App 431960 are downloaded.").color(egui::Color32::from_rgb(140, 160, 190)).small());
                                });
                            } else {
                                ui.label(egui::RichText::new(format!("Discovered {} Steam Wallpaper Engine Items:", self.steam_wallpapers.len())).strong().color(egui::Color32::from_rgb(255, 190, 50)));
                                ui.add_space(6.0);

                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing = egui::vec2(12.0, 10.0);
                                    let steam_walls = self.steam_wallpapers.clone();
                                    for wall in &steam_walls {
                                        let is_sel = self.selected_steam_wallpaper.as_ref().map(|s| s.id == wall.id).unwrap_or(false);
                                        let bg_color = if is_sel { egui::Color32::from_rgb(15, 35, 55) } else { egui::Color32::from_rgb(20, 24, 38) };
                                        let stroke_color = if is_sel { egui::Color32::from_rgb(0, 240, 255) } else { egui::Color32::from_rgb(50, 60, 85) };

                                        egui::Frame::none()
                                            .fill(bg_color)
                                            .stroke(egui::Stroke::new(1.5, stroke_color))
                                            .rounding(10.0)
                                            .inner_margin(egui::Margin::same(10.0))
                                            .show(ui, |ui| {
                                                ui.set_width(220.0);
                                                ui.vertical(|ui| {
                                                    ui.label(egui::RichText::new(&wall.title).strong().size(13.0).color(egui::Color32::from_rgb(0, 240, 255)));
                                                    ui.label(egui::RichText::new(format!("By {}", wall.author)).color(egui::Color32::from_rgb(140, 155, 180)).small());
                                                    ui.label(egui::RichText::new(format!("Type: {}", wall.wallpaper_type.as_str())).color(egui::Color32::from_rgb(0, 255, 160)).small().strong());

                                                    ui.add_space(6.0);
                                                    let apply_btn = egui::Button::new(
                                                        egui::RichText::new("▶ Apply Scene")
                                                            .color(egui::Color32::from_rgb(0, 12, 24))
                                                            .strong()
                                                            .small()
                                                    )
                                                    .fill(egui::Color32::from_rgb(0, 255, 150))
                                                    .rounding(6.0);

                                                    if ui.add(apply_btn).clicked() {
                                                        let overrides = self.build_steam_overrides(&wall.path);
                                                        pending_action = Some(IpcRequest::SetSteamWallpaper {
                                                            path: wall.path.to_string_lossy().to_string(),
                                                            screen: self.selected_screen.clone(),
                                                            overrides: Some(overrides),
                                                        });
                                                        pending_msg = Some(format!("Applied Steam Wallpaper: {}", wall.title));
                                                    }

                                                    let tune_btn = egui::Button::new(
                                                        egui::RichText::new("⚙ Tune")
                                                            .color(egui::Color32::from_rgb(0, 240, 255))
                                                            .strong()
                                                            .small()
                                                    )
                                                    .fill(egui::Color32::from_rgb(12, 30, 50))
                                                    .rounding(6.0);

                                                    if ui.add(tune_btn).clicked() {
                                                        let path_buf = wall.path.clone();
                                                        self.selected_steam_wallpaper = Some(wall.clone());
                                                        self.tuning_wall = Some(wall.clone());
                                                        let key = path_buf.to_string_lossy().to_string();
                                                        self.tuning_overrides = self
                                                            .config
                                                            .wallpaper_overrides
                                                            .get(&key)
                                                            .cloned()
                                                            .unwrap_or_default();
                                                        self.lwe_prop_values = self.tuning_overrides.custom_properties.clone();
                                                        self.lwe_props.clear();
                                                        self.lwe_props_status.clear();
                                                        self.begin_lwe_props_query(ctx, path_buf);
                                                        pending_msg = Some(format!("Tuning Steam Wallpaper: {}", wall.title));
                                                    }
                                                });
                                            });
                                    }
                                });
                            }

                            if self.tuning_wall.is_some() {
                                ui.add_space(10.0);
                                ui.separator();
                                ui.add_space(8.0);
                                self.ui_steam_tuning_panel(ui, ctx, &mut pending_action, &mut pending_msg);
                            }
                        });
                    }
                    AppTab::Settings => {
                        ui.group(|ui| {
                            ui.heading(egui::RichText::new("⚙ Global Settings & Hardware Preferences").color(egui::Color32::from_rgb(0, 240, 255)).strong());
                            ui.add_space(8.0);
                            ui.separator();
                            ui.add_space(8.0);

                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("Hardware Decoder:").strong());
                                let mut selected_hwdec = self.config.hwdec.clone();
                                egui::ComboBox::from_id_salt("hwdec_selector_settings")
                                    .selected_text(&selected_hwdec)
                                    .show_ui(ui, |ui| {
                                        for &(val, label, _desc) in &crate::config::get_available_hwdec_options() {
                                            if ui.selectable_value(&mut selected_hwdec, val.to_string(), label).clicked() {
                                                self.config.hwdec = val.to_string();
                                                let _ = self.config.save();
                                                pending_action = Some(IpcRequest::SetHwdec { hwdec: val.to_string() });
                                                pending_msg = Some(format!("Hardware decoder set to {}", label));
                                            }
                                        }
                                    });
                            });

                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("Target FPS Limit:").strong());
                                let mut fps_val = self.config.target_fps;
                                if ui.add(egui::Slider::new(&mut fps_val, 15..=240).suffix(" FPS")).changed() {
                                    self.config.target_fps = fps_val;
                                    let _ = self.config.save();
                                    pending_action = Some(IpcRequest::SetTargetFps { fps: fps_val });
                                }
                            });

                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("Volume:").strong());
                                if ui.add(egui::Slider::new(&mut self.volume_slider, 0..=150).suffix("%")).changed() {
                                    self.config.volume = self.volume_slider;
                                    let _ = self.config.save();
                                    pending_action = Some(IpcRequest::SetVolume { volume: self.volume_slider });
                                }
                            });

                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("Opacity:").strong());
                                let mut percent = (self.opacity_slider * 100.0).round() as i32;
                                if ui.add(egui::Slider::new(&mut percent, 5..=100).suffix("%")).changed() {
                                    self.opacity_slider = percent as f32 / 100.0;
                                    self.config.opacity = self.opacity_slider;
                                    let _ = self.config.save();
                                    pending_action = Some(IpcRequest::SetOpacity { opacity: self.opacity_slider });
                                }
                            });

                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("Autostart on Boot:").strong());
                                let mut auto = self.autostart_enabled;
                                if ui.checkbox(&mut auto, "Enable Autostart (~/.config/autostart)").changed() {
                                    if Config::set_autostart(auto).is_ok() {
                                        self.autostart_enabled = auto;
                                        pending_msg = Some(format!("Autostart set to {}", auto));
                                    }
                                }
                            });
                        });
                    }
                    AppTab::Installed => {
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
                                ui.label(egui::RichText::new("🖼 Screensaver Asset Source:").strong().small());
                                ui.horizontal_wrapped(|ui| {
                                    if ui.radio(self.config.hyprlock.screensaver_mode == "active", "Active Desktop").clicked() {
                                        self.config.hyprlock.screensaver_mode = "active".to_string();
                                    }
                                    if ui.radio(self.config.hyprlock.screensaver_mode == "video", "🎥 Video Asset").clicked() {
                                        self.config.hyprlock.screensaver_mode = "video".to_string();
                                        if let Some(file) = rfd::FileDialog::new().add_filter("Videos", &["mp4", "mkv", "webm", "avi", "mov", "gif"]).pick_file() {
                                            self.config.hyprlock.asset_path = file.to_string_lossy().to_string();
                                        }
                                    }
                                    if ui.radio(self.config.hyprlock.screensaver_mode == "web", "🌐 Web Asset").clicked() {
                                        self.config.hyprlock.screensaver_mode = "web".to_string();
                                        if let Some(file) = rfd::FileDialog::new().add_filter("Web", &["html", "htm", "js"]).pick_file() {
                                            self.config.hyprlock.asset_path = file.to_string_lossy().to_string();
                                        }
                                    }
                                    if ui.radio(self.config.hyprlock.screensaver_mode == "image", "🖼 Custom Image").clicked() {
                                        self.config.hyprlock.screensaver_mode = "image".to_string();
                                        if let Some(file) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "webp"]).pick_file() {
                                            self.config.hyprlock.asset_path = file.to_string_lossy().to_string();
                                        }
                                    }
                                    if ui.radio(self.config.hyprlock.screensaver_mode == "gradient", "🌈 Color Gradient").clicked() {
                                        self.config.hyprlock.screensaver_mode = "gradient".to_string();
                                    }
                                });

                                if matches!(self.config.hyprlock.screensaver_mode.as_str(), "video" | "web" | "image") {
                                    ui.add_space(4.0);
                                    ui.horizontal(|ui| {
                                        ui.add(egui::TextEdit::singleline(&mut self.config.hyprlock.asset_path).hint_text("Asset path or URL..."));
                                        if ui.button("📁 Pick...").clicked() {
                                            if let Some(file) = rfd::FileDialog::new().pick_file() {
                                                self.config.hyprlock.asset_path = file.to_string_lossy().to_string();
                                            }
                                        }
                                    });
                                } else if self.config.hyprlock.screensaver_mode == "gradient" {
                                    ui.add_space(4.0);
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label("Preset Color: ");
                                        for &(name, hex) in &[("Dark Navy", "#0d1b2a"), ("Deep Purple", "#1a002c"), ("Cyber Teal", "#002b36"), ("OLED Black", "#050505")] {
                                            if ui.selectable_label(self.config.hyprlock.gradient_color == hex, name).clicked() {
                                                self.config.hyprlock.gradient_color = hex.to_string();
                                            }
                                        }
                                    });
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
                        if ui.selectable_label(self.view_mode == ViewMode::Carousel, "🎠 Carousel View").clicked() {
                            self.view_mode = ViewMode::Carousel;
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
                        if self.view_mode == ViewMode::Carousel {
                            let mut web_scroll_delta = 0.0_f32;
                            ui.horizontal(|ui| {
                                let prev_btn = ui.button(egui::RichText::new("◀").size(20.0).color(egui::Color32::from_rgb(0, 240, 255)).strong());
                                let next_btn = ui.button(egui::RichText::new("▶").size(20.0).color(egui::Color32::from_rgb(0, 240, 255)).strong());
                                if prev_btn.clicked() {
                                    web_scroll_delta = -340.0;
                                }
                                if next_btn.clicked() {
                                    web_scroll_delta = 340.0;
                                }

                                egui::ScrollArea::horizontal()
                                    .id_salt("web_bookmarks_carousel")
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        if web_scroll_delta != 0.0 {
                                            ui.scroll_with_delta(egui::vec2(web_scroll_delta, 0.0));
                                        }
                                        ui.horizontal(|ui| {
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
                                                            if let Some(thumb_path) = get_web_thumbnail_path(Some(ctx.clone()), &target_url) {
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
                                                                    pending_action = Some(IpcRequest::SetWallpaper { path: target_url.clone() });
                                                                    pending_msg = Some(format!("Launched 3D web wallpaper: {}", bm.title));
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
                                                    pending_action = Some(IpcRequest::SetWallpaper { path: target_url.clone() });
                                                    pending_msg = Some(format!("Launched 3D web wallpaper: {}", bm.title));
                                                } else if interact.clicked() {
                                                    self.selected_wallpaper = Some(PathBuf::from(&target_url));
                                                }
                                            }
                                        });
                                    });
                            });
                        } else {
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

                                let card_resp = egui::Frame::none()
                                    .fill(card_bg)
                                    .stroke(card_stroke)
                                    .rounding(6.0)
                                    .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            if let Some(thumb_path) = get_web_thumbnail_path(Some(ctx.clone()), &target_url) {
                                                if let Some(tex) = self.get_cached_texture(ctx, &thumb_path) {
                                                    ui.add(egui::Image::new(tex).max_size(egui::vec2(60.0, 36.0)).rounding(4.0));
                                                }
                                            }
                                            ui.label(egui::RichText::new(format!("[{}]", bm.category)).color(egui::Color32::from_rgb(0, 240, 255)).strong().small());
                                            ui.label(egui::RichText::new(&bm.title).strong());

                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                if ui.button(egui::RichText::new("▶ Launch").color(egui::Color32::from_rgb(0, 255, 150)).strong().small()).clicked() {
                                                    self.selected_wallpaper = Some(PathBuf::from(&target_url));
                                                    pending_action = Some(IpcRequest::SetWallpaper { path: target_url.clone() });
                                                    pending_msg = Some(format!("Launched 3D web wallpaper: {}", bm.title));
                                                }
                                                if !bm.is_demo {
                                                    if ui.button("🗑").clicked() {
                                                        self.config.remove_web_bookmark(&bm.url);
                                                    }
                                                }
                                            });
                                        });
                                    }).response;

                                let interact = card_resp.interact(egui::Sense::click());
                                if interact.double_clicked() {
                                    self.selected_wallpaper = Some(PathBuf::from(&target_url));
                                    pending_action = Some(IpcRequest::SetWallpaper { path: target_url.clone() });
                                    pending_msg = Some(format!("Launched 3D web wallpaper: {}", bm.title));
                                } else if interact.clicked() {
                                    self.selected_wallpaper = Some(PathBuf::from(&target_url));
                                }
                            }
                        }

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
                                .add_filter("Videos, GIFs & PKG Scenes", &["mkv", "mp4", "webm", "avi", "mov", "gif", "pkg", "m4v", "flv"])
                                .pick_file()
                            {
                                let path_str = file.to_string_lossy().to_string();
                                pending_action = Some(IpcRequest::SetWallpaper { path: path_str });
                                pending_msg = Some(format!("Applied wallpaper: {}", file.display()));
                            }
                        }
                    });
                } else {
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

                        ctx.input(|i| {
                            if !filtered.is_empty() {
                                let curr_idx = self.selected_wallpaper.as_ref()
                                    .and_then(|p| filtered.iter().position(|&item| item == p))
                                    .unwrap_or(0);

                                if i.key_pressed(egui::Key::ArrowRight) || i.key_pressed(egui::Key::L) {
                                    let next = (curr_idx + 1) % filtered.len();
                                    let sel = filtered[next].clone();
                                    self.selected_wallpaper = Some(sel.clone());
                                } else if i.key_pressed(egui::Key::ArrowLeft) || i.key_pressed(egui::Key::H) {
                                    let prev = if curr_idx == 0 { filtered.len() - 1 } else { curr_idx - 1 };
                                    let sel = filtered[prev].clone();
                                    self.selected_wallpaper = Some(sel.clone());
                                } else if i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::J) {
                                    let next = (curr_idx + 4).min(filtered.len() - 1);
                                    let sel = filtered[next].clone();
                                    self.selected_wallpaper = Some(sel.clone());
                                } else if i.key_pressed(egui::Key::ArrowUp) || i.key_pressed(egui::Key::K) {
                                    let prev = curr_idx.saturating_sub(4);
                                    let sel = filtered[prev].clone();
                                    self.selected_wallpaper = Some(sel.clone());
                                } else if i.key_pressed(egui::Key::Enter) {
                                    if let Some(ref sel) = self.selected_wallpaper {
                                        pending_action = Some(IpcRequest::SetWallpaper { path: sel.to_string_lossy().to_string() });
                                        pending_msg = Some(format!("Applied wallpaper: {}", sel.file_name().and_then(|n| n.to_str()).unwrap_or("")));
                                    }
                                }
                            }
                        });

                        if self.view_mode == ViewMode::Carousel {
                            let mut carousel_scroll = 0.0_f32;
                            ui.horizontal(|ui| {
                                let prev_btn = ui.button(egui::RichText::new("◀").size(20.0).color(egui::Color32::from_rgb(0, 240, 255)).strong());
                                let next_btn = ui.button(egui::RichText::new("▶").size(20.0).color(egui::Color32::from_rgb(0, 240, 255)).strong());
                                if prev_btn.clicked() {
                                    carousel_scroll = -340.0;
                                }
                                if next_btn.clicked() {
                                    carousel_scroll = 340.0;
                                }

                                egui::ScrollArea::horizontal()
                                    .id_salt("carousel_scroll")
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        if carousel_scroll != 0.0 {
                                            ui.scroll_with_delta(egui::vec2(carousel_scroll, 0.0));
                                        }
                                        ui.horizontal(|ui| {
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

                                                let hovered = self.card_hover.get(path).copied().unwrap_or(false);

                                                let frame_res = egui::Frame::none()
                                                    .fill(card_bg)
                                                    .stroke(card_stroke)
                                                    .rounding(8.0)
                                                    .inner_margin(egui::Margin::same(8.0))
                                                    .show(ui, |ui| {
                                                        ui.set_width(210.0);
                                                        ui.set_height(160.0);
                                                        ui.vertical_centered(|ui| {
                                                            self.draw_asset_thumb(ctx, ui, path, hovered, egui::vec2(194.0, 95.0));
                                                            ui.add_space(4.0);
                                                            ui.horizontal(|ui| {
                                                                ui.label(egui::RichText::new(format!("[{}]", ext)).color(badge_color).strong().small());
                                                                ui.add(egui::Label::new(egui::RichText::new(filename).strong().small()).truncate());
                                                            });
                                                            ui.add_space(4.0);
                                                            ui.horizontal(|ui| {
                                                                if ui.button(egui::RichText::new("▶ Apply").color(egui::Color32::from_rgb(0, 255, 150)).strong().small()).clicked() {
                                                                    self.selected_wallpaper = Some(path.clone());
                                                                    pending_action = Some(IpcRequest::SetWallpaper { path: path_str.clone() });
                                                                    pending_msg = Some(format!("Applied wallpaper: {}", filename));
                                                                }
                                                                if ui.button(egui::RichText::new("👁 PiP").color(egui::Color32::from_rgb(0, 240, 255)).strong().small()).clicked() {
                                                                    self.pip_target = Some(path.clone());
                                                                    self.pip_active = true;
                                                                }
                                                                if is_active {
                                                                    ui.label(egui::RichText::new("● LIVE").color(egui::Color32::from_rgb(255, 190, 50)).small().strong());
                                                                }
                                                            });
                                                        });
                                                    });

                                                self.card_hover.insert(path.clone(), frame_res.response.hovered());

                                                if is_selected {
                                                    frame_res.response.scroll_to_me(Some(egui::Align::Center));
                                                }

                                                let card_interact = frame_res.response.interact(egui::Sense::click());
                                                if card_interact.double_clicked() {
                                                    self.selected_wallpaper = Some(path.clone());
                                                    self.pip_target = Some(path.clone());
                                                    self.pip_active = true;
                                                    pending_action = Some(IpcRequest::SetWallpaper { path: path_str.clone() });
                                                    pending_msg = Some(format!("Applied wallpaper & opened PiP: {}", filename));
                                                } else if card_interact.clicked() {
                                                    self.selected_wallpaper = Some(path.clone());
                                                }
                                            }
                                        });
                                    });
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

                                let hovered = self.card_hover.get(path).copied().unwrap_or(false);

                                let card_resp = egui::Frame::none()
                                    .fill(card_bg)
                                    .stroke(card_stroke)
                                    .rounding(6.0)
                                    .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            self.draw_asset_thumb(ctx, ui, path, hovered, egui::vec2(60.0, 36.0));

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
                                                    pending_action = Some(IpcRequest::SetWallpaper { path: path_str.clone() });
                                                    pending_msg = Some(format!("Applied wallpaper: {}", filename));
                                                }
                                                if ui.button(egui::RichText::new("👁 PiP").color(egui::Color32::from_rgb(0, 240, 255)).strong().small()).clicked() {
                                                    self.pip_target = Some(path.clone());
                                                    self.pip_active = true;
                                                }
                                            });
                                        });
                                    })
                                    .response;

                                self.card_hover.insert(path.clone(), card_resp.hovered());

                                let interact = card_resp.interact(egui::Sense::click());
                                if interact.double_clicked() {
                                    self.selected_wallpaper = Some(path.clone());
                                    self.pip_target = Some(path.clone());
                                    self.pip_active = true;
                                    pending_action = Some(IpcRequest::SetWallpaper { path: path_str.clone() });
                                    pending_msg = Some(format!("Applied wallpaper & opened PiP: {}", filename));
                                } else if interact.clicked() {
                                }
                            }
                        }
                    }
                });
            }
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
