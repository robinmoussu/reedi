//! The highlighting of the matches of a search.

use std::ops::Range;

/// The byte ranges of the matches of `query` in `line`.
pub fn matches(line: &str, query: &str) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    line.match_indices(query)
        .map(|(start, text)| start..start + text.len())
        .collect()
}
