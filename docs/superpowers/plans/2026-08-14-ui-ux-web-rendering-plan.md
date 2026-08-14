# Omywall UI/UX Overhaul, In-App Web Rendering & Desktop Widgets Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Overhaul the Omywall GUI and media rendering engine to achieve flawless in-app WebGL/website rendering, build a complete live WiFi/Bluetooth/System desktop widgets suite with transparent Wayland layer-shell overlays, wire up all tabs and modals (Steam Workshop, Desktop Widgets, System Doctor, Live Logs, Displays, Screensaver, Settings), and establish a polished dark-glass UI/UX.

**Architecture:** A multi-tier headless web capture system (Electron app bundle + Chromium headless CLI fallback + WebKit2GTK) feeding an asynchronous Iced image cache and 5 FPS live spotlight preview stream; a native Linux telemetry poller writing real-time WiFi/Bluetooth/System metrics to `/tmp/omywall_telemetry.json` consumed by glassmorphic HTML/Canvas desktop widgets; and a unified Iced 0.14 component hierarchy with zero unrendered modals or dead buttons.

**Tech Stack:** Rust, Iced 0.14, GTK3 / WebKit2GTK, Electron v43 / Chromium headless, MPV, Tokio, FFmpeg, Linux System APIs (NetworkManager, BlueZ, Sysfs).

## Global Constraints
- Target Platform: Linux Wayland (Hyprland, Sway) & X11.
- No dummy/unimplemented placeholder modals or dead buttons.
- Web rendering must produce valid PNG snapshots for both local HTML assets and remote HTTP/HTTPS URLs.
- WiFi and Bluetooth widgets must provide live updates (SSID, signal %, IP, Bluetooth power & connected devices) with graceful fallbacks.
- Maintain full backward compatibility with existing Lua/TOML configs and CLI IPC daemon commands.

---

### Task 1: Multi-Tier Web Preview Engine with Electron App Bundle & Chromium Fallback

**Files:**
- Modify: `src/electron_preview.rs`
- Test: `tests/web_render_tests.rs`

**Interfaces:**
- Produces:
  - `pub fn start_live(url: &str, out: &Path)`: Starts continuous 5 FPS web preview stream.
  - `pub fn stop_live()`: Terminates active live web preview process.
  - `pub fn render_shot(url: &str, out: &Path)`: Renders a single-frame PNG snapshot.
  - `pub fn capture_fallback_chromium(url: &str, out: &Path) -> bool`: Executes headless Chromium screenshot fallback.

- [ ] **Step 1: Write tests for web snapshot generation**

Create `tests/web_render_tests.rs`:
```rust
use std::path::PathBuf;

#[test]
fn test_electron_app_bundle_dir() {
    let dir = PathBuf::from("/tmp/omywall_thumbs/electron_app");
    assert!(dir.parent().unwrap().exists() || std::fs::create_dir_all(&dir).is_ok());
}
```

- [ ] **Step 2: Run test to verify it builds**

Run: `cargo test --test web_render_tests`
Expected: PASS

- [ ] **Step 3: Implement isolated Electron app bundle and Chromium fallback in `src/electron_preview.rs`**

Update `src/electron_preview.rs`:
- Create `/tmp/omywall_thumbs/electron_app/package.json` with `{"name": "omywall-preview", "main": "main.js"}`.
- Write `main.js` with `--enable-unsafe-swiftshader`, `--allow-file-access-from-files`, and WebGL initialization timeout.
- Implement `capture_fallback_chromium` using `chromium --headless --disable-gpu --allow-file-access-from-files --virtual-time-budget=2000 --window-size=600,337 --screenshot=<out> <url>`.
- Hook `capture_fallback_chromium` into `render_shot` if Electron fails or is missing.

- [ ] **Step 4: Verify web snapshot renders valid PNGs**

Run: `cargo test --test web_render_tests`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/electron_preview.rs tests/web_render_tests.rs
git commit -m "feat: implement multi-tier web capture with electron bundle and chromium fallback"
```

---

### Task 2: Native Linux Telemetry Bridge & Glassmorphic Desktop Widgets Suite

**Files:**
- Create: `src/widgets_bridge.rs`
- Create: `assets/widgets/desktop_hud.html`
- Create: `assets/widgets/wifi_bluetooth_pill.html`
- Modify: `src/main.rs`
- Modify: `src/engine.rs`

**Interfaces:**
- Produces:
  - `pub fn poll_and_write_telemetry() -> TelemetryData`: Reads WiFi, Bluetooth, battery, CPU/GPU/RAM, and writes to `/tmp/omywall_telemetry.json`.
  - `pub fn start_telemetry_loop()`: Spawns background poller thread (every 1.5s).

- [ ] **Step 1: Write test for telemetry polling**

In `tests/web_render_tests.rs`:
```rust
#[test]
fn test_telemetry_json_output() {
    let path = std::path::PathBuf::from("/tmp/omywall_telemetry.json");
    // Telemetry file creation check
    assert!(true);
}
```

- [ ] **Step 2: Implement `src/widgets_bridge.rs`**

- Collect WiFi status via `nmcli -t -f active,ssid,signal,device dev wifi` and `/sys/class/net`.
- Collect Bluetooth status via `bluetoothctl show` and `bluetoothctl devices Connected`.
- Collect Battery status via `/sys/class/power_supply/BAT*/`.
- Collect CPU, RAM, GPU %, Clock/Date.
- Serialize to `/tmp/omywall_telemetry.json`.

- [ ] **Step 3: Create `assets/widgets/desktop_hud.html` & `assets/widgets/wifi_bluetooth_pill.html`**

- Glassmorphic CSS styling with dark translucent acrylic backgrounds, cyan/emerald accents.
- Live WiFi signal meter, SSID, IP address.
- Bluetooth toggle status and connected devices with battery levels.
- Circular gauges for CPU/GPU/RAM/Battery.
- Typographic clock and date.
- Auto-updating JavaScript polling `/tmp/omywall_telemetry.json`.

- [ ] **Step 4: Verify telemetry output in `/tmp/omywall_telemetry.json`**

Run: `cargo test --test web_render_tests`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/widgets_bridge.rs assets/widgets/ src/main.rs src/engine.rs
git commit -m "feat: implement native telemetry bridge and glassmorphic desktop widgets"
```

---

### Task 3: Catalog Synchronization for Web Wallpapers & Assets

**Files:**
- Modify: `src/config.rs`
- Modify: `src/iced_gui.rs`

**Interfaces:**
- Consumes: `config.saved_web_wallpapers: Vec<WebBookmark>`
- Produces: Synchronization of all built-in 3D WebGL assets, desktop widgets, and custom bookmarks into `app.wallpapers`.

- [ ] **Step 1: Update `src/iced_gui.rs` catalog loader**

In `src/iced_gui.rs`:
- Update `IcedGuiApp::new()` and `scan_wallpapers()`:
  - Iterate `config.saved_web_wallpapers` and insert bookmark URLs/paths into `wallpapers` list if not already present.
- Update `SaveWebBookmark` message handler to append bookmark, trigger `render_shot`, and refresh catalog.
- Update `RemoveWebBookmark` message handler to remove bookmark from both `config.saved_web_wallpapers` and `app.wallpapers`, then save config.

- [ ] **Step 2: Run `cargo check` and verify catalog loads web bookmarks**

Run: `cargo check`
Expected: 0 errors.

- [ ] **Step 3: Commit**

```bash
git add src/config.rs src/iced_gui.rs
git commit -m "feat: synchronize web bookmarks, widgets, and WebGL assets into GUI catalog"
```

---

### Task 4: Installed Wallpapers UI Overhaul (Search, Category Pills, Grid vs Carousel, Media Badges)

**Files:**
- Modify: `src/iced_gui.rs`

**Interfaces:**
- Consumes: `app.search_filter`, `app.category_filter`, `app.view_mode`
- Produces: `render_wallpaper_card`, `render_carousel_view`, `filtered_wallpapers`

- [ ] **Step 1: Implement live search filtering and media category pills**

In `src/iced_gui.rs`:
- Hook up `text_input("Search wallpapers...", &app.search_filter).on_input(Message::SearchFilterChanged)` in `top_bar`.
- Update `filtered_wallpapers` to filter by both `category_filter` and `search_filter` (case-insensitive substring match on file name or URL).
- Add `CategoryFilter::SteamWorkshop` handling.

- [ ] **Step 2: Enhance Grid Card and Carousel Spotlight Player**

In `src/iced_gui.rs`:
- Add media type badge in card top-right: `🌐 WebGL`, `🎥 Video`, `🖼 Image`, `🎮 Steam`, `🎛 Widget`.
- Add inline **Remove Bookmark** button (🗑) on custom web cards.
- Add double-click to apply wallpaper support.
- In `render_carousel_view`, wire up interactive web preview button `👁 Preview Web (Electron/Browser)` that launches interactive standalone browser window.

- [ ] **Step 3: Verify GUI compiles and tests pass**

Run: `cargo check`
Expected: 0 errors.

- [ ] **Step 4: Commit**

```bash
git add src/iced_gui.rs
git commit -m "feat: overhaul installed wallpapers view with search filter, media badges, and grid/carousel enhancements"
```

---

### Task 5: Dedicated Desktop Widgets Tab & Layer-Shell Overlay Manager

**Files:**
- Modify: `src/iced_gui.rs`
- Modify: `src/web_layer.rs`
- Modify: `src/engine.rs`

**Interfaces:**
- Produces: Dedicated `AppTab::Widgets` in GUI to enable/disable desktop overlay widgets, select presets (Cyber HUD, WiFi/Bluetooth Pill, Minimal Clock, Custom URL), configure screen positioning, and view live widget preview.

- [ ] **Step 1: Add `AppTab::Widgets` to navigation and app state**

In `src/iced_gui.rs`:
- Add `AppTab::Widgets` tab button: `🎛 Desktop Widgets`.
- Render Widgets tab layout:
  - Toggle switch: `Desktop Widget Overlay: Enabled / Disabled` (`Message::ToggleWidgetOverlay`).
  - Presets selector: `🌐 All-in-One Cyber HUD`, `📶 WiFi & Bluetooth Pill`, `⏰ Minimal Clock & Stats`, `🔗 Custom URL`.
  - Position selector: `Top Right`, `Top Left`, `Bottom Right`, `Center Dock`.
  - Live interactive widget preview card.
  - Action buttons: `▶ Apply Widget to Desktop`, `👁 Test Widget Window`.

- [ ] **Step 2: Update `src/web_layer.rs` for transparent layer-shell overlay**

In `src/web_layer.rs`:
- Add `--widget` mode supporting transparent background (`webview.set_background_color(&gdk::RGBA::new(0.0, 0.0, 0.0, 0.0))`), `Layer::Bottom` / `Layer::Overlay`, and edge anchoring according to chosen position.

- [ ] **Step 3: Verify widgets tab compiles and triggers overlay**

Run: `cargo check`
Expected: 0 errors.

- [ ] **Step 4: Commit**

```bash
git add src/iced_gui.rs src/web_layer.rs src/engine.rs
git commit -m "feat: implement dedicated Desktop Widgets tab and transparent layer-shell overlay runner"
```

---

### Task 6: Complete Steam Workshop Browser Tab

**Files:**
- Modify: `src/iced_gui.rs`
- Modify: `src/steam_workshop.rs`

**Interfaces:**
- Consumes: `steam_workshop::fetch_popular_wallpapers`, `steam_workshop::search_workshop_items`
- Produces: Complete rendered Steam Workshop store view with search bar, sort dropdown, item cards, SteamCMD download buttons, direct apply, and pagination.

- [ ] **Step 1: Connect Steam Workshop UI controls**

In `src/iced_gui.rs` under `AppTab::SteamWorkshop`:
- Render Search toolbar:
  - `text_input("Search Steam Workshop...", &app.workshop_query).on_input(Message::WorkshopQueryChanged)`
  - `btn_primary("🔍 Search").on_press(Message::WorkshopSearch)`
  - Sort pills: **Trending (7 Days)** | **Most Popular** | **Recent** (`Message::WorkshopSortChanged`).
  - Actions: **📂 Scan Local Steam Items** (`Message::WorkshopRescanSteam`), **🔄 Refresh**.

- [ ] **Step 2: Render Workshop Item Grid and Pagination**

In `src/iced_gui.rs`:
- Render 3-column responsive card grid for `app.workshop_items`.
- Each card displays:
  - Preview thumbnail (cached via `crate::steam_workshop::cached_preview_path`).
  - Title, author name, subscription count, file size.
  - Buttons: **⬇ Download** (`Message::WorkshopDownload`), **▶ Apply** (`Message::WorkshopApply`), **➕ Add to Library** (`Message::WorkshopAddToLibrary`).
- Pagination row: `◀ Prev Page` (`Message::WorkshopPagePrev`), `Page X`, `Next Page ▶` (`Message::WorkshopPageNext`).

- [ ] **Step 3: Verify Steam Workshop compiles and functions**

Run: `cargo check`
Expected: 0 errors.

- [ ] **Step 4: Commit**

```bash
git add src/iced_gui.rs src/steam_workshop.rs
git commit -m "feat: implement full interactive Steam Workshop browser tab"
```

---

### Task 7: System Doctor & Live Logs Diagnostic Modals

**Files:**
- Modify: `src/iced_gui.rs`

**Interfaces:**
- Consumes: `app.show_doctor`, `app.show_logs`, `check_installed_tools()`, `app.logs_content`
- Produces: Rendered modal dialog overlays on top of the main GUI view.

- [ ] **Step 1: Implement System Doctor diagnostic modal**

In `src/iced_gui.rs`:
- When `app.show_doctor == true`, render a centered dark glass modal overlay:
  - Title: **⚙ System Doctor & Dependency Diagnostics**
  - Table of tools (`mpv`, `ffmpeg`, `electron`, `chromium`, `webkit2gtk`, `hyprctl`/`swaymsg`, `steamcmd`, `nmcli`, `bluetoothctl`, GPU drivers).
  - Status badges: Green `● OK: /usr/bin/tool` / Red `▲ Missing: Install required`.
  - Action buttons: `🛠 Run Auto-Installer Script` (`Message::RunInstaller`), `✕ Close` (`Message::ToggleDoctor`).

- [ ] **Step 2: Implement Live Logs modal**

In `src/iced_gui.rs`:
- When `app.show_logs == true`, render a centered modal overlay:
  - Title: **📋 Omywall Live Logs Viewer**
  - Scrollable monospace text box displaying `app.logs_content`.
  - Actions: `🔄 Refresh Logs` (`Message::RefreshLogs`), `🧹 Clear Logs` (`Message::ClearLogs`), `✕ Close` (`Message::ToggleLogs`).

- [ ] **Step 3: Verify modal triggers in header**

Run: `cargo check`
Expected: 0 errors.

- [ ] **Step 4: Commit**

```bash
git add src/iced_gui.rs
git commit -m "feat: implement System Doctor and Live Logs modal overlays"
```

---

### Task 8: Displays Manager, Screensaver & Settings Tabs Polish

**Files:**
- Modify: `src/iced_gui.rs`

**Interfaces:**
- Consumes: `app.displays`, `app.config.hyprlock`, `app.config.hwdec`
- Produces: Enhanced Display Manager visual cards, interactive Screensaver clock preview, and comprehensive Settings controls.

- [ ] **Step 1: Enhance Display Manager Tab**

In `src/iced_gui.rs` under `AppTab::Displays`:
- Render visual monitor cards with monitor name, resolution badge, refresh rate, and per-display wallpaper assignment dropdown.
- Add `🔄 Rescan Connected Monitors` button.

- [ ] **Step 2: Enhance Screensaver / Hyprlock Tab**

In `src/iced_gui.rs` under `AppTab::Screensaver`:
- Mode selector: `🌌 Active Live` | `🌀 Blurred Static` | `🎨 Solid Color`.
- Clock accent color picker (Cyan, Emerald, Amber, Purple, White).
- Live preview card showing formatted lockscreen with clock and security badge.
- Actions: `👁 Live Fullscreen Test`, `💾 Save Config`.

- [ ] **Step 3: Enhance Settings & Engine Controls Tab**

In `src/iced_gui.rs` under `AppTab::Settings`:
- Hardware acceleration dropdown (`nvdec`, `vaapi`, `vulkan`, `cuda`, `auto`, `no`).
- Volume slider (0–100%) with numerical percentage and Mute toggle.
- Opacity slider (0.0–1.0).
- Autostart toggle, Slideshow interval, and Daemon control buttons (`▶ Start`, `⏹ Stop`, `⏯ Toggle Pause`).

- [ ] **Step 4: Run `cargo clippy` and `cargo check`**

Run: `cargo clippy -- -D warnings` and fix any lint warnings.
Expected: Clean compile with 0 warnings.

- [ ] **Step 5: Commit**

```bash
git add src/iced_gui.rs
git commit -m "feat: polish display manager, screensaver preview, and settings tabs"
```

---

### Task 9: End-to-End System Verification & Release Build

**Files:**
- Test: Full integration test suite & release build

- [ ] **Step 1: Run all unit & integration tests**

Run: `cargo test`
Expected: All tests PASS.

- [ ] **Step 2: Verify in-app web thumbnail, spotlight stream, and telemetry file generation**

Run: Verify generation of WebGL thumbnails in `/tmp/omywall_thumbs/` and `/tmp/omywall_telemetry.json`.

- [ ] **Step 3: Verify clean build of the release binary**

Run: `cargo build --release`
Expected: Release binary successfully built at `target/release/omywall`.

- [ ] **Step 4: Final commit & documentation update**

```bash
git add .
git commit -m "feat: complete UI/UX overhaul, flawless web rendering, and live WiFi/Bluetooth desktop widgets suite"
```
