use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

/// Offscreen Electron renderer used to preview HTML / web widget wallpapers.
///
/// The GTK WebKit capture path (`webkit_render`) cannot grab composited page
/// content on Wayland, so HTML wallpapers come out blank in the spotlight
/// player. Electron's `webContents.capturePage()` reads from Chromium's own
/// compositor, which produces real pixels regardless of the window manager.
///
/// The captured frames are written to the same PNG paths the GUI already polls
/// (`HOVER_WEB_LIVE_PATH` for live previews, the per-wallpaper web thumbnail
/// file for cards), so the existing decode/display pipeline is reused.

const PREVIEW_SCRIPT: &str = r#"const { app, BrowserWindow } = require('electron');
const fs = require('fs');

const url = process.argv[2];
const outPng = process.argv[3];
const width = parseInt(process.argv[4] || '600', 10);
const height = parseInt(process.argv[5] || '337', 10);
const mode = process.argv[6] || 'shot';

const fsBase = outPng.substring(0, outPng.lastIndexOf('/'));
const profileDir = fsBase + '/electron_profile_' + process.pid;
app.setPath('userData', profileDir);

app.commandLine.appendSwitch('ozone-platform', 'wayland');
app.commandLine.appendSwitch('enable-features', 'UseOzonePlatform');
app.commandLine.appendSwitch('autoplay-policy', 'no-user-gesture-required');
app.commandLine.appendSwitch('ignore-gpu-blocklist');
app.commandLine.appendSwitch('enable-gpu-rasterization');

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
    },
  });
  win.setAlwaysOnTop(true, 'screen-saver');
  win.webContents.setAudioMuted(true);
  win.webContents.setFrameRate(5);
  win.webContents.on('did-finish-load', () => setTimeout(capture, 700));
  win.webContents.on('did-fail-load', (_e, code, desc) => {
    console.error('preview load failed:', code, desc);
    if (mode === 'shot') {
      setTimeout(() => app.exit(1), 300);
    }
  });
  win.loadURL(url);
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

/// Cap on concurrently running one-shot Electron capture instances. Each one
/// boots a full Chromium renderer, so unbounded spawning (a catalog can have
/// dozens of HTML wallpapers) would grind the machine to a halt.
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

fn script_path() -> PathBuf {
    let dir = PathBuf::from("/tmp/omywall_thumbs");
    let _ = std::fs::create_dir_all(&dir);
    let script = dir.join("electron_preview.js");
    let _ = std::fs::write(&script, PREVIEW_SCRIPT);

    // Sweep stale per-instance Chromium profiles (older than a day).
    if let Ok(entries) = std::fs::read_dir(&dir) {
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
    script
}

/// Normalize a wallpaper target into a loadable URL. Local paths must carry a
/// `file://` scheme or Electron's `loadURL` treats them as unknown schemes and
/// fails instantly.
fn to_url(input: &str) -> String {
    if input.starts_with("http://") || input.starts_with("https://") || input.starts_with("file://") {
        input.to_string()
    } else {
        format!("file://{}", input)
    }
}

fn electron_available() -> bool {
    std::process::Command::new("which")
        .arg("electron")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn spawn(args: &[String]) -> Option<Child> {
    Command::new("electron")
        .args(args)
        .env("ELECTRON_ENABLE_LOGGING", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()
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
    let script = script_path();
    let url_str = to_url(url);
    let out_str = out.to_string_lossy().to_string();
    if let Some(child) = spawn(&[
        script.to_string_lossy().into_owned(),
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

/// Render a single preview frame of an HTML/web target into `out` in the
/// background. Requests for the same output path are deduplicated so a render
/// is only kicked off once.
pub fn render_shot(url: &str, out: &Path) {
    if !electron_available() {
        if let Some(renderer) = crate::webkit_render::global_renderer() {
            renderer.render_thumbnail(url, out);
        }
        return;
    }
    {
        if shot_is_pending(out) {
            return;
        }
        // No slot free right now: leave this path unmarked so a later Tick
        // retries it once another capture finishes.
        if !acquire_shot_slot() {
            return;
        }
        shot_insert(out.to_path_buf());
    }

    let script = script_path();
    let url_str = to_url(url);
    let out_str = out.to_string_lossy().to_string();
    let _ = std::fs::remove_file(out);

    std::thread::spawn(move || {
        if spawn(&[
            script.to_string_lossy().into_owned(),
            url_str.clone(),
            out_str.clone(),
            "600".to_string(),
            "337".to_string(),
            "shot".to_string(),
        ])
        .and_then(|mut c| c.wait().ok())
        .is_none()
        {
            // Electron crashed or failed to launch; let the WebKit queue try.
            if let Some(renderer) = crate::webkit_render::global_renderer() {
                renderer.render_thumbnail(&url_str, Path::new(&out_str));
            }
        }
        shot_remove(Path::new(&out_str));
        release_shot_slot();
        crate::gui::notify_thumb_updated(PathBuf::from(&out_str));
    });
}
