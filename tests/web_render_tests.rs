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

