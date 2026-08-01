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
}
