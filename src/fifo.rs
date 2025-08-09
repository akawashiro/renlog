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
        let forward_path = PathBuf::from(format!("{temp_dir}/renlog.{pid}.f.pipe"));
        let reverse_path = PathBuf::from(format!("{temp_dir}/renlog.{pid}.r.pipe"));

        // Remove existing pipes if they exist
        let _ = remove_file(&forward_path);
        let _ = remove_file(&reverse_path);

        // Create the FIFOs using libc directly since nix doesn't expose mkfifo
        let forward_c_path = std::ffi::CString::new(forward_path.to_str().unwrap()).unwrap();
        let reverse_c_path = std::ffi::CString::new(reverse_path.to_str().unwrap()).unwrap();

        unsafe {
            if libc::mkfifo(forward_c_path.as_ptr(), 0o600) != 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::mkfifo(reverse_c_path.as_ptr(), 0o600) != 0 {
                return Err(io::Error::last_os_error());
            }
        }

        Ok(FifoPair {
            forward_path,
            reverse_path,
            forward_file: None,
            reverse_file: None,
        })
    }

    pub fn open(&mut self) -> Result<()> {
        // Open the forward pipe for reading/writing (non-blocking for now)
        let forward_fd = open(&self.forward_path, OFlag::O_RDWR, Mode::empty())
            .map_err(|e| io::Error::other(format!("Failed to open forward FIFO: {e}")))?;

        let reverse_fd = open(&self.reverse_path, OFlag::O_RDWR, Mode::empty())
            .map_err(|e| io::Error::other(format!("Failed to open reverse FIFO: {e}")))?;

        self.forward_file = Some(unsafe { File::from_raw_fd(forward_fd) });
        self.reverse_file = Some(unsafe { File::from_raw_fd(reverse_fd) });

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
        // Clean up the FIFO files
        let _ = remove_file(&self.forward_path);
        let _ = remove_file(&self.reverse_path);
    }
}
