# Omywall UI/UX Overhaul & In-App Web Rendering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Overhaul the Omywall GUI and media rendering engine to achieve flawless in-app WebGL/website rendering, wire up all tabs and modals (Steam Workshop browser, System Doctor, Live Logs, Display Manager, Screensaver, Settings), and establish a polished dark-glass UI/UX.

**Architecture:** A multi-tier headless web capture system (Electron app bundle + Chromium headless CLI fallback + WebKit2GTK) feeding an asynchronous Iced image cache and 5 FPS live spotlight preview stream, combined with a unified Iced 0.14 component hierarchy with zero unrendered modals or dead buttons.

**Tech Stack:** Rust, Iced 0.14, GTK3 / WebKit2GTK, Electron v43 / Chromium headless, MPV, Tokio, FFmpeg.

## Global Constraints
- Target Platform: Linux Wayland (Hyprland, Sway) & X11.
- No dummy/unimplemented placeholder modals or dead buttons.
- Web rendering must produce valid PNG snapshots for both local HTML assets and remote HTTP/HTTPS URLs.
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
use std::time::Duration;

#[test]
fn test_electron_app_bundle_creation() {
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

Run: `cargo test --test web_render_tests` and test snapshot on `assets/web_wallpapers/neon_oled_fluid_mouse_3d.html`.
Expected: Snapshot file created with size > 1000 bytes.

- [ ] **Step 5: Commit**

```bash
git add src/electron_preview.rs tests/web_render_tests.rs
git commit -m "feat: implement multi-tier web capture with electron bundle and chromium fallback"
```

---

### Task 2: Catalog Synchronization for Web Wallpapers & Asset Bookmarks

**Files:**
- Modify: `src/config.rs`
- Modify: `src/iced_gui.rs`

**Interfaces:**
- Consumes: `config.saved_web_wallpapers: Vec<WebBookmark>`
- Produces: `IcedGuiApp::sync_catalog(&mut self)` to ensure built-in WebGL presets and custom bookmarks are always in `app.wallpapers`.

- [ ] **Step 1: Write test for catalog web bookmark synchronization**

In `tests/web_render_tests.rs`:
```rust
#[test]
fn test_web_bookmarks_present_in_catalog() {
    let cfg = omywall::config::Config::default();
    assert!(!cfg.saved_web_wallpapers.is_empty(), "Default web bookmarks must not be empty");
}
```

- [ ] **Step 2: Run test to verify**

Run: `cargo test --test web_render_tests`
Expected: PASS

- [ ] **Step 3: Update `src/iced_gui.rs` to merge web bookmarks into wallpaper inventory**

In `src/iced_gui.rs`:
- Update `IcedGuiApp::new()` and `scan_wallpapers()`:
  - Iterate `config.saved_web_wallpapers` and insert bookmark URLs/paths into `wallpapers` list if not already present.
- Update `SaveWebBookmark` message handler to append bookmark, trigger `render_shot`, and refresh catalog.
- Update `RemoveWebBookmark` message handler to remove bookmark from both `config.saved_web_wallpapers` and `app.wallpapers`, then save config.

- [ ] **Step 4: Run `cargo check` and verify catalog loads web bookmarks**

Run: `cargo check`
Expected: 0 errors.

- [ ] **Step 5: Commit**

```bash
git add src/config.rs src/iced_gui.rs
git commit -m "feat: synchronize web bookmarks and WebGL assets into GUI catalog"
```

---

### Task 3: Installed Wallpapers UI Overhaul (Search, Category Pills, Grid vs Carousel, Media Badges)

**Files:**
- Modify: `src/iced_gui.rs`

**Interfaces:**
- Consumes: `app.search_filter`, `app.category_filter`, `app.view_mode`
- Produces: `render_wallpaper_card`, `render_carousel_view`, `filtered_wallpapers`

- [ ] **Step 1: Implement live search filtering and media category pills**

In `src/iced_gui.rs`:
- Hook up `text_input("Search wallpapers...", &app.search_filter).on_input(Message::SearchFilterChanged)` in `top_bar`.
- Update `filtered_wallpapers` to filter by both `category_filter` and `search_filter` (case-insensitive substring match on file name or URL).
- Add `CategoryFilter::SteamWorkshop` handling to filter Steam workshop items.

- [ ] **Step 2: Enhance Grid Card and Carousel Spotlight Player**

In `src/iced_gui.rs`:
- Add media type badge in card top-right: `🌐 WebGL`, `🎥 Video`, `🖼 Image`, `🎮 Steam`.
- Add inline **Remove Bookmark** button (🗑) on custom web cards.
- Add double-click to apply wallpaper support.
- In `render_carousel_view`, wire up interactive web preview button `👁 Preview Web (Electron/Browser)` that launches interactive standalone browser window.

- [ ] **Step 3: Verify GUI compiles and tests pass**

Run: `cargo check`
Expected: 0 errors, warnings reduced.

- [ ] **Step 4: Commit**

```bash
git add src/iced_gui.rs
git commit -m "feat: overhaul installed wallpapers view with search filter, media badges, and grid/carousel enhancements"
```

---

### Task 4: Complete Steam Workshop Browser Tab

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

### Task 5: System Doctor & Live Logs Diagnostic Modals

**Files:**
- Modify: `src/iced_gui.rs`

**Interfaces:**
- Consumes: `app.show_doctor`, `app.show_logs`, `check_installed_tools()`, `app.logs_content`
- Produces: Rendered modal dialog overlays on top of the main GUI view.

- [ ] **Step 1: Implement System Doctor diagnostic modal**

In `src/iced_gui.rs`:
- When `app.show_doctor == true`, render a centered dark glass modal overlay:
  - Title: **⚙ System Doctor & Dependency Diagnostics**
  - Table of tools (`mpv`, `ffmpeg`, `electron`, `chromium`, `webkit2gtk`, `hyprctl`/`swaymsg`, `steamcmd`, GPU drivers).
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

### Task 6: Display Manager, Screensaver & Settings Tabs Polish

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

### Task 7: End-to-End System Verification & Web Rendering Test

**Files:**
- Test: Full integration test script & cargo test suite

- [ ] **Step 1: Run all unit & integration tests**

Run: `cargo test`
Expected: All tests PASS.

- [ ] **Step 2: Verify in-app web thumbnail and spotlight stream generation**

Run: Verify generation of WebGL thumbnails in `/tmp/omywall_thumbs/` and live hover stream file.

- [ ] **Step 3: Verify clean build of the release binary**

Run: `cargo build --release`
Expected: Release binary successfully built at `target/release/omywall`.

- [ ] **Step 4: Final commit & documentation update**

```bash
git add .
git commit -m "feat: complete UI/UX overhaul and flawless in-app web rendering"
```
