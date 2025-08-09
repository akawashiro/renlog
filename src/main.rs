mod ansi_filter;
mod command_logger;
mod fifo;
mod io_forward;
mod logging;
mod pty;
mod session;

use chrono::Utc;
use clap::Parser;
use session::Session;
use std::io::Result;
use std::path::Path;
use std::os::unix::fs;

#[derive(Parser)]
#[command(name = "renlog")]
#[command(about = "Rust implementation of zenlog")]
#[command(
    long_about = "Renlog - Rust implementation of zenlog\nPhase 1 implementation - basic PTY and I/O forwarding"
)]
struct Args {
    #[arg(long, help = "Directory path for renlog log files (mandatory)")]
    renlog_dir: String,

    #[arg(
        long,
        default_value = "info",
        help = "Log level (error/warn/info/debug/trace)"
    )]
    log_level: String,
}

fn create_or_update_symlink(target: &str, symlink_path: &Path) -> Result<()> {
    // Remove existing symlink if it exists
    if symlink_path.exists() || symlink_path.is_symlink() {
        if let Err(e) = std::fs::remove_file(symlink_path) {
            log::warn!("Failed to remove existing symlink {:?}: {}", symlink_path, e);
        }
    }
    
    // Create new symlink
    fs::symlink(target, symlink_path).map_err(|e| {
        log::error!("Failed to create symlink {:?} -> {}: {}", symlink_path, target, e);
        std::io::Error::other(format!("Failed to create symlink {:?} -> {}: {}", symlink_path, target, e))
    })?;
    
    log::info!("Created symlink {:?} pointing to: {}", symlink_path, target);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Construct the log file path in the renlog log directory
    let pid = std::process::id();
    let timestamp = Utc::now().format("%Y%m%d_%H%M%S_%3f").to_string();
    let log_filename = format!("renlog_{}_{}.log", timestamp, pid);
    let log_file_path = Path::new(&args.renlog_dir).join(&log_filename);
    let log_file_str = log_file_path.to_string_lossy();

    // Create the log directory if it doesn't exist
    std::fs::create_dir_all(&args.renlog_dir).map_err(|e| {
        std::io::Error::other(format!(
            "Failed to create renlog directory '{}': {}", 
            &args.renlog_dir, e
        ))
    })?;

    // Initialize logging system first
    logging::init_logging(&log_file_str, &args.log_level)?;

    // Create symlink for renlog-latest.log pointing to the current renlog internal log file
    let renlog_latest_path = Path::new(&args.renlog_dir).join("renlog-latest.log");
    if let Err(e) = create_or_update_symlink(&log_filename, &renlog_latest_path) {
        log::warn!("Failed to create renlog-latest.log symlink: {}", e);
    }

    log::info!("Starting Renlog (Rust implementation of zenlog)...");
    println!("Starting Renlog (Rust implementation of zenlog)...");

    // Create and initialize session
    let mut session = Session::new(&args.renlog_dir)?;

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
