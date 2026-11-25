//! Runs the real `git`, to validate that what is documented as done "like git" is.

use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;

use gix::date::Time;
use gix::date::time::format;
use jiff::Timestamp;
use jiff::Zoned;
use jiff::tz::TimeZone;
use reedi_gix_compat::locale::Locale;
use tempfile::TempDir;

use crate::date::Environment;

/// The local branch of the repositories made by [`Git::on_branch()`].
pub const BRANCH: &str = "topic";
pub const ORIGIN: &str = "origin";
/// The branch `origin/HEAD` points to.
pub const DEFAULT_BRANCH: &str = "main";

/// The file, in the git directory, holding the message of the next commit of
/// [`Git::commit_all()`].
const COMMIT_MESSAGE_FILE: &str = ".git/TEST_COMMIT_MESSAGE";
/// The file, in the git directory, holding the commit object written by
/// [`Git::commit_with_raw_message()`].
const RAW_COMMIT_FILE: &str = ".git/TEST_RAW_COMMIT";
/// The file whose content differs on the 2 sides of the rebase of
/// [`Git::start_rebase_stopped_by_conflict()`].
pub const CONFLICTING_FILE: &str = "conflict.txt";

/// `program`, run in `directory` without the configuration of the user (so that the tests don't
/// depend on it), and without looking for a repository above `directory`.
pub fn isolated_command(program: impl AsRef<OsStr>, directory: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .current_dir(directory)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap())
        .env("HOME", directory)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CEILING_DIRECTORIES", directory.parent().unwrap());
    command
}

/// How `git rebase` replays the commits.
#[derive(Debug, Clone, Copy)]
pub enum RebaseBackend {
    /// `--merge`, whose state is in `.git/rebase-merge/`.
    Merge,
    /// `--apply`, whose state is in `.git/rebase-apply/`.
    Apply,
}

/// Where the repository of a [`Git`] is.
#[derive(Debug)]
pub enum Directory {
    /// A new temporary directory, removed with the [`Git`].
    Temporary(TempDir),
    /// A directory (created if it doesn't exist), kept after the [`Git`] is dropped.
    Persistent(PathBuf),
}

impl Directory {
    fn path(&self) -> &Path {
        match self {
            Directory::Temporary(directory) => directory.path(),
            Directory::Persistent(path) => path,
        }
    }
}

/// A new repository, and the environment `git` runs in (see [`isolated_command()`]).
pub struct Git {
    directory: Directory,
    /// The current date (`GIT_TEST_DATE_NOW`), in the current timezone (`TZ`).
    now: Zoned,
    /// `LC_ALL`.
    locale: String,
    /// The number of commits made by [`Git::commit_all()`], which sets their dates.
    commit_all_count: AtomicI64,
}

impl Git {
    /// An empty repository in a temporary directory, where the current date is `now` and the
    /// locale is `locale` (`C`, or the name of an installed locale like `fr_FR.UTF-8`).
    pub fn new(now: &Zoned, locale: &str) -> Self {
        Self::new_in(
            Directory::Temporary(tempfile::tempdir().unwrap()),
            now,
            locale,
        )
    }

    /// An empty repository in `directory`, where the current date is `now` and the locale is
    /// `locale` (`C`, or the name of an installed locale like `fr_FR.UTF-8`).
    pub fn new_in(directory: Directory, now: &Zoned, locale: &str) -> Self {
        fs::create_dir_all(directory.path()).unwrap();
        let git = Git {
            directory,
            now: now.clone(),
            locale: locale.to_string(),
            commit_all_count: AtomicI64::new(0),
        };
        assert!(git.run(&["init", "--quiet"]).status.success());
        git
    }

    /// An empty repository in a temporary directory, on `BRANCH`, for the tests that depend
    /// neither on the current date nor on the locale.
    pub fn on_branch() -> Self {
        Self::on_branch_in(Directory::Temporary(tempfile::tempdir().unwrap()))
    }

    /// An empty repository in `directory`, on `BRANCH`, where the current date and the locale
    /// are the same as with [`Git::on_branch()`].
    pub fn on_branch_in(directory: Directory) -> Self {
        let git = Self::new_in(
            directory,
            &Timestamp::UNIX_EPOCH.to_zoned(TimeZone::get("UTC").unwrap()),
            "C",
        );
        git.switch_to_new_branch(BRANCH, /* start */ None);
        git
    }

    /// A repository on `BRANCH`, whose upstream `origin/topic` is at its only commit.
    pub fn with_upstream() -> Self {
        let git = Self::on_branch();
        git.commit_all(b"Base");
        git.set_upstream_at_head();
        git
    }

    /// A repository on `BRANCH`, without upstream, whose `origin/HEAD` is at its only commit.
    pub fn with_origin_head() -> Self {
        let git = Self::on_branch();
        git.commit_all(b"Base");
        git.set_remote_head_at_head(ORIGIN);
        git
    }

    /// The environment `git` runs in, to display the dates like it.
    pub fn environment(&self) -> Environment {
        Environment {
            now: self.now.clone(),
            locale: Locale::named(&self.locale).unwrap(),
        }
    }

    pub fn path(&self) -> &Path {
        self.directory.path()
    }

    /// The repository, opened without the global and system configuration.
    pub fn repository(&self) -> gix::Repository {
        gix::open_opts(self.path(), gix::open::Options::isolated()).unwrap()
    }

    /// `git $args`, not run yet.
    fn command(&self, args: &[&str]) -> Command {
        let mut command = isolated_command("git", self.path());
        command
            .args(args)
            .env("GIT_AUTHOR_NAME", "Author")
            .env("GIT_AUTHOR_EMAIL", "author@example.com")
            .env("GIT_COMMITTER_NAME", "Committer")
            .env("GIT_COMMITTER_EMAIL", "committer@example.com")
            .env("TZ", self.now.time_zone().iana_name().unwrap())
            .env(
                "GIT_TEST_DATE_NOW",
                self.now.timestamp().as_second().to_string(),
            )
            .env("LC_ALL", &self.locale);
        command
    }

    /// `git $args`, whatever its exit status.
    pub fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    /// The stdout of `git $args`, which must succeed.
    pub fn stdout_of(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    /// The hashes of the commits `git log --reverse $range` displays.
    pub fn log_reverse(&self, range: &str) -> Vec<String> {
        self.stdout_of(&["log", "--reverse", "--format=%H", range])
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// The hash of the commit `revision` names.
    pub fn hash_of(&self, revision: &str) -> String {
        self.stdout_of(&["rev-parse", revision])
            .trim_end()
            .to_string()
    }

    /// `git config $key $value`.
    pub fn set_config(&self, key: &str, value: &str) {
        assert!(self.run(&["config", key, value]).status.success());
    }

    /// Write `content` in the file at `path` (relative to the working tree), creating its parent
    /// directories.
    pub fn write(&self, path: &str, content: &[u8]) {
        let path = self.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    /// Remove the file at `path` (relative to the working tree).
    pub fn remove(&self, path: &str) {
        fs::remove_file(self.path().join(path)).unwrap();
    }

    /// `git add $path`.
    pub fn stage(&self, path: &str) {
        self.stdout_of(&["add", "--", path]);
    }

    /// An empty commit in the submodule at `path` (relative to the working tree), created if it
    /// doesn't exist (it is a submodule once committed in this repository).
    pub fn commit_in_submodule(&self, path: &str) {
        self.stdout_of(&["init", "--quiet", path]);
        self.stdout_of(&[
            "-C",
            path,
            "commit",
            "--quiet",
            "--allow-empty",
            "--no-gpg-sign",
            "--message=In the submodule",
        ]);
    }

    /// `git switch --create $branch $start` (from HEAD if `start` is `None`).
    pub fn switch_to_new_branch(&self, branch: &str, start: Option<&str>) {
        let args = ["switch", "--quiet", "--create", branch]
            .into_iter()
            .chain(start);
        self.stdout_of(&args.collect::<Vec<_>>());
    }

    /// `git switch --detach`.
    pub fn detach_head(&self) {
        self.stdout_of(&["switch", "--quiet", "--detach"]);
    }

    /// `git switch $branch`.
    pub fn switch_to(&self, branch: &str) {
        self.stdout_of(&["switch", "--quiet", branch]);
    }

    /// `git commit --date=$date`, whose stderr is the error if git refuses `date`.
    fn commit_with_date(&self, date: &str) -> Result<(), String> {
        let output = self.run(&[
            "commit",
            "--quiet",
            "--allow-empty",
            "--no-gpg-sign",
            "--message=fake",
            &format!("--date={date}"),
        ]);
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8(output.stderr).unwrap())
        }
    }

    /// A commit authored at `time`.
    pub fn commit_at(&self, time: Time) {
        self.commit_with_date(&time.format(format::RAW).unwrap())
            .unwrap();
    }

    /// A commit of all the files of the working tree (possibly none changed), whose message is
    /// `message`, and its hash.
    ///
    /// It is authored and committed one second after the previous commit made by this method, so
    /// that `git log` orders the commits by creation.
    pub fn commit_all(&self, message: &[u8]) -> String {
        self.stdout_of(&["add", "--all"]);
        self.write(COMMIT_MESSAGE_FILE, message);
        let count = self.commit_all_count.fetch_add(1, Ordering::Relaxed) + 1;
        let date = format!("@{} +0000", self.now.timestamp().as_second() + count);
        let output = self
            .command(&[
                "commit",
                "--quiet",
                "--allow-empty",
                "--no-gpg-sign",
                "--cleanup=verbatim",
                &format!("--file={COMMIT_MESSAGE_FILE}"),
            ])
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        self.hash_of("HEAD")
    }

    /// A commit with the files of HEAD, on top of it, whose message is `message` as is, and its
    /// hash: unlike `git commit`, which converts a message that is not valid UTF-8 from Latin-1.
    pub fn commit_with_raw_message(&self, message: &[u8]) -> String {
        let tree = self.hash_of("HEAD^{tree}");
        let parent = self.hash_of("HEAD");
        let signature = "Author <author@example.com> 0 +0000";
        let header =
            format!("tree {tree}\nparent {parent}\nauthor {signature}\ncommitter {signature}\n\n");
        self.write(RAW_COMMIT_FILE, &[header.as_bytes(), message].concat());
        let commit = self.stdout_of(&["hash-object", "-t", "commit", "-w", RAW_COMMIT_FILE]);
        let commit = commit.trim_end();
        self.stdout_of(&["update-ref", "HEAD", commit]);
        commit.to_string()
    }

    /// The remote branch `$remote/$branch` at `commit`, as if it had been fetched.
    pub fn set_remote_branch(&self, remote: &str, branch: &str, commit: &str) {
        self.set_config(
            &format!("remote.{remote}.url"),
            "https://example.com/repository",
        );
        self.set_config(
            &format!("remote.{remote}.fetch"),
            &format!("+refs/heads/*:refs/remotes/{remote}/*"),
        );
        self.stdout_of(&[
            "update-ref",
            &format!("refs/remotes/{remote}/{branch}"),
            commit,
        ]);
    }

    /// `$remote/$remote_branch` is the upstream of `branch`.
    pub fn set_upstream(&self, branch: &str, remote: &str, remote_branch: &str) {
        self.set_config(&format!("branch.{branch}.remote"), remote);
        self.set_config(
            &format!("branch.{branch}.merge"),
            &format!("refs/heads/{remote_branch}"),
        );
    }

    /// `origin/topic`, the upstream of `BRANCH`, at HEAD.
    pub fn set_upstream_at_head(&self) {
        self.set_remote_branch(ORIGIN, BRANCH, &self.hash_of("HEAD"));
        self.set_upstream(BRANCH, ORIGIN, BRANCH);
    }

    /// `refs/remotes/$remote/HEAD`, pointing to `$remote/main`, at HEAD.
    pub fn set_remote_head_at_head(&self, remote: &str) {
        self.set_remote_branch(remote, DEFAULT_BRANCH, &self.hash_of("HEAD"));
        self.stdout_of(&[
            "symbolic-ref",
            &format!("refs/remotes/{remote}/HEAD"),
            &format!("refs/remotes/{remote}/{DEFAULT_BRANCH}"),
        ]);
    }

    /// A rebase of the current branch, stopped by a conflict after replaying
    /// `replayed_commits_count` commits, and the hash of the commit they are replayed onto.
    ///
    /// The current branch gets a commit creating a file, then `replayed_commits_count` commits,
    /// then a commit modifying the file. The commit they are replayed onto modifies the file
    /// differently, on top of the one creating it.
    pub fn start_rebase_stopped_by_conflict(
        &self,
        backend: RebaseBackend,
        replayed_commits_count: usize,
    ) -> String {
        self.write(CONFLICTING_FILE, b"before the rebase\n");
        let base = self.commit_all(b"Create the conflicting file");
        let branch = self.stdout_of(&["branch", "--show-current"]);
        let branch = branch.trim_end();
        self.switch_to_new_branch("onto", Some(&base));
        self.write(CONFLICTING_FILE, b"onto\n");
        let onto = self.commit_all(b"Modify the conflicting file onto");
        self.switch_to(branch);
        for commit_index in 0..replayed_commits_count {
            self.write(&format!("replayed_{commit_index}.txt"), b"replayed\n");
            self.commit_all(format!("Replayed commit {commit_index}").as_bytes());
        }
        self.write(CONFLICTING_FILE, b"rebased\n");
        self.commit_all(b"Modify the conflicting file");
        let backend = match backend {
            RebaseBackend::Merge => "--merge",
            RebaseBackend::Apply => "--apply",
        };
        assert!(
            !self
                .run(&["rebase", "--quiet", backend, &onto])
                .status
                .success()
        );
        onto
    }

    /// The author date of the last commit, displayed by `git log` with `--date=$format` (or with
    /// its default if `format` is `None`), whose stderr is the error if git refuses `format`.
    pub fn author_date(&self, format: Option<&str>) -> Result<String, String> {
        let date = format.map(|format| format!("--date={format}"));
        let args = ["log", "-1", "--format=%ad"]
            .into_iter()
            .chain(date.as_deref());
        let output = self.run(&args.collect::<Vec<_>>());
        if output.status.success() {
            let stdout = String::from_utf8(output.stdout).unwrap();
            Ok(stdout.strip_suffix('\n').unwrap().to_string())
        } else {
            Err(String::from_utf8(output.stderr).unwrap())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persistent_repository_is_kept_after_drop() {
        let parent = tempfile::tempdir().unwrap();
        let path = parent.path().join("repository");
        let git = Git::on_branch_in(Directory::Persistent(path.clone()));
        let commit = git.commit_all(b"Kept");
        drop(git);
        let repository = gix::open_opts(&path, gix::open::Options::isolated()).unwrap();
        assert_eq!(repository.head_id().unwrap().to_string(), commit);
    }

    #[test]
    fn temporary_repository_is_removed_on_drop() {
        let git = Git::on_branch();
        let path = git.path().to_path_buf();
        drop(git);
        assert!(!path.exists());
    }
}
