use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DisplayInfo {
    pub id: String,
    pub name: String,
    pub resolution: String,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub refresh_rate: u32,
    pub primary: bool,
    pub connected: bool,
}

pub fn detect_displays() -> Vec<DisplayInfo> {
    // 1. Try Hyprland (hyprctl monitors -j)
    if let Ok(monitors) = detect_hyprland_displays() {
        if !monitors.is_empty() {
            return monitors;
        }
    }

    // 2. Try wlr-randr
    if let Ok(monitors) = detect_wlr_randr_displays() {
        if !monitors.is_empty() {
            return monitors;
        }
    }

    // 3. Try xrandr
    if let Ok(monitors) = detect_xrandr_displays() {
        if !monitors.is_empty() {
            return monitors;
        }
    }

    // 4. Fallback default
    vec![DisplayInfo {
        id: "eDP-1".to_string(),
        name: "eDP-1".to_string(),
        resolution: "1920x1080".to_string(),
        width: 1920,
        height: 1080,
        x: 0,
        y: 0,
        refresh_rate: 60,
        primary: true,
        connected: true,
    }]
}

fn detect_hyprland_displays() -> Result<Vec<DisplayInfo>, String> {
    let output = Command::new("hyprctl")
        .args(["monitors", "-j"])
        .output()
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        return Err("hyprctl command failed".into());
    }

    let json_str = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&json_str).map_err(|e| e.to_string())?;

    let mut displays = Vec::new();
    if let Some(arr) = json.as_array() {
        for (idx, mon) in arr.iter().enumerate() {
            let name = mon.get("name").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string();
            let width = mon.get("width").and_then(|v| v.as_u64()).unwrap_or(1920) as u32;
            let height = mon.get("height").and_then(|v| v.as_u64()).unwrap_or(1080) as u32;
            let x = mon.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let y = mon.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let refresh_rate = mon.get("refreshRate").and_then(|v| v.as_f64()).unwrap_or(60.0).round() as u32;
            let focused = mon.get("focused").and_then(|v| v.as_bool()).unwrap_or(idx == 0);

            displays.push(DisplayInfo {
                id: name.clone(),
                name: name.clone(),
                resolution: format!("{}x{}", width, height),
                width,
                height,
                x,
                y,
                refresh_rate,
                primary: focused,
                connected: true,
            });
        }
    }

    Ok(displays)
}

fn detect_wlr_randr_displays() -> Result<Vec<DisplayInfo>, String> {
    let output = Command::new("wlr-randr")
        .output()
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        return Err("wlr-randr command failed".into());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut displays = Vec::new();
    let mut current_display: Option<DisplayInfo> = None;

    for line in stdout.lines() {
        // Output line: "eDP-1 \"AU Optronics 0x243D\" (1920x1080, scale 1.00)"
        if !line.starts_with(' ') && line.contains('"') {
            if let Some(disp) = current_display.take() {
                displays.push(disp);
            }

            let parts: Vec<&str> = line.split_whitespace().collect();
            if !parts.is_empty() {
                let name = parts[0].to_string();
                let mut width = 1920;
                let mut height = 1080;

                if let Some(res_start) = line.find('(') {
                    if let Some(res_end) = line[res_start..].find(',') {
                        let res_str = &line[res_start + 1..res_start + res_end];
                        let dims: Vec<&str> = res_str.split('x').collect();
                        if dims.len() == 2 {
                            width = dims[0].parse().unwrap_or(1920);
                            height = dims[1].parse().unwrap_or(1080);
                        }
                    }
                }

                current_display = Some(DisplayInfo {
                    id: name.clone(),
                    name,
                    resolution: format!("{}x{}", width, height),
                    width,
                    height,
                    x: 0,
                    y: 0,
                    refresh_rate: 60,
                    primary: displays.is_empty(),
                    connected: true,
                });
            }
        } else if let Some(ref mut disp) = current_display {
            if line.contains("Position:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    let pos_parts: Vec<&str> = parts[1].split(',').collect();
                    if pos_parts.len() == 2 {
                        disp.x = pos_parts[0].parse().unwrap_or(0);
                        disp.y = pos_parts[1].parse().unwrap_or(0);
                    }
                }
            } else if line.contains("Hz") && line.contains("current") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                for p in parts {
                    if let Ok(hz) = p.parse::<f32>() {
                        disp.refresh_rate = hz.round() as u32;
                        break;
                    }
                }
            }
        }
    }

    if let Some(disp) = current_display {
        displays.push(disp);
    }

    Ok(displays)
}

fn detect_xrandr_displays() -> Result<Vec<DisplayInfo>, String> {
    let output = Command::new("xrandr")
        .arg("--query")
        .output()
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        return Err("xrandr command failed".into());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut displays = Vec::new();

    // Regex match line format: "eDP-1 connected primary 1920x1080+0+0 ..."
    for line in stdout.lines() {
        if line.contains(" connected ") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                let name = parts[0].to_string();
                let is_primary = line.contains("primary");

                let geom_idx = if is_primary { 3 } else { 2 };
                if geom_idx < parts.len() {
                    let geom = parts[geom_idx]; // e.g. 1920x1080+0+0
                    if let Some(res_end) = geom.find('+') {
                        let res_part = &geom[..res_end];
                        let pos_part = &geom[res_end + 1..];

                        let dims: Vec<&str> = res_part.split('x').collect();
                        let pos: Vec<&str> = pos_part.split('+').collect();

                        if dims.len() == 2 && pos.len() == 2 {
                            let width = dims[0].parse().unwrap_or(1920);
                            let height = dims[1].parse().unwrap_or(1080);
                            let x = pos[0].parse().unwrap_or(0);
                            let y = pos[1].parse().unwrap_or(0);

                            displays.push(DisplayInfo {
                                id: name.clone(),
                                name,
                                resolution: format!("{}x{}", width, height),
                                width,
                                height,
                                x,
                                y,
                                refresh_rate: 60,
                                primary: is_primary || displays.is_empty(),
                                connected: true,
                            });
                        }
                    }
                }
            }
        }
    }

    Ok(displays)
}
