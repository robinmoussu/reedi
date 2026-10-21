//! Realistic fake data, used until real git data is read.

use gix::date::Time;
use jiff::civil;
use jiff::tz;
use jiff::tz::TimeZone;

use crate::diff;
use crate::diff::FileState;
use crate::model::Commit;
use crate::model::Decoration;
use crate::model::FileDiff;

/// The number of characters of the short hash of all the fake commits.
const FAKE_HASH_ABBREV_LEN: usize = 7;

fn lines(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| line.to_string()).collect()
}

/// The lines `first..=last` of a Rust file, never displayed because they are too far from the
/// modified lines.
fn hidden(first: usize, last: usize) -> Vec<String> {
    (first..=last).map(|n| format!("// Line {n}.")).collect()
}

/// The content of a text file made of `lines`, each one ending with a newline.
fn text<S: AsRef<str>>(lines: &[S]) -> Vec<u8> {
    lines
        .iter()
        .flat_map(|line| [line.as_ref(), "\n"])
        .collect::<String>()
        .into_bytes()
}

/// `year-month-day hour:minute:second`, in UTC+2.
fn date(year: i16, month: i8, day: i8, hour: i8, minute: i8, second: i8) -> Time {
    let offset = tz::offset(2);
    let zoned = civil::date(year, month, day)
        .at(hour, minute, second, 0)
        .to_zoned(TimeZone::fixed(offset))
        .expect("the fake date is valid");
    Time::new(zoned.timestamp().as_second(), offset.seconds())
}

/// A file modified by a fake commit: its path and content before and after the commit (`None` if
/// it doesn't exist in that version). A file is renamed if its paths are different.
struct FakeFile {
    before: Option<(&'static str, Vec<u8>)>,
    after: Option<(&'static str, Vec<u8>)>,
    /// Whether the file is binary (in the future, it will be known from git and `.gitattributes`).
    binary: bool,
}

impl FakeFile {
    fn modified(path: &'static str, before: Vec<u8>, after: Vec<u8>) -> Self {
        Self::renamed(path, path, before, after)
    }

    fn renamed(from: &'static str, to: &'static str, before: Vec<u8>, after: Vec<u8>) -> Self {
        Self {
            before: Some((from, before)),
            after: Some((to, after)),
            binary: false,
        }
    }

    fn added(path: &'static str, content: Vec<u8>) -> Self {
        Self {
            before: None,
            after: Some((path, content)),
            binary: false,
        }
    }

    fn deleted(path: &'static str, content: Vec<u8>) -> Self {
        Self {
            before: Some((path, content)),
            after: None,
            binary: false,
        }
    }

    fn binary(path: &'static str, before: Vec<u8>, after: Vec<u8>) -> Self {
        Self {
            binary: true,
            ..Self::modified(path, before, after)
        }
    }

    fn diff(&self) -> FileDiff {
        fn state<'a>(state: &'a Option<(&'static str, Vec<u8>)>) -> Option<FileState<'a>> {
            state.as_ref().map(|(path, content)| FileState {
                path: path.as_bytes().into(),
                content,
            })
        }
        diff::file(
            state(&self.before),
            state(&self.after),
            self.binary,
            diff::CONTEXT,
        )
    }
}

fn diffs(files: &[FakeFile]) -> Vec<FileDiff> {
    files.iter().map(FakeFile::diff).collect()
}

/// The fake commits, in topological order (oldest first).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakeCommit {
    FileChanges,
    LineWrapping,
    InvisibleCharacters,
    SearchRename,
    ScrolloffRename,
}

impl FakeCommit {
    /// The variants, in topological order (oldest first).
    const ALL: [FakeCommit; 5] = [
        FakeCommit::FileChanges,
        FakeCommit::LineWrapping,
        FakeCommit::InvisibleCharacters,
        FakeCommit::SearchRename,
        FakeCommit::ScrolloffRename,
    ];

    /// All the fake commits, in topological order (oldest first).
    pub fn all() -> Vec<Commit> {
        Self::ALL.into_iter().map(Self::commit).collect()
    }

    /// The most recent fake commit.
    pub fn newest() -> Commit {
        let [.., newest] = Self::ALL;
        newest.commit()
    }

    pub fn commit(self) -> Commit {
        match self {
            FakeCommit::FileChanges => file_changes(),
            FakeCommit::LineWrapping => line_wrapping(),
            FakeCommit::InvisibleCharacters => invisible_characters(),
            FakeCommit::SearchRename => search_rename(),
            FakeCommit::ScrolloffRename => scrolloff_rename(),
        }
    }
}

/// A commit with all kinds of file changes: added, deleted, renamed, binary and not valid UTF-8
/// files, files without newline at their end, and blocks of lines that are only removed or only
/// added.
fn file_changes() -> Commit {
    Commit {
        hash: "5e2b9d4a1f03c7e8b2d946051fa3c8e7d2b4096a".to_string(),
        hash_abbrev_len: FAKE_HASH_ABBREV_LEN,
        subject: "release: install with a shell script instead of the Makefile, and check the \
                  downloaded archive"
            .to_string(),
        author: "Noah Petit".to_string(),
        email: "noah.petit@example.org".to_string(),
        date: date(2026, 9, 28, 10, 24, 5),
        decorations: Vec::new(),
        body: "The Makefile only worked with GNU make, a POSIX shell script works everywhere.\n\
               The archive is now checked before being installed."
            .to_string(),
        files: diffs(&[
            FakeFile::modified(
                "src/release.rs",
                text(&[
                    "//! Download and installation of the releases.",
                    "",
                    "use std::fs;",
                    "use std::io;",
                    "use std::path::Path;",
                    "",
                    "use crate::archive::Archive;",
                    "",
                    "/// Installs the release archive at `path` in `prefix`.",
                    "pub fn install(path: &Path, prefix: &Path) -> io::Result<()> {",
                    "    let archive = Archive::open(path)?;",
                    "    archive.extract(prefix)?;",
                    "    Ok(())",
                    "}",
                    "",
                    "/// The directory in which the releases are installed by default.",
                    "pub fn default_prefix() -> &'static Path {",
                    "    Path::new(\"/usr/local\")",
                    "}",
                    "",
                    "/// Removes the temporary files of the previous installations.",
                    "pub fn clean(directory: &Path) -> io::Result<()> {",
                    "    // Kept for compatibility with the releases older than v0.1.",
                    "    let legacy = directory.join(\"reedi.old\");",
                    "    if legacy.exists() {",
                    "        fs::remove_file(legacy)?;",
                    "    }",
                    "    fs::remove_dir_all(directory.join(\"tmp\"))",
                    "}",
                ]),
                text(&[
                    "//! Download and installation of the releases.",
                    "",
                    "use std::fs;",
                    "use std::io;",
                    "use std::path::Path;",
                    "",
                    "use crate::archive::Archive;",
                    "",
                    "/// Installs the release archive at `path` in `prefix`.",
                    "pub fn install(path: &Path, prefix: &Path) -> io::Result<()> {",
                    "    let archive = Archive::open(path)?;",
                    // Only added lines.
                    "    if !archive.has_valid_checksum()? {",
                    "        return Err(io::Error::other(\"the checksum of the archive is invalid\"));",
                    "    }",
                    "    archive.extract(prefix)?;",
                    "    Ok(())",
                    "}",
                    "",
                    "/// The directory in which the releases are installed by default.",
                    "pub fn default_prefix() -> &'static Path {",
                    "    Path::new(\"/usr/local\")",
                    "}",
                    "",
                    "/// Removes the temporary files of the previous installations.",
                    "pub fn clean(directory: &Path) -> io::Result<()> {",
                    // Only removed lines.
                    "    fs::remove_dir_all(directory.join(\"tmp\"))",
                    "}",
                ]),
            ),
            FakeFile::renamed(
                "src/hash.rs",
                "src/digest.rs",
                text(&[
                    "//! Hash of the release archives.",
                    "",
                    "use sha2::Digest;",
                    "use sha2::Sha256;",
                    "",
                    "/// The SHA-256 hash of `data`, in hexadecimal.",
                    "pub fn sha256(data: &[u8]) -> String {",
                    "    let hash = Sha256::digest(data);",
                    "    hash.iter().map(|byte| format!(\"{byte:02x}\")).collect()",
                    "}",
                ]),
                text(&[
                    "//! Digest of the release archives.",
                    "",
                    "use sha2::Digest;",
                    "use sha2::Sha256;",
                    "",
                    "/// The SHA-256 hash of `data`, in hexadecimal.",
                    "pub fn sha256(data: &[u8]) -> String {",
                    "    let hash = Sha256::digest(data);",
                    "    hash.iter().map(|byte| format!(\"{byte:02x}\")).collect()",
                    "}",
                ]),
            ),
            // Both versions don't end with a newline.
            FakeFile::modified(
                "scripts/install.sh",
                "#!/bin/sh\n\
                 set -eu\n\
                 \n\
                 prefix=\"${PREFIX:-/usr/local}\"\n\
                 \n\
                 make build\n\
                 install -m 755 target/release/reedi \"$prefix/bin/reedi\"\n\
                 echo \"reedi installed in $prefix/bin\""
                    .into(),
                "#!/bin/sh\n\
                 set -eu\n\
                 \n\
                 prefix=\"${PREFIX:-/usr/local}\"\n\
                 \n\
                 cargo build --release\n\
                 sha256sum --check reedi.sha256\n\
                 install -m 755 target/release/reedi \"$prefix/bin/reedi\"\n\
                 echo \"reedi installed in $prefix/bin\""
                    .into(),
            ),
            FakeFile::added(
                "scripts/uninstall.sh",
                text(&[
                    "#!/bin/sh",
                    "set -eu",
                    "",
                    "rm \"${PREFIX:-/usr/local}/bin/reedi\"",
                ]),
            ),
            FakeFile::deleted(
                "Makefile",
                text(&[
                    "PREFIX ?= /usr/local",
                    "",
                    "build:",
                    "\tcargo build --release",
                    "",
                    "install: build",
                    "\tinstall -m 755 target/release/reedi $(PREFIX)/bin/reedi",
                ]),
            ),
            // Only the before version doesn't end with a newline.
            FakeFile::modified(
                "config/default.toml",
                "scrolloff = 5\ntheme = \"dark\"".into(),
                "scrolloff = 5\ntheme = \"dark\"\ntab_width = 4\n".into(),
            ),
            FakeFile::binary(
                "assets/logo.png",
                b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x10".to_vec(),
                b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x20".to_vec(),
            ),
            // Encoded in Latin-1.
            FakeFile::modified(
                "tests/fixtures/latin1.txt",
                b"caf\xe9\n".to_vec(),
                b"caf\xe9 cr\xe8me\n".to_vec(),
            ),
        ]),
    }
}

/// A commit whose lines are too wide for a screen 110 columns wide, in all the areas: header,
/// body, and code (including wide characters, tabs, trailing whitespace and right-to-left text).
fn line_wrapping() -> Commit {
    let checksum_imports = lines(&[
        "use crate::archive::checksum_of;",
        "use crate::archive::read_checksum;",
        "",
    ]);
    Commit {
        hash: "b0d4e2a3c91f5d7a8e260b4c9d1f3a5e7b82c046".to_string(),
        hash_abbrev_len: FAKE_HASH_ABBREV_LEN,
        subject:
            "view: wrap the lines too wide for the screen instead of cropping them at its edge"
                .to_string(),
        author: "Chloé Bernard".to_string(),
        email: "chloe.bernard@example.org".to_string(),
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
        files: diffs(&[
            FakeFile::modified(
                "src/checksum.rs",
                text(
                    &[
                        hidden(1, 6),
                        checksum_imports.clone(),
                        lines(&[
                            "/// Checksum of the release archive.",
                            "const SHA256: &str = \"9f86d081884c7d659a2feaa0c55ad015\";",
                            "// 確認済み",
                            "    let expected = checksum_of(&archive_path);",
                            "    let actual = read_checksum(&path)?;",
                            "    assert_eq!(expected, actual);",
                            "    const HEADER: &str = \"File name  Size  Checksum\";",
                        ]),
                    ]
                    .concat(),
                ),
                text(
                    &[
                        hidden(1, 6),
                        checksum_imports,
                        lines(&[
                            "/// Checksum of the release archive, checked before installing it, \
                             so that a corrupted download is never installed.",
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
                    ]
                    .concat(),
                ),
            ),
            FakeFile::modified(
                "tests/fixtures/prices.tsv",
                text(&[
                    "name\tunit\tprice",
                    "apple\tkg\t2.50",
                    "pear\tkg\t3.10",
                    "plum\tkg\t4.20",
                ]),
                text(&[
                    "name\tunit\tprice\tsupplier\tcomment",
                    "apple\tkg\t2.50\tVerger du Sud\tharvested in September, kept in a cold room \
                     until shipping",
                    "pear\tkg\t3.10\tLes Vergers Réunis\tsold out until next year",
                    // A tab followed by multiple spaces, at the break.
                    "plum\tkg\t4.20\tDomaine du Clos Fleuri\t      imported from Italy",
                ]),
            ),
            FakeFile::modified(
                "docs/welcome.fa.txt",
                text(&[
                    "سلام دنیا: hello world",
                    "Welcome: خوش آمدید",
                    "من این جمله را می\u{200c}بینم",
                ]),
                text(&[
                    // Starts with right-to-left text, ends with left-to-right text.
                    "سلام به همه\u{200c}ی کسانی که این برنامه را امتحان می\u{200c}کنند: \
                     hello to everyone trying reedi",
                    // Starts with left-to-right text, ends with right-to-left text.
                    "Welcome to everyone reading this file in a terminal: به همه\u{200c}ی شما خوش \
                     آمدید",
                    // Only right-to-left text.
                    "من می\u{200c}خواهم این جمله\u{200c}ی طولانی را در چند سطر ببینم تا مطمئن شوم \
                     که شکستن خط\u{200c}ها درست کار می\u{200c}کند",
                ]),
            ),
        ]),
    }
}

/// A commit with all kinds of invisible characters, in all the areas: subject, author, body, code
/// with consistent and inconsistent line endings.
fn invisible_characters() -> Commit {
    Commit {
        hash: "c7e05b9d2a4f6081c3e5b7d9f0a2c4e6b8d1f357".to_string(),
        hash_abbrev_len: FAKE_HASH_ABBREV_LEN,
        subject: "lint: add fixtures for\u{200b} invisible characters ".to_string(),
        author: "Zoé\u{a0}Durand".to_string(),
        email: "zoe.durand@example.org".to_string(),
        date: date(2026, 10, 5, 17, 3, 51),
        decorations: Vec::new(),
        body: "Most fixtures were pasted from a web page, hence the no-break spaces\u{a0}! \n\
               The batch script now always uses CRLF line endings.\r"
            .to_string(),
        files: diffs(&[
            FakeFile::modified(
                "tests/fixtures/lookalike.rs",
                text(&[
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
                text(&[
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
            ),
            // Only CRLF line endings, so the carriage returns are hidden.
            FakeFile::modified(
                "tests/fixtures/emoji.rs",
                text(&[
                    "//! Emoji made of several characters.\r",
                    "\r",
                    "const FAMILY: &str = \"👪\";\r",
                    "const HEART: &str = \"❤\";\r",
                    "const FLAG: &str = \"🏴\";\r",
                    "const PERSIAN: &str = \"من میخواهم این جملهی طولانی را در چند سطر ببینم\";\r",
                    "const JOINED: &str = \"ab\";\r",
                ]),
                text(&[
                    "//! Emoji made of several characters.\r",
                    "\r",
                    "const FAMILY: &str = \"👨\u{200d}👩\u{200d}👧\";\r",
                    "const HEART: &str = \"❤\u{fe0f}\";\r",
                    "const FLAG: &str = \"🏴\u{e0067}\u{e0062}\u{e0073}\u{e0063}\u{e0074}\u{e007f}\";\r",
                    "const PERSIAN: &str = \"من می\u{200c}خواهم این جمله\u{200c}ی طولانی را در چند سطر ببینم\";\r",
                    "const JOINED: &str = \"a\u{200d}b\u{fe0f}\";\r",
                ]),
            ),
            FakeFile::modified(
                "Makefile",
                text(&[
                    "CFLAGS = -O2",
                    "all: build ",
                    "",
                    "build:",
                    "\tcc $(CFLAGS) -o app main.c",
                ]),
                text(&[
                    "CFLAGS\t= -O2",
                    "all: build\t",
                    "  \t ",
                    "build:",
                    "\tcc $(CFLAGS) -o app main.c",
                ]),
            ),
            // The last line has no carriage return, so all of them are displayed.
            FakeFile::modified(
                "scripts/build.bat",
                text(&["@echo off\r", "cl /O2 main.c\r", "exit /b 0\r"]),
                text(&["@echo off\r", "cl /O2 /utf-8 main.c\r", "exit /b 0"]),
            ),
            // Only right-to-left characters.
            FakeFile::modified(
                "tests/fixtures/rtl.txt",
                text(&["سلام دنیا"]),
                text(&["سلام دنیای زیبا"]),
            ),
        ]),
    }
}

/// A commit modifying one chunk of a file and three chunks of another one.
fn search_rename() -> Commit {
    Commit {
        hash: "8d41e07b6c93a5f1e7d2084b6a9c3e5f7d1b2a48".to_string(),
        hash_abbrev_len: FAKE_HASH_ABBREV_LEN,
        subject: "search: rename pattern to query".to_string(),
        author: "Bruno Lefebvre".to_string(),
        email: "bruno.lefebvre@example.org".to_string(),
        date: date(2026, 10, 9, 9, 15, 42),
        decorations: Vec::new(),
        body: "`query` is the name used in the status bar.".to_string(),
        files: diffs(&[
            FakeFile::modified(
                "src/app.rs",
                text(
                    &[
                        hidden(1, 39),
                        lines(&[
                            "        KeyCode::Esc => self.mode = Mode::Review,",
                            "        KeyCode::Char(c) => self.search.push(c),",
                            "        KeyCode::Enter => {",
                            "            let pattern = &self.search.pattern;",
                            "            self.history.push(pattern.clone());",
                            "            self.mode = Mode::Review;",
                            "        }",
                            "        _ => {}",
                        ]),
                    ]
                    .concat(),
                ),
                text(
                    &[
                        hidden(1, 39),
                        lines(&[
                            "        KeyCode::Esc => self.mode = Mode::Review,",
                            "        KeyCode::Char(c) => self.search.push(c),",
                            "        KeyCode::Enter => {",
                            "            let query = &self.search.query;",
                            "            self.history.push(query.clone());",
                            "            self.mode = Mode::Review;",
                            "        }",
                            "        _ => {}",
                        ]),
                    ]
                    .concat(),
                ),
            ),
            FakeFile::modified(
                "src/search.rs",
                text(&search_file("pattern")),
                text(&search_file("query")),
            ),
        ]),
    }
}

/// The lines of `src/search.rs`, `field` being the name of the searched text.
fn search_file(field: &str) -> Vec<String> {
    [
        hidden(1, 10),
        lines(&[
            "",
            "/// An incremental search in the diffs.",
            "pub struct Search {",
            &format!("    pub {field}: String,"),
            "    pub case: Case,",
            "}",
            "",
        ]),
        hidden(18, 28),
        lines(&[
            "}",
            "",
            "impl Search {",
            &format!("    pub fn new({field}: &str) -> Self {{"),
            &format!("        let case = Case::detect({field});"),
            "        Self {",
            &format!("            {field}: {field}.to_string(),"),
            "            case,",
            "        }",
            "    }",
        ]),
        hidden(39, 55),
        lines(&[
            "",
            "    /// Whether `line` matches the search.",
            "    fn matches(&self, line: &str) -> bool {",
            &format!("        line.contains(&self.{field})"),
            "    }",
            "}",
        ]),
    ]
    .concat()
}

/// A commit modifying two chunks of the same file.
fn scrolloff_rename() -> Commit {
    let config = |field: &str, field_type: &str, default: &str| {
        [
            hidden(1, 7),
            lines(&[
                "/// Settings read from the config file.",
                "pub struct Config {",
                "    pub incremental_search: bool,",
                &format!("    pub {field}: {field_type},"),
                "}",
                "",
                "impl Config {",
            ]),
            hidden(15, 95),
            lines(&[
                "",
                "impl Default for Config {",
                "    fn default() -> Self {",
                "        Self {",
                "            incremental_search: true,",
                &format!("            {field}: {default},"),
                "        }",
                "    }",
                "}",
            ]),
        ]
        .concat()
    };
    Commit {
        hash: "3f9a2c1e5d7b0a4c8f2169e3b5d7a1c9f4e6280b".to_string(),
        hash_abbrev_len: FAKE_HASH_ABBREV_LEN,
        subject: "config: rename scroll_off to scrolloff".to_string(),
        author: "Alice Martin".to_string(),
        email: "alice.martin@example.org".to_string(),
        date: date(2026, 10, 12, 14, 32, 7),
        decorations: vec![
            Decoration::Head(Some("main".to_string())),
            Decoration::Tag("v1.0".to_string()),
            Decoration::RemoteBranch("origin/main".to_string()),
        ],
        body: "The option is named `scrolloff` in vim, using the same name makes it\n\
               easier to find. `usize` avoids casts when computing screen offsets."
            .to_string(),
        files: diffs(&[FakeFile::modified(
            "src/config.rs",
            text(&config("scroll_off", "u8", "5")),
            text(&config("scrolloff", "usize", "DEFAULT_SCROLLOFF")),
        )]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::FileChange;
    use crate::model::FileContentType;
    use crate::model::LineKind;

    #[test]
    fn fake_hashes_are_full_hashes() {
        FakeCommit::all().iter().for_each(|commit| {
            let hash = commit.hash.to_string();
            assert_eq!(hash.len(), 40, "{hash}");
            assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()), "{hash}");
        });
    }

    #[test]
    fn hidden_lines_are_numbered() {
        assert_eq!(hidden(3, 4), ["// Line 3.", "// Line 4."]);
    }

    #[test]
    fn text_lines_end_with_a_newline() {
        assert_eq!(text(&["a", "b"]), b"a\nb\n");
        assert_eq!(text::<&str>(&[]), b"");
    }

    #[test]
    fn all_kinds_of_file_changes_are_used() {
        let files = &file_changes().files;
        let changes: Vec<_> = files.iter().map(|file| &file.change).collect();
        assert!(changes.contains(&&FileChange::Added));
        assert!(changes.contains(&&FileChange::Deleted));
        assert!(changes.contains(&&FileChange::Renamed {
            from: "src/hash.rs".into()
        }));
        let contents: Vec<_> = files.iter().map(|file| file.content).collect();
        assert!(contents.contains(&FileContentType::Binary));
        assert!(contents.contains(&FileContentType::InvalidUtf8));
    }

    #[test]
    fn some_lines_are_only_removed_or_only_added() {
        let release = &file_changes().files[0];
        let kinds = |chunk: usize| -> Vec<_> {
            release.chunks[chunk]
                .lines
                .iter()
                .map(|line| line.kind)
                .filter(|kind| *kind != LineKind::Unchanged)
                .collect()
        };
        assert_eq!(kinds(0), [LineKind::Added; 3]);
        assert_eq!(kinds(1), [LineKind::Removed; 5]);
    }

    #[test]
    fn files_without_newline_at_their_end() {
        let files = file_changes().files;
        let missing = |path: &str| -> Vec<_> {
            let file = files.iter().find(|file| file.path == path).unwrap();
            let lines = file.chunks.iter().flat_map(|chunk| &chunk.lines);
            lines
                .filter(|line| line.missing_newline)
                .map(|line| line.kind)
                .collect()
        };
        assert_eq!(missing("scripts/install.sh"), [LineKind::Unchanged]);
        assert_eq!(missing("config/default.toml"), [LineKind::Removed]);
    }
}
