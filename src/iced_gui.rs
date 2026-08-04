use iced::theme::Theme;
use iced::widget::{
    button, checkbox, column, container, image, mouse_area,
    row, rule, scrollable, slider, space, text,
};
use iced::window;
use iced::{Background, Border, Color, Element, Length, Subscription, Task};

use ::image::{load_from_memory, open, RgbImage};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use crate::config::{Config, SystemMetrics, WallpaperOverrides};
use crate::display::DisplayInfo;
use crate::ipc::{send_ipc_request, DaemonStatus, IpcRequest, IpcResponse};
use crate::logger::get_log_path;
use crate::steam_scanner::SteamWallpaper;
use crate::steam_workshop::WorkshopItem;

type Elem<'a> = Element<'a, Message>;

const CYAN: Color = Color::from_rgb(0.0, 0.94, 1.0);
const EMERALD: Color = Color::from_rgb(0.0, 1.0, 0.59);
const AMBER: Color = Color::from_rgb(1.0, 0.75, 0.2);
const SOFT_TEXT: Color = Color::from_rgb(0.55, 0.61, 0.71);
const DIM_TEXT: Color = Color::from_rgb(0.35, 0.41, 0.51);
const CARD_BG: Color = Color::from_rgb(0.086, 0.106, 0.165);
const CARD_BG_SEL: Color = Color::from_rgb(0.118, 0.188, 0.294);
const CARD_BG_ACTIVE: Color = Color::from_rgb(0.047, 0.157, 0.11);
const CARD_STROKE: Color = Color::from_rgb(0.137, 0.176, 0.255);
const PANEL_BG: Color = Color::from_rgb(0.039, 0.047, 0.078);

// ---------------------------------------------------------------------------
// Background-result plumbing (shared with webkit_render + steam_workshop)
// ---------------------------------------------------------------------------

static UPDATED_THUMBS: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);
static PENDING_THUMBS: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// Called by the WebKit renderer / Steam preview downloader whenever a
/// thumbnail PNG/JPG is written or regenerated on disk.
pub fn notify_thumb_updated(path: PathBuf) {
    if let Ok(mut guard) = UPDATED_THUMBS.lock() {
        guard.get_or_insert_with(HashSet::new).insert(path);
    }
}

fn drain_updated_thumbs() -> HashSet<PathBuf> {
    UPDATED_THUMBS
        .lock()
        .ok()
        .and_then(|mut g| g.take())
        .unwrap_or_default()
}

fn is_thumb_pending(key: &str) -> bool {
    UPDATED_THUMBS
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|s| s.contains(Path::new(key))))
        .unwrap_or(false)
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

fn is_thumb_pending_key(key: &str) -> bool {
    PENDING_THUMBS
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|s| s.contains(key)))
        .unwrap_or(false)
}

/// djb2-style hash used to key thumbnail cache files (kept for parity).
pub fn md5_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 5381;
    for &b in bytes {
        hash = ((hash << 5).wrapping_add(hash)).wrapping_add(b as u64);
    }
    hash
}

// ---------------------------------------------------------------------------
// Tabs / filters / themes
// ---------------------------------------------------------------------------

#[derive(PartialEq, Clone, Copy, Debug)]
enum AppTab {
    Installed,
    SteamWorkshop,
    Displays,
    Settings,
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum CategoryFilter {
    All,
    Videos,
    WebWidgets,
    StaticImages,
    SteamWorkshop,
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum ViewMode {
    Grid,
    Carousel,
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
            ThemeScheme::DarkGlass => "Dark Glass",
            ThemeScheme::SteamAmber => "Steam Amber",
            ThemeScheme::HardLightCyber => "Cyber Light",
            ThemeScheme::OledPitchBlack => "OLED Black",
        }
    }
}

fn theme_for(scheme: ThemeScheme) -> Theme {
    let pal = |bg: (u8, u8, u8), text_c: (u8, u8, u8), primary: (u8, u8, u8)| {
        iced::theme::Palette {
            background: Color::from_rgb8(bg.0, bg.1, bg.2),
            text: Color::from_rgb8(text_c.0, text_c.1, text_c.2),
            primary: Color::from_rgb8(primary.0, primary.1, primary.2),
            success: EMERALD,
            warning: AMBER,
            danger: Color::from_rgb(0.94, 0.27, 0.27),
        }
    };
    match scheme {
        ThemeScheme::DarkGlass => Theme::custom("Dark Glass", pal((10, 12, 20), (235, 242, 255), (0, 200, 220))),
        ThemeScheme::SteamAmber => Theme::custom("Steam Amber", pal((11, 20, 29), (240, 246, 255), (102, 192, 244))),
        ThemeScheme::HardLightCyber => Theme::custom("Cyber Light", pal((8, 8, 14), (250, 250, 255), (255, 0, 85))),
        ThemeScheme::OledPitchBlack => Theme::custom("OLED Black", pal((0, 0, 0), (255, 255, 255), (0, 240, 255))),
    }
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Message {
    Tick,
    SetWindowId(Option<window::Id>),
    WindowEvent(window::Id, window::Event),
    GotStatus(Result<DaemonStatus, String>),
    GotSteamScan(Vec<SteamWallpaper>),
    GotDisplays(Vec<DisplayInfo>),
    GotWorkshop(Result<Vec<WorkshopItem>, String>),
    GotWorkshopDownload(Result<PathBuf, String>),
    GotLweProps(Result<Vec<crate::lwe::WallpaperProperty>, String>),
    GotLogs(String),
    ThumbDecoded(PathBuf, SystemTime, Result<(u32, u32, Vec<u8>), String>),
    FolderPicked(Option<PathBuf>),

    Tab(AppTab),
    ThemeChanged(ThemeScheme),
    Category(CategoryFilter),
    ViewMode(ViewMode),
    CarouselNext,
    CarouselPrev,
    SearchFilterChanged(String),

    CardEntered(PathBuf),
    CardExited(PathBuf),
    CardClicked(PathBuf),
    CardDoubleClicked(PathBuf),
    ApplyPath(PathBuf),
    ApplyUrl(String),

    // Workshop
    WorkshopQueryChanged(String),
    WorkshopSearch,
    WorkshopClear,
    WorkshopSortChanged(String),
    WorkshopDaysChanged(i64),
    WorkshopPagePrev,
    WorkshopPageNext,
    WorkshopRescanSteam,
    WorkshopDownload(String),
    WorkshopApply(String),
    WorkshopAddToLibrary(String),

    // Installed tab
    RescanWallpapers,
    WebTitleChanged(String),
    WebUrlChanged(String),
    WebCategoryChanged(String),
    SaveWebBookmark,
    RemoveWebBookmark(String),

    // Displays
    SelectDisplay(String),
    RescanDisplays,

    // Settings / GPU
    HwdecChanged(String),
    GpuDeviceChanged(Option<String>),
    FpsChanged(u32),
    VolumeChanged(i64),
    OpacityChanged(f32),
    MuteToggled,
    AutostartToggled,
    StopWallpaper,
    StartDaemon,
    TogglePause,
    NextWallpaper,
    PrevWallpaper,
    MinimizeToTray,
    TogglePin,
    ToggleDoctor,
    ToggleLogs,
    ToggleHyprlock,
    ToggleGpuSettings,
    TestScreensaver,
    SaveHyprlockConf,
    RunInstaller,

    // Steam tuning
    QueryProps,
    TuneFpsChanged(u32),
    TuneVolumeChanged(i64),
    TuneScalingChanged(String),
    TuneClampChanged(String),
    TuneLayerChanged(String),
    TuneScreenshotChanged(String),
    TuneBoolChanged(String, bool),
    TuneSliderChanged(String, String),
    TuneComboChanged(String, String),
    TuneTextChanged(String, String),
    TuneClose,
    SaveAndApplyTuning,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct CachedImage {
    mtime: SystemTime,
    handle: iced::widget::image::Handle,
}

struct IcedGuiApp {
    config: Config,
    window_id: Option<window::Id>,
    status: Option<DaemonStatus>,
    status_message: String,

    active_tab: AppTab,
    theme_scheme: ThemeScheme,

    wallpapers: Vec<PathBuf>,
    selected_wallpaper: Option<PathBuf>,
    search_filter: String,
    category_filter: CategoryFilter,
    view_mode: ViewMode,
    carousel_index: usize,

    web_url_input: String,
    new_web_title: String,
    new_web_category: String,

    steam_wallpapers: Vec<SteamWallpaper>,
    displays: Vec<DisplayInfo>,
    selected_screen: Option<String>,

    volume_slider: i64,
    opacity_slider: f32,
    autostart_enabled: bool,

    show_doctor: bool,
    show_logs: bool,
    show_hyprlock: bool,
    show_gpu_settings: bool,
    logs_content: String,

    workshop_items: Vec<WorkshopItem>,
    workshop_page: u32,
    workshop_sort: String,
    workshop_days: i64,
    workshop_query: String,
    workshop_loading: bool,
    workshop_status: String,
    workshop_downloading: Option<String>,

    tuning_wall: Option<SteamWallpaper>,
    tuning_overrides: WallpaperOverrides,
    lwe_props: Vec<crate::lwe::WallpaperProperty>,
    lwe_prop_values: HashMap<String, String>,
    lwe_props_busy: bool,
    lwe_props_status: String,

    system_metrics: SystemMetrics,
    last_poll: Option<Instant>,
    last_metrics_poll: Option<Instant>,

    card_hover: HashMap<PathBuf, bool>,
    hover_streaming: Option<PathBuf>,
    hover_video_process: Option<Child>,
    madamiru_player: Option<crate::video_render::MadamiruVideoPlayer>,
    hover_stream_mtime: Option<SystemTime>,

    image_cache: HashMap<PathBuf, CachedImage>,
    pending_decodes: HashSet<PathBuf>,
    last_click: Option<(PathBuf, Instant)>,

    start_minimized: bool,
    minimized_on_launch_done: bool,
    is_pinned_on_top: bool,
}

const HOVER_VIDEO_LIVE_PATH: &str = "/tmp/omywall_thumbs/hover_video_live.png";
const HOVER_WEB_LIVE_PATH: &str = "/tmp/omywall_thumbs/hover_web_live.png";

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn run_gui(config: Config, start_minimized: bool) -> iced::Result {
    let icon = load_window_icon().and_then(|(rgba, w, h)| window::icon::from_rgba(rgba, w, h).ok());
    let window_settings = window::Settings {
        size: iced::Size::new(1240.0, 820.0),
        min_size: Some(iced::Size::new(940.0, 640.0)),
        icon,
        visible: !start_minimized,
        exit_on_close_request: false,
        ..Default::default()
    };

    let cfg = config.clone();
    let result = iced::application(
        move || {
            let app = IcedGuiApp::new(cfg.clone(), start_minimized);
            let tasks = app.boot_tasks();
            (app, tasks)
        },
        update,
        view,
    )
    .window(window_settings)
    .title("OMYWALL Wallpaper Engine v4.5")
    .theme(app_theme)
    .subscription(subscription)
    .run();
    crate::webkit_render::shutdown_global_renderer();
    result
}

fn app_theme(state: &IcedGuiApp) -> Theme {
    theme_for(state.theme_scheme)
}

fn load_window_icon() -> Option<(Vec<u8>, u32, u32)> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/home/user"));
    let candidates = [
        PathBuf::from("assets/omywall.png"),
        PathBuf::from("assets/omywall.svg"),
        home.join(".local/share/omywall/assets/omywall.png"),
        home.join(".local/share/icons/hicolor/512x512/apps/omywall.png"),
    ];
    for path in &candidates {
        if let Ok(img) = open(&path) {
            let rgba = img.to_rgba8();
            let w = rgba.width();
            let h = rgba.height();
            return Some((rgba.into_raw(), w, h));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Boot
// ---------------------------------------------------------------------------

impl IcedGuiApp {
    fn new(config: Config, start_minimized: bool) -> Self {
        let wallpapers = Self::scan_wallpapers(&config.wallpaper_dir);
        let selected_wallpaper = wallpapers.first().cloned();
        crate::webkit_render::init_global_renderer();
        let mut app = IcedGuiApp {
            config,
            window_id: None,
            status: None,
            status_message: "Ready".to_string(),
            active_tab: AppTab::Installed,
            theme_scheme: ThemeScheme::default(),
            wallpapers,
            selected_wallpaper,
            search_filter: String::new(),
            category_filter: CategoryFilter::All,
            view_mode: ViewMode::Carousel,
            carousel_index: 0,
            web_url_input: "https://".to_string(),
            new_web_title: String::new(),
            new_web_category: "Web Animation".to_string(),
            steam_wallpapers: Vec::new(),
            displays: Vec::new(),
            selected_screen: None,
            volume_slider: 0,
            opacity_slider: 1.0,
            autostart_enabled: Config::is_autostart_enabled(),
            show_doctor: false,
            show_logs: false,
            show_hyprlock: false,
            show_gpu_settings: false,
            logs_content: String::new(),
            workshop_items: Vec::new(),
            workshop_page: 1,
            workshop_sort: "trend".to_string(),
            workshop_days: 7,
            workshop_query: String::new(),
            workshop_loading: false,
            workshop_status: String::new(),
            workshop_downloading: None,
            tuning_wall: None,
            tuning_overrides: WallpaperOverrides::default(),
            lwe_props: Vec::new(),
            lwe_prop_values: HashMap::new(),
            lwe_props_busy: false,
            lwe_props_status: String::new(),
            system_metrics: SystemMetrics::default(),
            last_poll: None,
            last_metrics_poll: None,
            card_hover: HashMap::new(),
            hover_streaming: None,
            hover_video_process: None,
            madamiru_player: None,
            hover_stream_mtime: None,
            image_cache: HashMap::new(),
            pending_decodes: HashSet::new(),
            last_click: None,
            start_minimized,
            minimized_on_launch_done: false,
            is_pinned_on_top: false,
        };
        app.volume_slider = app.config.volume;
        app.opacity_slider = app.config.opacity;
        app
    }

    fn boot_tasks(&self) -> Task<Message> {
        let mut tasks = vec![
            window::latest().map(Message::SetWindowId),
            Task::perform(poll_status(self.config.socket_path.clone()), Message::GotStatus),
            Task::perform(spawn_blocking(crate::steam_scanner::scan_steam_wallpapers), Message::GotSteamScan),
            Task::perform(spawn_blocking(crate::display::detect_displays), Message::GotDisplays),
            Task::perform(load_logs(), Message::GotLogs),
            Task::done(Message::Tick),
        ];
        if self.config.socket_path.exists() {
            // no-op marker; spawn daemon handled by GotStatus failure path
        }
        if let Some(id) = self.window_id {
            let _ = id;
        }
        Task::batch(tasks)
    }

    fn scan_wallpapers(dir: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let mut seen = HashSet::new();
        let valid_exts = ["mkv", "mp4", "webm", "avi", "mov", "gif", "html", "htm", "js", "pkg", "m4v", "flv", "wmv", "png", "jpg", "jpeg", "webp"];

        let _ = std::fs::create_dir_all(dir);

        fn walk_dir(d: &Path, depth: usize, files: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, valid_exts: &[&str]) {
            if depth > 2 {
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
            let omy_assets = home.join(".local").join("share").join("omywall").join("assets");
            if omy_assets.exists() && omy_assets != dir {
                walk_dir(&omy_assets, 0, &mut files, &mut seen, &valid_exts);
            }
        }

        files.sort_by_key(|f| f.file_name().unwrap_or_default().to_string_lossy().to_string());
        files
    }
}

// ---------------------------------------------------------------------------
// Async helpers
// ---------------------------------------------------------------------------

async fn spawn_blocking<T, F>(f: F) -> T
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    tokio::task::spawn_blocking(f).await.unwrap_or_else(|_| panic!("blocking task panicked"))
}

async fn poll_status(socket: PathBuf) -> Result<DaemonStatus, String> {
    match send_ipc_request(&socket, &IpcRequest::GetStatus).await {
        Ok(IpcResponse::Status(st)) => Ok(st),
        Ok(_) => Err("unexpected response".to_string()),
        Err(e) => Err(e),
    }
}

async fn send_req(socket: PathBuf, req: IpcRequest) -> Result<DaemonStatus, String> {
    let _ = send_ipc_request(&socket, &req).await;
    match send_ipc_request(&socket, &IpcRequest::GetStatus).await {
        Ok(IpcResponse::Status(st)) => Ok(st),
        Ok(_) => Err("unexpected response".to_string()),
        Err(e) => Err(e),
    }
}

async fn load_logs() -> String {
    let log_path = get_log_path();
    std::fs::read_to_string(log_path).unwrap_or_default()
}

async fn browse(empty_query: bool, query: String, page: u32, sort: String, days: i64) -> Result<Vec<WorkshopItem>, String> {
    spawn_blocking(move || {
        if !empty_query && !query.trim().is_empty() {
            crate::steam_workshop::search_workshop(&query, page, &sort, days)
        } else {
            crate::steam_workshop::browse_workshop(page, &sort, days)
        }
    })
    .await
}

async fn decode_thumb(path: PathBuf) -> (PathBuf, SystemTime, Result<(u32, u32, Vec<u8>), String>) {
    let p_clone = path.clone();
    let (mtime, res) = spawn_blocking(move || {
        let mtime = std::fs::metadata(&p_clone)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let res = std::fs::read(&p_clone)
            .map_err(|e| e.to_string())
            .and_then(|bytes| {
                load_from_memory(&bytes)
                    .map_err(|e| e.to_string())
                    .map(|img| {
                        let img = img.thumbnail(384, 216);
                        let rgba = img.to_rgba8();
                        (rgba.width(), rgba.height(), rgba.into_raw())
                    })
            });
        (mtime, res)
    })
    .await;
    (path, mtime, res)
}

// ---------------------------------------------------------------------------
// Thumbnail path helpers
// ---------------------------------------------------------------------------

fn generate_video_fallback_image(_title_text: &str, ext_str: &str, target_path: &Path) {
    let width = 320u32;
    let height = 180u32;
    let mut imgbuf = RgbImage::new(width, height);
    for (x, y, pixel) in imgbuf.enumerate_pixels_mut() {
        let factor = (x + y) as f32 / (width + height) as f32;
        let r = (13.0 * (1.0 - factor) + 22.0 * factor) as u8;
        let g = (17.0 * (1.0 - factor) + 28.0 * factor) as u8;
        let b = (26.0 * (1.0 - factor) + 43.0 * factor) as u8;
        *pixel = ::image::Rgb([r, g, b]);
    }
    let (br, bg, bb) = match ext_str.to_uppercase().as_str() {
        "MKV" => (255, 120, 0),
        "MP4" => (0, 180, 240),
        "GIF" => (220, 100, 255),
        _ => (168, 85, 247),
    };
    for x in 8..=311 {
        imgbuf.put_pixel(x, 8, ::image::Rgb([br, bg, bb]));
        imgbuf.put_pixel(x, 9, ::image::Rgb([br, bg, bb]));
        imgbuf.put_pixel(x, 170, ::image::Rgb([br, bg, bb]));
        imgbuf.put_pixel(x, 171, ::image::Rgb([br, bg, bb]));
    }
    for y in 8..=171 {
        imgbuf.put_pixel(8, y, ::image::Rgb([br, bg, bb]));
        imgbuf.put_pixel(9, y, ::image::Rgb([br, bg, bb]));
        imgbuf.put_pixel(310, y, ::image::Rgb([br, bg, bb]));
        imgbuf.put_pixel(311, y, ::image::Rgb([br, bg, bb]));
    }
    for y in 70..=110 {
        let max_x = 145 + ((y as i32 - 70) * 35 / 40);
        for x in 145..=max_x.min(185) {
            if x >= 0 && x < 320 && y < 180 {
                imgbuf.put_pixel(x as u32, y as u32, ::image::Rgb([br, bg, bb]));
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

    if !is_thumb_pending_key(&key) {
        set_thumb_pending(&key, true);
        let input_str = key.clone();
        let thumb_str = thumb_file.to_string_lossy().to_string();
        let ext_str = ext.clone();
        std::thread::spawn(move || {
            let mut res = Command::new("ffmpeg")
                .args(["-ss", "00:00:00.500", "-i", &input_str, "-vframes", "1", "-s", "320x180", "-f", "image2", "-y", &thumb_str])
                .output();
            if res.is_err() || !Path::new(&thumb_str).exists() {
                res = Command::new("ffmpeg")
                    .args(["-i", &input_str, "-vframes", "1", "-s", "320x180", "-f", "image2", "-y", &thumb_str])
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

/// Resolves a thumbnail path for a web/HTML target. Used both by the GUI and
/// by `config::generate_hyprlock_conf` (via `crate::iced_gui`).
pub fn get_web_thumbnail_path(target: &str) -> Option<PathBuf> {
    let resolved = crate::config::resolve_asset_path(target);
    let path = Path::new(&resolved);
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();

    if matches!(ext.as_str(), "mp4" | "mkv" | "webm" | "avi" | "mov" | "gif" | "png" | "jpg" | "jpeg" | "webp") {
        return get_thumbnail_path(path);
    }

    let cache_dir = PathBuf::from("/tmp/omywall_thumbs");
    let _ = std::fs::create_dir_all(&cache_dir);

    let hash = format!("{:x}", md5_hash(resolved.as_bytes()));
    let thumb_file = cache_dir.join(format!("web_{}.png", &hash[..8]));

    if !thumb_file.exists() {
        if let Some(renderer) = crate::webkit_render::global_renderer() {
            renderer.render_thumbnail(&resolved, &thumb_file);
        }
        return None;
    }
    Some(thumb_file)
}

// ---------------------------------------------------------------------------
// Hover live streams
// ---------------------------------------------------------------------------

fn start_hover_video_stream(app: &mut IcedGuiApp, target: &Path) {
    stop_hover_video_stream(app);
    let cache_dir = PathBuf::from("/tmp/omywall_thumbs");
    let _ = std::fs::create_dir_all(&cache_dir);
    let _ = std::fs::remove_file(HOVER_VIDEO_LIVE_PATH);
    let out_str = HOVER_VIDEO_LIVE_PATH.to_string();
    let input = target.to_string_lossy().to_string();
    match Command::new("ffmpeg")
        .args([
            "-hide_banner", "-loglevel", "error", "-re", "-i", &input,
            "-vf", "fps=5,scale=320:180:flags=lanczos", "-update", "1", "-f", "image2", "-y", &out_str,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(child) => app.hover_video_process = Some(child),
        Err(_) => app.hover_streaming = None,
    }
    app.hover_stream_mtime = None;
}

fn stop_hover_video_stream(app: &mut IcedGuiApp) {
    if let Some(player) = app.madamiru_player.take() {
        player.stop();
    }
    if let Some(mut child) = app.hover_video_process.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    app.hover_stream_mtime = None;
}

fn manage_hover_stream(app: &mut IcedGuiApp, hovered: &Path) {
    let path_str = hovered.to_string_lossy().to_string();
    let ext = hovered.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let is_web = matches!(ext.as_str(), "html" | "htm" | "js")
        || path_str.starts_with("http://")
        || path_str.starts_with("https://");
    let is_video = matches!(ext.as_str(), "mp4" | "mkv" | "webm" | "avi" | "mov" | "gif" | "flv" | "m4v" | "wmv");

    if app.hover_streaming.as_deref() == Some(hovered) {
        if is_web {
            let live_path = PathBuf::from(HOVER_WEB_LIVE_PATH);
            if let Ok(meta) = std::fs::metadata(&live_path) {
                app.hover_stream_mtime = Some(meta.modified().unwrap_or(SystemTime::UNIX_EPOCH));
            }
        }
        return;
    }

    stop_hover_stream(app);
    app.hover_streaming = Some(hovered.to_path_buf());
    app.hover_stream_mtime = None;

    if is_web {
        crate::webkit_render::start_live_pip(&path_str, Path::new(HOVER_WEB_LIVE_PATH));
    } else if is_video {
        if let Ok(player) = crate::video_render::MadamiruVideoPlayer::new(hovered) {
            app.madamiru_player = Some(player);
        } else {
            start_hover_video_stream(app, hovered);
        }
    }
}

fn stop_hover_stream(app: &mut IcedGuiApp) {
    crate::webkit_render::stop_live_pip();
    stop_hover_video_stream(app);
    app.hover_streaming = None;
    app.hover_stream_mtime = None;
}

// ---------------------------------------------------------------------------
// Minimize / pin helpers
// ---------------------------------------------------------------------------

fn minimize_gui_window(id: window::Id) -> Task<Message> {
    let mut tasks = vec![window::minimize::<Message>(id, true)];
    tasks.push(Task::perform(
        spawn_blocking(move || {
            let _ = Command::new("hyprctl").args(["dispatch", "movetoworkspacesilent", "special:omywall,title:OMYWALL"]).output();
            let _ = Command::new("hyprctl").args(["dispatch", "minimize"]).output();
            let _ = Command::new("swaymsg").args(["[title=\"OMYWALL\"]", "move", "scratchpad"]).output();
            let _ = Command::new("xdotool").args(["search", "--class", "omywall", "windowminimize"]).output();
            let _ = Command::new("wmctrl").args(["-r", "OMYWALL", "-b", "add,hidden"]).output();
        }),
        |_| Message::Tick,
    ));
    Task::batch(tasks)
}

fn toggle_always_on_top(id: window::Id, on_top: bool) -> Task<Message> {
    let mut tasks = vec![window::set_level(
        id,
        if on_top { window::Level::AlwaysOnTop } else { window::Level::Normal },
    )];
    tasks.push(Task::perform(
        spawn_blocking(move || {
            if on_top {
                let _ = Command::new("hyprctl").args(["dispatch", "setfloating", "title:^(OMYWALL.*)$"]).output();
                let _ = Command::new("hyprctl").args(["dispatch", "pin", "title:^(OMYWALL.*)$"]).output();
            } else {
                let _ = Command::new("hyprctl").args(["dispatch", "unpin", "title:^(OMYWALL.*)$"]).output();
            }
        }),
        |_| Message::Tick,
    ));
    Task::batch(tasks)
}

// ---------------------------------------------------------------------------
// System doctor / installer (kept from egui GUI)
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct ToolStatus {
    name: String,
    description: String,
    installed: bool,
}

fn check_tool_installed(cmd: &str) -> bool {
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            if dir.join(cmd).exists() {
                return true;
            }
        }
    }
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/home/user"));
    for c in [
        home.join(".local/bin").join(cmd),
        home.join(".cargo/bin").join(cmd),
        PathBuf::from("/usr/bin").join(cmd),
        PathBuf::from("/usr/local/bin").join(cmd),
        PathBuf::from("/bin").join(cmd),
    ] {
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
        ("hyprlock", "hyprlock", "Wayland GPU-accelerated screen locker & screensaver"),
    ];
    tools
        .into_iter()
        .map(|(name, cmd, desc)| ToolStatus {
            name: name.to_string(),
            description: desc.to_string(),
            installed: check_tool_installed(cmd),
        })
        .collect()
}

fn run_installer_script() -> String {
    let cwd_script = std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join("scripts").join("install_deps.sh");
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

    let shell_fallback = format!(
        "ghostty -e bash -c '{0}' || kitty -e bash -c '{0}' || alacritty -e bash -c '{0}' || foot bash -c '{0}' || konsole -e bash -c '{0}' || gnome-terminal -- bash -c '{0}' || x-terminal-emulator -e bash -c '{0}'",
        bash_cmd.replace('\'', "'\\''")
    );
    match Command::new("sh").args(["-c", &shell_fallback]).spawn() {
        Ok(_) => "Launched dependency installer script via shell fallback".to_string(),
        Err(e) => format!("Failed to launch terminal for installer: {}", e),
    }
}

fn fmt_f64(v: f64) -> String {
    let rounded = (v * 100000.0).round() / 100000.0;
    format!("{}", rounded)
}

fn get_current_time_str() -> (String, String) {
    let t = std::time::SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let tm = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        tm
    };
    let time_str = format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec);
    let months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let days = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let m_idx = (tm.tm_mon as usize).min(11);
    let d_idx = (tm.tm_wday as usize).min(6);
    let date_str = format!("{}, {} {} {:04}", days[d_idx], months[m_idx], tm.tm_mday, tm.tm_year + 1900);
    (time_str, date_str)
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

fn subscription(_app: &IcedGuiApp) -> Subscription<Message> {
    iced::time::every(Duration::from_millis(500)).map(|_| Message::Tick)
}

fn update(app: &mut IcedGuiApp, message: Message) -> Task<Message> {
    match message {
        Message::Tick => {
            let now = Instant::now();
            if app.last_metrics_poll.map_or(true, |t| now.duration_since(t) >= Duration::from_secs(1)) {
                app.last_metrics_poll = Some(now);
                app.system_metrics = crate::config::get_system_metrics();
            }

            let updated = drain_updated_thumbs();
            for path in updated {
                app.image_cache.remove(&path);
            }
            if let Some(hovered) = app.hover_streaming.clone() {
                manage_hover_stream(app, &hovered);
            }

            if let Some(ref player) = app.madamiru_player {
                if let Some(frame) = player.get_current_frame() {
                    let handle = iced::widget::image::Handle::from_rgba(frame.width, frame.height, frame.data);
                    app.image_cache.insert(PathBuf::from(HOVER_VIDEO_LIVE_PATH), CachedImage {
                        mtime: SystemTime::now(),
                        handle,
                    });
                }
            }

            // Decode live WebKit PiP frame if updated
            let web_live = PathBuf::from(HOVER_WEB_LIVE_PATH);
            if web_live.exists() {
                let mtime = std::fs::metadata(&web_live).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
                let cached_mtime = app.image_cache.get(&web_live).map(|c| c.mtime).unwrap_or(SystemTime::UNIX_EPOCH);
                if mtime > cached_mtime && !app.pending_decodes.contains(&web_live) {
                    app.pending_decodes.insert(web_live.clone());
                    return Task::perform(decode_thumb(web_live), |(p, m, r)| Message::ThumbDecoded(p, m, r));
                }
            }

            // Asynchronously decode next pending catalog wallpaper thumbnail
            for w in app.wallpapers.iter().take(60) {
                if let Some(thumb_path) = get_web_thumbnail_path(&w.to_string_lossy()) {
                    if thumb_path.exists() && !app.image_cache.contains_key(&thumb_path) && !app.pending_decodes.contains(&thumb_path) {
                        app.pending_decodes.insert(thumb_path.clone());
                        return Task::perform(decode_thumb(thumb_path), |(p, m, r)| Message::ThumbDecoded(p, m, r));
                    }
                }
            }
            Task::none()
        }
        Message::SetWindowId(id) => {
            app.window_id = id;
            if app.start_minimized && !app.minimized_on_launch_done {
                app.minimized_on_launch_done = true;
                if let Some(win_id) = id {
                    return minimize_gui_window(win_id);
                }
            }
            Task::none()
        }
        Message::WindowEvent(_id, event) => {
            if let window::Event::CloseRequested = event {
                if let Some(win_id) = app.window_id {
                    return minimize_gui_window(win_id);
                }
            }
            Task::none()
        }
        Message::GotStatus(res) => {
            match res {
                Ok(st) => {
                    app.status = Some(st);
                    app.status_message = "Daemon Online".to_string();
                }
                Err(e) => {
                    app.status = None;
                    app.status_message = format!("Daemon Offline: {}", e);
                }
            }
            Task::none()
        }
        Message::GotSteamScan(items) => {
            app.steam_wallpapers = items;
            Task::none()
        }
        Message::GotDisplays(info) => {
            app.displays = info;
            Task::none()
        }
        Message::GotWorkshop(res) => {
            app.workshop_loading = false;
            match res {
                Ok(items) => {
                    app.workshop_items = items;
                    app.workshop_status = format!("Loaded {} Workshop items", app.workshop_items.len());
                }
                Err(e) => {
                    app.workshop_status = format!("Workshop Error: {}", e);
                }
            }
            Task::none()
        }
        Message::GotWorkshopDownload(res) => {
            app.workshop_downloading = None;
            match res {
                Ok(path) => {
                    app.status_message = format!("Downloaded: {}", path.display());
                    return Task::perform(spawn_blocking(crate::steam_scanner::scan_steam_wallpapers), Message::GotSteamScan);
                }
                Err(e) => {
                    app.status_message = format!("Download failed: {}", e);
                }
            }
            Task::none()
        }
        Message::GotLweProps(res) => {
            app.lwe_props_busy = false;
            match res {
                Ok(props) => {
                    app.lwe_props = props;
                    app.lwe_props_status = format!("Loaded {} properties", app.lwe_props.len());
                }
                Err(e) => {
                    app.lwe_props_status = format!("Failed to parse properties: {}", e);
                }
            }
            Task::none()
        }
        Message::GotLogs(logs) => {
            app.logs_content = logs;
            Task::none()
        }
        Message::ThumbDecoded(path, mtime, res) => {
            app.pending_decodes.remove(&path);
            if let Ok((w, h, bytes)) = res {
                let handle = iced::widget::image::Handle::from_rgba(w, h, bytes);
                app.image_cache.insert(path, CachedImage { mtime, handle });
            }
            Task::none()
        }
        Message::FolderPicked(Some(dir)) => {
            app.config.wallpaper_dir = dir.clone();
            let _ = app.config.save();
            app.wallpapers = IcedGuiApp::scan_wallpapers(&dir);
            app.selected_wallpaper = app.wallpapers.first().cloned();
            Task::none()
        }
        Message::FolderPicked(None) => Task::none(),
        Message::Tab(tab) => {
            app.active_tab = tab;
            match tab {
                AppTab::Displays => Task::perform(spawn_blocking(crate::display::detect_displays), Message::GotDisplays),
                AppTab::SteamWorkshop => {
                    if app.workshop_items.is_empty() && !app.workshop_loading {
                        app.workshop_loading = true;
                        let q = app.workshop_query.clone();
                        let p = app.workshop_page;
                        let s = app.workshop_sort.clone();
                        let d = app.workshop_days;
                        Task::perform(browse(false, q, p, s, d), Message::GotWorkshop)
                    } else {
                        Task::none()
                    }
                }
                _ => Task::none(),
            }
        }
        Message::ThemeChanged(scheme) => {
            app.theme_scheme = scheme;
            Task::none()
        }
        Message::Category(cat) => {
            app.category_filter = cat;
            Task::none()
        }
        Message::ViewMode(mode) => {
            app.view_mode = mode;
            Task::none()
        }
        Message::CarouselNext => {
            if !app.wallpapers.is_empty() {
                app.carousel_index = (app.carousel_index + 1) % app.wallpapers.len();
            }
            Task::none()
        }
        Message::CarouselPrev => {
            if !app.wallpapers.is_empty() {
                if app.carousel_index == 0 {
                    app.carousel_index = app.wallpapers.len() - 1;
                } else {
                    app.carousel_index -= 1;
                }
            }
            Task::none()
        }
        Message::SearchFilterChanged(text) => {
            app.search_filter = text;
            Task::none()
        }
        Message::CardEntered(path) => {
            app.card_hover.insert(path.clone(), true);
            manage_hover_stream(app, &path);
            Task::none()
        }
        Message::CardExited(path) => {
            app.card_hover.insert(path.clone(), false);
            if app.hover_streaming.as_ref() == Some(&path) {
                stop_hover_stream(app);
            }
            Task::none()
        }
        Message::CardClicked(path) => {
            app.selected_wallpaper = Some(path);
            Task::none()
        }
        Message::CardDoubleClicked(path) => {
            app.selected_wallpaper = Some(path.clone());
            Task::perform(send_req(app.config.socket_path.clone(), IpcRequest::SetWallpaper { path: path.to_string_lossy().to_string() }), Message::GotStatus)
        }
        Message::ApplyPath(path) => {
            Task::perform(send_req(app.config.socket_path.clone(), IpcRequest::SetWallpaper { path: path.to_string_lossy().to_string() }), Message::GotStatus)
        }
        Message::ApplyUrl(url) => {
            Task::perform(send_req(app.config.socket_path.clone(), IpcRequest::SetUrl { url }), Message::GotStatus)
        }
        Message::WorkshopQueryChanged(q) => {
            app.workshop_query = q;
            Task::none()
        }
        Message::WorkshopSearch => {
            app.workshop_loading = true;
            app.workshop_page = 1;
            let q = app.workshop_query.clone();
            let p = app.workshop_page;
            let s = app.workshop_sort.clone();
            let d = app.workshop_days;
            Task::perform(browse(false, q, p, s, d), Message::GotWorkshop)
        }
        Message::WorkshopClear => {
            app.workshop_query.clear();
            app.workshop_loading = true;
            app.workshop_page = 1;
            let q = String::new();
            let p = app.workshop_page;
            let s = app.workshop_sort.clone();
            let d = app.workshop_days;
            Task::perform(browse(true, q, p, s, d), Message::GotWorkshop)
        }
        Message::WorkshopSortChanged(sort) => {
            app.workshop_sort = sort;
            app.workshop_loading = true;
            let q = app.workshop_query.clone();
            let p = app.workshop_page;
            let s = app.workshop_sort.clone();
            let d = app.workshop_days;
            Task::perform(browse(false, q, p, s, d), Message::GotWorkshop)
        }
        Message::WorkshopDaysChanged(days) => {
            app.workshop_days = days;
            app.workshop_loading = true;
            let q = app.workshop_query.clone();
            let p = app.workshop_page;
            let s = app.workshop_sort.clone();
            let d = app.workshop_days;
            Task::perform(browse(false, q, p, s, d), Message::GotWorkshop)
        }
        Message::WorkshopPagePrev => {
            if app.workshop_page > 1 {
                app.workshop_page -= 1;
                app.workshop_loading = true;
                let q = app.workshop_query.clone();
                let p = app.workshop_page;
                let s = app.workshop_sort.clone();
                let d = app.workshop_days;
                Task::perform(browse(false, q, p, s, d), Message::GotWorkshop)
            } else {
                Task::none()
            }
        }
        Message::WorkshopPageNext => {
            app.workshop_page += 1;
            app.workshop_loading = true;
            let q = app.workshop_query.clone();
            let p = app.workshop_page;
            let s = app.workshop_sort.clone();
            let d = app.workshop_days;
            Task::perform(browse(false, q, p, s, d), Message::GotWorkshop)
        }
        Message::WorkshopRescanSteam => {
            Task::perform(spawn_blocking(crate::steam_scanner::scan_steam_wallpapers), Message::GotSteamScan)
        }
        Message::WorkshopDownload(id) => {
            app.workshop_downloading = Some(id.clone());
            Task::perform(spawn_blocking(move || crate::steam_workshop::download_workshop_item(&id)), Message::GotWorkshopDownload)
        }
        Message::WorkshopApply(id) => {
            Task::perform(send_req(app.config.socket_path.clone(), IpcRequest::SetSteamWallpaper { path: id, screen: None, overrides: None }), Message::GotStatus)
        }
        Message::WorkshopAddToLibrary(_) => Task::none(),
        Message::RescanWallpapers => {
            app.wallpapers = IcedGuiApp::scan_wallpapers(&app.config.wallpaper_dir);
            Task::none()
        }
        Message::WebTitleChanged(s) => {
            app.new_web_title = s;
            Task::none()
        }
        Message::WebUrlChanged(s) => {
            app.web_url_input = s;
            Task::none()
        }
        Message::WebCategoryChanged(s) => {
            app.new_web_category = s;
            Task::none()
        }
        Message::SaveWebBookmark => {
            if !app.new_web_title.trim().is_empty() && !app.web_url_input.trim().is_empty() {
                app.config.add_web_bookmark(
                    app.new_web_title.trim().to_string(),
                    app.web_url_input.trim().to_string(),
                    app.new_web_category.clone(),
                );
                let _ = app.config.save();
                app.new_web_title.clear();
            }
            Task::none()
        }
        Message::RemoveWebBookmark(url) => {
            app.config.remove_web_bookmark(&url);
            let _ = app.config.save();
            Task::none()
        }
        Message::SelectDisplay(name) => {
            app.selected_screen = Some(name);
            Task::none()
        }
        Message::RescanDisplays => {
            Task::perform(spawn_blocking(crate::display::detect_displays), Message::GotDisplays)
        }
        Message::HwdecChanged(dec) => {
            app.config.hwdec = dec.clone();
            let _ = app.config.save();
            Task::perform(send_req(app.config.socket_path.clone(), IpcRequest::SetHwdec { hwdec: dec }), Message::GotStatus)
        }
        Message::GpuDeviceChanged(dev) => {
            app.config.gpu_device = dev;
            let _ = app.config.save();
            Task::none()
        }
        Message::FpsChanged(fps) => {
            app.config.target_fps = fps;
            let _ = app.config.save();
            Task::none()
        }
        Message::VolumeChanged(vol) => {
            app.volume_slider = vol;
            app.config.volume = vol;
            let _ = app.config.save();
            Task::perform(send_req(app.config.socket_path.clone(), IpcRequest::SetVolume { volume: vol }), Message::GotStatus)
        }
        Message::OpacityChanged(op) => {
            app.opacity_slider = op;
            app.config.opacity = op;
            let _ = app.config.save();
            Task::perform(send_req(app.config.socket_path.clone(), IpcRequest::SetOpacity { opacity: op }), Message::GotStatus)
        }
        Message::MuteToggled => {
            app.config.mute = !app.config.mute;
            let _ = app.config.save();
            Task::perform(send_req(app.config.socket_path.clone(), IpcRequest::SetMute { mute: app.config.mute }), Message::GotStatus)
        }
        Message::AutostartToggled => {
            let next = !app.autostart_enabled;
            if Config::set_autostart(next).is_ok() {
                app.autostart_enabled = next;
            }
            Task::none()
        }
        Message::StopWallpaper => {
            Task::perform(send_req(app.config.socket_path.clone(), IpcRequest::StopWallpaper), Message::GotStatus)
        }
        Message::StartDaemon => {
            let sock = app.config.socket_path.clone();
            let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("omywall"));
            let _ = Command::new(exe).arg("daemon").spawn();
            Task::perform(poll_status(sock), Message::GotStatus)
        }
        Message::TogglePause => {
            Task::perform(send_req(app.config.socket_path.clone(), IpcRequest::TogglePause), Message::GotStatus)
        }
        Message::NextWallpaper => {
            Task::perform(send_req(app.config.socket_path.clone(), IpcRequest::NextWallpaper), Message::GotStatus)
        }
        Message::PrevWallpaper => {
            Task::perform(send_req(app.config.socket_path.clone(), IpcRequest::PrevWallpaper), Message::GotStatus)
        }
        Message::MinimizeToTray => {
            if let Some(id) = app.window_id {
                minimize_gui_window(id)
            } else {
                Task::none()
            }
        }
        Message::TogglePin => {
            app.is_pinned_on_top = !app.is_pinned_on_top;
            if let Some(id) = app.window_id {
                toggle_always_on_top(id, app.is_pinned_on_top)
            } else {
                Task::none()
            }
        }
        Message::ToggleDoctor => {
            app.show_doctor = !app.show_doctor;
            Task::none()
        }
        Message::ToggleLogs => {
            app.show_logs = !app.show_logs;
            if app.show_logs {
                Task::perform(load_logs(), Message::GotLogs)
            } else {
                Task::none()
            }
        }
        Message::ToggleHyprlock => {
            app.show_hyprlock = !app.show_hyprlock;
            Task::none()
        }
        Message::ToggleGpuSettings => {
            app.show_gpu_settings = !app.show_gpu_settings;
            Task::none()
        }
        Message::TestScreensaver => {
            let _ = Command::new("hyprlock").spawn();
            Task::none()
        }
        Message::SaveHyprlockConf => Task::none(),
        Message::RunInstaller => {
            app.status_message = run_installer_script();
            Task::none()
        }
        Message::QueryProps => {
            if let Some(wall) = &app.tuning_wall {
                app.lwe_props_busy = true;
                let dir = wall.path.clone();
                Task::perform(spawn_blocking(move || crate::lwe::list_properties(&dir)), Message::GotLweProps)
            } else {
                Task::none()
            }
        }
        Message::TuneFpsChanged(fps) => {
            app.tuning_overrides.fps = Some(fps);
            Task::none()
        }
        Message::TuneVolumeChanged(vol) => {
            app.tuning_overrides.volume = Some(vol);
            Task::none()
        }
        Message::TuneScalingChanged(sc) => {
            app.tuning_overrides.scaling = Some(sc);
            Task::none()
        }
        Message::TuneClampChanged(cl) => {
            app.tuning_overrides.clamp = Some(cl);
            Task::none()
        }
        Message::TuneLayerChanged(ly) => {
            app.tuning_overrides.layer = Some(ly);
            Task::none()
        }
        Message::TuneScreenshotChanged(sc) => {
            app.tuning_overrides.screenshot = Some(sc);
            Task::none()
        }
        Message::TuneBoolChanged(_key, _val) => {
            Task::none()
        }
        Message::TuneSliderChanged(key, val) => {
            app.lwe_prop_values.insert(key, val);
            Task::none()
        }
        Message::TuneComboChanged(key, val) => {
            app.lwe_prop_values.insert(key, val);
            Task::none()
        }
        Message::TuneTextChanged(key, val) => {
            app.lwe_prop_values.insert(key, val);
            Task::none()
        }
        Message::TuneClose => {
            app.tuning_wall = None;
            Task::none()
        }
        Message::SaveAndApplyTuning => {
            if let Some(wall) = app.tuning_wall.clone() {
                app.config.wallpaper_overrides.insert(wall.path.to_string_lossy().to_string(), app.tuning_overrides.clone());
                let _ = app.config.save();
                Task::perform(send_req(app.config.socket_path.clone(), IpcRequest::SetWallpaper { path: wall.path.to_string_lossy().to_string() }), Message::GotStatus)
            } else {
                Task::none()
            }
        }
    }
}

fn render_wallpaper_card<'a>(app: &'a IcedGuiApp, path: &PathBuf) -> Element<'a, Message> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("Wallpaper");
    let path_str = path.to_string_lossy().to_string();
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let ext_upper = ext.to_uppercase();
    let is_web = matches!(ext.as_str(), "html" | "htm" | "js") || path_str.starts_with("http://");

    let is_hovered_and_streaming = app.hover_streaming.as_ref() == Some(path);

    let img_elem: Element<'a, Message> = if is_hovered_and_streaming {
        let live_path = if is_web {
            PathBuf::from(HOVER_WEB_LIVE_PATH)
        } else {
            PathBuf::from(HOVER_VIDEO_LIVE_PATH)
        };

        if app.image_cache.contains_key(&live_path) || live_path.exists() {
            if let Some(cached) = app.image_cache.get(&live_path) {
                image(cached.handle.clone())
                    .width(260)
                    .height(146)
                    .into()
            } else {
                container(text(if is_web { "● LIVE WEBKIT" } else { "● LIVE MPV" }).color(EMERALD).size(14))
                    .width(260)
                    .height(146)
                    .align_x(iced::Alignment::Center)
                    .align_y(iced::Alignment::Center)
                    .into()
            }
        } else {
            container(text(if is_web { "● STARTING WEBKIT..." } else { "● STARTING MPV..." }).color(AMBER).size(13))
                .width(260)
                .height(146)
                .align_x(iced::Alignment::Center)
                .align_y(iced::Alignment::Center)
                .into()
        }
    } else if let Some(thumb_path) = get_web_thumbnail_path(&path_str) {
        if let Some(cached) = app.image_cache.get(&thumb_path) {
            image(cached.handle.clone())
                .width(260)
                .height(146)
                .into()
        } else {
            container(text(format!("[ {} ]", ext_upper)).color(AMBER).size(15))
                .width(260)
                .height(146)
                .align_x(iced::Alignment::Center)
                .align_y(iced::Alignment::Center)
                .into()
        }
    } else {
        container(text(format!("[ {} ]", ext_upper)).color(AMBER).size(15))
            .width(260)
            .height(146)
            .align_x(iced::Alignment::Center)
            .align_y(iced::Alignment::Center)
            .into()
    };

    let is_selected = app.selected_wallpaper.as_ref() == Some(path);
    let is_card_hovered = app.card_hover.get(path) == Some(&true);

    let truncated_title = if name.len() > 22 { format!("{}...", &name[..20]) } else { name.to_string() };
    let badge_color = if is_hovered_and_streaming { EMERALD } else if is_web { CYAN } else { AMBER };
    let badge_text = if is_hovered_and_streaming { "● LIVE".to_string() } else { ext_upper };

    let card_body = column![
        img_elem,
        row![
            text(truncated_title).size(13).color(if is_selected { CYAN } else { Color::WHITE }).width(Length::Fill),
            text(badge_text).size(11).color(badge_color),
        ]
        .spacing(4)
        .align_y(iced::Alignment::Center),
        button(text("▶ Set Wallpaper")).on_press(Message::ApplyPath(path.clone())),
    ]
    .spacing(6)
    .padding(6);

    let path_enter = path.clone();
    let path_exit = path.clone();
    let path_click = path.clone();

    mouse_area(
        container(card_body)
            .width(272)
            .style(move |_| {
                if is_selected {
                    container::Style {
                        background: Some(Background::Color(CARD_BG_SEL)),
                        border: Border {
                            color: CYAN,
                            width: 2.0,
                            radius: 8.0.into(),
                        },
                        ..Default::default()
                    }
                } else if is_card_hovered || is_hovered_and_streaming {
                    container::Style {
                        background: Some(Background::Color(CARD_BG_ACTIVE)),
                        border: Border {
                            color: EMERALD,
                            width: 1.5,
                            radius: 8.0.into(),
                        },
                        ..Default::default()
                    }
                } else {
                    container::Style {
                        background: Some(Background::Color(CARD_BG)),
                        border: Border {
                            color: CARD_STROKE,
                            width: 1.0,
                            radius: 8.0.into(),
                        },
                        ..Default::default()
                    }
                }
            })
    )
    .on_enter(Message::CardEntered(path_enter))
    .on_exit(Message::CardExited(path_exit))
    .on_press(Message::CardClicked(path_click))
    .into()
}

fn render_carousel_view<'a>(app: &'a IcedGuiApp, filtered: &[PathBuf]) -> Element<'a, Message> {
    if filtered.is_empty() {
        return container(text("No wallpapers match the selected category filter.").color(AMBER).size(16))
            .width(Length::Fill)
            .padding(30)
            .into();
    }

    let active_idx = app.carousel_index.min(filtered.len() - 1);
    let target = &filtered[active_idx];
    let name = target.file_name().and_then(|n| n.to_str()).unwrap_or("Wallpaper").to_string();
    let path_str = target.to_string_lossy().to_string();
    let ext = target.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let is_web = matches!(ext.as_str(), "html" | "htm" | "js") || path_str.starts_with("http://");

    let live_path = if is_web {
        PathBuf::from(HOVER_WEB_LIVE_PATH)
    } else {
        PathBuf::from(HOVER_VIDEO_LIVE_PATH)
    };

    let is_live = app.hover_streaming.as_ref() == Some(target);

    let spotlight_img: Element<'a, Message> = if is_live && (app.image_cache.contains_key(&live_path) || live_path.exists()) {
        if let Some(cached) = app.image_cache.get(&live_path) {
            image(cached.handle.clone()).width(600).height(337).into()
        } else {
            container(text("Decoding Spotlight Frame...").color(CYAN).size(16))
                .width(600)
                .height(337)
                .align_x(iced::Alignment::Center)
                .align_y(iced::Alignment::Center)
                .into()
        }
    } else if let Some(thumb_path) = get_web_thumbnail_path(&path_str) {
        if let Some(cached) = app.image_cache.get(&thumb_path) {
            image(cached.handle.clone()).width(600).height(337).into()
        } else {
            container(text(if is_web { "Rendering WebKit2GTK..." } else { "Rendering Video Preview..." }).color(AMBER).size(16))
                .width(600)
                .height(337)
                .align_x(iced::Alignment::Center)
                .align_y(iced::Alignment::Center)
                .into()
        }
    } else {
        container(text("Spotlight Player Ready").color(SOFT_TEXT).size(16))
            .width(600)
            .height(337)
            .align_x(iced::Alignment::Center)
            .align_y(iced::Alignment::Center)
            .into()
    };

    let spotlight_card = container(
        column![
            row![
                text(format!("● SPOTLIGHT PLAYER ({}/{})", active_idx + 1, filtered.len()))
                    .color(if is_live { EMERALD } else { CYAN })
                    .size(14),
                space().width(Length::Fill),
                text(if is_web { "WebKit2GTK Web App" } else { "MPV Hardware Video" }).color(AMBER).size(13),
            ]
            .spacing(12)
            .align_y(iced::Alignment::Center),
            spotlight_img,
            text(name).color(Color::WHITE).size(18),
            text(path_str).color(DIM_TEXT).size(12),
            row![
                button(text("◀ Previous")).on_press(Message::CarouselPrev),
                button(text("▶ Set Wallpaper")).on_press(Message::ApplyPath(target.clone())),
                button(text("Next ▶")).on_press(Message::CarouselNext),
            ]
            .spacing(16),
        ]
        .spacing(12)
        .padding(16)
    )
    .style(|_| container::Style {
        background: Some(Background::Color(CARD_BG_SEL)),
        border: Border {
            color: CYAN,
            width: 2.0,
            radius: 12.0.into(),
        },
        ..Default::default()
    });

    let target_enter = target.clone();
    let target_exit = target.clone();

    let interactive_spotlight = mouse_area(spotlight_card)
        .on_enter(Message::CardEntered(target_enter))
        .on_exit(Message::CardExited(target_exit));

    let mut filmstrip = row![].spacing(8);
    for (idx, w) in filtered.iter().enumerate().take(12) {
        let is_current = idx == active_idx;
        let w_name = w.file_name().and_then(|n| n.to_str()).unwrap_or("Item");
        let truncated = if w_name.len() > 14 { format!("{}...", &w_name[..12]) } else { w_name.to_string() };
        let path_clone = w.clone();
        filmstrip = filmstrip.push(
            button(text(truncated).size(12).color(if is_current { CYAN } else { Color::WHITE }))
                .on_press(Message::CardClicked(path_clone))
        );
    }

    column![
        interactive_spotlight,
        scrollable(filmstrip).width(Length::Fill),
    ]
    .spacing(16)
    .align_x(iced::Alignment::Center)
    .into()
}

fn view(app: &IcedGuiApp) -> Element<'_, Message> {
    let header_title = text("OMYWALL Wallpaper Engine v4.5")
        .size(22)
        .color(CYAN);

    let status_indicator = if let Some(st) = &app.status {
        let active = st.current_wallpaper.as_deref().unwrap_or("None");
        let truncated = if active.len() > 30 { &active[..30] } else { active };
        text(format!("● Running | Active: {}", truncated)).color(EMERALD).size(13)
    } else {
        text(&app.status_message).color(AMBER).size(13)
    };

    let gpu_color = if app.system_metrics.gpu_usage > 85.0 {
        Color::from_rgb(0.95, 0.25, 0.25)
    } else if app.system_metrics.gpu_usage > 50.0 {
        AMBER
    } else {
        EMERALD
    };

    let gpu_name_display = if app.system_metrics.gpu_name.len() > 18 {
        format!("{}...", &app.system_metrics.gpu_name[..16])
    } else if app.system_metrics.gpu_name.is_empty() {
        "GPU".to_string()
    } else {
        app.system_metrics.gpu_name.clone()
    };

    let gpu_metrics_badge = container(
        row![
            text(format!("⚡ GPU: {:.0}%", app.system_metrics.gpu_usage)).size(13).color(gpu_color),
            text(format!(" ({})", gpu_name_display)).size(11).color(SOFT_TEXT),
            text(format!(" | 🖥️ CPU: {:.0}%", app.system_metrics.cpu_usage)).size(13).color(CYAN),
            text(format!(" | 🧠 RAM: {} MB", app.system_metrics.ram_used_mb)).size(12).color(SOFT_TEXT),
        ]
        .spacing(4)
        .align_y(iced::Alignment::Center)
    )
    .padding([4, 10])
    .style(move |_| container::Style {
        background: Some(Background::Color(Color::from_rgb(0.08, 0.10, 0.16))),
        border: Border {
            color: gpu_color,
            width: 1.5,
            radius: 6.0.into(),
        },
        ..Default::default()
    });

    let header = row![
        header_title,
        space().width(Length::Fill),
        gpu_metrics_badge,
        space().width(12),
        status_indicator,
        space().width(12),
        button(text("⚙ System Doctor")).on_press(Message::ToggleDoctor),
        button(text("📋 Logs")).on_press(Message::ToggleLogs),
    ]
    .spacing(12)
    .align_y(iced::Alignment::Center);

    let tab_installed = button(text("🖼 Installed Wallpapers"))
        .on_press(Message::Tab(AppTab::Installed));
    let tab_workshop = button(text("🌐 Steam Workshop"))
        .on_press(Message::Tab(AppTab::SteamWorkshop));
    let tab_displays = button(text("📺 Display Manager"))
        .on_press(Message::Tab(AppTab::Displays));
    let tab_settings = button(text("⚙ Settings"))
        .on_press(Message::Tab(AppTab::Settings));

    let nav_bar = row![tab_installed, tab_workshop, tab_displays, tab_settings].spacing(8);

    let body_content: Element<'_, Message> = match app.active_tab {
        AppTab::Installed => {
            let filtered_wallpapers: Vec<PathBuf> = app.wallpapers.iter().filter(|w| {
                let path_str = w.to_string_lossy().to_string();
                let ext = w.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
                let is_web = matches!(ext.as_str(), "html" | "htm" | "js") || path_str.starts_with("http://");
                let is_vid = matches!(ext.as_str(), "mp4" | "mkv" | "webm" | "avi" | "mov" | "gif" | "flv");
                let is_img = matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp");

                match app.category_filter {
                    CategoryFilter::All => true,
                    CategoryFilter::Videos => is_vid,
                    CategoryFilter::WebWidgets => is_web,
                    CategoryFilter::StaticImages => is_img,
                    CategoryFilter::SteamWorkshop => true,
                }
            }).cloned().collect();

            let cat_pills = row![
                button(text(if app.category_filter == CategoryFilter::All { "[ All ]" } else { "All" }))
                    .on_press(Message::Category(CategoryFilter::All)),
                button(text(if app.category_filter == CategoryFilter::Videos { "[ 🎥 Videos ]" } else { "🎥 Videos" }))
                    .on_press(Message::Category(CategoryFilter::Videos)),
                button(text(if app.category_filter == CategoryFilter::WebWidgets { "[ 🌐 Web Apps ]" } else { "🌐 Web Apps" }))
                    .on_press(Message::Category(CategoryFilter::WebWidgets)),
                button(text(if app.category_filter == CategoryFilter::StaticImages { "[ 🖼 Images ]" } else { "🖼 Images" }))
                    .on_press(Message::Category(CategoryFilter::StaticImages)),
            ].spacing(6);

            let view_mode_toggle = row![
                button(text(if app.view_mode == ViewMode::Grid { "[ ⣿ Grid ]" } else { "⣿ Grid" }))
                    .on_press(Message::ViewMode(ViewMode::Grid)),
                button(text(if app.view_mode == ViewMode::Carousel { "[ 🎠 Carousel ]" } else { "🎠 Carousel" }))
                    .on_press(Message::ViewMode(ViewMode::Carousel)),
            ].spacing(6);

            let top_bar = row![
                text("Local Wallpaper Catalog").size(18).color(CYAN),
                space().width(12),
                cat_pills,
                space().width(Length::Fill),
                view_mode_toggle,
            ].align_y(iced::Alignment::Center);

            if app.view_mode == ViewMode::Carousel {
                column![
                    top_bar,
                    render_carousel_view(app, &filtered_wallpapers),
                ]
                .spacing(16)
                .into()
            } else {
                let mut grid_rows = column![top_bar].spacing(12);
                let mut current_row = row![].spacing(12);
                let mut count_in_row = 0;

                for w in filtered_wallpapers.iter().take(60) {
                    current_row = current_row.push(render_wallpaper_card(app, w));
                    count_in_row += 1;
                    if count_in_row == 4 {
                        grid_rows = grid_rows.push(current_row);
                        current_row = row![].spacing(12);
                        count_in_row = 0;
                    }
                }
                if count_in_row > 0 {
                    grid_rows = grid_rows.push(current_row);
                }

                scrollable(grid_rows).width(Length::Fill).into()
            }
        }
        AppTab::SteamWorkshop => {
            column![
                text("Steam Workshop Browser").size(18).color(CYAN),
                text(&app.workshop_status).color(SOFT_TEXT).size(14),
                row![
                    button(text("🔍 Refresh Workshop")).on_press(Message::WorkshopSearch),
                    button(text("📂 Scan Installed Steam Wallpapers")).on_press(Message::WorkshopRescanSteam),
                ].spacing(8),
            ]
            .spacing(12)
            .into()
        }
        AppTab::Displays => {
            let mut disp_col = column![
                text("Connected Displays").size(18).color(CYAN),
                button(text("🔄 Rescan Displays")).on_press(Message::RescanDisplays),
            ].spacing(12);

            for d in &app.displays {
                let info = format!("{} ({}x{} @ {}Hz)", d.name, d.width, d.height, d.refresh_rate);
                disp_col = disp_col.push(text(info).size(14).color(SOFT_TEXT));
            }
            scrollable(disp_col).into()
        }
        AppTab::Settings => {
            column![
                text("Engine Settings").size(18).color(CYAN),
                row![
                    text("Volume:").size(14),
                    slider(0.0..=100.0, app.volume_slider as f32, |v| Message::VolumeChanged(v as i64)).width(200),
                    text(format!("{}%", app.volume_slider)).size(14),
                ].spacing(12),
                row![
                    text("Autostart on boot:").size(14),
                    checkbox(app.autostart_enabled).on_toggle(|_| Message::AutostartToggled),
                ].spacing(12),
                row![
                    button(text("▶ Start Daemon")).on_press(Message::StartDaemon),
                    button(text("⏹ Stop Daemon")).on_press(Message::StopWallpaper),
                    button(text("⏯ Toggle Pause")).on_press(Message::TogglePause),
                ].spacing(8),
                button(text("🛠 Run Dependency Installer Script")).on_press(Message::RunInstaller),
            ]
            .spacing(16)
            .into()
        }
    };

    let content = column![
        header,
        rule::horizontal(1),
        nav_bar,
        rule::horizontal(1),
        body_content,
    ]
    .spacing(16)
    .padding(20);

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
