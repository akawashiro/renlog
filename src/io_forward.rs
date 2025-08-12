use crate::command_logger;
use crate::command_markers::{self, END_COMMAND_NAME, START_COMMAND_NAME};
use std::fs::File;
use std::io::{self, Read, Result, Write};

const BUFFER_SIZE: usize = 32 * 1024;

// Command-aware input forwarding with command marker detection
pub fn forward_with_command_detection(mut input: File, mut output: File) -> Result<()> {
    log::debug!("Starting command-aware input forwarding with marker detection");
    let mut buffer = [0u8; BUFFER_SIZE];
    let mut total_bytes_forwarded = 0u64;
    let mut data_buffer = Vec::new();

    loop {
        match input.read(&mut buffer) {
            Ok(0) => {
                log::debug!(
                    "forward_with_command_detection: EOF reached, total bytes forwarded: {}",
                    total_bytes_forwarded
                );
                break; // EOF
            }
            Ok(n) => {
                log::trace!("forward_with_command_detection: read {} bytes", n);
                let data = &buffer[..n];

                // Write data to output immediately
                output.write_all(data)?;
                output.flush()?;

                // Add data to our buffer for marker detection
                data_buffer.extend_from_slice(data);

                // Try to decode command markers from the accumulated data
                let (success, prefix, args) = command_markers::try_decode_bytes(&data_buffer);

                if success {
                    log::debug!("Detected command marker with args: {:?}", args);

                    // Process the command markers but don't do any logging
                    // The unified_pty_reader handles all command logging
                    if !args.is_empty() {
                        if args[0] == END_COMMAND_NAME {
                            let exit_status = if args.len() > 1 {
                                args[1].parse::<i32>().ok()
                            } else {
                                None
                            };
                            log::info!(
                                "Detected end command marker with exit status: {:?}",
                                exit_status
                            );
                        } else if args[0] == START_COMMAND_NAME {
                            let command_str = if args.len() > 1 {
                                args[1..].join(" ")
                            } else {
                                "unknown".to_string()
                            };
                            log::info!("Detected start command marker for: {}", command_str);
                        }
                    }

                    // Keep any remaining data after the command marker for next iteration
                    let marker_and_command_len = prefix.len()
                        + command_markers::COMMAND_MARKER.len()
                        + args.join(" ").len()
                        + 1; // +1 for newline

                    if marker_and_command_len < data_buffer.len() {
                        data_buffer = data_buffer[marker_and_command_len..].to_vec();
                    } else {
                        data_buffer.clear();
                    }
                } else {
                    // No marker found, keep last part of buffer in case marker spans reads
                    let marker_len = command_markers::COMMAND_MARKER.len();
                    if data_buffer.len() > marker_len {
                        data_buffer = data_buffer[(data_buffer.len() - marker_len)..].to_vec();
                    }
                }

                total_bytes_forwarded += n as u64;
                if total_bytes_forwarded % (1024 * 1024) == 0 {
                    log::debug!(
                        "forward_with_command_detection: forwarded {} MB",
                        total_bytes_forwarded / (1024 * 1024)
                    );
                }
            }
            Err(e) => {
                if e.kind() == io::ErrorKind::Interrupted {
                    log::trace!("forward_with_command_detection: interrupted, continuing");
                    continue;
                }
                log::error!(
                    "forward_with_command_detection: I/O error after {} bytes: {}",
                    total_bytes_forwarded,
                    e
                );
                return Err(e);
            }
        }
    }

    log::info!(
        "forward_with_command_detection completed: {} bytes forwarded",
        total_bytes_forwarded
    );
    Ok(())
}

// Unified PTY reader that handles both output forwarding and command detection/logging
pub fn unified_pty_reader(
    mut input: File,
    mut stdout: File,
    mut forward_pipe: File,
    log_dir: &str,
) -> Result<()> {
    log::debug!("Starting unified PTY reader with command detection and buffering");
    let mut buffer = [0u8; BUFFER_SIZE];
    let mut total_bytes_read = 0u64;
    let mut stdout_errors = 0u32;
    let mut forward_pipe_errors = 0u32;

    // Buffer for marker detection
    let mut data_buffer = Vec::new();
    // Buffer for current command output
    let mut command_buffer = Vec::new();
    let mut current_command: Option<String> = None;

    loop {
        match input.read(&mut buffer) {
            Ok(0) => {
                log::debug!(
                    "unified_pty_reader: EOF reached, total bytes read: {}, stdout_errors: {}, forward_pipe_errors: {}",
                    total_bytes_read, stdout_errors, forward_pipe_errors
                );

                // Save any remaining buffered command data
                if !command_buffer.is_empty() {
                    if let Some(ref cmd) = current_command {
                        log::info!("Saving incomplete command buffer for: {}", cmd);
                        match command_logger::save_command_log(log_dir, cmd, &command_buffer, None)
                        {
                            Ok(log_file_path) => {
                                log::info!("Saved incomplete command log to: {}", log_file_path);
                            }
                            Err(e) => {
                                log::error!("Failed to save incomplete command log: {}", e);
                            }
                        }
                    }
                }
                break;
            }
            Ok(n) => {
                log::trace!("unified_pty_reader: read {} bytes", n);
                let data = &buffer[..n];
                log::debug!("unified_pty_reader: processing {} bytes of data", n);

                // Write to stdout immediately
                if let Err(e) = stdout.write_all(data) {
                    stdout_errors += 1;
                    log::warn!(
                        "unified_pty_reader: Failed to write to stdout (error #{}): {}",
                        stdout_errors,
                        e
                    );
                }
                if let Err(e) = stdout.flush() {
                    stdout_errors += 1;
                    log::warn!(
                        "unified_pty_reader: Failed to flush stdout (error #{}): {}",
                        stdout_errors,
                        e
                    );
                }

                // Write to forward pipe immediately
                if let Err(e) = forward_pipe.write_all(data) {
                    forward_pipe_errors += 1;
                    log::warn!(
                        "unified_pty_reader: Failed to write to forward pipe (error #{}): {}",
                        forward_pipe_errors,
                        e
                    );
                }
                if let Err(e) = forward_pipe.flush() {
                    forward_pipe_errors += 1;
                    log::warn!(
                        "unified_pty_reader: Failed to flush forward pipe (error #{}): {}",
                        forward_pipe_errors,
                        e
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
                    "unified_pty_reader: checking buffer for markers, buffer size: {}",
                    data_buffer.len()
                );

                // Look for start command marker
                if let Some(start_pos) = data_str.find(START_COMMAND_NAME) {
                    if let Some(newline_pos) = data_str[start_pos..].find('\n') {
                        let full_line = &data_str[start_pos..start_pos + newline_pos];
                        let parts: Vec<&str> = full_line.split_whitespace().collect();

                        // Save previous command if exists
                        if let Some(ref prev_cmd) = current_command {
                            log::info!("Saving previous command log: {}", prev_cmd);
                            match command_logger::save_command_log(
                                log_dir,
                                prev_cmd,
                                &command_buffer,
                                None,
                            ) {
                                Ok(log_file_path) => {
                                    log::info!("Saved previous command log to: {}", log_file_path);
                                }
                                Err(e) => {
                                    log::error!("Failed to save previous command log: {}", e);
                                }
                            }
                        }

                        // Start new command
                        let command_str = if parts.len() > 1 {
                            parts[1..].join(" ")
                        } else {
                            "unknown".to_string()
                        };
                        log::info!("Starting new command buffer for: {}", command_str);
                        current_command = Some(command_str);
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

                        if let Some(ref cmd) = current_command {
                            log::info!(
                                "Ending command '{}' with exit status: {:?}",
                                cmd,
                                exit_status
                            );
                            match command_logger::save_command_log(
                                log_dir,
                                cmd,
                                &command_buffer,
                                exit_status,
                            ) {
                                Ok(log_file_path) => {
                                    log::info!("Saved command log to: {}", log_file_path);
                                }
                                Err(e) => {
                                    log::error!("Failed to save command log: {}", e);
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
                        "unified_pty_reader: read {} MB",
                        total_bytes_read / (1024 * 1024)
                    );
                }
            }
            Err(e) => {
                if e.kind() == io::ErrorKind::Interrupted {
                    log::trace!("unified_pty_reader: interrupted, continuing");
                    continue;
                }
                log::error!(
                    "unified_pty_reader: I/O error after {} bytes: {}",
                    total_bytes_read,
                    e
                );
                return Err(e);
            }
        }
    }

    log::info!(
        "unified_pty_reader completed: {} bytes read, {} stdout errors, {} forward pipe errors",
        total_bytes_read,
        stdout_errors,
        forward_pipe_errors
    );
    Ok(())
}
