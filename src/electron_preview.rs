use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

/// Offscreen Electron renderer used to preview HTML / WebGL / web widget wallpapers.
///
/// Electron's `webContents.capturePage()` reads from Chromium's internal compositor,
/// capturing full hardware-accelerated and software WebGL content.
///
/// Captured frames are written to PNG files polled by the UI (spotlight preview,
/// hover live preview, card thumbnails).

const PACKAGE_JSON: &str = r#"{
  "name": "omywall-preview",
  "version": "1.0.0",
  "description": "Omywall Electron WebGL & HTML Preview Engine",
  "main": "main.js"
}
"#;

const MAIN_JS: &str = r#"const { app, BrowserWindow } = require('electron');
const fs = require('fs');

const url = process.argv[2];
const outPng = process.argv[3];
const width = parseInt(process.argv[4] || '600', 10);
const height = parseInt(process.argv[5] || '337', 10);
const mode = process.argv[6] || 'shot';

const fsBase = outPng ? outPng.substring(0, outPng.lastIndexOf('/')) : '/tmp/omywall_thumbs';
const profileDir = fsBase + '/electron_profile_' + process.pid;
app.setPath('userData', profileDir);

app.commandLine.appendSwitch('ozone-platform-hint', 'auto');
app.commandLine.appendSwitch('autoplay-policy', 'no-user-gesture-required');
app.commandLine.appendSwitch('ignore-gpu-blocklist');
app.commandLine.appendSwitch('enable-gpu-rasterization');
app.commandLine.appendSwitch('enable-unsafe-swiftshader');
app.commandLine.appendSwitch('allow-file-access-from-files');
app.commandLine.appendSwitch('disable-vulkan');

let win = null;
let saved = 0;
const SHOT_FRAMES = 3;

function saveFrame(image) {
  const size = image.getSize();
  if (!size || size.width < 2 || size.height < 2) return;
  fs.writeFile(outPng, image.toPNG(), (err) => {
    if (err) console.error('preview write error:', err.message);
  });
  saved += 1;
  if (mode === 'shot' && saved >= SHOT_FRAMES) {
    setTimeout(() => app.exit(0), 150);
  }
}

function capture() {
  if (!win || win.isDestroyed()) return;
  win.webContents.capturePage().then(saveFrame).catch(() => {});
  if (mode === 'live') {
    setTimeout(capture, 200);
  }
}

app.whenReady().then(() => {
  win = new BrowserWindow({
    width,
    height,
    show: true,
    frame: false,
    opacity: 0.0,
    transparent: false,
    backgroundColor: '#000000',
    skipTaskbar: true,
    focusable: false,
    alwaysOnTop: true,
    webPreferences: {
      backgroundThrottling: false,
      autoplayPolicy: 'no-user-gesture-required',
      webSecurity: false,
      allowRunningInsecureContent: true,
    },
  });
  win.setAlwaysOnTop(true, 'screen-saver');
  win.webContents.setAudioMuted(true);
  win.webContents.setFrameRate(5);
  win.webContents.on('did-finish-load', () => setTimeout(capture, 500));
  win.webContents.on('did-fail-load', (_e, code, desc) => {
    console.error('preview load failed:', code, desc);
    if (mode === 'shot') {
      setTimeout(() => app.exit(1), 300);
    }
  });
  if (url) {
    win.loadURL(url);
  }
  setTimeout(() => {
    if (saved === 0) capture();
  }, 3500);
  if (mode === 'shot') {
    setTimeout(() => app.exit(0), 12000);
  }
});
"#;

static LIVE_CHILD: Mutex<Option<Child>> = Mutex::new(None);
static SHOT_PENDING: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);

/// Cap on concurrently running one-shot Electron capture instances.
const MAX_SHOT_SPAWNS: u32 = 2;
static SHOT_SPAWNS: Mutex<u32> = Mutex::new(0);

fn acquire_shot_slot() -> bool {
    if let Ok(mut g) = SHOT_SPAWNS.lock() {
        if *g >= MAX_SHOT_SPAWNS {
            return false;
        }
        *g += 1;
        return true;
    }
    false
}

fn release_shot_slot() {
    if let Ok(mut g) = SHOT_SPAWNS.lock() {
        *g = g.saturating_sub(1);
    }
}

fn shot_is_pending(out: &Path) -> bool {
    SHOT_PENDING
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|s| s.contains(out)))
        .unwrap_or(false)
}

fn shot_insert(out: PathBuf) {
    if let Ok(mut g) = SHOT_PENDING.lock() {
        g.get_or_insert_with(HashSet::new).insert(out);
    }
}

fn shot_remove(out: &Path) {
    if let Ok(mut g) = SHOT_PENDING.lock() {
        if let Some(set) = g.as_mut() {
            set.remove(out);
        }
    }
}

/// Ensures the isolated Electron app bundle exists at `/tmp/omywall_thumbs/electron_app/`
/// containing `package.json` and `main.js`.
pub fn ensure_app_bundle() -> PathBuf {
    let base = PathBuf::from("/tmp/omywall_thumbs");
    let app_dir = base.join("electron_app");
    let _ = std::fs::create_dir_all(&app_dir);

    let pkg = app_dir.join("package.json");
    let _ = std::fs::write(&pkg, PACKAGE_JSON);

    let main = app_dir.join("main.js");
    let _ = std::fs::write(&main, MAIN_JS);

    // Sweep stale per-instance Chromium profiles (older than a day).
    if let Ok(entries) = std::fs::read_dir(&base) {
        let now = std::time::SystemTime::now();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with("electron_profile_") {
                if let Ok(meta) = entry.metadata() {
                    if let Ok(modified) = meta.modified() {
                        if now.duration_since(modified).map(|d| d.as_secs() > 86400).unwrap_or(false) {
                            let _ = std::fs::remove_dir_all(entry.path());
                        }
                    }
                }
            }
        }
    }
    app_dir
}

/// Normalize a wallpaper target into a loadable URL. Local paths must carry a
/// `file://` scheme.
pub fn to_url(input: &str) -> String {
    if input.starts_with("http://") || input.starts_with("https://") || input.starts_with("file://") {
        input.to_string()
    } else {
        format!("file://{}", input)
    }
}

pub fn electron_available() -> bool {
    Command::new("which")
        .arg("electron")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn find_chromium_bin() -> Option<String> {
    for bin in &["chromium", "google-chrome-stable", "google-chrome", "chromium-browser", "chrome"] {
        if Command::new("which").arg(bin).output().map(|o| o.status.success()).unwrap_or(false) {
            return Some((*bin).to_string());
        }
    }
    None
}

pub fn chromium_available() -> bool {
    find_chromium_bin().is_some()
}

fn spawn_electron(args: &[String]) -> Option<Child> {
    let app_dir = ensure_app_bundle();
    let mut cmd_args = vec![app_dir.to_string_lossy().into_owned()];
    cmd_args.extend_from_slice(args);

    Command::new("electron")
        .args(&cmd_args)
        .env("ELECTRON_ENABLE_LOGGING", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()
}

/// Execute headless Chromium screenshot fallback when Electron is unavailable or fails.
pub fn capture_fallback_chromium(url: &str, out: &Path) -> bool {
    let bin = match find_chromium_bin() {
        Some(b) => b,
        None => return false,
    };
    if let Some(parent) = out.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let url_str = to_url(url);
    let screenshot_arg = format!("--screenshot={}", out.to_string_lossy());
    let status = Command::new(bin)
        .arg("--headless")
        .arg("--disable-gpu")
        .arg("--allow-file-access-from-files")
        .arg("--virtual-time-budget=2000")
        .arg("--window-size=600,337")
        .arg(&screenshot_arg)
        .arg(&url_str)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    match status {
        Ok(s) if s.success() => {
            out.exists() && std::fs::metadata(out).map(|m| m.len() > 0).unwrap_or(false)
        }
        _ => false,
    }
}

/// Start a continuously-updating live preview of an HTML/web target into `out`.
/// Falls back to the WebKit live pipeline if Electron is unavailable.
pub fn start_live(url: &str, out: &Path) {
    stop_live();
    let _ = std::fs::remove_file(out);
    if !electron_available() {
        crate::webkit_render::start_live_pip(url, out);
        return;
    }
    let url_str = to_url(url);
    let out_str = out.to_string_lossy().to_string();
    if let Some(child) = spawn_electron(&[
        url_str,
        out_str,
        "600".to_string(),
        "337".to_string(),
        "live".to_string(),
    ]) {
        if let Ok(mut guard) = LIVE_CHILD.lock() {
            *guard = Some(child);
        }
    } else {
        crate::webkit_render::start_live_pip(url, out);
    }
}

pub fn stop_live() {
    if let Ok(mut guard) = LIVE_CHILD.lock() {
        if let Some(mut child) = guard.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Render a single preview frame of an HTML/web target into `out` in the background.
/// Follows a 3-tier fallback strategy:
/// 1. Electron App Bundle (Software WebGL & full canvas support)
/// 2. Headless Chromium Snapshot
/// 3. WebKit2GTK Render Pipeline
pub fn render_shot(url: &str, out: &Path) {
    {
        if shot_is_pending(out) {
            return;
        }
        // No slot free right now: leave this path unmarked so a later Tick retries it
        if !acquire_shot_slot() {
            return;
        }
        shot_insert(out.to_path_buf());
    }

    let url_str = to_url(url);
    let out_path = out.to_path_buf();
    let out_str = out_path.to_string_lossy().to_string();
    let _ = std::fs::remove_file(&out_path);

    std::thread::spawn(move || {
        let mut captured = false;

        // Tier 1: Electron App Bundle
        if electron_available() {
            if let Some(mut child) = spawn_electron(&[
                url_str.clone(),
                out_str.clone(),
                "600".to_string(),
                "337".to_string(),
                "shot".to_string(),
            ]) {
                if let Ok(status) = child.wait() {
                    if status.success() && Path::new(&out_str).exists() && std::fs::metadata(&out_str).map(|m| m.len() > 0).unwrap_or(false) {
                        captured = true;
                    }
                }
            }
        }

        // Tier 2: Chromium Headless Snapshot
        if !captured && chromium_available() {
            captured = capture_fallback_chromium(&url_str, Path::new(&out_str));
        }

        // Tier 3: WebKit Renderer
        if !captured {
            if let Some(renderer) = crate::webkit_render::global_renderer() {
                renderer.render_thumbnail(&url_str, Path::new(&out_str));
            }
        }

        shot_remove(Path::new(&out_str));
        release_shot_slot();
        crate::gui::notify_thumb_updated(PathBuf::from(&out_str));
    });
}
