mod command_logger;
mod io_forward;
mod log_processor;
mod logging;
mod pty;
mod session;
mod shell_config;
mod signals;
mod symlink_utils;

use chrono::Utc;
use clap::{Parser, Subcommand};
use session::Session;
use shell_config::{show_shell_rc, ShellType};
use signals::SignalHandler;
use std::io::Result;
use std::path::Path;
use std::sync::{Arc, Mutex};
use symlink_utils::create_or_update_symlink;

#[derive(Parser)]
#[command(name = "renlog")]
#[command(about = "Rust implementation of zenlog")]
#[command(
    long_about = "renlog - Rust implementation of zenlog\nRust-based logging tool with command boundary detection"
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
        #[arg(
            long,
            help = "Custom shell command to execute (e.g., 'zsh --no-globalrcs')"
        )]
        cmd: Option<String>,
        #[arg(long, help = "Log filter just before save log file")]
        log_filter_cmd: Option<String>,
    },
    /// Show bash configuration for shell integration
    ShowBashRc,
    /// Show zsh configuration for shell integration
    ShowZshRc,
}

fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Commands::ShowBashRc => {
            show_shell_rc(ShellType::Bash);
            Ok(())
        }
        Commands::ShowZshRc => {
            show_shell_rc(ShellType::Zsh);
            Ok(())
        }
        Commands::Log {
            renlog_dir,
            cmd,
            log_filter_cmd,
        } => {
            let pid = std::process::id();
            let renlog_dir = Path::new(&renlog_dir).join(format!("{pid}"));
            setup_logging_and_run_session(
                &renlog_dir,
                &args.log_level,
                cmd.as_deref(),
                log_filter_cmd.as_deref(),
            )
        }
    }
}

fn setup_logging_and_run_session(
    renlog_dir: &Path,
    log_level: &str,
    user_specified_shell_cmd: Option<&str>,
    log_filter_cmd: Option<&str>,
) -> Result<()> {
    let timestamp = Utc::now().format("%Y%m%d_%H%M%S_%3f").to_string();
    let log_filename = format!("renlog_{timestamp}.log");
    let log_file_path = Path::new(renlog_dir).join(&log_filename);
    let log_file_str = log_file_path.to_string_lossy();

    std::fs::create_dir_all(renlog_dir).map_err(|e| {
        std::io::Error::other(format!(
            "Failed to create renlog directory '{renlog_dir:?}': {e}"
        ))
    })?;

    logging::init_logging(&log_file_str, log_level)?;

    let renlog_latest_path = Path::new(renlog_dir).join("renlog-latest.log");
    if let Err(e) = create_or_update_symlink(&log_filename, &renlog_latest_path) {
        log::warn!("Failed to create renlog-latest.log symlink: {e}");
    }

    log::info!("Starting renlog...");
    println!("Starting renlog...");
    let result = run_pty_session(renlog_dir, user_specified_shell_cmd, log_filter_cmd);

    match result {
        Ok(exit_code) => {
            log::info!("renlog session ended with exit code: {exit_code}");
            println!("renlog session ended with exit code: {exit_code}");
            Ok(())
        }
        Err(e) => {
            log::error!("renlog session failed: {e}");
            eprintln!("renlog failed to start: {e}");
            std::process::exit(1);
        }
    }
}

fn run_pty_session(
    renlog_dir: &Path,
    user_specified_shell_cmd: Option<&str>,
    log_filter_cmd: Option<&str>,
) -> Result<i32> {
    let session = Session::new(renlog_dir, user_specified_shell_cmd, log_filter_cmd)?;
    session.start_child()?;
    session.start_forwarders()?;

    // Set up signal handling with all signals
    let master_fd = Arc::new(Mutex::new(Some(session.get_master_fd())));
    let session_callbacks = session.get_session_callbacks();
    let signal_handler = SignalHandler::new()?;

    // Spawn signal handler thread
    let signal_thread = signal_handler.start_handler_thread(master_fd.clone(), session_callbacks);

    let exit_code = session.wait_for_child()?;

    // Clear the master fd to stop signal handling
    {
        let mut fd_guard = master_fd.lock().unwrap();
        *fd_guard = None;
    }

    // The signal handler thread will exit when it detects the fd is None
    // Wait for it to finish
    if let Err(e) = signal_thread.join() {
        log::warn!("Signal handler thread panicked: {e:?}");
    }

    Ok(exit_code)
}
