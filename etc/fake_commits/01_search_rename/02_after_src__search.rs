// Line 1.
// Line 2.
// Line 3.
// Line 4.
// Line 5.
// Line 6.
// Line 7.
// Line 8.
// Line 9.
// Line 10.

/// An incremental search in the diffs.
pub struct Search {
    pub query: String,
    pub case: Case,
}

// Line 18.
// Line 19.
// Line 20.
// Line 21.
// Line 22.
// Line 23.
// Line 24.
// Line 25.
// Line 26.
// Line 27.
// Line 28.
}

impl Search {
    pub fn new(query: &str) -> Self {
        let case = Case::detect(query);
        Self {
            query: query.to_string(),
            case,
        }
    }
// Line 39.
// Line 40.
// Line 41.
// Line 42.
// Line 43.
// Line 44.
// Line 45.
// Line 46.
// Line 47.
// Line 48.
// Line 49.
// Line 50.
// Line 51.
// Line 52.
// Line 53.
// Line 54.
// Line 55.

    /// Whether `line` matches the search.
    fn matches(&self, line: &str) -> bool {
        line.contains(&self.query)
    }
}
