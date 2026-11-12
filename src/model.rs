//! Data model of the commits displayed by `reedi`.

use gix::date::Time;
use reediif::model::FileDiff;

/// A commit and its diff.
#[derive(Debug, Clone)]
pub struct Commit {
    /// Full commit hash.
    pub hash: String,
    /// Number of characters of the short form of `hash`: git picks the shortest unambiguous length
    /// (at least `core.abbrev`), which may differ from one commit to another.
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
    /// The short form of `hash`, as displayed: its first `hash_abbrev_len` characters.
    pub fn short_hash(&self) -> &str {
        &self.hash[..self.hash_abbrev_len]
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

    #[test]
    fn short_hash_has_the_abbreviation_length_of_its_commit() {
        let mut commit = fake::scrolloff_rename();
        commit.hash = "0123456789abcdef0123456789abcdef01234567".to_string();
        for len in [7, 10, 40] {
            commit.hash_abbrev_len = len;
            assert_eq!(commit.short_hash(), &commit.hash[..len]);
        }
    }
}
