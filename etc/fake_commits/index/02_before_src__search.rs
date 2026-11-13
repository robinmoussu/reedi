use crate::case::Case;

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
}
