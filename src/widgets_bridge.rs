use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};


pub const TELEMETRY_FILE_PATH: &str = "/tmp/omywall_telemetry.json";

static TELEMETRY_LOOP_RUNNING: AtomicBool = AtomicBool::new(false);
static LAST_CPU_IDLE: AtomicU64 = AtomicU64::new(0);
static LAST_CPU_TOTAL: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct WifiInfo {
    pub connected: bool,
    pub ssid: String,
    pub signal_percent: u8,
    pub interface: String,
    pub ip_address: String,
    pub bitrate: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct BluetoothDevice {
    pub mac: String,
    pub name: String,
    pub connected: bool,
    pub battery_percent: Option<u8>,
    pub icon: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct BluetoothInfo {
    pub enabled: bool,
    pub controller_name: String,
    pub connected_devices: Vec<BluetoothDevice>,
    pub device_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct BatteryInfo {
    pub present: bool,
    pub percentage: u8,
    pub status: String,
    pub is_charging: bool,
    pub health_percent: Option<u8>,
    pub power_watts: Option<f32>,
    pub time_remaining: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct HardwareInfo {
    pub cpu_usage: f32,
    pub cpu_temp_c: Option<f32>,
    pub cpu_model: String,
    pub cpu_cores: usize,
    pub ram_used_mb: u64,
    pub ram_total_mb: u64,
    pub ram_usage: f32,
    pub gpu_usage: f32,
    pub gpu_name: String,
    pub gpu_temp_c: Option<f32>,
    pub vram_used_mb: u64,
    pub vram_total_mb: u64,
    pub disk_usage: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TimeInfo {
    pub time_str: String,
    pub time_full_str: String,
    pub date_str: String,
    pub day_of_week: String,
    pub uptime_str: String,
    pub timestamp_epoch: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TelemetryData {
    pub timestamp: u64,
    pub time: TimeInfo,
    pub wifi: WifiInfo,
    pub bluetooth: BluetoothInfo,
    pub battery: BatteryInfo,
    pub hardware: HardwareInfo,
    pub hostname: String,
    pub username: String,
}

/// Start background telemetry collector thread that updates /tmp/omywall_telemetry.json every 1.5s.
pub fn start_telemetry_loop() {
    if TELEMETRY_LOOP_RUNNING.swap(true, Ordering::SeqCst) {
        // Already running
        return;
    }

    std::thread::Builder::new()
        .name("omywall-telemetry-loop".to_string())
        .spawn(move || {
            loop {
                let _ = poll_and_write_telemetry();
                std::thread::sleep(Duration::from_millis(1500));
            }
        })
        .expect("Failed to spawn telemetry poller thread");
}

/// Poll system telemetry and atomically serialize to /tmp/omywall_telemetry.json.
pub fn poll_and_write_telemetry() -> TelemetryData {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let (hostname, username) = read_host_info();
    let time = read_time_info(now);
    let wifi = read_wifi_info();
    let bluetooth = read_bluetooth_info();
    let battery = read_battery_info();
    let hardware = read_hardware_info();

    let data = TelemetryData {
        timestamp: now,
        time,
        wifi,
        bluetooth,
        battery,
        hardware,
        hostname,
        username,
    };

    if let Ok(json_str) = serde_json::to_string_pretty(&data) {
        let temp_path = format!("{}.tmp", TELEMETRY_FILE_PATH);
        if fs::write(&temp_path, &json_str).is_ok() {
            let _ = fs::rename(&temp_path, TELEMETRY_FILE_PATH);
        } else {
            let _ = fs::write(TELEMETRY_FILE_PATH, &json_str);
        }
    }

    data
}

fn read_host_info() -> (String, String) {
    let mut hostname = String::from("Linux");
    if let Ok(h) = fs::read_to_string("/etc/hostname") {
        let trimmed = h.trim();
        if !trimmed.is_empty() {
            hostname = trimmed.to_string();
        }
    }

    let username = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "user".to_string());

    (hostname, username)
}

fn read_time_info(epoch_secs: u64) -> TimeInfo {
    let mut time_str = String::from("00:00");
    let mut time_full_str = String::from("00:00:00");
    let mut date_str = String::from("Unknown");
    let mut day_of_week = String::from("Day");

    unsafe {
        let t = epoch_secs as libc::time_t;
        let mut tm_val: libc::tm = std::mem::zeroed();
        if !libc::localtime_r(&t, &mut tm_val).is_null() {
            let mut buf = [0u8; 64];

            if libc::strftime(buf.as_mut_ptr() as *mut libc::c_char, buf.len(), c"%H:%M".as_ptr(), &tm_val) > 0 {
                time_str = std::ffi::CStr::from_ptr(buf.as_ptr() as *const libc::c_char).to_string_lossy().to_string();
            }

            if libc::strftime(buf.as_mut_ptr() as *mut libc::c_char, buf.len(), c"%H:%M:%S".as_ptr(), &tm_val) > 0 {
                time_full_str = std::ffi::CStr::from_ptr(buf.as_ptr() as *const libc::c_char).to_string_lossy().to_string();
            }

            if libc::strftime(buf.as_mut_ptr() as *mut libc::c_char, buf.len(), c"%A, %b %d".as_ptr(), &tm_val) > 0 {
                date_str = std::ffi::CStr::from_ptr(buf.as_ptr() as *const libc::c_char).to_string_lossy().to_string();
            }

            if libc::strftime(buf.as_mut_ptr() as *mut libc::c_char, buf.len(), c"%A".as_ptr(), &tm_val) > 0 {
                day_of_week = std::ffi::CStr::from_ptr(buf.as_ptr() as *const libc::c_char).to_string_lossy().to_string();
            }
        }
    }

    let mut uptime_str = String::from("0m");
    if let Ok(uptime_content) = fs::read_to_string("/proc/uptime") {
        if let Some(first) = uptime_content.split_whitespace().next() {
            if let Ok(secs_f) = first.parse::<f64>() {
                let total_secs = secs_f as u64;
                let days = total_secs / 86400;
                let hours = (total_secs % 86400) / 3600;
                let mins = (total_secs % 3600) / 60;
                if days > 0 {
                    uptime_str = format!("{}d {}h {}m", days, hours, mins);
                } else if hours > 0 {
                    uptime_str = format!("{}h {}m", hours, mins);
                } else {
                    uptime_str = format!("{}m", mins);
                }
            }
        }
    }

    TimeInfo {
        time_str,
        time_full_str,
        date_str,
        day_of_week,
        uptime_str,
        timestamp_epoch: epoch_secs,
    }
}

fn read_wifi_info() -> WifiInfo {
    let mut wifi = WifiInfo {
        connected: false,
        ssid: String::new(),
        signal_percent: 0,
        interface: String::new(),
        ip_address: String::new(),
        bitrate: String::new(),
    };

    // 1. Try nmcli
    if let Ok(out) = Command::new("nmcli")
        .args(["-t", "-f", "active,ssid,signal,device", "dev", "wifi"])
        .output()
    {
        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout);
            for line in stdout.lines() {
                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() >= 4 && (parts[0] == "yes" || parts[0] == "*") {
                    wifi.connected = true;
                    wifi.ssid = parts[1].to_string();
                    wifi.signal_percent = parts[2].parse().unwrap_or(0);
                    wifi.interface = parts[3].to_string();
                    break;
                }
            }
        }
    }

    // Fallback: Check /sys/class/net if no active wifi from nmcli
    if !wifi.connected {
        if let Ok(entries) = fs::read_dir("/sys/class/net") {
            for entry in entries.flatten() {
                let iface_path = entry.path();
                let iface_name = entry.file_name().to_string_lossy().to_string();
                if iface_name.starts_with("wl") || iface_path.join("wireless").exists() {
                    wifi.interface = iface_name.clone();
                    if let Ok(oper) = fs::read_to_string(iface_path.join("operstate")) {
                        if oper.trim() == "up" {
                            wifi.connected = true;
                            if wifi.ssid.is_empty() {
                                wifi.ssid = "Connected Network".to_string();
                            }
                            if wifi.signal_percent == 0 {
                                wifi.signal_percent = 75;
                            }
                        }
                    }
                    if wifi.connected {
                        break;
                    }
                }
            }
        }
    }

    // Query IP address for the interface or global IP
    if !wifi.interface.is_empty() {
        if let Ok(out) = Command::new("ip")
            .args(["-o", "-4", "addr", "show", &wifi.interface])
            .output()
        {
            if out.status.success() {
                let stdout = String::from_utf8_lossy(&out.stdout);
                for line in stdout.lines() {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if let Some(pos) = parts.iter().position(|&p| p == "inet") {
                        if let Some(ip_cidr) = parts.get(pos + 1) {
                            let ip = ip_cidr.split('/').next().unwrap_or(ip_cidr);
                            wifi.ip_address = ip.to_string();
                            break;
                        }
                    }
                }
            }
        }
    }

    if wifi.ip_address.is_empty() && wifi.connected {
        if let Ok(out) = Command::new("ip")
            .args(["-o", "-4", "addr", "show"])
            .output()
        {
            if out.status.success() {
                let stdout = String::from_utf8_lossy(&out.stdout);
                for line in stdout.lines() {
                    if !line.contains(" lo ") {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if let Some(pos) = parts.iter().position(|&p| p == "inet") {
                            if let Some(ip_cidr) = parts.get(pos + 1) {
                                let ip = ip_cidr.split('/').next().unwrap_or(ip_cidr);
                                wifi.ip_address = ip.to_string();
                                break;
                            }
                        }
                    }
                }
            }
        }
    }

    wifi
}

fn read_bluetooth_info() -> BluetoothInfo {
    let mut bt = BluetoothInfo {
        enabled: false,
        controller_name: String::new(),
        connected_devices: Vec::new(),
        device_count: 0,
    };

    // Query bluetoothctl show
    if let Ok(out) = Command::new("bluetoothctl").arg("show").output() {
        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout);
            for line in stdout.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("Powered:") {
                    bt.enabled = trimmed.contains("yes");
                } else if (trimmed.starts_with("Name:") || trimmed.starts_with("Alias:"))
                    && bt.controller_name.is_empty() {
                        let name = trimmed.split(':').nth(1).unwrap_or("").trim();
                        bt.controller_name = name.to_string();
                    }
            }
        }
    }

    // Query connected bluetooth devices
    if bt.enabled {
        if let Ok(out) = Command::new("bluetoothctl")
            .args(["devices", "Connected"])
            .output()
        {
            if out.status.success() {
                let stdout = String::from_utf8_lossy(&out.stdout);
                for line in stdout.lines() {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 3 && parts[0] == "Device" {
                        let mac = parts[1].to_string();
                        let name = parts[2..].join(" ");
                        let (battery, icon) = get_bluetooth_device_details(&mac);
                        bt.connected_devices.push(BluetoothDevice {
                            mac,
                            name,
                            connected: true,
                            battery_percent: battery,
                            icon,
                        });
                    }
                }
            }
        }
    }

    bt.device_count = bt.connected_devices.len();
    bt
}

fn get_bluetooth_device_details(mac: &str) -> (Option<u8>, String) {
    let mut battery = None;
    let mut icon = String::from("bluetooth");

    if let Ok(out) = Command::new("bluetoothctl")
        .args(["info", mac])
        .output()
    {
        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout);
            for line in stdout.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("Battery Percentage:") {
                    if let Some(paren_open) = trimmed.find('(') {
                        if let Some(paren_close) = trimmed.find(')') {
                            if let Ok(val) = trimmed[paren_open + 1..paren_close].parse::<u8>() {
                                battery = Some(val);
                            }
                        }
                    }
                } else if trimmed.starts_with("Icon:") {
                    let icon_name = trimmed.split(':').nth(1).unwrap_or("").trim();
                    icon = icon_name.to_string();
                }
            }
        }
    }

    (battery, icon)
}

fn read_battery_info() -> BatteryInfo {
    let mut bat = BatteryInfo {
        present: false,
        percentage: 100,
        status: String::from("AC Connected"),
        is_charging: false,
        health_percent: None,
        power_watts: None,
        time_remaining: None,
    };

    let power_supply_dir = Path::new("/sys/class/power_supply");
    if let Ok(entries) = fs::read_dir(power_supply_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let is_battery = name.starts_with("BAT")
                || fs::read_to_string(path.join("type"))
                    .map(|t| t.trim() == "Battery")
                    .unwrap_or(false);

            if is_battery {
                bat.present = true;

                if let Ok(cap) = fs::read_to_string(path.join("capacity")) {
                    if let Ok(p) = cap.trim().parse::<u8>() {
                        bat.percentage = p;
                    }
                }

                if let Ok(st) = fs::read_to_string(path.join("status")) {
                    let s = st.trim().to_string();
                    bat.is_charging = s.eq_ignore_ascii_case("charging");
                    bat.status = s;
                }

                // Voltage and current/power calculation
                let voltage_u_v = fs::read_to_string(path.join("voltage_now"))
                    .ok()
                    .and_then(|s| s.trim().parse::<f64>().ok());
                let current_u_a = fs::read_to_string(path.join("current_now"))
                    .ok()
                    .and_then(|s| s.trim().parse::<f64>().ok());
                let power_u_w = fs::read_to_string(path.join("power_now"))
                    .ok()
                    .and_then(|s| s.trim().parse::<f64>().ok());

                if let Some(p_uw) = power_u_w {
                    bat.power_watts = Some((p_uw / 1_000_000.0) as f32);
                } else if let (Some(v), Some(c)) = (voltage_u_v, current_u_a) {
                    bat.power_watts = Some(((v * c) / 1_000_000_000_000.0) as f32);
                }


                // Health calculation
                let charge_full = fs::read_to_string(path.join("charge_full"))
                    .or_else(|_| fs::read_to_string(path.join("energy_full")))
                    .ok()
                    .and_then(|s| s.trim().parse::<f64>().ok());
                let charge_design = fs::read_to_string(path.join("charge_full_design"))
                    .or_else(|_| fs::read_to_string(path.join("energy_full_design")))
                    .ok()
                    .and_then(|s| s.trim().parse::<f64>().ok());

                if let (Some(full), Some(design)) = (charge_full, charge_design) {
                    if design > 0.0 {
                        let health = ((full / design) * 100.0).clamp(0.0, 100.0) as u8;
                        bat.health_percent = Some(health);
                    }
                }

                break;
            }
        }
    }

    bat
}

fn read_hardware_info() -> HardwareInfo {
    let mut hw = HardwareInfo::default();

    // CPU Usage
    if let Ok(stat) = fs::read_to_string("/proc/stat") {
        if let Some(first_line) = stat.lines().next() {
            let parts: Vec<&str> = first_line.split_whitespace().collect();
            if parts.len() >= 5 {
                let user: u64 = parts[1].parse().unwrap_or(0);
                let nice: u64 = parts[2].parse().unwrap_or(0);
                let system: u64 = parts[3].parse().unwrap_or(0);
                let idle: u64 = parts[4].parse().unwrap_or(0);
                let iowait: u64 = parts.get(5).and_then(|p| p.parse().ok()).unwrap_or(0);
                let irq: u64 = parts.get(6).and_then(|p| p.parse().ok()).unwrap_or(0);
                let softirq: u64 = parts.get(7).and_then(|p| p.parse().ok()).unwrap_or(0);

                let total_idle = idle + iowait;
                let total = user + nice + system + idle + iowait + irq + softirq;

                let prev_idle = LAST_CPU_IDLE.swap(total_idle, Ordering::Relaxed);
                let prev_total = LAST_CPU_TOTAL.swap(total, Ordering::Relaxed);

                let delta_total = total.saturating_sub(prev_total);
                let delta_idle = total_idle.saturating_sub(prev_idle);

                if delta_total > 0 {
                    hw.cpu_usage = (((delta_total.saturating_sub(delta_idle)) as f32 / delta_total as f32) * 100.0).clamp(0.0, 100.0);
                }
            }
        }
    }

    // CPU Model and Cores
    if let Ok(cpuinfo) = fs::read_to_string("/proc/cpuinfo") {
        let mut count = 0;
        for line in cpuinfo.lines() {
            if line.starts_with("model name") && hw.cpu_model.is_empty() {
                if let Some(model) = line.split(':').nth(1) {
                    hw.cpu_model = model.trim().to_string();
                }
            }
            if line.starts_with("processor") {
                count += 1;
            }
        }
        hw.cpu_cores = if count > 0 { count } else { 1 };
    }

    // CPU Temp
    if let Ok(temp_str) = fs::read_to_string("/sys/class/thermal/thermal_zone0/temp") {
        if let Ok(temp_raw) = temp_str.trim().parse::<f32>() {
            hw.cpu_temp_c = Some(temp_raw / 1000.0);
        }
    }

    // RAM Info
    if let Ok(mem) = fs::read_to_string("/proc/meminfo") {
        let mut total_kb = 0u64;
        let mut avail_kb = 0u64;
        for line in mem.lines() {
            if line.starts_with("MemTotal:") {
                total_kb = line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
            } else if line.starts_with("MemAvailable:") {
                avail_kb = line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
            }
        }
        hw.ram_total_mb = total_kb / 1024;
        hw.ram_used_mb = (total_kb.saturating_sub(avail_kb)) / 1024;
        if hw.ram_total_mb > 0 {
            hw.ram_usage = ((hw.ram_used_mb as f32 / hw.ram_total_mb as f32) * 100.0).clamp(0.0, 100.0);
        }
    }

    // GPU Info via nvidia-smi
    if let Ok(out) = Command::new("nvidia-smi")
        .args(["--query-gpu=utilization.gpu,memory.used,memory.total,temperature.gpu,name", "--format=csv,noheader,nounits"])
        .output()
    {
        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let parts: Vec<&str> = stdout.trim().split(',').map(|s| s.trim()).collect();
            if parts.len() >= 5 {
                hw.gpu_usage = parts[0].parse().unwrap_or(0.0);
                hw.vram_used_mb = parts[1].parse().unwrap_or(0);
                hw.vram_total_mb = parts[2].parse().unwrap_or(0);
                hw.gpu_temp_c = parts[3].parse().ok();
                hw.gpu_name = parts[4].to_string();
            }
        }
    }

    // Fallback GPU Name if not nvidia
    if hw.gpu_name.is_empty() {
        let gpus = crate::config::detect_system_gpus();
        if let Some(first_gpu) = gpus.first() {
            hw.gpu_name = first_gpu.name.clone();
        }
    }

    // Root filesystem disk usage
    unsafe {
        let path = std::ffi::CString::new("/").unwrap();
        let mut stat: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(path.as_ptr(), &mut stat) == 0 {
            let total_blocks = stat.f_blocks as f64;
            let free_blocks = stat.f_bfree as f64;
            if total_blocks > 0.0 {
                let used_ratio = ((total_blocks - free_blocks) / total_blocks) * 100.0;
                hw.disk_usage = Some(used_ratio.clamp(0.0, 100.0) as f32);
            }
        }
    }

    hw
}
