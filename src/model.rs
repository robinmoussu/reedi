//! Data model of the revisions displayed by `reedi`.

use gix::ObjectId;
use gix::bstr::ByteSlice;
use gix::date::Time;
use gix::refs::FullName;
use reediif::model::FileContentType;
use reediif::model::FileDiff;
use reediif::model::LineKind;
use std::fmt::Display;

/// The start of the line that starts a conflict, written by git in the unmerged files.
pub const CONFLICT_MARKER: &str = "<<<<<<<";

/// Something displayed like a commit: a diff and its metadata.
#[derive(Debug, Clone)]
pub enum Revision {
    Commit(Commit),
    Index(Index),
    WorkingDirectory(WorkingDirectory),
}

impl From<Commit> for Revision {
    fn from(commit: Commit) -> Self {
        Revision::Commit(commit)
    }
}

/// A commit and its diff.
#[derive(Debug, Clone)]
pub struct Commit {
    pub hash: ObjectId,
    /// Length of the short form of `hash`: git picks the shortest unambiguous length (at least
    /// `core.abbrev`), which may differ from one commit to another.
    pub hash_abbrev_len: usize,
    pub subject: String,
    /// Author name (without email).
    pub author: String,
    /// Author email.
    pub email: String,
    /// Author date.
    pub date: Time,
    /// What `git log --pretty='%d'` displays (`HEAD`, branches, tags).
    pub decorations: Vec<Decoration>,
    /// Commit message without the subject line.
    pub body: String,
    /// In the order of the commit (a merge commit has several parents).
    pub parent_ids: Vec<ObjectId>,
    pub files: Vec<FileDiff>,
}

impl Commit {
    /// Whether the commit has several parents.
    pub fn is_merge(&self) -> bool {
        self.parent_ids.len() > 1
    }

    /// Returns the short form of `hash`, as displayed.
    pub fn short_hash(&self) -> impl Display + '_ {
        self.hash.to_hex_with_len(self.hash_abbrev_len)
    }
}

/// The changes staged for the next commit (`git diff --cached`), and its message.
#[derive(Debug, Clone)]
pub struct Index {
    /// The branch HEAD is attached to (during a rebase, the rebased branch), `None` if HEAD is
    /// detached.
    pub branch: Option<FullName>,
    /// The subject of the next commit message, empty if there is none.
    pub subject: String,
    /// The next commit message without the subject line.
    pub body: String,
    pub files: Vec<FileDiff>,
}

impl Index {
    /// The branch, as displayed (`main` for `refs/heads/main`).
    pub fn short_branch(&self) -> Option<String> {
        self.branch
            .as_ref()
            .map(|branch| branch.shorten().to_str_lossy().into_owned())
    }

    /// Whether the file at `path` is unmerged.
    pub fn is_unmerged(&self, path: &gix::bstr::BStr) -> bool {
        self.files
            .iter()
            .any(|file| file.path == path && file.content == FileContentType::Unmerged)
    }
}

/// The changes that are not staged (`git diff`).
#[derive(Debug, Clone)]
pub struct WorkingDirectory {
    pub files: Vec<FileDiff>,
}

impl WorkingDirectory {
    /// Counts the conflicts left: the `<<<<<<<` markers added to the files that are unmerged in
    /// `index` (a marker already in HEAD is not a conflict).
    pub fn conflicts_count(&self, index: &Index) -> usize {
        self.files
            .iter()
            .filter(|file| index.is_unmerged(file.path.as_bstr()))
            .flat_map(|file| &file.hunks)
            .flat_map(|hunk| &hunk.lines)
            .filter(|line| line.kind == LineKind::Added && line.text.starts_with(CONFLICT_MARKER))
            .count()
    }
}

/// A ref pointing to a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decoration {
    /// `HEAD`, with the local branch it is attached to, if any (`HEAD -> main`).
    Head(Option<String>),
    LocalBranch(String),
    RemoteBranch(String),
    Tag(String),
}

#[cfg(test)]
mod tests {
    use crate::fake;
    use gix::ObjectId;
    use reediif::diff::unmerged;
    use reediif::model::LineKind;
    use reediif::test_support::line;
    use reediif::test_support::modified;

    use super::*;

    const HASH: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn short_hash_has_the_abbreviation_length_of_its_commit() {
        let mut commit = fake::scrolloff_rename();
        commit.hash = ObjectId::from_hex(HASH.as_bytes()).unwrap();
        for len in [7, 10, 40] {
            commit.hash_abbrev_len = len;
            assert_eq!(commit.short_hash().to_string(), &HASH[..len]);
        }
    }

    /// The start of a conflict, as written by git.
    const MARKER: &str = "<<<<<<< HEAD";

    /// The index where the files at `unmerged_paths` are unmerged.
    fn index_with_unmerged(unmerged_paths: &[&str]) -> Index {
        Index {
            branch: None,
            subject: String::new(),
            body: String::new(),
            files: unmerged_paths.iter().copied().map(unmerged).collect(),
        }
    }

    #[test]
    fn conflicts_are_the_markers_added_to_the_unmerged_files() {
        let working_directory = WorkingDirectory {
            files: vec![
                modified(
                    "a.txt",
                    vec![
                        line(LineKind::Added, MARKER),
                        line(LineKind::Unchanged, "unchanged"),
                        line(LineKind::Added, MARKER),
                    ],
                ),
                modified("b.txt", vec![line(LineKind::Added, MARKER)]),
            ],
        };
        let index = index_with_unmerged(&["a.txt", "b.txt"]);
        assert_eq!(working_directory.conflicts_count(&index), 3);
    }

    #[test]
    fn markers_already_in_head_are_not_conflicts() {
        let working_directory = WorkingDirectory {
            files: vec![modified(
                "a.txt",
                vec![
                    line(LineKind::Unchanged, MARKER),
                    line(LineKind::Removed, MARKER),
                    line(LineKind::Added, MARKER),
                ],
            )],
        };
        let index = index_with_unmerged(&["a.txt"]);
        assert_eq!(working_directory.conflicts_count(&index), 1);
    }

    #[test]
    fn markers_of_files_that_are_not_unmerged_are_not_conflicts() {
        let working_directory = WorkingDirectory {
            files: vec![modified("a.txt", vec![line(LineKind::Added, MARKER)])],
        };
        let index = index_with_unmerged(&["b.txt"]);
        assert_eq!(working_directory.conflicts_count(&index), 0);
    }
}
