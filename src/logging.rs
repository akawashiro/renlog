use log::LevelFilter;
use simplelog::{Config, WriteLogger};
use std::io::{self, Result};
use std::str::FromStr;

pub fn init_logging(log_file: Option<&str>, log_level: &str) -> Result<()> {
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

    match log_file {
        Some(file_path) => match init_file_logging(file_path, level_filter) {
            Ok(()) => {
                log::info!("Renlog internal logging initialized to file: {}", file_path);
                Ok(())
            }
            Err(e) => {
                eprintln!(
                    "Warning: Failed to initialize file logging to '{}': {}",
                    file_path, e
                );
                eprintln!("Continuing without logging...");
                Ok(())
            }
        },
        None => Ok(()),
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

    WriteLogger::init(level, Config::default(), file)
        .map_err(|e| io::Error::other(format!("Failed to initialize logger: {}", e)))?;

    Ok(())
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
