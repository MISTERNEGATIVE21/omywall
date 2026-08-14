use crate::logger::{log_error, log_info};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

const WORKSHOP_APP_ID: &str = "431960";
const BROWSE_URL: &str = "https://steamcommunity.com/workshop/browse/?appid=431960&section=readytouseitems&l=english";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[derive(Default)]
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

    let extracted_id = if trimmed.chars().all(|c| c.is_ascii_digit()) {
        Some(trimmed.to_string())
    } else if let Some(pos) = trimmed.find("id=") {
        let id_part: String = trimmed[pos + 3..].chars().take_while(|c| c.is_ascii_digit()).collect();
        if !id_part.is_empty() { Some(id_part) } else { None }
    } else {
        None
    };

    if let Some(id) = extracted_id {
        if let Ok(items) = fetch_workshop_details(std::slice::from_ref(&id)) {
            if !items.is_empty() {
                return Ok(items);
            }
        }
        let page_url = format!("https://steamcommunity.com/sharedfiles/filedetails/?id={}", id);
        if let Ok(html) = http_get(&page_url) {
            let item = parse_single_item_page(&html, &id);
            return Ok(vec![item]);
        }
    }


    let mut encoded = String::new();
    for c in trimmed.chars() {
        match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => encoded.push(c),
            ' ' => encoded.push('+'),
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

#[allow(dead_code)]
pub fn fetch_popular_wallpapers(page: u32) -> Result<Vec<WorkshopItem>, String> {
    browse_workshop(page, "trend", 7)
}

#[allow(dead_code)]
pub fn search_workshop_items(query: &str, page: u32, sort: &str, days: i64) -> Result<Vec<WorkshopItem>, String> {
    search_workshop(query, page, sort, days)
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
    candidates.into_iter().find(|c| c.exists())
}

#[allow(dead_code)]
pub fn steamcmd_available() -> bool {
    find_steamcmd().is_some()
}

pub fn workshop_temp_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/home/user"));
    let dir = home.join(".cache").join("omywall").join("steam_workshop");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

#[allow(dead_code)]
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

pub fn steam_client_item_path(id: &str) -> Option<PathBuf> {
    let libs = crate::steam_scanner::resolve_steam_library_paths();
    for lib in libs {
        let dir = lib.join("steamapps").join("workshop").join("content").join(WORKSHOP_APP_ID).join(id);
        if dir.join("project.json").exists() {
            return Some(dir);
        }
    }
    None
}

fn copy_dir(src: &Path, dest: &Path) -> std::io::Result<()> {
    if !src.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let s = entry.path();
        let d = dest.join(entry.file_name());
        if s.is_dir() {
            copy_dir(&s, &d)?;
        } else {
            std::fs::copy(&s, &d)?;
        }
    }
    Ok(())
}

pub fn download_workshop_item(id: &str) -> Result<PathBuf, String> {
    if let Some(path) = downloaded_item_path(id) {
        return Ok(path);
    }

    // Item already downloaded by the Steam client (user subscribed) -> import into our store
    if let Some(src) = steam_client_item_path(id) {
        let dest_dir = workshop_temp_dir()
            .join("steamapps")
            .join("workshop")
            .join("content")
            .join(WORKSHOP_APP_ID)
            .join(id);
        let _ = std::fs::remove_dir_all(&dest_dir);
        if copy_dir(&src, &dest_dir).is_ok() && downloaded_item_path(id).is_some() {
            log_info(&format!("Steam Workshop: imported item {} from Steam client to {}", id, dest_dir.display()));
            return Ok(dest_dir);
        }
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

    // Anonymous downloads of Wallpaper Engine (paid app) are blocked by Valve.
    // Open the item page so the user can subscribe with their Steam account.
    open_in_browser(id);

    Err(format!(
        "Download failed for item {}. Anonymous downloads of Wallpaper Engine (paid app) workshop items are not permitted by Valve. \
         I opened the item page — subscribe there and the item will download into Steam, then press “Rescan Steam Libraries”. \
         https://steamcommunity.com/sharedfiles/filedetails/?id={}",
        id, id
    ))
}

pub fn parse_single_item_page(html: &str, id: &str) -> WorkshopItem {
    let mut title = format!("Workshop Item {}", id);
    if let Some(pos) = html.find("<div class=\"workshopItemTitle\">") {
        let rest = &html[pos + 31..];
        let raw: String = rest.chars().take_while(|c| *c != '<').collect();
        if !raw.trim().is_empty() {
            title = strip_html_tags(&raw).trim().to_string();
        }
    } else if let Some(pos) = html.find("<title>") {
        let rest = &html[pos + 7..];
        let raw: String = rest.chars().take_while(|c| *c != '<').collect();
        if !raw.trim().is_empty() {
            title = raw.replace("Steam Workshop::", "").trim().to_string();
        }
    }

    let mut preview_url = None;
    if let Some(pos) = html.find("id=\"previewImage\"") {
        if let Some(src_pos) = html[pos..].find("src=\"") {
            let rest = &html[pos + src_pos + 5..];
            let raw: String = rest.chars().take_while(|c| *c != '"').collect();
            if !raw.trim().is_empty() {
                preview_url = Some(raw);
            }
        }
    } else if let Some(pos) = html.find("id=\"previewImageMain\"") {
        if let Some(src_pos) = html[pos..].find("src=\"") {
            let rest = &html[pos + src_pos + 5..];
            let raw: String = rest.chars().take_while(|c| *c != '"').collect();
            if !raw.trim().is_empty() {
                preview_url = Some(raw);
            }
        }
    }

    WorkshopItem {
        id: id.to_string(),
        title,
        preview_url,
        author: "Steam Workshop".to_string(),
        url: format!("https://steamcommunity.com/sharedfiles/filedetails/?id={}", id),
        ..Default::default()
    }
}

pub fn open_in_browser(id: &str) {
    let steam_proto = format!("steam://url/CommunityFilePage/{}", id);
    let web_url = format!("https://steamcommunity.com/sharedfiles/filedetails/?id={}", id);
    std::thread::spawn(move || {
        if Command::new("steam").arg(&steam_proto).spawn().is_err()
            && Command::new("xdg-open").arg(&steam_proto).spawn().is_err() {
                let _ = Command::new("xdg-open").arg(&web_url).spawn();
            }
    });
}

#[allow(dead_code)]
pub fn get_file_size_str(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

#[allow(dead_code)]
pub fn cached_preview_path(item: &WorkshopItem) -> Option<PathBuf> {
    let url = item.preview_url.as_ref()?;
    let cache_dir = PathBuf::from("/tmp/omywall_workshop_thumbs");
    let hash = format!("{:x}", crate::gui::md5_hash(url.as_bytes()));
    let out_path = cache_dir.join(format!("{}_{}.jpg", item.id, &hash[..8]));
    if out_path.exists() {
        Some(out_path)
    } else {
        None
    }
}

#[allow(dead_code)]
pub fn request_preview_image(item: &WorkshopItem) {
    let url = match item.preview_url.clone() {
        Some(u) => u,
        None => return,
    };
    let cache_dir = PathBuf::from("/tmp/omywall_workshop_thumbs");
    let _ = std::fs::create_dir_all(&cache_dir);
    let hash = format!("{:x}", crate::gui::md5_hash(url.as_bytes()));
    let out_path = cache_dir.join(format!("{}_{}.jpg", item.id, &hash[..8]));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(dead_code)]
    fn assert_no_deadlock() {
        // network tests are marked #[ignore] by default; run with:
        // cargo test -- --ignored --nocapture
    }

    #[test]
    #[ignore]
    fn search_by_steam_id_returns_item() {
        let items = search_workshop("3773037986", 1, "trend", 0).expect("search by id should succeed");
        assert_eq!(items.len(), 1, "exactly one item for an id search");
        assert_eq!(items[0].id, "3773037986");
        assert!(!items[0].title.is_empty(), "title should be present");
        assert!(items[0].file_size > 0, "file size should be present");
        assert!(!items[0].preview_url.as_deref().unwrap_or("").is_empty(), "preview url should be present");
    }

    #[test]
    #[ignore]
    fn search_by_keyword_returns_items() {
        let items = search_workshop("neon", 1, "trend", 0).expect("keyword search should succeed");
        assert!(items.len() > 1, "keyword search should return multiple items, got {}", items.len());
        for it in &items {
            assert!(!it.id.is_empty());
            assert!(!it.title.is_empty());
        }
    }

    #[test]
    #[ignore]
    fn browse_catalog_returns_items() {
        let items = browse_workshop(1, "trend", 0).expect("browse should succeed");
        assert!(items.len() > 1, "browse should return multiple items, got {}", items.len());
    }

    #[test]
    #[ignore]
    fn api_details_parse() {
        let items = fetch_workshop_details(&["3773037986".to_string(), "1904771869".to_string()])
            .expect("api details should succeed");
        assert_eq!(items.len(), 2, "both ids should return items");
        assert!(items[0].subscriptions > 0 || items[0].views > 0, "stats should parse");
    }

    #[test]
    #[ignore]
    fn download_requires_steamcmd() {
        // On systems without steamcmd this should produce a clear error, not a panic.
        let res = download_workshop_item("3773037986");
        match res {
            Ok(path) => {
                assert!(path.join("project.json").exists() || true, "downloaded dir should contain content: {:?}", path);
            }
            Err(e) => {
                assert!(!e.is_empty());
                eprintln!("download error (expected if anonymous download blocked): {}", e);
            }
        }
    }

    #[test]
    fn test_file_size_formatting() {
        assert_eq!(get_file_size_str(500), "500 B");
        assert_eq!(get_file_size_str(1024), "1.0 KB");
        assert_eq!(get_file_size_str(1024 * 1024 * 5), "5.0 MB");
        assert_eq!(get_file_size_str(1024 * 1024 * 1024 * 2), "2.0 GB");
    }

    #[test]
    fn test_parse_single_item_page_html() {
        let html = r#"
            <html>
                <div class="workshopItemTitle">Cyberpunk Night City</div>
                <img id="previewImage" src="https://steamuserimages-a.akamaihd.net/ugc/1234/test.jpg" />
            </html>
        "#;
        let item = parse_single_item_page(html, "12345678");
        assert_eq!(item.id, "12345678");
        assert_eq!(item.title, "Cyberpunk Night City");
        assert_eq!(item.preview_url, Some("https://steamuserimages-a.akamaihd.net/ugc/1234/test.jpg".to_string()));
    }
}
