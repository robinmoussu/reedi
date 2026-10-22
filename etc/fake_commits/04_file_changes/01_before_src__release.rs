//! Download and installation of the releases.

use std::fs;
use std::io;
use std::path::Path;

use crate::archive::Archive;

/// Installs the release archive at `path` in `prefix`.
pub fn install(path: &Path, prefix: &Path) -> io::Result<()> {
    let archive = Archive::open(path)?;
    archive.extract(prefix)?;
    Ok(())
}

/// The directory in which the releases are installed by default.
pub fn default_prefix() -> &'static Path {
    Path::new("/usr/local")
}

/// Removes the temporary files of the previous installations.
pub fn clean(directory: &Path) -> io::Result<()> {
    // Kept for compatibility with the releases older than v0.1.
    let legacy = directory.join("reedi.old");
    if legacy.exists() {
        fs::remove_file(legacy)?;
    }
    fs::remove_dir_all(directory.join("tmp"))
}
