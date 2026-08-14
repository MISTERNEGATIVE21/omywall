# Omywall UI/UX Overhaul & In-App Web Rendering Specification

- **Date**: 2026-08-14
- **Version**: 5.0.0
- **Target Platform**: Linux (Wayland / Hyprland / Sway / X11)
- **Primary Technologies**: Rust, Iced 0.14, GTK3 / WebKit2GTK, Electron / Chromium Headless, MPV, Tokio

---

## 1. Overview & Objectives

This specification defines the complete overhaul of the Omywall graphical user interface and media rendering engine. It addresses the in-app website and 3D WebGL rendering pipeline, wires up all unrendered tabs and modals, and establishes a modern, responsive, dark-glass UI/UX.

### Key Objectives:
1. **Flawless In-App Web & 3D Rendering**: Ensure local HTML/WebGL wallpaper assets and remote web URLs render crisp thumbnails, support 5 FPS live animated preview streaming in the Spotlight Player and hover cards, and provide one-click interactive preview windows.
2. **Robust Multi-Tier Headless Capture**: Resolve Electron v43 invocation failures by packaging preview scripts in an isolated application bundle with Chromium software WebGL fallbacks (`--enable-unsafe-swiftshader`), plus headless Chromium CLI and WebKit2GTK fallbacks.
3. **Catalog Synchronization**: Automatically merge built-in WebGL presets and user bookmarks from `config.saved_web_wallpapers` into the main catalog on startup and during user additions/deletions.
4. **Complete Tab & Modal Implementation**:
   - **Installed Wallpapers**: Real-time search filtering, media type categories, Grid vs. Carousel views, bookmark management, folder/file pickers.
   - **Steam Workshop Browser**: Live Steam Workshop item browser with query search, sorting (Trending, Popular, Recent), SteamCMD download tracking, and direct application.
   - **System Doctor Modal**: Interactive diagnostic dashboard checking system dependencies (`mpv`, `ffmpeg`, `electron`, `chromium`, `webkit2gtk`, `hyprctl`, `steamcmd`, GPU drivers) with one-click fix action.
   - **Live Logs Modal**: Real-time log inspector with refresh, clear, and clipboard copy.
   - **Display Manager Tab**: Visual monitor cards with per-screen wallpaper assignment.
   - **Screensaver Tab**: Hyprlock preview with customizable accent colors and test runner.
   - **Settings Tab**: Hardware video acceleration options, FPS limiter, audio volume/mute sliders, autostart toggle, and daemon controls.

---

## 2. Architecture & In-App Web Rendering Pipeline

```
+-----------------------------------------------------------------------+
|                             ICED GUI APP                              |
+-----------------------------------+-----------------------------------+
                                    |
            +-----------------------+-----------------------+
            |                                               |
  [Static Card Thumbnail]                         [Spotlight Live Stream]
            |                                               |
            v                                               v
+-----------------------+                       +-----------------------+
|  get_web_thumbnail()  |                       |   electron_preview    |
|  (/tmp/omywall_thumbs)|                       |      start_live()     |
+-----------+-----------+                       +-----------+-----------+
            |                                               |
            +---------------+                               |
            |               |                               |
            v               v                               v
    +---------------+---------------+               +---------------+
    |   Electron    |   Chromium    |               | 5 FPS Frame   |
    |   App Bundle  |   Headless    |               | Capture to    |
    |   Snapshot    |   Fallback    |               | hover_web_    |
    |               | (--headless)  |               | live.png      |
    +---------------+---------------+               +-------+-------+
            |               |                               |
            +---------------+                               |
                            |                               v
                            v                       +---------------+
                    +---------------+               |  Iced Async   |
                    |  Save PNG to  |               |  Image Cache  |
                    |  Cache & Fire |               |  & Spotlight  |
                    |  ThumbDecoded |               |  Render Loop  |
                    +---------------+               +---------------+
```

### 2.1 Electron Preview Bundle (`src/electron_preview.rs`)
- **Directory Structure**:
  - Automatically initializes `/tmp/omywall_thumbs/electron_app/` with:
    - `package.json`: `{"name": "omywall-preview", "main": "main.js"}`
    - `main.js`: Capture script supporting both `shot` (one-off snapshot) and `live` (continuous 5 FPS capture to `/tmp/omywall_thumbs/hover_web_live.png`).
- **Chromium / Ozone Switches**:
  - `--ozone-platform=wayland` with graceful X11 fallback
  - `--enable-features=UseOzonePlatform`
  - `--autoplay-policy=no-user-gesture-required`
  - `--allow-file-access-from-files`
  - `--enable-unsafe-swiftshader` (prevents WebGL blanking under headless drivers)
  - `--ignore-gpu-blocklist`
  - `--enable-gpu-rasterization`
- **Capture Execution**:
  - `win.webContents.capturePage()` invoked with a 500ms stabilization timeout after `did-finish-load` to ensure WebGL shaders and canvas drawing routines initialize.

### 2.2 Secondary Fallback: Chromium Headless Snapshot
- If Electron is not installed or errors on execution, execute:
  ```bash
  chromium --headless --disable-gpu --allow-file-access-from-files --virtual-time-budget=2000 --window-size=600,337 --screenshot=<out_path> <target_url>
  ```
- Guaranteed single-frame generation without dependency on an active Wayland compositor.

### 2.3 Tertiary Fallback: In-Process WebKit2GTK (`src/webkit_render.rs`)
- Dedicated GTK3 rendering thread providing offscreen cairo surface snapshot for environments with WebKit2GTK libraries.

### 2.4 Interactive Web Preview
- User clicking **"Preview Web"** launches an interactive standalone window using Electron or Chromium (`--app=<url>`), permitting full mouse/keyboard interaction with WebGL/3D scenes.

---

## 3. UI/UX Layout & Component Architecture

### 3.1 Header & System Telemetry
- **Branding**: `OMYWALL` logo in Cyan (`#00f0ff`) with subtitle tag `v5.0.0`.
- **System Metrics Badge**:
  - GPU Usage: Dynamic color (Emerald `< 50%`, Amber `50–85%`, Crimson `> 85%`).
  - GPU Device: Hardware name (e.g. `RTX 4070 / Intel Xe`).
  - CPU Usage: Cyan percentage text.
  - RAM Usage: Megabytes active memory.
- **Top Actions**:
  - Engine Status Pill (`● Running` / `● Idle`).
  - **"⚙ System Doctor"** button with alert badge if dependencies are missing.
  - **"📋 Logs"** button to toggle live log stream.

### 3.2 Navigation & Tabs
1. 🖼 **Installed Wallpapers**: Local library, 3D WebGL assets, video wallpapers, static images, and saved URLs.
2. 🌐 **Steam Workshop Browser**: Steam Workshop catalog explorer, query search, sorting, and SteamCMD downloader.
3. 📺 **Displays Manager**: Connected monitor geometry, refresh rate, and per-screen wallpaper assignment.
4. 🔒 **Screensaver**: Hyprlock settings, mode selector, clock accent colors, and live test runner.
5. ⚙ **Settings**: Hardware acceleration, audio volume/mute, opacity, autostart, slideshow, and daemon lifecycle.

---

## 4. Detailed Tab Specifications

### 4.1 Installed Wallpapers Tab
- **Filter Bar**:
  - Category Pills: **All** | 🎥 **Videos** | 🌐 **Web & 3D** | 🖼 **Images** | 🎮 **Steam Items**.
  - **Live Search Bar**: Text input filtering items in real time.
  - **View Mode Switch**: **⣿ Grid** (4-column card grid) vs. **🎠 Carousel** (Spotlight player + filmstrip).
  - **Actions**: **📁 Select Folder**, **➕ Select File**.
  - **Web URL Adder**: `[ Paste URL (https://...) ] [ Title (optional) ] [ 💾 Save & Add URL ]`.
- **Grid Card (`render_wallpaper_card`)**:
  - 16:9 thumbnail preview with media badge (`WebGL`, `Video`, `Image`, `Steam`).
  - Wallpaper title and path/source label.
  - Card interactions: Hover highlight, click to select, double-click to apply.
  - Actions: **▶ Apply**, **👁 Preview**, **🗑 Remove** (for custom bookmarks).
- **Carousel Spotlight Player (`render_carousel_view`)**:
  - 600x337px live spotlight player with real-time video/WebGL frame updates.
  - Metadata badges: Renderer type (`WebKit2GTK / Chromium`, `MPV Hardware`), dimensions, aspect ratio.
  - Navigation controls: `◀ Previous`, `▶ Set Active Wallpaper`, `👁 External Preview`, `Next ▶`.
  - 12-item scrollable thumbnail filmstrip below player.

### 4.2 Steam Workshop Browser Tab
- **Search & Sort Toolbar**:
  - Search input with clear button.
  - Sort selector: `Trending (7 Days)`, `Most Popular`, `Most Recent`.
  - Buttons: `🔍 Search Workshop`, `📂 Scan Local Steam Items`, `🔄 Refresh`.
- **Workshop Cards Grid**:
  - Thumbnail preview loaded from Steam CDN cache.
  - Title, author name, subscription / favorite count, file size.
  - Actions:
    - **⬇ Download (SteamCMD)** with status spinner.
    - **▶ Apply Directly** once downloaded.
    - **➕ Add to Local Library**.
- **Pagination Bar**: `◀ Prev Page`, `Page X of Y`, `Next Page ▶`.

### 4.3 Display Manager Tab
- Visual monitor cards representing each connected display output (e.g. `eDP-1`, `HDMI-A-1`, `DP-1`).
- Metrics: Active resolution, refresh rate (Hz), position offset.
- Per-display wallpaper selection dropdown and **"Set Wallpaper on Monitor"** trigger.
- **"🔄 Rescan Displays"** button.

### 4.4 Screensaver / Hyprlock Tab
- **Mode Selector**: `🌌 Active Live` | `🌀 Blurred Static` | `🎨 Solid Color`.
- **Status Toggle**: Enabled / Disabled switch.
- **Clock Accent Colors**: Cyan (`#00f0ff`), Emerald (`#10b981`), Amber (`#f59e0b`), Purple (`#a855f7`), White (`#ffffff`).
- **Interactive Live Preview Box**: Simulated lockscreen overlay with selected accent color, clock, and lock indicator.
- **Actions**: `👁 Live Fullscreen Test`, `💾 Save Screensaver Config`.

### 4.5 Settings & Engine Control Tab
- **Theme Schemes**: `🌌 Dark Glass`, `⚡ Steam Amber`, `🔮 Cyber Light`, `🖤 OLED Black`.
- **Hardware Acceleration**: Video decoder dropdown (`nvdec`, `vaapi`, `vulkan`, `cuda`, `auto`, `no`).
- **Audio & Visual**:
  - Volume slider (0–100%) with numerical percentage and **Mute Toggle**.
  - Wallpaper Opacity slider (0.0–1.0).
- **Daemon Lifecycle**:
  - Autostart on boot checkbox.
  - Slideshow interval input (seconds) and shuffle toggle.
  - Action buttons: `▶ Start Daemon`, `⏹ Stop Daemon`, `⏯ Toggle Pause`.
  - `🛠 Run Dependency Installer Script` trigger.

---

## 5. Modals & Diagnostics

### 5.1 System Doctor Modal
- Triggered by header button or missing dependency alert.
- Diagnostic table checking:
  1. `mpv`: Video engine & hardware acceleration.
  2. `ffmpeg`: Video frame extraction & hover streaming.
  3. `electron`: In-app WebGL live previewer.
  4. `chromium`: Headless snapshot fallback.
  5. `webkit2gtk`: Layer-shell desktop background engine.
  6. `hyprctl` / `swaymsg`: Wayland compositor integration.
  7. `steamcmd`: Workshop downloader.
  8. `nvidia-smi` / `vulkaninfo`: GPU hardware drivers.
- Status badges: Green `● OK` / Red `▲ Missing`.
- **"🛠 Run Auto-Fix / Install Missing Tools"** button executing `./install.sh`.
- Modal close & backdrop dismiss.

### 5.2 Live Logs Modal
- Monospace scrollable viewer reading `~/.local/state/omywall/omywall.log`.
- Auto-refresh toggle, manual refresh button, clear logs button, and clipboard copy.

---

## 6. Error Handling & Edge Cases

| Scenario | Behavior |
| :--- | :--- |
| **Electron not installed** | Automatically fallback to `chromium --headless --screenshot` for thumbnails and `webkit2gtk` for layer-shell. |
| **Wayland headless capture stall** | Set 3-second hard timeout on snapshot generation; gracefully release slots to prevent thread starvation. |
| **Corrupted / invalid web URL** | Display clear error message in status bar without crashing the UI thread. |
| **No wallpapers in directory** | Display informative empty-state container with buttons to import folder, file, or web URL. |
| **Daemon not running** | GUI operates in standalone mode; displays warning in status bar with one-click **"Start Daemon"** action. |

---

## 7. Verification & Testing Plan

1. **Compilation & Linting**:
   - Run `cargo check` and `cargo clippy` ensuring zero warnings or compile errors.
2. **Web Rendering Validation**:
   - Verify thumbnail generation for local WebGL assets (`assets/web_wallpapers/*.html`).
   - Verify thumbnail generation for external web URLs (`https://...`).
   - Verify live 5 FPS spotlight streaming in Carousel and Card hover.
   - Verify standalone interactive web preview window launch.
3. **UI/UX Interaction Testing**:
   - Test instant search filtering in Installed Wallpapers.
   - Test switching between Grid and Carousel view modes.
   - Test opening and closing System Doctor and Logs modals.
   - Test Steam Workshop browser search and sorting.
   - Test Display Manager monitor detection.
   - Test Screensaver clock color customization and config generation.
   - Test Settings volume, hardware acceleration, and daemon start/stop triggers.
