use crate::command_logger::CommandLogger;
use std::fs::File;
use std::io::{self, Read, Result, Write};

const BUFFER_SIZE: usize = 32 * 1024;

// Simple synchronous forwarding from input to output
pub fn forward_simple(mut input: File, mut output: File) -> Result<()> {
    log::debug!("Starting forward_simple I/O forwarding");
    let mut buffer = [0u8; BUFFER_SIZE];
    let mut total_bytes_forwarded = 0u64;

    loop {
        match input.read(&mut buffer) {
            Ok(0) => {
                log::debug!(
                    "forward_simple: EOF reached, total bytes forwarded: {}",
                    total_bytes_forwarded
                );
                break; // EOF
            }
            Ok(n) => {
                log::trace!("forward_simple: read {} bytes", n);
                output.write_all(&buffer[..n])?;
                output.flush()?;
                total_bytes_forwarded += n as u64;
                if total_bytes_forwarded % (1024 * 1024) == 0 {
                    log::debug!(
                        "forward_simple: forwarded {} MB",
                        total_bytes_forwarded / (1024 * 1024)
                    );
                }
            }
            Err(e) => {
                if e.kind() == io::ErrorKind::Interrupted {
                    log::trace!("forward_simple: interrupted, continuing");
                    continue;
                }
                log::error!(
                    "forward_simple: I/O error after {} bytes: {}",
                    total_bytes_forwarded,
                    e
                );
                return Err(e);
            }
        }
    }
    log::info!(
        "forward_simple completed: {} bytes forwarded",
        total_bytes_forwarded
    );
    Ok(())
}

// Tee functionality - read from input and write to both outputs
pub fn tee_simple(mut input: File, mut output1: File, mut output2: File) -> Result<()> {
    log::debug!("Starting tee_simple I/O forwarding (1 input -> 2 outputs)");
    let mut buffer = [0u8; BUFFER_SIZE];
    let mut total_bytes_teed = 0u64;
    let mut output1_errors = 0u32;
    let mut output2_errors = 0u32;

    loop {
        match input.read(&mut buffer) {
            Ok(0) => {
                log::debug!("tee_simple: EOF reached, total bytes teed: {}, output1_errors: {}, output2_errors: {}", 
                           total_bytes_teed, output1_errors, output2_errors);
                break; // EOF
            }
            Ok(n) => {
                log::trace!("tee_simple: read {} bytes", n);
                let data = &buffer[..n];

                // Write to first output (typically stdout)
                if let Err(e) = output1.write_all(data) {
                    output1_errors += 1;
                    log::warn!(
                        "tee_simple: Failed to write to output1 (error #{}): {}",
                        output1_errors,
                        e
                    );
                    eprintln!("Warning: Failed to write to output1: {e}");
                }
                if let Err(e) = output1.flush() {
                    output1_errors += 1;
                    log::warn!(
                        "tee_simple: Failed to flush output1 (error #{}): {}",
                        output1_errors,
                        e
                    );
                    eprintln!("Warning: Failed to flush output1: {e}");
                }

                // Write to second output (typically logger pipe)
                if let Err(e) = output2.write_all(data) {
                    output2_errors += 1;
                    log::warn!(
                        "tee_simple: Failed to write to output2 (error #{}): {}",
                        output2_errors,
                        e
                    );
                    eprintln!("Warning: Failed to write to output2: {e}");
                }
                if let Err(e) = output2.flush() {
                    output2_errors += 1;
                    log::warn!(
                        "tee_simple: Failed to flush output2 (error #{}): {}",
                        output2_errors,
                        e
                    );
                    eprintln!("Warning: Failed to flush output2: {e}");
                }

                total_bytes_teed += n as u64;
                if total_bytes_teed % (1024 * 1024) == 0 {
                    log::debug!("tee_simple: teed {} MB", total_bytes_teed / (1024 * 1024));
                }
            }
            Err(e) => {
                if e.kind() == io::ErrorKind::Interrupted {
                    log::trace!("tee_simple: interrupted, continuing");
                    continue;
                }
                log::error!(
                    "tee_simple: I/O error after {} bytes: {}",
                    total_bytes_teed,
                    e
                );
                return Err(e);
            }
        }
    }
    log::info!(
        "tee_simple completed: {} bytes teed, {} output1 errors, {} output2 errors",
        total_bytes_teed,
        output1_errors,
        output2_errors
    );
    Ok(())
}

// Command-aware input forwarding with command detection
pub fn forward_with_command_detection(
    mut input: File,
    mut output: File,
    command_logger: &CommandLogger,
) -> Result<()> {
    log::debug!("Starting command-aware input forwarding");
    let mut buffer = [0u8; BUFFER_SIZE];
    let mut total_bytes_forwarded = 0u64;
    let mut line_buffer = Vec::new();
    
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
                
                // Process data byte by byte to detect newlines (commands)
                for &byte in data {
                    line_buffer.push(byte);
                    
                    // On newline, we consider this a new command
                    if byte == b'\n' {
                        // Skip empty lines
                        let line_content: Vec<u8> = line_buffer.iter()
                            .filter(|&&b| b != b'\r' && b != b'\n')
                            .copied()
                            .collect();
                            
                        if !line_content.is_empty() {
                            log::debug!("Detected new command, creating new log file");
                            if let Err(e) = command_logger.start_new_command() {
                                log::error!("Failed to start new command log: {}", e);
                            }
                        }
                        
                        line_buffer.clear();
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
                    log::debug!("tee_with_command_logging: teed {} MB", total_bytes_teed / (1024 * 1024));
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
