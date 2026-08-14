# Omywall UI/UX Overhaul, In-App Web Rendering & Desktop Widgets Specification

- **Date**: 2026-08-14
- **Version**: 5.0.0
- **Target Platform**: Linux (Wayland / Hyprland / Sway / X11)
- **Primary Technologies**: Rust, Iced 0.14, GTK3 / WebKit2GTK, Electron / Chromium Headless, MPV, Tokio, Linux System APIs (NetworkManager, BlueZ/bluetoothctl, Sysfs)

---

## 1. Overview & Objectives

This specification defines the complete overhaul of the Omywall graphical user interface, in-app media/3D rendering pipeline, and desktop overlay widgets subsystem. It addresses in-app WebGL rendering, adds a live WiFi/Bluetooth/System desktop widgets suite, wires up all unrendered tabs and modals, and establishes a state-of-the-art dark-glass UI/UX.

### Key Objectives:
1. **Live WiFi, Bluetooth & Desktop Widgets Suite**:
   - Built-in glassmorphic desktop widgets displaying real-time WiFi (SSID, signal %, IP), Bluetooth (power state, connected devices, battery), System meters (CPU/GPU/RAM/Battery), Clock/Date, and Media player.
   - Transparent Wayland layer-shell overlay surface (`Layer::Bottom` / `Layer::Overlay`) with alpha blending over any video/image wallpaper.
   - Native Rust system telemetry daemon gathering status via `nmcli`/`sysfs`/`bluetoothctl` without lag or heavy overhead.
2. **Flawless In-App Web & 3D Rendering**:
   - Crisp thumbnails for local WebGL assets and external web URLs.
   - Asynchronous 5 FPS live animated spotlight streaming in Carousel and Card hover.
   - One-click interactive standalone preview window with mouse/audio controls.
3. **Multi-Tier Headless Capture Engine**:
   - Electron v43 isolated application bundle with Chromium software WebGL flags (`--enable-unsafe-swiftshader`), plus headless Chromium CLI and WebKit2GTK fallbacks.
4. **Catalog Synchronization**:
   - Auto-merge built-in 3D WebGL scenes, widgets, and user bookmarks from `config.saved_web_wallpapers` into the main catalog on boot.
5. **Complete Tab & Modal Implementation**:
   - **Installed Wallpapers**: Real-time search filter, category pills, Grid vs. Carousel views, bookmark addition/removal, file/folder pickers.
   - **Steam Workshop Browser**: Store explorer, query search, sorting, and SteamCMD downloader.
   - **Desktop Widgets Tab**: Widget toggle, preset picker (Cyber HUD, WiFi/Bluetooth Pill, Minimal Clock), position selector, and live preview.
   - **System Doctor Modal**: Interactive diagnostic dashboard checking `mpv`, `ffmpeg`, `electron`, `chromium`, `webkit2gtk`, `hyprctl`, `steamcmd`, `nmcli`, `bluetoothctl`, and GPU drivers.
   - **Live Logs Modal**: Real-time log inspector with refresh, clear, and clipboard copy.
   - **Display Manager Tab**: Visual monitor cards with per-screen wallpaper assignment.
   - **Screensaver Tab**: Hyprlock preview with customizable accent colors and test runner.
   - **Settings Tab**: Hardware acceleration dropdown, FPS limiter, audio volume/mute sliders, autostart toggle, and daemon controls.

---

## 2. Architecture & Subsystems

```
+---------------------------------------------------------------------------------+
|                                  ICED GUI APP                                   |
+----------------------------------------+----------------------------------------+
                                         |
         +-------------------------------+-------------------------------+
         |                               |                               |
[Installed Wallpapers]          [Desktop Widgets Tab]          [Steam Workshop & Modals]
         |                               |                               |
         v                               v                               v
+------------------+           +-------------------+           +-------------------+
| Multi-Tier Web   |           | Transparent Layer |           | Diagnostics &     |
| Capture Pipeline |           | Shell Web Overlay |           | Settings Controls |
+--------+---------+           +---------+---------+           +-------------------+
         |                               |
         +---------------+               |
         |               |               |
         v               v               v
+----------------+---------------+---------------+---------------------------------+
| Electron App   | Chromium CLI  | WebKit2GTK    | System Telemetry Bridge         |
| Bundle Capture | Headless Snap | Layer-Shell   | (WiFi / Bluetooth / Battery /   |
| (5 FPS Stream) | (Thumbnail)   | Overlay       | CPU / GPU / RAM / Media)        |
+----------------+---------------+---------------+---------------------------------+
```

---

## 3. WiFi, Bluetooth & Desktop Widgets Architecture

### 3.1 Native Telemetry Poller (`src/widgets_bridge.rs`)
- Background task running every 1.5 seconds in the daemon / GUI process.
- Gathers data and writes to `/tmp/omywall_telemetry.json`:
  ```json
  {
    "wifi": {
      "connected": true,
      "ssid": "Home-5G",
      "signal_percent": 88,
      "ip": "192.168.1.105",
      "interface": "wlan0"
    },
    "bluetooth": {
      "powered": true,
      "connected_devices": [
        { "name": "Sony WH-1000XM4", "mac": "XX:XX:XX:XX:XX:XX", "battery": 90 }
      ]
    },
    "battery": {
      "present": true,
      "percent": 85,
      "charging": false,
      "status": "Discharging"
    },
    "system": {
      "cpu_usage": 14.5,
      "gpu_usage": 8.0,
      "gpu_name": "NVIDIA GeForce RTX",
      "ram_used_mb": 4200,
      "ram_total_mb": 16000
    },
    "clock": {
      "time": "11:20:45",
      "date": "Friday, August 14",
      "day": "Friday"
    }
  }
  ```
- **Fallback Mechanisms**:
  - WiFi: Reads `nmcli -t -f active,ssid,signal,device dev wifi` -> falls back to `/proc/net/wireless` and `/sys/class/net/`.
  - Bluetooth: Reads `bluetoothctl show` & `bluetoothctl devices Connected` -> falls back to `/sys/class/bluetooth/`.
  - Battery: Directly reads `/sys/class/power_supply/BAT*/capacity` and `status`.

### 3.2 Glassmorphic Desktop Widget Asset (`assets/widgets/desktop_hud.html`)
- Ultra-modern HTML5/CSS3 glassmorphism dashboard:
  - **WiFi Card**: Real-time signal strength meter, SSID indicator, IP display, live connection status.
  - **Bluetooth Card**: Active power toggle indicator, connected devices list with battery indicators.
  - **Hardware Gauges**: Minimalist circular rings for CPU, GPU, RAM, and Battery.
  - **Clock & Calendar**: Clean typographic clock with animated glow and day/date.
- **Auto-Sync Script**: Periodically fetches `file:///tmp/omywall_telemetry.json` (or via fetch/XHR) to update widgets smoothly without reloading the DOM.

### 3.3 Transparent Wayland Layer-Shell Overlay (`src/web_layer.rs`)
- In Layer-Shell mode:
  - Window set to `Layer::Bottom` (or `Layer::Overlay`), anchored to desired screen edges.
  - WebKit WebView configured with RGBA transparent background (`alpha = 0.0`).
  - Composited cleanly over active video or static wallpapers.

---

## 4. In-App Web & 3D Rendering Pipeline

### 4.1 Electron Preview Bundle (`src/electron_preview.rs`)
- Directory `/tmp/omywall_thumbs/electron_app/` with `package.json` and `main.js`.
- Configured with `--enable-unsafe-swiftshader`, `--allow-file-access-from-files`, `--autoplay-policy=no-user-gesture-required`.
- 500ms stabilization delay before capturing WebGL/canvas pixels.

### 4.2 Secondary Fallback: Chromium Headless Snapshot
- `chromium --headless --disable-gpu --allow-file-access-from-files --virtual-time-budget=2000 --window-size=600,337 --screenshot=<out> <url>`.

### 4.3 Interactive Standalone Preview
- Launches interactive Electron / Chromium window allowing user interaction with WebGL/3D scenes and desktop widgets.

---

## 5. UI/UX Layout & Navigation

### 5.1 App Navigation Tabs
1. 🖼 **Installed Wallpapers**: Library, WebGL assets, videos, images, and saved URLs.
2. 🌐 **Steam Workshop Browser**: Query search, trend/popular sort, SteamCMD download tracking, direct apply.
3. 🎛 **Desktop Widgets**: Toggle desktop overlay widgets, preset selector (Cyber HUD, WiFi/Bluetooth Pill, Minimal Clock), position config, live widget preview.
4. 📺 **Displays Manager**: Monitor cards, resolution/refresh badges, per-screen wallpaper assignment.
5. 🔒 **Screensaver**: Hyprlock settings, mode selector, clock accent colors, live test runner.
6. ⚙ **Settings**: Hardware acceleration, audio volume/mute, opacity, autostart, slideshow, daemon controls.

### 5.2 Modals & Diagnostics
- **System Doctor Modal (`show_doctor`)**:
  - Diagnostic table for `mpv`, `ffmpeg`, `electron`, `chromium`, `webkit2gtk`, `hyprctl`, `steamcmd`, `nmcli`, `bluetoothctl`, and GPU drivers.
  - Status badges: Green `● OK` / Red `▲ Missing`.
  - Action button: `🛠 Run Auto-Fix / Install Missing Tools` (`./install.sh`).
- **Live Logs Modal (`show_logs`)**:
  - Monospace log viewer reading `~/.local/state/omywall/omywall.log` with refresh, clear, and clipboard copy.

---

## 6. Verification & Testing Plan
1. **Telemetry & Widgets Verification**:
   - Verify telemetry poller generates `/tmp/omywall_telemetry.json` with valid WiFi, Bluetooth, and battery data.
   - Verify `assets/widgets/desktop_hud.html` renders and updates dynamically.
   - Verify transparent layer-shell overlay spawns over active wallpapers.
2. **Web Rendering Validation**:
   - Verify thumbnails for local WebGL scenes and remote URLs.
   - Verify 5 FPS spotlight preview stream in Carousel and Card hover.
3. **UI/UX & Tab Testing**:
   - Verify instant search filtering, Steam Workshop browser, System Doctor modal, Displays manager, Screensaver preview, and Settings controls.
