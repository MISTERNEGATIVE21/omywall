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

        log_info(&format!("WebEngine: Applying native Rust GTK Layer Shell + WebKit2 WebGL wallpaper -> {}", target_url));

        let exe = match std::env::current_exe() {
            Ok(p) => p,
            Err(e) => {
                log_info(&format!("WebEngine: Cannot resolve own binary ({}), launching Chromium fallback...", e));
                return self.spawn_chromium_fallback(&target_url);
            }
        };

        let mut web_cmd = Command::new(exe);
        web_cmd.arg("web-layer").arg(&target_url);
        web_cmd.env("GDK_BACKEND", "wayland");
        web_cmd.env("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        web_cmd.env("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS", "1");
        web_cmd.env("LIBGL_ALWAYS_SOFTWARE", "0");

        let is_nvidia = crate::config::detect_system_gpus().iter().any(|g| g.vendor == "NVIDIA");
        if is_nvidia {
            web_cmd.env("__NV_PRIME_RENDER_OFFLOAD", "1");
            web_cmd.env("__GLX_VENDOR_LIBRARY_NAME", "nvidia");
            web_cmd.env("__VK_LAYER_NV_optimus", "NVIDIA_only");
            web_cmd.env("CUDA_VISIBLE_DEVICES", "0");
            web_cmd.env("DRI_PRIME", "1");
            web_cmd.env("GBM_BACKEND", "nvidia-drm");
            web_cmd.env("EGL_PLATFORM", "wayland");
        }

        match web_cmd.spawn() {
            Ok(child) => {
                std::thread::sleep(std::time::Duration::from_millis(600));
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
                "--ozone-platform=wayland".to_string(),
                "--enable-features=UseOzonePlatform".to_string(),
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
                    "--ozone-platform=wayland".to_string(),
                    "--enable-features=UseOzonePlatform".to_string(),
                    "--autoplay-policy=no-user-gesture-required".to_string(),
                    "--allow-file-access-from-files".to_string(),
                    "--kiosk".to_string(),
                ]).spawn()
            })
            .or_else(|_| {
                Command::new("electron").args([
                    "--title=omywall-web-wallpaper",
                    "--enable-features=UseOzonePlatform",
                    "--ozone-platform=wayland",
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
            std::thread::spawn(move || {
                let _ = child.kill();
                let _ = child.wait();
            });
        }
        let mut url_guard = self.current_url.lock().unwrap();
        *url_guard = None;
    }


    #[allow(dead_code)]
    pub fn current_url(&self) -> Option<String> {
        self.current_url.lock().unwrap().clone()
    }
}
