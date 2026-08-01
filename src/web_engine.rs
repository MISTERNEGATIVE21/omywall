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
        } else {
            let resolved = crate::config::resolve_asset_path(trimmed);
            if Path::new(&resolved).exists() {
                format!("file://{}", resolved)
            } else if trimmed.contains('.') && !trimmed.contains(' ') {
                format!("https://{}", trimmed)
            } else {
                format!("file://{}", resolved)
            }
        };

        log_info(&format!("WebEngine: Applying background wlr-layer-shell web wallpaper -> {}", target_url));

        // Primary native Wayland wlr-layer-shell Web Engine (GTK Layer Shell + WebKit2)
        let py_runner_path = PathBuf::from("/tmp/omywall_web_layer.py");
        let py_runner_code = r#"import sys
import os
import gi

try:
    gi.require_version('Gtk', '3.0')
    gi.require_version('GtkLayerShell', '0.1')
    try:
        gi.require_version('WebKit2', '4.1')
    except:
        gi.require_version('WebKit2', '4.0')
    from gi.repository import Gtk, GtkLayerShell, WebKit2
except Exception as e:
    sys.stderr.write(f"GtkLayerShell/WebKit2 import error: {e}\n")
    sys.exit(1)

target_url = sys.argv[1]

window = Gtk.Window()
GtkLayerShell.init_for_window(window)
GtkLayerShell.set_layer(window, GtkLayerShell.Layer.BACKGROUND)
GtkLayerShell.set_anchor(window, GtkLayerShell.Edge.TOP, True)
GtkLayerShell.set_anchor(window, GtkLayerShell.Edge.BOTTOM, True)
GtkLayerShell.set_anchor(window, GtkLayerShell.Edge.LEFT, True)
GtkLayerShell.set_anchor(window, GtkLayerShell.Edge.RIGHT, True)
GtkLayerShell.set_exclusive_zone(window, -1)

webview = WebKit2.WebView()
settings = webview.get_settings()
settings.set_enable_developer_extras(False)
settings.set_enable_webgl(True)
settings.set_enable_media_stream(True)
settings.set_enable_mediasource(True)
settings.set_enable_html5_local_storage(True)
settings.set_media_playback_requires_user_gesture(False)
settings.set_allow_file_access_from_file_urls(True)

webview.load_uri(target_url)
window.add(webview)
window.show_all()
Gtk.main()
"#;
        let _ = std::fs::write(&py_runner_path, py_runner_code);

        // Try spawning python3 GtkLayerShell web wallpaper runner first
        let mut py_cmd = Command::new("python3");
        py_cmd.args([py_runner_path.to_string_lossy().as_ref(), &target_url]);
        py_cmd.env("WEBKIT_FORCE_COMPOSITING_MODE", "1");
        py_cmd.env("LIBGL_ALWAYS_SOFTWARE", "0");

        if let Ok(child) = py_cmd.spawn()
        {
            // Wait briefly to confirm python script didn't exit with error
            std::thread::sleep(std::time::Duration::from_millis(150));
            let mut test_child = child;
            match test_child.try_wait() {
                Ok(Some(status)) => {
                    log_error(&format!("WebEngine: GTK Layer Shell runner exited with status {:?}. Falling back to Electron...", status));
                }
                Ok(None) => {
                    log_info("WebEngine: Successfully launched native wlr-layer-shell WebKit background wallpaper surface");
                    let mut guard = self.web_child.lock().unwrap();
                    *guard = Some(test_child);
                    let mut url_guard = self.current_url.lock().unwrap();
                    *url_guard = Some(target_url);
                    return Ok(());
                }
                Err(_) => {}
            }
        }

        // Secondary fallback: Electron runner with Wayland Ozone flags
        let electron_runner_path = PathBuf::from("/tmp/omywall_web_app.js");
        let electron_runner_code = r#"
const { app, BrowserWindow } = require('electron');
app.commandLine.appendSwitch('ozone-platform-hint', 'auto');
app.commandLine.appendSwitch('enable-features', 'UseOzonePlatform,WaylandWindowDecorations');
app.commandLine.appendSwitch('ozone-platform', 'wayland');
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
        let _ = std::fs::write(&electron_runner_path, electron_runner_code);

        let browser_bin = crate::engine::find_web_browser_binary();
        if let Some(bin) = browser_bin {
            let bin_name = bin.file_name().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
            log_info(&format!("WebEngine: Spawning fallback Electron binary '{}' for URL {}", bin.display(), target_url));

            let child = if bin_name.contains("electron") {
                Command::new(&bin)
                    .args([
                        electron_runner_path.to_string_lossy().as_ref(),
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

        Err("WebEngine Exception: Neither GtkLayerShell nor suitable Electron binary is available".into())
    }

    pub fn stop(&self) {
        let mut guard = self.web_child.lock().unwrap();
        if let Some(mut child) = guard.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = Command::new("pkill").args(["-9", "-f", "omywall_web_layer.py"]).status();
        let _ = Command::new("pkill").args(["-9", "-f", "omywall-web-wallpaper"]).status();
        let mut url_guard = self.current_url.lock().unwrap();
        *url_guard = None;
    }

    #[allow(dead_code)]
    pub fn current_url(&self) -> Option<String> {
        self.current_url.lock().unwrap().clone()
    }
}
