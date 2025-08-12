use std::io::Result;
use std::os::unix::fs;
use std::path::Path;

pub fn create_or_update_symlink(target: &str, symlink_path: &Path) -> Result<()> {
    // Remove existing symlink if it exists
    if symlink_path.exists() || symlink_path.is_symlink() {
        if let Err(e) = std::fs::remove_file(symlink_path) {
            log::warn!(
                "Failed to remove existing symlink {symlink_path:?}: {e}"
            );
        }
    }

    // Create new symlink
    fs::symlink(target, symlink_path).map_err(|e| {
        log::error!(
            "Failed to create symlink {symlink_path:?} -> {target}: {e}"
        );
        std::io::Error::other(format!(
            "Failed to create symlink {symlink_path:?} -> {target}: {e}"
        ))
    })?;

    log::info!("Created symlink {symlink_path:?} pointing to: {target}");
    Ok(())
}
