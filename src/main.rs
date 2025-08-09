mod fifo;
mod io_forward;
mod logger;
mod pty;

use logger::Logger;
use std::env;
use std::io::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() > 1 && args[1] == "--help" {
        print_help();
        return Ok(());
    }

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

fn print_help() {
    println!("Renlog - Rust implementation of zenlog");
    println!("Usage: renlog [options]");
    println!("");
    println!("Options:");
    println!("  --help    Show this help message");
    println!("");
    println!("Phase 1 implementation - basic PTY and I/O forwarding");
}
