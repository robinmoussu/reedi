//! Realistic fake data, used until real git data is read.

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

/// A single commit modifying two chunks of the same file.
pub fn commit() -> Commit {
    let date = civil::date(2026, 10, 12)
        .at(14, 32, 7, 0)
        .to_zoned(TimeZone::fixed(tz::offset(2)))
        .expect("the fake date is valid");

    Commit {
        hash: "3f9a2c1".to_string(),
        subject: "config: rename scroll_off to scrolloff".to_string(),
        author: "Alice Martin".to_string(),
        date,
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
        commit()
            .files
            .into_iter()
            .flat_map(|file| file.chunks)
            .for_each(|chunk| assert_eq!(chunk.before.lines.len(), chunk.after.lines.len()));
    }
}
