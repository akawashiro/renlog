use futures::StreamExt;
use signal_hook::consts::signal::SIGWINCH;
use signal_hook_tokio::Signals;
use std::io::Result;
use std::os::unix::io::RawFd;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::pty::propagate_terminal_size;

pub struct SignalHandler {
    signals: Signals,
}

impl SignalHandler {
    pub fn new() -> Result<Self> {
        log::debug!("Initializing signal handler for SIGWINCH");

        let signals = Signals::new(&[SIGWINCH]).map_err(|e| {
            log::error!("Failed to create signal handler: {}", e);
            std::io::Error::other(format!("Failed to create signal handler: {e}"))
        })?;

        log::info!("Signal handler created for SIGWINCH");
        Ok(SignalHandler { signals })
    }

    pub async fn handle_signals(mut self, master_fd: Arc<Mutex<Option<RawFd>>>) {
        log::debug!("Starting signal handler loop");

        while let Some(signal) = self.signals.next().await {
            match signal {
                SIGWINCH => {
                    log::debug!("Caught SIGWINCH signal");

                    let master_fd_guard = master_fd.lock().await;
                    if let Some(fd) = *master_fd_guard {
                        log::debug!("Propagating terminal size to PTY master fd: {}", fd);

                        if let Err(e) = propagate_terminal_size(libc::STDIN_FILENO, fd) {
                            log::error!("Failed to propagate terminal size: {}", e);
                        } else {
                            log::info!("Terminal size propagated successfully");
                        }
                    } else {
                        log::warn!("No PTY master fd available for terminal size propagation");
                    }
                }
                _ => {
                    log::warn!("Received unexpected signal: {}", signal);
                }
            }
        }

        log::debug!("Signal handler loop ended");
    }
}
