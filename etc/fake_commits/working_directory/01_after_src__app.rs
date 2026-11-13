use crate::search::Search;

/// What is displayed, and how.
pub struct App {
    pub search: Option<Search>,
    pub scrolloff: usize,
}

impl App {
    /// Start a search of `query`, from the cursor.
    pub fn search(&mut self, query: &str) {
        self.search = Some(Search::new(query));
<<<<<<< HEAD
        self.jump_to_next_match();
=======
        self.highlight_matches();
>>>>>>> 4b7e2d1 (search: highlight all the matches)
    }

    fn jump_to_next_match(&mut self) {}
}
