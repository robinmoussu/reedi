//! Data model of the revisions displayed by `reedi`.

use gix::ObjectId;
use gix::date::Time;
use reediif::model::FileDiff;
use std::fmt::Display;

/// Something displayed like a commit: a diff and its metadata.
#[derive(Debug, Clone)]
pub enum Revision {
    Commit(Commit),
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
    pub files: Vec<FileDiff>,
}

impl Commit {
    /// Returns the short form of `hash`, as displayed.
    pub fn short_hash(&self) -> impl Display + '_ {
        self.hash.to_hex_with_len(self.hash_abbrev_len)
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
}
