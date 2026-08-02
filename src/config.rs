use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

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
    #[serde(default)]
    pub monitor_wallpapers: HashMap<String, String>,
    #[serde(default = "default_web_bookmarks")]
    pub saved_web_wallpapers: Vec<WebBookmark>,
    #[serde(default)]
    pub autostart: bool,
    #[serde(default)]
    pub hyprlock: HyprlockConfig,
}

fn default_opacity() -> f32 {
    1.0
}

fn default_fps() -> u32 {
    60
}

fn default_web_bookmarks() -> Vec<WebBookmark> {
    vec![
        WebBookmark {
            title: "Neon OLED Liquid Fluid 3D (Mouse Follow)".to_string(),
            url: "assets/web_wallpapers/neon_oled_fluid_mouse_3d.html".to_string(),
            category: "OLED Interactive Fluid".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "OLED Cosmic Aurora 3D (Interactive Gravity)".to_string(),
            url: "assets/web_wallpapers/oled_cosmic_aurora_interactive.html".to_string(),
            category: "OLED Mouse Distortion".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "3D Holographic Black Hole".to_string(),
            url: "assets/web_wallpapers/holographic_blackhole_3d.html".to_string(),
            category: "3D Gravitational Physics".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "3D Cyberpunk Neon Skyline".to_string(),
            url: "assets/web_wallpapers/cyberpunk_city_3d.html".to_string(),
            category: "3D Cyberpunk City".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "3D Aurora Borealis Lights".to_string(),
            url: "assets/web_wallpapers/aurora_borealis_3d.html".to_string(),
            category: "3D Cosmic Aurora".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "3D Quantum Energy Field".to_string(),
            url: "assets/web_wallpapers/quantum_field_3d.html".to_string(),
            category: "3D Quantum Dynamics".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "3D Sacred Geometry Tesseract".to_string(),
            url: "assets/web_wallpapers/geometry_wireframe_3d.html".to_string(),
            category: "3D Laser Geometry".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "3D Cyber Hyperspace Tunnel".to_string(),
            url: "assets/web_wallpapers/cyber_tunnel_3d.html".to_string(),
            category: "3D WebGL / Canvas".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "3D Synthwave Horizon".to_string(),
            url: "assets/web_wallpapers/neon_synthwave_3d.html".to_string(),
            category: "3D Synthwave".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "3D Cosmic Nebula Vortex".to_string(),
            url: "assets/web_wallpapers/cosmic_nebula_3d.html".to_string(),
            category: "3D Space Particles".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "3D Solar Energy Fluid".to_string(),
            url: "assets/web_wallpapers/solar_fluid_3d.html".to_string(),
            category: "3D Fluid Dynamics".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "Matrix Digital Rain".to_string(),
            url: "assets/web_wallpapers/matrix_rain.html".to_string(),
            category: "Cyberpunk Rain".to_string(),
            is_demo: true,
        },
    ]
}

impl Default for Config {
    fn default() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/home/user"));
        let default_wall_dir = home.join("Pictures").join("Wallpapers");

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
            monitor_wallpapers: HashMap::new(),
            saved_web_wallpapers: default_web_bookmarks(),
            autostart: Self::is_autostart_enabled(),
            hyprlock: HyprlockConfig::default(),
        }
    }
}

impl Config {
    pub fn config_path() -> PathBuf {
        let base_config = dirs::config_dir().unwrap_or_else(|| PathBuf::from("/home/user/.config"));
        let new_path = base_config.join("omywall").join("config.toml");
        let old_path = base_config.join("omarchy-wall").join("config.toml");

        if !new_path.exists() && old_path.exists() {
            // Migrate old config if present
            if let Some(parent) = new_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::copy(&old_path, &new_path);
        }
        new_path
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(mut cfg) = toml::from_str::<Config>(&content) {
                    if cfg.saved_web_wallpapers.is_empty() {
                        cfg.saved_web_wallpapers = default_web_bookmarks();
                    }
                    cfg.autostart = Self::is_autostart_enabled();
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
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        fs::write(path, content)?;
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
            let desktop_content = r#"[Desktop Entry]
Type=Application
Name=OMYWALL Wallpaper Engine
GenericName=Live Video, Stream & Desktop Wallpaper Engine
Comment=Ultra-Lightweight Hardware-Accelerated Video, Stream & Desktop Wallpaper Engine
Exec=omywall daemon
Icon=omywall
Terminal=false
Categories=Utility;Appearance;
X-GNOME-Autostart-enabled=true
"#;
            fs::write(target, desktop_content)?;
        } else if target.exists() {
            let _ = fs::remove_file(target);
        }
        Ok(())
    }

    pub fn add_web_bookmark(&mut self, title: String, url: String, category: String) {
        let bookmark = WebBookmark {
            title,
            url,
            category,
            is_demo: false,
        };
        if !self.saved_web_wallpapers.contains(&bookmark) {
            self.saved_web_wallpapers.push(bookmark);
            let _ = self.save();
        }
    }

    pub fn remove_web_bookmark(&mut self, url: &str) {
        self.saved_web_wallpapers.retain(|b| b.url != url);
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
        let raw_bg = if !self.hyprlock.background_path.trim().is_empty() {
            self.hyprlock.background_path.clone()
        } else if let Some(wall) = active_wallpaper {
            wall.to_string()
        } else {
            "screenshot".to_string()
        };

        let bg_path = if raw_bg != "screenshot" {
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

    if let Some(home) = dirs::home_dir() {
        let candidates = [
            home.join(".local").join("share").join("omywall").join("assets").join(stripped),
            home.join(".local").join("share").join("omywall").join(trimmed),
            home.join(".config").join("omywall").join(trimmed),
            PathBuf::from("/usr/share/omywall").join("assets").join(stripped),
            PathBuf::from("/usr/share/omywall").join(trimmed),
            PathBuf::from("/usr/local/share/omywall").join("assets").join(stripped),
            PathBuf::from("/usr/local/share/omywall").join(trimmed),
            std::env::current_dir().unwrap_or_default().join(trimmed),
        ];

        for c in &candidates {
            if c.exists() {
                return std::fs::canonicalize(c)
                    .unwrap_or_else(|_| c.clone())
                    .to_string_lossy()
                    .to_string();
            }
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
