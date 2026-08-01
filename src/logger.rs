use std::fs::{OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::SystemTime;

static LOGGER: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn init_logging() -> PathBuf {
    let log_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("/home/user/.config"))
        .join("omywall");
    let _ = std::fs::create_dir_all(&log_dir);
    let log_file = log_dir.join("omywall.log");

    if let Ok(mut guard) = LOGGER.lock() {
        *guard = Some(log_file.clone());
    }

    log_info(&format!("Logging initialized at {}", log_file.display()));
    log_file
}

pub fn get_log_path() -> PathBuf {
    if let Ok(guard) = LOGGER.lock() {
        if let Some(ref path) = *guard {
            return path.clone();
        }
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("/home/user/.config"))
        .join("omywall")
        .join("omywall.log")
}

pub fn log_info(msg: &str) {
    write_log("INFO", msg);
}

pub fn log_error(msg: &str) {
    write_log("ERROR", msg);
}

fn write_log(level: &str, msg: &str) {
    let timestamp = match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(dur) => dur.as_secs(),
        Err(_) => 0,
    };

    let log_line = format!("[{}] [{}] {}\n", timestamp, level, msg);
    print!("{}", log_line);

    let path = get_log_path();
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(log_line.as_bytes());
    }
}
