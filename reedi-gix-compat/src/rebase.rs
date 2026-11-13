//! The state of a rebase in progress.

use std::fs;
use std::io;
use std::path::Path;

use gix::refs::FullName;
use gix_hash::ObjectId;

/// The files holding the commit the commits are replayed onto, relative to the git directory:
/// for `git rebase --merge` (the default), then for `git rebase --apply`.
const ONTO_FILES: [&str; 2] = ["rebase-merge/onto", "rebase-apply/onto"];

/// The files holding the rebased branch, relative to the git directory, like `ONTO_FILES`.
const HEAD_NAME_FILES: [&str; 2] = ["rebase-merge/head-name", "rebase-apply/head-name"];

/// The content of the first of `files` (relative to `git_dir`) that exists, without its final
/// newline, or `None` if none exists.
fn first_existing(git_dir: &Path, files: [&str; 2]) -> io::Result<Option<Vec<u8>>> {
    for file in files {
        match fs::read(git_dir.join(file)) {
            Ok(content) => return Ok(Some(content.trim_ascii_end().to_vec())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

/// The commit the commits are replayed onto by the rebase in progress in the repository whose git
/// directory is `git_dir`, or `None` if no rebase is in progress.
///
/// Fails if the file holding it cannot be read, or doesn't hold a hash.
pub fn onto(git_dir: &Path) -> io::Result<Option<ObjectId>> {
    first_existing(git_dir, ONTO_FILES)?
        .map(|content| {
            ObjectId::from_hex(&content)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
        })
        .transpose()
}

/// The branch rebased by the rebase in progress in the repository whose git directory is
/// `git_dir`, or `None` if no rebase is in progress or if it rebases a detached HEAD.
///
/// Fails if the file holding it cannot be read.
pub fn rebased_branch(git_dir: &Path) -> io::Result<Option<FullName>> {
    // A detached HEAD is written `detached HEAD`, which is not a reference name.
    Ok(first_existing(git_dir, HEAD_NAME_FILES)?
        .and_then(|content| FullName::try_from(gix::bstr::BString::from(content)).ok()))
}
