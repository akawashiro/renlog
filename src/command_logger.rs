use crate::ansi_filter;
use crate::symlink_utils::create_or_update_symlink;
use std::fs::File;
use std::io::{Result, Write};
use std::path::{Path, PathBuf};

fn sanitize_for_filename(command: &str) -> String {
    // Take first 50 characters and replace problematic characters
    let truncated = if command.len() > 50 {
        &command[..50]
    } else {
        command
    };

    truncated
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\n' | '\r' | '\t' => '_',
            ' ' => '-',
            c if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' => c,
            _ => '_',
        })
        .collect()
}

pub fn ensure_log_directory(log_dir: &str) -> Result<()> {
    log::debug!("Ensuring command log directory exists: {}", log_dir);

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

    log::info!("Command log directory ensured: {}", log_dir);
    Ok(())
}

fn create_latest_symlink(log_dir: &str, filename: &str) -> Result<()> {
    let latest_path = Path::new(log_dir).join("latest.log");
    create_or_update_symlink(filename, &latest_path)?;
    log::info!("Created latest.log symlink pointing to: {}", filename);
    Ok(())
}

pub fn save_command_log(
    log_dir: &str,
    pid: u32,
    command: &str,
    data: &[u8],
    exit_status: Option<i32>,
) -> Result<()> {
    log::debug!(
        "Saving command log for '{}', data size: {} bytes, exit status: {:?}",
        command,
        data.len(),
        exit_status
    );

    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S_%3f").to_string();
    let safe_command = sanitize_for_filename(command);
    let filename = format!("{}_{}_cmd_{}.log", timestamp, pid, safe_command);
    let filepath = PathBuf::from(log_dir).join(&filename);

    log::info!(
        "Creating command log file: {} for command: {}",
        filepath.display(),
        command
    );

    let mut file = File::create(&filepath).map_err(|e| {
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

    // Write command metadata header
    let mut header = format!(
        "# Renlog Command Log\n# Command: {}\n# Started: {}\n# PID: {}\n",
        command,
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
        pid
    );

    if let Some(status) = exit_status {
        header.push_str(&format!("# Exit Status: {}\n", status));
    }
    header.push_str("\n");

    file.write_all(header.as_bytes()).map_err(|e| {
        log::error!("Failed to write header to command log: {}", e);
        e
    })?;

    // Write the filtered command output
    let filtered_data = ansi_filter::strip_ansi_codes(data);
    file.write_all(&filtered_data).map_err(|e| {
        log::error!("Failed to write command data to log: {}", e);
        e
    })?;

    // Write exit status footer if available
    if let Some(status) = exit_status {
        let footer = format!("\n[Command ended with exit status: {}]\n", status);
        file.write_all(footer.as_bytes()).map_err(|e| {
            log::error!("Failed to write footer to command log: {}", e);
            e
        })?;
    }

    file.flush().map_err(|e| {
        log::error!("Failed to flush command log: {}", e);
        e
    })?;

    // Create symlink to the new log file
    if let Err(e) = create_latest_symlink(log_dir, &filename) {
        log::warn!("Failed to create latest.log symlink: {}", e);
    }

    log::info!(
        "Command log saved successfully: {} bytes (filtered from {} bytes)",
        filtered_data.len(),
        data.len()
    );

    Ok(())
}
