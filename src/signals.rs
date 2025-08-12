use futures::StreamExt;
use signal_hook::consts::signal::{SIGCHLD, SIGHUP, SIGUSR2, SIGWINCH};
use signal_hook_tokio::Signals;
use std::io::Result;
use std::os::unix::io::RawFd;
use std::process::Child;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::pty::{propagate_terminal_size, propagate_terminal_size_sync};

pub struct SessionCallbacks {
    pub child: Arc<Mutex<Option<Child>>>,
    pub exit_status: Arc<Mutex<Option<i32>>>,
    pub shutdown_flag: Arc<Mutex<bool>>,
}

pub struct SignalHandler {
    signals: Signals,
}

impl SignalHandler {
    pub fn new() -> Result<Self> {
        log::debug!("Initializing signal handler for all signals");

        let signals = Signals::new([SIGCHLD, SIGWINCH, SIGHUP, SIGUSR2]).map_err(|e| {
            log::error!("Failed to create signal handler: {e}");
            std::io::Error::other(format!("Failed to create signal handler: {e}"))
        })?;

        log::info!("Signal handler created for SIGCHLD, SIGWINCH, SIGHUP, SIGUSR2");
        Ok(SignalHandler { signals })
    }

    pub async fn handle_signals(
        mut self,
        master_fd: Arc<Mutex<Option<RawFd>>>,
        callbacks: SessionCallbacks,
    ) {
        log::debug!("Starting signal handler loop");

        while let Some(signal) = self.signals.next().await {
            match signal {
                SIGWINCH => {
                    log::info!("Caught SIGWINCH signal - handling terminal resize");

                    // First, forward the signal to the child process
                    let child_guard = callbacks.child.lock().await;
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
                    let master_fd_guard = master_fd.lock().await;
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
                    self.handle_sigchld(&callbacks).await;
                }
                SIGHUP => {
                    log::debug!("Caught SIGHUP signal");
                    self.handle_sighup(&callbacks).await;
                }
                SIGUSR2 => {
                    log::debug!("Caught SIGUSR2 signal");
                    self.handle_sigusr2().await;
                }
                _ => {
                    log::warn!("Received unexpected signal: {signal}");
                }
            }
        }

        log::debug!("Signal handler loop ended");
    }

    async fn handle_sigchld(&self, callbacks: &SessionCallbacks) {
        log::info!("Handling child process termination");

        let mut child_guard = callbacks.child.lock().await;
        if let Some(mut child) = child_guard.take() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let exit_code = status.code().unwrap_or(-1);
                    log::info!("Child process exited with status: {exit_code}");

                    let mut exit_status_guard = callbacks.exit_status.lock().await;
                    *exit_status_guard = Some(exit_code);

                    // Signal shutdown
                    let mut shutdown_guard = callbacks.shutdown_flag.lock().await;
                    *shutdown_guard = true;
                }
                Ok(None) => {
                    log::debug!("Child process still running, putting it back");
                    *child_guard = Some(child);
                }
                Err(e) => {
                    log::error!("Error checking child process status: {e}");
                    let mut exit_status_guard = callbacks.exit_status.lock().await;
                    *exit_status_guard = Some(255);

                    let mut shutdown_guard = callbacks.shutdown_flag.lock().await;
                    *shutdown_guard = true;
                }
            }
        } else {
            log::warn!("SIGCHLD received but no child process to handle");
        }
    }

    async fn handle_sighup(&self, callbacks: &SessionCallbacks) {
        log::info!("Handling session hangup - initiating graceful shutdown");

        // Signal graceful shutdown
        let mut shutdown_guard = callbacks.shutdown_flag.lock().await;
        *shutdown_guard = true;
    }

    async fn handle_sigusr2(&self) {
        log::info!("Caught SIGUSR2; dumping stack traces");

        // In Rust, we can use backtrace or just log the signal receipt
        // For now, we'll just log it as zenlog does with goroutine dumps
        println!("SIGUSR2 received - stack trace dump requested");

        // You could add backtrace here if needed:
        // let bt = std::backtrace::Backtrace::capture();
        // println!("Backtrace:\n{}", bt);
    }
}
