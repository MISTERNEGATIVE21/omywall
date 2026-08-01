use std::env;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixStream;

use crate::ipc::{send_ipc_request, IpcRequest};
use crate::logger::log_info;

pub async fn start_workspace_listener(socket_path: PathBuf) {
    if let (Ok(signature), Ok(runtime_dir)) = (env::var("HYPRLAND_INSTANCE_SIGNATURE"), env::var("XDG_RUNTIME_DIR")) {
        let hypr_socket = PathBuf::from(runtime_dir)
            .join("hypr")
            .join(&signature)
            .join(".socket2.sock");

        if hypr_socket.exists() {
            log_info(&format!("Workspace Listener: Connected to Hyprland IPC at {}", hypr_socket.display()));
            tokio::spawn(listen_hyprland_events(hypr_socket, socket_path));
            return;
        }
    }

    if let Ok(sway_socket) = env::var("SWAYSOCK").or_else(|_| env::var("I3SOCK")) {
        let path = PathBuf::from(&sway_socket);
        if path.exists() {
            log_info(&format!("Workspace Listener: Connected to Sway/i3 IPC at {}", path.display()));
            tokio::spawn(listen_sway_events(path, socket_path));
            return;
        }
    }

    log_info("Workspace Listener: Ready for workspace IPC switch commands.");
}

async fn listen_hyprland_events(hypr_socket: PathBuf, daemon_socket: PathBuf) {
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    loop {
        if let Ok(stream) = UnixStream::connect(&hypr_socket).await {
            let reader = BufReader::new(stream);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                if line.starts_with("focusedmon>>") {
                    let rest = line.trim_start_matches("focusedmon>>");
                    let mut parts = rest.split(',');
                    let monitor = parts.next().unwrap_or("").trim().to_string();
                    let workspace = parts.next().unwrap_or("").trim().to_string();

                    if !workspace.is_empty() || !monitor.is_empty() {
                        log_info(&format!("Hyprland Event: Focused Monitor '{}', Workspace '{}'", monitor, workspace));
                        let _ = send_ipc_request(
                            &daemon_socket,
                            &IpcRequest::SwitchWorkspaceAndMonitor { workspace, monitor },
                        )
                        .await;
                    }
                } else if line.starts_with("workspace>>") || line.starts_with("workspacev2>>") {
                    let raw = if line.starts_with("workspacev2>>") {
                        line.trim_start_matches("workspacev2>>")
                    } else {
                        line.trim_start_matches("workspace>>")
                    };

                    let ws_name = raw.split(',').next().unwrap_or("").trim().to_string();
                    if !ws_name.is_empty() {
                        log_info(&format!("Hyprland Event: Workspace switch to '{}'", ws_name));
                        let _ = send_ipc_request(
                            &daemon_socket,
                            &IpcRequest::SwitchWorkspace { workspace: ws_name },
                        )
                        .await;
                    }
                }
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
    }
}

async fn listen_sway_events(sway_socket: PathBuf, daemon_socket: PathBuf) {
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    loop {
        if let Ok(stream) = UnixStream::connect(&sway_socket).await {
            let reader = BufReader::new(stream);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                if line.contains("\"change\":\"focus\"") || line.contains("workspace") {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
                        if let Some(current) = json.get("current").and_then(|c| c.get("name")).and_then(|n| n.as_str()) {
                            log_info(&format!("Sway/i3 Event: Workspace switch to '{}'", current));
                            let _ = send_ipc_request(
                                &daemon_socket,
                                &IpcRequest::SwitchWorkspace { workspace: current.to_string() },
                            )
                            .await;
                        }
                    }
                }
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
    }
}
