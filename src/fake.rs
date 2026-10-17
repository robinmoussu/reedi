//! Realistic fake data, used until real git data is read.

use jiff::Zoned;
use jiff::civil;
use jiff::tz;
use jiff::tz::TimeZone;

use crate::model::Chunk;
use crate::model::Commit;
use crate::model::Decoration;
use crate::model::FileDiff;
use crate::model::Side;

fn lines(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| line.to_string()).collect()
}

/// `year-month-day hour:minute:second`, in UTC+2.
fn date(year: i16, month: i8, day: i8, hour: i8, minute: i8, second: i8) -> Zoned {
    civil::date(year, month, day)
        .at(hour, minute, second, 0)
        .to_zoned(TimeZone::fixed(tz::offset(2)))
        .expect("the fake date is valid")
}

/// Two commits, in topological order (oldest first).
pub fn commits() -> Vec<Commit> {
    vec![search_rename(), scrolloff_rename()]
}

/// A commit modifying one chunk of a file and three chunks of another one.
fn search_rename() -> Commit {
    Commit {
        hash: "8d41e07".to_string(),
        subject: "search: rename pattern to query".to_string(),
        author: "Bruno Lefebvre".to_string(),
        date: date(2026, 10, 9, 9, 15, 42),
        decorations: Vec::new(),
        body: "`query` is the name used in the status bar.".to_string(),
        files: vec![
            FileDiff {
                path: "src/app.rs".to_string(),
                chunks: vec![Chunk {
                    before: Side {
                        first_line: 42,
                        lines: lines(&[
                            "        KeyCode::Enter => {",
                            "            let pattern = &self.search.pattern;",
                            "            self.history.push(pattern.clone());",
                            "            self.mode = Mode::Review;",
                            "        }",
                        ]),
                    },
                    after: Side {
                        first_line: 42,
                        lines: lines(&[
                            "        KeyCode::Enter => {",
                            "            let query = &self.search.query;",
                            "            self.history.push(query.clone());",
                            "            self.mode = Mode::Review;",
                            "        }",
                        ]),
                    },
                }],
            },
            FileDiff {
                path: "src/search.rs".to_string(),
                chunks: vec![
                    Chunk {
                        before: Side {
                            first_line: 12,
                            lines: lines(&[
                                "/// An incremental search in the diffs.",
                                "pub struct Search {",
                                "    pub pattern: String,",
                                "    pub case: Case,",
                                "}",
                            ]),
                        },
                        after: Side {
                            first_line: 12,
                            lines: lines(&[
                                "/// An incremental search in the diffs.",
                                "pub struct Search {",
                                "    pub query: String,",
                                "    pub case: Case,",
                                "}",
                            ]),
                        },
                    },
                    Chunk {
                        before: Side {
                            first_line: 31,
                            lines: lines(&[
                                "impl Search {",
                                "    pub fn new(pattern: &str) -> Self {",
                                "        let case = Case::detect(pattern);",
                                "        Self {",
                                "            pattern: pattern.to_string(),",
                                "            case,",
                                "        }",
                                "    }",
                            ]),
                        },
                        after: Side {
                            first_line: 31,
                            lines: lines(&[
                                "impl Search {",
                                "    pub fn new(query: &str) -> Self {",
                                "        let case = Case::detect(query);",
                                "        Self {",
                                "            query: query.to_string(),",
                                "            case,",
                                "        }",
                                "    }",
                            ]),
                        },
                    },
                    Chunk {
                        before: Side {
                            first_line: 58,
                            lines: lines(&[
                                "    fn matches(&self, line: &str) -> bool {",
                                "        line.contains(&self.pattern)",
                                "    }",
                            ]),
                        },
                        after: Side {
                            first_line: 58,
                            lines: lines(&[
                                "    fn matches(&self, line: &str) -> bool {",
                                "        line.contains(&self.query)",
                                "    }",
                            ]),
                        },
                    },
                ],
            },
        ],
    }
}

/// A commit modifying two chunks of the same file.
fn scrolloff_rename() -> Commit {
    Commit {
        hash: "3f9a2c1".to_string(),
        subject: "config: rename scroll_off to scrolloff".to_string(),
        author: "Alice Martin".to_string(),
        date: date(2026, 10, 12, 14, 32, 7),
        decorations: vec![
            Decoration::Head(Some("main".to_string())),
            Decoration::Tag("v1.0".to_string()),
            Decoration::RemoteBranch("origin/main".to_string()),
        ],
        body: "The option is named `scrolloff` in vim, using the same name makes it\n\
               easier to find. `usize` avoids casts when computing screen offsets."
            .to_string(),
        files: vec![FileDiff {
            path: "src/config.rs".to_string(),
            chunks: vec![
                Chunk {
                    before: Side {
                        first_line: 8,
                        lines: lines(&[
                            "/// Settings read from the config file.",
                            "pub struct Config {",
                            "    pub incremental_search: bool,",
                            "    pub scroll_off: u8,",
                            "}",
                        ]),
                    },
                    after: Side {
                        first_line: 8,
                        lines: lines(&[
                            "/// Settings read from the config file.",
                            "pub struct Config {",
                            "    pub incremental_search: bool,",
                            "    pub scrolloff: usize,",
                            "}",
                        ]),
                    },
                },
                Chunk {
                    before: Side {
                        first_line: 97,
                        lines: lines(&[
                            "impl Default for Config {",
                            "    fn default() -> Self {",
                            "        Self {",
                            "            incremental_search: true,",
                            "            scroll_off: 5,",
                            "        }",
                            "    }",
                        ]),
                    },
                    after: Side {
                        first_line: 97,
                        lines: lines(&[
                            "impl Default for Config {",
                            "    fn default() -> Self {",
                            "        Self {",
                            "            incremental_search: true,",
                            "            scrolloff: DEFAULT_SCROLLOFF,",
                            "        }",
                            "    }",
                        ]),
                    },
                },
            ],
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_have_no_unpaired_lines() {
        commits()
            .into_iter()
            .flat_map(|commit| commit.files)
            .flat_map(|file| file.chunks)
            .for_each(|chunk| assert_eq!(chunk.before.lines.len(), chunk.after.lines.len()));
    }
}
