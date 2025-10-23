use crate::log_processor;
use crate::shell_config::LAST_LOG_FILE;
use crate::symlink_utils::create_or_update_symlink;
use std::fs::File;
use std::io::{Result, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

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

fn create_latest_symlink(log_dir: &Path, filename: &str) -> Result<()> {
    let latest_path = log_dir.join("latest.log");
    create_or_update_symlink(filename, &latest_path)?;
    log::info!("Created latest.log symlink pointing to: {filename}");
    Ok(())
}

/// Update the current log file tracking file (PID-specific to avoid conflicts)
fn update_current_log_file(log_dir: &Path, log_file_path: &str, pid: u32) -> Result<()> {
    let current_log_marker = log_dir.join(LAST_LOG_FILE);
    std::fs::write(&current_log_marker, log_file_path).map_err(|e| {
        log::warn!("Failed to update current log file marker: {e}");
        e
    })?;
    log::debug!("Updated current log file marker for PID {pid}: {log_file_path}");
    Ok(())
}

/// Apply log filter command to the log file
fn apply_log_filter(log_file_path: &Path, filter_cmd: &str) -> Result<()> {
    log::info!(
        "Applying log filter '{}' to file: {}",
        filter_cmd,
        log_file_path.display()
    );

    // Create a temporary file in the same directory
    let temp_file_path = log_file_path.with_extension("log.tmp");

    // Build the command: cat log_file | filter_cmd > temp_file
    let full_command = format!(
        "cat '{}' | {} > '{}'",
        log_file_path.display(),
        filter_cmd,
        temp_file_path.display()
    );

    log::debug!("Executing filter command: {full_command}");

    let status = Command::new("sh")
        .arg("-c")
        .arg(&full_command)
        .status()
        .map_err(|e| {
            log::error!("Failed to execute filter command: {e}");
            std::io::Error::other(format!("Failed to execute filter command: {e}"))
        })?;

    if !status.success() {
        log::error!("Filter command failed with exit code: {:?}", status.code());
        // Clean up temporary file if it exists
        let _ = std::fs::remove_file(&temp_file_path);
        return Err(std::io::Error::other(format!(
            "Filter command failed with exit code: {:?}",
            status.code()
        )));
    }

    // Check if temp file was created
    if !temp_file_path.exists() {
        log::error!(
            "Temporary file was not created: {}",
            temp_file_path.display()
        );
        return Err(std::io::Error::other(format!(
            "Temporary file was not created: {}",
            temp_file_path.display()
        )));
    }

    // Replace the original file with the filtered file
    std::fs::rename(&temp_file_path, log_file_path).map_err(|e| {
        log::error!("Failed to replace log file with filtered version: {e}");
        // Try to clean up temporary file
        let _ = std::fs::remove_file(&temp_file_path);
        std::io::Error::other(format!(
            "Failed to replace log file with filtered version: {e}"
        ))
    })?;

    log::info!(
        "Successfully applied filter to log file: {}",
        log_file_path.display()
    );
    Ok(())
}

pub fn save_command_log(
    log_dir: &Path,
    command: &str,
    data: &[u8],
    exit_status: Option<i32>,
    pid: u32,
    log_filter_cmd: Option<&str>,
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

    // Apply log filter if specified
    if let Some(filter_cmd) = log_filter_cmd {
        if let Err(e) = apply_log_filter(&filepath, filter_cmd) {
            log::error!("Failed to apply log filter: {e}");
            // Don't fail the entire operation, just log the error
        }
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

    #[test]
    fn test_apply_log_filter_with_grep() {
        use std::io::Write;

        // Create a temporary directory with unique name
        let temp_dir = std::env::temp_dir().join(format!(
            "renlog_test_grep_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).unwrap();

        // Create a test log file
        let log_file = temp_dir.join("test.log");
        let mut file = File::create(&log_file).unwrap();
        writeln!(file, "KEEP_THIS line 1").unwrap();
        writeln!(file, "REMOVE_THIS line").unwrap();
        writeln!(file, "KEEP_THIS line 2").unwrap();
        file.flush().unwrap();
        drop(file);

        // Apply grep filter
        let result = apply_log_filter(&log_file, "grep KEEP_THIS");
        assert!(result.is_ok(), "Filter should succeed: {:?}", result.err());

        // Read the filtered content
        let content = std::fs::read_to_string(&log_file).unwrap();

        // Check that KEEP_THIS lines are present
        assert!(content.contains("KEEP_THIS line 1"));
        assert!(content.contains("KEEP_THIS line 2"));

        // Check that REMOVE_THIS line is NOT present
        assert!(!content.contains("REMOVE_THIS"));

        // Clean up
        std::fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn test_apply_log_filter_with_sed() {
        use std::io::Write;

        // Create a temporary directory with unique name
        let temp_dir = std::env::temp_dir().join(format!(
            "renlog_test_sed_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).unwrap();

        // Create a test log file
        let log_file = temp_dir.join("test_sed.log");
        let mut file = File::create(&log_file).unwrap();
        writeln!(file, "REPLACE_ME first").unwrap();
        writeln!(file, "REPLACE_ME second").unwrap();
        file.flush().unwrap();
        drop(file);

        // Apply sed filter
        let result = apply_log_filter(&log_file, "sed 's/REPLACE_ME/REPLACED/g'");
        assert!(result.is_ok(), "Filter should succeed: {:?}", result.err());

        // Read the filtered content
        let content = std::fs::read_to_string(&log_file).unwrap();

        // Check that text was replaced
        assert!(content.contains("REPLACED first"));
        assert!(content.contains("REPLACED second"));

        // Check that original text is NOT present
        assert!(!content.contains("REPLACE_ME"));

        // Clean up
        std::fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn test_apply_log_filter_invalid_command() {
        use std::io::Write;

        // Create a temporary directory with unique name
        let temp_dir = std::env::temp_dir().join(format!(
            "renlog_test_invalid_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).unwrap();

        // Create a test log file
        let log_file = temp_dir.join("test_invalid.log");
        let mut file = File::create(&log_file).unwrap();
        writeln!(file, "test content").unwrap();
        file.flush().unwrap();
        drop(file);

        // Get the original content
        let original_content = std::fs::read_to_string(&log_file).unwrap();

        // Apply invalid filter command
        let result = apply_log_filter(&log_file, "nonexistent_command_xyz");
        assert!(result.is_err(), "Filter should fail with invalid command");

        // Verify original file still exists and has original content
        let content = std::fs::read_to_string(&log_file).unwrap();
        assert_eq!(
            content, original_content,
            "Original file should be unchanged"
        );

        // Clean up
        std::fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn test_apply_log_filter_pipeline() {
        use std::io::Write;

        // Create a temporary directory with unique name
        let temp_dir = std::env::temp_dir().join(format!(
            "renlog_test_pipeline_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).unwrap();

        // Create a test log file
        let log_file = temp_dir.join("test_pipeline.log");
        let mut file = File::create(&log_file).unwrap();
        writeln!(file, "KEEP_THIS first").unwrap();
        writeln!(file, "REMOVE_THIS line").unwrap();
        writeln!(file, "KEEP_THIS second").unwrap();
        file.flush().unwrap();
        drop(file);

        // Apply pipeline filter: grep then sed
        let result = apply_log_filter(&log_file, "grep KEEP_THIS | sed 's/KEEP_THIS/KEPT/g'");
        assert!(result.is_ok(), "Filter should succeed: {:?}", result.err());

        // Read the filtered content
        let content = std::fs::read_to_string(&log_file).unwrap();

        // Check that text was both filtered and replaced
        assert!(content.contains("KEPT first"));
        assert!(content.contains("KEPT second"));
        assert!(!content.contains("KEEP_THIS"));
        assert!(!content.contains("REMOVE_THIS"));

        // Clean up
        std::fs::remove_dir_all(&temp_dir).unwrap();
    }
}
