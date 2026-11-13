//! Data model of the diff of a file.

use bstr::BString;

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
    /// At least one version is binary. The file has no hunks.
    Binary,
    /// At least one version is not valid UTF-8. The file has no hunks.
    InvalidUtf8,
    /// The file has conflicts, so it has no version in the index. The file has no hunks.
    Unmerged,
}

/// The modifications of a single file.
#[derive(Debug, Clone)]
pub struct FileDiff {
    /// Path of the file after the commit (before it, if the file is deleted). Like in git, it may
    /// not be valid UTF-8.
    pub path: BString,
    pub change: FileChange,
    pub content: FileContentType,
    pub hunks: Vec<Hunk>,
}

impl FileDiff {
    /// Whether a version of the file mixes line endings (with and without a carriage return), or
    /// whether the before and after versions use different line endings.
    ///
    /// Only the lines of the hunks are known, so they are the only ones compared. A line missing
    /// its newline has no line ending, so it is ignored.
    pub fn has_inconsistent_line_endings(&self) -> bool {
        let mut crlf = self
            .hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .filter(|line| !line.missing_newline)
            .map(|line| line.text.ends_with('\r'));
        crlf.next()
            .is_some_and(|first| crlf.any(|crlf| crlf != first))
    }
}

/// Whether a line of a hunk is in the before version of the file, in the after version, or in
/// both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Unchanged,
    Removed,
    Added,
}

/// A line of a hunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    /// The line, without its newline. A carriage return before the newline is kept.
    pub text: String,
    /// Whether the line is the last one of its version of the file, and the file doesn't end with
    /// a newline.
    pub missing_newline: bool,
}

/// The line numbers of a line of a hunk, in the versions of the file it is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineNumbers {
    pub before: Option<usize>,
    pub after: Option<usize>,
}

/// A contiguous range of lines, with the modified lines and the unchanged lines around them (like
/// the hunks of a unified diff).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    /// Line number (starting at 1) of the first line of the hunk in the before version.
    pub before_first_line: usize,
    /// Line number (starting at 1) of the first line of the hunk in the after version.
    pub after_first_line: usize,
    pub lines: Vec<DiffLine>,
}

impl Hunk {
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

    /// Whether the `line_index`-th line is the last line of a version of the file that doesn't end
    /// with a newline, while the other version ends with one.
    ///
    /// If a version of the file doesn't end with a newline, but not the other one, their last
    /// lines are different, so they are both in the last hunk.
    pub fn has_newline_mismatch(&self, line_index: usize) -> bool {
        let line = &self.lines[line_index];
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
    use crate::test_support::line;
    use crate::test_support::unchanged;

    /// A hunk replacing `before` by `after`.
    fn hunk(before: &[&str], after: &[&str]) -> Hunk {
        let removed = before.iter().map(|text| line(LineKind::Removed, text));
        let added = after.iter().map(|text| line(LineKind::Added, text));
        Hunk {
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
            hunks: vec![hunk(before, after)],
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
        file.hunks[0].lines[1].missing_newline = true;
        assert!(!file.has_inconsistent_line_endings());
    }

    #[test]
    fn line_numbers_skip_the_lines_missing_in_a_version() {
        let mut hunk = hunk(&["a", "b"], &["c"]);
        hunk.lines
            .insert(/* index */ 0, line(LineKind::Unchanged, "z"));
        hunk.after_first_line = 10;
        let numbers: Vec<_> = hunk
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
    fn only_one_version_missing_its_newline_is_a_newline_mismatch() {
        let mut hunk = hunk(&["a"], &["a"]);
        hunk.lines[0].missing_newline = true;
        assert!(hunk.has_newline_mismatch(/* line_index */ 0));
        assert!(!hunk.has_newline_mismatch(/* line_index */ 1));
        hunk.lines[1].missing_newline = true;
        assert!(!hunk.has_newline_mismatch(/* line_index */ 0));
        assert!(!hunk.has_newline_mismatch(/* line_index */ 1));
    }

    #[test]
    fn unchanged_line_has_no_newline_mismatch() {
        let mut hunk = unchanged(/* first_line */ 1, /* len */ 1);
        hunk.lines[0].missing_newline = true;
        assert!(!hunk.has_newline_mismatch(/* line_index */ 0));
    }
}
