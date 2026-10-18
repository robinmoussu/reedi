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

/// Four commits, in topological order (oldest first).
pub fn commits() -> Vec<Commit> {
    vec![
        line_wrapping(),
        invisible_characters(),
        search_rename(),
        scrolloff_rename(),
    ]
}

/// A commit whose lines are too wide for a screen 110 columns wide, in all the areas: header,
/// body, and code (including wide characters, tabs, trailing whitespace and right-to-left text).
fn line_wrapping() -> Commit {
    Commit {
        hash: "b0d4e2a".to_string(),
        subject:
            "view: wrap the lines too wide for the screen instead of cropping them at its edge"
                .to_string(),
        author: "Chloé Bernard".to_string(),
        date: date(2026, 10, 1, 11, 47, 20),
        decorations: vec![
            Decoration::Tag("v0.2".to_string()),
            Decoration::RemoteBranch("origin/line-wrapping".to_string()),
        ],
        body:
            "Long lines used to be cropped at the edge of the screen, hiding their end, which is \
               often where the interesting part of a modification is. They are now displayed on \
               several rows, broken at the Unicode line break opportunities.\n\
               A `↵` marks each row of a diff that continues on the next one."
                .to_string(),
        files: vec![
            FileDiff {
                path: "src/checksum.rs".to_string(),
                chunks: vec![Chunk {
                    before: Side {
                        first_line: 10,
                        lines: lines(&[
                            "/// Checksum of the release archive.",
                            "const SHA256: &str = \"9f86d081884c7d659a2feaa0c55ad015\";",
                            "// 確認済み",
                            "    let expected = checksum_of(&archive_path);",
                            "    let actual = read_checksum(&path)?;",
                            "    assert_eq!(expected, actual);",
                            "    const HEADER: &str = \"File name  Size  Checksum\";",
                        ]),
                    },
                    after: Side {
                        first_line: 10,
                        lines: lines(&[
                            "/// Checksum of the release archive, checked before installing it, so \
                             that a corrupted download is never installed.",
                            // The checksum is wider than the text area, so it is split.
                            "const SHA256: &str = \
                             \"9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08\";",
                            "// チェックサムは公開鍵で署名されています🔏 署名がない場合はインストールを中止します✅",
                            "    let expected = checksum_of(&archive_path);    ",
                            "    let actual = read_checksum(&path)?;\t\t",
                            "    assert_eq!(expected, actual);  \t  ",
                            // Multiple spaces in the middle, at the break.
                            "    const HEADER: &str = \"File name              Size    Checksum\";",
                        ]),
                    },
                }],
            },
            FileDiff {
                path: "tests/fixtures/prices.tsv".to_string(),
                chunks: vec![Chunk {
                    before: Side {
                        first_line: 1,
                        lines: lines(&[
                            "name\tunit\tprice",
                            "apple\tkg\t2.50",
                            "pear\tkg\t3.10",
                            "plum\tkg\t4.20",
                        ]),
                    },
                    after: Side {
                        first_line: 1,
                        lines: lines(&[
                            "name\tunit\tprice\tsupplier\tcomment",
                            "apple\tkg\t2.50\tVerger du Sud\tharvested in September, kept in a \
                             cold room until shipping",
                            "pear\tkg\t3.10\tLes Vergers Réunis\tsold out until next year",
                            // A tab followed by multiple spaces, at the break.
                            "plum\tkg\t4.20\tDomaine du Clos Fleuri\t      imported from Italy",
                        ]),
                    },
                }],
            },
            FileDiff {
                path: "docs/welcome.fa.txt".to_string(),
                chunks: vec![Chunk {
                    before: Side {
                        first_line: 1,
                        lines: lines(&[
                            "سلام دنیا: hello world",
                            "Welcome: خوش آمدید",
                            "من این جمله را می\u{200c}بینم",
                        ]),
                    },
                    after: Side {
                        first_line: 1,
                        lines: lines(&[
                            // Starts with right-to-left text, ends with left-to-right text.
                            "سلام به همه\u{200c}ی کسانی که این برنامه را امتحان می\u{200c}کنند: \
                             hello to everyone trying reedi",
                            // Starts with left-to-right text, ends with right-to-left text.
                            "Welcome to everyone reading this file in a terminal: به همه\u{200c}ی \
                             شما خوش آمدید",
                            // Only right-to-left text.
                            "من می\u{200c}خواهم این جمله\u{200c}ی طولانی را در چند سطر ببینم تا \
                             مطمئن شوم که شکستن خط\u{200c}ها درست کار می\u{200c}کند",
                        ]),
                    },
                }],
            },
        ],
    }
}

/// A commit with all kinds of invisible characters, in all the areas: subject, author, body, code
/// with consistent and inconsistent line endings.
fn invisible_characters() -> Commit {
    Commit {
        hash: "c7e05b9".to_string(),
        subject: "lint: add fixtures for\u{200b} invisible characters ".to_string(),
        author: "Zoé\u{a0}Durand".to_string(),
        date: date(2026, 10, 5, 17, 3, 51),
        decorations: Vec::new(),
        body: "Most fixtures were pasted from a web page, hence the no-break spaces\u{a0}! \n\
               The batch script now always uses CRLF line endings.\r"
            .to_string(),
        files: vec![
            FileDiff {
                path: "tests/fixtures/lookalike.rs".to_string(),
                chunks: vec![Chunk {
                    before: Side {
                        first_line: 1,
                        lines: lines(&[
                            "\u{feff}//! Strings that look alike.",
                            "const ADMIN: &str = \"admin\";",
                            "const USER: &str = \"user\";",
                            "const SPACES: &str = \"a b c d\";",
                            "const RED: &str = \"red\";",
                            "const BELL: &str = \"\";",
                            "const LINES: &str = \"a b c d\";",
                            "let filler = is_admin;",
                            "let word = \"abcd\";",
                            "/* admins only */",
                            "// Looks good",
                        ]),
                    },
                    after: Side {
                        first_line: 1,
                        lines: lines(&[
                            "\u{feff}//! Strings that look alike.",
                            "const ADMIN: &str = \"ad\u{200b}min\";",
                            "const USER: &str = \"us\u{ad}er\";",
                            "const SPACES: &str = \"a\u{a0}b\u{2009}c\u{3000}d\";",
                            "const RED: &str = \"\u{1b}[31mred\u{1b}[0m\";",
                            "const BELL: &str = \"\u{7}\u{0}\u{7f}\u{9b}\";",
                            "const LINES: &str = \"a\u{2028}b\u{2029}c\u{85}d\";",
                            "let \u{3164} = is_admin;",
                            "let word = \"a\u{2060}b\u{180e}c\u{34f}d\";",
                            "/*\u{202e} } \u{2066}if is_admin\u{2069} \u{2066} admins only */",
                            // The tag characters spell `Ignore`.
                            "// Looks good\u{e0049}\u{e0067}\u{e006e}\u{e006f}\u{e0072}\u{e0065}",
                        ]),
                    },
                }],
            },
            FileDiff {
                path: "tests/fixtures/emoji.rs".to_string(),
                // Only CRLF line endings, so the carriage returns are hidden.
                chunks: vec![Chunk {
                    before: Side {
                        first_line: 3,
                        lines: lines(&[
                            "const FAMILY: &str = \"👪\";\r",
                            "const HEART: &str = \"❤\";\r",
                            "const FLAG: &str = \"🏴\";\r",
                            "const PERSIAN: &str = \"من میخواهم این جملهی طولانی را در چند سطر ببینم\";\r",
                            "const JOINED: &str = \"ab\";\r",
                        ]),
                    },
                    after: Side {
                        first_line: 3,
                        lines: lines(&[
                            "const FAMILY: &str = \"👨\u{200d}👩\u{200d}👧\";\r",
                            "const HEART: &str = \"❤\u{fe0f}\";\r",
                            "const FLAG: &str = \"🏴\u{e0067}\u{e0062}\u{e0073}\u{e0063}\u{e0074}\u{e007f}\";\r",
                            "const PERSIAN: &str = \"من می\u{200c}خواهم این جمله\u{200c}ی طولانی را در چند سطر ببینم\";\r",
                            "const JOINED: &str = \"a\u{200d}b\u{fe0f}\";\r",
                        ]),
                    },
                }],
            },
            FileDiff {
                path: "Makefile".to_string(),
                chunks: vec![Chunk {
                    before: Side {
                        first_line: 1,
                        lines: lines(&[
                            "CFLAGS = -O2",
                            "all: build ",
                            "",
                            "build:",
                            "\tcc $(CFLAGS) -o app main.c",
                        ]),
                    },
                    after: Side {
                        first_line: 1,
                        lines: lines(&[
                            "CFLAGS\t= -O2",
                            "all: build\t",
                            "  \t ",
                            "build:",
                            "\tcc $(CFLAGS) -o app main.c",
                        ]),
                    },
                }],
            },
            FileDiff {
                path: "scripts/build.bat".to_string(),
                // The last line has no carriage return, so all of them are displayed.
                chunks: vec![Chunk {
                    before: Side {
                        first_line: 1,
                        lines: lines(&["@echo off\r", "cl /O2 main.c\r", "exit /b 0\r"]),
                    },
                    after: Side {
                        first_line: 1,
                        lines: lines(&["@echo off\r", "cl /O2 /utf-8 main.c\r", "exit /b 0"]),
                    },
                }],
            },
            FileDiff {
                path: "tests/fixtures/rtl.txt".to_string(),
                // Only right-to-left characters.
                chunks: vec![Chunk {
                    before: Side {
                        first_line: 1,
                        lines: lines(&["سلام دنیا"]),
                    },
                    after: Side {
                        first_line: 1,
                        lines: lines(&["سلام دنیای زیبا"]),
                    },
                }],
            },
        ],
    }
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
