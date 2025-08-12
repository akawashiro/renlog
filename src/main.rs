mod ansi_filter;
mod command_logger;
mod command_markers;
mod emergency_shell;
mod fifo;
mod io_forward;
mod logging;
mod pty;
mod session;
mod signals;
mod symlink_utils;
mod zsh_config;

use chrono::Utc;
use clap::{Parser, Subcommand};
use command_logger::truncate_filename_to_limit;
use emergency_shell::EmergencyShell;
use session::Session;
use signals::SignalHandler;
use std::io::Result;
use std::path::Path;
use std::sync::Arc;
use symlink_utils::create_or_update_symlink;
use tokio::sync::Mutex;
use zsh_config::show_zsh_rc;

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
    let timestamp = Utc::now().format("%Y%m%d_%H%M%S_%3f").to_string();
    let log_filename_raw = format!("renlog_{timestamp}.log");
    let log_filename = truncate_filename_to_limit(&log_filename_raw);
    let log_file_path = Path::new(renlog_dir).join(&log_filename);
    let log_file_str = log_file_path.to_string_lossy();

    // Create the log directory if it doesn't exist
    std::fs::create_dir_all(renlog_dir).map_err(|e| {
        std::io::Error::other(format!(
            "Failed to create renlog directory '{renlog_dir}': {e}"
        ))
    })?;

    logging::init_logging(&log_file_str, log_level)?;

    // Create symlink for renlog-latest.log pointing to the current renlog internal log file
    let renlog_latest_path = Path::new(renlog_dir).join("renlog-latest.log");
    if let Err(e) = create_or_update_symlink(&log_filename, &renlog_latest_path) {
        log::warn!("Failed to create renlog-latest.log symlink: {e}");
    }

    log::info!("Starting Renlog (Rust implementation of zenlog)...");
    println!("Starting Renlog (Rust implementation of zenlog)...");

    // Create emergency shell for fallback
    let emergency_shell = EmergencyShell::new();

    // Try to start renlog session with emergency fallback
    let result = run_renlog_session(renlog_dir).await;

    match result {
        Ok(exit_code) => {
            log::info!("Renlog session ended with exit code: {exit_code}");
            println!("Renlog session ended with exit code: {exit_code}");
            Ok(())
        }
        Err(e) => {
            log::error!("Renlog session failed: {e}");
            eprintln!("Renlog failed to start: {e}");

            // Fall back to emergency shell
            match emergency_shell.start_with_error(&format!("Renlog initialization failed: {e}"))
            {
                Ok(status) => {
                    let exit_code = status.code().unwrap_or(1);
                    log::info!("Emergency shell exited with code: {exit_code}");
                    std::process::exit(exit_code);
                }
                Err(emergency_err) => {
                    log::error!("Emergency shell also failed: {emergency_err}");
                    eprintln!("CRITICAL: Both renlog and emergency shell failed!");
                    eprintln!("Emergency shell error: {emergency_err}");
                    std::process::exit(1);
                }
            }
        }
    }
}

async fn run_renlog_session(renlog_dir: &str) -> Result<i32> {
    let mut session = Session::new(renlog_dir)?;

    session.start_child().await?;

    session.start_forwarders()?;

    // Set up signal handling with all signals
    let master_fd = Arc::new(Mutex::new(Some(session.get_master_fd())));
    let session_callbacks = session.get_session_callbacks();
    let signal_handler = SignalHandler::new()?;

    // Spawn signal handler task
    let signal_task =
        tokio::spawn(signal_handler.handle_signals(master_fd.clone(), session_callbacks));

    println!("Session initialized. Type 'exit' to quit.");

    let exit_code = session.wait_for_child().await?;

    // Clear the master fd to stop signal handling
    {
        let mut fd_guard = master_fd.lock().await;
        *fd_guard = None;
    }

    // Cancel signal handler task
    signal_task.abort();

    Ok(exit_code)
}
