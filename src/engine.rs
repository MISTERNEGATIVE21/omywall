use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::logger::{log_info, log_memory_diagnostic};

pub struct WallpaperEngine {
    mpv_process: Arc<Mutex<Option<Child>>>,
    lwe_process: Arc<Mutex<Option<Child>>>,
    web_engine: Arc<crate::web_engine::WebEngineManager>,
    widget_process: Arc<Mutex<Option<Child>>>,
    current_wallpaper: Arc<Mutex<Option<String>>>,
    is_paused: Arc<Mutex<bool>>,
    is_hidden: Arc<Mutex<bool>>,
    user_stopped: Arc<Mutex<bool>>,
    hwdec: Arc<Mutex<String>>,
    gpu_device: Arc<Mutex<Option<String>>>,
    target_fps: Arc<Mutex<u32>>,
    volume: Arc<Mutex<i64>>,
    mute: Arc<Mutex<bool>>,
    screen_id: Arc<Mutex<i64>>,
    opacity: Arc<Mutex<f32>>,
    widget_enabled: Arc<Mutex<bool>>,
    widget_url: Arc<Mutex<Option<String>>>,
    widget_position: Arc<Mutex<String>>,
    _window_id: u64,
    socket_path: PathBuf,
}

pub fn find_lwe_binary() -> Option<PathBuf> {
    crate::lwe::find_binary()
}

pub fn find_mpvpaper_binary() -> Option<PathBuf> {
    if let Ok(out) = Command::new("which").arg("mpvpaper").output() {
        if out.status.success() {
            let p_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !p_str.is_empty() {
                return Some(PathBuf::from(p_str));
            }
        }
    }
    if let Ok(out) = Command::new("sh").args(["-c", "command -v mpvpaper"]).output() {
        if out.status.success() {
            let p_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !p_str.is_empty() {
                return Some(PathBuf::from(p_str));
            }
        }
    }
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/home/user"));
    let candidates = [
        home.join(".local").join("bin").join("mpvpaper"),
        PathBuf::from("/usr/bin/mpvpaper"),
        PathBuf::from("/usr/local/bin/mpvpaper"),
        PathBuf::from("/bin/mpvpaper"),
    ];
    for c in candidates {
        if c.exists() {
            return Some(c.clone());
        }
    }
    None
}



fn kill_child_async(mut child: std::process::Child) {
    std::thread::spawn(move || {
        let _ = child.kill();
        let _ = child.wait();
    });
}

impl WallpaperEngine {

    pub fn new(hwdec: &str, gpu_device: Option<String>, target_fps: u32, volume: i64, mute: bool, window_id: u64, screen_id: i64) -> Result<Self, String> {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
        let socket_path = PathBuf::from(runtime_dir).join(format!("omywall-mpv-{}.sock", std::process::id()));

        let engine = Self {
            mpv_process: Arc::new(Mutex::new(None)),
            lwe_process: Arc::new(Mutex::new(None)),
            web_engine: Arc::new(crate::web_engine::WebEngineManager::new()),
            widget_process: Arc::new(Mutex::new(None)),
            current_wallpaper: Arc::new(Mutex::new(None)),
            is_paused: Arc::new(Mutex::new(false)),
            is_hidden: Arc::new(Mutex::new(false)),
            user_stopped: Arc::new(Mutex::new(false)),
            hwdec: Arc::new(Mutex::new(hwdec.to_string())),
            gpu_device: Arc::new(Mutex::new(gpu_device)),
            target_fps: Arc::new(Mutex::new(target_fps)),
            volume: Arc::new(Mutex::new(volume)),
            mute: Arc::new(Mutex::new(mute)),
            screen_id: Arc::new(Mutex::new(screen_id)),
            opacity: Arc::new(Mutex::new(1.0)),
            widget_enabled: Arc::new(Mutex::new(false)),
            widget_url: Arc::new(Mutex::new(None)),
            widget_position: Arc::new(Mutex::new("top_right".to_string())),
            _window_id: window_id,
            socket_path,
        };

        crate::widgets_bridge::start_telemetry_loop();

        Ok(engine)
    }


    fn send_mpv_command(&self, args: serde_json::Value) -> Result<(), String> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .map_err(|e| format!("Failed to connect to MPV IPC socket at {}: {}", self.socket_path.display(), e))?;

        let cmd = serde_json::json!({
            "command": args
        });

        let mut data = serde_json::to_vec(&cmd).map_err(|e| e.to_string())?;
        data.push(b'\n');

        stream
            .write_all(&data)
            .map_err(|e| format!("Failed to send command to MPV IPC socket: {}", e))?;

        Ok(())
    }

    fn ensure_mpv_running(&self, initial_file: Option<&str>) -> Result<(), String> {
        let mut mpv_guard = self.mpv_process.lock().unwrap();
        let is_alive = mpv_guard.as_mut().is_some_and(|child| {
            child.try_wait().ok().flatten().is_none()
        });

        if !is_alive {
            self.web_engine.stop();
            if self.socket_path.exists() {
                let _ = std::fs::remove_file(&self.socket_path);
            }

            let hwdec = self.hwdec.lock().unwrap().clone();
            let gpu_device = self.gpu_device.lock().unwrap().clone();
            let fps = *self.target_fps.lock().unwrap();
            let volume = *self.volume.lock().unwrap();
            let mute = *self.mute.lock().unwrap();
            let socket_str = self.socket_path.to_string_lossy().to_string();

            let is_nvidia = gpu_device.as_ref().map_or_else(
                || crate::config::detect_system_gpus().iter().any(|g| g.vendor == "NVIDIA"),
                |dev| dev.contains("129") || dev.to_lowercase().contains("nvidia") || dev.contains("card2")
            ) || matches!(hwdec.as_str(), "nvdec" | "cuda");

            let effective_hwdec = if hwdec == "auto" {
                if is_nvidia {
                    "nvdec".to_string()
                } else {
                    "auto".to_string()
                }
            } else {
                hwdec.clone()
            };

            let video_target = initial_file.unwrap_or("");
            let mpvpaper_path = find_mpvpaper_binary();
            let mut spawned_child = None;

            if let Some(mpvpaper_bin) = mpvpaper_path {
                let mut mpv_opts = format!(
                    "--config=no --input-ipc-server={} --loop-file=inf --image-display-duration=inf --no-osc --no-osd-bar --hwdec={} --volume={} --mute={} --panscan=1.0",
                    socket_str,
                    effective_hwdec,
                    volume,
                    if mute { "yes" } else { "no" }
                );

                if fps > 0 {
                    mpv_opts.push_str(&format!(" --override-display-fps={}", fps));
                }

                if let Some(ref dev) = gpu_device {
                    if !dev.trim().is_empty() {
                        mpv_opts.push_str(&format!(" --gpu-device={} --vo=gpu", dev));
                    }
                }

                let mut mpvpaper_args = vec![
                    "-o".to_string(),
                    mpv_opts,
                    "*".to_string(),
                ];
                if !video_target.is_empty() {
                    mpvpaper_args.push(video_target.to_string());
                } else {
                    mpvpaper_args.push("/dev/null".to_string());
                }

                log_info(&format!("Spawning mpvpaper ({}) wlr-layer-shell background process with args: {:?}", mpvpaper_bin.display(), mpvpaper_args));

                let mut cmd = Command::new(&mpvpaper_bin);
                cmd.args(&mpvpaper_args);
                cmd.env("MPV_HOME", "/tmp/omywall_mpv_isolated");
                cmd.env("XDG_CONFIG_HOME", "/tmp/omywall_mpv_isolated");

                if is_nvidia {
                    cmd.env("__NV_PRIME_RENDER_OFFLOAD", "1");
                    cmd.env("__GLX_VENDOR_LIBRARY_NAME", "nvidia");
                    cmd.env("__VK_LAYER_NV_optimus", "NVIDIA_only");
                    cmd.env("CUDA_VISIBLE_DEVICES", "0");
                    cmd.env("VK_DRIVER_FILES", "/usr/share/vulkan/icd.d/nvidia_icd.json");
                    log_info("Engine: Enabled NVIDIA PRIME Render Offload (__NV_PRIME_RENDER_OFFLOAD=1)");
                }

                if let Ok(child) = cmd.spawn() {
                    spawned_child = Some(child);
                }
            }

            if spawned_child.is_none() {
                log_info("Engine: Falling back to direct mpv background renderer...");
                let mpv_bin = PathBuf::from("/usr/bin/mpv");
                let mut mpv_args = vec![
                    "--config=no".to_string(),
                    format!("--input-ipc-server={}", socket_str),
                    "--loop-file=inf".to_string(),
                    format!("--hwdec={}", effective_hwdec),
                    format!("--volume={}", volume),
                    format!("--mute={}", if mute { "yes" } else { "no" }),
                    "--no-osc".to_string(),
                    "--no-osd-bar".to_string(),
                    "--no-border".to_string(),
                    "--fullscreen".to_string(),
                    "--ontop=no".to_string(),
                    "--panscan=1.0".to_string(),
                ];
                if !video_target.is_empty() {
                    mpv_args.push(video_target.to_string());
                } else {
                    mpv_args.push("/dev/null".to_string());
                }

                let mut cmd = Command::new(&mpv_bin);
                cmd.args(&mpv_args);

                if is_nvidia {
                    cmd.env("__NV_PRIME_RENDER_OFFLOAD", "1");
                    cmd.env("__GLX_VENDOR_LIBRARY_NAME", "nvidia");
                    cmd.env("__VK_LAYER_NV_optimus", "NVIDIA_only");
                    cmd.env("CUDA_VISIBLE_DEVICES", "0");
                }

                let child = cmd.spawn().map_err(|e| format!("Failed to spawn mpv fallback renderer: {}", e))?;
                spawned_child = Some(child);
            }

            *mpv_guard = spawned_child;

            // Wait for MPV IPC socket to bind, with early termination check if process dies
            for _ in 0..30 {
                if self.socket_path.exists() {
                    break;
                }
                if let Some(ref mut c) = *mpv_guard {
                    if let Ok(Some(status)) = c.try_wait() {
                        return Err(format!("mpvpaper process exited unexpectedly with status: {:?}", status));
                    }
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }

        Ok(())
    }

    pub fn set_wallpaper(&self, path: &Path) -> Result<(), String> {
        let raw_str = path.to_string_lossy().to_string();

        if raw_str.starts_with("http://") || raw_str.starts_with("https://") {
            return self.set_url(&raw_str);
        }

        let resolved_str = crate::config::resolve_asset_path(&raw_str);
        let resolved_path = Path::new(&resolved_str);

        let ext = resolved_path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        if ext == "html" || ext == "htm" || ext == "js" {
            return self.set_url(&resolved_str);
        }

        if ext == "pkg" || crate::steam_scanner::is_pkg_file(resolved_path) {
            let target_path = if resolved_path.is_file() {
                if let Some(parent) = resolved_path.parent() {
                    if parent.join("project.json").exists() {
                        parent
                    } else {
                        resolved_path
                    }
                } else {
                    resolved_path
                }
            } else {
                resolved_path
            };
            return self.set_steam_wallpaper(target_path, None, None);
        }

        if resolved_path.is_dir() {
            let has_project = resolved_path.join("project.json").exists();
            let has_pkg = std::fs::read_dir(resolved_path).ok().is_some_and(|entries| {
                entries.flatten().any(|e| crate::steam_scanner::is_pkg_file(&e.path()))
            });
            if has_project || has_pkg {
                return self.set_steam_wallpaper(resolved_path, None, None);
            }
        }

        if !resolved_path.exists() {
            log_memory_diagnostic();
            return Err(format!("Wallpaper Exception: File does not exist at '{}'", resolved_path.display()));
        }

        let path_str = resolved_path.to_string_lossy().to_string();

        let is_user_stopped = *self.user_stopped.lock().unwrap();
        let is_paused = *self.is_paused.lock().unwrap();
        let current_wall = self.current_wallpaper.lock().unwrap().clone();

        let is_mpv_alive = {
            let mut mpv_guard = self.mpv_process.lock().unwrap();
            mpv_guard.as_mut().is_some_and(|c| c.try_wait().ok().flatten().is_none())
        };

        if let Some(ref curr) = current_wall {
            if curr == &path_str && !is_user_stopped && is_mpv_alive {
                if is_paused {
                    let _ = self.resume();
                }
                return Ok(());
            }
        }

        if is_mpv_alive && self.socket_path.exists() {
            let res = self.send_mpv_command(serde_json::json!(["loadfile", path_str, "replace"]));
            if res.is_ok() {
                let _ = self.send_mpv_command(serde_json::json!(["set_property", "pause", false]));
            } else {
                self.ensure_mpv_running(Some(&path_str))?;
            }
        } else {
            self.ensure_mpv_running(Some(&path_str))?;
        }

        let mut curr = self.current_wallpaper.lock().unwrap();
        *curr = Some(path_str);
        let mut stopped_guard = self.user_stopped.lock().unwrap();
        *stopped_guard = false;
        let mut pause_guard = self.is_paused.lock().unwrap();
        *pause_guard = false;

        Ok(())
    }

    pub fn set_url(&self, url: &str) -> Result<(), String> {
        let trimmed = url.trim();
        if trimmed.is_empty() {
            return Err("Wallpaper Exception: Provided URL is empty".into());
        }

        self.stop_mpv_internal();
        self.stop_lwe_internal();
        self.stop_widget_internal();
        self.web_engine.apply_web_wallpaper(trimmed)?;


        let mut curr = self.current_wallpaper.lock().unwrap();
        *curr = Some(trimmed.to_string());
        let mut p = self.is_paused.lock().unwrap();
        *p = false;
        let mut st = self.user_stopped.lock().unwrap();
        *st = false;

        Ok(())
    }

    #[allow(dead_code)]
    fn set_wallpaper_url_mpv(&self, url: &str) -> Result<(), String> {
        let is_mpv_alive = {
            let mut mpv_guard = self.mpv_process.lock().unwrap();
            mpv_guard.as_mut().is_some_and(|c| c.try_wait().ok().flatten().is_none())
        };

        if is_mpv_alive && self.socket_path.exists() {
            let _ = self.send_mpv_command(serde_json::json!(["loadfile", url, "replace"]));
            let _ = self.send_mpv_command(serde_json::json!(["set_property", "pause", false]));
        } else {
            self.ensure_mpv_running(Some(url))?;
        }

        let mut curr = self.current_wallpaper.lock().unwrap();
        *curr = Some(url.to_string());
        let mut st = self.user_stopped.lock().unwrap();
        *st = false;
        let mut p = self.is_paused.lock().unwrap();
        *p = false;

        Ok(())
    }





    fn stop_mpv_internal(&self) {
        if self.socket_path.exists() {
            let _ = self.send_mpv_command(serde_json::json!(["quit"]));
        }
        let mut proc_guard = self.mpv_process.lock().unwrap();
        if let Some(child) = proc_guard.take() {
            kill_child_async(child);
        }
        if self.socket_path.exists() {
            let _ = std::fs::remove_file(&self.socket_path);
        }
    }

    pub fn set_opacity(&self, opacity: f32) -> Result<(), String> {
        let clamped = opacity.clamp(0.0, 1.0);
        let mut op_guard = self.opacity.lock().unwrap();
        *op_guard = clamped;
        log_info(&format!("Engine: Opacity set to {:.2}", clamped));
        Ok(())
    }

    pub fn get_opacity(&self) -> f32 {
        *self.opacity.lock().unwrap()
    }

    #[allow(dead_code)]
    pub fn set_widget(&self, url: &str, enabled: bool) -> Result<(), String> {
        let pos = self.widget_position.lock().unwrap().clone();
        self.set_widget_with_position(url, enabled, &pos)
    }

    pub fn set_widget_with_position(&self, url: &str, enabled: bool, position: &str) -> Result<(), String> {
        let mut en_guard = self.widget_enabled.lock().unwrap();
        *en_guard = enabled;

        let mut url_guard = self.widget_url.lock().unwrap();
        *url_guard = if url.trim().is_empty() { None } else { Some(url.to_string()) };

        let mut pos_guard = self.widget_position.lock().unwrap();
        *pos_guard = position.to_string();

        self.stop_widget_internal();

        if enabled {
            if let Some(ref target_url) = *url_guard {
                log_info(&format!("Engine: Launching desktop widget at '{}' (position: {})", target_url, position));

                let child = if let Ok(exe) = std::env::current_exe() {
                    Command::new(exe)
                        .args(["web-layer", target_url, "--widget", "--position", position])
                        .spawn()
                        .ok()
                } else {
                    None
                };

                let child = match child {
                    Some(c) => Some(c),
                    None => {
                        Command::new("omywall")
                            .args(["web-layer", target_url, "--widget", "--position", position])
                            .spawn()
                            .or_else(|_| {
                                Command::new("electron")
                                    .args(["--title=omywall-widget", target_url])
                                    .spawn()
                            })
                            .or_else(|_| {
                                Command::new("chromium")
                                    .args([format!("--app={}", target_url), "--user-data-dir=/tmp/omywall-widget-profile".to_string()])
                                    .spawn()
                            })
                            .ok()
                    }
                };

                let mut proc_guard = self.widget_process.lock().unwrap();
                *proc_guard = child;
            }
        }

        Ok(())
    }

    pub fn get_widget_info(&self) -> (bool, Option<String>) {
        let en = *self.widget_enabled.lock().unwrap();
        let url = self.widget_url.lock().unwrap().clone();
        (en, url)
    }

    #[allow(dead_code)]
    pub fn get_widget_position(&self) -> String {
        self.widget_position.lock().unwrap().clone()
    }

    fn stop_widget_internal(&self) {
        let mut proc_guard = self.widget_process.lock().unwrap();
        if let Some(child) = proc_guard.take() {
            kill_child_async(child);
        }
    }

    fn stop_lwe_internal(&self) {
        let mut proc_guard = self.lwe_process.lock().unwrap();
        if let Some(child) = proc_guard.take() {
            kill_child_async(child);
        }
    }


    pub fn set_steam_wallpaper(
        &self,
        wallpaper_path: &Path,
        screen: Option<&str>,
        overrides: Option<&crate::config::WallpaperOverrides>,
    ) -> Result<(), String> {
        let lwe_bin = find_lwe_binary().ok_or_else(|| {
            "linux-wallpaperengine binary not found. Please install linux-wallpaperengine.".to_string()
        })?;

        self.stop_mpv_internal();
        self.web_engine.stop();
        self.stop_lwe_internal();

        let mut args = Vec::new();
        let wallpaper_str = wallpaper_path.to_string_lossy().to_string();
        if let Some(scr) = screen {
            if !scr.is_empty() {
                args.push("--screen-root".to_string());
                args.push(scr.to_string());
                args.push("--bg".to_string());
                args.push(wallpaper_str);
            } else {
                args.push(wallpaper_str);
            }
        } else {
            args.push(wallpaper_str);
        }

        let target_fps = overrides.and_then(|o| o.fps).unwrap_or(*self.target_fps.lock().unwrap());
        if target_fps > 0 {
            args.push("--fps".to_string());
            args.push(target_fps.to_string());
        }

        let is_silent = overrides.and_then(|o| o.silent).unwrap_or(*self.mute.lock().unwrap());
        if is_silent {
            args.push("--silent".to_string());
        } else {
            let vol = overrides.and_then(|o| o.volume).unwrap_or(*self.volume.lock().unwrap());
            args.push("--volume".to_string());
            args.push(vol.to_string());
        }

        if let Some(scaling) = overrides.and_then(|o| o.scaling.as_ref()) {
            if !scaling.is_empty() && scaling != "default" {
                args.push("--scaling".to_string());
                args.push(scaling.clone());
            }
        }

        if overrides.and_then(|o| o.disable_mouse).unwrap_or(false) {
            args.push("--disable-mouse".to_string());
        }
        if overrides.and_then(|o| o.disable_parallax).unwrap_or(false) {
            args.push("--disable-parallax".to_string());
        }
        if overrides.and_then(|o| o.disable_particles).unwrap_or(false) {
            args.push("--disable-particles".to_string());
        }

        if let Some(clamp) = overrides.and_then(|o| o.clamp.as_ref()) {
            if !clamp.is_empty() && clamp != "default" {
                args.push("--clamp".to_string());
                args.push(clamp.clone());
            }
        }

        if let Some(layer) = overrides.and_then(|o| o.layer.as_ref()) {
            if !layer.is_empty() {
                args.push("--layer".to_string());
                args.push(layer.clone());
            }
        }

        if overrides.and_then(|o| o.no_automute).unwrap_or(false) {
            args.push("--noautomute".to_string());
        }
        if overrides.and_then(|o| o.no_audio_processing).unwrap_or(false) {
            args.push("--no-audio-processing".to_string());
        }
        if overrides.and_then(|o| o.no_fullscreen_pause).unwrap_or(false) {
            args.push("--no-fullscreen-pause".to_string());
        }
        if overrides.and_then(|o| o.fullscreen_pause_only_active).unwrap_or(false) {
            args.push("--fullscreen-pause-only-active".to_string());
        }

        if let Some(shot) = overrides.and_then(|o| o.screenshot.as_ref()) {
            if !shot.is_empty() {
                args.push("--screenshot".to_string());
                args.push(shot.clone());
            }
        }

        if let Some(assets_dir) = crate::steam_scanner::resolve_wallpaper_engine_assets_dir() {
            args.push("--assets-dir".to_string());
            args.push(assets_dir.to_string_lossy().to_string());
        }

        if let Some(ov) = overrides {
            for (key, val) in &ov.custom_properties {
                args.push("--set-property".to_string());
                args.push(format!("{}={}", key, val));
            }
        }

        log_info(&format!("Spawning linux-wallpaperengine backend process: {:?} {:?}", lwe_bin.display(), args));

        let mut cmd = Command::new(&lwe_bin);
        cmd.args(&args);

        let child = cmd.spawn().map_err(|e| format!("Failed to spawn linux-wallpaperengine: {}", e))?;
        let mut proc_guard = self.lwe_process.lock().unwrap();
        *proc_guard = Some(child);

        let mut curr = self.current_wallpaper.lock().unwrap();
        *curr = Some(wallpaper_path.to_string_lossy().to_string());
        let mut st = self.user_stopped.lock().unwrap();
        *st = false;
        let mut p = self.is_paused.lock().unwrap();
        *p = false;

        Ok(())
    }

    pub fn stop_wallpaper(&self) -> Result<(), String> {
        let mut st = self.user_stopped.lock().unwrap();
        *st = true;
        let mut p = self.is_paused.lock().unwrap();
        *p = false;

        self.web_engine.stop();
        self.stop_widget_internal();
        self.stop_mpv_internal();
        self.stop_lwe_internal();

        let mut curr = self.current_wallpaper.lock().unwrap();
        *curr = None;

        log_info("Engine: Wallpaper stopped completely by user.");
        Ok(())
    }

    pub fn pause(&self) -> Result<(), String> {
        let mut p = self.is_paused.lock().unwrap();
        *p = true;

        if self.socket_path.exists() {
            let _ = self.send_mpv_command(serde_json::json!(["set_property", "pause", true]));
        }
        log_info("Engine: Playback paused.");
        Ok(())
    }

    pub fn resume(&self) -> Result<(), String> {
        let mut st = self.user_stopped.lock().unwrap();
        *st = false;
        let mut p = self.is_paused.lock().unwrap();
        *p = false;

        if self.socket_path.exists() {
            let res = self.send_mpv_command(serde_json::json!(["set_property", "pause", false]));
            if res.is_err() {
                if let Some(curr) = self.current_wallpaper.lock().unwrap().clone() {
                    let _ = self.set_wallpaper(Path::new(&curr));
                }
            }
        } else if let Some(curr) = self.current_wallpaper.lock().unwrap().clone() {
            let _ = self.set_wallpaper(Path::new(&curr));
        }
        log_info("Engine: Playback resumed.");
        Ok(())
    }

    pub fn toggle_pause(&self) -> Result<bool, String> {
        let is_stopped = *self.user_stopped.lock().unwrap();
        if is_stopped {
            if let Some(curr) = self.current_wallpaper.lock().unwrap().clone() {
                let _ = self.set_wallpaper(Path::new(&curr));
                return Ok(false);
            }
        }

        let current_state = *self.is_paused.lock().unwrap();
        let new_state = !current_state;

        if new_state {
            let _ = self.pause();
        } else {
            let _ = self.resume();
        }

        log_info(&format!("Engine: Toggled pause state to {}", new_state));
        Ok(new_state)
    }

    pub fn hide(&self) -> Result<(), String> {
        let mut h = self.is_hidden.lock().unwrap();
        *h = true;
        let _ = self.pause();
        log_info("Engine: Wallpaper hidden / paused for hypridle / user command.");
        Ok(())
    }

    pub fn show(&self) -> Result<(), String> {
        let mut h = self.is_hidden.lock().unwrap();
        *h = false;
        let _ = self.resume();
        log_info("Engine: Wallpaper restored / resumed from hide.");
        Ok(())
    }

    pub fn toggle_hide(&self) -> Result<bool, String> {
        let current_state = *self.is_hidden.lock().unwrap();
        let new_state = !current_state;
        if new_state {
            let _ = self.hide();
        } else {
            let _ = self.show();
        }
        Ok(new_state)
    }

    pub fn is_hidden(&self) -> bool {
        *self.is_hidden.lock().unwrap()
    }

    pub fn is_paused(&self) -> Result<bool, String> {
        let p = self.is_paused.lock().unwrap();
        Ok(*p)
    }

    pub fn is_user_stopped(&self) -> bool {
        *self.user_stopped.lock().unwrap()
    }

    pub fn set_volume(&mut self, vol: i64) -> Result<(), String> {
        let clamped = vol.clamp(0, 100);
        if self.socket_path.exists() {
            let _ = self.send_mpv_command(serde_json::json!(["set_property", "volume", clamped]));
        }
        let mut v = self.volume.lock().unwrap();
        *v = clamped;
        Ok(())
    }

    pub fn set_mute(&mut self, mute: bool) -> Result<(), String> {
        if self.socket_path.exists() {
            let _ = self.send_mpv_command(serde_json::json!(["set_property", "mute", mute]));
        }
        let mut m = self.mute.lock().unwrap();
        *m = mute;
        Ok(())
    }

    pub fn set_hwdec(&mut self, hwdec: &str) -> Result<(), String> {
        if self.socket_path.exists() {
            let _ = self.send_mpv_command(serde_json::json!(["set_property", "hwdec", hwdec]));
        }
        let mut h = self.hwdec.lock().unwrap();
        *h = hwdec.to_string();
        Ok(())
    }

    pub fn set_gpu_device(&mut self, gpu_device: Option<String>) -> Result<(), String> {
        let mut dev = self.gpu_device.lock().unwrap();
        *dev = gpu_device;
        Ok(())
    }

    pub fn gpu_device(&self) -> Option<String> {
        self.gpu_device.lock().unwrap().clone()
    }

    pub fn set_target_fps(&mut self, fps: u32) -> Result<(), String> {
        if self.socket_path.exists() {
            let _ = self.send_mpv_command(serde_json::json!(["set_property", "override-display-fps", fps]));
        }
        let mut f = self.target_fps.lock().unwrap();
        *f = fps;
        Ok(())
    }

    pub fn target_fps(&self) -> u32 {
        *self.target_fps.lock().unwrap()
    }

    pub fn set_screen(&mut self, screen_id: i64) -> Result<(), String> {
        if self.socket_path.exists() {
            let _ = self.send_mpv_command(serde_json::json!(["set_property", "fs-screen", screen_id]));
        }
        let mut s = self.screen_id.lock().unwrap();
        *s = screen_id;
        Ok(())
    }

    pub fn current_wallpaper(&self) -> Option<String> {
        self.current_wallpaper.lock().unwrap().clone()
    }

    pub fn volume(&self) -> i64 {
        *self.volume.lock().unwrap()
    }

    pub fn is_muted(&self) -> bool {
        *self.mute.lock().unwrap()
    }

    pub fn hwdec(&self) -> String {
        self.hwdec.lock().unwrap().clone()
    }

    pub fn screen_id(&self) -> i64 {
        *self.screen_id.lock().unwrap()
    }
}
