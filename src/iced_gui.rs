use iced::theme::Theme;
use iced::widget::{
    button, checkbox, column, container, image, mouse_area,
    row, rule, scrollable, slider, space, stack, text, text_input,
};

use iced::window;
use iced::{clipboard, Background, Border, Color, Element, Length, Subscription, Task};

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

#[allow(dead_code)]
type Elem<'a> = Element<'a, Message>;

const CYAN: Color = Color::from_rgb(0.18, 0.83, 0.78);

const EMERALD: Color = Color::from_rgb(0.06, 0.92, 0.55);
const AMBER: Color = Color::from_rgb(0.98, 0.65, 0.12);
const PURPLE: Color = Color::from_rgb(0.66, 0.33, 0.97);
const SOFT_TEXT: Color = Color::from_rgb(0.58, 0.64, 0.76);
const DIM_TEXT: Color = Color::from_rgb(0.38, 0.44, 0.56);
const CARD_BG: Color = Color::from_rgb(0.06, 0.08, 0.14);
const CARD_BG_SEL: Color = Color::from_rgb(0.09, 0.15, 0.26);
const CARD_BG_ACTIVE: Color = Color::from_rgb(0.04, 0.14, 0.10);
const CARD_STROKE: Color = Color::from_rgb(0.12, 0.16, 0.26);
#[allow(dead_code)]
const PANEL_BG: Color = Color::from_rgb(0.04, 0.05, 0.09);


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

#[allow(dead_code)]
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
pub enum AppTab {
    Installed,
    SteamWorkshop,
    Widgets,
    Displays,
    Screensaver,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetPreset {
    CyberHud,
    WifiBluetoothPill,
    MinimalClock,
    Custom,
}

impl WidgetPreset {
    pub fn label(&self) -> &'static str {
        match self {
            WidgetPreset::CyberHud => "🌐 All-in-One Cyber HUD",
            WidgetPreset::WifiBluetoothPill => "📶 WiFi & Bluetooth Pill",
            WidgetPreset::MinimalClock => "⏰ Minimal Clock & Stats",
            WidgetPreset::Custom => "🔗 Custom Widget URL",
        }
    }

    pub fn url(&self) -> &'static str {
        match self {
            WidgetPreset::CyberHud => "assets/widgets/desktop_hud.html",
            WidgetPreset::WifiBluetoothPill => "assets/widgets/wifi_bluetooth_pill.html",
            WidgetPreset::MinimalClock => "assets/widgets/minimal_clock_stats.html",
            WidgetPreset::Custom => "",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            WidgetPreset::CyberHud => "Full glassmorphic HUD: Circular CPU gauge, Memory graph, Network I/O, Battery, WiFi & Bluetooth telemetry.",
            WidgetPreset::WifiBluetoothPill => "Ultra-compact pill badge: Live WiFi SSID & signal strength, Bluetooth paired devices & hardware status.",
            WidgetPreset::MinimalClock => "Clean digital clock card: Big bold typography, Date, live CPU & RAM usage bars with AC power indicator.",
            WidgetPreset::Custom => "Load any local HTML/JS widget file or remote Web URL directly onto your desktop overlay layer.",
        }
    }
}


#[derive(Debug, PartialEq, Clone, Copy)]
pub enum CategoryFilter {
    All,
    Videos,
    WebWidgets,
    StaticImages,
    SteamWorkshop,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ViewMode {
    Grid,
    Carousel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[derive(Default)]
pub enum ThemeScheme {
    #[default]
    DarkGlass,
    SteamAmber,
    HardLightCyber,
    OledPitchBlack,
}


impl ThemeScheme {
    #[allow(dead_code)]
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

fn btn_primary<'a>(txt: impl iced::widget::text::IntoFragment<'a>) -> iced::widget::Button<'a, Message> {
    button(text(txt).size(13).color(CYAN))
        .padding([8, 14])
        .style(|_theme, status| {
            let bg = match status {
                iced::widget::button::Status::Hovered => Color::from_rgb(0.08, 0.16, 0.28),
                iced::widget::button::Status::Pressed => Color::from_rgb(0.04, 0.10, 0.20),
                _ => CARD_BG,
            };
            iced::widget::button::Style {
                background: Some(Background::Color(bg)),
                text_color: CYAN,
                border: Border {
                    color: CYAN,
                    width: 1.5,
                    radius: 8.0.into(),
                },
                shadow: iced::Shadow::default(),
                snap: true,
            }
        })
}

fn btn_tab<'a>(txt: &'a str, is_active: bool) -> iced::widget::Button<'a, Message> {
    let text_color = if is_active { CYAN } else { SOFT_TEXT };
    let border_color = if is_active { CYAN } else { CARD_STROKE };
    let bg_color = if is_active { CARD_BG_SEL } else { CARD_BG };

    button(text(txt).size(13).color(text_color))
        .padding([8, 16])
        .style(move |_theme, status| {
            let bg = match status {
                iced::widget::button::Status::Hovered => CARD_BG_SEL,
                _ => bg_color,
            };
            iced::widget::button::Style {
                background: Some(Background::Color(bg)),
                text_color,
                border: Border {
                    color: border_color,
                    width: if is_active { 2.0 } else { 1.0 },
                    radius: 8.0.into(),
                },
                shadow: iced::Shadow::default(),
                snap: true,
            }
        })
}

fn btn_pill<'a>(txt: impl iced::widget::text::IntoFragment<'a>, is_active: bool) -> iced::widget::Button<'a, Message> {

    let text_color = if is_active { Color::from_rgb(0.04, 0.06, 0.10) } else { SOFT_TEXT };
    let bg_color = if is_active { CYAN } else { CARD_BG };

    button(text(txt).size(12).color(text_color))
        .padding([6, 14])
        .style(move |_theme, status| {
            let bg = match status {
                iced::widget::button::Status::Hovered => if is_active { CYAN } else { CARD_BG_SEL },
                _ => bg_color,
            };
            iced::widget::button::Style {
                background: Some(Background::Color(bg)),
                text_color,
                border: Border {
                    color: if is_active { CYAN } else { CARD_STROKE },
                    width: 1.0,
                    radius: 12.0.into(),
                },
                shadow: iced::Shadow::default(),
                snap: true,
            }
        })
}


fn btn_danger<'a>(txt: impl iced::widget::text::IntoFragment<'a>) -> iced::widget::Button<'a, Message> {
    let danger_red = Color::from_rgb(0.95, 0.25, 0.25);
    button(text(txt).size(13).color(danger_red))
        .padding([8, 14])
        .style(move |_theme, status| {
            let bg = match status {
                iced::widget::button::Status::Hovered => Color::from_rgb(0.25, 0.08, 0.08),
                _ => CARD_BG,
            };
            iced::widget::button::Style {
                background: Some(Background::Color(bg)),
                text_color: danger_red,
                border: Border {
                    color: danger_red,
                    width: 1.5,
                    radius: 8.0.into(),
                },
                shadow: iced::Shadow::default(),
                snap: true,
            }
        })
}

fn btn_secondary<'a>(txt: impl iced::widget::text::IntoFragment<'a>) -> iced::widget::Button<'a, Message> {
    button(text(txt).size(13).color(SOFT_TEXT))
        .padding([8, 14])
        .style(|_theme, status| {
            let bg = match status {
                iced::widget::button::Status::Hovered => CARD_BG_SEL,
                _ => CARD_BG,
            };
            iced::widget::button::Style {
                background: Some(Background::Color(bg)),
                text_color: SOFT_TEXT,
                border: Border {
                    color: CARD_STROKE,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                shadow: iced::Shadow::default(),
                snap: true,
            }
        })
}



// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum Message {
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
    OpenFolderPicker,
    OpenFilePicker,
    FolderPicked(Option<PathBuf>),
    FilePicked(Option<PathBuf>),
    OpenWebPreview(String),
    OpenVideoPreview(String),
    OpenImagePreview(String),



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
    WorkshopOpenBrowser(String),

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
    SetMonitorWallpaper(String, String),
    ClearMonitorWallpaper(String),

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
    RecheckDoctor,
    ToggleLogs,
    RefreshLogs,
    ClearLogs,
    CopyLogs,
    ToggleHyprlock,
    ToggleGpuSettings,
    TestScreensaver,
    SaveHyprlockConf,
    ScreensaverEnabledToggled(bool),
    ScreensaverModeChanged(String),
    ScreensaverClockColorChanged(String),
    RunInstaller,
    SlideshowIntervalChanged(u64),
    SlideshowShuffleToggled,
    StartSlideshow,
    StopSlideshow,


    // Desktop Widgets
    ToggleWidgetOverlay,
    SetWidgetEnabled(bool),
    SelectWidgetPreset(WidgetPreset),
    SelectWidgetPosition(String),
    CustomWidgetUrlChanged(String),
    ApplyWidgetToDesktop,
    TestWidgetWindow,
    StopWidgetOverlay,
    OpenWidgetFilePicker,
    WidgetFilePicked(Option<PathBuf>),

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

pub struct CachedImage {
    pub mtime: SystemTime,
    pub handle: iced::widget::image::Handle,
}

pub struct IcedGuiApp {
    pub config: Config,
    pub window_id: Option<window::Id>,
    pub status: Option<DaemonStatus>,
    pub status_message: String,

    pub active_tab: AppTab,
    pub theme_scheme: ThemeScheme,

    pub wallpapers: Vec<PathBuf>,
    pub selected_wallpaper: Option<PathBuf>,
    pub search_filter: String,
    pub category_filter: CategoryFilter,
    pub view_mode: ViewMode,
    pub carousel_index: usize,

    pub web_url_input: String,
    pub new_web_title: String,
    pub new_web_category: String,

    pub widget_preset: WidgetPreset,
    pub custom_widget_url: String,

    pub steam_wallpapers: Vec<SteamWallpaper>,
    pub displays: Vec<DisplayInfo>,
    pub selected_screen: Option<String>,

    pub volume_slider: i64,
    pub opacity_slider: f32,
    pub autostart_enabled: bool,

    pub show_doctor: bool,
    pub show_logs: bool,
    pub show_hyprlock: bool,
    pub show_gpu_settings: bool,
    pub logs_content: String,

    pub workshop_items: Vec<WorkshopItem>,
    pub workshop_page: u32,
    pub workshop_sort: String,
    pub workshop_days: i64,
    pub workshop_query: String,
    pub workshop_loading: bool,
    pub workshop_status: String,
    pub workshop_downloading: Option<String>,

    pub tuning_wall: Option<SteamWallpaper>,
    pub tuning_overrides: WallpaperOverrides,
    pub lwe_props: Vec<crate::lwe::WallpaperProperty>,
    pub lwe_prop_values: HashMap<String, String>,
    pub lwe_props_busy: bool,
    pub lwe_props_status: String,

    pub system_metrics: SystemMetrics,
    #[allow(dead_code)]
    pub last_poll: Option<Instant>,
    pub last_metrics_poll: Option<Instant>,

    pub card_hover: HashMap<PathBuf, bool>,
    pub hover_streaming: Option<PathBuf>,
    pub hover_video_process: Option<Child>,
    pub madamiru_player: Option<crate::video_render::MadamiruVideoPlayer>,
    pub hover_stream_mtime: Option<SystemTime>,

    pub image_cache: HashMap<PathBuf, CachedImage>,
    pub pending_decodes: HashSet<PathBuf>,
    pub last_click: Option<(PathBuf, Instant)>,

    pub start_minimized: bool,
    pub minimized_on_launch_done: bool,
    pub is_pinned_on_top: bool,
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
        if let Ok(img) = open(path) {
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
    pub fn new(config: Config, start_minimized: bool) -> Self {
        let current_widget_url = config.widget_url.clone().unwrap_or_default();
        let widget_preset = if current_widget_url.contains("wifi_bluetooth_pill.html") {
            WidgetPreset::WifiBluetoothPill
        } else if current_widget_url.contains("minimal_clock_stats.html") {
            WidgetPreset::MinimalClock
        } else if current_widget_url.contains("desktop_hud.html") || current_widget_url.is_empty() {
            WidgetPreset::CyberHud
        } else {
            WidgetPreset::Custom
        };
        let custom_widget_url = if widget_preset == WidgetPreset::Custom {
            current_widget_url
        } else {
            "assets/widgets/desktop_hud.html".to_string()
        };

        let wallpapers = Self::scan_wallpapers(&config.wallpaper_dir, &config.saved_web_wallpapers);
        let selected_wallpaper = wallpapers.first().cloned();

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
            widget_preset,
            custom_widget_url,
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
        let tasks = vec![
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

    pub fn scan_wallpapers(dir: &Path, bookmarks: &[crate::config::WebBookmark]) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let mut seen = HashSet::new();
        let valid_exts = ["mkv", "mp4", "webm", "avi", "mov", "gif", "html", "htm", "js", "pkg", "m4v", "flv", "wmv", "png", "jpg", "jpeg", "webp"];

        let _ = std::fs::create_dir_all(dir);

        fn walk_dir(d: &Path, depth: usize, files: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, valid_exts: &[&str]) {
            if depth > 4 {
                return;
            }
            let dir_name = d.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if dir_name.starts_with('.')
                || dir_name.eq_ignore_ascii_case("docs")
                || dir_name.eq_ignore_ascii_case("documentation")
                || dir_name.eq_ignore_ascii_case("test")
                || dir_name.eq_ignore_ascii_case("tests")
                || dir_name.eq_ignore_ascii_case("node_modules")
                || dir_name.eq_ignore_ascii_case("manual")
                || dir_name.eq_ignore_ascii_case("devtools")
                || dir_name.eq_ignore_ascii_case("coverage")
            {
                return;
            }

            if d.join("project.json").exists() {
                let canon = std::fs::canonicalize(d).unwrap_or_else(|_| d.to_path_buf());
                if seen.insert(canon.clone()) {
                    files.push(canon);
                }
                return;
            }

            if let Ok(entries) = std::fs::read_dir(d) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                            if valid_exts.contains(&ext.to_lowercase().as_str()) {
                                let canon = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                                if seen.insert(canon.clone()) {
                                    files.push(canon);
                                }
                            }
                        }
                    } else if path.is_dir() {
                        let sub_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                        if sub_name.starts_with('.')
                            || sub_name.eq_ignore_ascii_case("docs")
                            || sub_name.eq_ignore_ascii_case("documentation")
                            || sub_name.eq_ignore_ascii_case("test")
                            || sub_name.eq_ignore_ascii_case("tests")
                            || sub_name.eq_ignore_ascii_case("node_modules")
                            || sub_name.eq_ignore_ascii_case("manual")
                            || sub_name.eq_ignore_ascii_case("devtools")
                            || sub_name.eq_ignore_ascii_case("coverage")
                        {
                            continue;
                        }
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

        if let Some(web_dir) = crate::config::resolve_web_assets_dir() {
            if web_dir.exists() && web_dir != dir {
                walk_dir(&web_dir, 0, &mut files, &mut seen, &valid_exts);
            }
        }

        if let Some(widget_dir) = crate::config::resolve_widgets_dir() {
            if widget_dir.exists() && widget_dir != dir {
                walk_dir(&widget_dir, 0, &mut files, &mut seen, &valid_exts);
            }
        }

        for bm in bookmarks {
            let url = bm.url.trim();
            if url.starts_with("http://") || url.starts_with("https://") {
                let p = PathBuf::from(url);
                if seen.insert(p.clone()) {
                    files.push(p);
                }
            } else {
                let resolved = crate::config::resolve_asset_path(url);
                let p = Path::new(&resolved);
                if p.exists() {
                    let canon = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
                    if seen.insert(canon.clone()) {
                        files.push(canon);
                    }
                } else {
                    let p_raw = PathBuf::from(url);
                    if seen.insert(p_raw.clone()) {
                        files.push(p_raw);
                    }
                }
            }
        }

        files.sort_by_key(|f| f.file_name().unwrap_or_default().to_string_lossy().to_string());
        files
    }
}

pub fn truncate_text(s: &str, max_chars: usize) -> String {
    let char_count = s.chars().count();
    if char_count > max_chars {
        let truncated: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{}...", truncated)
    } else {
        s.to_string()
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
    let mut res = send_ipc_request(&socket, &req).await;
    if res.is_err() {
        if let Ok(exe) = std::env::current_exe() {
            let _ = std::process::Command::new(exe).arg("daemon").spawn();
            tokio::time::sleep(std::time::Duration::from_millis(350)).await;
            res = send_ipc_request(&socket, &req).await;
        }
    }
    match send_ipc_request(&socket, &IpcRequest::GetStatus).await {
        Ok(IpcResponse::Status(st)) => Ok(st),
        Ok(_) => Err(res.err().unwrap_or_else(|| "unexpected response".to_string())),
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
        let max_x = 145 + ((y - 70) * 35 / 40);
        for x in 145..=max_x.min(185) {
            if (0..320).contains(&x) && y < 180 {
                imgbuf.put_pixel(x as u32, y as u32, ::image::Rgb([br, bg, bb]));
            }
        }
    }
    let _ = imgbuf.save(target_path);
}

fn get_thumbnail_path(video_path: &Path) -> Option<PathBuf> {
    let cache_dir = PathBuf::from("/tmp/omywall_thumbs");
    let _ = std::fs::create_dir_all(&cache_dir);

    // Handle Directory (Steam Workshop Item or Web Wallpaper Folder)
    if video_path.is_dir() {
        let project_file = video_path.join("project.json");
        if project_file.exists() {
            if let Ok(content) = std::fs::read_to_string(&project_file) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(preview) = json.get("preview").and_then(|v| v.as_str()) {
                        let p = video_path.join(preview);
                        if p.exists() {
                            return Some(p);
                        }
                    }
                    let w_type = json.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    let w_file = json.get("file").and_then(|v| v.as_str()).unwrap_or("");
                    if w_type == "video" && !w_file.is_empty() {
                        let p = video_path.join(w_file);
                        if p.exists() {
                            return get_thumbnail_path(&p);
                        }
                    } else if w_type == "web" && !w_file.is_empty() {
                        let p = video_path.join(w_file);
                        if p.exists() {
                            return get_web_thumbnail_path(&p.to_string_lossy());
                        }
                    }
                }
            }
        }

        let candidates = ["preview.jpg", "preview.png", "preview.gif", "preview.jpeg", "preview.webp", "thumb.jpg", "thumb.png", "thumbnail.jpg", "cover.jpg"];
        for cand in candidates {
            let p = video_path.join(cand);
            if p.exists() {
                return Some(p);
            }
        }

        if let Some(html_entry) = crate::config::find_primary_html_entry(video_path) {
            return get_web_thumbnail_path(&html_entry.to_string_lossy());
        }

        if let Ok(entries) = std::fs::read_dir(video_path) {
            for entry in entries.flatten() {
                let p = entry.path();
                let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
                if matches!(ext.as_str(), "mp4" | "webm" | "mkv" | "mov" | "avi") {
                    return get_thumbnail_path(&p);
                }
            }
        }
    }

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

    if path.is_dir() {
        if let Some(html_entry) = crate::config::find_primary_html_entry(path) {
            return get_web_thumbnail_path(&html_entry.to_string_lossy());
        }
        return get_thumbnail_path(path);
    }

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();

    if matches!(ext.as_str(), "mp4" | "mkv" | "webm" | "avi" | "mov" | "gif" | "png" | "jpg" | "jpeg" | "webp") {
        return get_thumbnail_path(path);
    }

    let cache_dir = PathBuf::from("/tmp/omywall_thumbs");
    let _ = std::fs::create_dir_all(&cache_dir);

    let hash = format!("{:x}", md5_hash(resolved.as_bytes()));
    let thumb_file = cache_dir.join(format!("web_{}.png", &hash[..8]));

    if !thumb_file.exists() {
        crate::electron_preview::render_shot(&resolved, &thumb_file);
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
        crate::electron_preview::start_live(&path_str, Path::new(HOVER_WEB_LIVE_PATH));
    } else if is_video {
        if let Ok(player) = crate::video_render::MadamiruVideoPlayer::new(hovered) {
            app.madamiru_player = Some(player);
        } else {
            start_hover_video_stream(app, hovered);
        }
    }
}

fn stop_hover_stream(app: &mut IcedGuiApp) {
    crate::electron_preview::stop_live();
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

#[derive(Clone, Debug)]
pub struct ToolStatus {
    pub name: String,
    pub description: String,
    pub installed: bool,
    pub path_or_info: String,
}

pub fn find_tool_path(cmd: &str) -> Option<PathBuf> {
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let p = dir.join(cmd);
            if p.exists() {
                return Some(p);
            }
        }
    }
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/home/user"));
    [
        home.join(".local/bin").join(cmd),
        home.join(".cargo/bin").join(cmd),
        PathBuf::from("/usr/bin").join(cmd),
        PathBuf::from("/usr/local/bin").join(cmd),
        PathBuf::from("/bin").join(cmd),
    ]
    .into_iter()
    .find(|c| c.exists())
}

pub fn check_installed_tools() -> Vec<ToolStatus> {
    let mut list = Vec::new();

    // 1. mpvpaper
    if let Some(p) = find_tool_path("mpvpaper") {
        list.push(ToolStatus {
            name: "mpvpaper".to_string(),
            description: "Primary Wayland video wallpaper renderer (wlr-layer-shell protocol)".to_string(),
            installed: true,
            path_or_info: p.to_string_lossy().to_string(),
        });
    } else {
        list.push(ToolStatus {
            name: "mpvpaper".to_string(),
            description: "Primary Wayland video wallpaper renderer (wlr-layer-shell protocol)".to_string(),
            installed: false,
            path_or_info: "Missing - install via pacman or cargo".to_string(),
        });
    }

    // 2. mpv
    if let Some(p) = find_tool_path("mpv") {
        list.push(ToolStatus {
            name: "mpv".to_string(),
            description: "Hardware-accelerated media player engine & IPC control socket".to_string(),
            installed: true,
            path_or_info: p.to_string_lossy().to_string(),
        });
    } else {
        list.push(ToolStatus {
            name: "mpv".to_string(),
            description: "Hardware-accelerated media player engine & IPC control socket".to_string(),
            installed: false,
            path_or_info: "Missing - install via package manager".to_string(),
        });
    }

    // 3. ffmpeg
    if let Some(p) = find_tool_path("ffmpeg") {
        list.push(ToolStatus {
            name: "ffmpeg".to_string(),
            description: "Video thumbnail extraction & media transcode pipeline".to_string(),
            installed: true,
            path_or_info: p.to_string_lossy().to_string(),
        });
    } else {
        list.push(ToolStatus {
            name: "ffmpeg".to_string(),
            description: "Video thumbnail extraction & media transcode pipeline".to_string(),
            installed: false,
            path_or_info: "Missing - install via package manager".to_string(),
        });
    }

    // 4. electron
    if let Some(p) = find_tool_path("electron") {
        list.push(ToolStatus {
            name: "electron".to_string(),
            description: "Desktop web streams & widget overlay runtime engine".to_string(),
            installed: true,
            path_or_info: p.to_string_lossy().to_string(),
        });
    } else {
        list.push(ToolStatus {
            name: "electron".to_string(),
            description: "Desktop web streams & widget overlay runtime engine".to_string(),
            installed: false,
            path_or_info: "Missing - install electron".to_string(),
        });
    }

    // 5. chromium / chrome
    let chromium_candidates = ["chromium", "google-chrome-stable", "google-chrome", "brave-browser", "chromium-browser"];
    let chromium_found = chromium_candidates.iter().find_map(|&c| find_tool_path(c));
    if let Some(p) = chromium_found {
        list.push(ToolStatus {
            name: "chromium / chrome".to_string(),
            description: "Web engine fallback for HTML5 & WebGL live wallpaper scenes".to_string(),
            installed: true,
            path_or_info: p.to_string_lossy().to_string(),
        });
    } else {
        list.push(ToolStatus {
            name: "chromium".to_string(),
            description: "Web engine fallback for HTML5 & WebGL live wallpaper scenes".to_string(),
            installed: false,
            path_or_info: "Missing - install chromium or google-chrome".to_string(),
        });
    }

    // 6. webkit2gtk
    let webkit_paths = [
        "/usr/lib/libwebkit2gtk-4.1.so",
        "/usr/lib/libwebkit2gtk-4.0.so",
        "/usr/lib/x86_64-linux-gnu/libwebkit2gtk-4.1.so",
        "/usr/lib/x86_64-linux-gnu/libwebkit2gtk-4.0.so",
        "/usr/lib64/libwebkit2gtk-4.1.so",
        "/usr/lib64/libwebkit2gtk-4.0.so",
    ];
    let webkit_found = webkit_paths.iter().find(|&&p| Path::new(p).exists());
    let webkit_pkg = if webkit_found.is_none() {
        Command::new("pkg-config")
            .args(["--exists", "webkit2gtk-4.1"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else {
        true
    };

    if let Some(&p) = webkit_found {
        list.push(ToolStatus {
            name: "webkit2gtk".to_string(),
            description: "WebKitGTK hardware-accelerated embedded web engine".to_string(),
            installed: true,
            path_or_info: p.to_string(),
        });
    } else if webkit_pkg {
        list.push(ToolStatus {
            name: "webkit2gtk".to_string(),
            description: "WebKitGTK hardware-accelerated embedded web engine".to_string(),
            installed: true,
            path_or_info: "libwebkit2gtk-4.1 (System Library)".to_string(),
        });
    } else {
        list.push(ToolStatus {
            name: "webkit2gtk".to_string(),
            description: "WebKitGTK hardware-accelerated embedded web engine".to_string(),
            installed: false,
            path_or_info: "Missing - install webkit2gtk-4.1 library".to_string(),
        });
    }

    // 7. hyprctl / swaymsg
    let comp_tools = ["hyprctl", "swaymsg", "niri", "riverctl"];
    let comp_found = comp_tools.iter().find_map(|&c| find_tool_path(c));
    if let Some(p) = comp_found {
        list.push(ToolStatus {
            name: "hyprctl / swaymsg".to_string(),
            description: "Wayland compositor IPC & display geometry controller".to_string(),
            installed: true,
            path_or_info: p.to_string_lossy().to_string(),
        });
    } else {
        list.push(ToolStatus {
            name: "hyprctl / swaymsg".to_string(),
            description: "Wayland compositor IPC & display geometry controller".to_string(),
            installed: false,
            path_or_info: "Missing - no active Wayland IPC controller found".to_string(),
        });
    }

    // 8. steamcmd / steam
    let steam_candidates = ["steamcmd", "steam"];
    let steam_found = steam_candidates.iter().find_map(|&c| find_tool_path(c));
    if let Some(p) = steam_found {
        list.push(ToolStatus {
            name: "steamcmd / steam".to_string(),
            description: "Steam Workshop wallpaper scanner & background asset downloader".to_string(),
            installed: true,
            path_or_info: p.to_string_lossy().to_string(),
        });
    } else {
        list.push(ToolStatus {
            name: "steamcmd".to_string(),
            description: "Steam Workshop wallpaper scanner & background asset downloader".to_string(),
            installed: false,
            path_or_info: "Optional - install steamcmd for direct workshop downloading".to_string(),
        });
    }

    // 9. nmcli
    if let Some(p) = find_tool_path("nmcli") {
        list.push(ToolStatus {
            name: "nmcli".to_string(),
            description: "NetworkManager CLI for desktop telemetry & WiFi overlay widgets".to_string(),
            installed: true,
            path_or_info: p.to_string_lossy().to_string(),
        });
    } else {
        list.push(ToolStatus {
            name: "nmcli".to_string(),
            description: "NetworkManager CLI for desktop telemetry & WiFi overlay widgets".to_string(),
            installed: false,
            path_or_info: "Missing - install networkmanager".to_string(),
        });
    }

    // 10. bluetoothctl
    if let Some(p) = find_tool_path("bluetoothctl") {
        list.push(ToolStatus {
            name: "bluetoothctl".to_string(),
            description: "BlueZ Bluetooth controller for desktop status widgets".to_string(),
            installed: true,
            path_or_info: p.to_string_lossy().to_string(),
        });
    } else {
        list.push(ToolStatus {
            name: "bluetoothctl".to_string(),
            description: "BlueZ Bluetooth controller for desktop status widgets".to_string(),
            installed: false,
            path_or_info: "Missing - install bluez-utils".to_string(),
        });
    }

    // 11. GPU Drivers & Acceleration
    let has_dri = Path::new("/dev/dri/renderD128").exists() || Path::new("/dev/dri/card0").exists();
    let has_nvidia = find_tool_path("nvidia-smi").is_some();
    if has_dri || has_nvidia {
        let info = if has_nvidia {
            "NVIDIA GPU Driver & NVDEC / NVENC Active"
        } else {
            "Direct Rendering Manager (/dev/dri/renderD128 - VA-API/Vulkan)"
        };
        list.push(ToolStatus {
            name: "GPU Drivers (VA-API / NVDEC)".to_string(),
            description: "Hardware video decoding & zero-copy GPU wallpaper rendering".to_string(),
            installed: true,
            path_or_info: info.to_string(),
        });
    } else {
        list.push(ToolStatus {
            name: "GPU Drivers".to_string(),
            description: "Hardware video decoding & zero-copy GPU wallpaper rendering".to_string(),
            installed: false,
            path_or_info: "No /dev/dri/renderD128 or nvidia device found".to_string(),
        });
    }

    // 12. hyprlock / swaylock
    let lock_tools = ["hyprlock", "swaylock", "gtklock", "waylock"];
    let lock_found = lock_tools.iter().find_map(|&c| find_tool_path(c));
    if let Some(p) = lock_found {
        list.push(ToolStatus {
            name: "hyprlock / swaylock".to_string(),
            description: "Wayland GPU-accelerated screen locker & animated screensaver".to_string(),
            installed: true,
            path_or_info: p.to_string_lossy().to_string(),
        });
    } else {
        list.push(ToolStatus {
            name: "hyprlock".to_string(),
            description: "Wayland GPU-accelerated screen locker & animated screensaver".to_string(),
            installed: false,
            path_or_info: "Missing - install hyprlock or swaylock for screensavers".to_string(),
        });
    }

    // 13. notify-send
    if let Some(p) = find_tool_path("notify-send") {
        list.push(ToolStatus {
            name: "notify-send".to_string(),
            description: "Desktop notification daemon client (libnotify)".to_string(),
            installed: true,
            path_or_info: p.to_string_lossy().to_string(),
        });
    } else {
        list.push(ToolStatus {
            name: "notify-send".to_string(),
            description: "Desktop notification daemon client (libnotify)".to_string(),
            installed: false,
            path_or_info: "Missing - install libnotify".to_string(),
        });
    }

    // 14. jq
    if let Some(p) = find_tool_path("jq") {
        list.push(ToolStatus {
            name: "jq".to_string(),
            description: "High-performance JSON processor for IPC & events".to_string(),
            installed: true,
            path_or_info: p.to_string_lossy().to_string(),
        });
    } else {
        list.push(ToolStatus {
            name: "jq".to_string(),
            description: "High-performance JSON processor for IPC & events".to_string(),
            installed: false,
            path_or_info: "Missing - install jq".to_string(),
        });
    }

    list
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

#[allow(dead_code)]
fn fmt_f64(v: f64) -> String {
    let rounded = (v * 100000.0).round() / 100000.0;
    format!("{}", rounded)
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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
    Subscription::batch([
        iced::time::every(Duration::from_millis(500)).map(|_| Message::Tick),
        window::events().map(|(id, event)| Message::WindowEvent(id, event)),
    ])
}


fn update(app: &mut IcedGuiApp, message: Message) -> Task<Message> {
    match message {
        Message::Tick => {
            let now = Instant::now();
            if app.last_metrics_poll.is_none_or(|t| now.duration_since(t) >= Duration::from_secs(1)) {
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

            // Keep the spotlight player previewing the active catalog item
            // (HTML/web and video) even without hovering.
            if app.hover_streaming.is_none() {
                if let Some(active) = active_carousel_item(app) {
                    if is_live_item(&active) {
                        manage_hover_stream(app, &active);
                    }
                }
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

            // Decode live web preview frame if updated (batched with catalog
            // decodes so a continuously-updating live stream can't starve the
            // wallpaper thumbnail queue).
            let mut decode_tasks: Vec<Task<Message>> = Vec::new();
            let web_live = PathBuf::from(HOVER_WEB_LIVE_PATH);
            if web_live.exists() {
                let mtime = std::fs::metadata(&web_live).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
                let cached_mtime = app.image_cache.get(&web_live).map(|c| c.mtime).unwrap_or(SystemTime::UNIX_EPOCH);
                if mtime > cached_mtime && !app.pending_decodes.contains(&web_live) {
                    app.pending_decodes.insert(web_live.clone());
                    decode_tasks.push(Task::perform(decode_thumb(web_live), |(p, m, r)| Message::ThumbDecoded(p, m, r)));
                }
            }

            // Asynchronously decode catalog wallpaper thumbnails, prioritizing
            // the wallpaper currently shown in the spotlight player.
            let filtered = filtered_wallpapers(app);
            if !filtered.is_empty() {
                let active_idx = app.carousel_index.min(filtered.len() - 1);
                if let Some(thumb_path) = get_web_thumbnail_path(&filtered[active_idx].to_string_lossy()) {
                    if thumb_path.exists() && !app.image_cache.contains_key(&thumb_path) && !app.pending_decodes.contains(&thumb_path) {
                        app.pending_decodes.insert(thumb_path.clone());
                        decode_tasks.push(Task::perform(decode_thumb(thumb_path), |(p, m, r)| Message::ThumbDecoded(p, m, r)));
                    }
                }
            }
            for w in app.wallpapers.iter().take(60) {
                if let Some(thumb_path) = get_web_thumbnail_path(&w.to_string_lossy()) {
                    if thumb_path.exists() && !app.image_cache.contains_key(&thumb_path) && !app.pending_decodes.contains(&thumb_path) {
                        app.pending_decodes.insert(thumb_path.clone());
                        decode_tasks.push(Task::perform(decode_thumb(thumb_path), |(p, m, r)| Message::ThumbDecoded(p, m, r)));
                        break;
                    }
                }
            }

            if app.active_tab == AppTab::SteamWorkshop {
                for item in app.workshop_items.iter().take(60) {
                    if let Some(thumb_path) = crate::steam_workshop::cached_preview_path(item) {
                        if thumb_path.exists() && !app.image_cache.contains_key(&thumb_path) && !app.pending_decodes.contains(&thumb_path) {
                            app.pending_decodes.insert(thumb_path.clone());
                            decode_tasks.push(Task::perform(decode_thumb(thumb_path), |(p, m, r)| Message::ThumbDecoded(p, m, r)));
                            break;
                        }
                    }
                }
            }

            if !decode_tasks.is_empty() {
                return Task::batch(decode_tasks);
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
                    return window::close::<Message>(win_id);
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
            for item in &items {
                if !app.wallpapers.contains(&item.path) {
                    app.wallpapers.push(item.path.clone());
                }
            }
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
                    for item in &items {
                        if crate::steam_workshop::cached_preview_path(item).is_none() && item.preview_url.is_some() {
                            crate::steam_workshop::request_preview_image(item);
                        }
                    }
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
        Message::OpenFolderPicker => {
            Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .set_title("Select Wallpaper Directory")
                        .pick_folder()
                        .await
                        .map(|f| f.path().to_path_buf())
                },
                Message::FolderPicked,
            )
        }
        Message::OpenFilePicker => {
            Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .set_title("Select External Wallpaper File (Video, Web HTML, Image)")
                        .add_filter(
                            "All Supported Wallpapers",
                            &["mp4", "mkv", "webm", "avi", "mov", "gif", "html", "htm", "png", "jpg", "jpeg", "webp"],
                        )
                        .add_filter("Videos", &["mp4", "mkv", "webm", "avi", "mov", "gif"])
                        .add_filter("Web / HTML", &["html", "htm", "js"])
                        .add_filter("Images", &["png", "jpg", "jpeg", "webp"])
                        .pick_file()
                        .await
                        .map(|f| f.path().to_path_buf())
                },
                Message::FilePicked,
            )
        }
        Message::FolderPicked(Some(dir)) => {
            app.config.wallpaper_dir = dir.clone();
            let _ = app.config.save();
            app.wallpapers = IcedGuiApp::scan_wallpapers(&dir, &app.config.saved_web_wallpapers);
            app.selected_wallpaper = app.wallpapers.first().cloned();
            Task::none()
        }
        Message::FolderPicked(None) => Task::none(),
        Message::FilePicked(Some(file_path)) => {
            if !app.wallpapers.contains(&file_path) {
                app.wallpapers.insert(0, file_path.clone());
            }
            app.selected_wallpaper = Some(file_path.clone());
            Task::perform(
                send_req(
                    app.config.socket_path.clone(),
                    IpcRequest::SetWallpaper {
                        path: file_path.to_string_lossy().to_string(),
                    },
                ),
                Message::GotStatus,
            )
        }
        Message::FilePicked(None) => Task::none(),
        Message::OpenWebPreview(url) => {
            let target_url = if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("file://") {
                url
            } else {
                format!("file://{}", crate::config::resolve_asset_path(&url))
            };
            std::thread::spawn(move || {
                if Command::new("electron").args(["--title=OMYWALL Web Preview", &target_url]).spawn().is_err()
                    && Command::new("chromium").args([format!("--app={}", target_url)]).spawn().is_err()
                        && Command::new("google-chrome").args([format!("--app={}", target_url)]).spawn().is_err() {
                            let _ = Command::new("firefox").args([&target_url]).spawn();
                        }
            });
            Task::none()
        }
        Message::OpenVideoPreview(path) => {
            std::thread::spawn(move || {
                let _ = Command::new("mpv").args(["--title=OMYWALL Video Preview", "--autofit=640x360", &path]).spawn();
            });
            Task::none()
        }
        Message::OpenImagePreview(path) => {
            std::thread::spawn(move || {
                if Command::new("imv").arg(&path).spawn().is_err()
                    && Command::new("mpv").args(["--title=OMYWALL Image Preview", &path]).spawn().is_err() {
                        let _ = Command::new("xdg-open").arg(&path).spawn();
                    }
            });
            Task::none()
        }


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
            app.carousel_index = 0;
            Task::none()
        }
        Message::ViewMode(mode) => {
            app.view_mode = mode;
            Task::none()
        }
        Message::CarouselNext => {
            let filtered = filtered_wallpapers(app);
            if !filtered.is_empty() {
                app.carousel_index = (app.carousel_index + 1) % filtered.len();
                if let Some(active) = active_carousel_item(app) {
                    if app.hover_streaming.as_deref() != Some(&active) {
                        stop_hover_stream(app);
                    }
                }
            }
            Task::none()
        }
        Message::CarouselPrev => {
            let filtered = filtered_wallpapers(app);
            if !filtered.is_empty() {
                if app.carousel_index == 0 {
                    app.carousel_index = filtered.len() - 1;
                } else {
                    app.carousel_index -= 1;
                }
                if let Some(active) = active_carousel_item(app) {
                    if app.hover_streaming.as_deref() != Some(&active) {
                        stop_hover_stream(app);
                    }
                }
            }
            Task::none()
        }
        Message::SearchFilterChanged(text) => {
            app.search_filter = text;
            app.carousel_index = 0;
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
            let is_double_click = if let Some((ref last_p, last_t)) = app.last_click {
                last_p == &path && last_t.elapsed() < Duration::from_millis(400)
            } else {
                false
            };

            app.last_click = Some((path.clone(), Instant::now()));
            app.selected_wallpaper = Some(path.clone());
            let filtered = filtered_wallpapers(app);
            if let Some(idx) = filtered.iter().position(|w| w == &path) {
                app.carousel_index = idx;
            }
            if app.hover_streaming.as_deref() != Some(&path) {
                stop_hover_stream(app);
            }

            if is_double_click {
                let path_str = path.to_string_lossy().to_string();
                if path_str.starts_with("http://") || path_str.starts_with("https://") {
                    return Task::perform(send_req(app.config.socket_path.clone(), IpcRequest::SetUrl { url: path_str }), Message::GotStatus);
                } else {
                    return Task::perform(send_req(app.config.socket_path.clone(), IpcRequest::SetWallpaper { path: path_str }), Message::GotStatus);
                }
            }

            Task::none()
        }

        Message::CardDoubleClicked(path) => {
            app.selected_wallpaper = Some(path.clone());
            let filtered = filtered_wallpapers(app);
            if let Some(idx) = filtered.iter().position(|w| w == &path) {
                app.carousel_index = idx;
            }
            if app.hover_streaming.as_deref() != Some(&path) {
                stop_hover_stream(app);
            }
            let path_str = path.to_string_lossy().to_string();
            if path_str.starts_with("http://") || path_str.starts_with("https://") {
                Task::perform(send_req(app.config.socket_path.clone(), IpcRequest::SetUrl { url: path_str }), Message::GotStatus)
            } else {
                Task::perform(send_req(app.config.socket_path.clone(), IpcRequest::SetWallpaper { path: path_str }), Message::GotStatus)
            }
        }

        Message::ApplyPath(path) => {
            let path_str = path.to_string_lossy().to_string();
            if path_str.starts_with("http://") || path_str.starts_with("https://") {
                Task::perform(send_req(app.config.socket_path.clone(), IpcRequest::SetUrl { url: path_str }), Message::GotStatus)
            } else {
                Task::perform(send_req(app.config.socket_path.clone(), IpcRequest::SetWallpaper { path: path_str }), Message::GotStatus)
            }
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
            app.workshop_sort = sort.clone();
            if sort == "trend" {
                app.workshop_days = 7;
            } else {
                app.workshop_days = 0;
            }
            app.workshop_page = 1;
            app.workshop_loading = true;
            let q = app.workshop_query.clone();
            let p = app.workshop_page;
            let s = app.workshop_sort.clone();
            let d = app.workshop_days;
            Task::perform(browse(false, q, p, s, d), Message::GotWorkshop)
        }
        Message::WorkshopDaysChanged(days) => {
            app.workshop_days = days;
            app.workshop_page = 1;
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
            if let Some(path) = crate::steam_workshop::downloaded_item_path(&id).or_else(|| crate::steam_workshop::steam_client_item_path(&id)) {
                let path_str = path.to_string_lossy().to_string();
                Task::perform(send_req(app.config.socket_path.clone(), IpcRequest::SetSteamWallpaper { path: path_str, screen: None, overrides: None }), Message::GotStatus)
            } else {
                app.workshop_downloading = Some(id.clone());
                app.status_message = format!("Downloading item {} before applying...", id);
                Task::perform(spawn_blocking(move || crate::steam_workshop::download_workshop_item(&id)), Message::GotWorkshopDownload)
            }
        }
        Message::WorkshopAddToLibrary(id) => {
            if crate::steam_workshop::is_downloaded(&id) || crate::steam_workshop::steam_client_item_path(&id).is_some() {
                app.status_message = format!("Item {} is available in Steam library", id);
                Task::perform(spawn_blocking(crate::steam_scanner::scan_steam_wallpapers), Message::GotSteamScan)
            } else {
                app.workshop_downloading = Some(id.clone());
                app.status_message = format!("Downloading item {} to library...", id);
                Task::perform(spawn_blocking(move || crate::steam_workshop::download_workshop_item(&id)), Message::GotWorkshopDownload)
            }
        }
        Message::WorkshopOpenBrowser(id) => {
            crate::steam_workshop::open_in_browser(&id);
            Task::none()
        }
        Message::RescanWallpapers => {
            app.wallpapers = IcedGuiApp::scan_wallpapers(&app.config.wallpaper_dir, &app.config.saved_web_wallpapers);
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
            let raw_url = app.web_url_input.trim().to_string();
            if !raw_url.is_empty() {
                let title = if app.new_web_title.trim().is_empty() {
                    raw_url.clone()
                } else {
                    app.new_web_title.trim().to_string()
                };
                app.config.add_web_bookmark(
                    title.clone(),
                    raw_url.clone(),
                    app.new_web_category.clone(),
                );
                let _ = app.config.save();

                // Trigger render_shot
                let resolved = crate::config::resolve_asset_path(&raw_url);
                let cache_dir = PathBuf::from("/tmp/omywall_thumbs");
                let _ = std::fs::create_dir_all(&cache_dir);
                let hash = format!("{:x}", md5_hash(resolved.as_bytes()));
                let thumb_file = cache_dir.join(format!("web_{}.png", &hash[..8]));
                crate::electron_preview::render_shot(&resolved, &thumb_file);

                app.wallpapers = IcedGuiApp::scan_wallpapers(&app.config.wallpaper_dir, &app.config.saved_web_wallpapers);

                let target_path = if raw_url.starts_with("http://") || raw_url.starts_with("https://") {
                    PathBuf::from(&raw_url)
                } else {
                    let p = Path::new(&resolved);
                    std::fs::canonicalize(p).unwrap_or_else(|_| PathBuf::from(&resolved))
                };
                app.selected_wallpaper = Some(target_path);
                app.status_message = format!("Saved Web / YouTube URL: {}", title);
                app.new_web_title.clear();
            }
            Task::none()
        }

        Message::RemoveWebBookmark(url) => {
            app.config.remove_web_bookmark(&url);
            let _ = app.config.save();
            let resolved_target = crate::config::resolve_asset_path(&url);
            let canon_target = Path::new(&resolved_target).canonicalize().ok();

            app.wallpapers.retain(|w| {
                let w_str = w.to_string_lossy().to_string();
                if w_str == url || w_str == resolved_target {
                    return false;
                }
                if let (Some(c1), Some(c2)) = (canon_target.as_ref(), w.canonicalize().ok().as_ref()) {
                    if c1 == c2 {
                        return false;
                    }
                }
                true
            });
            if let Some(sel) = &app.selected_wallpaper {
                let sel_str = sel.to_string_lossy().to_string();
                if sel_str == url || sel_str == resolved_target || canon_target.as_ref() == sel.canonicalize().ok().as_ref() {
                    app.selected_wallpaper = app.wallpapers.first().cloned();
                }
            }
            app.status_message = format!("Removed bookmark: {}", url);
            Task::none()
        }
        Message::SelectDisplay(name) => {
            app.selected_screen = Some(name);
            Task::none()
        }
        Message::RescanDisplays => {
            Task::perform(spawn_blocking(crate::display::detect_displays), Message::GotDisplays)
        }
        Message::SetMonitorWallpaper(monitor, path) => {
            app.config.monitor_wallpapers.insert(monitor.clone(), path.clone());
            let _ = app.config.save();
            app.status_message = format!("Mapped wallpaper to monitor {}", monitor);
            Task::perform(
                send_req(app.config.socket_path.clone(), IpcRequest::SetMonitorWallpaper { monitor, path }),
                Message::GotStatus,
            )
        }
        Message::ClearMonitorWallpaper(monitor) => {
            app.config.monitor_wallpapers.remove(&monitor);
            let _ = app.config.save();
            app.status_message = format!("Cleared wallpaper mapping for monitor {}", monitor);
            Task::perform(
                send_req(app.config.socket_path.clone(), IpcRequest::SetMonitorWallpaper { monitor, path: String::new() }),
                Message::GotStatus,
            )
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
            Task::perform(send_req(app.config.socket_path.clone(), IpcRequest::SetTargetFps { fps }), Message::GotStatus)
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
        Message::SlideshowIntervalChanged(secs) => {
            app.config.slideshow_interval = secs;
            let _ = app.config.save();
            if let Some(st) = &app.status {
                if st.slideshow_active {
                    return Task::perform(
                        send_req(app.config.socket_path.clone(), IpcRequest::StartSlideshow { interval_secs: secs, shuffle: app.config.slideshow_shuffle }),
                        Message::GotStatus,
                    );
                }
            }
            Task::none()
        }
        Message::SlideshowShuffleToggled => {
            app.config.slideshow_shuffle = !app.config.slideshow_shuffle;
            let _ = app.config.save();
            if let Some(st) = &app.status {
                if st.slideshow_active {
                    return Task::perform(
                        send_req(app.config.socket_path.clone(), IpcRequest::StartSlideshow { interval_secs: app.config.slideshow_interval, shuffle: app.config.slideshow_shuffle }),
                        Message::GotStatus,
                    );
                }
            }
            Task::none()
        }
        Message::StartSlideshow => {
            Task::perform(
                send_req(app.config.socket_path.clone(), IpcRequest::StartSlideshow { interval_secs: app.config.slideshow_interval, shuffle: app.config.slideshow_shuffle }),
                Message::GotStatus,
            )
        }
        Message::StopSlideshow => {
            Task::perform(
                send_req(app.config.socket_path.clone(), IpcRequest::StopSlideshow),
                Message::GotStatus,
            )
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
            if app.show_doctor {
                app.show_logs = false;
            }
            Task::none()
        }
        Message::RecheckDoctor => {
            app.status_message = "Re-checked system dependencies.".to_string();
            Task::none()
        }
        Message::ToggleLogs => {
            app.show_logs = !app.show_logs;
            if app.show_logs {
                app.show_doctor = false;
                Task::perform(load_logs(), Message::GotLogs)
            } else {
                Task::none()
            }
        }
        Message::RefreshLogs => {
            app.status_message = "Refreshing live logs...".to_string();
            Task::perform(load_logs(), Message::GotLogs)
        }
        Message::ClearLogs => {
            let log_path = get_log_path();
            let _ = std::fs::write(&log_path, "");
            app.logs_content = String::new();
            app.status_message = format!("Cleared log file: {}", log_path.display());
            Task::none()
        }
        Message::CopyLogs => {
            let text = app.logs_content.clone();
            if let Ok(mut child) = Command::new("wl-copy").stdin(std::process::Stdio::piped()).spawn() {
                use std::io::Write;
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = stdin.write_all(text.as_bytes());
                }
            }
            app.status_message = "Logs copied to clipboard!".to_string();
            clipboard::write(text)
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
            let active_wall = app.selected_wallpaper.as_ref().map(|p| p.to_string_lossy().to_string());
            let _ = app.config.save_hyprlock_conf(active_wall.as_deref());
            if Command::new("hyprlock").spawn().is_err()
                && Command::new("swaylock").spawn().is_err() {
                    let _ = Command::new("gtklock").spawn();
                }
            Task::none()
        }
        Message::ScreensaverEnabledToggled(enabled) => {
            app.config.hyprlock.enabled = enabled;
            let _ = app.config.save();
            let active_wall = app.selected_wallpaper.as_ref().map(|p| p.to_string_lossy().to_string());
            let _ = app.config.save_hyprlock_conf(active_wall.as_deref());
            Task::none()
        }
        Message::ScreensaverModeChanged(mode) => {
            app.config.hyprlock.screensaver_mode = mode;
            let _ = app.config.save();
            let active_wall = app.selected_wallpaper.as_ref().map(|p| p.to_string_lossy().to_string());
            let _ = app.config.save_hyprlock_conf(active_wall.as_deref());
            Task::none()
        }
        Message::ScreensaverClockColorChanged(color) => {
            app.config.hyprlock.clock_color = color;
            let _ = app.config.save();
            let active_wall = app.selected_wallpaper.as_ref().map(|p| p.to_string_lossy().to_string());
            let _ = app.config.save_hyprlock_conf(active_wall.as_deref());
            Task::none()
        }

        Message::SaveHyprlockConf => {
            let active_wall = app.selected_wallpaper.as_ref().map(|p| p.to_string_lossy().to_string());
            match app.config.save_hyprlock_conf(active_wall.as_deref()) {
                Ok(path) => {
                    app.status_message = format!("Screensaver config saved to {}", path.display());
                }
                Err(e) => {
                    app.status_message = format!("Error saving hyprlock config: {}", e);
                }
            }
            Task::none()
        }
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

        // Desktop Widgets handlers
        Message::ToggleWidgetOverlay => {
            app.config.enable_widgets = !app.config.enable_widgets;
            let target_url = if app.widget_preset == WidgetPreset::Custom {
                if app.custom_widget_url.trim().is_empty() {
                    "assets/widgets/desktop_hud.html".to_string()
                } else {
                    app.custom_widget_url.clone()
                }
            } else {
                app.widget_preset.url().to_string()
            };
            app.config.widget_url = Some(target_url.clone());
            let _ = app.config.save();
            app.status_message = format!(
                "Desktop widgets {}",
                if app.config.enable_widgets { "enabled" } else { "disabled" }
            );
            Task::perform(
                send_req(
                    app.config.socket_path.clone(),
                    IpcRequest::SetWidget {
                        url: target_url,
                        enabled: app.config.enable_widgets,
                        position: Some(app.config.widget_position.clone()),
                    },
                ),
                Message::GotStatus,
            )
        }
        Message::SetWidgetEnabled(enabled) => {
            app.config.enable_widgets = enabled;
            let target_url = if app.widget_preset == WidgetPreset::Custom {
                if app.custom_widget_url.trim().is_empty() {
                    "assets/widgets/desktop_hud.html".to_string()
                } else {
                    app.custom_widget_url.clone()
                }
            } else {
                app.widget_preset.url().to_string()
            };
            app.config.widget_url = Some(target_url.clone());
            let _ = app.config.save();
            app.status_message = format!(
                "Desktop widgets {}",
                if enabled { "enabled" } else { "disabled" }
            );
            Task::perform(
                send_req(
                    app.config.socket_path.clone(),
                    IpcRequest::SetWidget {
                        url: target_url,
                        enabled,
                        position: Some(app.config.widget_position.clone()),
                    },
                ),
                Message::GotStatus,
            )
        }
        Message::SelectWidgetPreset(preset) => {
            app.widget_preset = preset;
            if preset != WidgetPreset::Custom {
                let url = preset.url().to_string();
                app.custom_widget_url = url.clone();
                app.config.widget_url = Some(url);
            } else {
                app.config.widget_url = Some(app.custom_widget_url.clone());
            }
            let _ = app.config.save();
            if app.config.enable_widgets {
                let url = app.config.widget_url.clone().unwrap_or_else(|| "assets/widgets/desktop_hud.html".to_string());
                Task::perform(
                    send_req(
                        app.config.socket_path.clone(),
                        IpcRequest::SetWidget {
                            url,
                            enabled: true,
                            position: Some(app.config.widget_position.clone()),
                        },
                    ),
                    Message::GotStatus,
                )
            } else {
                Task::none()
            }
        }
        Message::SelectWidgetPosition(pos) => {
            app.config.widget_position = pos.clone();
            let _ = app.config.save();
            if app.config.enable_widgets {
                let url = if app.widget_preset == WidgetPreset::Custom {
                    app.custom_widget_url.clone()
                } else {
                    app.widget_preset.url().to_string()
                };
                Task::perform(
                    send_req(
                        app.config.socket_path.clone(),
                        IpcRequest::SetWidget {
                            url,
                            enabled: true,
                            position: Some(pos),
                        },
                    ),
                    Message::GotStatus,
                )
            } else {
                Task::none()
            }
        }
        Message::CustomWidgetUrlChanged(url) => {
            app.custom_widget_url = url.clone();
            app.config.widget_url = Some(url);
            let _ = app.config.save();
            Task::none()
        }
        Message::ApplyWidgetToDesktop => {
            app.config.enable_widgets = true;
            let target_url = if app.widget_preset == WidgetPreset::Custom {
                if app.custom_widget_url.trim().is_empty() {
                    "assets/widgets/desktop_hud.html".to_string()
                } else {
                    app.custom_widget_url.clone()
                }
            } else {
                app.widget_preset.url().to_string()
            };
            app.config.widget_url = Some(target_url.clone());
            let _ = app.config.save();
            app.status_message = "Applying widget to desktop overlay...".to_string();
            Task::perform(
                send_req(
                    app.config.socket_path.clone(),
                    IpcRequest::SetWidget {
                        url: target_url,
                        enabled: true,
                        position: Some(app.config.widget_position.clone()),
                    },
                ),
                Message::GotStatus,
            )
        }
        Message::StopWidgetOverlay => {
            app.config.enable_widgets = false;
            let _ = app.config.save();
            let url = app.config.widget_url.clone().unwrap_or_else(|| "assets/widgets/desktop_hud.html".to_string());
            app.status_message = "Stopping desktop widget overlay...".to_string();
            Task::perform(
                send_req(
                    app.config.socket_path.clone(),
                    IpcRequest::SetWidget {
                        url,
                        enabled: false,
                        position: Some(app.config.widget_position.clone()),
                    },
                ),
                Message::GotStatus,
            )
        }
        Message::TestWidgetWindow => {
            let raw_url = if app.widget_preset == WidgetPreset::Custom {
                if app.custom_widget_url.trim().is_empty() {
                    "assets/widgets/desktop_hud.html".to_string()
                } else {
                    app.custom_widget_url.clone()
                }
            } else {
                app.widget_preset.url().to_string()
            };
            let pos = app.config.widget_position.clone();
            std::thread::spawn(move || {
                let resolved = crate::config::resolve_asset_path(&raw_url);
                let target = if Path::new(&resolved).exists() {
                    format!("file://{}", resolved)
                } else {
                    raw_url.clone()
                };

                if let Ok(exe) = std::env::current_exe() {
                    if Command::new(exe)
                        .args(["web-layer", &target, "--widget", "--position", &pos])
                        .spawn()
                        .is_ok()
                    {
                        return;
                    }
                }
                if Command::new("omywall")
                    .args(["web-layer", &target, "--widget", "--position", &pos])
                    .spawn()
                    .is_err()
                    && Command::new("electron")
                        .args(["--title=OMYWALL Widget Test", &target])
                        .spawn()
                        .is_err()
                    {
                        let _ = Command::new("chromium")
                            .args([format!("--app={}", target)])
                            .spawn();
                    }
            });
            app.status_message = "Launched test widget window".to_string();
            Task::none()
        }
        Message::OpenWidgetFilePicker => {
            Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .set_title("Select Local Desktop Widget (HTML/JS)")
                        .add_filter("HTML Widget", &["html", "htm", "js"])
                        .pick_file()
                        .await
                        .map(|f| f.path().to_path_buf())
                },
                Message::WidgetFilePicked,
            )
        }
        Message::WidgetFilePicked(Some(file_path)) => {
            let path_str = file_path.to_string_lossy().to_string();
            app.widget_preset = WidgetPreset::Custom;
            app.custom_widget_url = path_str.clone();
            app.config.widget_url = Some(path_str.clone());
            let _ = app.config.save();
            if app.config.enable_widgets {
                Task::perform(
                    send_req(
                        app.config.socket_path.clone(),
                        IpcRequest::SetWidget {
                            url: path_str,
                            enabled: true,
                            position: Some(app.config.widget_position.clone()),
                        },
                    ),
                    Message::GotStatus,
                )
            } else {
                Task::none()
            }
        }
        Message::WidgetFilePicked(None) => Task::none(),
    }
}

pub fn media_type_badge(app: &IcedGuiApp, path: &Path) -> (&'static str, Color) {
    let path_str = path.to_string_lossy();
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();

    // 1. Steam Workshop item check
    let is_steam = path_str.contains("steamapps")
        || path_str.contains("431960")
        || path_str.contains("workshop")
        || ext == "pkg"
        || app.steam_wallpapers.iter().any(|s| s.path == *path || path.starts_with(&s.path) || s.path.starts_with(path));
    if is_steam {
        return ("🎮 Steam", Color::from_rgb(0.4, 0.75, 1.0));
    }

    // 2. Widget check
    let is_widget_path = path_str.contains("widget")
        || path_str.contains("desktop_hud")
        || path_str.contains("pomodoro");
    let is_widget_bm = app.config.saved_web_wallpapers.iter().any(|b| {
        (b.url == path_str || Path::new(&crate::config::resolve_asset_path(&b.url)) == path || Path::new(&b.url) == path)
            && b.category.to_lowercase().contains("widget")
    });
    if is_widget_path || is_widget_bm {
        return ("🎛 Widget", AMBER);
    }

    // 3. WebGL / Web 3D check
    let is_web = matches!(ext.as_str(), "html" | "htm" | "js")
        || path_str.starts_with("http://")
        || path_str.starts_with("https://");
    if is_web {
        return ("🌐 WebGL", CYAN);
    }

    // 4. Video check
    let is_vid = matches!(ext.as_str(), "mp4" | "mkv" | "webm" | "avi" | "mov" | "gif" | "flv" | "m4v" | "wmv");
    if is_vid {
        return ("🎥 Video", PURPLE);
    }

    // 5. Image check
    let is_img = matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "bmp");
    if is_img {
        return ("🖼 Image", EMERALD);
    }

    ("📁 File", SOFT_TEXT)
}

fn render_wallpaper_card<'a>(app: &'a IcedGuiApp, path: &PathBuf) -> Element<'a, Message> {
    let path_str = path.to_string_lossy().to_string();
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let ext_upper = ext.to_uppercase();
    let is_web = matches!(ext.as_str(), "html" | "htm" | "js")
        || path_str.starts_with("http://")
        || path_str.starts_with("https://");

    let bookmark_opt = app.config.saved_web_wallpapers.iter().find(|b| {
        b.url == path_str
            || Path::new(&crate::config::resolve_asset_path(&b.url)) == path
            || Path::new(&b.url) == path
            || std::fs::canonicalize(Path::new(&crate::config::resolve_asset_path(&b.url))).map(|p| p == *path).unwrap_or(false)
    });

    let default_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("Wallpaper").to_string();
    let name: String = bookmark_opt.map(|b| b.title.clone()).unwrap_or(default_name);

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
                .content_fit(iced::ContentFit::Cover)
                .into()
        } else if thumb_path.exists() {
            image(thumb_path)
                .width(260)
                .height(146)
                .content_fit(iced::ContentFit::Cover)
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

    let (badge_text, badge_color) = if is_hovered_and_streaming {
        ("● LIVE", EMERALD)
    } else {
        media_type_badge(app, path)
    };

    let badge_pill = container(
        text(badge_text).size(10).color(badge_color)
    )
    .padding([2, 6])
    .style(move |_| container::Style {
        background: Some(Background::Color(Color::from_rgba(0.04, 0.06, 0.10, 0.90))),
        border: Border {
            color: badge_color,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    });

    let truncated_title = truncate_text(&name, 20);

    let action_row = if let Some(bm) = bookmark_opt {
        if !bm.is_demo {
            row![
                btn_primary("▶ Set Wallpaper").on_press(Message::ApplyPath(path.clone())).width(Length::Fill),
                btn_danger("🗑").on_press(Message::RemoveWebBookmark(bm.url.clone())),
            ]
            .spacing(6)
        } else {
            row![btn_primary("▶ Set Wallpaper").on_press(Message::ApplyPath(path.clone())).width(Length::Fill)]
        }
    } else {
        row![btn_primary("▶ Set Wallpaper").on_press(Message::ApplyPath(path.clone())).width(Length::Fill)]
    };

    let card_body = column![
        img_elem,
        row![
            text(truncated_title).size(13).color(if is_selected { CYAN } else { Color::WHITE }).width(Length::Fill),
            badge_pill,
        ]
        .spacing(4)
        .align_y(iced::Alignment::Center),
        action_row,
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

fn format_compact_number(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn render_workshop_item_card<'a>(app: &'a IcedGuiApp, item: &'a WorkshopItem) -> Element<'a, Message> {
    let thumb_cached_path = crate::steam_workshop::cached_preview_path(item);
    let img_elem: Element<'a, Message> = if let Some(ref path) = thumb_cached_path {
        if let Some(cached) = app.image_cache.get(path) {
            image(cached.handle.clone())
                .width(334)
                .height(188)
                .content_fit(iced::ContentFit::Cover)
                .into()
        } else if path.exists() {
            image(path.clone())
                .width(334)
                .height(188)
                .content_fit(iced::ContentFit::Cover)
                .into()
        } else {
            container(
                column![
                    text("🖼").size(28).color(CYAN),
                    text("Loading Preview...").size(12).color(SOFT_TEXT),
                ]
                .spacing(4)
                .align_x(iced::Alignment::Center),
            )
            .width(334)
            .height(188)
            .align_x(iced::Alignment::Center)
            .align_y(iced::Alignment::Center)
            .style(|_| container::Style {
                background: Some(Background::Color(Color::from_rgb(0.06, 0.08, 0.12))),
                border: Border {
                    color: Color::from_rgba(0.2, 0.3, 0.4, 0.4),
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..Default::default()
            })
            .into()
        }
    } else {
        if item.preview_url.is_some() {
            crate::steam_workshop::request_preview_image(item);
        }
        container(
            column![
                text("🌐").size(28).color(CYAN),
                text(if item.preview_url.is_some() { "Fetching Preview..." } else { "No Preview Image" }).size(12).color(SOFT_TEXT),
            ]
            .spacing(4)
            .align_x(iced::Alignment::Center),
        )
        .width(334)
        .height(188)
        .align_x(iced::Alignment::Center)
        .align_y(iced::Alignment::Center)
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgb(0.06, 0.08, 0.12))),
            border: Border {
                color: Color::from_rgba(0.2, 0.3, 0.4, 0.4),
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        })
        .into()
    };

    let title_display = if item.title.is_empty() {
        format!("Workshop Item {}", item.id)
    } else {
        truncate_text(&item.title, 30)
    };

    let author_display = if item.author.is_empty() {
        "Steam Creator".to_string()
    } else {
        format!("by {}", truncate_text(&item.author, 18))
    };

    let mut stats_row = row![
        text(author_display).size(12).color(SOFT_TEXT).width(Length::Fill),
    ].spacing(8).align_y(iced::Alignment::Center);

    if item.subscriptions > 0 {
        stats_row = stats_row.push(
            text(format!("👥 {}", format_compact_number(item.subscriptions))).size(11).color(CYAN)
        );
    }
    if item.views > 0 {
        stats_row = stats_row.push(
            text(format!("👁 {}", format_compact_number(item.views))).size(11).color(SOFT_TEXT)
        );
    }
    if item.file_size > 0 {
        stats_row = stats_row.push(
            text(format!("💾 {}", crate::steam_workshop::get_file_size_str(item.file_size))).size(11).color(DIM_TEXT)
        );
    }

    let is_downloaded = crate::steam_workshop::is_downloaded(&item.id)
        || crate::steam_workshop::steam_client_item_path(&item.id).is_some();
    let is_downloading = app.workshop_downloading.as_deref() == Some(&item.id);

    let action_row: Element<'a, Message> = if is_downloading {
        row![
            btn_pill("⏳ Downloading...", true).width(Length::Fill),
            btn_primary("🌐 Steam").on_press(Message::WorkshopOpenBrowser(item.id.clone())),
        ]
        .spacing(6)
        .into()
    } else if is_downloaded {
        row![
            btn_primary("▶ Apply").on_press(Message::WorkshopApply(item.id.clone())).width(Length::Fill),
            btn_pill("✓ Installed", true),
            btn_primary("🌐 Steam").on_press(Message::WorkshopOpenBrowser(item.id.clone())),
        ]
        .spacing(6)
        .into()
    } else {
        row![
            btn_primary("⬇ Download").on_press(Message::WorkshopDownload(item.id.clone())).width(Length::Fill),
            btn_primary("▶ Apply").on_press(Message::WorkshopApply(item.id.clone())),
            btn_primary("➕ Library").on_press(Message::WorkshopAddToLibrary(item.id.clone())),
            btn_primary("🌐").on_press(Message::WorkshopOpenBrowser(item.id.clone())),
        ]
        .spacing(6)
        .into()
    };

    let mut tags_row = row![].spacing(4);
    for tag in item.tags.iter().take(2) {
        let tag_display = truncate_text(tag, 14);
        tags_row = tags_row.push(
            container(text(tag_display).size(10).color(Color::from_rgb(0.7, 0.75, 0.85)))
                .padding([2, 5])
                .style(|_| container::Style {
                    background: Some(Background::Color(Color::from_rgba(0.12, 0.16, 0.24, 0.8))),
                    border: Border {
                        color: Color::from_rgba(0.25, 0.35, 0.5, 0.5),
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                })
        );
    }

    let card_content = column![
        img_elem,
        row![
            text(title_display).size(14).color(Color::WHITE).width(Length::Fill),
            tags_row,
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center),
        stats_row,
        action_row,
    ]
    .spacing(8)
    .padding(8);

    container(card_content)
        .width(350)
        .style(move |_| container::Style {
            background: Some(Background::Color(CARD_BG)),
            border: Border {
                color: if is_downloaded {
                    Color::from_rgba(0.1, 0.8, 0.5, 0.6)
                } else {
                    CARD_STROKE
                },
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
        })
        .into()
}

pub fn filtered_wallpapers(app: &IcedGuiApp) -> Vec<PathBuf> {
    let search_query = app.search_filter.trim().to_lowercase();
    app.wallpapers.iter().filter(|w| {
        let path_str = w.to_string_lossy().to_string();
        let ext = w.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        let is_web = matches!(ext.as_str(), "html" | "htm" | "js")
            || path_str.starts_with("http://")
            || path_str.starts_with("https://");
        let is_vid = matches!(ext.as_str(), "mp4" | "mkv" | "webm" | "avi" | "mov" | "gif" | "flv" | "m4v" | "wmv");
        let is_img = matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "bmp");
        let is_steam = path_str.contains("steamapps")
            || path_str.contains("431960")
            || path_str.contains("workshop")
            || ext == "pkg"
            || app.steam_wallpapers.iter().any(|s| &s.path == *w || s.path.starts_with(w) || w.starts_with(&s.path));

        let category_match = match app.category_filter {
            CategoryFilter::All => true,
            CategoryFilter::Videos => is_vid,
            CategoryFilter::WebWidgets => is_web,
            CategoryFilter::StaticImages => is_img,
            CategoryFilter::SteamWorkshop => is_steam,
        };

        if !category_match {
            return false;
        }

        if search_query.is_empty() {
            return true;
        }

        let file_name = w.file_name().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
        if file_name.contains(&search_query) || path_str.to_lowercase().contains(&search_query) {
            return true;
        }

        if let Some(bm) = app.config.saved_web_wallpapers.iter().find(|b| {
            b.url == path_str
                || Path::new(&crate::config::resolve_asset_path(&b.url)) == *w
                || Path::new(&b.url) == *w
        }) {
            if bm.title.to_lowercase().contains(&search_query)
                || bm.category.to_lowercase().contains(&search_query)
            {
                return true;
            }
        }

        if let Some(sw) = app.steam_wallpapers.iter().find(|s| &s.path == *w || s.path.starts_with(w) || w.starts_with(&s.path)) {
            if sw.title.to_lowercase().contains(&search_query) || sw.author.to_lowercase().contains(&search_query) {
                return true;
            }
        }

        false
    }).cloned().collect()
}

pub fn active_carousel_item(app: &IcedGuiApp) -> Option<PathBuf> {
    let filtered = filtered_wallpapers(app);
    if filtered.is_empty() {
        return None;
    }
    let active_idx = app.carousel_index.min(filtered.len() - 1);
    Some(filtered[active_idx].clone())
}

#[allow(dead_code)]
fn is_live_item(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let path_str = path.to_string_lossy().to_string();
    let is_web = matches!(ext.as_str(), "html" | "htm" | "js")
        || path_str.starts_with("http://")
        || path_str.starts_with("https://");
    let is_video = matches!(ext.as_str(), "mp4" | "mkv" | "webm" | "avi" | "mov" | "gif" | "flv" | "m4v" | "wmv");
    is_web || is_video
}

fn render_carousel_view<'a>(app: &'a IcedGuiApp, filtered: &[PathBuf]) -> Element<'a, Message> {
    if filtered.is_empty() {
        return container(text("No wallpapers match the search or category filter.").color(AMBER).size(16))
            .width(Length::Fill)
            .padding(30)
            .into();
    }

    let active_idx = app.carousel_index.min(filtered.len() - 1);
    let target = &filtered[active_idx];
    let path_str = target.to_string_lossy().to_string();
    let ext = target.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let is_web = matches!(ext.as_str(), "html" | "htm" | "js")
        || path_str.starts_with("http://")
        || path_str.starts_with("https://");

    let bookmark_opt = app.config.saved_web_wallpapers.iter().find(|b| {
        b.url == path_str
            || Path::new(&crate::config::resolve_asset_path(&b.url)) == target
            || Path::new(&b.url) == target
            || std::fs::canonicalize(Path::new(&crate::config::resolve_asset_path(&b.url))).map(|p| p == *target).unwrap_or(false)
    });

    let default_name = target.file_name().and_then(|n| n.to_str()).unwrap_or("Wallpaper").to_string();
    let name: String = bookmark_opt.map(|b| b.title.clone()).unwrap_or(default_name);

    let live_path = if is_web {
        PathBuf::from(HOVER_WEB_LIVE_PATH)
    } else {
        PathBuf::from(HOVER_VIDEO_LIVE_PATH)
    };

    let is_live = app.hover_streaming.as_ref() == Some(target);

    let is_img = matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif");

    let spotlight_img: Element<'a, Message> = if is_img && Path::new(&path_str).exists() {
        image(PathBuf::from(&path_str)).width(600).height(337).content_fit(iced::ContentFit::Cover).into()
    } else if let Some(cached) = app.image_cache.get(target) {
        image(cached.handle.clone()).width(600).height(337).content_fit(iced::ContentFit::Cover).into()
    } else if let Some(cached) = app.image_cache.get(&live_path) {
        image(cached.handle.clone()).width(600).height(337).content_fit(iced::ContentFit::Cover).into()
    } else if let Some(thumb_path) = get_web_thumbnail_path(&path_str) {
        if let Some(cached) = app.image_cache.get(&thumb_path) {
            image(cached.handle.clone()).width(600).height(337).content_fit(iced::ContentFit::Cover).into()
        } else if thumb_path.exists() {
            image(thumb_path).width(600).height(337).content_fit(iced::ContentFit::Cover).into()
        } else {
            container(text(if is_web { "🌐 Web 3D Preset Preview" } else { "🎥 Video Wallpaper Preview" }).color(AMBER).size(18))
                .width(600)
                .height(337)
                .align_x(iced::Alignment::Center)
                .align_y(iced::Alignment::Center)
                .into()
        }
    } else {
        container(text("🌌 OMYWALL Spotlight Preview Player").color(CYAN).size(18))
            .width(600)
            .height(337)
            .align_x(iced::Alignment::Center)
            .align_y(iced::Alignment::Center)
            .into()
    };


    let target_click = target.clone();
    let interactive_img = mouse_area(spotlight_img)
        .on_press(Message::CardClicked(target_click));

    let mut controls = row![
        btn_primary("◀ Previous").on_press(Message::CarouselPrev),
        btn_primary("▶ Set Wallpaper").on_press(Message::ApplyPath(target.clone())),
    ]
    .spacing(12);

    if is_web {
        controls = controls.push(btn_primary("👁 Preview Web (Electron/Browser)").on_press(Message::OpenWebPreview(target.to_string_lossy().to_string())));
    } else if is_img {
        controls = controls.push(btn_primary("👁 Preview Image").on_press(Message::OpenImagePreview(target.to_string_lossy().to_string())));
    } else {
        controls = controls.push(btn_primary("👁 Preview Video (MPV)").on_press(Message::OpenVideoPreview(target.to_string_lossy().to_string())));
    }

    if let Some(bm) = bookmark_opt {
        if !bm.is_demo {
            controls = controls.push(btn_danger("🗑 Remove Bookmark").on_press(Message::RemoveWebBookmark(bm.url.clone())));
        }
    }

    controls = controls.push(btn_primary("Next ▶").on_press(Message::CarouselNext));

    let (spot_badge_text, spot_badge_color) = if is_live {
        ("● LIVE STREAMING", EMERALD)
    } else {
        media_type_badge(app, target)
    };

    let spot_badge_pill = container(
        text(spot_badge_text).size(11).color(spot_badge_color)
    )
    .padding([3, 8])
    .style(move |_| container::Style {
        background: Some(Background::Color(Color::from_rgba(0.04, 0.06, 0.10, 0.90))),
        border: Border {
            color: spot_badge_color,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    });

    let spotlight_card = mouse_area(
        container(
            column![
                row![
                    text(format!("● SPOTLIGHT PLAYER ({}/{})", active_idx + 1, filtered.len()))
                        .color(if is_live { EMERALD } else { CYAN })
                        .size(14),
                    space().width(Length::Fill),
                    spot_badge_pill,
                    space().width(8),
                    text(if is_web { "WebKit2GTK / Electron Layer" } else { "MPV Hardware Video" }).color(AMBER).size(12),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
                interactive_img,
                text(name).color(Color::WHITE).size(18),
                text(path_str).color(DIM_TEXT).size(12),
                controls,
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
        })
    )
    .on_enter(Message::CardEntered(target.clone()))
    .on_exit(Message::CardExited(target.clone()));

    let mut filmstrip = row![].spacing(8);
    for (idx, w) in filtered.iter().enumerate().take(24) {
        let is_current = idx == active_idx;
        let w_name = w.file_name().and_then(|n| n.to_str()).unwrap_or("Item");
        let truncated = truncate_text(w_name, 14);
        let path_clone = w.clone();
        filmstrip = filmstrip.push(
            btn_pill(truncated, is_current)
                .on_press(Message::CardClicked(path_clone))
        );
    }

    column![
        spotlight_card,
        scrollable(filmstrip).width(Length::Fill),
    ]
    .spacing(16)
    .align_x(iced::Alignment::Center)
    .into()
}

fn view(app: &IcedGuiApp) -> Element<'_, Message> {
    let header_title = text("OMYWALL")
        .size(24)
        .color(CYAN);


    let status_indicator = if let Some(st) = &app.status {
        let active = st.current_wallpaper.as_deref().unwrap_or("None");
        let truncated = truncate_text(active, 30);
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

    let gpu_name_display = if app.system_metrics.gpu_name.is_empty() {
        "GPU".to_string()
    } else {
        truncate_text(&app.system_metrics.gpu_name, 18)
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

    let btn_doctor_active = app.show_doctor;
    let btn_doctor = button(
        row![
            text("⚙").size(13).color(if btn_doctor_active { CYAN } else { SOFT_TEXT }),
            text("System Doctor").size(12).color(if btn_doctor_active { CYAN } else { SOFT_TEXT }),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center)
    )
    .padding([6, 12])
    .style(move |_, status| {
        let bg = match status {
            iced::widget::button::Status::Hovered => CARD_BG_SEL,
            _ => if btn_doctor_active { CARD_BG_SEL } else { CARD_BG },
        };
        iced::widget::button::Style {
            background: Some(Background::Color(bg)),
            text_color: if btn_doctor_active { CYAN } else { SOFT_TEXT },
            border: Border {
                color: if btn_doctor_active { CYAN } else { CARD_STROKE },
                width: 1.0,
                radius: 6.0.into(),
            },
            shadow: iced::Shadow::default(),
            snap: true,
        }
    })
    .on_press(Message::ToggleDoctor);

    let btn_logs_active = app.show_logs;
    let btn_logs = button(
        row![
            text("📋").size(13).color(if btn_logs_active { CYAN } else { SOFT_TEXT }),
            text("Logs").size(12).color(if btn_logs_active { CYAN } else { SOFT_TEXT }),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center)
    )
    .padding([6, 12])
    .style(move |_, status| {
        let bg = match status {
            iced::widget::button::Status::Hovered => CARD_BG_SEL,
            _ => if btn_logs_active { CARD_BG_SEL } else { CARD_BG },
        };
        iced::widget::button::Style {
            background: Some(Background::Color(bg)),
            text_color: if btn_logs_active { CYAN } else { SOFT_TEXT },
            border: Border {
                color: if btn_logs_active { CYAN } else { CARD_STROKE },
                width: 1.0,
                radius: 6.0.into(),
            },
            shadow: iced::Shadow::default(),
            snap: true,
        }
    })
    .on_press(Message::ToggleLogs);

    let header = row![
        header_title,
        space().width(Length::Fill),
        gpu_metrics_badge,
        space().width(12),
        status_indicator,
        space().width(12),
        btn_doctor,
        btn_logs,
    ]
    .spacing(12)
    .align_y(iced::Alignment::Center);

    let tab_installed = btn_tab("🖼 Installed Wallpapers", app.active_tab == AppTab::Installed)
        .on_press(Message::Tab(AppTab::Installed));
    let tab_workshop = btn_tab("🌐 Steam Workshop", app.active_tab == AppTab::SteamWorkshop)
        .on_press(Message::Tab(AppTab::SteamWorkshop));
    let tab_widgets = btn_tab("🎛 Desktop Widgets", app.active_tab == AppTab::Widgets)
        .on_press(Message::Tab(AppTab::Widgets));
    let tab_displays = btn_tab("📺 Display Manager", app.active_tab == AppTab::Displays)
        .on_press(Message::Tab(AppTab::Displays));
    let tab_screensaver = btn_tab("🔒 Screensaver", app.active_tab == AppTab::Screensaver)
        .on_press(Message::Tab(AppTab::Screensaver));
    let tab_settings = btn_tab("⚙ Settings", app.active_tab == AppTab::Settings)
        .on_press(Message::Tab(AppTab::Settings));

    let nav_bar = row![tab_installed, tab_workshop, tab_widgets, tab_displays, tab_screensaver, tab_settings].spacing(10);



    let body_content: Element<'_, Message> = match app.active_tab {
        AppTab::Installed => {
            let filtered_wallpapers = filtered_wallpapers(app);

            let cat_pills = row![
                btn_pill("All", app.category_filter == CategoryFilter::All)
                    .on_press(Message::Category(CategoryFilter::All)),
                btn_pill("🎥 Videos", app.category_filter == CategoryFilter::Videos)
                    .on_press(Message::Category(CategoryFilter::Videos)),
                btn_pill("🌐 Web & 3D", app.category_filter == CategoryFilter::WebWidgets)
                    .on_press(Message::Category(CategoryFilter::WebWidgets)),
                btn_pill("🖼 Images", app.category_filter == CategoryFilter::StaticImages)
                    .on_press(Message::Category(CategoryFilter::StaticImages)),
                btn_pill("🎮 Steam Items", app.category_filter == CategoryFilter::SteamWorkshop)
                    .on_press(Message::Category(CategoryFilter::SteamWorkshop)),
            ].spacing(8);

            let search_input = text_input("Search wallpapers...", &app.search_filter)
                .on_input(Message::SearchFilterChanged)
                .padding(6)
                .width(220);

            let view_mode_toggle = row![
                btn_pill("⣿ Grid", app.view_mode == ViewMode::Grid)
                    .on_press(Message::ViewMode(ViewMode::Grid)),
                btn_pill("🎠 Carousel", app.view_mode == ViewMode::Carousel)
                    .on_press(Message::ViewMode(ViewMode::Carousel)),
            ].spacing(8);

            let file_folder_buttons = row![
                btn_primary("📁 Select Folder").on_press(Message::OpenFolderPicker),
                btn_primary("➕ Select File").on_press(Message::OpenFilePicker),
            ].spacing(8);


            let url_input_row = row![
                text_input("Paste Web or YouTube URL (https://...)", &app.web_url_input)
                    .on_input(Message::WebUrlChanged)
                    .padding(6)
                    .width(340),
                text_input("Title (optional)", &app.new_web_title)
                    .on_input(Message::WebTitleChanged)
                    .padding(6)
                    .width(180),
                btn_primary("💾 Save & Add URL").on_press(Message::SaveWebBookmark),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center);

            let top_bar = column![
                row![
                    text("Local & Web Catalog").size(18).color(CYAN),
                    space().width(12),
                    cat_pills,
                    space().width(Length::Fill),
                    search_input,
                    space().width(8),
                    file_folder_buttons,
                    space().width(8),
                    view_mode_toggle,
                ].align_y(iced::Alignment::Center),
                url_input_row,
            ]
            .spacing(8);


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
            let search_input = text_input("Search Steam Workshop wallpapers or paste ID...", &app.workshop_query)
                .on_input(Message::WorkshopQueryChanged)
                .on_submit(Message::WorkshopSearch)
                .padding(8)
                .width(320);

            let search_row = row![
                text("🌐 Steam Workshop Store").size(18).color(CYAN),
                space().width(Length::Fill),
                search_input,
                btn_primary("🔍 Search").on_press(Message::WorkshopSearch),
                if !app.workshop_query.is_empty() {
                    btn_pill("✕ Clear", false).on_press(Message::WorkshopClear)
                } else {
                    btn_pill("✕ Clear", false)
                },
                btn_primary("📂 Scan Local Steam Items").on_press(Message::WorkshopRescanSteam),
                btn_primary("🔄 Refresh Store").on_press(Message::WorkshopSearch),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center);

            let sort_pills = row![
                text("Sort:").size(13).color(SOFT_TEXT),
                btn_pill("🔥 Trending (7 Days)", app.workshop_sort == "trend")
                    .on_press(Message::WorkshopSortChanged("trend".to_string())),
                btn_pill("⭐ Most Popular", app.workshop_sort == "top_rated")
                    .on_press(Message::WorkshopSortChanged("top_rated".to_string())),
                btn_pill("👥 Most Subscribed", app.workshop_sort == "most_subscribed")
                    .on_press(Message::WorkshopSortChanged("most_subscribed".to_string())),
                btn_pill("🕒 Most Recent", app.workshop_sort == "newest")
                    .on_press(Message::WorkshopSortChanged("newest".to_string())),
                space().width(Length::Fill),
                if app.workshop_loading {
                    text("⏳ Loading workshop items...").size(13).color(CYAN)
                } else if let Some(ref dl_id) = app.workshop_downloading {
                    text(format!("⬇ Downloading item {} via SteamCMD...", dl_id)).size(13).color(EMERALD)
                } else if !app.workshop_status.is_empty() {
                    text(&app.workshop_status).size(13).color(if app.workshop_status.contains("Error") { AMBER } else { SOFT_TEXT })
                } else {
                    text(format!("{} items found", app.workshop_items.len())).size(13).color(SOFT_TEXT)
                },
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center);

            let top_toolbar = column![
                search_row,
                sort_pills,
            ]
            .spacing(10);

            let prev_btn = if app.workshop_page > 1 && !app.workshop_loading {
                btn_primary("◀ Prev Page").on_press(Message::WorkshopPagePrev)
            } else {
                btn_pill("◀ Prev Page", false)
            };

            let next_btn = if !app.workshop_loading && !app.workshop_items.is_empty() {
                btn_primary("Next Page ▶").on_press(Message::WorkshopPageNext)
            } else {
                btn_pill("Next Page ▶", false)
            };

            let page_indicator = container(
                text(format!("Page {}", app.workshop_page)).size(14).color(CYAN)
            )
            .padding([6, 16])
            .style(|_| container::Style {
                background: Some(Background::Color(Color::from_rgba(0.08, 0.12, 0.18, 0.9))),
                border: Border {
                    color: Color::from_rgba(0.0, 0.94, 1.0, 0.3),
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..Default::default()
            });

            let pagination_row = row![
                prev_btn,
                page_indicator,
                next_btn,
            ]
            .spacing(12)
            .align_y(iced::Alignment::Center);

            let pagination_container = container(pagination_row)
                .width(Length::Fill)
                .align_x(iced::Alignment::Center)
                .padding(8);

            let content: Element<'_, Message> = if app.workshop_loading && app.workshop_items.is_empty() {
                container(
                    column![
                        text("⏳ Loading Steam Workshop...").size(18).color(CYAN),
                        text("Fetching popular and trending wallpapers from Steam...").size(13).color(SOFT_TEXT),
                    ]
                    .spacing(8)
                    .align_x(iced::Alignment::Center)
                )
                .width(Length::Fill)
                .padding(60)
                .align_x(iced::Alignment::Center)
                .into()
            } else if app.workshop_items.is_empty() {
                container(
                    column![
                        text("No Steam Workshop items found").size(18).color(AMBER),
                        text("Try searching with different keywords or browse trending wallpapers.").size(13).color(SOFT_TEXT),
                        btn_primary("🔥 Load Popular Wallpapers").on_press(Message::WorkshopClear),
                    ]
                    .spacing(12)
                    .align_x(iced::Alignment::Center)
                )
                .width(Length::Fill)
                .padding(60)
                .align_x(iced::Alignment::Center)
                .into()
            } else {
                let mut grid_rows = column![].spacing(14);
                let mut current_row = row![].spacing(14);
                let mut count_in_row = 0;

                for item in &app.workshop_items {
                    current_row = current_row.push(render_workshop_item_card(app, item));
                    count_in_row += 1;
                    if count_in_row == 3 {
                        grid_rows = grid_rows.push(current_row);
                        current_row = row![].spacing(14);
                        count_in_row = 0;
                    }
                }
                if count_in_row > 0 {
                    grid_rows = grid_rows.push(current_row);
                }

                column![
                    grid_rows,
                    pagination_container,
                ]
                .spacing(16)
                .into()
            };

            column![
                top_toolbar,
                scrollable(content).width(Length::Fill),
            ]
            .spacing(14)
            .into()
        }
        AppTab::Displays => render_displays_tab(app),
        AppTab::Screensaver => render_screensaver_tab(app),
        AppTab::Widgets => render_widgets_tab(app),
        AppTab::Settings => render_settings_tab(app),
    };

    let content = column![
        header,
        rule::horizontal(1),
        nav_bar,
        rule::horizontal(1),
        body_content,
    ]
    .spacing(16);

    let base_container: Element<'_, Message> = container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(20)
        .into();

    if app.show_doctor {
        stack![
            base_container,
            render_system_doctor_modal(app),
        ]
        .into()
    } else if app.show_logs {
        stack![
            base_container,
            render_logs_modal(app),
        ]
        .into()
    } else {
        base_container
    }
}

fn render_widgets_tab<'a>(app: &'a IcedGuiApp) -> Element<'a, Message> {
    // 1. Header Hero Card
    let header_card = container(
        column![
            row![
                text("🎛 Desktop Layer-Shell Overlay Widgets").size(20).color(CYAN),
                space().width(Length::Fill),
                container(
                    text(if app.config.enable_widgets { "● OVERLAY ACTIVE" } else { "○ OVERLAY INACTIVE" })
                        .size(11)
                        .color(if app.config.enable_widgets { EMERALD } else { SOFT_TEXT })
                )
                .padding([4, 10])
                .style(|_| container::Style {
                    background: Some(Background::Color(Color::from_rgba(0.04, 0.08, 0.14, 0.90))),
                    border: Border {
                        color: if app.config.enable_widgets { EMERALD } else { CARD_STROKE },
                        width: 1.0,
                        radius: 12.0.into(),
                    },
                    ..Default::default()
                }),
            ]
            .align_y(iced::Alignment::Center),
            text("Pin real-time hardware telemetry HUDs, desktop pills, and custom WebGL/HTML dashboards cleanly onto your Wayland desktop layer.").size(13).color(SOFT_TEXT),
        ]
        .spacing(8)
    )
    .padding(16)
    .style(|_| container::Style {
        background: Some(Background::Color(CARD_BG)),
        border: Border {
            color: CARD_STROKE,
            width: 1.0,
            radius: 10.0.into(),
        },
        ..Default::default()
    });

    // 2. Master Enable / Quick Actions Bar
    let is_enabled = app.config.enable_widgets;
    let master_control_card = container(
        row![
            checkbox(is_enabled).on_toggle(Message::SetWidgetEnabled),
            column![
                text("Enable Desktop Widget Overlay").size(15).color(Color::WHITE),
                text(if is_enabled {
                    "Transparent layer-shell overlay is currently active and anchored."
                } else {
                    "Overlay is currently disabled. Toggle on to display on desktop."
                }).size(12).color(SOFT_TEXT),
            ].spacing(2),
            space().width(Length::Fill),
            btn_primary("▶ Apply to Desktop").on_press(Message::ApplyWidgetToDesktop),
            btn_primary("👁 Test in Window").on_press(Message::TestWidgetWindow),
            btn_danger("⏹ Stop Overlay").on_press(Message::StopWidgetOverlay),
        ]
        .spacing(14)
        .align_y(iced::Alignment::Center)
    )
    .padding(16)
    .style(move |_| container::Style {
        background: Some(Background::Color(if is_enabled { CARD_BG_SEL } else { CARD_BG })),
        border: Border {
            color: if is_enabled { CYAN } else { CARD_STROKE },
            width: if is_enabled { 1.5 } else { 1.0 },
            radius: 10.0.into(),
        },
        ..Default::default()
    });

    // 3. Preset Selector Cards
    let presets = [
        WidgetPreset::CyberHud,
        WidgetPreset::WifiBluetoothPill,
        WidgetPreset::MinimalClock,
        WidgetPreset::Custom,
    ];

    let mut preset_cards = row![].spacing(12);
    for preset in presets {
        let is_selected = app.widget_preset == preset;
        let card = container(
            column![
                row![
                    text(preset.label()).size(14).color(if is_selected { CYAN } else { Color::WHITE }),
                    space().width(Length::Fill),
                    if is_selected {
                        container(text("SELECTED").size(10).color(CYAN))
                            .padding([2, 6])
                            .style(|_| container::Style {
                                background: Some(Background::Color(Color::from_rgba(0.18, 0.83, 0.78, 0.15))),
                                border: Border {
                                    color: CYAN,
                                    width: 1.0,
                                    radius: 4.0.into(),
                                },
                                ..Default::default()
                            })
                    } else {
                        container(text("PRESET").size(10).color(SOFT_TEXT))
                            .padding([2, 6])
                            .style(|_| container::Style {
                                background: Some(Background::Color(CARD_BG)),
                                border: Border {
                                    color: CARD_STROKE,
                                    width: 1.0,
                                    radius: 4.0.into(),
                                },
                                ..Default::default()
                            })
                    }
                ]
                .align_y(iced::Alignment::Center),
                text(preset.description()).size(12).color(SOFT_TEXT),
                space().height(6),
                btn_pill(if is_selected { "Active Preset" } else { "Select Preset" }, is_selected)
                    .on_press(Message::SelectWidgetPreset(preset))
                    .width(Length::Fill),
            ]
            .spacing(8)
            .padding(12)
        )
        .width(Length::FillPortion(1))
        .style(move |_| container::Style {
            background: Some(Background::Color(if is_selected { CARD_BG_SEL } else { CARD_BG })),
            border: Border {
                color: if is_selected { CYAN } else { CARD_STROKE },
                width: if is_selected { 2.0 } else { 1.0 },
                radius: 8.0.into(),
            },
            ..Default::default()
        });

        preset_cards = preset_cards.push(card);
    }

    // 4. Anchoring / Positioning
    let current_pos = &app.config.widget_position;
    let pos_row = row![
        text("Screen Position:").size(14).color(Color::WHITE),
        btn_pill("↗ Top Right", current_pos == "top_right")
            .on_press(Message::SelectWidgetPosition("top_right".to_string())),
        btn_pill("↖ Top Left", current_pos == "top_left")
            .on_press(Message::SelectWidgetPosition("top_left".to_string())),
        btn_pill("↘ Bottom Right", current_pos == "bottom_right")
            .on_press(Message::SelectWidgetPosition("bottom_right".to_string())),
        btn_pill("↙ Bottom Left", current_pos == "bottom_left")
            .on_press(Message::SelectWidgetPosition("bottom_left".to_string())),
        btn_pill("⚓ Center Dock", current_pos == "center_dock")
            .on_press(Message::SelectWidgetPosition("center_dock".to_string())),
    ]
    .spacing(10)
    .align_y(iced::Alignment::Center);

    let position_card = container(
        column![
            text("📍 Screen Anchoring & Placement").size(15).color(CYAN),
            text("Determines which corner or dock edge the transparent layer-shell surface attaches to on Wayland.").size(12).color(SOFT_TEXT),
            pos_row,
        ]
        .spacing(10)
    )
    .padding(14)
    .style(|_| container::Style {
        background: Some(Background::Color(CARD_BG)),
        border: Border {
            color: CARD_STROKE,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    });

    // 5. Custom URL & Local Picker Input
    let custom_url_card = container(
        column![
            text("🔗 Custom Widget Source (Local File or Web URL)").size(15).color(CYAN),
            text("You can load any local HTML/JS widget, WebGL page, or remote dashboard URL (e.g. Grafana, Home Assistant).").size(12).color(SOFT_TEXT),
            row![
                text_input("Enter local file path or http(s) URL...", &app.custom_widget_url)
                    .on_input(Message::CustomWidgetUrlChanged)
                    .padding(8)
                    .width(Length::Fill),
                btn_primary("📁 Browse Local HTML File").on_press(Message::OpenWidgetFilePicker),
            ]
            .spacing(10)
            .align_y(iced::Alignment::Center),
        ]
        .spacing(10)
    )
    .padding(14)
    .style(|_| container::Style {
        background: Some(Background::Color(CARD_BG)),
        border: Border {
            color: CARD_STROKE,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    });

    // 6. Live Telemetry & Bridge Status Card
    let target_display = if app.widget_preset == WidgetPreset::Custom {
        if app.custom_widget_url.trim().is_empty() {
            "assets/widgets/desktop_hud.html (default)".to_string()
        } else {
            app.custom_widget_url.clone()
        }
    } else {
        app.widget_preset.url().to_string()
    };

    let telemetry_card = container(
        column![
            row![
                text("⚡ LIVE TELEMETRY & LAYER-SHELL BRIDGE").size(14).color(CYAN),
                space().width(Length::Fill),
                text("IPC Bridge Active").size(12).color(EMERALD),
            ]
            .align_y(iced::Alignment::Center),
            rule::horizontal(1),
            row![
                column![
                    text("Target HTML / URL:").size(12).color(SOFT_TEXT),
                    text(target_display).size(13).color(Color::WHITE),
                ].spacing(4).width(Length::FillPortion(2)),
                column![
                    text("Layer-Shell Mode:").size(12).color(SOFT_TEXT),
                    text("gtk-layer-shell / wlr-layer-shell").size(13).color(CYAN),
                ].spacing(4).width(Length::FillPortion(1)),
                column![
                    text("Background Alpha:").size(12).color(SOFT_TEXT),
                    text("100% Transparent RGBA").size(13).color(EMERALD),
                ].spacing(4).width(Length::FillPortion(1)),
                column![
                    text("Telemetry JSON:").size(12).color(SOFT_TEXT),
                    text("/tmp/omywall_telemetry.json").size(13).color(AMBER),
                ].spacing(4).width(Length::FillPortion(1)),
            ]
            .spacing(16),
        ]
        .spacing(10)
    )
    .padding(14)
    .style(|_| container::Style {
        background: Some(Background::Color(CARD_BG)),
        border: Border {
            color: CARD_STROKE,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    });

    let main_col = column![
        header_card,
        master_control_card,
        column![
            text("📦 Choose Widget Preset").size(16).color(CYAN),
            preset_cards,
        ].spacing(8),
        position_card,
        custom_url_card,
        telemetry_card,
    ]
    .spacing(16);

    scrollable(main_col).into()
}

fn render_displays_tab<'a>(app: &'a IcedGuiApp) -> Element<'a, Message> {
    // 1. Header Hero Card
    let header_card = container(
        column![
            row![
                text("📺 Multi-Monitor & Display Layout Manager").size(20).color(CYAN),
                space().width(Length::Fill),
                container(
                    text(format!("● {} CONNECTED MONITORS", app.displays.len()))
                        .size(11)
                        .color(if !app.displays.is_empty() { EMERALD } else { AMBER })
                )
                .padding([4, 10])
                .style(|_| container::Style {
                    background: Some(Background::Color(Color::from_rgba(0.04, 0.08, 0.14, 0.90))),
                    border: Border {
                        color: EMERALD,
                        width: 1.0,
                        radius: 12.0.into(),
                    },
                    ..Default::default()
                }),
                space().width(8),
                btn_primary("🔄 Rescan Connected Monitors").on_press(Message::RescanDisplays),
            ]
            .align_y(iced::Alignment::Center),
            text("Detect all connected display outputs, view refresh rates and geometry topologies, and bind dedicated wallpapers to specific screens.")
                .size(13)
                .color(SOFT_TEXT),
        ]
        .spacing(8)
    )
    .padding(16)
    .style(|_| container::Style {
        background: Some(Background::Color(CARD_BG)),
        border: Border {
            color: CARD_STROKE,
            width: 1.0,
            radius: 10.0.into(),
        },
        ..Default::default()
    });

    // 2. Wallpaper Assignment Context Banner
    let selected_wall_stem = app.selected_wallpaper.as_ref().and_then(|p| p.file_name()).and_then(|n| n.to_str());

    let context_card = container(
        row![
            text("🖼️ Current Library Selection:").size(13).color(SOFT_TEXT),
            if let Some(wall_name) = selected_wall_stem {
                text(wall_name).size(14).color(CYAN)
            } else {
                text("None (Select a wallpaper from Installed tab to assign per-display)").size(13).color(DIM_TEXT)
            },
            space().width(Length::Fill),
            text("Click 'Assign Selected' on any display card below to bind wallpaper").size(12).color(AMBER),
        ]
        .spacing(10)
        .align_y(iced::Alignment::Center)
    )
    .padding([10, 16])
    .style(|_| container::Style {
        background: Some(Background::Color(Color::from_rgba(0.06, 0.10, 0.18, 0.85))),
        border: Border {
            color: Color::from_rgba(0.18, 0.83, 0.78, 0.35),
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    });

    // 3. Virtual Display Topology Layout Canvas
    let mut topology_row = row![].spacing(14).align_y(iced::Alignment::Center);
    for (idx, d) in app.displays.iter().enumerate() {
        let is_selected_screen = app.selected_screen.as_ref() == Some(&d.name) || (app.selected_screen.is_none() && d.primary);
        let assigned_wall = app.config.monitor_wallpapers.get(&d.name);

        let mini_box = container(
            column![
                row![
                    text(format!("🖥️ {}", d.name)).size(14).color(if is_selected_screen { CYAN } else { Color::WHITE }),
                    space().width(Length::Fill),
                    if d.primary {
                        container(text("PRIMARY").size(9).color(AMBER))
                            .padding([2, 5])
                            .style(|_| container::Style {
                                background: Some(Background::Color(Color::from_rgba(0.98, 0.65, 0.12, 0.15))),
                                border: Border { color: AMBER, width: 1.0, radius: 4.0.into() },
                                ..Default::default()
                            })
                    } else {
                        container(text(format!("#{}", idx + 1)).size(9).color(SOFT_TEXT))
                            .padding([2, 5])
                            .style(|_| container::Style {
                                background: Some(Background::Color(CARD_BG)),
                                border: Border { color: CARD_STROKE, width: 1.0, radius: 4.0.into() },
                                ..Default::default()
                            })
                    }
                ]
                .align_y(iced::Alignment::Center),
                text(format!("{}x{} @ {}Hz", d.width, d.height, d.refresh_rate))
                    .size(11)
                    .color(EMERALD),
                text(format!("Offset: +{}, +{}", d.x, d.y))
                    .size(10)
                    .color(SOFT_TEXT),
                space().height(4),
                if let Some(wall) = assigned_wall {
                    let w_clean = Path::new(wall).file_name().and_then(|n| n.to_str()).unwrap_or(wall);
                    let trunc = truncate_text(w_clean, 18);
                    text(format!("📌 {}", trunc)).size(10).color(CYAN)
                } else {
                    text("🌐 Global Active").size(10).color(DIM_TEXT)
                },
            ]
            .spacing(4)
            .padding(10)
        )
        .width(Length::Fixed(190.0))
        .height(Length::Fixed(115.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(if is_selected_screen { CARD_BG_SEL } else { CARD_BG })),
            border: Border {
                color: if is_selected_screen { CYAN } else { CARD_STROKE },
                width: if is_selected_screen { 2.0 } else { 1.0 },
                radius: 8.0.into(),
            },
            ..Default::default()
        });

        topology_row = topology_row.push(
            mouse_area(mini_box).on_press(Message::SelectDisplay(d.name.clone()))
        );
    }

    let topology_card = container(
        column![
            row![
                text("🗺️ Virtual Multi-Display Topology").size(14).color(CYAN),
                space().width(Length::Fill),
                text("Click a monitor tile to focus / target output").size(11).color(SOFT_TEXT),
            ]
            .align_y(iced::Alignment::Center),
            rule::horizontal(1),
            scrollable(topology_row).direction(scrollable::Direction::Horizontal(scrollable::Scrollbar::default())),
        ]
        .spacing(10)
    )
    .padding(14)
    .style(|_| container::Style {
        background: Some(Background::Color(CARD_BG)),
        border: Border {
            color: CARD_STROKE,
            width: 1.0,
            radius: 10.0.into(),
        },
        ..Default::default()
    });

    // 4. Detailed Monitor Cards
    let mut displays_col = column![].spacing(14);

    if app.displays.is_empty() {
        let empty_card = container(
            column![
                text("⚠️ No Monitors Detected").size(16).color(AMBER),
                text("Could not query display info via hyprctl, wlr-randr, or xrandr. Click below to rescan.").size(13).color(SOFT_TEXT),
                btn_primary("🔄 Rescan Connected Displays").on_press(Message::RescanDisplays),
            ]
            .spacing(10)
            .align_x(iced::Alignment::Center)
        )
        .padding(30)
        .width(Length::Fill)
        .align_x(iced::Alignment::Center)
        .style(|_| container::Style {
            background: Some(Background::Color(CARD_BG)),
            border: Border {
                color: CARD_STROKE,
                width: 1.0,
                radius: 10.0.into(),
            },
            ..Default::default()
        });
        displays_col = displays_col.push(empty_card);
    } else {
        for d in &app.displays {
            let is_selected_screen = app.selected_screen.as_ref() == Some(&d.name) || (app.selected_screen.is_none() && d.primary);
            let assigned_wall = app.config.monitor_wallpapers.get(&d.name);

            let res_pill = container(
                text(format!("{}x{} @ {}Hz", d.width, d.height, d.refresh_rate))
                    .size(12)
                    .color(EMERALD)
            )
            .padding([3, 8])
            .style(|_| container::Style {
                background: Some(Background::Color(Color::from_rgba(0.04, 0.20, 0.12, 0.85))),
                border: Border {
                    color: EMERALD,
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..Default::default()
            });

            let role_pill = if d.primary {
                container(text("⭐ Primary Display").size(11).color(AMBER))
                    .padding([3, 8])
                    .style(|_| container::Style {
                        background: Some(Background::Color(Color::from_rgba(0.24, 0.16, 0.04, 0.85))),
                        border: Border { color: AMBER, width: 1.0, radius: 6.0.into() },
                        ..Default::default()
                    })
            } else {
                container(text("Secondary Display").size(11).color(SOFT_TEXT))
                    .padding([3, 8])
                    .style(|_| container::Style {
                        background: Some(Background::Color(CARD_BG)),
                        border: Border { color: CARD_STROKE, width: 1.0, radius: 6.0.into() },
                        ..Default::default()
                    })
            };

            let focus_pill = if is_selected_screen {
                container(text("● Active Target").size(11).color(CYAN))
                    .padding([3, 8])
                    .style(|_| container::Style {
                        background: Some(Background::Color(Color::from_rgba(0.04, 0.15, 0.25, 0.85))),
                        border: Border { color: CYAN, width: 1.0, radius: 6.0.into() },
                        ..Default::default()
                    })
            } else {
                container(text("Connected").size(11).color(SOFT_TEXT))
                    .padding([3, 8])
                    .style(|_| container::Style {
                        background: Some(Background::Color(CARD_BG)),
                        border: Border { color: CARD_STROKE, width: 1.0, radius: 6.0.into() },
                        ..Default::default()
                    })
            };

            let card_header = row![
                text(format!("🖥️ {}", d.name)).size(16).color(if is_selected_screen { CYAN } else { Color::WHITE }),
                space().width(8),
                res_pill,
                role_pill,
                focus_pill,
                space().width(Length::Fill),
                text(format!("Position: +{}, +{}", d.x, d.y)).size(12).color(SOFT_TEXT),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center);

            let specs_row = row![
                column![
                    text("Width & Height:").size(11).color(DIM_TEXT),
                    text(format!("{} × {} px", d.width, d.height)).size(13).color(Color::WHITE),
                ].spacing(2).width(Length::FillPortion(1)),
                column![
                    text("Refresh Rate:").size(11).color(DIM_TEXT),
                    text(format!("{} Hz", d.refresh_rate)).size(13).color(CYAN),
                ].spacing(2).width(Length::FillPortion(1)),
                column![
                    text("Virtual Offset:").size(11).color(DIM_TEXT),
                    text(format!("x: {}, y: {}", d.x, d.y)).size(13).color(SOFT_TEXT),
                ].spacing(2).width(Length::FillPortion(1)),
                column![
                    text("Output Identifier:").size(11).color(DIM_TEXT),
                    text(&d.id).size(13).color(AMBER),
                ].spacing(2).width(Length::FillPortion(1)),
            ]
            .spacing(12);

            let mapping_row = if let Some(assigned_path) = assigned_wall {
                let p_clean = Path::new(assigned_path).file_name().and_then(|n| n.to_str()).unwrap_or(assigned_path);
                row![
                    text("📌 Custom Assigned Wallpaper:").size(13).color(CYAN),
                    text(p_clean).size(13).color(Color::WHITE),
                    space().width(Length::Fill),
                    btn_danger("✕ Clear Mapping").on_press(Message::ClearMonitorWallpaper(d.name.clone())),
                ]
                .spacing(10)
                .align_y(iced::Alignment::Center)
            } else {
                row![
                    text("🌐 Wallpaper:").size(13).color(SOFT_TEXT),
                    text("Using Global Active Wallpaper").size(13).color(DIM_TEXT),
                    space().width(Length::Fill),
                ]
                .spacing(10)
                .align_y(iced::Alignment::Center)
            };

            let mut actions_row = row![
                btn_pill(if is_selected_screen { "🎯 Targeted Output" } else { "🎯 Focus Output" }, is_selected_screen)
                    .on_press(Message::SelectDisplay(d.name.clone())),
            ]
            .spacing(10)
            .align_y(iced::Alignment::Center);

            if let Some(sel) = &app.selected_wallpaper {
                let name = sel.file_name().and_then(|n| n.to_str()).unwrap_or("Item");
                let trunc = truncate_text(name, 24);
                actions_row = actions_row.push(
                    btn_primary(format!("▶ Assign Selected ('{}')", trunc))
                        .on_press(Message::SetMonitorWallpaper(d.name.clone(), sel.to_string_lossy().to_string()))
                );
            }

            let card = container(
                column![
                    card_header,
                    rule::horizontal(1),
                    specs_row,
                    container(mapping_row)
                        .padding([8, 12])
                        .style(|_| container::Style {
                            background: Some(Background::Color(Color::from_rgba(0.03, 0.05, 0.09, 0.60))),
                            border: Border {
                                color: Color::from_rgba(0.12, 0.16, 0.26, 0.60),
                                width: 1.0,
                                radius: 6.0.into(),
                            },
                            ..Default::default()
                        }),
                    actions_row,
                ]
                .spacing(12)
            )
            .padding(16)
            .style(move |_| container::Style {
                background: Some(Background::Color(if is_selected_screen { CARD_BG_SEL } else { CARD_BG })),
                border: Border {
                    color: if is_selected_screen { CYAN } else { CARD_STROKE },
                    width: if is_selected_screen { 2.0 } else { 1.0 },
                    radius: 10.0.into(),
                },
                ..Default::default()
            });

            displays_col = displays_col.push(card);
        }
    }

    let main_col = column![
        header_card,
        context_card,
        topology_card,
        displays_col,
    ]
    .spacing(16);

    scrollable(main_col).into()
}

fn render_screensaver_tab<'a>(app: &'a IcedGuiApp) -> Element<'a, Message> {
    // 1. Header Card
    let is_enabled = app.config.hyprlock.enabled;
    let header_card = container(
        column![
            row![
                text("🔒 Screensaver & Hyprlock Security Layer").size(20).color(CYAN),
                space().width(Length::Fill),
                container(
                    text(if is_enabled { "● HYPRLOCK INTEGRATION ACTIVE" } else { "○ INTEGRATION DISABLED" })
                        .size(11)
                        .color(if is_enabled { EMERALD } else { SOFT_TEXT })
                )
                .padding([4, 10])
                .style(move |_| container::Style {
                    background: Some(Background::Color(if is_enabled {
                        Color::from_rgba(0.04, 0.20, 0.12, 0.85)
                    } else {
                        Color::from_rgba(0.10, 0.12, 0.18, 0.85)
                    })),
                    border: Border {
                        color: if is_enabled { EMERALD } else { CARD_STROKE },
                        width: 1.0,
                        radius: 12.0.into(),
                    },
                    ..Default::default()
                }),
            ]
            .align_y(iced::Alignment::Center),
            text("Synchronize your active video and 3D wallpapers with Wayland lockscreens via hyprlock / swaylock / gtklock.")
                .size(13)
                .color(SOFT_TEXT),
        ]
        .spacing(8)
    )
    .padding(16)
    .style(|_| container::Style {
        background: Some(Background::Color(CARD_BG)),
        border: Border {
            color: CARD_STROKE,
            width: 1.0,
            radius: 10.0.into(),
        },
        ..Default::default()
    });

    // 2. Control Cards: Mode Selector + Clock Color
    let mode_card = container(
        column![
            row![
                checkbox(is_enabled).on_toggle(Message::ScreensaverEnabledToggled),
                column![
                    text("Enable Lockscreen & Screensaver Sync").size(15).color(Color::WHITE),
                    text("Automatically generates ~/.config/hypr/hyprlock.conf matching current wallpaper mode.")
                        .size(12)
                        .color(SOFT_TEXT),
                ].spacing(2),
            ]
            .spacing(12)
            .align_y(iced::Alignment::Center),
            rule::horizontal(1),
            text("Screensaver Background Mode:").size(14).color(CYAN),
            row![
                btn_pill("🌌 Active Live", app.config.hyprlock.screensaver_mode == "active")
                    .on_press(Message::ScreensaverModeChanged("active".to_string())),
                btn_pill("🌀 Blurred Static", app.config.hyprlock.screensaver_mode == "blur")
                    .on_press(Message::ScreensaverModeChanged("blur".to_string())),
                btn_pill("🎨 Solid Color", app.config.hyprlock.screensaver_mode == "color")
                    .on_press(Message::ScreensaverModeChanged("color".to_string())),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
            text(match app.config.hyprlock.screensaver_mode.as_str() {
                "blur" => "🌀 Blurred Static: Captures desktop background snapshot and applies high-speed Gaussian blur.",
                "color" => "🎨 Solid Color: Minimalist pitch-black / midnight dark gradient canvas for OLED power saving.",
                _ => "🌌 Active Live: Runs active MPV hardware video decoder or WebGL shader live behind the lockscreen.",
            })
            .size(12)
            .color(SOFT_TEXT),
        ]
        .spacing(10)
    )
    .padding(16)
    .style(|_| container::Style {
        background: Some(Background::Color(CARD_BG)),
        border: Border {
            color: CARD_STROKE,
            width: 1.0,
            radius: 10.0.into(),
        },
        ..Default::default()
    });

    let current_clock_hex = &app.config.hyprlock.clock_color;
    let clock_color = match current_clock_hex.as_str() {
        "#10b981" => EMERALD,
        "#f59e0b" => AMBER,
        "#a855f7" => PURPLE,
        "#ffffff" => Color::WHITE,
        _ => CYAN,
    };

    let clock_card = container(
        column![
            row![
                text("⏰ Clock & UI Accent Palette").size(14).color(CYAN),
                space().width(Length::Fill),
                container(text(current_clock_hex).size(11).color(clock_color))
                    .padding([2, 6])
                    .style(move |_| container::Style {
                        background: Some(Background::Color(Color::from_rgba(0.04, 0.08, 0.14, 0.90))),
                        border: Border { color: clock_color, width: 1.0, radius: 4.0.into() },
                        ..Default::default()
                    }),
            ]
            .align_y(iced::Alignment::Center),
            row![
                btn_pill("Cyan", current_clock_hex == "#00f0ff")
                    .on_press(Message::ScreensaverClockColorChanged("#00f0ff".to_string())),
                btn_pill("Emerald", current_clock_hex == "#10b981")
                    .on_press(Message::ScreensaverClockColorChanged("#10b981".to_string())),
                btn_pill("Amber", current_clock_hex == "#f59e0b")
                    .on_press(Message::ScreensaverClockColorChanged("#f59e0b".to_string())),
                btn_pill("Purple", current_clock_hex == "#a855f7")
                    .on_press(Message::ScreensaverClockColorChanged("#a855f7".to_string())),
                btn_pill("White", current_clock_hex == "#ffffff")
                    .on_press(Message::ScreensaverClockColorChanged("#ffffff".to_string())),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
            rule::horizontal(1),
            row![
                column![
                    text("Blur Passes:").size(11).color(DIM_TEXT),
                    text("2 Passes (7px Radius)").size(12).color(Color::WHITE),
                ].spacing(2).width(Length::FillPortion(1)),
                column![
                    text("Security Protocol:").size(11).color(DIM_TEXT),
                    text("PAM Shadow Auth").size(12).color(EMERALD),
                ].spacing(2).width(Length::FillPortion(1)),
                column![
                    text("Grace Period:").size(11).color(DIM_TEXT),
                    text("0s (Immediate Lock)").size(12).color(SOFT_TEXT),
                ].spacing(2).width(Length::FillPortion(1)),
            ]
            .spacing(12),
        ]
        .spacing(10)
    )
    .padding(16)
    .style(|_| container::Style {
        background: Some(Background::Color(CARD_BG)),
        border: Border {
            color: CARD_STROKE,
            width: 1.0,
            radius: 10.0.into(),
        },
        ..Default::default()
    });

    // 3. High-Fidelity Live Lockscreen Preview Simulation Box
    let username = std::env::var("USER").unwrap_or_else(|_| "mister".to_string());
    let mode_str = app.config.hyprlock.screensaver_mode.to_uppercase();

    // Lockscreen inner overlay elements builder
    let build_lockscreen_content = || {
        let clock_display = text("12:45").size(52).color(clock_color);
        let date_display = text("Wednesday, August 14").size(15).color(SOFT_TEXT);

        let user_pill = container(
            row![
                text("👤").size(12).color(SOFT_TEXT),
                text(format!("Welcome back, {}", username)).size(13).color(Color::WHITE),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center)
        )
        .padding([4, 12])
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgba(0.08, 0.12, 0.20, 0.70))),
            border: Border {
                color: Color::from_rgba(0.2, 0.3, 0.45, 0.50),
                width: 1.0,
                radius: 14.0.into(),
            },
            ..Default::default()
        });

        let password_box = container(
            row![
                text("••••••••").size(14).color(SOFT_TEXT),
                text("▎").size(14).color(clock_color),
            ]
            .spacing(2)
            .align_y(iced::Alignment::Center)
        )
        .padding([6, 20])
        .style(move |_| container::Style {
            background: Some(Background::Color(Color::from_rgba(0.04, 0.06, 0.10, 0.85))),
            border: Border {
                color: clock_color,
                width: 1.5,
                radius: 16.0.into(),
            },
            ..Default::default()
        });

        let security_pill = container(
            text("🔒 Hyprlock Desktop Secured · PAM Authentication").size(11).color(EMERALD)
        )
        .padding([3, 10])
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgba(0.02, 0.12, 0.08, 0.85))),
            border: Border {
                color: EMERALD,
                width: 1.0,
                radius: 10.0.into(),
            },
            ..Default::default()
        });

        column![
            clock_display,
            date_display,
            space().height(4),
            user_pill,
            space().height(2),
            password_box,
            space().height(4),
            security_pill,
        ]
        .spacing(6)
        .align_x(iced::Alignment::Center)
    };

    let is_color_mode = app.config.hyprlock.screensaver_mode == "color";
    let is_blur_mode = app.config.hyprlock.screensaver_mode == "blur";

    // Preview wallpaper background
    let bg_preview_element: Element<'a, Message> = if is_color_mode {
        container(build_lockscreen_content())
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::Alignment::Center)
            .align_y(iced::Alignment::Center)
            .style(|_| container::Style {
                background: Some(Background::Color(Color::from_rgb(0.03, 0.04, 0.07))),
                ..Default::default()
            })
            .into()
    } else {
        // Find thumbnail if available
        let wall_thumb: Option<Element<'a, Message>> = if let Some(ref target) = app.selected_wallpaper {
            let path_str = target.to_string_lossy().to_string();
            let ext = target.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
            let is_img = matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp");

            if is_img && target.exists() {
                Some(image(target.clone()).width(Length::Fill).height(Length::Fill).content_fit(iced::ContentFit::Cover).into())
            } else if let Some(cached) = app.image_cache.get(target) {
                Some(image(cached.handle.clone()).width(Length::Fill).height(Length::Fill).content_fit(iced::ContentFit::Cover).into())
            } else if let Some(thumb_path) = get_web_thumbnail_path(&path_str) {
                if let Some(cached) = app.image_cache.get(&thumb_path) {
                    Some(image(cached.handle.clone()).width(Length::Fill).height(Length::Fill).content_fit(iced::ContentFit::Cover).into())
                } else if thumb_path.exists() {
                    Some(image(thumb_path).width(Length::Fill).height(Length::Fill).content_fit(iced::ContentFit::Cover).into())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let overlay_tint = container(build_lockscreen_content())
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::Alignment::Center)
            .align_y(iced::Alignment::Center)
            .style(move |_| container::Style {
                background: Some(Background::Color(if is_blur_mode {
                    Color::from_rgba(0.05, 0.08, 0.16, 0.82)
                } else {
                    Color::from_rgba(0.03, 0.05, 0.10, 0.65)
                })),
                ..Default::default()
            });

        if let Some(img_elem) = wall_thumb {
            stack![
                img_elem,
                overlay_tint,
            ]
            .into()
        } else {
            container(build_lockscreen_content())
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(iced::Alignment::Center)
                .align_y(iced::Alignment::Center)
                .style(|_| container::Style {
                    background: Some(Background::Color(Color::from_rgb(0.04, 0.07, 0.14))),
                    ..Default::default()
                })
                .into()
        }
    };

    let simulated_monitor_frame = container(
        column![
            row![
                text("🖥️ HYPRLOCK LOCKSCREEN PREVIEW").size(13).color(CYAN),
                space().width(Length::Fill),
                text(format!("MODE: {}", mode_str)).size(12).color(AMBER),
            ]
            .align_y(iced::Alignment::Center),
            container(bg_preview_element)
                .width(Length::Fill)
                .height(Length::Fixed(270.0))
                .style(|_| container::Style {
                    background: Some(Background::Color(Color::from_rgb(0.02, 0.03, 0.05))),
                    border: Border {
                        color: Color::from_rgba(0.18, 0.83, 0.78, 0.40),
                        width: 1.5,
                        radius: 8.0.into(),
                    },
                    ..Default::default()
                }),
            row![
                btn_primary("👁 Live Fullscreen Test").on_press(Message::TestScreensaver),
                btn_primary("💾 Save Screensaver Config").on_press(Message::SaveHyprlockConf),
                space().width(Length::Fill),
                text("Target: ~/.config/hypr/hyprlock.conf").size(12).color(SOFT_TEXT),
            ]
            .spacing(12)
            .align_y(iced::Alignment::Center),
        ]
        .spacing(12)
    )
    .padding(16)
    .style(|_| container::Style {
        background: Some(Background::Color(CARD_BG)),
        border: Border {
            color: CYAN,
            width: 1.5,
            radius: 12.0.into(),
        },
        ..Default::default()
    });

    let main_col = column![
        header_card,
        row![
            container(mode_card).width(Length::FillPortion(1)),
            container(clock_card).width(Length::FillPortion(1)),
        ].spacing(14),
        simulated_monitor_frame,
    ]
    .spacing(16);

    scrollable(main_col).into()
}

fn render_settings_tab<'a>(app: &'a IcedGuiApp) -> Element<'a, Message> {
    let is_daemon_running = app.status.is_some();

    // 1. Header Card
    let header_card = container(
        column![
            row![
                text("⚙ Wallpaper Engine & System Settings").size(20).color(CYAN),
                space().width(Length::Fill),
                container(
                    text(if is_daemon_running { "● DAEMON ONLINE" } else { "○ DAEMON OFFLINE" })
                        .size(11)
                        .color(if is_daemon_running { EMERALD } else { AMBER })
                )
                .padding([4, 10])
                .style(move |_| container::Style {
                    background: Some(Background::Color(if is_daemon_running {
                        Color::from_rgba(0.04, 0.20, 0.12, 0.85)
                    } else {
                        Color::from_rgba(0.24, 0.16, 0.04, 0.85)
                    })),
                    border: Border {
                        color: if is_daemon_running { EMERALD } else { AMBER },
                        width: 1.0,
                        radius: 12.0.into(),
                    },
                    ..Default::default()
                }),
            ]
            .align_y(iced::Alignment::Center),
            text("Configure hardware video decoding pipelines, framerate limits, audio volume, opacity, and systemd/autostart persistence.")
                .size(13)
                .color(SOFT_TEXT),
        ]
        .spacing(8)
    )
    .padding(16)
    .style(|_| container::Style {
        background: Some(Background::Color(CARD_BG)),
        border: Border {
            color: CARD_STROKE,
            width: 1.0,
            radius: 10.0.into(),
        },
        ..Default::default()
    });

    // 2. Section 1: Themes & Color Scheme
    let theme_card = container(
        column![
            text("🎨 Interface Color Scheme").size(15).color(CYAN),
            text("Choose your preferred aesthetic palette for the Omywall control center.").size(12).color(SOFT_TEXT),
            row![
                btn_pill("🌌 Dark Glass", app.theme_scheme == ThemeScheme::DarkGlass)
                    .on_press(Message::ThemeChanged(ThemeScheme::DarkGlass)),
                btn_pill("⚡ Steam Amber", app.theme_scheme == ThemeScheme::SteamAmber)
                    .on_press(Message::ThemeChanged(ThemeScheme::SteamAmber)),
                btn_pill("🔮 Cyber Light", app.theme_scheme == ThemeScheme::HardLightCyber)
                    .on_press(Message::ThemeChanged(ThemeScheme::HardLightCyber)),
                btn_pill("🖤 OLED Black", app.theme_scheme == ThemeScheme::OledPitchBlack)
                    .on_press(Message::ThemeChanged(ThemeScheme::OledPitchBlack)),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        ]
        .spacing(10)
    )
    .padding(14)
    .style(|_| container::Style {
        background: Some(Background::Color(CARD_BG)),
        border: Border {
            color: CARD_STROKE,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    });

    // 3. Section 2: Hardware Video Acceleration (HWDEC)
    let hwdec_card = container(
        column![
            row![
                text("🚀 Hardware Video Acceleration (HWDEC)").size(15).color(CYAN),
                space().width(Length::Fill),
                container(text(format!("Active: {}", app.config.hwdec)).size(11).color(EMERALD))
                    .padding([2, 6])
                    .style(|_| container::Style {
                        background: Some(Background::Color(Color::from_rgba(0.04, 0.20, 0.12, 0.85))),
                        border: Border { color: EMERALD, width: 1.0, radius: 4.0.into() },
                        ..Default::default()
                    }),
            ]
            .align_y(iced::Alignment::Center),
            text("Hardware decoding offloads 4K/60fps video rendering directly to your GPU, saving CPU cycles and battery life.").size(12).color(SOFT_TEXT),
            row![
                btn_pill("auto (Auto-detect)", app.config.hwdec == "auto")
                    .on_press(Message::HwdecChanged("auto".to_string())),
                btn_pill("nvdec (NVIDIA)", app.config.hwdec == "nvdec")
                    .on_press(Message::HwdecChanged("nvdec".to_string())),
                btn_pill("vaapi (Intel/AMD)", app.config.hwdec == "vaapi")
                    .on_press(Message::HwdecChanged("vaapi".to_string())),
                btn_pill("vulkan (Vulkan Video)", app.config.hwdec == "vulkan")
                    .on_press(Message::HwdecChanged("vulkan".to_string())),
                btn_pill("cuda (CUDA)", app.config.hwdec == "cuda")
                    .on_press(Message::HwdecChanged("cuda".to_string())),
                btn_pill("no (Software CPU)", app.config.hwdec == "no")
                    .on_press(Message::HwdecChanged("no".to_string())),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        ]
        .spacing(10)
    )
    .padding(14)
    .style(|_| container::Style {
        background: Some(Background::Color(CARD_BG)),
        border: Border {
            color: CARD_STROKE,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    });

    // 4. Section 3: FPS Limiter & Surface Opacity
    let fps_card = container(
        column![
            row![
                text("🎯 Target FPS Limiter").size(15).color(CYAN),
                space().width(Length::Fill),
                text(format!("{} FPS", app.config.target_fps)).size(13).color(EMERALD),
            ]
            .align_y(iced::Alignment::Center),
            text("Caps the maximum rendering framerate for video shaders and WebGL animations.").size(12).color(SOFT_TEXT),
            row![
                btn_pill("30 FPS", app.config.target_fps == 30)
                    .on_press(Message::FpsChanged(30)),
                btn_pill("60 FPS", app.config.target_fps == 60)
                    .on_press(Message::FpsChanged(60)),
                btn_pill("120 FPS", app.config.target_fps == 120)
                    .on_press(Message::FpsChanged(120)),
                btn_pill("144 FPS", app.config.target_fps == 144)
                    .on_press(Message::FpsChanged(144)),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        ]
        .spacing(10)
    )
    .padding(14)
    .style(|_| container::Style {
        background: Some(Background::Color(CARD_BG)),
        border: Border {
            color: CARD_STROKE,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    });

    let opacity_pct = (app.opacity_slider * 100.0).round() as u32;
    let opacity_card = container(
        column![
            row![
                text("✨ Desktop Surface Opacity").size(15).color(CYAN),
                space().width(Length::Fill),
                text(format!("{}% ({:.2})", opacity_pct, app.opacity_slider)).size(13).color(CYAN),
            ]
            .align_y(iced::Alignment::Center),
            text("Adjusts transparency of the layer-shell background surface (0.0 to 1.0).").size(12).color(SOFT_TEXT),
            row![
                slider(0.0..=1.0, app.opacity_slider, Message::OpacityChanged)
                    .width(Length::FillPortion(2)),
                btn_pill("100%", app.opacity_slider >= 0.98).on_press(Message::OpacityChanged(1.0)),
                btn_pill("80%", (app.opacity_slider - 0.8).abs() < 0.05).on_press(Message::OpacityChanged(0.8)),
                btn_pill("60%", (app.opacity_slider - 0.6).abs() < 0.05).on_press(Message::OpacityChanged(0.6)),
                btn_pill("40%", (app.opacity_slider - 0.4).abs() < 0.05).on_press(Message::OpacityChanged(0.4)),
            ]
            .spacing(10)
            .align_y(iced::Alignment::Center),
        ]
        .spacing(10)
    )
    .padding(14)
    .style(|_| container::Style {
        background: Some(Background::Color(CARD_BG)),
        border: Border {
            color: CARD_STROKE,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    });

    // 5. Section 4: Audio Volume & Mute Controls
    let is_muted = app.config.mute;
    let audio_card = container(
        column![
            row![
                text("🔊 Master Audio & Volume").size(15).color(CYAN),
                space().width(Length::Fill),
                if is_muted {
                    text("MUTED 🔇").size(13).color(AMBER)
                } else {
                    text(format!("{}%", app.volume_slider)).size(13).color(EMERALD)
                },
            ]
            .align_y(iced::Alignment::Center),
            text("Control live sound playback levels for video wallpapers and interactive HTML5 audio.").size(12).color(SOFT_TEXT),
            row![
                slider(0.0..=100.0, app.volume_slider as f32, |v| Message::VolumeChanged(v as i64))
                    .width(Length::FillPortion(2)),
                if is_muted {
                    btn_danger("🔇 Muted (Unmute)").on_press(Message::MuteToggled)
                } else {
                    btn_secondary("🔊 Mute Audio").on_press(Message::MuteToggled)
                },
                btn_pill("25%", app.volume_slider == 25).on_press(Message::VolumeChanged(25)),
                btn_pill("50%", app.volume_slider == 50).on_press(Message::VolumeChanged(50)),
                btn_pill("75%", app.volume_slider == 75).on_press(Message::VolumeChanged(75)),
                btn_pill("100%", app.volume_slider == 100).on_press(Message::VolumeChanged(100)),
            ]
            .spacing(10)
            .align_y(iced::Alignment::Center),
        ]
        .spacing(10)
    )
    .padding(14)
    .style(|_| container::Style {
        background: Some(Background::Color(CARD_BG)),
        border: Border {
            color: CARD_STROKE,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    });

    // 6. Section 5: Wallpaper Library Directory
    let dir_card = container(
        column![
            text("📁 Wallpaper Library Directory").size(15).color(CYAN),
            text("Configured directory scanned recursively for video, 3D web, and image wallpapers.").size(12).color(SOFT_TEXT),
            row![
                container(
                    text(format!("{}", app.config.wallpaper_dir.display())).size(13).color(Color::WHITE)
                )
                .padding([8, 12])
                .width(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(Color::from_rgba(0.03, 0.05, 0.09, 0.80))),
                    border: Border { color: CARD_STROKE, width: 1.0, radius: 6.0.into() },
                    ..Default::default()
                }),
                btn_primary("📁 Change Folder").on_press(Message::OpenFolderPicker),
                btn_primary("➕ Select File").on_press(Message::OpenFilePicker),
                btn_secondary("🔄 Rescan").on_press(Message::RescanWallpapers),
            ]
            .spacing(10)
            .align_y(iced::Alignment::Center),
        ]
        .spacing(10)
    )
    .padding(14)
    .style(|_| container::Style {
        background: Some(Background::Color(CARD_BG)),
        border: Border {
            color: CARD_STROKE,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    });

    // 7. Section 6: Autostart & Automated Slideshow
    let is_autostart = app.autostart_enabled;
    let autostart_card = container(
        column![
            row![
                checkbox(is_autostart).on_toggle(|_| Message::AutostartToggled),
                column![
                    text("Launch Omywall Daemon on Boot").size(15).color(Color::WHITE),
                    text("Registers autostart entry in ~/.config/autostart/omywall.desktop on session login.").size(12).color(SOFT_TEXT),
                ].spacing(2),
                space().width(Length::Fill),
                text(if is_autostart { "ENABLED" } else { "DISABLED" })
                    .size(12)
                    .color(if is_autostart { EMERALD } else { SOFT_TEXT }),
            ]
            .spacing(12)
            .align_y(iced::Alignment::Center),
        ]
        .spacing(6)
    )
    .padding(14)
    .style(|_| container::Style {
        background: Some(Background::Color(CARD_BG)),
        border: Border {
            color: CARD_STROKE,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    });

    let is_slideshow_active = app.status.as_ref().map(|s| s.slideshow_active).unwrap_or(false);
    let interval = app.config.slideshow_interval;
    let is_shuffle = app.config.slideshow_shuffle;

    let slideshow_card = container(
        column![
            row![
                text("🎠 Automated Wallpaper Slideshow").size(15).color(CYAN),
                space().width(Length::Fill),
                container(
                    text(if is_slideshow_active { "● SLIDESHOW RUNNING" } else { "○ SLIDESHOW INACTIVE" })
                        .size(11)
                        .color(if is_slideshow_active { EMERALD } else { SOFT_TEXT })
                )
                .padding([2, 6])
                .style(move |_| container::Style {
                    background: Some(Background::Color(Color::from_rgba(0.04, 0.08, 0.14, 0.90))),
                    border: Border { color: if is_slideshow_active { EMERALD } else { CARD_STROKE }, width: 1.0, radius: 4.0.into() },
                    ..Default::default()
                }),
            ]
            .align_y(iced::Alignment::Center),
            text("Automatically transitions between installed wallpapers at your selected interval.").size(12).color(SOFT_TEXT),
            row![
                text("Interval:").size(13).color(SOFT_TEXT),
                btn_pill("15s", interval == 15).on_press(Message::SlideshowIntervalChanged(15)),
                btn_pill("30s", interval == 30).on_press(Message::SlideshowIntervalChanged(30)),
                btn_pill("1 min", interval == 60).on_press(Message::SlideshowIntervalChanged(60)),
                btn_pill("5 min", interval == 300).on_press(Message::SlideshowIntervalChanged(300)),
                btn_pill("15 min", interval == 900).on_press(Message::SlideshowIntervalChanged(900)),
                btn_pill("1 hour", interval == 3600).on_press(Message::SlideshowIntervalChanged(3600)),
                space().width(Length::Fill),
                btn_pill("🔀 Shuffle Order", is_shuffle).on_press(Message::SlideshowShuffleToggled),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
            row![
                if is_slideshow_active {
                    btn_danger("⏹ Stop Slideshow").on_press(Message::StopSlideshow)
                } else {
                    btn_primary("▶ Start Slideshow").on_press(Message::StartSlideshow)
                },
                btn_secondary("⏮ Previous").on_press(Message::PrevWallpaper),
                btn_secondary("⏭ Next Wallpaper").on_press(Message::NextWallpaper),
            ]
            .spacing(10)
            .align_y(iced::Alignment::Center),
        ]
        .spacing(10)
    )
    .padding(14)
    .style(|_| container::Style {
        background: Some(Background::Color(CARD_BG)),
        border: Border {
            color: CARD_STROKE,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    });

    // 8. Section 7: Daemon Control & Maintenance
    let daemon_card = container(
        column![
            row![
                text("⚡ Daemon Process Management").size(15).color(CYAN),
                space().width(Length::Fill),
                text(format!("Socket: {}", app.config.socket_path.display())).size(11).color(DIM_TEXT),
            ]
            .align_y(iced::Alignment::Center),
            text("Control the background wallpaper service, pause playback, or execute dependency repairs.").size(12).color(SOFT_TEXT),
            row![
                btn_primary("▶ Start Daemon").on_press(Message::StartDaemon),
                btn_danger("⏹ Stop Daemon").on_press(Message::StopWallpaper),
                btn_primary("⏯ Toggle Pause").on_press(Message::TogglePause),
                btn_secondary("⏮ Prev").on_press(Message::PrevWallpaper),
                btn_secondary("⏭ Next").on_press(Message::NextWallpaper),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
            rule::horizontal(1),
            row![
                btn_primary("🛠 Run Dependency Installer Script").on_press(Message::RunInstaller),
                btn_secondary("⚙ System Doctor Diagnostics").on_press(Message::ToggleDoctor),
                btn_secondary("📋 View Live Logs").on_press(Message::ToggleLogs),
            ]
            .spacing(10)
            .align_y(iced::Alignment::Center),
        ]
        .spacing(10)
    )
    .padding(14)
    .style(|_| container::Style {
        background: Some(Background::Color(CARD_BG)),
        border: Border {
            color: CARD_STROKE,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    });

    let main_col = column![
        header_card,
        theme_card,
        hwdec_card,
        row![
            container(fps_card).width(Length::FillPortion(1)),
            container(opacity_card).width(Length::FillPortion(1)),
        ].spacing(14),
        audio_card,
        dir_card,
        autostart_card,
        slideshow_card,
        daemon_card,
    ]
    .spacing(16);

    scrollable(main_col).into()
}

fn render_system_doctor_modal<'a>(app: &'a IcedGuiApp) -> Element<'a, Message> {
    let tools = check_installed_tools();
    let total_count = tools.len();
    let installed_count = tools.iter().filter(|t| t.installed).count();
    let all_installed = installed_count == total_count;

    // Header title and count badge
    let summary_badge = container(
        text(format!("{}/{} INSTALLED", installed_count, total_count))
            .size(11)
            .color(if all_installed { EMERALD } else { AMBER })
    )
    .padding([4, 10])
    .style(move |_| container::Style {
        background: Some(Background::Color(if all_installed {
            Color::from_rgba(0.04, 0.20, 0.12, 0.90)
        } else {
            Color::from_rgba(0.24, 0.16, 0.04, 0.90)
        })),
        border: Border {
            color: if all_installed { EMERALD } else { AMBER },
            width: 1.0,
            radius: 12.0.into(),
        },
        ..Default::default()
    });

    let header_row = row![
        text("⚙ System Doctor & Dependency Diagnostics").size(18).color(CYAN),
        space().width(12),
        summary_badge,
        space().width(Length::Fill),
        button(text("✕ Close").size(13).color(SOFT_TEXT))
            .padding([6, 12])
            .style(|_, status| {
                let bg = match status {
                    iced::widget::button::Status::Hovered => Color::from_rgba(0.95, 0.25, 0.25, 0.2),
                    _ => Color::from_rgba(0.1, 0.12, 0.18, 0.8),
                };
                iced::widget::button::Style {
                    background: Some(Background::Color(bg)),
                    text_color: Color::WHITE,
                    border: Border {
                        color: CARD_STROKE,
                        width: 1.0,
                        radius: 6.0.into(),
                    },
                    shadow: iced::Shadow::default(),
                    snap: true,
                }
            })
            .on_press(Message::ToggleDoctor),
    ]
    .align_y(iced::Alignment::Center);

    let subtitle = text(
        "Diagnostic checklist for hardware-accelerated video decoding, WebGL rendering engines, IPC bridges, and desktop layer-shell integrations."
    )
    .size(12)
    .color(SOFT_TEXT);

    // Table Header
    let table_header = container(
        row![
            text("COMPONENT").size(11).color(DIM_TEXT).width(160),
            text("STATUS").size(11).color(DIM_TEXT).width(110),
            text("DETECTED PATH / CAPABILITY").size(11).color(DIM_TEXT).width(250),
            text("DESCRIPTION & ROLE").size(11).color(DIM_TEXT).width(Length::Fill),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center)
    )
    .padding([8, 12])
    .style(|_| container::Style {
        background: Some(Background::Color(Color::from_rgba(0.08, 0.10, 0.16, 0.90))),
        border: Border {
            color: CARD_STROKE,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    });

    let mut rows_col = column![].spacing(6);

    for (idx, t) in tools.into_iter().enumerate() {
        let is_alt = idx % 2 == 1;

        let status_badge = if t.installed {
            container(
                text("● Installed").size(11).color(EMERALD)
            )
            .padding([3, 8])
            .style(|_| container::Style {
                background: Some(Background::Color(Color::from_rgba(0.04, 0.20, 0.12, 0.85))),
                border: Border {
                    color: EMERALD,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            })
        } else {
            container(
                text("▲ Missing").size(11).color(Color::from_rgb(0.95, 0.25, 0.25))
            )
            .padding([3, 8])
            .style(|_| container::Style {
                background: Some(Background::Color(Color::from_rgba(0.25, 0.06, 0.06, 0.85))),
                border: Border {
                    color: Color::from_rgb(0.95, 0.25, 0.25),
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            })
        };

        let row_item = container(
            row![
                text(t.name).size(13).color(Color::WHITE).width(160),
                container(status_badge).width(110),
                text(t.path_or_info)
                    .size(11)
                    .color(if t.installed { SOFT_TEXT } else { Color::from_rgb(0.90, 0.45, 0.45) })
                    .width(250),
                text(t.description).size(12).color(DIM_TEXT).width(Length::Fill),
            ]
            .spacing(12)
            .align_y(iced::Alignment::Center)
        )
        .padding([8, 12])
        .style(move |_| container::Style {
            background: Some(Background::Color(if is_alt {
                Color::from_rgba(0.06, 0.08, 0.14, 0.60)
            } else {
                Color::from_rgba(0.04, 0.05, 0.09, 0.40)
            })),
            border: Border {
                color: Color::from_rgba(0.12, 0.16, 0.26, 0.40),
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        });

        rows_col = rows_col.push(row_item);
    }

    let scrollable_table = scrollable(rows_col)
        .height(Length::Fixed(320.0));

    // Action footer
    let mut footer_row = row![
        btn_primary("🛠 Run Auto-Fix / Install Missing Tools")
            .on_press(Message::RunInstaller),
        btn_secondary("🔄 Re-Check Dependencies")
            .on_press(Message::RecheckDoctor),
        space().width(Length::Fill),
    ]
    .spacing(12)
    .align_y(iced::Alignment::Center);

    if !app.status_message.is_empty() && app.status_message != "Ready" {
        footer_row = footer_row.push(
            text(&app.status_message).size(12).color(AMBER)
        );
        footer_row = footer_row.push(space().width(12));
    }

    footer_row = footer_row.push(
        btn_secondary("✕ Close Dialog")
            .on_press(Message::ToggleDoctor)
    );

    let modal_card = container(
        column![
            header_row,
            subtitle,
            rule::horizontal(1),
            table_header,
            scrollable_table,
            rule::horizontal(1),
            footer_row,
        ]
        .spacing(14)
    )
    .width(Length::Fixed(880.0))
    .max_height(600.0)
    .padding(22)
    .style(|_| container::Style {
        background: Some(Background::Color(Color::from_rgba(0.05, 0.07, 0.12, 0.97))),
        border: Border {
            color: CYAN,
            width: 1.5,
            radius: 12.0.into(),
        },
        shadow: iced::Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.75),
            offset: iced::Vector::new(0.0, 10.0),
            blur_radius: 28.0,
        },
        ..Default::default()
    });

    container(modal_card)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::Alignment::Center)
        .align_y(iced::Alignment::Center)
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgba(0.01, 0.02, 0.04, 0.78))),
            ..Default::default()
        })
        .into()
}

fn render_logs_modal<'a>(app: &'a IcedGuiApp) -> Element<'a, Message> {
    let log_path_display = get_log_path().display().to_string();

    let path_badge = container(
        text(log_path_display)
            .size(11)
            .color(SOFT_TEXT)
    )
    .padding([4, 10])
    .style(|_| container::Style {
        background: Some(Background::Color(Color::from_rgba(0.08, 0.10, 0.16, 0.90))),
        border: Border {
            color: CARD_STROKE,
            width: 1.0,
            radius: 10.0.into(),
        },
        ..Default::default()
    });

    let header_row = row![
        text("📋 Omywall Live System Logs").size(18).color(CYAN),
        space().width(12),
        path_badge,
        space().width(Length::Fill),
        button(text("✕ Close").size(13).color(SOFT_TEXT))
            .padding([6, 12])
            .style(|_, status| {
                let bg = match status {
                    iced::widget::button::Status::Hovered => Color::from_rgba(0.95, 0.25, 0.25, 0.2),
                    _ => Color::from_rgba(0.1, 0.12, 0.18, 0.8),
                };
                iced::widget::button::Style {
                    background: Some(Background::Color(bg)),
                    text_color: Color::WHITE,
                    border: Border {
                        color: CARD_STROKE,
                        width: 1.0,
                        radius: 6.0.into(),
                    },
                    shadow: iced::Shadow::default(),
                    snap: true,
                }
            })
            .on_press(Message::ToggleLogs),
    ]
    .align_y(iced::Alignment::Center);

    let logs_text_display = if app.logs_content.is_empty() {
        "No logs recorded yet. Background daemon events, IPC transactions, and render pipeline messages will appear here."
    } else {
        &app.logs_content
    };

    let log_box = container(
        scrollable(
            text(logs_text_display)
                .size(12)
                .color(Color::from_rgb(0.85, 0.90, 0.96))
        )
        .height(Length::Fill)
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .padding(14)
    .style(|_| container::Style {
        background: Some(Background::Color(Color::from_rgba(0.02, 0.03, 0.05, 0.95))),
        border: Border {
            color: Color::from_rgba(0.15, 0.20, 0.32, 0.80),
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    });

    let mut footer_row = row![
        btn_primary("🔄 Refresh Logs").on_press(Message::RefreshLogs),
        btn_secondary("📋 Copy to Clipboard").on_press(Message::CopyLogs),
        btn_danger("🧹 Clear Log File").on_press(Message::ClearLogs),
        space().width(Length::Fill),
    ]
    .spacing(12)
    .align_y(iced::Alignment::Center);

    if !app.status_message.is_empty() && app.status_message != "Ready" {
        footer_row = footer_row.push(
            text(&app.status_message).size(12).color(AMBER)
        );
        footer_row = footer_row.push(space().width(12));
    }

    footer_row = footer_row.push(
        btn_secondary("✕ Close Dialog")
            .on_press(Message::ToggleLogs)
    );

    let modal_card = container(
        column![
            header_row,
            rule::horizontal(1),
            log_box,
            rule::horizontal(1),
            footer_row,
        ]
        .spacing(14)
    )
    .width(Length::Fixed(880.0))
    .height(Length::Fixed(560.0))
    .padding(22)
    .style(|_| container::Style {
        background: Some(Background::Color(Color::from_rgba(0.04, 0.06, 0.10, 0.97))),
        border: Border {
            color: CYAN,
            width: 1.5,
            radius: 12.0.into(),
        },
        shadow: iced::Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.75),
            offset: iced::Vector::new(0.0, 10.0),
            blur_radius: 28.0,
        },
        ..Default::default()
    });

    container(modal_card)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::Alignment::Center)
        .align_y(iced::Alignment::Center)
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgba(0.01, 0.02, 0.04, 0.78))),
            ..Default::default()
        })
        .into()
}

