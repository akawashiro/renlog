use nix::pty::{openpty, Winsize};
use nix::sys::termios::{tcgetattr, tcsetattr, SetArg, Termios};
use std::io::{self, Result};
use std::os::fd::OwnedFd;
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};

pub struct PtyPair {
    master_fd: OwnedFd,
    slave_fd: OwnedFd,
}

impl PtyPair {
    pub fn new() -> Result<Self> {
        log::debug!("Creating new PTY pair");
        let winsize = Winsize {
            ws_row: 24,
            ws_col: 80,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        log::debug!("PTY window size: {}x{}", winsize.ws_col, winsize.ws_row);

        let pty_result = openpty(Some(&winsize), None).map_err(|e| {
            log::error!("Failed to create PTY: {}", e);
            io::Error::other(format!("Failed to create PTY: {e}"))
        })?;

        let master_fd = pty_result.master;
        let slave_fd = pty_result.slave;

        log::info!(
            "PTY pair created successfully: master_fd={}, slave_fd={}",
            master_fd.as_raw_fd(),
            slave_fd.as_raw_fd()
        );

        Ok(PtyPair {
            master_fd,
            slave_fd,
        })
    }

    pub fn master_fd(&self) -> RawFd {
        self.master_fd.as_raw_fd()
    }

    pub fn slave_fd(&self) -> RawFd {
        self.slave_fd.as_raw_fd()
    }
}

pub fn set_raw_mode(fd: RawFd) -> Result<Termios> {
    log::debug!("Setting raw mode for fd: {}", fd);
    let owned_fd = unsafe { OwnedFd::from_raw_fd(fd) };

    let mut termios = tcgetattr(&owned_fd).map_err(|e| {
        log::error!("tcgetattr failed for fd {}: {}", fd, e);
        io::Error::other(format!("tcgetattr failed: {e}"))
    })?;

    let original = termios.clone();
    log::debug!("Saved original terminal attributes for fd: {}", fd);

    nix::sys::termios::cfmakeraw(&mut termios);
    log::debug!("Applied raw mode transformation to termios");

    tcsetattr(&owned_fd, SetArg::TCSANOW, &termios).map_err(|e| {
        log::error!("tcsetattr failed for fd {}: {}", fd, e);
        io::Error::other(format!("tcsetattr failed: {e}"))
    })?;

    log::info!("Terminal fd {} set to raw mode successfully", fd);
    std::mem::forget(owned_fd); // Don't close the fd
    Ok(original)
}

pub fn restore_termios(fd: RawFd, termios: &Termios) -> Result<()> {
    log::debug!("Restoring terminal settings for fd: {}", fd);
    let owned_fd = unsafe { OwnedFd::from_raw_fd(fd) };

    tcsetattr(&owned_fd, SetArg::TCSANOW, termios).map_err(|e| {
        log::error!("restore termios failed for fd {}: {}", fd, e);
        io::Error::other(format!("restore termios failed: {e}"))
    })?;

    log::info!("Terminal settings restored successfully for fd: {}", fd);
    std::mem::forget(owned_fd); // Don't close the fd
    Ok(())
}
