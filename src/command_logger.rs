use crate::ansi_filter;
use std::fs::File;
use std::io::{Result, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct CommandLogger {
    log_dir: String,
    pid: u32,
    current_log_file: Arc<Mutex<Option<File>>>,
}

impl CommandLogger {
    pub fn new(log_dir: &str, pid: u32) -> Result<Self> {
        log::debug!(
            "Creating CommandLogger with log_dir: {}, pid: {}",
            log_dir,
            pid
        );

        std::fs::create_dir_all(log_dir).map_err(|e| {
            log::error!(
                "Failed to create command log directory '{}': {}",
                log_dir,
                e
            );
            std::io::Error::other(format!(
                "Failed to create command log directory '{}': {}",
                log_dir, e
            ))
        })?;

        log::info!("CommandLogger created for directory: {}", log_dir);
        Ok(CommandLogger {
            log_dir: log_dir.to_string(),
            pid,
            current_log_file: Arc::new(Mutex::new(None)),
        })
    }

    pub fn start_new_command(&self) -> Result<()> {
        log::debug!("Starting new command, creating new log file");

        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S_%3f").to_string();
        let filename = format!("{}_{}.log", timestamp, self.pid);
        let filepath = PathBuf::from(&self.log_dir).join(filename);

        log::info!("Creating new command log file: {}", filepath.display());

        let file = File::create(&filepath).map_err(|e| {
            log::error!(
                "Failed to create command log file '{}': {}",
                filepath.display(),
                e
            );
            std::io::Error::other(format!(
                "Failed to create command log file '{}': {}",
                filepath.display(),
                e
            ))
        })?;

        let mut current_file = self.current_log_file.lock().unwrap();
        *current_file = Some(file);

        log::debug!("New command log file created successfully");
        Ok(())
    }

    pub fn log_command_output(&self, data: &[u8]) -> Result<()> {
        let mut current_file = self.current_log_file.lock().unwrap();

        if let Some(ref mut file) = *current_file {
            let filtered_data = ansi_filter::strip_ansi_codes(data);
            file.write_all(&filtered_data)?;
            file.flush()?;
            log::debug!(
                "Logged {} bytes to command file (filtered from {} bytes)",
                filtered_data.len(),
                data.len()
            );
        } else {
            log::warn!("Attempted to log command output but no active log file");
        }

        Ok(())
    }

    pub fn close_current_command(&self) {
        log::debug!("Closing current command log file");
        let mut current_file = self.current_log_file.lock().unwrap();
        *current_file = None;
        log::debug!("Current command log file closed");
    }

    pub fn has_active_command(&self) -> bool {
        let current_file = self.current_log_file.lock().unwrap();
        current_file.is_some()
    }
}
