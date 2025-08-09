mod ansi_filter;
mod command_logger;
mod fifo;
mod io_forward;
mod logging;
mod pty;
mod session;

use clap::Parser;
use session::Session;
use std::io::Result;
use std::path::Path;

#[derive(Parser)]
#[command(name = "renlog")]
#[command(about = "Rust implementation of zenlog")]
#[command(
    long_about = "Renlog - Rust implementation of zenlog\nPhase 1 implementation - basic PTY and I/O forwarding"
)]
struct Args {
    #[arg(long, help = "Directory path for renlog log files (mandatory)")]
    renlog_log_dir: String,

    #[arg(
        long,
        default_value = "info",
        help = "Log level (error/warn/info/debug/trace)"
    )]
    log_level: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Construct the log file path in the renlog log directory
    let log_file_path = Path::new(&args.renlog_log_dir).join("renlog.log");
    let log_file_str = log_file_path.to_string_lossy();

    // Initialize logging system first
    logging::init_logging(&log_file_str, &args.log_level)?;

    log::info!("Starting Renlog (Rust implementation of zenlog)...");
    println!("Starting Renlog (Rust implementation of zenlog)...");

    // Create and initialize session
    let mut session = Session::new(&args.renlog_log_dir)?;

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
