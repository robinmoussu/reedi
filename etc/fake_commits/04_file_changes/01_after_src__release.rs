//! Download and installation of the releases.

use std::fs;
use std::io;
use std::path::Path;

use crate::archive::Archive;

/// Installs the release archive at `path` in `prefix`.
pub fn install(path: &Path, prefix: &Path) -> io::Result<()> {
    let archive = Archive::open(path)?;
    if !archive.has_valid_checksum()? {
        return Err(io::Error::other("the checksum of the archive is invalid"));
    }
    archive.extract(prefix)?;
    Ok(())
}

/// The directory in which the releases are installed by default.
pub fn default_prefix() -> &'static Path {
    Path::new("/usr/local")
}

/// Removes the temporary files of the previous installations.
pub fn clean(directory: &Path) -> io::Result<()> {
    fs::remove_dir_all(directory.join("tmp"))
}
