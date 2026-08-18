use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WebBookmark {
    pub title: String,
    pub url: String,
    pub category: String,
    pub is_demo: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HyprlockConfig {
    pub enabled: bool,
    pub background_path: String,
    pub screensaver_mode: String,
    pub asset_path: String,
    pub gradient_color: String,
    pub blur_passes: u32,
    pub blur_size: u32,
    pub clock_color: String,
    pub clock_size: u32,
    pub text_color: String,
    pub welcome_message: String,
    pub input_field_ring: String,
    pub input_field_fill: String,
    pub grace_period: u32,
}

impl Default for HyprlockConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            background_path: String::new(),
            screensaver_mode: "active".to_string(),
            asset_path: String::new(),
            gradient_color: "#121826".to_string(),
            blur_passes: 2,
            blur_size: 7,
            clock_color: "#00f0ff".to_string(),
            clock_size: 72,
            text_color: "#ffffff".to_string(),
            welcome_message: "Welcome back, $USER!".to_string(),
            input_field_ring: "#00f0ff".to_string(),
            input_field_fill: "#0b0d14".to_string(),
            grace_period: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct WallpaperOverrides {
    #[serde(default)]
    pub volume: Option<i64>,
    #[serde(default)]
    pub silent: Option<bool>,
    #[serde(default)]
    pub scaling: Option<String>,
    #[serde(default)]
    pub fps: Option<u32>,
    #[serde(default)]
    pub disable_mouse: Option<bool>,
    #[serde(default)]
    pub disable_parallax: Option<bool>,
    #[serde(default)]
    pub disable_particles: Option<bool>,
    #[serde(default)]
    pub clamp: Option<String>,
    #[serde(default)]
    pub layer: Option<String>,
    #[serde(default)]
    pub no_automute: Option<bool>,
    #[serde(default)]
    pub no_audio_processing: Option<bool>,
    #[serde(default)]
    pub no_fullscreen_pause: Option<bool>,
    #[serde(default)]
    pub fullscreen_pause_only_active: Option<bool>,
    #[serde(default)]
    pub screenshot: Option<String>,
    #[serde(default)]
    pub custom_properties: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub wallpaper_dir: PathBuf,
    pub socket_path: PathBuf,
    pub hwdec: String,
    #[serde(default)]
    pub gpu_device: Option<String>,
    #[serde(default = "default_fps")]
    pub target_fps: u32,
    pub volume: i64,
    pub mute: bool,
    pub loop_file: String,
    pub window_id: u64,
    pub screen_id: i64,
    pub slideshow_interval: u64,
    pub slideshow_shuffle: bool,
    pub default_wallpaper: Option<PathBuf>,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default)]
    pub enable_widgets: bool,
    #[serde(default)]
    pub widget_url: Option<String>,
    #[serde(default = "default_widget_position")]
    pub widget_position: String,
    #[serde(default)]
    pub monitor_wallpapers: HashMap<String, String>,
    #[serde(default = "default_web_bookmarks")]
    pub saved_web_wallpapers: Vec<WebBookmark>,
    #[serde(default)]
    pub autostart: bool,
    #[serde(default)]
    pub hyprlock: HyprlockConfig,
    #[serde(default)]
    pub wallpaper_overrides: HashMap<String, WallpaperOverrides>,
    #[serde(default)]
    pub steam_library_paths: Vec<PathBuf>,
}

fn default_widget_position() -> String {
    "top_right".to_string()
}

fn default_opacity() -> f32 {
    1.0
}

fn default_fps() -> u32 {
    60
}

pub fn humanize_title(stem: &str) -> String {
    let clean_stem = stem.trim_start_matches(|c: char| c.is_ascii_digit() || c == '-' || c == '_');
    let target = if clean_stem.is_empty() { stem } else { clean_stem };
    target.split(['-', '_'])
        .filter(|s| !s.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn resolve_widgets_dir() -> Option<PathBuf> {
    let relative = PathBuf::from("assets").join("widgets");
    if let Some(home) = dirs::home_dir() {
        let candidates = [
            home.join(".local").join("share").join("omywall").join("assets").join("widgets"),
            PathBuf::from("/usr/share/omywall").join("assets").join("widgets"),
            PathBuf::from("/usr/local/share/omywall").join("assets").join("widgets"),
            std::env::current_dir().unwrap_or_default().join(&relative),
        ];
        for c in &candidates {
            if c.is_dir() {
                return Some(c.to_path_buf());
            }
        }
    }
    let cwd = std::env::current_dir().unwrap_or_default().join(relative);
    if cwd.is_dir() {
        Some(cwd)
    } else {
        None
    }
}

pub fn scan_web_asset_bookmarks() -> Vec<WebBookmark> {
    let mut bookmarks = Vec::new();
    if let Some(web_dir) = resolve_web_assets_dir() {
        if let Ok(entries) = fs::read_dir(&web_dir) {
            let mut categories: Vec<PathBuf> = Vec::new();
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    categories.push(p);
                } else if p.is_file() {
                    if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                        if ext.eq_ignore_ascii_case("html") || ext.eq_ignore_ascii_case("htm") {
                            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("wallpaper");
                            bookmarks.push(WebBookmark {
                                title: humanize_title(stem),
                                url: format!("assets/web_wallpapers/{}", p.file_name().unwrap_or_default().to_string_lossy()),
                                category: "3D WebGL".to_string(),
                                is_demo: true,
                            });
                        }
                    }
                }
            }

            categories.sort_by_key(|c| c.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
            for cat in categories {
                let cat_name = cat.file_name().and_then(|n| n.to_str()).unwrap_or("Misc").to_string();
                if let Ok(files) = fs::read_dir(&cat) {
                    let mut cat_files: Vec<PathBuf> = Vec::new();
                    for f in files.flatten() {
                        let p = f.path();
                        if p.is_file() {
                            if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                                if ext.eq_ignore_ascii_case("html") || ext.eq_ignore_ascii_case("htm") {
                                    cat_files.push(p);
                                }
                            }
                        }
                    }
                    cat_files.sort();
                    for p in cat_files {
                        let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("wallpaper");
                        let url = format!("assets/web_wallpapers/{}/{}", cat_name, p.file_name().unwrap_or_default().to_string_lossy());
                        bookmarks.push(WebBookmark {
                            title: humanize_title(stem),
                            url,
                            category: humanize_title(&cat_name),
                            is_demo: true,
                        });
                    }
                }
            }
        }
    }

    if let Some(widget_dir) = resolve_widgets_dir() {
        if let Ok(entries) = fs::read_dir(&widget_dir) {
            let mut widget_files: Vec<PathBuf> = Vec::new();
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() {
                    if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                        if ext.eq_ignore_ascii_case("html") || ext.eq_ignore_ascii_case("htm") {
                            widget_files.push(p);
                        }
                    }
                }
            }
            widget_files.sort();
            for p in widget_files {
                let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("widget");
                let url = format!("assets/widgets/{}", p.file_name().unwrap_or_default().to_string_lossy());
                bookmarks.push(WebBookmark {
                    title: humanize_title(stem),
                    url,
                    category: "Desktop Widgets".to_string(),
                    is_demo: true,
                });
            }
        }
    }

    bookmarks
}

pub fn default_web_bookmarks() -> Vec<WebBookmark> {
    let scanned = scan_web_asset_bookmarks();
    if !scanned.is_empty() {
        return scanned;
    }

    vec![
        WebBookmark {
            title: "Desktop HUD (System Telemetry & Weather)".to_string(),
            url: "assets/widgets/desktop_hud.html".to_string(),
            category: "Desktop Widgets".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "Wifi & Bluetooth Status Pill".to_string(),
            url: "assets/widgets/wifi_bluetooth_pill.html".to_string(),
            category: "Desktop Widgets".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "Minimal Clock & System Stats".to_string(),
            url: "assets/widgets/minimal_clock_stats.html".to_string(),
            category: "Desktop Widgets".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "WebGL Fluid Simulation (Navier-Stokes)".to_string(),
            url: "assets/web_wallpapers/01-fluid-particles/webgl_fluid_simulation.html".to_string(),
            category: "Fluid & Particles".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "tsParticles Constellation Network".to_string(),
            url: "assets/web_wallpapers/01-fluid-particles/tsparticles_constellation.html".to_string(),
            category: "Fluid & Particles".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "FieldPlay Vector Flow Field".to_string(),
            url: "assets/web_wallpapers/01-fluid-particles/fieldplay_vector_flow.html".to_string(),
            category: "Fluid & Particles".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "Particle Image Dispersion".to_string(),
            url: "assets/web_wallpapers/01-fluid-particles/particle_image_dispersion.html".to_string(),
            category: "Fluid & Particles".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "Vanta 3D Waves & Topology".to_string(),
            url: "assets/web_wallpapers/02-3d-procedural/vanta_waves_topology.html".to_string(),
            category: "3D Procedural".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "Three.js PathTracing Glass Refraction".to_string(),
            url: "assets/web_wallpapers/02-3d-procedural/pathtracing_glass_refraction.html".to_string(),
            category: "3D Procedural".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "3D Point Cloud Sculpture".to_string(),
            url: "assets/web_wallpapers/02-3d-procedural/point_cloud_3d_sculpture.html".to_string(),
            category: "3D Procedural".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "Three.js Ocean & Glitch Visualizer".to_string(),
            url: "assets/web_wallpapers/02-3d-procedural/threejs_ocean_glitch_visualizer.html".to_string(),
            category: "3D Procedural".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "Rezmason 3D Matrix Digital Rain".to_string(),
            url: "assets/web_wallpapers/03-retro-cyberpunk/rezmason_matrix_3d.html".to_string(),
            category: "Retro & Cyberpunk".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "80s Retro Grid Synthwave Outrun".to_string(),
            url: "assets/web_wallpapers/03-retro-cyberpunk/retro_grid_synthwave.html".to_string(),
            category: "Retro & Cyberpunk".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "v86 Retro OS Canvas Terminal".to_string(),
            url: "assets/web_wallpapers/03-retro-cyberpunk/v86_os_canvas_terminal.html".to_string(),
            category: "Retro & Cyberpunk".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "25th Hour Dynamic Day/Night Landscape".to_string(),
            url: "assets/web_wallpapers/04-astronomy-clocks/twenty_fifth_hour_daynight.html".to_string(),
            category: "Astronomy & Clocks".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "Astronomical Star Chart & Sky".to_string(),
            url: "assets/web_wallpapers/04-astronomy-clocks/astronomy_canvas_stars.html".to_string(),
            category: "Astronomy & Clocks".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "Orbital Minimalist Chrono Clock".to_string(),
            url: "assets/web_wallpapers/04-astronomy-clocks/orbital_minimal_clock.html".to_string(),
            category: "Astronomy & Clocks".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "GL Audio Spectrum GPU Analyzer".to_string(),
            url: "assets/web_wallpapers/05-audio-shaders/gl_audio_spectrum_analyzer.html".to_string(),
            category: "Audio & Shaders".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "Circular Waveform Audio Visualizer".to_string(),
            url: "assets/web_wallpapers/05-audio-shaders/circular_waveform_audiovisualizer.html".to_string(),
            category: "Audio & Shaders".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "glslCanvas Live GLSL Shader Engine".to_string(),
            url: "assets/web_wallpapers/05-audio-shaders/glsl_canvas_live_shader.html".to_string(),
            category: "Audio & Shaders".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "Three.js Postprocessing Bloom & Depth".to_string(),
            url: "assets/web_wallpapers/05-audio-shaders/threejs_postprocessing_bloom.html".to_string(),
            category: "Audio & Shaders".to_string(),
            is_demo: true,
        },
    ]
}

impl Default for Config {
    fn default() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/home/user"));
        let default_wall_dir = home.join(".local").join("share").join("omywall").join("wallpapers");
        let _ = std::fs::create_dir_all(&default_wall_dir);

        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/tmp"));
        let socket_path = runtime_dir.join("omywall.sock");

        Self {
            wallpaper_dir: default_wall_dir,
            socket_path,
            hwdec: "auto".to_string(),
            gpu_device: None,
            target_fps: 60,
            volume: 0,
            mute: true,
            loop_file: "inf".to_string(),
            window_id: 0,
            screen_id: 0,
            slideshow_interval: 300,
            slideshow_shuffle: false,
            default_wallpaper: None,
            opacity: 1.0,
            enable_widgets: false,
            widget_url: None,
            widget_position: "top_right".to_string(),
            monitor_wallpapers: HashMap::new(),
            saved_web_wallpapers: default_web_bookmarks(),
            autostart: Self::is_autostart_enabled(),
            hyprlock: HyprlockConfig::default(),
            wallpaper_overrides: HashMap::new(),
            steam_library_paths: Vec::new(),
        }
    }
}

impl Config {
    pub fn config_path() -> PathBuf {
        let base_config = dirs::config_dir().unwrap_or_else(|| PathBuf::from("/home/user/.config"));
        let lua_path = base_config.join("omywall").join("config.lua");
        let toml_path = base_config.join("omywall").join("config.toml");
        let old_path = base_config.join("omarchy-wall").join("config.toml");

        if !lua_path.exists() {
            if toml_path.exists() {
                if let Ok(content) = fs::read_to_string(&toml_path) {
                    if let Ok(cfg) = toml::from_str::<Config>(&content) {
                        let _ = cfg.save_lua_to_path(&lua_path);
                    }
                }
            } else if old_path.exists() {
                if let Ok(content) = fs::read_to_string(&old_path) {
                    if let Ok(cfg) = toml::from_str::<Config>(&content) {
                        let _ = cfg.save_lua_to_path(&lua_path);
                    }
                }
            }
        }
        lua_path
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                let lua = mlua::Lua::new();
                use mlua::LuaSerdeExt;
                let parsed: Result<Config, _> = lua.load(&content).eval::<mlua::Value>().and_then(|val| lua.from_value::<Config>(val));
                if let Ok(mut cfg) = parsed {
                    cfg.saved_web_wallpapers.retain(|b| {
                        if b.url.contains("clock.html") || b.url.contains("cyber_clock.html") {
                            return false;
                        }
                        let resolved = resolve_asset_path(&b.url);
                        b.url.starts_with("http://") || b.url.starts_with("https://") || Path::new(&resolved).exists()
                    });

                    for default_bm in default_web_bookmarks() {
                        if !cfg.saved_web_wallpapers.iter().any(|b| b.url == default_bm.url) {
                            cfg.saved_web_wallpapers.push(default_bm);
                        }
                    }

                    cfg.autostart = Self::is_autostart_enabled();
                    let _ = cfg.save();
                    return cfg;
                }
            }
        }
        let cfg = Config::default();
        let _ = cfg.save();
        cfg
    }


    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let path = Self::config_path();
        self.save_lua_to_path(&path)
    }

    pub fn save_lua_to_path(&self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let lua_content = format_config_as_lua(self)?;
        fs::write(path, lua_content)?;
        Ok(())
    }


    pub fn is_autostart_enabled() -> bool {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/home/user"));
        let desktop_file = home.join(".config/autostart/omywall.desktop");
        desktop_file.exists()
    }

    pub fn set_autostart(enable: bool) -> Result<(), Box<dyn std::error::Error>> {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/home/user"));
        let autostart_dir = home.join(".config/autostart");
        let target = autostart_dir.join("omywall.desktop");

        if enable {
            fs::create_dir_all(&autostart_dir)?;
            let exe_path = std::env::current_exe()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| format!("{}/.local/bin/omywall", home.display()));

            let desktop_content = format!(
                "[Desktop Entry]\n\
                 Type=Application\n\
                 Name=OMYWALL Wallpaper Engine\n\
                 GenericName=Live Video, Stream & Desktop Wallpaper Engine\n\
                 Comment=Ultra-Lightweight Hardware-Accelerated Video, Stream & Desktop Wallpaper Engine\n\
                 Exec={} daemon\n\
                 Icon=omywall\n\
                 Terminal=false\n\
                 Categories=Utility;Appearance;\n\
                 X-GNOME-Autostart-enabled=true\n",
                exe_path
            );
            fs::write(target, desktop_content)?;
        } else if target.exists() {
            let _ = fs::remove_file(target);
        }
        Ok(())
    }


    pub fn add_web_bookmark(&mut self, title: String, url: String, category: String) {
        let bookmark = WebBookmark {
            title,
            url: url.clone(),
            category,
            is_demo: false,
        };
        if let Some(existing) = self.saved_web_wallpapers.iter_mut().find(|b| b.url == url) {
            *existing = bookmark;
        } else {
            self.saved_web_wallpapers.push(bookmark);
        }
        let _ = self.save();
    }

    pub fn remove_web_bookmark(&mut self, url: &str) {
        let resolved = resolve_asset_path(url);
        self.saved_web_wallpapers.retain(|b| {
            b.url != url
                && b.url.trim() != url.trim()
                && resolve_asset_path(&b.url) != resolved
        });
        let _ = self.save();
    }

    pub fn get_monitor_wallpaper(&self, mon: &str) -> Option<&String> {
        let candidates = [mon.to_string(), mon.to_lowercase()];
        for key in &candidates {
            if let Some(path) = self.monitor_wallpapers.get(key) {
                if !path.trim().is_empty() {
                    return Some(path);
                }
            }
        }
        None
    }

    pub fn generate_hyprlock_conf(&self, active_wallpaper: Option<&str>) -> String {
        let raw_bg = match self.hyprlock.screensaver_mode.as_str() {
            "video" | "web" | "image" => {
                if !self.hyprlock.asset_path.trim().is_empty() {
                    self.hyprlock.asset_path.clone()
                } else if !self.hyprlock.background_path.trim().is_empty() {
                    self.hyprlock.background_path.clone()
                } else if let Some(wall) = active_wallpaper {
                    wall.to_string()
                } else {
                    "screenshot".to_string()
                }
            }
            "gradient" => {
                if !self.hyprlock.gradient_color.trim().is_empty() {
                    self.hyprlock.gradient_color.clone()
                } else {
                    "#121826".to_string()
                }
            }
            _ => {
                if !self.hyprlock.background_path.trim().is_empty() {
                    self.hyprlock.background_path.clone()
                } else if let Some(wall) = active_wallpaper {
                    wall.to_string()
                } else {
                    "screenshot".to_string()
                }
            }
        };

        let bg_path = if raw_bg != "screenshot" && !raw_bg.starts_with('#') {
            let ext = Path::new(&raw_bg).extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
            if matches!(ext.as_str(), "mp4" | "mkv" | "webm" | "avi" | "mov" | "gif" | "html" | "htm" | "js") {
                if let Some(thumb) = crate::gui::get_web_thumbnail_path(&raw_bg) {
                    thumb.to_string_lossy().to_string()
                } else {
                    raw_bg
                }
            } else {
                raw_bg
            }
        } else {
            raw_bg
        };

        let user_name = std::env::var("USER").unwrap_or_else(|_| "User".to_string());
        let welcome = self.hyprlock.welcome_message.replace("$USER", &user_name);

        format!(
            "# Generated by OMYWALL Wallpaper Engine\n\
            background {{\n\
                monitor =\n\
                path = {}\n\
                blur_passes = {}\n\
                blur_size = {}\n\
                noise = 0.0117\n\
                contrast = 0.8916\n\
                brightness = 0.8172\n\
                vibrancy = 0.1696\n\
                vibrancy_darkness = 0.0\n\
            }}\n\n\
            # TIME / CLOCK DISPLAY\n\
            label {{\n\
                monitor =\n\
                text = cmd[update:1000] echo \"$(date +\"%H:%M:%S\")\"\n\
                color = {}\n\
                font_size = {}\n\
                font_family = Outfit, Inter, sans-serif\n\
                position = 0, 160\n\
                halign = center\n\
                valign = center\n\
                shadow_passes = 2\n\
                shadow_size = 4\n\
            }}\n\n\
            # DATE DISPLAY\n\
            label {{\n\
                monitor =\n\
                text = cmd[update:1000] echo \"$(date +\"%A, %B %d, %Y\")\"\n\
                color = {}\n\
                font_size = 20\n\
                font_family = Inter, sans-serif\n\
                position = 0, 80\n\
                halign = center\n\
                valign = center\n\
            }}\n\n\
            # WELCOME / CUSTOM MESSAGE\n\
            label {{\n\
                monitor =\n\
                text = {}\n\
                color = {}\n\
                font_size = 22\n\
                font_family = Inter, sans-serif\n\
                position = 0, 20\n\
                halign = center\n\
                valign = center\n\
            }}\n\n\
            # PASSWORD INPUT FIELD\n\
            input-field {{\n\
                monitor =\n\
                size = 260, 50\n\
                outline_thickness = 3\n\
                dots_size = 0.33\n\
                dots_spacing = 0.15\n\
                dots_center = true\n\
                outer_color = {}\n\
                inner_color = {}\n\
                font_color = {}\n\
                fade_on_empty = false\n\
                placeholder_text = <i>Input Password...</i>\n\
                hide_input = false\n\
                position = 0, -80\n\
                halign = center\n\
                valign = center\n\
            }}\n",
            bg_path,
            self.hyprlock.blur_passes,
            self.hyprlock.blur_size,
            self.hyprlock.clock_color,
            self.hyprlock.clock_size,
            self.hyprlock.text_color,
            welcome,
            self.hyprlock.text_color,
            self.hyprlock.input_field_ring,
            self.hyprlock.input_field_fill,
            self.hyprlock.text_color
        )
    }

    pub fn save_hyprlock_conf(&self, active_wallpaper: Option<&str>) -> Result<PathBuf, String> {
        let home = dirs::home_dir().ok_or_else(|| "Could not find home directory".to_string())?;
        let hypr_dir = home.join(".config").join("hypr");
        fs::create_dir_all(&hypr_dir).map_err(|e| e.to_string())?;

        let conf_file = hypr_dir.join("hyprlock.conf");
        let content = self.generate_hyprlock_conf(active_wallpaper);
        fs::write(&conf_file, content).map_err(|e| e.to_string())?;
        Ok(conf_file)
    }
}

pub fn resolve_web_assets_dir() -> Option<PathBuf> {
    let relative = PathBuf::from("assets").join("web_wallpapers");
    if let Some(home) = dirs::home_dir() {
        let candidates = [
            home.join(".local").join("share").join("omywall").join("assets").join("web_wallpapers"),
            PathBuf::from("/usr/share/omywall").join("assets").join("web_wallpapers"),
            PathBuf::from("/usr/local/share/omywall").join("assets").join("web_wallpapers"),
            std::env::current_dir().unwrap_or_default().join(&relative),
        ];
        for c in &candidates {
            if c.is_dir() {
                return Some(c.to_path_buf());
            }
        }
    }
    let cwd = std::env::current_dir().unwrap_or_default().join(relative);
    if cwd.is_dir() {
        Some(cwd)
    } else {
        None
    }
}

pub fn resolve_asset_path(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") || trimmed.starts_with("file://") {
        return trimmed.to_string();
    }

    let p = std::path::Path::new(trimmed);
    if p.is_absolute() && p.exists() {
        return std::fs::canonicalize(p)
            .unwrap_or_else(|_| p.to_path_buf())
            .to_string_lossy()
            .to_string();
    }
    if p.exists() {
        return std::fs::canonicalize(p)
            .unwrap_or_else(|_| p.to_path_buf())
            .to_string_lossy()
            .to_string();
    }

    let stripped = trimmed.strip_prefix("assets/").unwrap_or(trimmed);
    let cwd = std::env::current_dir().unwrap_or_default();

    let mut candidates = vec![
        cwd.join(trimmed),
        cwd.join("assets").join(stripped),
    ];

    if let Some(home) = dirs::home_dir() {
        candidates.extend_from_slice(&[
            home.join(".local").join("share").join("omywall").join("assets").join(stripped),
            home.join(".local").join("share").join("omywall").join(trimmed),
            home.join(".config").join("omywall").join(trimmed),
            PathBuf::from("/usr/share/omywall").join("assets").join(stripped),
            PathBuf::from("/usr/share/omywall").join(trimmed),
            PathBuf::from("/usr/local/share/omywall").join("assets").join(stripped),
            PathBuf::from("/usr/local/share/omywall").join(trimmed),
        ]);
    }

    for c in &candidates {
        if c.exists() {
            return std::fs::canonicalize(c)
                .unwrap_or_else(|_| c.clone())
                .to_string_lossy()
                .to_string();
        }
    }

    trimmed.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GpuInfo {
    pub name: String,
    pub vendor: String,
    pub device_path: Option<String>,
    pub is_primary: bool,
}

pub fn detect_system_gpus() -> Vec<GpuInfo> {
    let mut gpus = Vec::new();

    if let Ok(output) = std::process::Command::new("lspci").output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if line.contains("VGA compatible controller") || line.contains("3D controller") || line.contains("Display controller") {
                let name = if let Some(pos) = line.find(": ") {
                    line[pos + 2..].to_string()
                } else {
                    line.to_string()
                };

                let vendor = if name.to_lowercase().contains("nvidia") {
                    "NVIDIA".to_string()
                } else if name.to_lowercase().contains("amd") || name.to_lowercase().contains("radeon") {
                    "AMD".to_string()
                } else if name.to_lowercase().contains("intel") {
                    "Intel".to_string()
                } else {
                    "Generic".to_string()
                };

                gpus.push(GpuInfo {
                    name,
                    vendor,
                    device_path: None,
                    is_primary: gpus.is_empty(),
                });
            }
        }
    }

    if let Ok(entries) = std::fs::read_dir("/sys/class/drm") {
        let mut idx = 0;
        for entry in entries.flatten() {
            let filename = entry.file_name().to_string_lossy().to_string();
            if filename.starts_with("renderD") {
                let dev_path = format!("/dev/dri/{}", filename);
                if idx < gpus.len() {
                    gpus[idx].device_path = Some(dev_path);
                } else {
                    gpus.push(GpuInfo {
                        name: format!("GPU Render Node ({})", filename),
                        vendor: "DRM/KMS".to_string(),
                        device_path: Some(dev_path),
                        is_primary: gpus.is_empty(),
                    });
                }
                idx += 1;
            }
        }
    }

    if gpus.is_empty() {
        gpus.push(GpuInfo {
            name: "Auto-Detected Graphics Processing Unit".to_string(),
            vendor: "Auto".to_string(),
            device_path: None,
            is_primary: true,
        });
    }

    gpus
}

#[allow(dead_code)]
pub fn get_available_hwdec_options() -> Vec<(&'static str, &'static str, &'static str)> {
    let mut options = vec![("auto", "⚡ Auto-Detect GPU (Recommended)", "Automatic hardware acceleration detection")];
    let gpus = detect_system_gpus();

    let has_nvidia = gpus.iter().any(|g| g.vendor == "NVIDIA") || std::path::Path::new("/dev/nvidia0").exists() || std::path::Path::new("/proc/driver/nvidia").exists();
    let has_intel_amd = gpus.iter().any(|g| g.vendor == "Intel" || g.vendor == "AMD") || std::path::Path::new("/dev/dri/renderD128").exists();

    if has_nvidia {
        options.push(("nvdec", "💚 NVIDIA NVDEC", "NVIDIA NVDEC Hardware Video Decoder"));
        options.push(("cuda", "⚡ NVIDIA CUDA Acceleration", "NVIDIA CUDA Hardware Video Acceleration"));
    }

    if has_intel_amd {
        options.push(("vaapi", "🔷 VA-API (Intel / AMD GPU)", "Linux VA-API Hardware Video Acceleration"));
    }

    if std::path::Path::new("/usr/lib/libvulkan.so.1").exists() 
        || std::path::Path::new("/usr/lib64/libvulkan.so.1").exists()
        || std::path::Path::new("/usr/lib/x86_64-linux-gnu/libvulkan.so.1").exists() {
        options.push(("vulkan", "🌋 Vulkan Video", "Modern Vulkan Hardware Video Decoder"));
    }

    options.push(("no", "⚙️ CPU (Software Only)", "Software video decoding using CPU cores"));
    options
}

static LAST_CPU_IDLE: AtomicU64 = AtomicU64::new(0);
static LAST_CPU_TOTAL: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SystemMetrics {
    pub cpu_usage: f32,
    pub ram_used_mb: u64,
    pub ram_total_mb: u64,
    pub gpu_usage: f32,
    pub vram_used_mb: u64,
    pub gpu_name: String,
}

pub fn get_system_metrics() -> SystemMetrics {
    let mut cpu_usage = 0.0f32;
    let mut ram_used_mb = 0u64;
    let mut ram_total_mb = 0u64;
    let mut gpu_usage = 0.0f32;
    let mut vram_used_mb = 0u64;
    let mut gpu_name = String::new();

    if let Ok(stat) = fs::read_to_string("/proc/stat") {
        if let Some(first_line) = stat.lines().next() {
            let parts: Vec<&str> = first_line.split_whitespace().collect();
            if parts.len() >= 5 {
                let user: u64 = parts[1].parse().unwrap_or(0);
                let nice: u64 = parts[2].parse().unwrap_or(0);
                let system: u64 = parts[3].parse().unwrap_or(0);
                let idle: u64 = parts[4].parse().unwrap_or(0);
                let iowait: u64 = parts.get(5).and_then(|p| p.parse().ok()).unwrap_or(0);
                let irq: u64 = parts.get(6).and_then(|p| p.parse().ok()).unwrap_or(0);
                let softirq: u64 = parts.get(7).and_then(|p| p.parse().ok()).unwrap_or(0);

                let total_idle = idle + iowait;
                let total = user + nice + system + idle + iowait + irq + softirq;

                let prev_idle = LAST_CPU_IDLE.swap(total_idle, Ordering::Relaxed);
                let prev_total = LAST_CPU_TOTAL.swap(total, Ordering::Relaxed);

                let delta_total = total.saturating_sub(prev_total);
                let delta_idle = total_idle.saturating_sub(prev_idle);

                if delta_total > 0 {
                    cpu_usage = ((delta_total.saturating_sub(delta_idle)) as f32 / delta_total as f32) * 100.0;
                }
            }
        }
    }

    if let Ok(mem) = fs::read_to_string("/proc/meminfo") {
        let mut total_kb = 0u64;
        let mut avail_kb = 0u64;
        for line in mem.lines() {
            if line.starts_with("MemTotal:") {
                total_kb = line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
            } else if line.starts_with("MemAvailable:") {
                avail_kb = line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
            }
        }
        ram_total_mb = total_kb / 1024;
        ram_used_mb = (total_kb.saturating_sub(avail_kb)) / 1024;
    }

    if let Ok(out) = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=utilization.gpu,memory.used,name", "--format=csv,noheader,nounits"])
        .output()
    {
        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let parts: Vec<&str> = stdout.trim().split(',').map(|s| s.trim()).collect();
            if parts.len() >= 2 {
                gpu_usage = parts[0].parse().unwrap_or(0.0);
                vram_used_mb = parts[1].parse().unwrap_or(0);
                if parts.len() >= 3 && !parts[2].is_empty() {
                    gpu_name = parts[2].to_string();
                }
            }
        }
    }

    if gpu_name.is_empty() {
        for card_idx in 0..=3 {
            let busy_path = format!("/sys/class/drm/card{}/device/gpu_busy_percent", card_idx);
            if let Ok(busy) = fs::read_to_string(&busy_path) {
                if let Ok(val) = busy.trim().parse::<f32>() {
                    gpu_usage = val;
                    break;
                }
            }
        }
        let gpus = detect_system_gpus();
        if let Some(gpu) = gpus.first() {
            gpu_name = gpu.name.clone();
        } else {
            gpu_name = "Auto Graphics Processing Unit".to_string();
        }
    }

    SystemMetrics {
        cpu_usage: cpu_usage.clamp(0.0, 100.0),
        ram_used_mb,
        ram_total_mb,
        gpu_usage: gpu_usage.clamp(0.0, 100.0),
        vram_used_mb,
        gpu_name,
    }
}

fn is_valid_lua_identifier(s: &str) -> bool {
    let keywords = [
        "and", "break", "do", "else", "elseif", "end", "false", "for",
        "function", "goto", "if", "in", "local", "nil", "not", "or",
        "repeat", "return", "then", "true", "until", "while",
    ];
    if keywords.contains(&s) || s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_alphabetic() && first != '_' {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn json_val_to_lua(val: &serde_json::Value, indent: usize) -> String {
    let ind = "  ".repeat(indent);
    match val {
        serde_json::Value::Null => "nil".to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => format!("{:?}", s),
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                "{}".to_string()
            } else {
                let inner_ind = "  ".repeat(indent + 1);
                let items: Vec<String> = arr.iter().map(|item| format!("{}{}", inner_ind, json_val_to_lua(item, indent + 1))).collect();
                format!("{{\n{}\n{}}}", items.join(",\n"), ind)
            }
        }
        serde_json::Value::Object(obj) => {
            if obj.is_empty() {
                "{}".to_string()
            } else {
                let inner_ind = "  ".repeat(indent + 1);
                let mut keys: Vec<&String> = obj.keys().collect();
                keys.sort();
                let items: Vec<String> = keys
                    .into_iter()
                    .map(|k| {
                        let v = &obj[k];
                        let key_str = if is_valid_lua_identifier(k) {
                            k.clone()
                        } else {
                            format!("[{:?}]", k)
                        };
                        format!("{}{key_str} = {}", inner_ind, json_val_to_lua(v, indent + 1))
                    })
                    .collect();
                format!("{{\n{}\n{}}}", items.join(",\n"), ind)
            }
        }

    }
}

pub fn format_config_as_lua(cfg: &Config) -> Result<String, Box<dyn std::error::Error>> {
    let json_val = serde_json::to_value(cfg)?;
    let lua_tbl = json_val_to_lua(&json_val, 0);
    Ok(format!("-- OMYWALL Wallpaper Engine Configuration (Lua Script)\nreturn {}\n", lua_tbl))
}

