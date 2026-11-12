//! The upstream of a branch.

use gix::refs::FullName;
use gix::refs::FullNameRef;
use gix::remote::Direction;

/// The value of `branch.<name>.remote` designating the repository itself.
const LOCAL_REMOTE: &str = ".";

/// The upstream of `branch` in `repository`, if it has one: like
/// `gix::Repository::branch_remote_tracking_ref_name()` for fetching, but also when the upstream
/// is a local branch (`branch.<name>.remote = .`), which is then `branch.<name>.merge` itself,
/// like git.
pub fn tracking_branch(
    repository: &gix::Repository,
    branch: &FullNameRef,
) -> Option<gix::Result<FullName>> {
    let is_local = repository
        .branch_remote_name(branch.shorten(), Direction::Fetch)
        .is_some_and(|remote| remote.as_bstr() == LOCAL_REMOTE);
    if is_local {
        repository.branch_remote_ref_name(branch, Direction::Fetch)
    } else {
        repository.branch_remote_tracking_ref_name(branch, Direction::Fetch)
    }
}
