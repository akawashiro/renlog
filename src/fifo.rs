use nix::fcntl::{open, OFlag};
use nix::sys::stat::Mode;
use std::fs::{remove_file, File};
use std::io::{self, Result};
use std::os::unix::io::FromRawFd;
use std::path::PathBuf;

pub struct FifoPair {
    pub forward_path: PathBuf,
    pub reverse_path: PathBuf,
    pub forward_file: Option<File>,
    pub reverse_file: Option<File>,
}

impl FifoPair {
    pub fn new(temp_dir: &str, pid: u32) -> Result<Self> {
        log::debug!(
            "Creating FIFO pair for PID {pid} in temp_dir: {temp_dir}"
        );
        let forward_path = PathBuf::from(format!("{temp_dir}/renlog.{pid}.f.pipe"));
        let reverse_path = PathBuf::from(format!("{temp_dir}/renlog.{pid}.r.pipe"));

        log::debug!(
            "FIFO paths: forward={}, reverse={}",
            forward_path.display(),
            reverse_path.display()
        );

        // Remove existing pipes if they exist
        if forward_path.exists() {
            log::debug!("Removing existing forward pipe: {}", forward_path.display());
            let _ = remove_file(&forward_path);
        }
        if reverse_path.exists() {
            log::debug!("Removing existing reverse pipe: {}", reverse_path.display());
            let _ = remove_file(&reverse_path);
        }

        // Create the FIFOs using libc directly since nix doesn't expose mkfifo
        let forward_c_path = std::ffi::CString::new(forward_path.to_str().unwrap()).unwrap();
        let reverse_c_path = std::ffi::CString::new(reverse_path.to_str().unwrap()).unwrap();

        log::debug!("Creating forward FIFO with mkfifo");
        unsafe {
            if libc::mkfifo(forward_c_path.as_ptr(), 0o600) != 0 {
                let error = io::Error::last_os_error();
                log::error!(
                    "Failed to create forward FIFO {}: {error}",
                    forward_path.display()
                );
                return Err(error);
            }
        }

        log::debug!("Creating reverse FIFO with mkfifo");
        unsafe {
            if libc::mkfifo(reverse_c_path.as_ptr(), 0o600) != 0 {
                let error = io::Error::last_os_error();
                log::error!(
                    "Failed to create reverse FIFO {}: {error}",
                    reverse_path.display()
                );
                return Err(error);
            }
        }

        log::info!(
            "FIFO pair created successfully: forward={}, reverse={}",
            forward_path.display(),
            reverse_path.display()
        );

        Ok(FifoPair {
            forward_path,
            reverse_path,
            forward_file: None,
            reverse_file: None,
        })
    }

    pub fn open(&mut self) -> Result<()> {
        log::debug!("Opening FIFO pair");
        // Open the forward pipe for reading/writing (non-blocking for now)
        log::debug!("Opening forward FIFO: {}", self.forward_path.display());
        let forward_fd = open(&self.forward_path, OFlag::O_RDWR, Mode::empty()).map_err(|e| {
            log::error!(
                "Failed to open forward FIFO {}: {e}",
                self.forward_path.display()
            );
            io::Error::other(format!("Failed to open forward FIFO: {e}"))
        })?;

        log::debug!("Opening reverse FIFO: {}", self.reverse_path.display());
        let reverse_fd = open(&self.reverse_path, OFlag::O_RDWR, Mode::empty()).map_err(|e| {
            log::error!(
                "Failed to open reverse FIFO {}: {e}",
                self.reverse_path.display()
            );
            io::Error::other(format!("Failed to open reverse FIFO: {e}"))
        })?;

        self.forward_file = Some(unsafe { File::from_raw_fd(forward_fd) });
        self.reverse_file = Some(unsafe { File::from_raw_fd(reverse_fd) });

        log::info!(
            "FIFO pair opened successfully: forward_fd={forward_fd}, reverse_fd={reverse_fd}"
        );
        Ok(())
    }

    pub fn forward_path_str(&self) -> &str {
        self.forward_path.to_str().unwrap()
    }

    pub fn reverse_path_str(&self) -> &str {
        self.reverse_path.to_str().unwrap()
    }
}

impl Drop for FifoPair {
    fn drop(&mut self) {
        log::debug!("FifoPair Drop trait called - cleaning up FIFO files");
        // Clean up the FIFO files
        if self.forward_path.exists() {
            log::debug!("Removing forward FIFO: {}", self.forward_path.display());
            if let Err(e) = remove_file(&self.forward_path) {
                log::error!(
                    "Failed to remove forward FIFO {}: {e}",
                    self.forward_path.display()
                );
            }
        }

        if self.reverse_path.exists() {
            log::debug!("Removing reverse FIFO: {}", self.reverse_path.display());
            if let Err(e) = remove_file(&self.reverse_path) {
                log::error!(
                    "Failed to remove reverse FIFO {}: {e}",
                    self.reverse_path.display()
                );
            }
        }

        log::info!("FifoPair cleanup completed");
    }
}
