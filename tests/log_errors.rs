//! The errors of reading the commits of the repository, displayed by `reedi` before it enters the
//! alternate screen.

use std::path::Path;

use reedi::test_git::BRANCH;
use reedi::test_git::DEFAULT_BRANCH;
use reedi::test_git::Git;
use reedi::test_git::ORIGIN;
use reedi::test_git::RebaseBackend;
use reedi::test_git::isolated_command;

/// The exit code of `reedi` when it fails.
const FAILURE_EXIT_CODE: i32 = 1;

/// The stderr of `reedi` run in `directory`, which must fail.
fn error_in(directory: &Path) -> String {
    let output = isolated_command(env!("CARGO_BIN_EXE_reedi"), directory)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(FAILURE_EXIT_CODE));
    String::from_utf8(output.stderr).unwrap()
}

#[test]
fn outside_a_repository_is_an_error() {
    let directory = tempfile::tempdir().unwrap();
    assert_eq!(
        error_in(directory.path()),
        "reedi: not in a git repository\n"
    );
}

#[test]
fn branch_without_commits_is_an_error() {
    let git = Git::on_branch();
    assert_eq!(error_in(git.path()), "reedi: no commits on topic\n");
}

/// The repository has no remote.
#[test]
fn branch_without_upstream_nor_default_remote_is_an_error() {
    let git = Git::on_branch();
    git.commit_all(b"First");
    assert_eq!(
        error_in(git.path()),
        "reedi: branch topic has no upstream and there is no default remote\n"
    );
}

/// The repository has no remote.
#[test]
fn detached_head_without_default_remote_is_an_error() {
    let git = Git::on_branch();
    git.commit_all(b"First");
    git.detach_head();
    assert_eq!(
        error_in(git.path()),
        "reedi: HEAD is detached and there is no default remote\n"
    );
}

/// The only remote, `origin`, has a branch but no HEAD.
#[test]
fn branch_without_upstream_nor_head_of_the_default_remote_is_an_error() {
    let git = Git::on_branch();
    let head = git.commit_all(b"First");
    git.set_remote_branch(ORIGIN, DEFAULT_BRANCH, &head);
    assert_eq!(
        error_in(git.path()),
        "reedi: branch topic has no upstream and there is no origin/HEAD\n"
    );
}

/// The only remote, `origin`, has a branch but no HEAD.
#[test]
fn detached_head_without_head_of_the_default_remote_is_an_error() {
    let git = Git::on_branch();
    let head = git.commit_all(b"First");
    git.set_remote_branch(ORIGIN, DEFAULT_BRANCH, &head);
    git.detach_head();
    assert_eq!(
        error_in(git.path()),
        "reedi: HEAD is detached and there is no origin/HEAD\n"
    );
}

#[test]
fn upstream_never_fetched_is_an_error() {
    let git = Git::on_branch();
    let head = git.commit_all(b"First");
    git.set_remote_branch(ORIGIN, DEFAULT_BRANCH, &head);
    git.set_upstream(BRANCH, ORIGIN, BRANCH);
    assert_eq!(
        error_in(git.path()),
        "reedi: the upstream origin/topic of branch topic was never fetched\n"
    );
}

#[test]
fn no_commits_after_the_upstream_of_the_same_name_is_an_error() {
    let git = Git::with_upstream();
    assert_eq!(
        error_in(git.path()),
        "reedi: no unpushed commits on topic\n"
    );
}

#[test]
fn no_commits_after_the_upstream_of_another_name_is_an_error() {
    let git = Git::on_branch();
    let head = git.commit_all(b"First");
    git.set_remote_branch(ORIGIN, DEFAULT_BRANCH, &head);
    git.set_upstream(BRANCH, ORIGIN, DEFAULT_BRANCH);
    assert_eq!(
        error_in(git.path()),
        "reedi: no commits on topic that are not on origin/main\n"
    );
}

#[test]
fn no_commits_after_origin_head_is_an_error() {
    let git = Git::with_origin_head();
    assert_eq!(
        error_in(git.path()),
        "reedi: no commits on topic that are not on origin/HEAD\n"
    );
}

#[test]
fn no_commits_after_origin_head_with_detached_head_is_an_error() {
    let git = Git::with_origin_head();
    git.detach_head();
    assert_eq!(
        error_in(git.path()),
        "reedi: no commits on HEAD that are not on origin/HEAD\n"
    );
}

#[test]
fn no_commits_replayed_during_a_rebase_is_an_error() {
    for backend in [RebaseBackend::Merge, RebaseBackend::Apply] {
        let git = Git::on_branch();
        let onto =
            git.start_rebase_stopped_by_conflict(backend, /* replayed_commits_count */ 0);
        let short_hash = git.stdout_of(&["log", "-1", "--format=format:%h", &onto]);
        assert_eq!(
            error_in(git.path()),
            format!("reedi: no commits replayed yet onto {short_hash}\n"),
            "{backend:?}"
        );
    }
}
