use std::path::PathBuf;
use std::time::{Duration, Instant};

#[test]
fn test_electron_app_bundle_dir() {
    let dir = PathBuf::from("/tmp/omywall_thumbs/electron_app");
    assert!(dir.parent().unwrap().exists() || std::fs::create_dir_all(&dir).is_ok());
}

#[test]
fn test_electron_app_bundle_files() {
    let bundle_dir = omywall::electron_preview::ensure_app_bundle();
    assert!(bundle_dir.exists(), "Electron app directory must exist");

    let pkg_file = bundle_dir.join("package.json");
    assert!(pkg_file.exists(), "package.json must exist in electron app bundle");
    let pkg_content = std::fs::read_to_string(&pkg_file).expect("read package.json");
    assert!(pkg_content.contains("\"main\": \"main.js\""), "package.json must point to main.js");

    let main_file = bundle_dir.join("main.js");
    assert!(main_file.exists(), "main.js must exist in electron app bundle");
    let main_content = std::fs::read_to_string(&main_file).expect("read main.js");
    assert!(main_content.contains("enable-unsafe-swiftshader"), "main.js must configure swiftshader");
    assert!(main_content.contains("allow-file-access-from-files"), "main.js must allow file access");
}

#[test]
fn test_capture_fallback_chromium() {
    if !omywall::electron_preview::chromium_available() {
        eprintln!("Chromium binary not found; skipping fallback execution test");
        return;
    }

    let temp_html = PathBuf::from("/tmp/omywall_test_fallback.html");
    let temp_png = PathBuf::from("/tmp/omywall_test_fallback.png");
    let _ = std::fs::remove_file(&temp_png);

    std::fs::write(&temp_html, "<!DOCTYPE html><html><body style='background:purple;'><h1 style='color:white;'>Omywall Test</h1></body></html>").expect("write test html");

    let success = omywall::electron_preview::capture_fallback_chromium(&temp_html.to_string_lossy(), &temp_png);
    assert!(success, "Chromium fallback capture must return true on valid input");
    assert!(temp_png.exists(), "Snapshot PNG file must be generated");
    let meta = std::fs::metadata(&temp_png).expect("PNG metadata");
    assert!(meta.len() > 0, "Generated PNG must not be empty");

    let _ = std::fs::remove_file(&temp_html);
    let _ = std::fs::remove_file(&temp_png);
}

#[test]
fn test_render_shot_generates_image() {
    let temp_html = PathBuf::from("/tmp/omywall_test_shot.html");
    let temp_png = PathBuf::from("/tmp/omywall_test_shot.png");
    let _ = std::fs::remove_file(&temp_png);

    std::fs::write(&temp_html, "<!DOCTYPE html><html><body style='background:teal;'><h1 style='color:yellow;'>Omywall Shot</h1></body></html>").expect("write test html");

    omywall::electron_preview::render_shot(&temp_html.to_string_lossy(), &temp_png);

    // Wait up to 10 seconds for background thread to render
    let start = Instant::now();
    let mut found = false;
    while start.elapsed() < Duration::from_secs(10) {
        if temp_png.exists() && std::fs::metadata(&temp_png).map(|m| m.len() > 0).unwrap_or(false) {
            found = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    assert!(found, "render_shot should generate a valid non-empty PNG file");

    let _ = std::fs::remove_file(&temp_html);
    let _ = std::fs::remove_file(&temp_png);
}

#[test]
fn test_telemetry_json_output() {
    let data = omywall::widgets_bridge::poll_and_write_telemetry();
    assert!(data.timestamp > 0, "Telemetry timestamp must be greater than zero");
    assert!(!data.time.time_str.is_empty(), "Time string must not be empty");
    assert!(!data.hostname.is_empty(), "Hostname must not be empty");

    let path = PathBuf::from(omywall::widgets_bridge::TELEMETRY_FILE_PATH);
    assert!(path.exists(), "Telemetry JSON file must exist at /tmp/omywall_telemetry.json");

    let content = std::fs::read_to_string(&path).expect("Read telemetry JSON file");
    assert!(!content.is_empty(), "Telemetry JSON file must not be empty");

    let parsed: omywall::widgets_bridge::TelemetryData = serde_json::from_str(&content).expect("Deserialize telemetry JSON");
    assert_eq!(parsed.timestamp, data.timestamp);
    assert_eq!(parsed.hostname, data.hostname);
}

#[test]
fn test_widget_asset_files() {
    let hud_path = PathBuf::from("assets/widgets/desktop_hud.html");
    assert!(hud_path.exists(), "assets/widgets/desktop_hud.html must exist");
    let hud_content = std::fs::read_to_string(&hud_path).expect("Read desktop_hud.html");
    assert!(hud_content.contains("omywall_telemetry.json"), "desktop_hud.html must poll omywall_telemetry.json");
    assert!(hud_content.contains("cpu-gauge-circle"), "desktop_hud.html must contain CPU gauge circle");
    assert!(hud_content.contains("wifi-badge"), "desktop_hud.html must contain WiFi status badge");

    let pill_path = PathBuf::from("assets/widgets/wifi_bluetooth_pill.html");
    assert!(pill_path.exists(), "assets/widgets/wifi_bluetooth_pill.html must exist");
    let pill_content = std::fs::read_to_string(&pill_path).expect("Read wifi_bluetooth_pill.html");
    assert!(pill_content.contains("omywall_telemetry.json"), "wifi_bluetooth_pill.html must poll omywall_telemetry.json");
    assert!(pill_content.contains("wifi-section"), "wifi_bluetooth_pill.html must contain wifi section");
    assert!(pill_content.contains("bt-section"), "wifi_bluetooth_pill.html must contain bluetooth section");
}

#[test]
fn test_catalog_synchronization_web_assets_and_widgets() {
    let config = omywall::config::Config::default();
    let scanned = omywall::iced_gui::IcedGuiApp::scan_wallpapers(&config.wallpaper_dir, &config.saved_web_wallpapers);

    assert!(!scanned.is_empty(), "Scanned wallpapers catalog must not be empty");

    // Verify widgets are discoverable in scanned catalog
    let has_desktop_hud = scanned.iter().any(|p| {
        let s = p.to_string_lossy();
        s.contains("desktop_hud.html")
    });
    assert!(has_desktop_hud, "Catalog must include desktop_hud.html widget");

    let has_wifi_pill = scanned.iter().any(|p| {
        let s = p.to_string_lossy();
        s.contains("wifi_bluetooth_pill.html")
    });
    assert!(has_wifi_pill, "Catalog must include wifi_bluetooth_pill.html widget");

    // Verify 3D WebGL assets are discoverable
    let has_webgl_asset = scanned.iter().any(|p| {
        let s = p.to_string_lossy();
        s.contains("matrix_rain.html") || s.contains("aurora_borealis_3d.html") || s.contains("cyberpunk_city_3d.html")
    });
    assert!(has_webgl_asset, "Catalog must include built-in WebGL HTML assets");
}

#[test]
fn test_add_and_remove_web_bookmark() {
    let mut config = omywall::config::Config::default();
    let initial_count = config.saved_web_wallpapers.len();

    let custom_url = "https://example.com/cyberpunk-scene";
    let custom_title = "Cyberpunk Scene Stream";
    let custom_category = "Online Streams";

    // Add bookmark
    config.add_web_bookmark(custom_title.to_string(), custom_url.to_string(), custom_category.to_string());
    assert_eq!(config.saved_web_wallpapers.len(), initial_count + 1);

    // Verify it is merged into scanned wallpapers
    let scanned = omywall::iced_gui::IcedGuiApp::scan_wallpapers(&config.wallpaper_dir, &config.saved_web_wallpapers);
    let contains_custom = scanned.iter().any(|p| p.to_string_lossy() == custom_url);
    assert!(contains_custom, "Scanned catalog must include newly added bookmark URL");

    // Remove bookmark
    config.remove_web_bookmark(custom_url);
    assert_eq!(config.saved_web_wallpapers.len(), initial_count);
    assert!(!config.saved_web_wallpapers.iter().any(|b| b.url == custom_url));
}

#[test]
fn test_resolve_asset_paths() {
    let hud_resolved = omywall::config::resolve_asset_path("assets/widgets/desktop_hud.html");
    assert!(PathBuf::from(&hud_resolved).exists(), "assets/widgets/desktop_hud.html must resolve to an existing file");

    let matrix_resolved = omywall::config::resolve_asset_path("assets/web_wallpapers/matrix_rain.html");
    assert!(PathBuf::from(&matrix_resolved).exists(), "assets/web_wallpapers/matrix_rain.html must resolve to an existing file");

    let remote_url = "https://youtube.com/live/xyz";
    let remote_resolved = omywall::config::resolve_asset_path(remote_url);
    assert_eq!(remote_resolved, remote_url, "Remote URLs should remain unchanged");
}

#[test]
fn test_default_web_bookmarks_contains_widgets_and_webgl() {
    let bookmarks = omywall::config::default_web_bookmarks();
    assert!(!bookmarks.is_empty(), "Default web bookmarks must not be empty");

    let has_hud = bookmarks.iter().any(|b| b.url.contains("desktop_hud.html"));
    assert!(has_hud, "Default web bookmarks must include desktop_hud.html widget");

    let has_pill = bookmarks.iter().any(|b| b.url.contains("wifi_bluetooth_pill.html"));
    assert!(has_pill, "Default web bookmarks must include wifi_bluetooth_pill.html widget");

    let has_particles = bookmarks.iter().any(|b| b.category.contains("Particles") || b.category.contains("WebGL") || b.category.contains("Space"));
    assert!(has_particles, "Default web bookmarks must include 3D WebGL categories");
}

#[test]
fn test_widget_presets_metadata() {
    use omywall::iced_gui::WidgetPreset;

    let hud = WidgetPreset::CyberHud;
    assert_eq!(hud.url(), "assets/widgets/desktop_hud.html");
    assert!(hud.label().contains("Cyber HUD"));
    assert!(hud.description().contains("CPU gauge"));

    let pill = WidgetPreset::WifiBluetoothPill;
    assert_eq!(pill.url(), "assets/widgets/wifi_bluetooth_pill.html");
    assert!(pill.label().contains("WiFi & Bluetooth"));
    assert!(pill.description().contains("pill"));

    let clock = WidgetPreset::MinimalClock;
    assert_eq!(clock.url(), "assets/widgets/minimal_clock_stats.html");
    assert!(clock.label().contains("Minimal Clock"));
    assert!(clock.description().contains("clock"));

    let custom = WidgetPreset::Custom;
    assert_eq!(custom.url(), "");
    assert!(custom.label().contains("Custom"));
}

#[test]
fn test_iced_gui_app_initialization_widgets_tab() {
    let mut config = omywall::config::Config::default();
    config.enable_widgets = true;
    config.widget_url = Some("assets/widgets/wifi_bluetooth_pill.html".to_string());
    config.widget_position = "bottom_right".to_string();

    let app = omywall::iced_gui::IcedGuiApp::new(config.clone(), false);
    assert_eq!(app.widget_preset, omywall::iced_gui::WidgetPreset::WifiBluetoothPill);
    assert_eq!(app.config.widget_position, "bottom_right");
    assert!(app.config.enable_widgets);
    assert!(!app.wallpapers.is_empty(), "Wallpapers must be initialized on boot");
    assert!(app.selected_wallpaper.is_some(), "Selected wallpaper must be initialized on boot");
}

#[test]
fn test_ipc_set_widget_roundtrip() {
    let req = omywall::ipc::IpcRequest::SetWidget {
        url: "assets/widgets/desktop_hud.html".to_string(),
        enabled: true,
        position: Some("center_dock".to_string()),
    };

    let serialized = serde_json::to_string(&req).expect("serialize IpcRequest::SetWidget");
    let deserialized: omywall::ipc::IpcRequest = serde_json::from_str(&serialized).expect("deserialize IpcRequest::SetWidget");

    match deserialized {
        omywall::ipc::IpcRequest::SetWidget { url, enabled, position } => {
            assert_eq!(url, "assets/widgets/desktop_hud.html");
            assert!(enabled);
            assert_eq!(position.as_deref(), Some("center_dock"));
        }
        _ => panic!("Expected SetWidget variant"),
    }
}

#[test]
fn test_web_layer_resolve_target_url() {
    let local_widget = "assets/widgets/desktop_hud.html";
    let resolved = omywall::web_layer::resolve_target_url(local_widget);
    assert!(resolved.starts_with("file://"), "Local widget path must resolve to file:// URI");
    assert!(resolved.contains("desktop_hud.html"));

    let remote_http = "https://dashboard.example.com";
    assert_eq!(omywall::web_layer::resolve_target_url(remote_http), remote_http);

    let yt_url = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
    let yt_resolved = omywall::web_layer::resolve_target_url(yt_url);
    assert!(yt_resolved.contains("embed/dQw4w9WgXcQ"));
    assert!(yt_resolved.contains("autoplay=1"));
}

#[test]
fn test_minimal_clock_stats_widget_file() {
    let path = PathBuf::from("assets/widgets/minimal_clock_stats.html");
    assert!(path.exists(), "minimal_clock_stats.html must exist");
    let content = std::fs::read_to_string(&path).expect("read minimal_clock_stats.html");
    assert!(content.contains("omywall_telemetry.json"), "minimal clock must poll telemetry");
    assert!(content.contains("clock-time"), "minimal clock must have clock-time element");
}

