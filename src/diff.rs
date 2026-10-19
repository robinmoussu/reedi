//! Computation of the chunks of a file, from its before and after versions.

use std::ops::Range;
use std::str;

use gix::bstr::BStr;
use gix::diff::blob::Algorithm;
use gix::diff::blob::Diff;
use gix::diff::blob::InternedInput;
use gix::diff::blob::Token;
use gix::diff::blob::sources;

use crate::model::Chunk;
use crate::model::DiffLine;
use crate::model::FileChange;
use crate::model::FileContentType;
use crate::model::FileDiff;
use crate::model::LineKind;

/// Number of unchanged lines displayed before and after the modified lines.
pub const CONTEXT: usize = 3;

/// The path and content of a file, before or after a commit.
#[derive(Debug, Clone, Copy)]
pub struct FileState<'a> {
    /// Like in git, it may not be valid UTF-8.
    pub path: &'a BStr,
    pub content: &'a [u8],
}

/// The modifications of a file from `before` to `after` (`None` if the file doesn't exist in that
/// version), with `context` unchanged lines around the modified lines.
///
/// Binary files, and files with a version that is not valid UTF-8, have no chunks.
///
/// # Panics
///
/// If the file exists in none of the versions.
pub fn file(
    before: Option<FileState>,
    after: Option<FileState>,
    binary: bool,
    context: usize,
) -> FileDiff {
    let (path, change) = match (before, after) {
        (None, Some(after)) => (after.path, FileChange::Added),
        (Some(before), None) => (before.path, FileChange::Deleted),
        (Some(before), Some(after)) if before.path == after.path => {
            (after.path, FileChange::Modified)
        }
        (Some(before), Some(after)) => (
            after.path,
            FileChange::Renamed {
                from: before.path.to_owned(),
            },
        ),
        (None, None) => panic!("the file exists in at least one version"),
    };
    let before = before.map(|state| state.content).unwrap_or_default();
    let after = after.map(|state| state.content).unwrap_or_default();
    let texts = str::from_utf8(before).ok().zip(str::from_utf8(after).ok());
    let (content, chunks) = match texts {
        _ if binary => (FileContentType::Binary, Vec::new()),
        Some((before, after)) => (FileContentType::Text, chunks(before, after, context)),
        None => (FileContentType::InvalidUtf8, Vec::new()),
    };
    FileDiff {
        path: path.to_owned(),
        change,
        content,
        chunks,
    }
}

/// The chunks modifying `before` into `after`, with `context` unchanged lines around the modified
/// lines.
///
/// The lines are compared like git does by default: with the Myers algorithm, and the indent
/// heuristic to choose between equivalent diffs. Like git, the modifications separated by at most
/// `2 * context` unchanged lines are in the same chunk.
fn chunks(before: &str, after: &str, context: usize) -> Vec<Chunk> {
    let input = InternedInput::new(sources::lines(before), sources::lines(after));
    let mut diff = Diff::compute(Algorithm::Myers, &input);
    diff.postprocess_lines(&input);
    let line = |token: &Token, kind| {
        let text = input.interner[*token];
        let (text, missing_newline) = match text.strip_suffix('\n') {
            Some(text) => (text, false),
            None => (text, true),
        };
        DiffLine {
            kind,
            text: text.to_string(),
            missing_newline,
        }
    };
    // The lines of `range` in the version of the file of `kind`.
    let lines = |range: Range<u32>, kind| {
        let tokens = if kind == LineKind::Added {
            &input.after
        } else {
            &input.before
        };
        tokens[range.start as usize..range.end as usize]
            .iter()
            .map(move |token| line(token, kind))
    };
    let unchanged = |range: Range<u32>| lines(range, LineKind::Unchanged);
    let hunks: Vec<_> = diff.hunks().collect();
    let before_len = input.before.len() as u32;
    let context = context as u32;
    hunks
        .chunk_by(|previous, next| next.before.start - previous.before.end <= 2 * context)
        .map(|hunks| {
            let (first, last) = (&hunks[0], &hunks[hunks.len() - 1]);
            // The unchanged lines before a hunk are the same in both versions.
            let leading = first.before.start.min(context);
            let trailing = (before_len - last.before.end).min(context);
            let mut position = first.before.start - leading;
            let mut chunk_lines: Vec<_> = hunks
                .iter()
                .flat_map(|hunk| {
                    let unchanged = unchanged(position..hunk.before.start);
                    position = hunk.before.end;
                    unchanged
                        .chain(lines(hunk.before.clone(), LineKind::Removed))
                        .chain(lines(hunk.after.clone(), LineKind::Added))
                })
                .collect();
            chunk_lines.extend(unchanged(position..last.before.end + trailing));
            Chunk {
                before_first_line: (first.before.start - leading) as usize + 1,
                after_first_line: (first.after.start - leading) as usize + 1,
                lines: chunk_lines,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LineKind::Added;
    use crate::model::LineKind::Removed;
    use crate::model::LineKind::Unchanged;

    fn state<'a>(path: &'a str, content: &'a str) -> Option<FileState<'a>> {
        Some(FileState {
            path: path.into(),
            content: content.as_bytes(),
        })
    }

    fn modified(before: &str, after: &str) -> FileDiff {
        file(state("a", before), state("a", after), false, CONTEXT)
    }

    /// The lines `first..=last`, each one ending with a newline.
    fn numbered(first: usize, last: usize) -> String {
        (first..=last).map(|n| format!("{n}\n")).collect()
    }

    /// The kind and text of the lines of `chunk`.
    fn lines(chunk: &Chunk) -> Vec<(LineKind, &str)> {
        chunk
            .lines
            .iter()
            .map(|line| (line.kind, line.text.as_str()))
            .collect()
    }

    #[test]
    fn identical_versions_have_no_chunks() {
        let diff = modified("a\nb\n", "a\nb\n");
        assert_eq!(diff.change, FileChange::Modified);
        assert_eq!(diff.content, FileContentType::Text);
        assert!(diff.chunks.is_empty());
    }

    #[test]
    fn modified_lines_are_surrounded_by_three_unchanged_lines() {
        let before = numbered(1, 20);
        let after = before.replace("10\n", "ten\n");
        let diff = modified(&before, &after);
        assert_eq!(diff.chunks.len(), 1);
        let chunk = &diff.chunks[0];
        assert_eq!((chunk.before_first_line, chunk.after_first_line), (7, 7));
        assert_eq!(
            lines(chunk),
            [
                (Unchanged, "7"),
                (Unchanged, "8"),
                (Unchanged, "9"),
                (Removed, "10"),
                (Added, "ten"),
                (Unchanged, "11"),
                (Unchanged, "12"),
                (Unchanged, "13"),
            ]
        );
    }

    #[test]
    fn context_is_cropped_at_the_edges_of_the_file() {
        let diff = modified("1\n2\n3\n", "one\n2\nthree\n");
        assert_eq!(diff.chunks.len(), 1);
        assert_eq!(
            lines(&diff.chunks[0]),
            [
                (Removed, "1"),
                (Added, "one"),
                (Unchanged, "2"),
                (Removed, "3"),
                (Added, "three"),
            ]
        );
    }

    #[test]
    fn modifications_separated_by_at_most_six_lines_are_in_the_same_chunk() {
        let before = numbered(1, 30);
        // 6 unchanged lines between them.
        let after = before
            .replace("10\n", "ten\n")
            .replace("17\n", "seventeen\n");
        assert_eq!(modified(&before, &after).chunks.len(), 1);
        // 7 unchanged lines between them.
        let after = before
            .replace("10\n", "ten\n")
            .replace("18\n", "eighteen\n");
        let diff = modified(&before, &after);
        assert_eq!(diff.chunks.len(), 2);
        let second = &diff.chunks[1];
        assert_eq!(second.before_first_line, 15);
        assert_eq!(lines(second)[0], (Unchanged, "15"));
    }

    #[test]
    fn line_numbers_of_each_version_are_counted_separately() {
        let before = numbered(1, 20);
        // The line 2 is removed, and a line is added after the line 15.
        let after = numbered(1, 1) + &numbered(3, 15) + "fifteen\n" + &numbered(16, 20);
        let diff = modified(&before, &after);
        assert_eq!(diff.chunks.len(), 2);
        assert_eq!(
            (
                diff.chunks[1].before_first_line,
                diff.chunks[1].after_first_line
            ),
            (13, 12)
        );
    }

    #[test]
    fn carriage_returns_are_kept() {
        let diff = modified("a\r\nb\r\n", "a\r\nc\r\n");
        assert_eq!(
            lines(&diff.chunks[0]),
            [(Unchanged, "a\r"), (Removed, "b\r"), (Added, "c\r")]
        );
    }

    #[test]
    fn missing_newline_is_a_modification() {
        let diff = modified("a\nb", "a\nb\n");
        let chunk = &diff.chunks[0];
        assert_eq!(
            lines(chunk),
            [(Unchanged, "a"), (Removed, "b"), (Added, "b")]
        );
        let missing: Vec<_> = chunk
            .lines
            .iter()
            .map(|line| line.missing_newline)
            .collect();
        assert_eq!(missing, [false, true, false]);
    }

    #[test]
    fn added_and_deleted_files() {
        let added = file(None, state("a", "x\ny\n"), false, CONTEXT);
        assert_eq!(added.change, FileChange::Added);
        assert_eq!(lines(&added.chunks[0]), [(Added, "x"), (Added, "y")]);
        assert_eq!(added.chunks[0].after_first_line, 1);
        let deleted = file(state("a", "x\n"), None, false, CONTEXT);
        assert_eq!(deleted.path, "a");
        assert_eq!(deleted.change, FileChange::Deleted);
        assert_eq!(lines(&deleted.chunks[0]), [(Removed, "x")]);
    }

    #[test]
    fn renamed_file_is_displayed_with_its_new_path() {
        let diff = file(state("a", "x\n"), state("b", "x\n"), false, CONTEXT);
        assert_eq!(diff.path, "b");
        assert_eq!(diff.change, FileChange::Renamed { from: "a".into() });
        assert!(diff.chunks.is_empty());
    }

    #[test]
    fn binary_files_have_no_chunks() {
        let diff = file(state("a", "x\0"), state("a", "y\0"), true, CONTEXT);
        assert_eq!(diff.content, FileContentType::Binary);
        assert!(diff.chunks.is_empty());
    }

    #[test]
    fn files_that_are_not_valid_utf8_have_no_chunks() {
        let invalid = FileState {
            path: "a".into(),
            content: b"caf\xe9\n",
        };
        let diff = file(state("a", "cafe\n"), Some(invalid), false, CONTEXT);
        assert_eq!(diff.content, FileContentType::InvalidUtf8);
        assert!(diff.chunks.is_empty());
        let diff = file(Some(invalid), None, false, CONTEXT);
        assert_eq!(diff.content, FileContentType::InvalidUtf8);
    }
}
