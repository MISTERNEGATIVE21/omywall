use crate::logger::{log_error, log_info};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;

const WORKSHOP_APP_ID: &str = "431960";
const BROWSE_URL: &str = "https://steamcommunity.com/workshop/browse/?appid=431960&section=readytouseitems&l=english";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkshopItem {
    pub id: String,
    pub title: String,
    pub preview_url: Option<String>,
    pub author: String,
    pub subscriptions: u64,
    pub views: u64,
    pub file_size: u64,
    pub description: String,
    pub tags: Vec<String>,
    pub hcontent_file: String,
    pub time_updated: u64,
    pub url: String,
}

impl Default for WorkshopItem {
    fn default() -> Self {
        Self {
            id: String::new(),
            title: String::new(),
            preview_url: None,
            author: String::new(),
            subscriptions: 0,
            views: 0,
            file_size: 0,
            description: String::new(),
            tags: Vec::new(),
            hcontent_file: String::new(),
            time_updated: 0,
            url: String::new(),
        }
    }
}

fn http_get(url: &str) -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36")
        .timeout(std::time::Duration::from_secs(25))
        .build()
        .map_err(|e| format!("http client error: {}", e))?;

    let resp = client.get(url).send().map_err(|e| format!("http get error: {}", e))?;
    resp.text().map_err(|e| format!("http read error: {}", e))
}

pub fn search_workshop(query: &str, page: u32, sort: &str, days: i64) -> Result<Vec<WorkshopItem>, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return browse_workshop(page, sort, days);
    }

    if trimmed.chars().all(|c| c.is_ascii_digit()) {
        return fetch_workshop_details(&[trimmed.to_string()]);
    }

    let mut encoded = String::new();
    for c in trimmed.chars() {
        match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => encoded.push(c),
            ' ' => encoded.push_str("+"),
            _ => encoded.push_str(&format!("%{:02X}", c as u32)),
        }
    }

    let mut url = format!("https://steamcommunity.com/workshop/browse/?appid=431960&section=readytouseitems&searchtext={}&p={}", encoded, page.max(1));
    let sort_key = match sort {
        "trend" => "trend",
        "top_rated" => "toprated",
        "most_subscribed" => "totaluniquesubscribers",
        "newest" => "timeupdated",
        _ => "trend",
    };
    url.push_str("&browsesort=");
    url.push_str(sort_key);
    if days > 0 {
        url.push_str("&days=");
        url.push_str(&days.to_string());
    }

    let html = http_get(&url)?;
    let mut ids = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for cap in html.match_indices("sharedfiles/filedetails/?id=") {
        let start = cap.0 + "sharedfiles/filedetails/?id=".len();
        let id: String = html[start..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if !id.is_empty() && seen.insert(id.clone()) {
            ids.push(id);
        }
    }

    if ids.is_empty() {
        return parse_browse_html(&html);
    }

    match fetch_workshop_details(&ids) {
        Ok(items) if !items.is_empty() => Ok(items),
        _ => parse_browse_html(&html),
    }
}

pub fn browse_workshop(page: u32, sort: &str, days: i64) -> Result<Vec<WorkshopItem>, String> {
    let mut url = format!("{}&p={}", BROWSE_URL, page.max(1));
    let sort_key = match sort {
        "trend" => "trend",
        "top_rated" => "toprated",
        "most_subscribed" => "totaluniquesubscribers",
        "most_viewed" => "totaluniquesubscribers",
        "newest" => "timeupdated",
        _ => "trend",
    };
    if sort != "most_viewed" {
        url.push_str("&browsesort=");
        url.push_str(sort_key);
    } else {
        url.push_str("&browsesort=totaluniquesubscribers");
    }
    if days > 0 {
        url.push_str("&days=");
        url.push_str(&days.to_string());
    }

    let html = http_get(&url)?;
    let mut ids = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for cap in html.match_indices("sharedfiles/filedetails/?id=") {
        let start = cap.0 + "sharedfiles/filedetails/?id=".len();
        let id: String = html[start..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if !id.is_empty() && seen.insert(id.clone()) {
            ids.push(id);
        }
    }

    if ids.is_empty() {
        return parse_browse_html(&html);
    }

    match fetch_workshop_details(&ids) {
        Ok(items) if !items.is_empty() => Ok(items),
        _ => parse_browse_html(&html),
    }
}

pub fn fetch_workshop_details(ids: &[String]) -> Result<Vec<WorkshopItem>, String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36")
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| format!("http client error: {}", e))?;

    let mut params = Vec::new();
    params.push(("itemcount".to_string(), ids.len().to_string()));
    for (i, id) in ids.iter().enumerate() {
        params.push((format!("publishedfileids[{}]", i), id.clone()));
    }

    let resp = client.post("https://api.steampowered.com/ISteamRemoteStorage/GetPublishedFileDetails/v1/")
        .form(&params)
        .send()
        .map_err(|e| format!("api post error: {}", e))?;

    let val: serde_json::Value = resp.json().map_err(|e| format!("json parse error: {}", e))?;

    let mut items = Vec::new();
    if let Some(details) = val.get("response").and_then(|r| r.get("publishedfiledetails")).and_then(|d| d.as_array()) {
        for d in details {
            let id = d.get("publishedfileid").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if id.is_empty() { continue; }
            let title = d.get("title").and_then(|v| v.as_str()).unwrap_or("Untitled Wallpaper").to_string();
            let preview_url = d.get("preview_url").and_then(|v| v.as_str()).map(|s| s.to_string());
            let author = d.get("creator").and_then(|v| v.as_str()).unwrap_or("Steam Author").to_string();
            let subscriptions = d.get("subscriptions").and_then(|v| v.as_u64()).unwrap_or(0);
            let views = d.get("views").and_then(|v| v.as_u64()).unwrap_or(0);
            let file_size = d.get("file_size").and_then(|v| v.as_u64()).or_else(|| d.get("file_size").and_then(|v| v.as_str()).and_then(|s| s.parse().ok())).unwrap_or(0);
            let description = d.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let tags = d.get("tags")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|t| t.get("tag").and_then(|s| s.as_str()).map(|s| s.to_string())).collect())
                .unwrap_or_default();
            let time_updated = d.get("time_updated").and_then(|v| v.as_u64()).unwrap_or(0);
            let url = format!("https://steamcommunity.com/sharedfiles/filedetails/?id={}", id);

            items.push(WorkshopItem {
                id,
                title,
                preview_url,
                author,
                subscriptions,
                views,
                file_size,
                description,
                tags,
                hcontent_file: String::new(),
                time_updated,
                url,
            });
        }
    }

    Ok(items)
}

pub fn parse_browse_html(html: &str) -> Result<Vec<WorkshopItem>, String> {
    let mut items: Vec<WorkshopItem> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for cap in html.match_indices("sharedfiles/filedetails/?id=") {
        let start = cap.0 + "sharedfiles/filedetails/?id=".len();
        let id: String = html[start..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if id.is_empty() || !seen.insert(id.clone()) {
            continue;
        }

        let id_marker = format!("id={}", id);
        let title = extract_title_after(html, &id_marker);
        let preview_url = extract_preview_url(html, &id_marker);
        let item_url = format!("https://steamcommunity.com/sharedfiles/filedetails/?id={}", id);

        items.push(WorkshopItem {
            id,
            title: title.unwrap_or_else(|| "Untitled Wallpaper".to_string()),
            preview_url,
            url: item_url,
            ..WorkshopItem::default()
        });
    }

    Ok(items)
}

fn extract_title_after(html: &str, marker: &str) -> Option<String> {
    let idx = html.find(marker)? + marker.len();
    let rest = &html[idx..];
    let after_gt = rest.find('>')? + 1;
    let end = rest[after_gt..].find("</a>")?;
    let raw = &rest[after_gt..after_gt + end];
    let clean = strip_html_tags(raw).trim().to_string();
    if clean.is_empty() {
        None
    } else {
        Some(clean)
    }
}

fn extract_preview_url(html: &str, marker: &str) -> Option<String> {
    let start = html.find(marker)?;
    let before = &html[start.saturating_sub(3000)..start];
    let img_idx = before.rfind("<img src=")?;
    let src_start = img_idx + "<img src=\"".len();
    let rest = &before[src_start..];
    let end = rest.find('"')?;
    let url = &rest[..end];
    if url.starts_with("http://") || url.starts_with("https://") {
        Some(url.to_string())
    } else {
        None
    }
}

fn strip_html_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    let mut in_entity = false;
    let mut entity = String::new();
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            '&' if !in_tag => {
                in_entity = true;
                entity.clear();
            }
            ';' if in_entity => {
                in_entity = false;
                let decoded = match entity.as_str() {
                    "amp" => "&".to_string(),
                    "lt" => "<".to_string(),
                    "gt" => ">".to_string(),
                    "quot" => "\"".to_string(),
                    "apos" => "'".to_string(),
                    "nbsp" => " ".to_string(),
                    _ => String::new(),
                };
                out.push_str(&decoded);
            }
            _ if !in_tag => {
                if in_entity {
                    entity.push(ch);
                } else {
                    out.push(ch);
                }
            }
            _ => {}
        }
    }
    out
}

pub fn find_steamcmd() -> Option<PathBuf> {
    if let Ok(out) = Command::new("which").arg("steamcmd").output() {
        if out.status.success() {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !p.is_empty() {
                return Some(PathBuf::from(p));
            }
        }
    }
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/home/user"));
    let candidates = [
        home.join(".steam").join("steamcmd").join("steamcmd.sh"),
        PathBuf::from("/usr/games/steamcmd"),
        PathBuf::from("/usr/bin/steamcmd"),
        PathBuf::from("/opt/steamcmd/steamcmd.sh"),
    ];
    for c in candidates {
        if c.exists() {
            return Some(c);
        }
    }
    None
}

pub fn steamcmd_available() -> bool {
    find_steamcmd().is_some()
}

pub fn workshop_temp_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/home/user"));
    let dir = home.join(".cache").join("omywall").join("steam_workshop");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn is_downloaded(id: &str) -> bool {
    let dir = workshop_temp_dir()
        .join("steamapps")
        .join("workshop")
        .join("content")
        .join(WORKSHOP_APP_ID)
        .join(id);
    dir.exists() && dir.join("project.json").exists()
}

pub fn downloaded_item_path(id: &str) -> Option<PathBuf> {
    let dir = workshop_temp_dir()
        .join("steamapps")
        .join("workshop")
        .join("content")
        .join(WORKSHOP_APP_ID)
        .join(id);
    if dir.join("project.json").exists() {
        Some(dir)
    } else {
        None
    }
}

pub fn download_workshop_item(id: &str) -> Result<PathBuf, String> {
    if let Some(path) = downloaded_item_path(id) {
        return Ok(path);
    }

    let steamcmd = find_steamcmd().ok_or_else(|| {
        "steamcmd is not installed. Install steamcmd (e.g. `sudo pacman -S steamcmd` or `sudo apt install steamcmd`) to download workshop items, or subscribe via the Steam app.".to_string()
    })?;

    let install_dir = workshop_temp_dir();
    let args = [
        "+force_install_dir".to_string(),
        install_dir.to_string_lossy().to_string(),
        "+login".to_string(),
        "anonymous".to_string(),
        format!("+workshop_download_item {} {}", WORKSHOP_APP_ID, id),
        "+quit".to_string(),
    ];

    log_info(&format!("Steam Workshop: downloading item {} via {}", id, steamcmd.display()));
    let out = Command::new(&steamcmd)
        .args(&args)
        .output()
        .map_err(|e| format!("Failed to run steamcmd: {}", e))?;

    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let combined = format!("{}\n{}", stdout, stderr);

    if let Some(path) = downloaded_item_path(id) {
        log_info(&format!("Steam Workshop: item {} downloaded to {}", id, path.display()));
        return Ok(path);
    }

    log_error(&format!("Steam Workshop: steamcmd download failed for {}: {}", id, combined.lines().last().unwrap_or("")));
    Err(format!(
        "Download failed for item {}. Anonymous downloads of Wallpaper Engine (paid app) workshop items are not permitted by Valve. \
         Open the item page to subscribe with your Steam account: https://steamcommunity.com/sharedfiles/filedetails/?id={}",
        id, id
    ))
}

pub fn open_in_browser(id: &str) {
    let url = format!("https://steamcommunity.com/sharedfiles/filedetails/?id={}", id);
    let _ = Command::new("xdg-open").arg(&url).spawn();
}

pub fn get_file_size_str(bytes: u64) -> String {
    if bytes > 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes > 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes > 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

pub fn cached_preview_path(item: &WorkshopItem) -> Option<PathBuf> {
    let url = item.preview_url.as_ref()?;
    let cache_dir = PathBuf::from("/tmp/omywall_workshop_thumbs");
    let hash = format!("{:x}", crate::gui::md5_hash(url.as_bytes()));
    let out_path = cache_dir.join(format!("{}_{}.jpg", &item.id, &hash[..8]));
    if out_path.exists() {
        Some(out_path)
    } else {
        None
    }
}

pub fn request_preview_image(item: &WorkshopItem) {
    let url = match item.preview_url.clone() {
        Some(u) => u,
        None => return,
    };
    let cache_dir = PathBuf::from("/tmp/omywall_workshop_thumbs");
    let _ = std::fs::create_dir_all(&cache_dir);
    let hash = format!("{:x}", crate::gui::md5_hash(url.as_bytes()));
    let out_path = cache_dir.join(format!("{}_{}.jpg", &item.id, &hash[..8]));
    if out_path.exists() {
        return;
    }
    std::thread::spawn(move || {
        if let Ok(client) = reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(20)).build() {
            if let Ok(resp) = client.get(&url).send() {
                if let Ok(bytes) = resp.bytes() {
                    if bytes.len() > 100 {
                        let _ = std::fs::write(&out_path, &bytes);
                        crate::gui::notify_thumb_updated(out_path);
                    }
                }
            }
        }
    });
}
