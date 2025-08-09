use crate::fifo::FifoPair;
use crate::io_forward::{forward_simple, tee_simple};
use crate::pty::{restore_termios, set_raw_mode, PtyPair};

use nix::sys::termios::Termios;
use nix::unistd;
use std::env;
use std::fs::File;
use std::io::{self, Result};
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::process::{Child, Command, Stdio};
use std::thread;

pub struct Logger {
    pty: PtyPair,
    fifos: FifoPair,
    child: Option<Child>,
    original_termios: Option<Termios>,
    pid: u32,
}

impl Logger {
    pub fn new() -> Result<Self> {
        log::debug!("Creating new Logger instance");
        let temp_dir = env::var("TMPDIR").unwrap_or_else(|_| "/tmp".to_string());
        let pid = std::process::id();
        log::info!("Logger initializing with PID={} temp_dir={}", pid, temp_dir);

        log::debug!("Creating PTY pair");
        let pty = PtyPair::new()?;

        log::debug!("Creating FIFO pair");
        let mut fifos = FifoPair::new(&temp_dir, pid)?;
        fifos.open()?;
        log::info!(
            "FIFO pipes created and opened: forward={}, reverse={}",
            fifos.forward_path_str(),
            fifos.reverse_path_str()
        );

        // Set stdin to raw mode
        log::debug!("Setting stdin to raw mode");
        let original_termios = set_raw_mode(libc::STDIN_FILENO).map_err(|e| {
            log::error!("Failed to set raw mode: {}", e);
            io::Error::other(format!("Failed to set raw mode: {e}"))
        })?;
        log::info!("Terminal set to raw mode successfully");

        println!("Renlog starting... [PID={pid}]");

        log::info!("Logger instance created successfully");
        Ok(Logger {
            pty,
            fifos,
            child: None,
            original_termios: Some(original_termios),
            pid,
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
        cmd.env("RENLOG_LOGGER_IN", self.fifos.forward_path_str());
        cmd.env("RENLOG_LOGGER_OUT", self.fifos.reverse_path_str());
        log::debug!(
            "Set environment variables: RENLOG_PID={}, RENLOG_LOGGER_IN={}, RENLOG_LOGGER_OUT={}",
            self.pid,
            self.fifos.forward_path_str(),
            self.fifos.reverse_path_str()
        );

        // Set up stdio to use the PTY slave
        cmd.stdin(unsafe { Stdio::from_raw_fd(self.pty.slave_fd()) });
        cmd.stdout(unsafe { Stdio::from_raw_fd(self.pty.slave_fd()) });
        cmd.stderr(unsafe { Stdio::from_raw_fd(self.pty.slave_fd()) });
        log::debug!(
            "Configured child stdio to use PTY slave fd: {}",
            self.pty.slave_fd()
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

        // Forward stdin to PTY master
        log::debug!("Starting stdin->master forwarder thread");
        thread::spawn(move || {
            log::debug!("Stdin forwarder thread started");
            if let Err(e) = forward_simple(stdin, stdin_master) {
                log::error!("Error in stdin forwarding: {}", e);
                eprintln!("Error in stdin forwarding: {e}");
            }
            log::debug!("Stdin forwarder thread ending");
        });

        // Tee PTY master output to both stdout and forward pipe
        log::debug!("Starting master->stdout+pipe tee thread");
        thread::spawn(move || {
            log::debug!("Tee forwarder thread started");
            if let Err(e) = tee_simple(master_for_output, stdout, forward_pipe_clone) {
                log::error!("Error in tee forwarding: {}", e);
                eprintln!("Error in tee forwarding: {e}");
            }
            log::debug!("Tee forwarder thread ending");
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
}

impl Drop for Logger {
    fn drop(&mut self) {
        log::debug!("Logger Drop trait called - starting cleanup");

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

        log::info!("Logger cleanup completed");
        println!("Logger cleanup completed");
    }
}
