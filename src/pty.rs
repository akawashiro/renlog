use nix::pty::{openpty, Winsize};
use nix::sys::termios::{tcgetattr, tcsetattr, SetArg, Termios};
use std::io::{self, Result};
use std::mem;
use std::os::fd::OwnedFd;
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};

pub struct PtyPair {
    master_fd: OwnedFd,
    slave_fd: OwnedFd,
}

impl PtyPair {
    pub fn new() -> Result<Self> {
        log::debug!("Creating new PTY pair");

        // Try to get the current terminal size from stdin, fall back to defaults if not a TTY
        let winsize = if nix::unistd::isatty(libc::STDIN_FILENO).unwrap_or(false) {
            match get_window_size(libc::STDIN_FILENO) {
                Ok(size) => {
                    log::debug!(
                        "Using current terminal size: {}x{}",
                        size.ws_col,
                        size.ws_row
                    );
                    size
                }
                Err(e) => {
                    log::warn!(
                        "Failed to get terminal size from stdin: {e}, using defaults"
                    );
                    Winsize {
                        ws_row: 24,
                        ws_col: 80,
                        ws_xpixel: 0,
                        ws_ypixel: 0,
                    }
                }
            }
        } else {
            log::debug!("Stdin is not a TTY, using default size");
            Winsize {
                ws_row: 24,
                ws_col: 80,
                ws_xpixel: 0,
                ws_ypixel: 0,
            }
        };

        log::debug!("PTY window size: {}x{}", winsize.ws_col, winsize.ws_row);

        let pty_result = openpty(Some(&winsize), None).map_err(|e| {
            log::error!("Failed to create PTY: {e}");
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
    log::debug!("Setting raw mode for fd: {fd}");
    let owned_fd = unsafe { OwnedFd::from_raw_fd(fd) };

    let mut termios = tcgetattr(&owned_fd).map_err(|e| {
        log::error!("tcgetattr failed for fd {fd}: {e}");
        io::Error::other(format!("tcgetattr failed: {e}"))
    })?;

    let original = termios.clone();
    log::debug!("Saved original terminal attributes for fd: {fd}");

    nix::sys::termios::cfmakeraw(&mut termios);
    log::debug!("Applied raw mode transformation to termios");

    tcsetattr(&owned_fd, SetArg::TCSANOW, &termios).map_err(|e| {
        log::error!("tcsetattr failed for fd {fd}: {e}");
        io::Error::other(format!("tcsetattr failed: {e}"))
    })?;

    log::info!("Terminal fd {fd} set to raw mode successfully");
    std::mem::forget(owned_fd); // Don't close the fd
    Ok(original)
}

pub fn restore_termios(fd: RawFd, termios: &Termios) -> Result<()> {
    log::debug!("Restoring terminal settings for fd: {fd}");
    let owned_fd = unsafe { OwnedFd::from_raw_fd(fd) };

    tcsetattr(&owned_fd, SetArg::TCSANOW, termios).map_err(|e| {
        log::error!("restore termios failed for fd {fd}: {e}");
        io::Error::other(format!("restore termios failed: {e}"))
    })?;

    log::info!("Terminal settings restored successfully for fd: {fd}");
    std::mem::forget(owned_fd); // Don't close the fd
    Ok(())
}

pub fn get_window_size(fd: RawFd) -> Result<Winsize> {
    log::debug!("Getting window size for fd: {fd}");

    let mut winsize: Winsize = unsafe { mem::zeroed() };

    let result = unsafe {
        libc::ioctl(
            fd,
            libc::TIOCGWINSZ,
            &mut winsize as *mut Winsize as *mut libc::c_void,
        )
    };

    if result < 0 {
        let err = io::Error::last_os_error();
        log::error!("Failed to get window size for fd {fd}: {err}");
        return Err(err);
    }

    log::debug!(
        "Got window size for fd {}: {}x{} (pixels: {}x{})",
        fd,
        winsize.ws_col,
        winsize.ws_row,
        winsize.ws_xpixel,
        winsize.ws_ypixel
    );

    Ok(winsize)
}

pub fn set_window_size(fd: RawFd, winsize: &Winsize) -> Result<()> {
    log::debug!(
        "Setting window size for fd {}: {}x{} (pixels: {}x{})",
        fd,
        winsize.ws_col,
        winsize.ws_row,
        winsize.ws_xpixel,
        winsize.ws_ypixel
    );

    let result = unsafe {
        libc::ioctl(
            fd,
            libc::TIOCSWINSZ,
            winsize as *const Winsize as *const libc::c_void,
        )
    };

    if result < 0 {
        let err = io::Error::last_os_error();
        log::error!("Failed to set window size for fd {fd}: {err}");
        return Err(err);
    }

    log::info!("Window size set successfully for fd {fd}");
    Ok(())
}

pub fn propagate_terminal_size(from_fd: RawFd, to_fd: RawFd) -> Result<()> {
    log::debug!(
        "Propagating terminal size from fd {from_fd} to fd {to_fd}"
    );

    let winsize = get_window_size(from_fd)?;
    set_window_size(to_fd, &winsize)?;

    log::info!(
        "Terminal size propagated successfully from fd {} to fd {} ({}x{})",
        from_fd,
        to_fd,
        winsize.ws_col,
        winsize.ws_row
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_window_size_on_stdin() {
        // This test will only pass if stdin is a TTY
        if nix::unistd::isatty(libc::STDIN_FILENO).unwrap_or(false) {
            let result = get_window_size(libc::STDIN_FILENO);
            assert!(
                result.is_ok(),
                "Should be able to get window size from TTY stdin"
            );

            let winsize = result.unwrap();
            assert!(winsize.ws_row > 0, "TTY should have non-zero rows");
            assert!(winsize.ws_col > 0, "TTY should have non-zero columns");
        } else {
            println!("Skipping test - stdin is not a TTY");
        }
    }

    #[test]
    fn test_window_size_propagation() {
        // Create a PTY pair
        let pty = PtyPair::new().expect("Failed to create PTY pair");

        // Get the current window size
        let master_fd = pty.master_fd();
        let initial_size = get_window_size(master_fd).expect("Should get initial PTY window size");

        // Verify we got a reasonable size
        assert!(initial_size.ws_row > 0);
        assert!(initial_size.ws_col > 0);

        // Set a new size
        let new_size = Winsize {
            ws_row: 40,
            ws_col: 120,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };

        set_window_size(master_fd, &new_size).expect("Should be able to set window size");

        // Verify the size was set
        let updated_size = get_window_size(master_fd).expect("Should get updated PTY window size");

        assert_eq!(updated_size.ws_row, 40);
        assert_eq!(updated_size.ws_col, 120);
    }
}
