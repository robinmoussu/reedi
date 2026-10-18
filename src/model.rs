//! Data model of the commits displayed by `reedi`.

use jiff::Zoned;

/// A commit and its diff.
#[derive(Debug, Clone)]
pub struct Commit {
    /// Short form of the commit hash.
    pub hash: String,
    pub subject: String,
    /// Author name (without email).
    pub author: String,
    /// Author date.
    pub date: Zoned,
    /// What `git log --pretty='%d'` displays (`HEAD`, branches, tags).
    pub decorations: Vec<Decoration>,
    /// Commit message without the subject line.
    pub body: String,
    pub files: Vec<FileDiff>,
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

/// The modifications of a single file.
#[derive(Debug, Clone)]
pub struct FileDiff {
    pub path: String,
    pub chunks: Vec<Chunk>,
}

impl FileDiff {
    /// Whether a version of the file mixes line endings (with and without a carriage return), or
    /// whether the before and after versions use different line endings.
    ///
    /// Only the lines of the chunks are known, so they are the only ones compared.
    pub fn has_inconsistent_line_endings(&self) -> bool {
        let mut crlf = self
            .chunks
            .iter()
            .flat_map(|chunk| chunk.before.lines.iter().chain(&chunk.after.lines))
            .map(|line| line.ends_with('\r'));
        crlf.next()
            .is_some_and(|first| crlf.any(|crlf| crlf != first))
    }
}

/// A contiguous range of modified lines.
///
/// The `n`-th line of `before` is displayed next to the `n`-th line of `after`.
#[derive(Debug, Clone)]
pub struct Chunk {
    pub before: Side,
    pub after: Side,
}

/// The lines of a chunk, either before or after the commit.
#[derive(Debug, Clone)]
pub struct Side {
    /// Line number (starting at 1) of the first element of `lines` in the file.
    pub first_line: usize,
    pub lines: Vec<String>,
}

impl Side {
    /// Line number of the `index`-th element of `lines`.
    pub fn line_number(&self, index: usize) -> usize {
        self.first_line + index
    }

    fn last_line_number(&self) -> usize {
        self.line_number(self.lines.len().saturating_sub(1))
    }
}

impl Chunk {
    /// Number of digits needed to display all the line numbers of this chunk.
    pub fn line_number_width(&self) -> usize {
        let largest = self
            .before
            .last_line_number()
            .max(self.after.last_line_number());
        largest.to_string().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn side(first_line: usize, len: usize) -> Side {
        Side {
            first_line,
            lines: vec![String::new(); len],
        }
    }

    fn file(before: &[&str], after: &[&str]) -> FileDiff {
        let side = |lines: &[&str]| Side {
            first_line: 1,
            lines: lines.iter().map(|line| line.to_string()).collect(),
        };
        FileDiff {
            path: "a".to_string(),
            chunks: vec![Chunk {
                before: side(before),
                after: side(after),
            }],
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
    fn line_number_width_fits_last_line() {
        let chunk = Chunk {
            before: side(5, 5),
            after: side(5, 5),
        };
        assert_eq!(chunk.line_number_width(), 1);

        let chunk = Chunk {
            before: side(5, 6),
            after: side(5, 6),
        };
        assert_eq!(chunk.line_number_width(), 2);
    }

    #[test]
    fn line_number_width_uses_the_largest_side() {
        let chunk = Chunk {
            before: side(95, 3),
            after: side(98, 3),
        };
        assert_eq!(chunk.line_number_width(), 3);

        let chunk = Chunk {
            before: side(98, 3),
            after: side(95, 3),
        };
        assert_eq!(chunk.line_number_width(), 3);
    }
}
