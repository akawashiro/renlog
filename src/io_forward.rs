use crate::command_logger;
use crate::shell_config::{END_COMMAND_NAME, START_COMMAND_NAME};
use std::fs::File;
use std::io::{self, Read, Result, Write};

const BUFFER_SIZE: usize = 32 * 1024;

// Forward input from stdin to PTY master without command detection
pub fn forward_stdin_to_pty(mut input: File, mut output: File) -> Result<()> {
    log::debug!("Starting stdin to PTY forwarding");
    let mut buffer = [0u8; BUFFER_SIZE];
    let mut total_bytes_forwarded = 0u64;

    loop {
        match input.read(&mut buffer) {
            Ok(0) => {
                log::debug!(
                    "forward_stdin_to_pty: EOF reached, total bytes forwarded: {total_bytes_forwarded}"
                );
                break; // EOF
            }
            Ok(n) => {
                log::trace!("forward_stdin_to_pty: read {n} bytes");
                let data = &buffer[..n];

                // Write data to output immediately
                output.write_all(data)?;
                output.flush()?;

                total_bytes_forwarded += n as u64;
                if total_bytes_forwarded % (1024 * 1024) == 0 {
                    log::debug!(
                        "forward_stdin_to_pty: forwarded {} MB",
                        total_bytes_forwarded / (1024 * 1024)
                    );
                }
            }
            Err(e) => {
                if e.kind() == io::ErrorKind::Interrupted {
                    log::trace!("forward_stdin_to_pty: interrupted, continuing");
                    continue;
                }
                log::error!(
                    "forward_stdin_to_pty: I/O error after {total_bytes_forwarded} bytes: {e}"
                );
                return Err(e);
            }
        }
    }

    log::info!("forward_stdin_to_pty completed: {total_bytes_forwarded} bytes forwarded");
    Ok(())
}

fn should_skip_saving(command: &str) -> bool {
    let skip_command_prefix = vec!["nvim", "pass"];
    for prefix in skip_command_prefix {
        if command.starts_with(prefix) {
            return true;
        }
    }
    false
}

// Forward output from PTY master to stdout with command detection and logging
pub fn forward_pty_to_stdout_with_logging(
    mut input: File,
    mut stdout: File,
    log_dir: &str,
    pid: u32,
) -> Result<()> {
    log::debug!("Starting PTY to stdout forwarding with command detection and logging");
    let mut buffer = [0u8; BUFFER_SIZE];
    let mut total_bytes_read = 0u64;
    let mut stdout_errors = 0u32;

    // Buffer for marker detection
    let mut data_buffer = Vec::new();
    // Buffer for current command output
    let mut command_buffer = Vec::new();

    // (Command name, should_skip_saving)
    let mut current_command: Option<(String, bool)> = None;

    loop {
        match input.read(&mut buffer) {
            Ok(0) => {
                log::debug!(
                    "forward_pty_to_stdout: EOF reached, total bytes read: {total_bytes_read}, stdout_errors: {stdout_errors}"
                );

                // Save any remaining buffered command data
                if !command_buffer.is_empty() {
                    if let Some((ref cmd, false)) = current_command {
                        log::info!("Saving incomplete command buffer for: {cmd}");
                        match command_logger::save_command_log(
                            log_dir,
                            cmd,
                            &command_buffer,
                            None,
                            pid,
                        ) {
                            Ok(log_file_path) => {
                                log::info!("Saved incomplete command log to: {log_file_path}");
                            }
                            Err(e) => {
                                log::error!("Failed to save incomplete command log: {e}");
                            }
                        }
                    }
                }
                break;
            }
            Ok(n) => {
                log::trace!("forward_pty_to_stdout: read {n} bytes");
                let data = &buffer[..n];
                log::debug!("forward_pty_to_stdout: processing {n} bytes of data");

                // Write to stdout immediately
                if let Err(e) = stdout.write_all(data) {
                    stdout_errors += 1;
                    log::warn!(
                        "forward_pty_to_stdout: Failed to write to stdout (error #{stdout_errors}): {e}"
                    );
                }
                if let Err(e) = stdout.flush() {
                    stdout_errors += 1;
                    log::warn!(
                        "forward_pty_to_stdout: Failed to flush stdout (error #{stdout_errors}): {e}"
                    );
                }

                // Add to command buffer if we have an active command
                if current_command.is_some() {
                    command_buffer.extend_from_slice(data);
                }

                // Add data to marker detection buffer
                data_buffer.extend_from_slice(data);

                // Look for plain text command markers
                let data_str = String::from_utf8_lossy(&data_buffer);
                log::trace!(
                    "forward_pty_to_stdout: checking buffer for markers, buffer size: {}",
                    data_buffer.len()
                );

                // Look for start command marker
                if let Some(start_pos) = data_str.find(START_COMMAND_NAME) {
                    if let Some(newline_pos) = data_str[start_pos..].find('\n') {
                        let full_line = &data_str[start_pos..start_pos + newline_pos];
                        let parts: Vec<&str> = full_line.split_whitespace().collect();

                        // Save previous command if exists
                        if let Some((ref prev_cmd, false)) = current_command {
                            log::info!("Saving previous command log: {prev_cmd}");
                            match command_logger::save_command_log(
                                log_dir,
                                prev_cmd,
                                &command_buffer,
                                None,
                                pid,
                            ) {
                                Ok(log_file_path) => {
                                    log::info!("Saved previous command log to: {log_file_path}");
                                }
                                Err(e) => {
                                    log::error!("Failed to save previous command log: {e}");
                                }
                            }
                        }

                        // Start new command
                        let command_str = if parts.len() > 1 {
                            parts[1..].join(" ")
                        } else {
                            "unknown".to_string()
                        };
                        log::info!("Starting new command buffer for: {command_str}");
                        let should_skip_saving = should_skip_saving(&command_str);
                        current_command = Some((command_str, should_skip_saving));
                        command_buffer.clear();

                        // Remove processed part from buffer
                        let bytes_processed = start_pos + newline_pos + 1;
                        if bytes_processed < data_buffer.len() {
                            data_buffer = data_buffer[bytes_processed..].to_vec();
                        } else {
                            data_buffer.clear();
                        }
                        continue;
                    }
                }

                // Look for end command marker
                if let Some(end_pos) = data_str.find(END_COMMAND_NAME) {
                    if let Some(newline_pos) = data_str[end_pos..].find('\n') {
                        let full_line = &data_str[end_pos..end_pos + newline_pos];
                        let parts: Vec<&str> = full_line.split_whitespace().collect();

                        let exit_status = if parts.len() > 1 {
                            parts[1].parse::<i32>().ok()
                        } else {
                            None
                        };

                        if let Some((ref cmd, false)) = current_command {
                            log::info!("Ending command '{cmd}' with exit status: {exit_status:?}");
                            match command_logger::save_command_log(
                                log_dir,
                                cmd,
                                &command_buffer,
                                exit_status,
                                pid,
                            ) {
                                Ok(log_file_path) => {
                                    log::info!("Saved command log to: {log_file_path}");
                                }
                                Err(e) => {
                                    log::error!("Failed to save command log: {e}");
                                }
                            }
                        }

                        current_command = None;
                        command_buffer.clear();

                        // Remove processed part from buffer
                        let bytes_processed = end_pos + newline_pos + 1;
                        if bytes_processed < data_buffer.len() {
                            data_buffer = data_buffer[bytes_processed..].to_vec();
                        } else {
                            data_buffer.clear();
                        }
                        continue;
                    }
                }

                // Keep buffer size manageable - keep only last 1KB for marker detection
                if data_buffer.len() > 1024 {
                    data_buffer = data_buffer[(data_buffer.len() - 1024)..].to_vec();
                }

                total_bytes_read += n as u64;
                if total_bytes_read % (1024 * 1024) == 0 {
                    log::debug!(
                        "forward_pty_to_stdout: read {} MB",
                        total_bytes_read / (1024 * 1024)
                    );
                }
            }
            Err(e) => {
                if e.kind() == io::ErrorKind::Interrupted {
                    log::trace!("forward_pty_to_stdout: interrupted, continuing");
                    continue;
                }
                log::error!("forward_pty_to_stdout: I/O error after {total_bytes_read} bytes: {e}");
                return Err(e);
            }
        }
    }

    log::info!(
        "forward_pty_to_stdout_with_logging completed: {total_bytes_read} bytes read, {stdout_errors} stdout errors"
    );
    Ok(())
}
