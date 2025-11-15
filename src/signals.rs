use signal_hook::consts::signal::{SIGCHLD, SIGHUP, SIGINT, SIGQUIT, SIGTSTP, SIGWINCH};
use signal_hook::iterator::Signals;
use std::collections::HashSet;
use std::fs;
use std::io::Result;
use std::os::unix::io::RawFd;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::pty::{propagate_terminal_size, propagate_terminal_size_sync};
use crate::session::SessionState;

pub struct SignalHandler {
    // We'll create the signals iterator when starting the handler thread
}

impl SignalHandler {
    pub fn new() -> Result<Self> {
        log::debug!("Initializing signal handler for all signals");
        log::info!(
            "Signal handler initialized for SIGCHLD, SIGWINCH, SIGHUP, SIGINT, SIGQUIT, SIGTSTP"
        );
        Ok(SignalHandler {})
    }

    fn get_session_id(&self, pid: i32) -> Option<i32> {
        let stat_path = format!("/proc/{pid}/stat");

        match fs::read_to_string(&stat_path) {
            Ok(contents) => {
                // The session ID is the 6th field in /proc/pid/stat
                let fields: Vec<&str> = contents.split_whitespace().collect();
                if fields.len() >= 6 {
                    fields[5].parse().ok()
                } else {
                    None
                }
            }
            Err(_) => None,
        }
    }

    fn get_process_group_id(&self, pid: i32) -> Option<i32> {
        let stat_path = format!("/proc/{pid}/stat");

        match fs::read_to_string(&stat_path) {
            Ok(contents) => {
                // The process group ID is the 5th field in /proc/pid/stat
                let fields: Vec<&str> = contents.split_whitespace().collect();
                if fields.len() >= 5 {
                    fields[4].parse().ok()
                } else {
                    None
                }
            }
            Err(_) => None,
        }
    }

    fn get_session_processes(&self, target_pid: i32) -> Vec<i32> {
        // First get the session ID of the target process
        let target_session_id = match self.get_session_id(target_pid) {
            Some(sid) => sid,
            None => {
                log::warn!("Could not get session ID for PID {target_pid}");
                return vec![target_pid]; // Fallback to just the target process
            }
        };

        log::debug!("Target process {target_pid} has session ID: {target_session_id}");

        // Read /proc directory to find all processes
        let proc_entries = match fs::read_dir("/proc") {
            Ok(entries) => entries,
            Err(e) => {
                log::error!("Could not read /proc directory: {e}");
                return vec![target_pid]; // Fallback to just the target process
            }
        };

        let mut session_processes = Vec::new();

        for entry in proc_entries.flatten() {
            let entry_name = entry.file_name();
            if let Some(name_str) = entry_name.to_str() {
                // Check if this is a process directory (numeric name)
                if let Ok(pid) = name_str.parse::<i32>() {
                    // Get session ID for this process
                    if let Some(session_id) = self.get_session_id(pid) {
                        if session_id == target_session_id {
                            session_processes.push(pid);
                        }
                    }
                }
            }
        }

        log::debug!(
            "Found {} processes in session {target_session_id}: {:?}",
            session_processes.len(),
            session_processes
        );

        if session_processes.is_empty() {
            log::warn!(
                "No processes found in session {target_session_id}, falling back to target process"
            );
            vec![target_pid]
        } else {
            session_processes
        }
    }

    fn get_process_group_processes(&self, target_pid: i32) -> Vec<i32> {
        // First get the process group ID of the target process
        let target_pgid = match self.get_process_group_id(target_pid) {
            Some(pgid) => pgid,
            None => {
                log::warn!("Could not get process group ID for PID {target_pid}");
                return vec![target_pid]; // Fallback to just the target process
            }
        };

        log::debug!("Target process {target_pid} has process group ID: {target_pgid}");

        // Read /proc directory to find all processes
        let proc_entries = match fs::read_dir("/proc") {
            Ok(entries) => entries,
            Err(e) => {
                log::error!("Could not read /proc directory: {e}");
                return vec![target_pid]; // Fallback to just the target process
            }
        };

        let mut group_processes = Vec::new();

        for entry in proc_entries.flatten() {
            let entry_name = entry.file_name();
            if let Some(name_str) = entry_name.to_str() {
                // Check if this is a process directory (numeric name)
                if let Ok(pid) = name_str.parse::<i32>() {
                    // Get process group ID for this process
                    if let Some(pgid) = self.get_process_group_id(pid) {
                        if pgid == target_pgid {
                            group_processes.push(pid);
                        }
                    }
                }
            }
        }

        log::debug!(
            "Found {} processes in process group {target_pgid}: {:?}",
            group_processes.len(),
            group_processes
        );

        if group_processes.is_empty() {
            log::warn!(
                "No processes found in process group {target_pgid}, falling back to target process"
            );
            vec![target_pid]
        } else {
            group_processes
        }
    }

    fn send_signal_to_session_and_group(&self, target_pid: i32, signal: i32, signal_name: &str) {
        log::info!(
            "Forwarding {signal_name} to session and process group processes of PID: {target_pid}"
        );

        // Get all processes in the same session
        let session_processes = self.get_session_processes(target_pid);
        log::debug!("Session processes: {} found", session_processes.len());

        // Get all processes in the same process group
        let group_processes = self.get_process_group_processes(target_pid);
        log::debug!("Process group processes: {} found", group_processes.len());

        // Combine and deduplicate processes
        let mut all_processes: HashSet<i32> = HashSet::new();
        all_processes.extend(session_processes.iter());
        all_processes.extend(group_processes.iter());

        let unique_processes: Vec<i32> = all_processes.into_iter().collect();
        log::info!(
            "Total unique processes to send {signal_name} to: {} (session: {}, group: {})",
            unique_processes.len(),
            session_processes.len(),
            group_processes.len()
        );

        let mut success_count = 0;
        let mut error_count = 0;

        // Send signal to each unique process individually
        for process_pid in &unique_processes {
            let result = unsafe { libc::kill(*process_pid, signal) };
            if result == 0 {
                log::debug!("{signal_name} sent to process {process_pid} successfully");
                success_count += 1;
            } else {
                log::warn!("Failed to send {signal_name} to process {process_pid}: errno {result}");
                error_count += 1;
            }
        }

        if success_count > 0 {
            log::info!(
                "{signal_name} successfully sent to {success_count}/{} processes",
                unique_processes.len()
            );
        }
        if error_count > 0 {
            log::warn!(
                "{signal_name} failed to send to {error_count}/{} processes",
                unique_processes.len()
            );

            // Fallback: try process group signal if individual sends failed
            log::info!("Attempting fallback to process group signal for PID {target_pid}");
            let result = unsafe { libc::kill(-target_pid, signal) };
            if result == 0 {
                log::info!(
                    "{signal_name} fallback to process group signal {target_pid} successful"
                );
            } else {
                log::error!("Fallback {signal_name} to process group signal {target_pid} also failed: errno {result}");
            }
        }
    }

    pub fn start_handler_thread(
        self,
        master_fd: Arc<Mutex<Option<RawFd>>>,
        callbacks: SessionState,
    ) -> JoinHandle<()> {
        thread::spawn(move || {
            log::debug!("Starting signal handler thread");

            // Create the signals iterator inside the thread
            let signals = match Signals::new([SIGCHLD, SIGWINCH, SIGHUP, SIGINT, SIGQUIT, SIGTSTP])
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
        callbacks: SessionState,
    ) {
        log::debug!("Starting signal handler loop");

        loop {
            // Check if we should exit (master_fd is None)
            {
                let fd_guard = master_fd.lock().unwrap();
                if fd_guard.is_none() {
                    log::debug!("Master fd is None, exiting signal handler");
                    break;
                }
            }

            // Process any pending signals (non-blocking)
            for signal in signals.pending() {
                log::info!("Received signal: {signal}");

                match signal {
                    SIGWINCH => {
                        log::info!("Caught SIGWINCH signal - handling terminal resize");

                        // First, forward the signal to the child session
                        let child_guard = callbacks.child.lock().unwrap();
                        if let Some(child) = child_guard.as_ref() {
                            let child_pid = child.id() as i32;
                            self.send_signal_to_session_and_group(
                                child_pid,
                                libc::SIGWINCH,
                                "SIGWINCH",
                            );
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
                    SIGINT => {
                        log::debug!("Caught SIGINT signal");
                        self.handle_sigint(&callbacks);
                    }
                    SIGQUIT => {
                        log::debug!("Caught SIGQUIT signal");
                        self.handle_sigquit(&callbacks);
                    }
                    SIGTSTP => {
                        log::debug!("Caught SIGTSTP signal");
                        self.handle_sigtstp(&callbacks);
                    }
                    _ => {
                        log::warn!("Received unexpected signal: {signal}");
                    }
                }
            }

            // Sleep briefly before checking again
            thread::sleep(Duration::from_millis(50));
        }

        log::debug!("Signal handler loop ended");
    }

    fn handle_sigchld(&self, callbacks: &SessionState) {
        log::info!("Handling child process termination");

        let mut child_guard = callbacks.child.lock().unwrap();
        if let Some(mut child) = child_guard.take() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let exit_code = status.code().unwrap_or(-1);
                    log::info!("Child process exited with status: {exit_code}");

                    log::info!("Setting exit status...");
                    let mut exit_status_guard = callbacks.exit_status.lock().unwrap();
                    *exit_status_guard = Some(exit_code);
                    log::info!("Exit status set to {exit_code}");

                    log::info!("Signaling shutdown...");
                    let mut shutdown_guard = callbacks.shutdown_flag.lock().unwrap();
                    *shutdown_guard = true;
                    log::info!("Shutdown signaled");
                }
                Ok(None) => {
                    log::debug!("Child process still running, putting it back");
                    *child_guard = Some(child);
                }
                Err(e) => {
                    log::error!("Error checking child process status: {e}");
                    let mut exit_status_guard = callbacks.exit_status.lock().unwrap();
                    *exit_status_guard = Some(255);

                    log::info!("Signaling shutdown due to error...");
                    let mut shutdown_guard = callbacks.shutdown_flag.lock().unwrap();
                    *shutdown_guard = true;
                    log::info!("Shutdown signaled");
                }
            }
        } else {
            log::warn!("SIGCHLD received but no child process to handle");
        }
    }

    fn handle_sighup(&self, callbacks: &SessionState) {
        log::info!("Handling session hangup - initiating graceful shutdown");

        // Signal graceful shutdown
        let mut shutdown_guard = callbacks.shutdown_flag.lock().unwrap();
        *shutdown_guard = true;
    }

    fn handle_sigint(&self, callbacks: &SessionState) {
        log::info!(
            "Handling SIGINT signal - forwarding to session and process group (renlog will not exit)"
        );

        let child_guard = callbacks.child.lock().unwrap();
        if let Some(child) = child_guard.as_ref() {
            let child_pid = child.id() as i32;
            self.send_signal_to_session_and_group(child_pid, libc::SIGINT, "SIGINT");
        } else {
            log::info!("No child process to forward SIGINT to - ignoring signal");
        }
        // Note: We do NOT set shutdown_flag here - renlog continues running
    }

    fn handle_sigquit(&self, callbacks: &SessionState) {
        log::info!(
            "Handling SIGQUIT signal - forwarding to session and process group (renlog will not exit)"
        );

        let child_guard = callbacks.child.lock().unwrap();
        if let Some(child) = child_guard.as_ref() {
            let child_pid = child.id() as i32;
            self.send_signal_to_session_and_group(child_pid, libc::SIGQUIT, "SIGQUIT");
        } else {
            log::info!("No child process to forward SIGQUIT to - ignoring signal");
        }
        // Note: We do NOT set shutdown_flag here - renlog continues running
    }

    fn handle_sigtstp(&self, callbacks: &SessionState) {
        log::info!(
            "Handling SIGTSTP signal - forwarding to session and process group (renlog will not exit)"
        );

        let child_guard = callbacks.child.lock().unwrap();
        if let Some(child) = child_guard.as_ref() {
            let child_pid = child.id() as i32;
            self.send_signal_to_session_and_group(child_pid, libc::SIGTSTP, "SIGTSTP");
        } else {
            log::info!("No child process to forward SIGTSTP to - ignoring signal");
        }
        // Note: We do NOT set shutdown_flag here - renlog continues running
    }
}
