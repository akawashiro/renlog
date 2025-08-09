use crate::ansi_filter;
use std::fs::File;
use std::io::{Result, Write};
use std::os::unix::fs;
use std::path::{Path, PathBuf};
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

    fn create_latest_symlink(&self, filename: &str) -> Result<()> {
        let latest_path = Path::new(&self.log_dir).join("latest.log");
        
        // Remove existing symlink if it exists
        if latest_path.exists() || latest_path.is_symlink() {
            if let Err(e) = std::fs::remove_file(&latest_path) {
                log::warn!("Failed to remove existing latest.log symlink: {}", e);
            }
        }
        
        // Create new symlink
        fs::symlink(filename, &latest_path).map_err(|e| {
            log::error!("Failed to create latest.log symlink -> {}: {}", filename, e);
            std::io::Error::other(format!("Failed to create latest.log symlink -> {}: {}", filename, e))
        })?;
        
        log::info!("Created latest.log symlink pointing to: {}", filename);
        Ok(())
    }

    pub fn start_new_command(&self) -> Result<()> {
        log::debug!("Starting new command, creating new log file");

        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S_%3f").to_string();
        let filename = format!("{}_{}.log", timestamp, self.pid);
        let filepath = PathBuf::from(&self.log_dir).join(&filename);

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

        // Create symlink to the new log file
        if let Err(e) = self.create_latest_symlink(&filename) {
            log::warn!("Failed to create latest.log symlink: {}", e);
        }

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

}
