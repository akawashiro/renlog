mod ansi_filter;
mod command_logger;
mod command_markers;
mod fifo;
mod io_forward;
mod logging;
mod pty;
mod session;
mod symlink_utils;

use command_markers::{END_COMMAND_NAME, START_COMMAND_NAME};

use chrono::Utc;
use clap::{Parser, Subcommand};
use session::Session;
use std::io::Result;
use std::path::Path;
use symlink_utils::create_or_update_symlink;

#[derive(Parser)]
#[command(name = "renlog")]
#[command(about = "Rust implementation of zenlog")]
#[command(
    long_about = "Renlog - Rust implementation of zenlog\nRust-based logging tool with command boundary detection"
)]
struct Args {
    #[command(subcommand)]
    command: Commands,

    #[arg(
        long,
        default_value = "info",
        help = "Log level (error/warn/info/debug/trace)"
    )]
    log_level: String,
}

#[derive(Subcommand)]
enum Commands {
    /// Run renlog session
    Log {
        #[arg(long, help = "Directory path for renlog log files")]
        renlog_dir: String,
    },
    /// Show zsh configuration for shell integration
    ShowZshRc,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Commands::ShowZshRc => {
            show_zsh_rc();
            Ok(())
        }
        Commands::Log { renlog_dir } => run_log_session(&renlog_dir, &args.log_level).await,
    }
}

async fn run_log_session(renlog_dir: &str, log_level: &str) -> Result<()> {
    // Construct the log file path in the renlog log directory
    let pid = std::process::id();
    let timestamp = Utc::now().format("%Y%m%d_%H%M%S_%3f").to_string();
    let log_filename = format!("renlog_{}_{}.log", timestamp, pid);
    let log_file_path = Path::new(renlog_dir).join(&log_filename);
    let log_file_str = log_file_path.to_string_lossy();

    // Create the log directory if it doesn't exist
    std::fs::create_dir_all(renlog_dir).map_err(|e| {
        std::io::Error::other(format!(
            "Failed to create renlog directory '{}': {}",
            renlog_dir, e
        ))
    })?;

    // Initialize logging system first
    logging::init_logging(&log_file_str, log_level)?;

    // Create symlink for renlog-latest.log pointing to the current renlog internal log file
    let renlog_latest_path = Path::new(renlog_dir).join("renlog-latest.log");
    if let Err(e) = create_or_update_symlink(&log_filename, &renlog_latest_path) {
        log::warn!("Failed to create renlog-latest.log symlink: {}", e);
    }

    log::info!("Starting Renlog (Rust implementation of zenlog)...");
    println!("Starting Renlog (Rust implementation of zenlog)...");

    // Create and initialize session
    let mut session = Session::new(renlog_dir)?;

    // Start child process in PTY
    session.start_child()?;

    // Start I/O forwarders
    session.start_forwarders()?;

    // Run the main session loop (simplified for Phase 1)
    println!("Session initialized. Type 'exit' to quit.");

    // For now, just run the session loop and wait for child
    let exit_code = session.wait_for_child()?;

    log::info!("Renlog session ended with exit code: {}", exit_code);
    println!("Renlog session ended with exit code: {exit_code}");
    Ok(())
}

fn show_zsh_rc() {
    println!(
        r#"
#==============================================================================
# Renlog Zsh setup script
#
# Add the following line to your $HOME/.zshrc file:
#
# eval "$(renlog show-zsh-rc)"
#==============================================================================

#==============================================================================
# Basic helper functions
#==============================================================================

# Execute this command in Zsh's preexec_functions to tell renlog to start logging for a command.
function renlog_zsh_preexec() {{
    local cmd="$3" # Current command line.
    if [[ -n "$RENLOG_PID" ]]; then
        # Send command marker to renlog via invisible stdout
        printf '{} %s\n' "$cmd"
        # Move cursor up and clear the line to make it invisible
        echo -ne "\e[A\e[2K"
    fi
}}

# Execute this command in Zsh's precmd_functions to tell renlog to stop logging for the current command.
function renlog_zsh_postexec() {{
    local last_status=$?
    if [[ -n "$RENLOG_PID" ]]; then
        # Send end command marker to renlog via invisible stdout
        printf '{} %d\n' "$last_status"
        # Move cursor up and clear the line to make it invisible
        echo -ne "\e[A\e[2K"
    fi
}}

#==============================================================================
# Set up pre/post exec hooks if we're running inside renlog
#==============================================================================

if [[ -n "$RENLOG_PID" ]]; then
    # By default, zsh always prints a percent followed by a bunch of spaces
    # before prompt, which will be picked up by renlog.
    # Disable it.
    # https://unix.stackexchange.com/questions/167582/
    unsetopt prompt_cr prompt_sp

    precmd_functions=(renlog_zsh_postexec "$precmd_functions[@]")
    preexec_functions+=(renlog_zsh_preexec)
    
    echo "Renlog zsh integration loaded (PID: $RENLOG_PID)"
fi

"#,
        START_COMMAND_NAME, END_COMMAND_NAME
    );
}
