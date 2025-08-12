mod ansi_filter;
mod command_logger;
mod command_markers;
mod fifo;
mod io_forward;
mod logging;
mod pty;
mod session;
mod symlink_utils;
mod zsh_config;

use chrono::Utc;
use clap::{Parser, Subcommand};
use session::Session;
use std::io::Result;
use std::path::Path;
use symlink_utils::create_or_update_symlink;
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
    let log_filename = format!("renlog_{}.log", timestamp);
    let log_file_path = Path::new(renlog_dir).join(&log_filename);
    let log_file_str = log_file_path.to_string_lossy();

    // Create the log directory if it doesn't exist
    std::fs::create_dir_all(renlog_dir).map_err(|e| {
        std::io::Error::other(format!(
            "Failed to create renlog directory '{}': {}",
            renlog_dir, e
        ))
    })?;

    logging::init_logging(&log_file_str, log_level)?;

    // Create symlink for renlog-latest.log pointing to the current renlog internal log file
    let renlog_latest_path = Path::new(renlog_dir).join("renlog-latest.log");
    if let Err(e) = create_or_update_symlink(&log_filename, &renlog_latest_path) {
        log::warn!("Failed to create renlog-latest.log symlink: {}", e);
    }

    log::info!("Starting Renlog (Rust implementation of zenlog)...");
    println!("Starting Renlog (Rust implementation of zenlog)...");

    let mut session = Session::new(renlog_dir)?;

    session.start_child()?;

    session.start_forwarders()?;

    println!("Session initialized. Type 'exit' to quit.");

    let exit_code = session.wait_for_child()?;

    log::info!("Renlog session ended with exit code: {}", exit_code);
    println!("Renlog session ended with exit code: {exit_code}");
    Ok(())
}
