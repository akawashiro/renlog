use crate::fifo::FifoPair;
use crate::io_forward::{forward_simple, tee_simple};
use crate::pty::{restore_termios, set_raw_mode, PtyPair};

use nix::sys::termios::Termios;
use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Result};
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::process::{Child, Command, Stdio};
use std::thread;

pub struct Logger {
    pty: PtyPair,
    fifos: FifoPair,
    child: Option<Child>,
    original_termios: Option<Termios>,
    temp_dir: String,
    pid: u32,
}

impl Logger {
    pub fn new() -> Result<Self> {
        let temp_dir = env::var("TMPDIR").unwrap_or_else(|_| "/tmp".to_string());
        let pid = std::process::id();

        let pty = PtyPair::new()?;
        let mut fifos = FifoPair::new(&temp_dir, pid)?;
        fifos.open()?;

        // Set stdin to raw mode
        let original_termios = set_raw_mode(libc::STDIN_FILENO).map_err(|e| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("Failed to set raw mode: {}", e),
            )
        })?;

        println!("Renlog starting... [PID={}]", pid);

        Ok(Logger {
            pty,
            fifos,
            child: None,
            original_termios: Some(original_termios),
            temp_dir,
            pid,
        })
    }

    pub fn start_child(&mut self) -> Result<()> {
        // Set up environment variables for the child
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c");
        cmd.arg(env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string()));

        // Set environment variables that the shell integration would use
        cmd.env("RENLOG_PID", self.pid.to_string());
        cmd.env("RENLOG_LOGGER_IN", self.fifos.forward_path_str());
        cmd.env("RENLOG_LOGGER_OUT", self.fifos.reverse_path_str());

        // Set up stdio to use the PTY slave
        cmd.stdin(unsafe { Stdio::from_raw_fd(self.pty.slave_fd()) });
        cmd.stdout(unsafe { Stdio::from_raw_fd(self.pty.slave_fd()) });
        cmd.stderr(unsafe { Stdio::from_raw_fd(self.pty.slave_fd()) });

        let child = cmd.spawn()?;
        self.child = Some(child);

        println!(
            "Child process started with PID: {}",
            self.child.as_ref().unwrap().id()
        );
        Ok(())
    }

    pub fn start_forwarders(&self) -> Result<()> {
        // Get file handles for I/O forwarding
        let stdin = unsafe { File::from_raw_fd(libc::STDIN_FILENO) };
        let stdout = unsafe { File::from_raw_fd(libc::STDOUT_FILENO) };
        let _master_for_input = unsafe { File::from_raw_fd(self.pty.master_fd()) };
        let master_for_output = unsafe { File::from_raw_fd(self.pty.master_fd()) };

        // We need the forward pipe for tee functionality
        let forward_pipe = self
            .fifos
            .forward_file
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Forward pipe not opened"))?;

        // Clone file descriptors for threading
        let forward_pipe_fd = forward_pipe.as_raw_fd();
        let forward_pipe_clone = unsafe { File::from_raw_fd(forward_pipe_fd) };

        // Forward stdin to PTY master
        let stdin_master = unsafe { File::from_raw_fd(self.pty.master_fd()) };
        thread::spawn(move || {
            if let Err(e) = forward_simple(stdin, stdin_master) {
                eprintln!("Error in stdin forwarding: {}", e);
            }
        });

        // Tee PTY master output to both stdout and forward pipe
        thread::spawn(move || {
            if let Err(e) = tee_simple(master_for_output, stdout, forward_pipe_clone) {
                eprintln!("Error in tee forwarding: {}", e);
            }
        });

        Ok(())
    }

    pub fn run_logger_loop(&self) -> Result<()> {
        // Simple logger loop that reads from the forward pipe
        let forward_file = self
            .fifos
            .forward_file
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Forward pipe not opened"))?;

        let reader = BufReader::new(forward_file);

        println!("Logger loop started, reading from pipe...");

        for line in reader.lines() {
            match line {
                Ok(content) => {
                    // For Phase 1, just echo what we receive from the pipe
                    println!("[LOG] {}", content);

                    // Check for special commands (simplified for Phase 1)
                    if content.trim() == "exit" {
                        println!("Exit command received, shutting down...");
                        break;
                    }
                }
                Err(e) => {
                    eprintln!("Error reading from pipe: {}", e);
                    break;
                }
            }
        }

        Ok(())
    }

    pub fn wait_for_child(&mut self) -> Result<i32> {
        if let Some(mut child) = self.child.take() {
            match child.wait() {
                Ok(status) => {
                    let exit_code = status.code().unwrap_or(-1);
                    println!("Child process exited with code: {}", exit_code);
                    Ok(exit_code)
                }
                Err(e) => Err(e),
            }
        } else {
            Ok(0)
        }
    }
}

impl Drop for Logger {
    fn drop(&mut self) {
        // Restore original terminal settings
        if let Some(termios) = &self.original_termios {
            let _ = restore_termios(libc::STDIN_FILENO, termios);
        }

        // Kill child process if still running
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }

        println!("Logger cleanup completed");
    }
}
