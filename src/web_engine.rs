use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use crate::logger::{log_error, log_info};

pub struct WebEngineManager {
    web_child: Arc<Mutex<Option<Child>>>,
    current_url: Arc<Mutex<Option<String>>>,
}

impl WebEngineManager {
    pub fn new() -> Self {
        Self {
            web_child: Arc::new(Mutex::new(None)),
            current_url: Arc::new(Mutex::new(None)),
        }
    }

    pub fn apply_web_wallpaper(&self, raw_url: &str) -> Result<(), String> {
        let trimmed = raw_url.trim();
        if trimmed.is_empty() {
            return Err("WebEngine Exception: URL target is empty".into());
        }

        self.stop();

        let target_url = if trimmed.starts_with("http://") || trimmed.starts_with("https://") || trimmed.starts_with("file://") {
            trimmed.to_string()
        } else if Path::new(trimmed).exists() {
            let canon = std::fs::canonicalize(trimmed).unwrap_or_else(|_| PathBuf::from(trimmed));
            format!("file://{}", canon.to_string_lossy())
        } else {
            let resolved = crate::config::resolve_asset_path(trimmed);
            if Path::new(&resolved).exists() {
                format!("file://{}", resolved)
            } else {
                format!("https://{}", trimmed)
            }
        };

        log_info(&format!("WebEngine: Applying background web wallpaper -> {}", target_url));

        // Inject Wayland / Hyprland window rules for background layer placement
        let _ = Command::new("hyprctl")
            .args(["keyword", "windowrulev2", "background, class:^(omywall-web-wallpaper)$"])
            .output();
        let _ = Command::new("hyprctl")
            .args(["keyword", "windowrulev2", "pin, class:^(omywall-web-wallpaper)$"])
            .output();
        let _ = Command::new("hyprctl")
            .args(["keyword", "windowrulev2", "fullscreen, class:^(omywall-web-wallpaper)$"])
            .output();
        let _ = Command::new("hyprctl")
            .args(["keyword", "windowrulev2", "nofocus, class:^(omywall-web-wallpaper)$"])
            .output();
        let _ = Command::new("hyprctl")
            .args(["keyword", "windowrulev2", "noblur, class:^(omywall-web-wallpaper)$"])
            .output();
        let _ = Command::new("hyprctl")
            .args(["keyword", "windowrulev2", "size 100% 100%, class:^(omywall-web-wallpaper)$"])
            .output();
        let _ = Command::new("hyprctl")
            .args(["keyword", "windowrulev2", "move 0 0, class:^(omywall-web-wallpaper)$"])
            .output();

        let runner_path = PathBuf::from("/tmp/omywall_web_app.js");
        let runner_code = r#"
const { app, BrowserWindow } = require('electron');
app.commandLine.appendSwitch('ozone-platform-hint', 'auto');
app.commandLine.appendSwitch('enable-features', 'UseOzonePlatform');
app.commandLine.appendSwitch('autoplay-policy', 'no-user-gesture-required');
app.commandLine.appendSwitch('disable-gpu-vsync');

app.whenReady().then(() => {
    const win = new BrowserWindow({
        width: 1920,
        height: 1080,
        type: 'desktop',
        frame: false,
        transparent: true,
        fullscreen: true,
        skipTaskbar: true,
        focusable: false,
        webPreferences: {
            nodeIntegration: false,
            contextIsolation: true,
            backgroundThrottling: false
        }
    });
    win.setMenu(null);
    win.setIgnoreMouseEvents(true, { forward: true });
    win.loadURL(process.argv[2]);
});
"#;
        let _ = std::fs::write(&runner_path, runner_code);

        let browser_bin = crate::engine::find_web_browser_binary();
        if let Some(bin) = browser_bin {
            let bin_name = bin.file_name().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
            log_info(&format!("WebEngine: Spawning binary '{}' for URL {}", bin.display(), target_url));

            let child = if bin_name.contains("electron") {
                Command::new(&bin)
                    .args([
                        runner_path.to_string_lossy().as_ref(),
                        &target_url,
                        "--class=omywall-web-wallpaper",
                    ])
                    .spawn()
            } else {
                let app_arg = format!("--app={}", target_url);
                Command::new(&bin)
                    .args([
                        &app_arg,
                        "--class=omywall-web-wallpaper",
                        "--no-first-run",
                        "--disable-infobars",
                        "--user-data-dir=/tmp/omywall-chrome-profile",
                        "--autoplay-policy=no-user-gesture-required",
                    ])
                    .spawn()
            };

            match child {
                Ok(c) => {
                    let mut guard = self.web_child.lock().unwrap();
                    *guard = Some(c);
                    let mut url_guard = self.current_url.lock().unwrap();
                    *url_guard = Some(target_url);
                    return Ok(());
                }
                Err(e) => {
                    log_error(&format!("WebEngine Error: Failed to spawn web process: {}", e));
                    return Err(format!("Failed to spawn web process: {}", e));
                }
            }
        }

        Err("WebEngine Exception: No suitable web browser or Electron binary found".into())
    }

    pub fn stop(&self) {
        let mut guard = self.web_child.lock().unwrap();
        if let Some(mut child) = guard.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = Command::new("pkill").args(["-9", "-f", "omywall-web-wallpaper"]).status();
        let mut url_guard = self.current_url.lock().unwrap();
        *url_guard = None;
    }

    #[allow(dead_code)]
    pub fn current_url(&self) -> Option<String> {
        self.current_url.lock().unwrap().clone()
    }
}
