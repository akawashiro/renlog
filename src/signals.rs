use signal_hook::consts::signal::{SIGCHLD, SIGHUP, SIGINT, SIGTERM, SIGUSR2, SIGWINCH};
use signal_hook::iterator::Signals;
use std::io::Result;
use std::os::unix::io::RawFd;
use std::process::Child;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crate::pty::{propagate_terminal_size, propagate_terminal_size_sync};

pub struct SessionCallbacks {
    pub child: Arc<Mutex<Option<Child>>>,
    pub exit_status: Arc<Mutex<Option<i32>>>,
    pub shutdown_flag: Arc<Mutex<bool>>,
}

pub struct SignalHandler {
    // We'll create the signals iterator when starting the handler thread
}

impl SignalHandler {
    pub fn new() -> Result<Self> {
        log::debug!("Initializing signal handler for all signals");
        log::info!(
            "Signal handler initialized for SIGCHLD, SIGWINCH, SIGHUP, SIGUSR2, SIGINT, SIGTERM"
        );
        Ok(SignalHandler {})
    }

    pub fn start_handler_thread(
        self,
        master_fd: Arc<Mutex<Option<RawFd>>>,
        callbacks: SessionCallbacks,
    ) -> JoinHandle<()> {
        thread::spawn(move || {
            log::debug!("Starting signal handler thread");

            // Create the signals iterator inside the thread
            let signals = match Signals::new([SIGCHLD, SIGWINCH, SIGHUP, SIGUSR2, SIGINT, SIGTERM])
            {
                Ok(s) => s,
                Err(e) => {
                    log::error!("Failed to create signal handler: {e}");
                    return;
                }
            };

            self.handle_signals_sync(signals, master_fd, callbacks);
            log::debug!("Signal handler thread ending");
        })
    }

    fn handle_signals_sync(
        self,
        mut signals: Signals,
        master_fd: Arc<Mutex<Option<RawFd>>>,
        callbacks: SessionCallbacks,
    ) {
        log::debug!("Starting signal handler loop");

        for signal in signals.forever() {
            // Check if we should exit (master_fd is None)
            {
                let fd_guard = master_fd.lock().unwrap();
                if fd_guard.is_none() {
                    log::debug!("Master fd is None, exiting signal handler");
                    break;
                }
            }

            match signal {
                SIGWINCH => {
                    log::info!("Caught SIGWINCH signal - handling terminal resize");

                    // First, forward the signal to the child process
                    let child_guard = callbacks.child.lock().unwrap();
                    if let Some(child) = child_guard.as_ref() {
                        let child_pid = child.id() as i32;
                        log::info!("Forwarding SIGWINCH to child process PID: {child_pid}");

                        // Use libc::kill to send SIGWINCH to child process
                        let result = unsafe { libc::kill(child_pid, libc::SIGWINCH) };
                        if result != 0 {
                            log::warn!(
                                "Failed to forward SIGWINCH to child process PID {}: errno {}",
                                child_pid,
                                result
                            );
                        } else {
                            log::info!(
                                "SIGWINCH forwarded to child process PID {} successfully",
                                child_pid
                            );
                        }
                    } else {
                        log::debug!("No child process to forward SIGWINCH to");
                    }
                    drop(child_guard);

                    // Then, propagate terminal size to our PTY master
                    let master_fd_guard = master_fd.lock().unwrap();
                    if let Some(fd) = *master_fd_guard {
                        log::debug!("Propagating terminal size to PTY master fd: {fd}");

                        // Try synchronous propagation first for immediate response
                        if let Err(e) = propagate_terminal_size_sync(libc::STDIN_FILENO, fd) {
                            log::error!("Synchronous terminal size propagation failed: {e}");
                            // Fall back to async propagation
                            if let Err(e) = propagate_terminal_size(libc::STDIN_FILENO, fd) {
                                log::error!("Failed to propagate terminal size: {e}");
                            } else {
                                log::info!(
                                    "Terminal size propagated successfully (async fallback)"
                                );
                            }
                        } else {
                            log::info!("Terminal size propagated successfully (sync)");
                        }
                    } else {
                        log::warn!("No PTY master fd available for terminal size propagation");
                    }
                }
                SIGCHLD => {
                    log::debug!("Caught SIGCHLD signal");
                    self.handle_sigchld(&callbacks);
                }
                SIGHUP => {
                    log::debug!("Caught SIGHUP signal");
                    self.handle_sighup(&callbacks);
                }
                SIGUSR2 => {
                    log::debug!("Caught SIGUSR2 signal");
                    self.handle_sigusr2();
                }
                SIGINT => {
                    log::debug!("Caught SIGINT signal");
                    self.handle_sigint(&callbacks);
                }
                SIGTERM => {
                    log::debug!("Caught SIGTERM signal");
                    self.handle_sigterm(&callbacks);
                }
                _ => {
                    log::warn!("Received unexpected signal: {signal}");
                }
            }
        }

        log::debug!("Signal handler loop ended");
    }

    fn handle_sigchld(&self, callbacks: &SessionCallbacks) {
        log::info!("Handling child process termination");

        let mut child_guard = callbacks.child.lock().unwrap();
        if let Some(mut child) = child_guard.take() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let exit_code = status.code().unwrap_or(-1);
                    log::info!("Child process exited with status: {exit_code}");

                    let mut exit_status_guard = callbacks.exit_status.lock().unwrap();
                    *exit_status_guard = Some(exit_code);

                    // Signal shutdown
                    let mut shutdown_guard = callbacks.shutdown_flag.lock().unwrap();
                    *shutdown_guard = true;
                }
                Ok(None) => {
                    log::debug!("Child process still running, putting it back");
                    *child_guard = Some(child);
                }
                Err(e) => {
                    log::error!("Error checking child process status: {e}");
                    let mut exit_status_guard = callbacks.exit_status.lock().unwrap();
                    *exit_status_guard = Some(255);

                    let mut shutdown_guard = callbacks.shutdown_flag.lock().unwrap();
                    *shutdown_guard = true;
                }
            }
        } else {
            log::warn!("SIGCHLD received but no child process to handle");
        }
    }

    fn handle_sighup(&self, callbacks: &SessionCallbacks) {
        log::info!("Handling session hangup - initiating graceful shutdown");

        // Signal graceful shutdown
        let mut shutdown_guard = callbacks.shutdown_flag.lock().unwrap();
        *shutdown_guard = true;
    }

    fn handle_sigusr2(&self) {
        log::info!("Caught SIGUSR2; dumping stack traces");

        // In Rust, we can use backtrace or just log the signal receipt
        // For now, we'll just log it as zenlog does with goroutine dumps
        println!("SIGUSR2 received - stack trace dump requested");

        // You could add backtrace here if needed:
        // let bt = std::backtrace::Backtrace::capture();
        // println!("Backtrace:\n{}", bt);
    }

    fn handle_sigint(&self, callbacks: &SessionCallbacks) {
        log::info!("Handling SIGINT signal - forwarding to child process");

        let child_guard = callbacks.child.lock().unwrap();
        if let Some(child) = child_guard.as_ref() {
            let child_pid = child.id() as i32;
            log::info!("Forwarding SIGINT to child process PID: {child_pid}");

            // Use libc::kill to send SIGINT to child process
            let result = unsafe { libc::kill(child_pid, libc::SIGINT) };
            if result != 0 {
                log::warn!(
                    "Failed to forward SIGINT to child process PID {}: errno {}",
                    child_pid,
                    result
                );
            } else {
                log::info!(
                    "SIGINT forwarded to child process PID {} successfully",
                    child_pid
                );
            }
        } else {
            log::info!("No child process to forward SIGINT to - allowing renlog to exit");
            // If no child process, allow the signal to terminate renlog
            let mut shutdown_guard = callbacks.shutdown_flag.lock().unwrap();
            *shutdown_guard = true;
        }
    }

    fn handle_sigterm(&self, callbacks: &SessionCallbacks) {
        log::info!("Handling SIGTERM signal - forwarding to child process");

        let child_guard = callbacks.child.lock().unwrap();
        if let Some(child) = child_guard.as_ref() {
            let child_pid = child.id() as i32;
            log::info!("Forwarding SIGTERM to child process PID: {child_pid}");

            // Use libc::kill to send SIGTERM to child process
            let result = unsafe { libc::kill(child_pid, libc::SIGTERM) };
            if result != 0 {
                log::warn!(
                    "Failed to forward SIGTERM to child process PID {}: errno {}",
                    child_pid,
                    result
                );
            } else {
                log::info!(
                    "SIGTERM forwarded to child process PID {} successfully",
                    child_pid
                );
            }
        } else {
            log::info!("No child process to forward SIGTERM to - allowing renlog to exit");
            // If no child process, allow the signal to terminate renlog
            let mut shutdown_guard = callbacks.shutdown_flag.lock().unwrap();
            *shutdown_guard = true;
        }
    }
}
