use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum IpcRequest {
    SetWallpaper { path: String },
    SetUrl { url: String },
    StopWallpaper,
    Pause,
    Resume,
    TogglePause,
    Hide,
    Show,
    ToggleHide,
    Minimize,
    GetWaybarStatus,
    NextWallpaper,
    PrevWallpaper,
    SetVolume { volume: i64 },
    SetMute { mute: bool },
    SetHwdec { hwdec: String },
    SetGpuDevice { gpu_device: Option<String> },
    SetTargetFps { fps: u32 },
    SetScreen { screen_id: i64 },
    StartSlideshow { interval_secs: u64, shuffle: bool },
    StopSlideshow,
    SwitchMonitor { monitor: String },
    SetMonitorWallpaper { monitor: String, path: String },
    GetMonitorMappings,
    SetOpacity { opacity: f32 },
    SetWidget { url: String, enabled: bool },
    CycleLiveWallpaper,
    GetStatus,
    ListWallpapers,
    GetDisplays,
    QuerySteamWallpapers,
    SetSteamWallpaper {
        path: String,
        screen: Option<String>,
        overrides: Option<crate::config::WallpaperOverrides>,
    },
    QuitDaemon,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum IpcResponse {
    Ok { message: String },
    Status(DaemonStatus),
    WaybarStatus { json: String },
    MonitorMappings { mappings: HashMap<String, String> },
    WallpaperList { files: Vec<String>, current: Option<String> },
    Displays { displays: Vec<crate::display::DisplayInfo> },
    SteamWallpapers { wallpapers: Vec<crate::steam_scanner::SteamWallpaper> },
    Err { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub current_wallpaper: Option<String>,
    pub active_monitor: Option<String>,
    pub is_paused: bool,
    pub is_hidden: bool,
    pub volume: i64,
    pub is_muted: bool,
    pub hwdec: String,
    pub gpu_device: Option<String>,
    pub target_fps: u32,
    pub screen_id: i64,
    pub slideshow_active: bool,
    pub slideshow_interval: u64,
    pub slideshow_shuffle: bool,
    pub opacity: f32,
    pub widget_enabled: bool,
    pub widget_url: Option<String>,
    pub monitor_wallpapers: HashMap<String, String>,
    pub total_wallpapers: usize,
}

pub async fn send_ipc_request(socket_path: &Path, req: &IpcRequest) -> Result<IpcResponse, String> {
    let timeout_dur = Duration::from_secs(3);
    tokio::time::timeout(timeout_dur, async {
        let mut stream = UnixStream::connect(socket_path)
            .await
            .map_err(|e| format!("Daemon Connection Refused at {}: {}", socket_path.display(), e))?;

        let json_bytes = serde_json::to_vec(req)
            .map_err(|e| format!("Failed to serialize request: {}", e))?;

        let len = json_bytes.len() as u32;
        stream
            .write_all(&len.to_be_bytes())
            .await
            .map_err(|e| format!("Failed to write length header: {}", e))?;

        stream
            .write_all(&json_bytes)
            .await
            .map_err(|e| format!("Failed to write IPC payload: {}", e))?;

        stream.flush().await.map_err(|e| e.to_string())?;

        let mut len_buf = [0u8; 4];
        stream
            .read_exact(&mut len_buf)
            .await
            .map_err(|e| format!("Failed to read response header: {}", e))?;

        let resp_len = u32::from_be_bytes(len_buf) as usize;
        let mut resp_buf = vec![0u8; resp_len];
        stream
            .read_exact(&mut resp_buf)
            .await
            .map_err(|e| format!("Failed to read response body: {}", e))?;

        let resp: IpcResponse = serde_json::from_slice(&resp_buf)
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        Ok(resp)
    })
    .await
    .map_err(|_| "IPC Request Timeout: Daemon did not respond within 3s".to_string())?
}
