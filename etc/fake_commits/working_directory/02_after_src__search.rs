use std::ops::Range;

use crate::case::Case;
use crate::highlight;

/// An incremental search in the diffs.
pub struct Search {
    pub query: String,
    pub case: Case,
}

impl Search {
    pub fn new(query: &str) -> Self {
        let case = Case::detect(query);
        Self {
            query: query.to_string(),
            case,
        }
    }

    /// Whether `line` matches the search.
    pub fn matches(&self, line: &str) -> bool {
        line.contains(&self.query)
    }

    /// The byte ranges of the matches in `line`, to highlight them. A search ignoring the case
    /// highlights the matches of any case.
    pub fn highlighted(&self, line: &str) -> Vec<Range<usize>> {
        match self.case {
            Case::Sensitive => highlight::matches(line, &self.query),
            Case::Insensitive => {
                highlight::matches(&line.to_lowercase(), &self.query.to_lowercase())
            }
        }
    }
}
