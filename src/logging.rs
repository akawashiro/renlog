use chrono::Utc;
use log::{Level, LevelFilter, Log, Metadata, Record};
use std::fs::{File, OpenOptions};
use std::io::{self, Result, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

static LOGGING_ENABLED: AtomicBool = AtomicBool::new(false);
static LOGGER: GlogLogger = GlogLogger {
    file: Mutex::new(None),
    level: Mutex::new(LevelFilter::Off),
};

struct GlogLogger {
    file: Mutex<Option<File>>,
    level: Mutex<LevelFilter>,
}

impl GlogLogger {
    fn set_file(&self, file: File) {
        *self.file.lock().unwrap() = Some(file);
    }

    fn set_level(&self, level: LevelFilter) {
        *self.level.lock().unwrap() = level;
    }

    fn level_to_char(level: Level) -> char {
        match level {
            Level::Error => 'E',
            Level::Warn => 'W',
            Level::Info => 'I',
            Level::Debug => 'D',
            Level::Trace => 'T',
        }
    }
}

impl Log for GlogLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= *self.level.lock().unwrap()
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let now = Utc::now();
            let timestamp = now.format("%Y%m%d %H:%M:%S%.6f");
            let pid = std::process::id();
            let level_char = Self::level_to_char(record.level());

            let file_line = if let (Some(file), Some(line)) = (record.file(), record.line()) {
                let file_name = file.rsplit('/').next().unwrap_or(file);
                format!(" {file_name}:{line}")
            } else {
                String::new()
            };

            let formatted = format!(
                "{}{} {}{}] {}\n",
                level_char,
                timestamp,
                pid,
                file_line,
                record.args()
            );

            if let Ok(mut file_guard) = self.file.lock() {
                if let Some(file) = file_guard.as_mut() {
                    let _ = file.write_all(formatted.as_bytes());
                    let _ = file.flush();
                }
            }
        }
    }

    fn flush(&self) {
        if let Ok(mut file_guard) = self.file.lock() {
            if let Some(file) = file_guard.as_mut() {
                let _ = file.flush();
            }
        }
    }
}

unsafe impl Sync for GlogLogger {}

pub fn init_logging(log_file: &str, log_level: &str) -> Result<()> {
    let level_filter = match LevelFilter::from_str(log_level) {
        Ok(level) => level,
        Err(_) => {
            eprintln!("Warning: Invalid log level '{log_level}', defaulting to 'info'");
            LevelFilter::Info
        }
    };

    match init_file_logging(log_file, level_filter) {
        Ok(()) => {
            LOGGING_ENABLED.store(true, Ordering::Relaxed);
            log::info!("Renlog internal logging initialized to file: {log_file}");
            Ok(())
        }
        Err(e) => {
            eprintln!("Warning: Failed to initialize file logging to '{log_file}': {e}");
            eprintln!("Continuing without logging...");
            init_null_logging()?;
            Ok(())
        }
    }
}

fn init_file_logging(file_path: &str, level: LevelFilter) -> Result<()> {
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(file_path)
        .map_err(|e| io::Error::other(format!("Failed to create/open log file: {e}")))?;

    LOGGER.set_file(file);
    LOGGER.set_level(level);

    log::set_logger(&LOGGER).map_err(|e| io::Error::other(format!("Failed to set logger: {e}")))?;

    log::set_max_level(level);

    Ok(())
}

fn init_null_logging() -> Result<()> {
    log::set_max_level(LevelFilter::Off);
    LOGGING_ENABLED.store(false, Ordering::Relaxed);
    Ok(())
}
