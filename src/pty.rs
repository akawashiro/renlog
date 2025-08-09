use nix::pty::{openpty, Winsize};
use nix::sys::termios::{tcgetattr, tcsetattr, SetArg, Termios};
use nix::unistd::{close, dup2};
use std::fs::File;
use std::io::{self, Result};
use std::os::fd::OwnedFd;
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};

pub struct PtyPair {
    pub master: File,
    pub slave: File,
    master_fd: OwnedFd,
    slave_fd: OwnedFd,
}

impl PtyPair {
    pub fn new() -> Result<Self> {
        let winsize = Winsize {
            ws_row: 24,
            ws_col: 80,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };

        let pty_result = openpty(Some(&winsize), None).map_err(|e| {
            io::Error::new(io::ErrorKind::Other, format!("Failed to create PTY: {}", e))
        })?;

        let master_fd = pty_result.master;
        let slave_fd = pty_result.slave;

        let master = unsafe { File::from_raw_fd(master_fd.as_raw_fd()) };
        let slave = unsafe { File::from_raw_fd(slave_fd.as_raw_fd()) };

        Ok(PtyPair {
            master,
            slave,
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

    pub fn setup_slave_for_child(&self) -> Result<()> {
        // Set up the slave side for the child process
        // Redirect stdin, stdout, stderr to the slave
        let slave_raw = self.slave_fd.as_raw_fd();
        dup2(slave_raw, 0).map_err(|e| {
            io::Error::new(io::ErrorKind::Other, format!("dup2 stdin failed: {}", e))
        })?;
        dup2(slave_raw, 1).map_err(|e| {
            io::Error::new(io::ErrorKind::Other, format!("dup2 stdout failed: {}", e))
        })?;
        dup2(slave_raw, 2).map_err(|e| {
            io::Error::new(io::ErrorKind::Other, format!("dup2 stderr failed: {}", e))
        })?;

        // Close the original slave fd as it's now duplicated
        let slave_raw = self.slave_fd.as_raw_fd();
        if slave_raw > 2 {
            close(slave_raw).map_err(|e| {
                io::Error::new(io::ErrorKind::Other, format!("close slave failed: {}", e))
            })?;
        }

        Ok(())
    }
}

pub fn set_raw_mode(fd: RawFd) -> Result<Termios> {
    let owned_fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let mut termios = tcgetattr(&owned_fd)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("tcgetattr failed: {}", e)))?;

    let original = termios.clone();

    nix::sys::termios::cfmakeraw(&mut termios);

    tcsetattr(&owned_fd, SetArg::TCSANOW, &termios)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("tcsetattr failed: {}", e)))?;

    std::mem::forget(owned_fd); // Don't close the fd
    Ok(original)
}

pub fn restore_termios(fd: RawFd, termios: &Termios) -> Result<()> {
    let owned_fd = unsafe { OwnedFd::from_raw_fd(fd) };
    tcsetattr(&owned_fd, SetArg::TCSANOW, termios).map_err(|e| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("restore termios failed: {}", e),
        )
    })?;
    std::mem::forget(owned_fd); // Don't close the fd
    Ok(())
}
