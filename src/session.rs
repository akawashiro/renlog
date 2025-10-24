use crate::io_forward::{forward_pty_to_stdout_with_logging, forward_stdin_to_pty};
use crate::pty::{restore_termios, set_raw_mode, PtyPair};
use crate::shell_config::LAST_LOG_FILE;

use nix::sys::termios::Termios;
use nix::unistd;
use std::env;
use std::fs::File;
use std::io::{self, Result};
use std::os::unix::io::{FromRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub struct Session {
    pty: PtyPair,
    child: Arc<Mutex<Option<Child>>>,
    original_termios: Option<Termios>,
    pid: u32,
    renlog_dir: PathBuf,
    exit_status: Arc<Mutex<Option<i32>>>,
    shutdown_flag: Arc<Mutex<bool>>,
    log_filter_cmd: Option<String>,
}

fn validate_fd(fd: RawFd, name: &str) -> Result<()> {
    use nix::fcntl::{fcntl, FcntlArg};

    // See the following quotation from man fcntl(2):
    //
    // The  following  operations  manipulate  the  flags  associated with a file descriptor.
    // Currently, only one such flag is defined: FD_CLOEXEC, the close-on-exec flag.  If the
    // FD_CLOEXEC bit is set, the file descriptor will automatically be closed during a successful
    // execve(2).  (If  the  ex‐ ecve(2) fails, the file descriptor is left open.)  If the
    // FD_CLOEXEC bit is not set, the file descriptor will remain open across an execve(2).
    //
    // F_GETFD (void)
    //        Return (as the function result) the file descriptor flags; arg is ignored.
    match fcntl(fd, FcntlArg::F_GETFD) {
        Ok(_) => {
            log::debug!("File descriptor {fd} ({name}) is valid");
            Ok(())
        }
        Err(e) => {
            log::error!("File descriptor {fd} ({name}) is invalid: {e}");
            Err(io::Error::other(format!(
                "Invalid file descriptor {fd} ({name}): {e}"
            )))
        }
    }
}

fn ensure_fd_ready(fd: RawFd, name: &str) -> Result<()> {
    validate_fd(fd, name)?;

    use nix::poll::{poll, PollFd, PollFlags, PollTimeout};
    use std::os::unix::io::BorrowedFd;

    let borrowed_fd = unsafe { BorrowedFd::borrow_raw(fd) };
    let mut poll_fds = [PollFd::new(
        borrowed_fd,
        PollFlags::POLLIN | PollFlags::POLLOUT,
    )];

    // Use zero timeout for non-blocking poll
    match poll(&mut poll_fds, PollTimeout::ZERO) {
        Ok(_) => {
            log::debug!("File descriptor {fd} ({name}) is ready");
            Ok(())
        }
        Err(e) => {
            log::warn!("File descriptor {fd} ({name}) readiness check failed: {e}");
            Ok(())
        }
    }
}

impl Session {
    pub fn new(
        renlog_dir: &Path,
        user_specified_shell_cmd: Option<&str>,
        log_filter_cmd: Option<&str>,
    ) -> Result<Self> {
        log::debug!("Creating new Session instance");
        let pid = std::process::id();
        log::info!(
            "Session initializing with PID={pid} renlog_dir={renlog_dir:?} user_specified_shell_cmd={user_specified_shell_cmd:?} log_filter_cmd={log_filter_cmd:?}"
        );

        // Create the log directory if it doesn't exist
        std::fs::create_dir_all(renlog_dir).map_err(|e| {
            log::error!("Failed to create renlog log directory '{renlog_dir:?}': {e}");
            io::Error::other(format!(
                "Failed to create renlog log directory '{renlog_dir:?}': {e}"
            ))
        })?;

        log::debug!("Creating PTY pair");
        let pty = PtyPair::new()?;

        // Set stdin to cbreak mode (only if it's a TTY)
        log::debug!("Checking if stdin is a TTY");
        let original_termios = if nix::unistd::isatty(libc::STDIN_FILENO).unwrap_or(false) {
            log::debug!("Setting stdin to raw mode");
            Some(set_raw_mode(libc::STDIN_FILENO).map_err(|e| {
                log::error!("Failed to set raw mode: {e}");
                io::Error::other(format!("Failed to set cbreak mode: {e}"))
            })?)
        } else {
            log::info!("Stdin is not a TTY, skipping cbreak mode setup");
            None
        };
        if original_termios.is_some() {
            log::info!("Terminal set to cbreak mode successfully");
        }

        let child_arc = Arc::new(Mutex::new(None));

        // Start child process
        Self::start_child(&pty, user_specified_shell_cmd, pid, renlog_dir, &child_arc)?;

        log::info!("Session instance created successfully");
        Ok(Session {
            pty,
            child: child_arc,
            original_termios,
            pid,
            renlog_dir: renlog_dir.to_path_buf(),
            exit_status: Arc::new(Mutex::new(None)),
            shutdown_flag: Arc::new(Mutex::new(false)),
            log_filter_cmd: log_filter_cmd.map(|s| s.to_string()),
        })
    }

    pub fn start_child(
        pty: &PtyPair,
        user_specified_shell_cmd: Option<&str>,
        pid: u32,
        renlog_dir: &Path,
        child: &Arc<Mutex<Option<Child>>>,
    ) -> Result<()> {
        log::debug!("Starting child process");

        // Validate PTY readiness before proceeding
        pty.validate_readiness()?;
        log::debug!("PTY validated and ready for child process");

        let mut cmd = if let Some(user_specified_shell_cmd) = user_specified_shell_cmd {
            // Parse the custom command
            let parts: Vec<&str> = user_specified_shell_cmd.split_whitespace().collect();
            if parts.is_empty() {
                return Err(io::Error::other("Empty custom command provided"));
            }

            log::info!("Using custom command: {user_specified_shell_cmd}");
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

        cmd.env("RENLOG_PID", pid.to_string());

        let mut last_log_file = renlog_dir.to_path_buf().into_os_string();
        last_log_file.push(format!("/{LAST_LOG_FILE}"));
        cmd.env("RENLOG_LAST_LOG_FILE", &last_log_file);

        // Set up stdio to use the PTY slave (duplicate fd for each stdio stream)
        let slave_fd = pty.slave_fd();

        // Validate the slave FD before duplicating
        validate_fd(slave_fd, "PTY slave")?;
        ensure_fd_ready(slave_fd, "PTY slave")?;

        let stdin_fd = unistd::dup(slave_fd).map_err(|e| {
            log::error!("Failed to dup slave fd for stdin: {e}");
            io::Error::other(format!("Failed to dup slave fd for stdin: {e}"))
        })?;

        // Validate duplicated stdin FD
        validate_fd(stdin_fd, "duplicated stdin")?;

        let stdout_fd = unistd::dup(slave_fd).map_err(|e| {
            log::error!("Failed to dup slave fd for stdout: {e}");
            io::Error::other(format!("Failed to dup slave fd for stdout: {e}"))
        })?;

        // Validate duplicated stdout FD
        validate_fd(stdout_fd, "duplicated stdout")?;

        let stderr_fd = unistd::dup(slave_fd).map_err(|e| {
            log::error!("Failed to dup slave fd for stderr: {e}");
            io::Error::other(format!("Failed to dup slave fd for stderr: {e}"))
        })?;

        // Validate duplicated stderr FD
        validate_fd(stderr_fd, "duplicated stderr")?;

        cmd.stdin(unsafe { Stdio::from_raw_fd(stdin_fd) });
        cmd.stdout(unsafe { Stdio::from_raw_fd(stdout_fd) });
        cmd.stderr(unsafe { Stdio::from_raw_fd(stderr_fd) });

        // Set up process group handling for proper signal delivery
        unsafe {
            let slave_fd_to_close = slave_fd;
            cmd.pre_exec(move || {
                // Validate that standard I/O file descriptors are available
                use nix::fcntl::{fcntl, FcntlArg, FdFlag};

                // IMPORTANT: Close the original slave FD in the child process
                // This prevents FD leakage to tmux or other processes
                if libc::close(slave_fd_to_close) == -1 {
                    let err = std::io::Error::last_os_error();
                    eprintln!("WARNING: Failed to close slave FD in child: {err}");
                }

                // Set close-on-exec flag for all FDs except stdin/stdout/stderr
                // This prevents FD leakage when the shell spawns other processes
                for fd in 3..256 {
                    // Set FD_CLOEXEC flag
                    let _ = fcntl(fd, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC));
                }

                // Check stdin (fd 0)
                if fcntl(0, FcntlArg::F_GETFD).is_err() {
                    eprintln!("ERROR: stdin (fd 0) is not available in child process");
                    return Err(io::Error::other("stdin not available"));
                }

                // Check stdout (fd 1)
                if fcntl(1, FcntlArg::F_GETFD).is_err() {
                    eprintln!("ERROR: stdout (fd 1) is not available in child process");
                    return Err(io::Error::other("stdout not available"));
                }

                // Check stderr (fd 2)
                if fcntl(2, FcntlArg::F_GETFD).is_err() {
                    eprintln!("ERROR: stderr (fd 2) is not available in child process");
                    return Err(io::Error::other("stderr not available"));
                }

                // Create a new process group for the child process
                // This ensures proper signal handling and terminal control
                let result = libc::setsid();
                if result == -1 {
                    let err = std::io::Error::last_os_error();
                    eprintln!("ERROR: Failed to create new session for child: {err}");
                    return Err(err);
                }

                // Set controlling terminal for the new session
                // This is important for proper terminal job control
                if libc::ioctl(0, libc::TIOCSCTTY, 0) == -1 {
                    let err = std::io::Error::last_os_error();
                    eprintln!("WARNING: Failed to set controlling terminal: {err}");
                }

                Ok(())
            });
        }

        log::debug!(
            "Configured child stdio to use duplicated PTY slave fds: stdin={stdin_fd}, stdout={stdout_fd}, stderr={stderr_fd}"
        );

        let spawned_child = cmd.spawn().map_err(|e| {
            log::error!("Failed to spawn child process: {e}");
            e
        })?;
        let child_pid = spawned_child.id();

        // Store child in Arc<Mutex<>>
        let mut child_guard = child.lock().unwrap();
        *child_guard = Some(spawned_child);

        log::info!("Child process started with PID: {child_pid}");
        Ok(())
    }

    pub fn start_forwarders(session: &Session) -> Result<()> {
        log::debug!("Starting I/O forwarders");
        // Get raw file descriptors
        let stdin_fd = libc::STDIN_FILENO;
        let stdout_fd = libc::STDOUT_FILENO;
        let master_fd = session.pty.master_fd();

        log::debug!(
            "File descriptors - stdin: {stdin_fd}, stdout: {stdout_fd}, master: {master_fd}"
        );

        // Validate file descriptors before duplicating
        validate_fd(stdin_fd, "stdin")?;
        validate_fd(stdout_fd, "stdout")?;
        validate_fd(master_fd, "PTY master")?;

        // Create separate owned file descriptors by duplicating them
        let stdin_dup = unistd::dup(stdin_fd).map_err(|e| {
            log::error!("Failed to dup stdin: {e}");
            io::Error::other(format!("Failed to dup stdin: {e}"))
        })?;

        // Validate duplicated stdin
        validate_fd(stdin_dup, "duplicated stdin for forwarder")?;

        let stdout_dup = unistd::dup(stdout_fd).map_err(|e| {
            log::error!("Failed to dup stdout: {e}");
            io::Error::other(format!("Failed to dup stdout: {e}"))
        })?;

        // Validate duplicated stdout
        validate_fd(stdout_dup, "duplicated stdout for forwarder")?;
        let master_dup1 = unistd::dup(master_fd).map_err(|e| {
            log::error!("Failed to dup master1: {e}");
            io::Error::other(format!("Failed to dup master1: {e}"))
        })?;

        // Validate duplicated master for stdin forwarding
        validate_fd(master_dup1, "duplicated master for stdin forwarding")?;

        let master_dup2 = unistd::dup(master_fd).map_err(|e| {
            log::error!("Failed to dup master2: {e}");
            io::Error::other(format!("Failed to dup master2: {e}"))
        })?;

        // Validate duplicated master for output forwarding
        validate_fd(master_dup2, "duplicated master for output forwarding")?;

        log::debug!("All file descriptors duplicated and validated successfully");

        // Create File instances from the duplicated descriptors
        let stdin = unsafe { File::from_raw_fd(stdin_dup) };
        let stdout = unsafe { File::from_raw_fd(stdout_dup) };
        let stdin_master = unsafe { File::from_raw_fd(master_dup1) };
        let master_for_output = unsafe { File::from_raw_fd(master_dup2) };

        // Clone necessary data for the output thread
        let renlog_dir_for_output = session.renlog_dir.clone();
        let pid_for_output = session.pid;
        let log_filter_cmd_for_output = session.log_filter_cmd.clone();

        // Forward stdin to PTY master
        log::debug!("Starting stdin->master forwarder thread");
        thread::spawn(move || {
            log::debug!("Stdin forwarder thread started");
            if let Err(e) = forward_stdin_to_pty(stdin, stdin_master) {
                log::error!("Error in stdin forwarding: {e}");
                eprintln!("Error in stdin forwarding: {e}");
            }
            log::debug!("Stdin forwarder thread ending");
        });

        // Forward PTY output to stdout with command detection and logging
        log::debug!("Starting PTY to stdout forwarder thread");
        thread::spawn(move || {
            log::debug!("PTY to stdout forwarder thread started");
            if let Err(e) = forward_pty_to_stdout_with_logging(
                master_for_output,
                stdout,
                &renlog_dir_for_output,
                pid_for_output,
                log_filter_cmd_for_output.as_deref(),
            ) {
                log::error!("Error in PTY to stdout forwarder: {e}");
                eprintln!("Error in unified PTY reader: {e}");
            }
            log::debug!("PTY to stdout forwarder thread ending");
        });

        log::info!("I/O forwarders started successfully");
        Ok(())
    }

    pub fn wait_for_child(session: &Session) -> Result<i32> {
        log::debug!("Waiting for child process to exit");

        // Wait for shutdown flag to be set by signal handler
        loop {
            let shutdown_guard = session.shutdown_flag.lock().unwrap();
            if *shutdown_guard {
                drop(shutdown_guard);
                break;
            }
            drop(shutdown_guard);

            // Sleep briefly before checking again
            thread::sleep(Duration::from_millis(100));
        }

        // Get the exit status set by the signal handler
        let exit_status_guard = session.exit_status.lock().unwrap();
        let exit_code = exit_status_guard.unwrap_or(0);
        drop(exit_status_guard);

        log::info!("Child process exited with code: {exit_code}");
        println!("Child process exited with code: {exit_code}");
        Ok(exit_code)
    }

    pub fn get_master_fd(session: &Session) -> RawFd {
        session.pty.master_fd()
    }

    pub fn get_session_callbacks(session: &Session) -> crate::signals::SessionCallbacks {
        crate::signals::SessionCallbacks {
            child: session.child.clone(),
            exit_status: session.exit_status.clone(),
            shutdown_flag: session.shutdown_flag.clone(),
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
