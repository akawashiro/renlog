use crate::command_logger;
use crate::fifo::FifoPair;
use crate::io_forward::{simple_forward, unified_pty_reader};
use crate::pty::{restore_termios, set_raw_mode, PtyPair};

use nix::sys::termios::Termios;
use nix::unistd;
use std::env;
use std::fs::File;
use std::io::{self, Result};
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub struct Session {
    pty: PtyPair,
    fifos: FifoPair,
    child: Arc<Mutex<Option<Child>>>,
    original_termios: Option<Termios>,
    pid: u32,
    renlog_dir: String,
    exit_status: Arc<Mutex<Option<i32>>>,
    shutdown_flag: Arc<Mutex<bool>>,
    custom_cmd: Option<String>,
}

impl Session {
    pub fn new(renlog_dir: &str, custom_cmd: Option<&str>) -> Result<Self> {
        log::debug!("Creating new Session instance");
        let pid = std::process::id();
        log::info!(
            "Session initializing with PID={pid} renlog_dir={renlog_dir} custom_cmd={custom_cmd:?}"
        );

        // Create the log directory if it doesn't exist
        std::fs::create_dir_all(renlog_dir).map_err(|e| {
            log::error!("Failed to create renlog log directory '{renlog_dir}': {e}");
            io::Error::other(format!(
                "Failed to create renlog log directory '{renlog_dir}': {e}"
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
                log::error!("Failed to set raw mode: {e}");
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
            child: Arc::new(Mutex::new(None)),
            original_termios,
            pid,
            renlog_dir: renlog_dir.to_string(),
            exit_status: Arc::new(Mutex::new(None)),
            shutdown_flag: Arc::new(Mutex::new(false)),
            custom_cmd: custom_cmd.map(|s| s.to_string()),
        })
    }

    pub fn start_child(&mut self) -> Result<()> {
        log::debug!("Starting child process");

        let mut cmd = if let Some(custom_cmd) = &self.custom_cmd {
            // Parse the custom command
            let parts: Vec<&str> = custom_cmd.split_whitespace().collect();
            if parts.is_empty() {
                return Err(io::Error::other("Empty custom command provided"));
            }

            log::info!("Using custom command: {custom_cmd}");
            let mut cmd = Command::new(parts[0]);
            for arg in &parts[1..] {
                cmd.arg(arg);
            }
            cmd
        } else {
            // Use default shell from environment
            let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
            log::info!("Using shell: {shell}");

            let mut cmd = Command::new("/bin/sh");
            cmd.arg("-c");
            cmd.arg(&shell);
            cmd
        };

        // Set environment variables that the shell integration would use
        cmd.env("RENLOG_PID", self.pid.to_string());

        // Set up the current log file marker path for shell integration
        let current_log_marker = format!("{}/.renlog_current_log", &self.renlog_dir);
        cmd.env("RENLOG_LOG_FILE", &current_log_marker);

        log::debug!(
            "Set environment variables: RENLOG_PID={}, RENLOG_LOG_FILE={}",
            self.pid,
            &current_log_marker
        );

        // Set up stdio to use the PTY slave (duplicate fd for each stdio stream)
        let slave_fd = self.pty.slave_fd();
        let stdin_fd = unistd::dup(slave_fd).map_err(|e| {
            log::error!("Failed to dup slave fd for stdin: {e}");
            io::Error::other(format!("Failed to dup slave fd for stdin: {e}"))
        })?;
        let stdout_fd = unistd::dup(slave_fd).map_err(|e| {
            log::error!("Failed to dup slave fd for stdout: {e}");
            io::Error::other(format!("Failed to dup slave fd for stdout: {e}"))
        })?;
        let stderr_fd = unistd::dup(slave_fd).map_err(|e| {
            log::error!("Failed to dup slave fd for stderr: {e}");
            io::Error::other(format!("Failed to dup slave fd for stderr: {e}"))
        })?;

        cmd.stdin(unsafe { Stdio::from_raw_fd(stdin_fd) });
        cmd.stdout(unsafe { Stdio::from_raw_fd(stdout_fd) });
        cmd.stderr(unsafe { Stdio::from_raw_fd(stderr_fd) });

        // Set up process group handling for proper signal delivery
        unsafe {
            cmd.pre_exec(|| {
                // Create a new process group for the child process
                // This ensures proper signal handling and terminal control
                let result = libc::setsid();
                if result == -1 {
                    eprintln!(
                        "WARNING: Failed to create new session for child: errno {}",
                        std::io::Error::last_os_error()
                    );
                }
                Ok(())
            });
        }

        log::debug!(
            "Configured child stdio to use duplicated PTY slave fds: stdin={stdin_fd}, stdout={stdout_fd}, stderr={stderr_fd}"
        );

        let child = cmd.spawn().map_err(|e| {
            log::error!("Failed to spawn child process: {e}");
            e
        })?;
        let child_pid = child.id();

        // Store child in Arc<Mutex<>>
        let mut child_guard = self.child.lock().unwrap();
        *child_guard = Some(child);

        log::info!("Child process started with PID: {child_pid}");
        println!("Child process started with PID: {child_pid}");
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
            "File descriptors - stdin: {stdin_fd}, stdout: {stdout_fd}, master: {master_fd}, forward_pipe: {forward_pipe_fd}"
        );

        // Create separate owned file descriptors by duplicating them
        let stdin_dup = unistd::dup(stdin_fd).map_err(|e| {
            log::error!("Failed to dup stdin: {e}");
            io::Error::other(format!("Failed to dup stdin: {e}"))
        })?;
        let stdout_dup = unistd::dup(stdout_fd).map_err(|e| {
            log::error!("Failed to dup stdout: {e}");
            io::Error::other(format!("Failed to dup stdout: {e}"))
        })?;
        let master_dup1 = unistd::dup(master_fd).map_err(|e| {
            log::error!("Failed to dup master1: {e}");
            io::Error::other(format!("Failed to dup master1: {e}"))
        })?;
        let master_dup2 = unistd::dup(master_fd).map_err(|e| {
            log::error!("Failed to dup master2: {e}");
            io::Error::other(format!("Failed to dup master2: {e}"))
        })?;
        let forward_dup = unistd::dup(forward_pipe_fd).map_err(|e| {
            log::error!("Failed to dup forward pipe: {e}");
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

        // Forward stdin to PTY master
        log::debug!("Starting stdin->master forwarder thread");
        thread::spawn(move || {
            log::debug!("Stdin forwarder thread started");
            if let Err(e) = simple_forward(stdin, stdin_master) {
                log::error!("Error in stdin forwarding: {e}");
                eprintln!("Error in stdin forwarding: {e}");
            }
            log::debug!("Stdin forwarder thread ending");
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
                log::error!("Error in unified PTY reader: {e}");
                eprintln!("Error in unified PTY reader: {e}");
            }
            log::debug!("Unified PTY reader thread ending");
        });

        log::info!("I/O forwarders started successfully");
        Ok(())
    }

    pub fn wait_for_child(&mut self) -> Result<i32> {
        log::debug!("Waiting for child process to exit");

        // Wait for shutdown flag to be set by signal handler
        loop {
            let shutdown_guard = self.shutdown_flag.lock().unwrap();
            if *shutdown_guard {
                drop(shutdown_guard);
                break;
            }
            drop(shutdown_guard);

            // Sleep briefly before checking again
            thread::sleep(Duration::from_millis(100));
        }

        // Get the exit status set by the signal handler
        let exit_status_guard = self.exit_status.lock().unwrap();
        let exit_code = exit_status_guard.unwrap_or(0);
        drop(exit_status_guard);

        log::info!("Child process exited with code: {exit_code}");
        println!("Child process exited with code: {exit_code}");
        Ok(exit_code)
    }

    pub fn get_master_fd(&self) -> RawFd {
        self.pty.master_fd()
    }

    pub fn get_session_callbacks(&self) -> crate::signals::SessionCallbacks {
        crate::signals::SessionCallbacks {
            child: self.child.clone(),
            exit_status: self.exit_status.clone(),
            shutdown_flag: self.shutdown_flag.clone(),
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        log::debug!("Session Drop trait called - starting cleanup");

        // Restore original terminal settings
        if let Some(termios) = &self.original_termios {
            log::debug!("Restoring original terminal settings");
            if let Err(e) = restore_termios(libc::STDIN_FILENO, termios) {
                log::error!("Failed to restore terminal settings: {e}");
            } else {
                log::debug!("Terminal settings restored successfully");
            }
        }

        // Kill child process if still running
        if let Ok(mut child_guard) = self.child.lock() {
            if let Some(mut child) = child_guard.take() {
                log::debug!("Killing child process with PID: {}", child.id());
                if let Err(e) = child.kill() {
                    log::error!("Failed to kill child process: {e}");
                }
                if let Err(e) = child.wait() {
                    log::error!("Failed to wait for killed child process: {e}");
                } else {
                    log::debug!("Child process cleanup completed");
                }
            }
        } else {
            log::warn!("Could not acquire lock to kill child process during cleanup");
        }

        log::info!("Session cleanup completed");
        println!("Session cleanup completed");
    }
}
