use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use crate::logger::log_info;

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

        log_info(&format!("WebEngine: Applying native GTK Layer Shell + WebKit2 WebGL wallpaper -> {}", target_url));

        // Primary native Wayland wlr-layer-shell Web Engine (GTK Layer Shell + WebKit2)
        let py_runner_path = PathBuf::from("/tmp/omywall_web_layer.py");
        let py_runner_code = r#"import sys
import os
import gi

try:
    gi.require_version('Gtk', '3.0')
    gi.require_version('Gdk', '3.0')
    gi.require_version('GtkLayerShell', '0.1')
    try:
        gi.require_version('WebKit2', '4.1')
    except:
        gi.require_version('WebKit2', '4.0')
    from gi.repository import Gtk, Gdk, GtkLayerShell, WebKit2
except Exception as e:
    sys.stderr.write(f"GtkLayerShell/WebKit2 import error: {e}\n")
    sys.exit(1)

target_url = sys.argv[1]
if not (target_url.startswith('http://') or target_url.startswith('https://') or target_url.startswith('file://') or target_url.startswith('data:')):
    target_url = 'file://' + os.path.abspath(target_url)

window = Gtk.Window()
GtkLayerShell.init_for_window(window)
GtkLayerShell.set_layer(window, GtkLayerShell.Layer.BACKGROUND)
GtkLayerShell.set_anchor(window, GtkLayerShell.Edge.TOP, True)
GtkLayerShell.set_anchor(window, GtkLayerShell.Edge.BOTTOM, True)
GtkLayerShell.set_anchor(window, GtkLayerShell.Edge.LEFT, True)
GtkLayerShell.set_anchor(window, GtkLayerShell.Edge.RIGHT, True)
GtkLayerShell.set_exclusive_zone(window, -1)
GtkLayerShell.set_keyboard_interactivity(window, False)

window.set_support_multidevice(True)
window.add_events(Gdk.EventMask.POINTER_MOTION_MASK | Gdk.EventMask.POINTER_MOTION_HINT_MASK)

webview = WebKit2.WebView()
webview.connect('load-failed', lambda *args: True)
settings = webview.get_settings()
settings.set_enable_developer_extras(False)
settings.set_enable_webgl(True)
settings.set_enable_media_stream(True)
settings.set_enable_mediasource(True)
settings.set_enable_html5_local_storage(True)
settings.set_media_playback_requires_user_gesture(False)
settings.set_allow_file_access_from_file_urls(True)
try:
    settings.set_hardware_acceleration_policy(WebKit2.HardwareAccelerationPolicy.ALWAYS)
except Exception:
    pass
try:
    settings.set_enable_accelerated_2d_canvas(True)
except Exception:
    pass

webview.load_uri(target_url)
window.add(webview)
window.show_all()
Gtk.main()
"#;
        let _ = std::fs::write(&py_runner_path, py_runner_code);

        let mut py_cmd = Command::new("python3");
        py_cmd.args([py_runner_path.to_string_lossy().as_ref(), &target_url]);
        py_cmd.env("WEBKIT_FORCE_COMPOSITING_MODE", "1");
        py_cmd.env("WEBKIT_DISABLE_COMPOSITING_MODE", "0");
        py_cmd.env("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS", "1");
        py_cmd.env("LIBGL_ALWAYS_SOFTWARE", "0");

        let is_nvidia = crate::config::detect_system_gpus().iter().any(|g| g.vendor == "NVIDIA");
        if is_nvidia {
            py_cmd.env("__NV_PRIME_RENDER_OFFLOAD", "1");
            py_cmd.env("__GLX_VENDOR_LIBRARY_NAME", "nvidia");
            py_cmd.env("__VK_LAYER_NV_optimus", "NVIDIA_only");
            py_cmd.env("CUDA_VISIBLE_DEVICES", "0");
            py_cmd.env("DRI_PRIME", "1");
            py_cmd.env("GBM_BACKEND", "nvidia-drm");
            py_cmd.env("EGL_PLATFORM", "wayland");
        }

        match py_cmd.spawn() {
            Ok(child) => {
                std::thread::sleep(std::time::Duration::from_millis(150));
                let mut test_child = child;
                match test_child.try_wait() {
                    Ok(Some(status)) => {
                        log_info(&format!("WebEngine: GTK Layer Shell runner exited with status {:?}, trying Chromium wallpaper fallback...", status));
                        self.spawn_chromium_fallback(&target_url)
                    }
                    Ok(None) => {
                        log_info("WebEngine: Successfully launched native GTK Layer Shell + WebKit2 background surface");
                        let mut guard = self.web_child.lock().unwrap();
                        *guard = Some(test_child);
                        let mut url_guard = self.current_url.lock().unwrap();
                        *url_guard = Some(target_url);
                        Ok(())
                    }
                    Err(e) => Err(format!("WebEngine Error: {}", e)),
                }
            }
            Err(e) => {
                log_info(&format!("WebEngine: Failed to spawn GTK Layer Shell process ({}), launching Chromium fallback...", e));
                self.spawn_chromium_fallback(&target_url)
            }
        }
    }

    fn spawn_chromium_fallback(&self, target_url: &str) -> Result<(), String> {
        let profile_dir = PathBuf::from("/tmp/omywall_browser_profile");
        let _ = std::fs::create_dir_all(&profile_dir);

        let child = Command::new("chromium")
            .args([
                format!("--app={}", target_url),
                format!("--user-data-dir={}", profile_dir.display()),
                "--autoplay-policy=no-user-gesture-required".to_string(),
                "--allow-file-access-from-files".to_string(),
                "--disable-session-crashed-bubble".to_string(),
                "--disable-infobars".to_string(),
                "--kiosk".to_string(),
            ])
            .spawn()
            .or_else(|_| {
                Command::new("google-chrome").args([
                    format!("--app={}", target_url),
                    format!("--user-data-dir={}", profile_dir.display()),
                    "--autoplay-policy=no-user-gesture-required".to_string(),
                    "--allow-file-access-from-files".to_string(),
                    "--kiosk".to_string(),
                ]).spawn()
            })
            .or_else(|_| {
                Command::new("electron").args([
                    "--title=omywall-web-wallpaper",
                    target_url,
                ]).spawn()
            })
            .map_err(|e| format!("Failed to launch fallback web wallpaper browser: {}", e))?;

        let mut guard = self.web_child.lock().unwrap();
        *guard = Some(child);
        let mut url_guard = self.current_url.lock().unwrap();
        *url_guard = Some(target_url.to_string());
        log_info(&format!("WebEngine: Spawned browser wallpaper fallback for '{}'", target_url));
        Ok(())
    }

    pub fn stop(&self) {
        let mut guard = self.web_child.lock().unwrap();
        if let Some(mut child) = guard.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = Command::new("pkill").args(["-9", "-f", "omywall_web_layer.py"]).status();
        let mut url_guard = self.current_url.lock().unwrap();
        *url_guard = None;
    }

    #[allow(dead_code)]
    pub fn current_url(&self) -> Option<String> {
        self.current_url.lock().unwrap().clone()
    }
}
