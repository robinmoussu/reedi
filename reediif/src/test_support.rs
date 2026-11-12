//! Helpers shared by the tests of `reediif` and of `reedi`.

use crate::model::DiffLine;
use crate::model::Hunk;
use crate::model::LineKind;

/// A line of `kind`, followed by a newline.
pub fn line(kind: LineKind, text: &str) -> DiffLine {
    DiffLine {
        kind,
        text: text.to_string(),
        missing_newline: false,
    }
}

/// A hunk of `len` unchanged lines, starting at `first_line` in both versions.
pub fn unchanged(first_line: usize, len: usize) -> Hunk {
    Hunk {
        before_first_line: first_line,
        after_first_line: first_line,
        lines: vec![line(LineKind::Unchanged, ""); len],
    }
}
