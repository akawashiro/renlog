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
        log::info!("Emergency shell initialized with: {}", shell);

        EmergencyShell {
            shell_command: shell,
        }
    }

    /// Start the emergency shell directly without any renlog features
    pub fn start(&self) -> Result<ExitStatus> {
        log::warn!("Starting emergency shell due to renlog failure");
        eprintln!("WARNING: Starting emergency shell - renlog features disabled");
        eprintln!("Shell: {}", self.shell_command);

        let mut cmd = Command::new(&self.shell_command);

        // Run the shell directly, inheriting stdin/stdout/stderr
        cmd.spawn()?.wait()
    }

    /// Start emergency shell with a specific error message
    pub fn start_with_error(&self, error_msg: &str) -> Result<ExitStatus> {
        log::error!("Emergency shell triggered by error: {}", error_msg);
        eprintln!("ERROR: {}", error_msg);
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
