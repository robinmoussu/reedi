//! The state of a rebase in progress.

use std::fs;
use std::io;
use std::path::Path;

use gix_hash::ObjectId;

/// The files holding the commit the commits are replayed onto, relative to the git directory:
/// for `git rebase --merge` (the default), then for `git rebase --apply`.
const ONTO_FILES: [&str; 2] = ["rebase-merge/onto", "rebase-apply/onto"];

/// The commit the commits are replayed onto by the rebase in progress in the repository whose git
/// directory is `git_dir`, or `None` if no rebase is in progress.
///
/// Fails if the file holding it cannot be read, or doesn't hold a hash.
pub fn onto(git_dir: &Path) -> io::Result<Option<ObjectId>> {
    for onto_file in ONTO_FILES {
        match fs::read(git_dir.join(onto_file)) {
            Ok(content) => {
                return ObjectId::from_hex(content.trim_ascii_end())
                    .map(Some)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}
