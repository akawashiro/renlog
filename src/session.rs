use crate::command_logger;
use crate::fifo::FifoPair;
use crate::io_forward::{forward_with_command_detection, unified_pty_reader};
use crate::pty::{restore_termios, set_raw_mode, PtyPair};

use nix::sys::termios::Termios;
use nix::unistd;
use std::env;
use std::fs::File;
use std::io::{self, Result};
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::process::{Child, Command, Stdio};
use std::thread;

pub struct Session {
    pty: PtyPair,
    fifos: FifoPair,
    child: Option<Child>,
    original_termios: Option<Termios>,
    pid: u32,
    renlog_dir: String,
}

impl Session {
    pub fn new(renlog_dir: &str) -> Result<Self> {
        log::debug!("Creating new Session instance");
        let pid = std::process::id();
        log::info!(
            "Session initializing with PID={} renlog_dir={}",
            pid,
            renlog_dir
        );

        // Create the log directory if it doesn't exist
        std::fs::create_dir_all(renlog_dir).map_err(|e| {
            log::error!(
                "Failed to create renlog log directory '{}': {}",
                renlog_dir,
                e
            );
            io::Error::other(format!(
                "Failed to create renlog log directory '{}': {}",
                renlog_dir, e
            ))
        })?;

        log::debug!("Creating PTY pair");
        let pty = PtyPair::new()?;

        log::debug!("Creating FIFO pair");
        let mut fifos = FifoPair::new(renlog_dir, pid)?;
        fifos.open()?;
        log::info!(
            "FIFO pipes created and opened: forward={}, reverse={}",
            fifos.forward_path_str(),
            fifos.reverse_path_str()
        );

        // Set stdin to raw mode (only if it's a TTY)
        log::debug!("Checking if stdin is a TTY");
        let original_termios = if nix::unistd::isatty(libc::STDIN_FILENO).unwrap_or(false) {
            log::debug!("Setting stdin to raw mode");
            Some(set_raw_mode(libc::STDIN_FILENO).map_err(|e| {
                log::error!("Failed to set raw mode: {}", e);
                io::Error::other(format!("Failed to set raw mode: {e}"))
            })?)
        } else {
            log::info!("Stdin is not a TTY, skipping raw mode setup");
            None
        };
        if original_termios.is_some() {
            log::info!("Terminal set to raw mode successfully");
        }

        println!("Renlog starting... [PID={pid}]");

        log::debug!("Ensuring command log directory exists");
        command_logger::ensure_log_directory(renlog_dir)?;

        log::info!("Session instance created successfully");
        Ok(Session {
            pty,
            fifos,
            child: None,
            original_termios,
            pid,
            renlog_dir: renlog_dir.to_string(),
        })
    }

    pub fn start_child(&mut self) -> Result<()> {
        log::debug!("Starting child process");
        // Set up environment variables for the child
        let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        log::info!("Using shell: {}", shell);

        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c");
        cmd.arg(&shell);

        // Set environment variables that the shell integration would use
        cmd.env("RENLOG_PID", self.pid.to_string());
        cmd.env("RENLOG_DIR", &self.renlog_dir);
        cmd.env("RENLOG_LOGGER_OUT", self.fifos.reverse_path_str());
        
        // Set up the current log file marker path for shell integration
        let current_log_marker = format!("{}/.renlog_current_log", &self.renlog_dir);
        cmd.env("RENLOG_LOG_FILE", &current_log_marker);
        
        log::debug!(
            "Set environment variables: RENLOG_PID={}, RENLOG_DIR={}, RENLOG_LOGGER_OUT={}, RENLOG_LOG_FILE={}",
            self.pid,
            &self.renlog_dir,
            self.fifos.reverse_path_str(),
            &current_log_marker
        );

        // Set up stdio to use the PTY slave (duplicate fd for each stdio stream)
        let slave_fd = self.pty.slave_fd();
        let stdin_fd = unistd::dup(slave_fd).map_err(|e| {
            log::error!("Failed to dup slave fd for stdin: {}", e);
            io::Error::other(format!("Failed to dup slave fd for stdin: {e}"))
        })?;
        let stdout_fd = unistd::dup(slave_fd).map_err(|e| {
            log::error!("Failed to dup slave fd for stdout: {}", e);
            io::Error::other(format!("Failed to dup slave fd for stdout: {e}"))
        })?;
        let stderr_fd = unistd::dup(slave_fd).map_err(|e| {
            log::error!("Failed to dup slave fd for stderr: {}", e);
            io::Error::other(format!("Failed to dup slave fd for stderr: {e}"))
        })?;

        cmd.stdin(unsafe { Stdio::from_raw_fd(stdin_fd) });
        cmd.stdout(unsafe { Stdio::from_raw_fd(stdout_fd) });
        cmd.stderr(unsafe { Stdio::from_raw_fd(stderr_fd) });
        log::debug!(
            "Configured child stdio to use duplicated PTY slave fds: stdin={}, stdout={}, stderr={}",
            stdin_fd,
            stdout_fd,
            stderr_fd
        );

        let child = cmd.spawn().map_err(|e| {
            log::error!("Failed to spawn child process: {}", e);
            e
        })?;
        let child_pid = child.id();
        self.child = Some(child);

        log::info!("Child process started with PID: {}", child_pid);
        println!("Child process started with PID: {}", child_pid);
        Ok(())
    }

    pub fn start_forwarders(&self) -> Result<()> {
        log::debug!("Starting I/O forwarders");
        // Get raw file descriptors
        let stdin_fd = libc::STDIN_FILENO;
        let stdout_fd = libc::STDOUT_FILENO;
        let master_fd = self.pty.master_fd();
        let forward_pipe_fd = self
            .fifos
            .forward_file
            .as_ref()
            .ok_or_else(|| {
                log::error!("Forward pipe not opened");
                io::Error::other("Forward pipe not opened")
            })?
            .as_raw_fd();

        log::debug!(
            "File descriptors - stdin: {}, stdout: {}, master: {}, forward_pipe: {}",
            stdin_fd,
            stdout_fd,
            master_fd,
            forward_pipe_fd
        );

        // Create separate owned file descriptors by duplicating them
        let stdin_dup = unistd::dup(stdin_fd).map_err(|e| {
            log::error!("Failed to dup stdin: {}", e);
            io::Error::other(format!("Failed to dup stdin: {e}"))
        })?;
        let stdout_dup = unistd::dup(stdout_fd).map_err(|e| {
            log::error!("Failed to dup stdout: {}", e);
            io::Error::other(format!("Failed to dup stdout: {e}"))
        })?;
        let master_dup1 = unistd::dup(master_fd).map_err(|e| {
            log::error!("Failed to dup master1: {}", e);
            io::Error::other(format!("Failed to dup master1: {e}"))
        })?;
        let master_dup2 = unistd::dup(master_fd).map_err(|e| {
            log::error!("Failed to dup master2: {}", e);
            io::Error::other(format!("Failed to dup master2: {e}"))
        })?;
        let forward_dup = unistd::dup(forward_pipe_fd).map_err(|e| {
            log::error!("Failed to dup forward pipe: {}", e);
            io::Error::other(format!("Failed to dup forward pipe: {e}"))
        })?;

        log::debug!("All file descriptors duplicated successfully");

        // Create File instances from the duplicated descriptors
        let stdin = unsafe { File::from_raw_fd(stdin_dup) };
        let stdout = unsafe { File::from_raw_fd(stdout_dup) };
        let stdin_master = unsafe { File::from_raw_fd(master_dup1) };
        let master_for_output = unsafe { File::from_raw_fd(master_dup2) };
        let forward_pipe_clone = unsafe { File::from_raw_fd(forward_dup) };

        // Clone necessary data for the output thread
        let renlog_dir_for_output = self.renlog_dir.clone();

        // Forward stdin to PTY master with command detection
        log::debug!("Starting stdin->master forwarder thread with command detection");
        thread::spawn(move || {
            log::debug!("Command-aware stdin forwarder thread started");
            if let Err(e) = forward_with_command_detection(stdin, stdin_master) {
                log::error!("Error in command-aware stdin forwarding: {}", e);
                eprintln!("Error in command-aware stdin forwarding: {e}");
            }
            log::debug!("Command-aware stdin forwarder thread ending");
        });

        // Use unified PTY reader for output forwarding and command detection/logging
        log::debug!("Starting unified PTY reader thread");
        thread::spawn(move || {
            log::debug!("Unified PTY reader thread started");
            if let Err(e) = unified_pty_reader(
                master_for_output,
                stdout,
                forward_pipe_clone,
                &renlog_dir_for_output,
            ) {
                log::error!("Error in unified PTY reader: {}", e);
                eprintln!("Error in unified PTY reader: {e}");
            }
            log::debug!("Unified PTY reader thread ending");
        });

        log::info!("I/O forwarders started successfully");
        Ok(())
    }

    pub fn wait_for_child(&mut self) -> Result<i32> {
        log::debug!("Waiting for child process to exit");
        if let Some(mut child) = self.child.take() {
            match child.wait() {
                Ok(status) => {
                    let exit_code = status.code().unwrap_or(-1);
                    log::info!("Child process exited with code: {}", exit_code);
                    println!("Child process exited with code: {exit_code}");
                    Ok(exit_code)
                }
                Err(e) => {
                    log::error!("Error waiting for child process: {}", e);
                    Err(e)
                }
            }
        } else {
            log::warn!("wait_for_child called but no child process exists");
            Ok(0)
        }
    }

    pub fn get_master_fd(&self) -> RawFd {
        self.pty.master_fd()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        log::debug!("Session Drop trait called - starting cleanup");

        // Restore original terminal settings
        if let Some(termios) = &self.original_termios {
            log::debug!("Restoring original terminal settings");
            if let Err(e) = restore_termios(libc::STDIN_FILENO, termios) {
                log::error!("Failed to restore terminal settings: {}", e);
            } else {
                log::debug!("Terminal settings restored successfully");
            }
        }

        // Kill child process if still running
        if let Some(mut child) = self.child.take() {
            log::debug!("Killing child process with PID: {}", child.id());
            if let Err(e) = child.kill() {
                log::error!("Failed to kill child process: {}", e);
            }
            if let Err(e) = child.wait() {
                log::error!("Failed to wait for killed child process: {}", e);
            } else {
                log::debug!("Child process cleanup completed");
            }
        }

        log::info!("Session cleanup completed");
        println!("Session cleanup completed");
    }
}
