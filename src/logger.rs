use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static LOGGER: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn init_logging() -> PathBuf {
    let log_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("omywall");
    let _ = std::fs::create_dir_all(&log_dir);
    let log_file = log_dir.join("omywall.log");

    if let Ok(mut guard) = LOGGER.lock() {
        *guard = Some(log_file.clone());
    }

    setup_panic_hook();
    log_info(&format!("Logging initialized at {}", log_file.display()));
    log_memory_diagnostic();
    log_file
}

pub fn get_log_path() -> PathBuf {
    if let Ok(guard) = LOGGER.lock() {
        if let Some(ref path) = *guard {
            return path.clone();
        }
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("omywall")
        .join("omywall.log")
}

pub fn log_info(msg: &str) {
    write_log("INFO", msg);
}

pub fn log_error(msg: &str) {
    write_log("ERROR", msg);
    log_memory_diagnostic();
}

pub fn log_memory_diagnostic() {
    if let Ok(statm) = std::fs::read_to_string("/proc/self/statm") {
        let parts: Vec<&str> = statm.split_whitespace().collect();
        if parts.len() >= 2 {
            let page_size_kb = 4;
            let vm_size = parts[0].parse::<u64>().unwrap_or(0) * page_size_kb;
            let rss_size = parts[1].parse::<u64>().unwrap_or(0) * page_size_kb;
            write_log("MEMORY_DIAG", &format!("Process Memory -> RSS: {} KB, VIRT: {} KB", rss_size, vm_size));
        }
    }
}

pub fn setup_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let location = info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())).unwrap_or_else(|| "unknown location".into());
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic payload".to_string()
        };

        log_error(&format!("FATAL PANIC at [{}]: {}", location, payload));
    }));
}

fn write_log(level: &str, msg: &str) {
    let now = SystemTime::now();
    let timestamp = match now.duration_since(UNIX_EPOCH) {
        Ok(dur) => dur.as_secs(),
        Err(_) => 0,
    };

    let log_line = format!("[TS:{}] [{}] {}\n", timestamp, level, msg);
    print!("{}", log_line);

    let path = get_log_path();
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(log_line.as_bytes());
    }
}
