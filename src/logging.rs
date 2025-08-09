use log::LevelFilter;
use simplelog::{ConfigBuilder, WriteLogger};
use std::io::{self, Result};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};

static LOGGING_ENABLED: AtomicBool = AtomicBool::new(false);

pub fn init_logging(log_file: &str, log_level: &str) -> Result<()> {
    let level_filter = match LevelFilter::from_str(log_level) {
        Ok(level) => level,
        Err(_) => {
            eprintln!(
                "Warning: Invalid log level '{}', defaulting to 'info'",
                log_level
            );
            LevelFilter::Info
        }
    };

    match init_file_logging(log_file, level_filter) {
        Ok(()) => {
            LOGGING_ENABLED.store(true, Ordering::Relaxed);
            log::info!("Renlog internal logging initialized to file: {}", log_file);
            Ok(())
        }
        Err(e) => {
            eprintln!(
                "Warning: Failed to initialize file logging to '{}': {}",
                log_file, e
            );
            eprintln!("Continuing without logging...");
            init_null_logging()?;
            Ok(())
        }
    }
}

fn init_file_logging(file_path: &str, level: LevelFilter) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;

    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(file_path)
        .map_err(|e| io::Error::other(format!("Failed to create/open log file: {}", e)))?;

    let config = ConfigBuilder::new()
        .set_location_level(log::LevelFilter::Trace)
        .build();
    WriteLogger::init(level, config, file)
        .map_err(|e| io::Error::other(format!("Failed to initialize logger: {}", e)))?;

    Ok(())
}

fn init_null_logging() -> Result<()> {
    log::set_max_level(LevelFilter::Off);
    LOGGING_ENABLED.store(false, Ordering::Relaxed);
    Ok(())
}

pub fn is_logging_enabled() -> bool {
    LOGGING_ENABLED.load(Ordering::Relaxed)
}

pub fn log_function_entry(function_name: &str) {
    log::debug!("Entering function: {}", function_name);
}

pub fn log_function_exit(function_name: &str) {
    log::debug!("Exiting function: {}", function_name);
}

pub fn log_error_with_context(error: &dyn std::error::Error, context: &str) {
    log::error!("{}: {}", context, error);
}
