use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum PropertyType {
    Slider,
    Boolean,
    Combolist,
    Color,
    Text,
    File,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WallpaperProperty {
    pub name: String,
    pub prop_type: PropertyType,
    pub description: String,
    pub value: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    pub options: Vec<(String, String)>,
}

pub fn find_binary() -> Option<PathBuf> {
    if let Ok(out) = Command::new("which").arg("linux-wallpaperengine").output() {
        if out.status.success() {
            let p_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !p_str.is_empty() {
                return Some(PathBuf::from(p_str));
            }
        }
    }
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/home/user"));
    let candidates = [
        home.join(".local").join("bin").join("linux-wallpaperengine"),
        PathBuf::from("/usr/bin/linux-wallpaperengine"),
        PathBuf::from("/usr/local/bin/linux-wallpaperengine"),
        PathBuf::from("/bin/linux-wallpaperengine"),
    ];
    candidates.into_iter().find(|c| c.exists())
}

pub fn list_properties(wallpaper_path: &Path) -> Result<Vec<WallpaperProperty>, String> {
    let bin = find_binary().ok_or_else(|| {
        "linux-wallpaperengine binary not found. Please install linux-wallpaperengine.".to_string()
    })?;
    let out = Command::new(&bin)
        .arg("--list-properties")
        .arg(wallpaper_path)
        .output()
        .map_err(|e| format!("Failed to run linux-wallpaperengine --list-properties: {}", e))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "linux-wallpaperengine --list-properties failed ({}): {}",
            out.status,
            err.trim()
        ));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    Ok(parse_properties(&stdout))
}

fn parse_properties(output: &str) -> Vec<WallpaperProperty> {
    let mut props: Vec<WallpaperProperty> = Vec::new();
    let mut current: Option<WallpaperProperty> = None;
    let mut collecting = false;

    for raw_line in output.lines() {
        let line = raw_line.trim_end();
        let trimmed = line.trim_start_matches(['\t', ' ']);

        if let Some((name, ptype)) = parse_header(trimmed) {
            if collecting {
                if let Some(p) = current.take() {
                    props.push(p);
                }
            }
            current = Some(WallpaperProperty {
                name: name.to_string(),
                prop_type: ptype,
                description: String::new(),
                value: String::new(),
                min: None,
                max: None,
                step: None,
                options: Vec::new(),
            });
            collecting = true;
            continue;
        }

        if !collecting {
            continue;
        }

        let Some(prop) = current.as_mut() else {
            continue;
        };

        if trimmed.is_empty() {
            continue;
        }

        if let Some(v) = strip_key(trimmed, "Text")
            .or_else(|| strip_key(trimmed, "Description"))
        {
            prop.description = v.trim().to_string();
            continue;
        }
        if let Some(v) = strip_key(trimmed, "Value") {
            prop.value = v.trim().to_string();
            continue;
        }
        if let Some(v) = strip_key(trimmed, "Min").or_else(|| strip_key(trimmed, "Minimum value")) {
            prop.min = v.trim().parse().ok();
            continue;
        }
        if let Some(v) = strip_key(trimmed, "Max").or_else(|| strip_key(trimmed, "Maximum value")) {
            prop.max = v.trim().parse().ok();
            continue;
        }
        if let Some(v) = strip_key(trimmed, "Step") {
            prop.step = v.trim().parse().ok();
            continue;
        }
        if trimmed == "Values" || trimmed.starts_with("Posible values") || trimmed.starts_with("Possible values") {
            continue;
        }
        if prop.prop_type == PropertyType::Color && trimmed.starts_with("R:") {
            if let Some(v) = parse_color_line(trimmed) {
                prop.value = v;
            }
            continue;
        }
        if let Some((stored, display)) = parse_option_line(trimmed) {
            prop.options.push((display.to_string(), stored.to_string()));
        }
    }

    if collecting {
        if let Some(p) = current.take() {
            props.push(p);
        }
    }

    props
}

fn parse_header(trimmed: &str) -> Option<(&str, PropertyType)> {
    let idx = trimmed.find(" - ")?;
    let name = trimmed[..idx].trim();
    if name.is_empty() {
        return None;
    }
    let t = trimmed[idx + 3..].trim();
    let ptype = match t {
        "slider" => PropertyType::Slider,
        "boolean" => PropertyType::Boolean,
        "combo" | "combolist" => PropertyType::Combolist,
        "color" => PropertyType::Color,
        "file" => PropertyType::File,
        "text" => PropertyType::Text,
        _ => return None,
    };
    Some((name, ptype))
}

fn strip_key<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{}:", key);
    if line.starts_with(&prefix) {
        return Some(&line[prefix.len()..]);
    }
    None
}

fn parse_color_line(line: &str) -> Option<String> {
    let mut r = None;
    let mut g = None;
    let mut b = None;
    let mut a = None;
    let rest = line.strip_prefix("R:")?;
    for component in rest.split([' ', '\t']) {
        if component.is_empty() {
            continue;
        }
        if r.is_none() {
            r = component.parse::<f32>().ok();
        } else if g.is_none() {
            g = component.parse::<f32>().ok();
        } else if b.is_none() {
            b = component.parse::<f32>().ok();
        } else if a.is_none() {
            a = component.parse::<f32>().ok();
        }
    }
    let (r, g, b, a) = (r?, g?, b?, a?);
    Some(format!("{:.6}, {:.6}, {:.6}, {:.6}", r, g, b, a))
}

fn parse_option_line(line: &str) -> Option<(&str, &str)> {
    if let Some(idx) = line.find(" = ") {
        let stored = line[..idx].trim();
        let display = line[idx + 3..].trim();
        if !stored.is_empty() && !display.is_empty() {
            return Some((stored, display));
        }
    }
    if let Some(idx) = line.find(" -> ") {
        let display = line[..idx].trim();
        let stored = line[idx + 4..].trim();
        if !stored.is_empty() && !display.is_empty() {
            return Some((stored, display));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_current_main_branch_format() {
        let out = "\
barcount - slider
\tText: Bar Count
\tMin: 16
\tMax: 64
\tStep: 1
\tValue: 32

bloom - boolean
\tText: Bloom
\tValue: 0

frequency - combo
\tText: Frequency
\tValue: 32
Values:
\t\t16 = 16
\t\t32 = 32
\t\t64 = 64

schemecolor - color
\tText: Scheme Color
\tValue: 0.149020, 0.231373, 0.400000, 1.000000

label - text
\tText: Caption
\tValue: hello world
";
        let props = parse_properties(out);
        assert_eq!(props.len(), 5);

        let slider = &props[0];
        assert_eq!(slider.name, "barcount");
        assert_eq!(slider.prop_type, PropertyType::Slider);
        assert_eq!(slider.min, Some(16.0));
        assert_eq!(slider.max, Some(64.0));
        assert_eq!(slider.step, Some(1.0));
        assert_eq!(slider.value, "32");
        assert_eq!(slider.description, "Bar Count");

        assert_eq!(props[1].prop_type, PropertyType::Boolean);
        assert_eq!(props[1].value, "0");

        let combo = &props[2];
        assert_eq!(combo.prop_type, PropertyType::Combolist);
        assert_eq!(combo.value, "32");
        assert_eq!(combo.options, vec![
            ("16".to_string(), "16".to_string()),
            ("32".to_string(), "32".to_string()),
            ("64".to_string(), "64".to_string()),
        ]);

        assert_eq!(props[3].prop_type, PropertyType::Color);
        assert_eq!(props[3].value, "0.149020, 0.231373, 0.400000, 1.000000");

        assert_eq!(props[4].prop_type, PropertyType::Text);
        assert_eq!(props[4].value, "hello world");
    }

    #[test]
    fn parses_legacy_readme_format() {
        let out = "\
owl - boolean
\tDescription: Owl
\tValue: 1

rain - slider
\tDescription: Rain
\tValue: 2
\tMinimum value: 0
\tMaximum value: 4
\tStep: 0.5

schemecolor - color
\tDescription: ui_browse_properties_scheme_color
\tR: 0.14902 G: 0.23137 B: 0.4 A: 1

visualizer - combolist
\tDescription: Add Visualizer
\tValue: 2
\t\tPosible values:
\t\t16 -> 1
\t\t32 -> 2
";
        let props = parse_properties(out);
        assert_eq!(props.len(), 4);

        assert_eq!(props[0].prop_type, PropertyType::Boolean);
        assert_eq!(props[0].value, "1");

        assert_eq!(props[1].prop_type, PropertyType::Slider);
        assert_eq!(props[1].min, Some(0.0));
        assert_eq!(props[1].max, Some(4.0));
        assert_eq!(props[1].step, Some(0.5));

        assert_eq!(props[2].prop_type, PropertyType::Color);
        assert_eq!(props[2].value, "0.149020, 0.231370, 0.400000, 1.000000");

        assert_eq!(props[3].prop_type, PropertyType::Combolist);
        assert_eq!(props[3].value, "2");
        assert_eq!(props[3].options, vec![
            ("16".to_string(), "1".to_string()),
            ("32".to_string(), "2".to_string()),
        ]);
    }
}
