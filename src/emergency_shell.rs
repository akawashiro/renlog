use std::env;
use std::io::Result;
use std::process::{Command, ExitStatus};

/// Emergency shell fallback when renlog fails to start properly
/// This mimics zenlog's emergency shell functionality
pub struct EmergencyShell {
    shell_command: String,
}

impl EmergencyShell {
    pub fn new() -> Self {
        let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        log::info!("Emergency shell initialized with: {shell}");

        EmergencyShell {
            shell_command: shell,
        }
    }

    /// Validate that standard I/O file descriptors are available
    fn validate_stdio(&self) -> Result<()> {
        use nix::fcntl::{fcntl, FcntlArg};
        use std::io::Error;

        // Check stdin (fd 0)
        if fcntl(libc::STDIN_FILENO, FcntlArg::F_GETFD).is_err() {
            log::error!("Emergency shell: stdin (fd 0) is not available");
            eprintln!("CRITICAL: stdin is not available for emergency shell");
            return Err(Error::other("stdin not available for emergency shell"));
        }

        // Check stdout (fd 1)
        if fcntl(libc::STDOUT_FILENO, FcntlArg::F_GETFD).is_err() {
            log::error!("Emergency shell: stdout (fd 1) is not available");
            eprintln!("CRITICAL: stdout is not available for emergency shell");
            return Err(Error::other("stdout not available for emergency shell"));
        }

        // Check stderr (fd 2)
        if fcntl(libc::STDERR_FILENO, FcntlArg::F_GETFD).is_err() {
            log::error!("Emergency shell: stderr (fd 2) is not available");
            // Can't print to stderr if it's not available
            return Err(Error::other("stderr not available for emergency shell"));
        }

        // Check if stdin is a TTY (optional, just for logging)
        if nix::unistd::isatty(libc::STDIN_FILENO).unwrap_or(false) {
            log::info!("Emergency shell: stdin is a TTY");
        } else {
            log::info!("Emergency shell: stdin is not a TTY");
        }

        log::info!("Emergency shell: All standard I/O file descriptors validated");
        Ok(())
    }

    /// Start the emergency shell directly without any renlog features
    pub fn start(&self) -> Result<ExitStatus> {
        log::warn!("Starting emergency shell due to renlog failure");
        eprintln!("WARNING: Starting emergency shell - renlog features disabled");
        eprintln!("Shell: {}", self.shell_command);

        // Validate that stdin/stdout/stderr are available
        self.validate_stdio()?;

        let mut cmd = Command::new(&self.shell_command);

        // Run the shell directly, inheriting stdin/stdout/stderr
        cmd.spawn()?.wait()
    }

    /// Start emergency shell with a specific error message
    pub fn start_with_error(&self, error_msg: &str) -> Result<ExitStatus> {
        log::error!("Emergency shell triggered by error: {error_msg}");
        eprintln!("ERROR: {error_msg}");
        eprintln!("Falling back to emergency shell...");

        self.start()
    }
}

impl Default for EmergencyShell {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emergency_shell_creation() {
        let shell = EmergencyShell::new();
        assert!(!shell.shell_command.is_empty());
    }
}
