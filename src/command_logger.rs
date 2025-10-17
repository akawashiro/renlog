use crate::log_processor;
use crate::symlink_utils::create_or_update_symlink;
use std::fs::File;
use std::io::{Result, Write};
use std::path::{Path, PathBuf};

const MAX_FILENAME_LENGTH: usize = 256;

fn sanitize_for_filename(command: &str) -> String {
    // Replace problematic characters with safe alternatives
    command
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\n' | '\r' | '\t' => '_',
            ' ' => '-',
            c if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' => c,
            _ => '_',
        })
        .collect()
}

/// Truncate filename to ensure it doesn't exceed MAX_FILENAME_LENGTH characters
/// Preserves the file extension if present
fn truncate_filename_to_limit(filename: &str) -> String {
    if filename.len() <= MAX_FILENAME_LENGTH {
        return filename.to_string();
    }

    // Find the extension (last occurrence of '.')
    let (name_part, extension) = if let Some(dot_pos) = filename.rfind('.') {
        let name = &filename[..dot_pos];
        let ext = &filename[dot_pos..]; // includes the dot
        (name, ext)
    } else {
        (filename, "")
    };

    // Calculate how much space we have for the name part
    if extension.len() >= MAX_FILENAME_LENGTH {
        // If extension is too long, just truncate the whole thing
        log::warn!(
            "Filename extension too long, truncating entire filename to {MAX_FILENAME_LENGTH} chars"
        );
        return filename[..MAX_FILENAME_LENGTH].to_string();
    }

    let available_space = MAX_FILENAME_LENGTH - extension.len();

    // Truncate the name part and add the extension back
    let truncated_name = if name_part.len() > available_space {
        &name_part[..available_space]
    } else {
        name_part
    };

    let result = format!("{truncated_name}{extension}");

    log::debug!(
        "Truncated filename from {} chars to {} chars: '{filename}' -> '{result}'",
        filename.len(),
        result.len()
    );

    result
}

pub fn ensure_log_directory(log_dir: &str) -> Result<()> {
    log::debug!("Ensuring command log directory exists: {log_dir}");

    std::fs::create_dir_all(log_dir).map_err(|e| {
        log::error!("Failed to create command log directory '{log_dir}': {e}");
        std::io::Error::other(format!(
            "Failed to create command log directory '{log_dir}': {e}"
        ))
    })?;

    log::info!("Command log directory ensured: {log_dir}");
    Ok(())
}

fn create_latest_symlink(log_dir: &str, filename: &str) -> Result<()> {
    let latest_path = Path::new(log_dir).join("latest.log");
    create_or_update_symlink(filename, &latest_path)?;
    log::info!("Created latest.log symlink pointing to: {filename}");
    Ok(())
}

/// Update the current log file tracking file (PID-specific to avoid conflicts)
fn update_current_log_file(log_dir: &str, log_file_path: &str, pid: u32) -> Result<()> {
    let current_log_marker = Path::new(log_dir).join(format!(".renlog_current_log_{pid}"));
    std::fs::write(&current_log_marker, log_file_path).map_err(|e| {
        log::warn!("Failed to update current log file marker: {e}");
        e
    })?;
    log::debug!("Updated current log file marker for PID {pid}: {log_file_path}");
    Ok(())
}

pub fn save_command_log(
    log_dir: &str,
    command: &str,
    data: &[u8],
    exit_status: Option<i32>,
    pid: u32,
) -> Result<String> {
    log::debug!(
        "Saving command log for '{command}', data size: {} bytes, exit status: {exit_status:?}",
        data.len()
    );

    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S_%3f").to_string();
    let safe_command = sanitize_for_filename(command);
    let filename_raw = format!("{timestamp}_{safe_command}.log");
    let filename = truncate_filename_to_limit(&filename_raw);
    let filepath = PathBuf::from(log_dir).join(&filename);

    log::info!(
        "Creating command log file: {} for command: {command}",
        filepath.display()
    );

    let mut file = File::create(&filepath).map_err(|e| {
        log::error!(
            "Failed to create command log file '{}': {e}",
            filepath.display()
        );
        std::io::Error::other(format!(
            "Failed to create command log file '{}': {e}",
            filepath.display()
        ))
    })?;

    // Write command metadata header
    let mut header = format!(
        "# renlog Command Log\n# Command: {}\n# Started: {}\n",
        command,
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    );

    if let Some(status) = exit_status {
        header.push_str(&format!("# Exit Status: {status}\n"));
    }
    header.push('\n');

    file.write_all(header.as_bytes()).map_err(|e| {
        log::error!("Failed to write header to command log: {e}");
        e
    })?;

    // Write the filtered command output (truncate after end command, then strip ANSI codes and command markers)
    let filtered_data = log_processor::process_command_log_content(data);
    file.write_all(&filtered_data).map_err(|e| {
        log::error!("Failed to write command data to log: {e}");
        e
    })?;

    file.flush().map_err(|e| {
        log::error!("Failed to flush command log: {e}");
        e
    })?;

    // Create symlink to the new log file
    if let Err(e) = create_latest_symlink(log_dir, &filename) {
        log::warn!("Failed to create latest.log symlink: {e}");
    }

    log::info!(
        "Command log saved successfully: {} bytes (filtered from {} bytes)",
        filtered_data.len(),
        data.len()
    );

    let log_file_path = filepath.to_string_lossy().to_string();

    // Update the current log file tracking
    if let Err(e) = update_current_log_file(log_dir, &log_file_path, pid) {
        log::warn!("Failed to update current log file tracking: {e}");
    }

    Ok(log_file_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_filename_short() {
        let short_filename = "test.log";
        let result = truncate_filename_to_limit(short_filename);
        assert_eq!(result, "test.log");
        assert!(result.len() <= MAX_FILENAME_LENGTH);
    }

    #[test]
    fn test_truncate_filename_exact_limit() {
        let filename = "a".repeat(MAX_FILENAME_LENGTH);
        let result = truncate_filename_to_limit(&filename);
        assert_eq!(result.len(), MAX_FILENAME_LENGTH);
        assert_eq!(result, filename);
    }

    #[test]
    fn test_truncate_filename_over_limit() {
        let long_filename = "a".repeat(MAX_FILENAME_LENGTH + 50);
        let result = truncate_filename_to_limit(&long_filename);
        assert_eq!(result.len(), MAX_FILENAME_LENGTH);
        assert_eq!(result, "a".repeat(MAX_FILENAME_LENGTH));
    }

    #[test]
    fn test_truncate_filename_with_extension() {
        let name_part = "a".repeat(MAX_FILENAME_LENGTH - 3); // Leave space for .log
        let long_filename = format!("{}.log", name_part);
        assert!(long_filename.len() > MAX_FILENAME_LENGTH);

        let result = truncate_filename_to_limit(&long_filename);
        assert!(result.len() <= MAX_FILENAME_LENGTH);
        assert!(result.ends_with(".log"));

        // Should truncate the name part but preserve extension
        let expected_name_len = MAX_FILENAME_LENGTH - 4; // 4 chars for ".log"
        let expected = format!("{}.log", "a".repeat(expected_name_len));
        assert_eq!(result, expected);
    }

    #[test]
    fn test_truncate_filename_very_long_extension() {
        let very_long_ext = ".".to_string() + &"x".repeat(MAX_FILENAME_LENGTH + 10);
        let filename = format!("test{}", very_long_ext);

        let result = truncate_filename_to_limit(&filename);
        assert_eq!(result.len(), MAX_FILENAME_LENGTH);
        // Should truncate the entire filename since extension is too long
        assert_eq!(result, filename[..MAX_FILENAME_LENGTH]);
    }

    #[test]
    fn test_sanitize_for_filename() {
        let problematic_command = "ls -la /path/with spaces & <special> chars*?";
        let result = sanitize_for_filename(problematic_command);

        // Should not contain problematic characters
        assert!(!result.contains('/'));
        assert!(!result.contains('<'));
        assert!(!result.contains('>'));
        assert!(!result.contains('*'));
        assert!(!result.contains('?'));
        assert!(!result.contains('&'));

        // Spaces should become dashes
        assert!(result.contains('-'));

        // Should preserve alphanumeric characters
        assert!(result.contains('l'));
        assert!(result.contains('s'));
        assert!(result.contains('a'));
    }

    #[test]
    fn test_command_filename_generation() {
        let timestamp = "20250812_121805_123";
        let long_command = "a".repeat(300); // Very long command
        let safe_command = sanitize_for_filename(&long_command);
        let filename_raw = format!("{timestamp}_{safe_command}.log");
        let filename = truncate_filename_to_limit(&filename_raw);

        assert!(filename.len() <= MAX_FILENAME_LENGTH);
        assert!(filename.starts_with(timestamp));
        assert!(filename.ends_with(".log"));
    }
}
