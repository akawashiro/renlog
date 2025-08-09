use crate::command_logger::CommandLogger;
use crate::command_markers;
use std::fs::File;
use std::io::{self, Read, Result, Write};

const BUFFER_SIZE: usize = 32 * 1024;



// Command-aware input forwarding with command marker detection
pub fn forward_with_command_detection(
    mut input: File,
    mut output: File,
    command_logger: &CommandLogger,
) -> Result<()> {
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
                    
                    // Process the command
                    if !args.is_empty() {
                        if args[0] == "end-command" {
                            // Handle end-command marker
                            let exit_status = if args.len() > 1 {
                                args[1].parse::<i32>().ok()
                            } else {
                                None
                            };
                            log::info!("Ending command with exit status: {:?}", exit_status);
                            command_logger.end_command(exit_status);
                        } else {
                            // Handle regular command start marker
                            log::info!("Starting new command log for: {}", args.join(" "));
                            if let Err(e) = command_logger.start_new_command() {
                                log::error!("Failed to start new command log: {}", e);
                            }
                        }
                    }

                    // Keep any remaining data after the command marker for next iteration
                    let marker_and_command_len = prefix.len() + 
                        command_markers::COMMAND_MARKER.len() + 
                        args.join(" ").len() + 1; // +1 for newline
                    
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

// Command-aware tee functionality - read from input and write to outputs plus command log
pub fn tee_with_command_logging(
    mut input: File,
    mut output1: File,
    mut output2: File,
    command_logger: &CommandLogger,
) -> Result<()> {
    log::debug!("Starting command-aware tee forwarding");
    let mut buffer = [0u8; BUFFER_SIZE];
    let mut total_bytes_teed = 0u64;
    let mut output1_errors = 0u32;
    let mut output2_errors = 0u32;

    loop {
        match input.read(&mut buffer) {
            Ok(0) => {
                log::debug!("tee_with_command_logging: EOF reached, total bytes teed: {}, output1_errors: {}, output2_errors: {}", 
                           total_bytes_teed, output1_errors, output2_errors);
                command_logger.close_current_command();
                break; // EOF
            }
            Ok(n) => {
                log::trace!("tee_with_command_logging: read {} bytes", n);
                let data = &buffer[..n];

                // Write to first output (typically stdout)
                if let Err(e) = output1.write_all(data) {
                    output1_errors += 1;
                    log::warn!(
                        "tee_with_command_logging: Failed to write to output1 (error #{}): {}",
                        output1_errors,
                        e
                    );
                }
                if let Err(e) = output1.flush() {
                    output1_errors += 1;
                    log::warn!(
                        "tee_with_command_logging: Failed to flush output1 (error #{}): {}",
                        output1_errors,
                        e
                    );
                }

                // Write to second output (typically logger pipe)
                if let Err(e) = output2.write_all(data) {
                    output2_errors += 1;
                    log::warn!(
                        "tee_with_command_logging: Failed to write to output2 (error #{}): {}",
                        output2_errors,
                        e
                    );
                }
                if let Err(e) = output2.flush() {
                    output2_errors += 1;
                    log::warn!(
                        "tee_with_command_logging: Failed to flush output2 (error #{}): {}",
                        output2_errors,
                        e
                    );
                }

                // Log to command-specific file if we have an active command
                if let Err(e) = command_logger.log_command_output(data) {
                    log::warn!("Failed to log command output: {}", e);
                }

                total_bytes_teed += n as u64;
                if total_bytes_teed % (1024 * 1024) == 0 {
                    log::debug!(
                        "tee_with_command_logging: teed {} MB",
                        total_bytes_teed / (1024 * 1024)
                    );
                }
            }
            Err(e) => {
                if e.kind() == io::ErrorKind::Interrupted {
                    log::trace!("tee_with_command_logging: interrupted, continuing");
                    continue;
                }
                log::error!(
                    "tee_with_command_logging: I/O error after {} bytes: {}",
                    total_bytes_teed,
                    e
                );
                return Err(e);
            }
        }
    }

    log::info!(
        "tee_with_command_logging completed: {} bytes teed, {} output1 errors, {} output2 errors",
        total_bytes_teed,
        output1_errors,
        output2_errors
    );
    Ok(())
}
