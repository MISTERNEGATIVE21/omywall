use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

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
    pub volume: i64,
    pub mute: bool,
    pub loop_file: String,
    pub window_id: u64,
    pub screen_id: i64,
    pub slideshow_interval: u64,
    pub slideshow_shuffle: bool,
    pub default_wallpaper: Option<PathBuf>,
    pub workspace_wallpapers: HashMap<String, String>,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default)]
    pub enable_widgets: bool,
    #[serde(default)]
    pub widget_url: Option<String>,
    #[serde(default)]
    pub monitor_wallpapers: HashMap<String, String>,
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default = "default_web_bookmarks")]
    pub saved_web_wallpapers: Vec<WebBookmark>,
    #[serde(default)]
    pub autostart: bool,
    #[serde(default)]
    pub hyprlock: HyprlockConfig,
}

fn default_mode() -> String {
    "workspace".to_string()
}

fn default_opacity() -> f32 {
    1.0
}

fn default_web_bookmarks() -> Vec<WebBookmark> {
    vec![
        WebBookmark {
            title: "Matrix Digital Rain".to_string(),
            url: "assets/web_wallpapers/matrix_rain.html".to_string(),
            category: "Canvas Animation".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "Cyber Neon Clock".to_string(),
            url: "assets/web_wallpapers/cyber_clock.html".to_string(),
            category: "Digital Clock".to_string(),
            is_demo: true,
        },
        WebBookmark {
            title: "Clock Zone Live".to_string(),
            url: "https://clock.zone".to_string(),
            category: "Live Web Stream".to_string(),
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

        let mut workspace_wallpapers = HashMap::new();
        for i in 1..=10 {
            workspace_wallpapers.insert(i.to_string(), String::new());
        }

        Self {
            wallpaper_dir: default_wall_dir,
            socket_path,
            hwdec: "auto".to_string(),
            volume: 0,
            mute: true,
            loop_file: "inf".to_string(),
            window_id: 0,
            screen_id: 0,
            slideshow_interval: 300,
            slideshow_shuffle: false,
            default_wallpaper: None,
            workspace_wallpapers,
            opacity: 1.0,
            enable_widgets: false,
            widget_url: None,
            monitor_wallpapers: HashMap::new(),
            mode: "workspace".to_string(),
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
GenericName=Live Video, Stream & Workspace Wallpaper Engine
Comment=Ultra-Lightweight Hardware-Accelerated Video, Stream & Workspace Wallpaper Engine
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

    pub fn get_workspace_wallpaper(&self, ws: &str) -> Option<&String> {
        let clean_id = ws
            .trim_start_matches("workspace_")
            .trim_start_matches("Workspace ")
            .trim();
        let candidates = [
            ws.to_string(),
            clean_id.to_string(),
            format!("Workspace {}", clean_id),
            format!("workspace {}", clean_id),
        ];

        for key in &candidates {
            if let Some(path) = self.workspace_wallpapers.get(key) {
                if !path.trim().is_empty() {
                    return Some(path);
                }
            }
        }
        None
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
        let bg_path = if !self.hyprlock.background_path.trim().is_empty() {
            self.hyprlock.background_path.clone()
        } else if let Some(wall) = active_wallpaper {
            wall.to_string()
        } else {
            "screenshot".to_string()
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
