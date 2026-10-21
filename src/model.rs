//! Data model of the commits displayed by `reedi`.

use gix::bstr::BString;
use gix::date::Time;

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

/// How a file is modified by a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileChange {
    Added,
    Deleted,
    Modified,
    /// Moved from `from` (and possibly modified).
    Renamed {
        from: BString,
    },
}

/// What the versions of a file contain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileContentType {
    Text,
    /// At least one version is binary. The file has no chunks.
    Binary,
    /// At least one version is not valid UTF-8. The file has no chunks.
    InvalidUtf8,
}

/// The modifications of a single file.
#[derive(Debug, Clone)]
pub struct FileDiff {
    /// Path of the file after the commit (before it, if the file is deleted). Like in git, it may
    /// not be valid UTF-8.
    pub path: BString,
    pub change: FileChange,
    pub content: FileContentType,
    pub chunks: Vec<Chunk>,
}

impl FileDiff {
    /// Whether a version of the file mixes line endings (with and without a carriage return), or
    /// whether the before and after versions use different line endings.
    ///
    /// Only the lines of the chunks are known, so they are the only ones compared. A line missing
    /// its newline has no line ending, so it is ignored.
    pub fn has_inconsistent_line_endings(&self) -> bool {
        let mut crlf = self
            .chunks
            .iter()
            .flat_map(|chunk| &chunk.lines)
            .filter(|line| !line.missing_newline)
            .map(|line| line.text.ends_with('\r'));
        crlf.next()
            .is_some_and(|first| crlf.any(|crlf| crlf != first))
    }
}

/// Whether a line of a chunk is in the before version of the file, in the after version, or in
/// both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Unchanged,
    Removed,
    Added,
}

/// A line of a chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    /// The line, without its newline. A carriage return before the newline is kept.
    pub text: String,
    /// Whether the line is the last one of its version of the file, and the file doesn't end with
    /// a newline.
    pub missing_newline: bool,
}

/// The line numbers of a line of a chunk, in the versions of the file it is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineNumbers {
    pub before: Option<usize>,
    pub after: Option<usize>,
}

/// A contiguous range of lines, with the modified lines and the unchanged lines around them (like
/// the hunks of a unified diff).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// Line number (starting at 1) of the first line of the chunk in the before version.
    pub before_first_line: usize,
    /// Line number (starting at 1) of the first line of the chunk in the after version.
    pub after_first_line: usize,
    pub lines: Vec<DiffLine>,
}

impl Chunk {
    /// The line numbers of each line of `lines`.
    pub fn line_numbers(&self) -> Vec<LineNumbers> {
        self.lines
            .iter()
            .scan(
                (self.before_first_line, self.after_first_line),
                |(before, after), line| {
                    let number = |next: &mut usize, present: bool| {
                        if present {
                            *next += 1;
                            Some(*next - 1)
                        } else {
                            None
                        }
                    };
                    Some(LineNumbers {
                        before: number(before, line.kind != LineKind::Added),
                        after: number(after, line.kind != LineKind::Removed),
                    })
                },
            )
            .collect()
    }

    /// Number of digits needed to display all the line numbers of this chunk.
    pub fn line_number_width(&self) -> usize {
        let largest = self
            .line_numbers()
            .into_iter()
            .flat_map(|numbers| numbers.before.into_iter().chain(numbers.after))
            .max()
            .unwrap_or(0);
        largest.to_string().len()
    }

    /// Whether `\ No newline at end of file` is displayed because of the `index`-th line: only one
    /// of the versions of the file doesn't end with a newline, and the line is the last one of this
    /// version.
    ///
    /// If a version of the file doesn't end with a newline, but not the other one, their last
    /// lines are different, so they are both in the last chunk.
    pub fn shows_missing_newline(&self, index: usize) -> bool {
        let line = &self.lines[index];
        let other = match line.kind {
            LineKind::Unchanged => return false,
            LineKind::Removed => LineKind::Added,
            LineKind::Added => LineKind::Removed,
        };
        line.missing_newline
            && !self
                .lines
                .iter()
                .any(|line| line.kind == other && line.missing_newline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeCommit;

    #[test]
    fn short_hash_has_the_abbreviation_length_of_its_commit() {
        let mut commit = FakeCommit::newest();
        commit.hash = "0123456789abcdef0123456789abcdef01234567".to_string();
        [7, 10, 40].into_iter().for_each(|len| {
            commit.hash_abbrev_len = len;
            assert_eq!(commit.short_hash(), &commit.hash[..len]);
        });
    }

    fn line(kind: LineKind, text: &str) -> DiffLine {
        DiffLine {
            kind,
            text: text.to_string(),
            missing_newline: false,
        }
    }

    /// A chunk replacing `before` by `after`.
    fn chunk(before: &[&str], after: &[&str]) -> Chunk {
        let removed = before.iter().map(|text| line(LineKind::Removed, text));
        let added = after.iter().map(|text| line(LineKind::Added, text));
        Chunk {
            before_first_line: 1,
            after_first_line: 1,
            lines: removed.chain(added).collect(),
        }
    }

    fn file(before: &[&str], after: &[&str]) -> FileDiff {
        FileDiff {
            path: "a".into(),
            change: FileChange::Modified,
            content: FileContentType::Text,
            chunks: vec![chunk(before, after)],
        }
    }

    #[test]
    fn same_line_endings_are_consistent() {
        assert!(!file(&["a", "b"], &["a", "c"]).has_inconsistent_line_endings());
        assert!(!file(&["a\r", "b\r"], &["a\r", "c\r"]).has_inconsistent_line_endings());
        assert!(!file(&[], &[]).has_inconsistent_line_endings());
    }

    #[test]
    fn mixed_line_endings_are_inconsistent() {
        assert!(file(&["a\r", "b"], &["a\r", "b\r"]).has_inconsistent_line_endings());
        assert!(file(&["a", "b"], &["a", "b\r"]).has_inconsistent_line_endings());
    }

    #[test]
    fn different_line_endings_between_versions_are_inconsistent() {
        assert!(file(&["a", "b"], &["a\r", "b\r"]).has_inconsistent_line_endings());
    }

    #[test]
    fn line_missing_its_newline_has_no_line_ending() {
        let mut file = file(&["a\r", "b"], &["a\r", "b\r"]);
        file.chunks[0].lines[1].missing_newline = true;
        assert!(!file.has_inconsistent_line_endings());
    }

    /// A chunk of `len` unchanged lines, starting at `first_line` in both versions.
    fn unchanged(first_line: usize, len: usize) -> Chunk {
        Chunk {
            before_first_line: first_line,
            after_first_line: first_line,
            lines: vec![line(LineKind::Unchanged, ""); len],
        }
    }

    #[test]
    fn line_numbers_skip_the_lines_missing_in_a_version() {
        let mut chunk = chunk(&["a", "b"], &["c"]);
        chunk.lines.insert(0, line(LineKind::Unchanged, "z"));
        chunk.after_first_line = 10;
        let numbers: Vec<_> = chunk
            .line_numbers()
            .into_iter()
            .map(|numbers| (numbers.before, numbers.after))
            .collect();
        assert_eq!(
            numbers,
            [
                (Some(1), Some(10)),
                (Some(2), None),
                (Some(3), None),
                (None, Some(11))
            ]
        );
    }

    #[test]
    fn line_number_width_fits_last_line() {
        assert_eq!(unchanged(5, 5).line_number_width(), 1);
        assert_eq!(unchanged(5, 6).line_number_width(), 2);
    }

    #[test]
    fn line_number_width_uses_the_largest_version() {
        let mut chunk = unchanged(95, 3);
        chunk.after_first_line = 98;
        assert_eq!(chunk.line_number_width(), 3);
        let mut chunk = unchanged(98, 3);
        chunk.after_first_line = 95;
        assert_eq!(chunk.line_number_width(), 3);
    }

    #[test]
    fn missing_newline_is_shown_if_only_one_version_misses_it() {
        let mut chunk = chunk(&["a"], &["a"]);
        chunk.lines[0].missing_newline = true;
        assert!(chunk.shows_missing_newline(0));
        assert!(!chunk.shows_missing_newline(1));
        chunk.lines[1].missing_newline = true;
        assert!(!chunk.shows_missing_newline(0));
        assert!(!chunk.shows_missing_newline(1));
    }

    #[test]
    fn missing_newline_of_an_unchanged_line_is_not_shown() {
        let mut chunk = unchanged(1, 1);
        chunk.lines[0].missing_newline = true;
        assert!(!chunk.shows_missing_newline(0));
    }
}
