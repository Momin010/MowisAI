//! Structured JSON logging with file output and log rotation.

use lazy_static::lazy_static;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

const MAX_LOG_SIZE: u64 = 10 * 1024 * 1024; // 10 MB
const MAX_LOG_BACKUPS: usize = 5;

static TUI_ACTIVE: AtomicBool = AtomicBool::new(false);

lazy_static! {
    static ref LOGGER: Mutex<Option<StructuredLogger>> = Mutex::new(None);
    static ref TUI_LOG_SENDER: Mutex<Option<std::sync::mpsc::Sender<(String, String, u64)>>> = Mutex::new(None);
}

struct StructuredLogger {
    file: File,
    path: PathBuf,
    bytes_written: u64,
}

impl StructuredLogger {
    fn open(path: &Path) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        let bytes_written = file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(Self {
            file,
            path: path.to_path_buf(),
            bytes_written,
        })
    }

    fn write_entry(&mut self, level: &str, message: &str) {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let entry = serde_json::json!({
            "ts": timestamp,
            "level": level,
            "msg": message,
        });

        let line = format!("{}\n", entry);
        let bytes = line.as_bytes();
        if self.file.write_all(bytes).is_ok() {
            self.bytes_written += bytes.len() as u64;
        }

        if self.bytes_written >= MAX_LOG_SIZE {
            self.rotate();
        }
    }

    fn rotate(&mut self) {
        // Shift existing backups: .5 drop, .4→.5, .3→.4, ...
        for i in (1..MAX_LOG_BACKUPS).rev() {
            let from = self.path.with_extension(format!("log.{}", i));
            let to = self.path.with_extension(format!("log.{}", i + 1));
            let _ = fs::rename(&from, &to);
        }
        let backup = self.path.with_extension("log.1");
        let _ = fs::rename(&self.path, &backup);

        // Open fresh file
        if let Ok(new_file) = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&self.path)
        {
            self.file = new_file;
            self.bytes_written = 0;
        }
    }
}

/// Initialize the structured logger. Call once at startup.
pub fn init(log_path: &Path) -> anyhow::Result<()> {
    let logger = StructuredLogger::open(log_path)
        .map_err(|e| anyhow::anyhow!("Failed to open log file {:?}: {}", log_path, e))?;
    if let Ok(mut guard) = LOGGER.lock() {
        *guard = Some(logger);
    }
    Ok(())
}

/// Signal that the TUI is active; console output is suppressed when true.
pub fn set_tui_active(active: bool) {
    TUI_ACTIVE.store(active, Ordering::Relaxed);
}

/// Register a sender to forward log entries to the TUI development view.
pub fn set_tui_log_sender(sender: std::sync::mpsc::Sender<(String, String, u64)>) {
    if let Ok(mut guard) = TUI_LOG_SENDER.lock() {
        *guard = Some(sender);
    }
}

/// Write a structured log entry.
pub fn log(level: &str, message: &str) {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if let Ok(mut guard) = LOGGER.lock() {
        if let Some(ref mut logger) = *guard {
            logger.write_entry(level, message);
        }
    }

    if let Ok(guard) = TUI_LOG_SENDER.lock() {
        if let Some(ref sender) = *guard {
            let _ = sender.send((level.to_string(), message.to_string(), timestamp));
        }
    }

    // Mirror to stderr when TUI is not active
    if !TUI_ACTIVE.load(Ordering::Relaxed) {
        eprintln!("[{}] {}", level, message);
    }
}

#[allow(dead_code)]
pub fn info(msg: &str) {
    log("INFO", msg);
}

#[allow(dead_code)]
pub fn warn(msg: &str) {
    log("WARN", msg);
}

#[allow(dead_code)]
pub fn error(msg: &str) {
    log("ERROR", msg);
}

#[allow(dead_code)]
pub fn debug(msg: &str) {
    log("DEBUG", msg);
}
