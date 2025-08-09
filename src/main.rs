mod fifo;
mod io_forward;
mod logger;
mod pty;

use clap::Parser;
use logger::Logger;
use std::io::Result;

#[derive(Parser)]
#[command(name = "renlog")]
#[command(about = "Rust implementation of zenlog")]
#[command(long_about = "Renlog - Rust implementation of zenlog\nPhase 1 implementation - basic PTY and I/O forwarding")]
struct Args {}

#[tokio::main]
async fn main() -> Result<()> {
    let _args = Args::parse();

    println!("Starting Renlog (Rust implementation of zenlog)...");

    // Create and initialize logger
    let mut logger = Logger::new()?;

    // Start child process in PTY
    logger.start_child()?;

    // Start I/O forwarders
    logger.start_forwarders()?;

    // Run the main logger loop (simplified for Phase 1)
    println!("Logger initialized. Type 'exit' to quit.");

    // For now, just run the logger loop and wait for child
    let exit_code = logger.wait_for_child()?;

    println!("Renlog session ended with exit code: {}", exit_code);
    Ok(())
}

