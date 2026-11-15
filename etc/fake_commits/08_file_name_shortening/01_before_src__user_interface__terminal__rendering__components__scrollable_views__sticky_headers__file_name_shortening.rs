//! Shortening of the file names displayed in the sticky header.

use unicode_segmentation::UnicodeSegmentation;

/// Minimal number of graphemes kept from the start of a file name.
const MIN_KEPT: usize = 8;

/// Displayed in place of the removed part of a file name.
const ELLIPSIS: &str = "...";

/// The start of `text`, made of `count` graphemes.
fn start(text: &str, count: usize) -> &str {
    text.grapheme_indices(true)
        .nth(count)
        .map_or(text, |(index, _)| &text[..index])
}

/// `path` with the end of its folders replaced by `ELLIPSIS`.
pub fn shorten_folders(path: &str, kept: usize) -> String {
    match path.rsplit_once('/') {
        Some((folders, name)) => format!("{}{ELLIPSIS}/{name}", start(folders, kept)),
        None => path.to_string(),
    }
}

/// `name` with the end of its stem replaced by `ELLIPSIS`.
pub fn shorten_stem(name: &str, kept: usize) -> String {
    match name.rsplit_once('.') {
        Some((stem, extension)) => format!("{}{ELLIPSIS}.{extension}", start(stem, kept)),
        None => format!("{}{ELLIPSIS}", start(name, kept)),
    }
}

/// `name` cut after `MIN_KEPT` graphemes.
pub fn shorten_name(name: &str) -> String {
    format!("{}{ELLIPSIS}", start(name, MIN_KEPT))
}
