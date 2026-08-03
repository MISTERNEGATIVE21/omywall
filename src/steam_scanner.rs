use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum WallpaperType {
    Scene,
    Video,
    Web,
    Application,
    Unknown,
}

impl WallpaperType {
    pub fn as_str(&self) -> &'static str {
        match self {
            WallpaperType::Scene => "scene",
            WallpaperType::Video => "video",
            WallpaperType::Web => "web",
            WallpaperType::Application => "application",
            WallpaperType::Unknown => "unknown",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "scene" => WallpaperType::Scene,
            "video" => WallpaperType::Video,
            "web" => WallpaperType::Web,
            "application" => WallpaperType::Application,
            _ => WallpaperType::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyOption {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WallpaperProperty {
    pub key: String,
    pub label: String,
    pub prop_type: String,
    pub default_value: Option<serde_json::Value>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    pub options: Vec<PropertyOption>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteamWallpaper {
    pub id: String,
    pub workshop_id: String,
    pub title: String,
    pub author: String,
    pub description: String,
    pub wallpaper_type: WallpaperType,
    pub thumbnail: Option<PathBuf>,
    pub preview_url: Option<String>,
    pub file_size: u64,
    pub date_added: u64,
    pub tags: Vec<String>,
    pub path: PathBuf,
    pub properties: Vec<WallpaperProperty>,
}

const WALLPAPER_ENGINE_APP_ID: &str = "431960";

fn get_home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/home/user"))
}

pub fn resolve_steam_library_paths() -> Vec<PathBuf> {
    let home = get_home_dir();
    let mut candidate_roots = vec![
        home.join(".steam").join("steam"),
        home.join(".local").join("share").join("Steam"),
        home.join(".var").join("app").join("com.valvesoftware.Steam").join("data").join("Steam"),
    ];

    // Scan Snap paths
    let snap_dir = home.join("snap").join("steam");
    if snap_dir.exists() {
        if let Ok(entries) = fs::read_dir(snap_dir) {
            for entry in entries.flatten() {
                let steam_path = entry.path().join(".local").join("share").join("Steam");
                if steam_path.exists() {
                    candidate_roots.push(steam_path);
                }
            }
        }
    }

    let mut libraries = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for root in candidate_roots {
        if !root.exists() {
            continue;
        }

        let canonical_root = root.canonicalize().unwrap_or(root.clone());
        if seen.insert(canonical_root.clone()) {
            libraries.push(canonical_root.clone());
        }

        let vdf_path = canonical_root.join("steamapps").join("libraryfolders.vdf");
        if vdf_path.exists() {
            if let Ok(content) = fs::read_to_string(&vdf_path) {
                for line in content.lines() {
                    if line.contains("\"path\"") {
                        let parts: Vec<&str> = line.split('"').collect();
                        if parts.len() >= 4 {
                            let path_str = parts[3].replace("\\\\", "/");
                            let lib_path = PathBuf::from(path_str);
                            if lib_path.exists() {
                                let canonical_lib = lib_path.canonicalize().unwrap_or(lib_path);
                                if seen.insert(canonical_lib.clone()) {
                                    libraries.push(canonical_lib);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    libraries
}

pub fn resolve_wallpaper_engine_assets_dir() -> Option<PathBuf> {
    let libraries = resolve_steam_library_paths();
    for lib in libraries {
        let assets_dir = lib.join("steamapps").join("common").join("wallpaper_engine").join("assets");
        if assets_dir.exists() {
            return Some(assets_dir);
        }
    }
    None
}

pub fn is_pkg_file(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }
    if path.is_file() {
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if ext.eq_ignore_ascii_case("pkg") {
                return true;
            }
        }
        if let Ok(mut f) = fs::File::open(path) {
            use std::io::Read;
            let mut buf = [0u8; 4];
            if f.read_exact(&mut buf).is_ok() && &buf == b"PKGV" {
                return true;
            }
        }
    }
    false
}

pub fn scan_steam_wallpapers() -> Vec<SteamWallpaper> {
    let libraries = resolve_steam_library_paths();
    let mut workshop_dirs = Vec::new();

    for lib in libraries {
        let workshop_path = lib.join("steamapps").join("workshop").join("content").join(WALLPAPER_ENGINE_APP_ID);
        if workshop_path.exists() {
            workshop_dirs.push(workshop_path);
        }
        let presets_path = lib.join("steamapps").join("common").join("wallpaper_engine").join("assets").join("presets");
        if presets_path.exists() {
            workshop_dirs.push(presets_path);
        }
    }

    // Also scan the temporary workshop download directory (steamcmd anonymous downloads)
    let temp_workshop = crate::steam_workshop::workshop_temp_dir()
        .join("steamapps")
        .join("workshop")
        .join("content")
        .join(WALLPAPER_ENGINE_APP_ID);
    if temp_workshop.exists() {
        workshop_dirs.push(temp_workshop);
    }

    let mut wallpapers = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    for w_dir in workshop_dirs {
        let entries = match fs::read_dir(&w_dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let item_path = entry.path();
            let item_id = item_path.file_name().unwrap_or_default().to_string_lossy().to_string();
            if seen_ids.contains(&item_id) {
                continue;
            }

            let project_file = item_path.join("project.json");
            if project_file.exists() {
                if let Ok(content) = fs::read_to_string(&project_file) {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                        let title = json.get("title").and_then(|v| v.as_str()).unwrap_or("Untitled").to_string();
                        let author = json.get("author").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string();
                        let description = json.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let raw_type = json.get("type").and_then(|v| v.as_str()).unwrap_or("scene");
                        let wallpaper_type = WallpaperType::parse(raw_type);

                        let tags = json.get("tags")
                            .and_then(|v| v.as_array())
                            .map(|arr| arr.iter().filter_map(|t| t.as_str().map(|s| s.to_string())).collect())
                            .unwrap_or_default();

                        let thumbnail = resolve_thumbnail(&item_path, &json);

                        let file_size = get_dir_size(&item_path).unwrap_or(0);
                        let date_added = fs::metadata(&item_path)
                            .and_then(|m| m.modified())
                            .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs())
                            .unwrap_or(0);

                        let properties = parse_properties(&json);

                        seen_ids.insert(item_id.clone());
                        wallpapers.push(SteamWallpaper {
                            id: item_id.clone(),
                            workshop_id: item_id,
                            title,
                            author,
                            description,
                            wallpaper_type,
                            thumbnail,
                            preview_url: json.get("preview").and_then(|v| v.as_str()).map(|s| s.to_string()),
                            file_size,
                            date_added,
                            tags,
                            path: item_path,
                            properties,
                        });
                        continue;
                    }
                }
            }

            // Fallback for standalone PKG files or directories containing scene.pkg / *.pkg
            let is_pkg = is_pkg_file(&item_path);
            let has_inner_pkg = item_path.is_dir() && fs::read_dir(&item_path).ok().map_or(false, |entries| {
                entries.flatten().any(|e| is_pkg_file(&e.path()))
            });

            if is_pkg || has_inner_pkg {
                let title = item_path.file_stem().unwrap_or_default().to_string_lossy().to_string();
                let thumbnail = resolve_thumbnail(&item_path, &serde_json::Value::Null);
                let file_size = if item_path.is_file() { fs::metadata(&item_path).map(|m| m.len()).unwrap_or(0) } else { get_dir_size(&item_path).unwrap_or(0) };
                let date_added = fs::metadata(&item_path)
                    .and_then(|m| m.modified())
                    .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs())
                    .unwrap_or(0);

                seen_ids.insert(item_id.clone());
                wallpapers.push(SteamWallpaper {
                    id: item_id.clone(),
                    workshop_id: item_id,
                    title,
                    author: "Steam Workshop".to_string(),
                    description: "Wallpaper Engine Scene Package (.pkg)".to_string(),
                    wallpaper_type: WallpaperType::Scene,
                    thumbnail,
                    preview_url: None,
                    file_size,
                    date_added,
                    tags: vec!["Scene".to_string(), "PKG".to_string()],
                    path: item_path,
                    properties: Vec::new(),
                });
            }
        }
    }

    wallpapers.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    wallpapers
}

fn resolve_thumbnail(item_path: &Path, json: &serde_json::Value) -> Option<PathBuf> {
    if let Some(preview) = json.get("preview").and_then(|v| v.as_str()) {
        let p = item_path.join(preview);
        if p.exists() {
            return Some(p);
        }
    }

    let candidates = ["preview.jpg", "preview.png", "preview.gif", "preview.jpeg"];
    for cand in candidates {
        let p = item_path.join(cand);
        if p.exists() {
            return Some(p);
        }
    }

    None
}

fn get_dir_size(path: &Path) -> std::io::Result<u64> {
    let mut total = 0;
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let p = entry.path();
            if p.is_dir() {
                total += get_dir_size(&p).unwrap_or(0);
            } else {
                total += entry.metadata()?.len();
            }
        }
    } else {
        total = fs::metadata(path)?.len();
    }
    Ok(total)
}

fn parse_properties(json: &serde_json::Value) -> Vec<WallpaperProperty> {
    let mut properties = Vec::new();

    let props_obj = json
        .get("general")
        .and_then(|g| g.get("properties"))
        .or_else(|| json.get("properties"))
        .and_then(|p| p.as_object());

    if let Some(map) = props_obj {
        for (key, val) in map {
            let label = val.get("text").and_then(|t| t.as_str()).unwrap_or(key.as_str()).to_string();
            let prop_type = val.get("type").and_then(|t| t.as_str()).unwrap_or("text").to_string();
            let default_value = val.get("value").cloned();
            let min = val.get("min").and_then(|v| v.as_f64());
            let max = val.get("max").and_then(|v| v.as_f64());
            let step = val.get("step").and_then(|v| v.as_f64());

            let mut options = Vec::new();
            if let Some(opts_arr) = val.get("options").and_then(|o| o.as_array()) {
                for opt in opts_arr {
                    let opt_label = opt.get("label").and_then(|l| l.as_str()).unwrap_or("").to_string();
                    let opt_val = opt.get("value").and_then(|v| {
                        if v.is_string() {
                            v.as_str().map(|s| s.to_string())
                        } else {
                            Some(v.to_string())
                        }
                    }).unwrap_or_default();
                    options.push(PropertyOption { label: opt_label, value: opt_val });
                }
            }

            properties.push(WallpaperProperty {
                key: key.clone(),
                label,
                prop_type,
                default_value,
                min,
                max,
                step,
                options,
            });
        }
    }

    properties
}
