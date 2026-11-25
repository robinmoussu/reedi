//! A repository matching the performance target of the README: [`REVISIONS_COUNT`] unpushed
//! revisions of about 1000 changed lines each (insertions + deletions, as `git log --stat` counts
//! them).
//!
//! Each revision mixes what makes the diffs costly: several file types, hunks whose removed and
//! added lines counts differ, edited lines (paired by the line-diff, then word-diffed), moved
//! blocks (some of them lightly edited), long lines (wrapped), new, deleted and renamed files,
//! and a binary file. Some revisions are guaranteed to make each of these changes (see
//! [`GUARANTEED_CHANGES`]), including non-trivial moves: code embedded in a file (an inline script
//! of an HTML file, an example of a doc comment) moved to a dedicated file. Its diffs are shaped like those of real code: mostly small hunks adding more
//! lines than they remove, edits of varied strength, and files using words of their own, so that
//! unrelated lines share few words. The repository is the same each time it is generated.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::ops::Range;
use std::ops::RangeInclusive;
use std::thread;
use std::time::Duration;

use rand::RngExt;
use rand::SeedableRng;
use rand::seq::IndexedRandom;
use rand::seq::index;
use rand_chacha::ChaCha8Rng;

use crate::test_git::Git;

/// The number of unpushed revisions.
pub const REVISIONS_COUNT: usize = 10;
/// The insertions + deletions of each revision, as `git log --stat` counts them.
pub const CHANGED_LINES_COUNTS: RangeInclusive<usize> = 750..=1200;

/// The seed of the random generator, so that the repository is the same each time.
const SEED: u64 = 14;
/// How long after its files the index is written again, so that their timestamps are older than
/// it. A file whose timestamp isn't older than the index might have changed since it was indexed
/// ("racy git"): every load would read and hash it, unlike in a repository whose files were not
/// just written.
const INDEX_REFRESH_DELAY: Duration = Duration::from_secs(1);
/// The changed lines counted by the generator for each revision. Above most of
/// [`CHANGED_LINES_COUNTS`], since `git` finds a shorter diff than the generated changes (hunks
/// overlapping in a revision, removed and added lines that happen to be equal).
const GENERATED_CHANGED_LINES_COUNTS: RangeInclusive<usize> = 1100..=1200;
/// The lines count of each file of the base commit.
const BASE_LINES_COUNTS: RangeInclusive<usize> = 50..=600;
/// The lines count of each new file.
const NEW_LINES_COUNTS: RangeInclusive<usize> = 20..=150;
/// The maximal lines count of a deleted file: real revisions rarely delete large files.
const MAX_DELETED_LINES_COUNT: usize = 100;
/// The number of text files modified by each revision (besides the new, deleted, renamed and
/// binary files).
const TOUCHED_FILES_COUNTS: RangeInclusive<usize> = 3..=12;
/// The maximal number of hunks of a file in each revision.
const MAX_HUNKS_COUNT: usize = 16;
/// The maximal number of lines removed, and of lines added, by a hunk.
const MAX_HUNK_LINES_COUNT: usize = 100;
/// The scale of the lines counts of the hunks (see [`Generator::skewed_count()`]).
const HUNK_LINES_SCALE: f64 = 1.0;
/// The probability that a hunk only adds lines.
const PURE_ADDITION_PROBABILITY: f64 = 0.22;
/// The probability that a hunk only removes lines.
const PURE_REMOVAL_PROBABILITY: f64 = 0.04;
/// The probability that an added line of a hunk is one of its removed lines, edited.
const EDITED_LINE_PROBABILITY: f64 = 0.5;
/// The directory of the test files, which receive the examples moved out of doc comments.
const TESTS_DIRECTORY: &str = "tests";
/// The number of embedded blocks (inline scripts, doc comment examples) of each new file whose
/// language has them.
const EMBEDDED_BLOCKS_COUNT: usize = 3;
/// The lines count of the code of each embedded block.
const EMBEDDED_LINES_COUNTS: RangeInclusive<usize> = 5..=12;
/// The lines enclosing an inline script of an HTML file.
const SCRIPT_OPENING: &str = "<script>";
const SCRIPT_CLOSING: &str = "</script>";
/// The line opening and closing an example of a Rust doc comment.
const RUST_EXAMPLE_FENCE: &str = "/// ```";
/// The quotes opening and closing a Python docstring.
const DOCSTRING_QUOTES: &str = "\"\"\"";
/// The maximal number of lines edited in a lightly edited moved block.
const MAX_LIGHTLY_EDITED_LINES_COUNT: usize = 3;
/// The probability that a change of a modified file moves a block, instead of being a hunk.
const MOVE_PROBABILITY: f64 = 0.04;
/// The lines count of each moved block.
const MOVED_LINES_COUNTS: RangeInclusive<usize> = 5..=30;
/// The maximal number of lines removed, and of lines added, by each hunk of a renamed file, so
/// that `git` still detects the rename.
const MAX_RENAMED_HUNK_LINES_COUNT: usize = 5;
/// The probability that a revision adds a file, that it deletes one, and that it renames one.
const FILE_CHANGE_PROBABILITY: f64 = 1.0 / 3.0;
/// The size of the binary file.
const BINARY_SIZE: usize = 2048;
/// The binary file, modified by a single revision.
const BINARY_PATH: &str = "assets/icon.png";
/// The syllables of the generated words.
const SYLLABLES: &[&str] = &[
    "ba", "bel", "ce", "cor", "di", "dun", "fir", "fo", "gu", "ka", "le", "mi", "no", "pu", "ra",
    "se", "ti", "vo", "xu", "zan",
];
/// The number of syllables of each generated word.
const WORD_SYLLABLES_COUNTS: RangeInclusive<usize> = 2..=3;
/// The number of distinct words of the repository.
const VOCABULARY_SIZE: usize = 500;
/// The number of words of each file, drawn from the vocabulary: lines of the same file share
/// words, lines of different files share few.
const FILE_WORDS_COUNT: usize = 150;
/// The scale of the number of words of a sentence (comments, strings, paragraphs; see
/// [`Generator::skewed_count()`]): most sentences are short, a few are long.
const SENTENCE_WORDS_SCALE: f64 = 3.0;
/// The maximal number of words of a sentence, up to about 250 columns.
const MAX_SENTENCE_WORDS_COUNT: usize = 35;
/// The probability that a generated line is blank.
const BLANK_LINE_PROBABILITY: f64 = 0.1;
/// The number of words replaced by an edit replacing words.
const REPLACED_WORDS_COUNTS: RangeInclusive<usize> = 1..=3;
/// The maximal number of indentation levels.
const MAX_INDENTATION_LEVEL: usize = 3;

/// The language of a text file, which gives its extension and the shape of its lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Language {
    Rust,
    Python,
    C,
    CHeader,
    Markdown,
    Toml,
    Json,
    Shell,
    Html,
    JavaScript,
}

/// The directory, the language and the number of the text files of the base commit.
const BASE_FILES: [(&str, Language, usize); 10] = [
    ("src", Language::Rust, 5),
    ("scripts", Language::Python, 3),
    ("native", Language::C, 2),
    ("native", Language::CHeader, 1),
    ("doc", Language::Markdown, 3),
    ("config", Language::Toml, 2),
    ("data", Language::Json, 2),
    ("scripts", Language::Shell, 2),
    ("web", Language::Html, 2),
    ("web", Language::JavaScript, 2),
];

impl Language {
    fn extension(self) -> &'static str {
        match self {
            Language::Rust => "rs",
            Language::Python => "py",
            Language::C => "c",
            Language::CHeader => "h",
            Language::Markdown => "md",
            Language::Toml => "toml",
            Language::Json => "json",
            Language::Shell => "sh",
            Language::Html => "html",
            Language::JavaScript => "js",
        }
    }

    /// The language of the code embedded in a file in this language (inline scripts, doc comment
    /// examples), if it has some.
    fn embedded_language(self) -> Option<Language> {
        match self {
            Language::Html => Some(Language::JavaScript),
            Language::Rust | Language::Python => Some(self),
            _ => None,
        }
    }

    /// The prefix of each line of embedded code, after the indentation of its block.
    fn embedded_code_prefix(self) -> String {
        match self {
            Language::Rust => "/// ".to_string(),
            Language::Python => ">>> ".to_string(),
            _ => self.indentation(1),
        }
    }

    /// Whether `code` (a line without its indentation) opens an embedded block.
    fn is_embedded_opening(self, code: &str) -> bool {
        match self {
            Language::Html => code == SCRIPT_OPENING,
            Language::Rust => code == RUST_EXAMPLE_FENCE,
            Language::Python => code.starts_with(DOCSTRING_QUOTES) && code != DOCSTRING_QUOTES,
            _ => false,
        }
    }

    /// Whether `code` (a line without its indentation) closes an embedded block.
    fn is_embedded_closing(self, code: &str) -> bool {
        match self {
            Language::Html => code == SCRIPT_CLOSING,
            Language::Rust => code == RUST_EXAMPLE_FENCE,
            Language::Python => code == DOCSTRING_QUOTES,
            _ => false,
        }
    }

    /// The indentation of a line `level` levels deep.
    fn indentation(self, level: usize) -> String {
        match self {
            Language::C | Language::CHeader | Language::Shell => "\t".repeat(level),
            Language::Markdown | Language::Toml => String::new(),
            Language::Rust | Language::Python | Language::Json => "    ".repeat(level),
            Language::Html | Language::JavaScript => "  ".repeat(level),
        }
    }
}

/// A text file of the working tree.
#[derive(Debug, Clone)]
struct TextFile {
    language: Language,
    /// The words of its lines, drawn from the vocabulary.
    words: Vec<String>,
    lines: Vec<String>,
}

/// The content of a file of the working tree.
#[derive(Debug, Clone)]
enum Content {
    Text(TextFile),
    Binary(Vec<u8>),
}

/// The generator: its random generator, the words of the repository, and the files of the working
/// tree by path.
struct Generator {
    rng: ChaCha8Rng,
    /// Sorted.
    vocabulary: Vec<String>,
    files: BTreeMap<String, Content>,
}

/// The uncommitted changes the generator leaves, each made like a revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Uncommitted {
    None,
    /// Staged changes.
    Index,
    /// Unstaged changes, including an untracked file.
    WorkingDirectory,
    /// Staged changes, and unstaged changes on top of them.
    Both,
}

impl Uncommitted {
    pub const ALL: [Uncommitted; 4] = [
        Uncommitted::None,
        Uncommitted::Index,
        Uncommitted::WorkingDirectory,
        Uncommitted::Both,
    ];

    /// The name of the option, as written on the command line.
    pub fn name(self) -> &'static str {
        match self {
            Uncommitted::None => "none",
            Uncommitted::Index => "index",
            Uncommitted::WorkingDirectory => "working-directory",
            Uncommitted::Both => "both",
        }
    }

    pub fn from_name(name: &str) -> Option<Uncommitted> {
        Uncommitted::ALL
            .into_iter()
            .find(|uncommitted| uncommitted.name() == name)
    }

    fn has_index(self) -> bool {
        matches!(self, Uncommitted::Index | Uncommitted::Both)
    }

    fn has_working_directory(self) -> bool {
        matches!(self, Uncommitted::WorkingDirectory | Uncommitted::Both)
    }
}

/// Generate the repository in `git`: a base commit, which is the upstream of `BRANCH`,
/// [`REVISIONS_COUNT`] revisions on top of it, and the `uncommitted` changes, then refresh its
/// index [`INDEX_REFRESH_DELAY`] later.
pub fn generate(git: &Git, uncommitted: Uncommitted) {
    let mut rng = ChaCha8Rng::seed_from_u64(SEED);
    let vocabulary = vocabulary(&mut rng);
    let mut generator = Generator { rng, vocabulary, files: BTreeMap::new() };
    generator.base_files();
    generator.write_all(git);
    git.commit_all(b"Base");
    git.set_upstream_at_head();
    let mut guaranteed_changes = vec![Vec::new(); REVISIONS_COUNT];
    for change in GUARANTEED_CHANGES {
        let revision_index = generator.rng.random_range(0..REVISIONS_COUNT);
        guaranteed_changes[revision_index].push(change);
    }
    for (revision_index, guaranteed_changes) in guaranteed_changes.iter().enumerate() {
        let changes = generator.revision(guaranteed_changes);
        generator.apply(git, &changes);
        let words = generator.vocabulary.clone();
        let message = format!(
            "Revision {revision_index}: {}\n\n{}\n",
            generator.sentence(&words),
            generator.sentence(&words)
        );
        git.commit_all(message.as_bytes());
    }
    if uncommitted.has_index() {
        let changes = generator.revision(&[]);
        generator.apply(git, &changes);
        changes
            .removed_paths
            .iter()
            .chain(&changes.written_paths)
            .for_each(|path| git.stage(path));
    }
    if uncommitted.has_working_directory() {
        // Unstaged, the new file is untracked.
        let changes = generator.revision(&[GuaranteedChange::NewFile]);
        generator.apply(git, &changes);
    }
    thread::sleep(INDEX_REFRESH_DELAY);
    // `-q`: the unstaged changes don't make it fail.
    git.stdout_of(&["update-index", "-q", "--refresh"]);
}

/// [`VOCABULARY_SIZE`] distinct words made of [`SYLLABLES`], sorted.
fn vocabulary(rng: &mut ChaCha8Rng) -> Vec<String> {
    let mut words = BTreeSet::new();
    while words.len() < VOCABULARY_SIZE {
        let syllables_count = rng.random_range(WORD_SYLLABLES_COUNTS);
        let word = (0..syllables_count)
            .map(|_| *SYLLABLES.choose(rng).unwrap())
            .collect::<String>();
        words.insert(word);
    }
    words.into_iter().collect()
}

/// The files changed by a revision.
#[derive(Debug, Default)]
struct Changes {
    written_paths: BTreeSet<String>,
    removed_paths: BTreeSet<String>,
}

impl Generator {
    fn word<'w>(&mut self, words: &'w [String]) -> &'w str {
        words.choose(&mut self.rng).unwrap()
    }

    fn number(&mut self) -> u32 {
        self.rng.random_range(0..1000)
    }

    /// A count from 1 to `max`, below `2 * scale` half of the time and at least `10 * scale` a
    /// tenth of the time (a Pareto distribution): mostly small, sometimes large.
    fn skewed_count(&mut self, scale: f64, max: usize) -> usize {
        let uniform: f64 = self.rng.random();
        ((scale / (1.0 - uniform)) as usize).clamp(1, max)
    }

    fn sentence(&mut self, words: &[String]) -> String {
        let words_count = self.skewed_count(SENTENCE_WORDS_SCALE, MAX_SENTENCE_WORDS_COUNT);
        (0..words_count)
            .map(|_| self.word(words))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// A line of a file in `language` using `words`.
    fn line(&mut self, language: Language, words: &[String]) -> String {
        if self.rng.random_bool(BLANK_LINE_PROBABILITY) {
            return String::new();
        }
        let level = self.rng.random_range(0..=MAX_INDENTATION_LEVEL);
        let indentation = language.indentation(level);
        let code = self.code(language, words);
        format!("{indentation}{code}")
    }

    /// The code of a line of a file in `language` using `words`: the line without its
    /// indentation, never blank.
    fn code(&mut self, language: Language, words: &[String]) -> String {
        let (w1, w2, w3) = (self.word(words), self.word(words), self.word(words));
        let n = self.number();
        let shapes_count = 4;
        let shape_index = self.rng.random_range(0..shapes_count);
        match (language, shape_index) {
            (Language::Rust, 0) => format!("let {w1}_{n} = {w2}.{w3}({n});"),
            (Language::Rust, 1) => format!("fn {w1}_{w2}(&self, {w3}: usize) -> Option<usize> {{"),
            (Language::Rust, 2) => format!("// {}", self.sentence(words)),
            (Language::Rust, _) => format!("let {w1} = format!(\"{}\");", self.sentence(words)),
            (Language::Python, 0) => format!("{w1}_{n} = self.{w2}({w3}, {n})"),
            (Language::Python, 1) => format!("def {w1}_{w2}(self, {w3}):"),
            (Language::Python, 2) => format!("# {}", self.sentence(words)),
            (Language::Python, _) => format!("return {w1} + {n}"),
            (Language::C | Language::CHeader, 0) => format!("int {w1}_{n} = {w2}({w3}, {n});"),
            (Language::C | Language::CHeader, 1) => {
                format!("static void {w1}_{w2}(struct {w3} *{w3}) {{")
            }
            (Language::C | Language::CHeader, 2) => format!("/* {} */", self.sentence(words)),
            (Language::C | Language::CHeader, _) => format!("#define {w1}_{w2} {n}"),
            (Language::Markdown, 0) => format!("## {w1} {w2}"),
            (Language::Markdown, 1) => format!("- {}", self.sentence(words)),
            (Language::Markdown, 2) => format!("1. {}", self.sentence(words)),
            (Language::Markdown, _) => self.sentence(words),
            (Language::Toml, 0) => format!("[{w1}]"),
            (Language::Toml, 1) => format!("{w1}_{w2} = {n}"),
            (Language::Toml, _) => format!("{w1} = \"{}\"", self.sentence(words)),
            (Language::Json, 0) => format!("\"{w1}_{n}\": {n},"),
            (Language::Json, 1) => format!("\"{w1}\": [\"{w2}\", \"{w3}\"],"),
            (Language::Json, _) => format!("\"{w1}\": \"{}\",", self.sentence(words)),
            (Language::Shell, 0) => format!("{w1}_{n}=$(({n} + 1))"),
            (Language::Shell, 1) => format!("if [ -n \"${w1}\" ]; then"),
            (Language::Shell, 2) => format!("echo \"{}\"", self.sentence(words)),
            (Language::Shell, _) => format!("{w1} \"${w2}\" \"${w3}\""),
            (Language::Html, 0) => format!("<div class=\"{w1}-{w2}\">"),
            (Language::Html, 1) => format!("<p>{}</p>", self.sentence(words)),
            (Language::Html, 2) => format!("<a href=\"{w1}/{w2}.html\">{w3}</a>"),
            (Language::Html, _) => "</div>".to_string(),
            (Language::JavaScript, 0) => format!("const {w1}_{n} = {w2}.{w3}({n});"),
            (Language::JavaScript, 1) => format!("function {w1}_{w2}({w3}) {{"),
            (Language::JavaScript, 2) => format!("// {}", self.sentence(words)),
            (Language::JavaScript, _) => format!("return {w1} + {n};"),
        }
    }

    /// An embedded block of a file in `language` (see [`Language::embedded_language()`]), indented
    /// `level` levels deep.
    fn embedded_block(
        &mut self,
        language: Language,
        words: &[String],
        level: usize,
    ) -> Vec<String> {
        let embedded_language = language.embedded_language().unwrap();
        let indentation = language.indentation(level);
        let (opening, closing) = match language {
            Language::Html => (SCRIPT_OPENING.to_string(), SCRIPT_CLOSING),
            Language::Rust => (RUST_EXAMPLE_FENCE.to_string(), RUST_EXAMPLE_FENCE),
            _ => (
                format!("{DOCSTRING_QUOTES}{}", self.sentence(words)),
                DOCSTRING_QUOTES,
            ),
        };
        let code_prefix = language.embedded_code_prefix();
        let code_lines_count = self.rng.random_range(EMBEDDED_LINES_COUNTS);
        let code_lines = (0..code_lines_count)
            .map(|_| {
                let code = self.code(embedded_language, words);
                format!("{indentation}{code_prefix}{code}")
            })
            .collect::<Vec<_>>();
        [format!("{indentation}{opening}")]
            .into_iter()
            .chain(code_lines)
            .chain([format!("{indentation}{closing}")])
            .collect()
    }

    fn lines(&mut self, language: Language, words: &[String], lines_count: usize) -> Vec<String> {
        (0..lines_count)
            .map(|_| self.line(language, words))
            .collect()
    }

    /// The byte ranges of the words of the vocabulary in `text`.
    fn word_ranges(&self, text: &str) -> Vec<Range<usize>> {
        text.split(|character: char| !character.is_ascii_lowercase())
            .filter(|run| {
                self.vocabulary
                    .binary_search_by(|word| word.as_str().cmp(run))
                    .is_ok()
            })
            .map(|run| text.substr_range(run).unwrap().into())
            .collect()
    }

    /// `line` of a file in `language`, edited with `words` so that the line-diff pairs it with
    /// `line`: some of its words replaced (the more words, the less similar), a word inserted or
    /// removed, or its indentation changed. A blank line is replaced by a new line.
    fn edited(&mut self, line: &str, language: Language, words: &[String]) -> String {
        let code = line.trim_start();
        if code.is_empty() {
            return self.line(language, words);
        }
        let indentation = &line[..line.len() - code.len()];
        let word_ranges = self.word_ranges(code);
        let mut edited_code = code.to_string();
        let mut edited_indentation = indentation.to_string();
        match *LINE_EDITS.choose(&mut self.rng).unwrap() {
            LineEdit::ReplaceWords => {
                let replaced_count = self
                    .rng
                    .random_range(REPLACED_WORDS_COUNTS)
                    .min(word_ranges.len());
                let mut replaced_indexes =
                    index::sample(&mut self.rng, word_ranges.len(), replaced_count).into_vec();
                // From the end, so that the ranges of the words before stay valid.
                replaced_indexes.sort_unstable_by(|left, right| right.cmp(left));
                for word_index in replaced_indexes {
                    let replacement = self.word(words);
                    edited_code.replace_range(word_ranges[word_index].clone(), replacement);
                }
            }
            LineEdit::InsertWord => {
                let position = word_ranges
                    .choose(&mut self.rng)
                    .map_or(edited_code.len(), |range| range.start);
                let inserted = format!("{} ", self.word(words));
                edited_code.insert_str(position, &inserted);
            }
            LineEdit::RemoveWord => {
                if let Some(range) = word_ranges.choose(&mut self.rng) {
                    edited_code.replace_range(range.clone(), "");
                }
            }
            LineEdit::Reindent => {
                let level = language.indentation(1);
                edited_indentation = match indentation.strip_prefix(&level) {
                    Some(outdented) => outdented.to_string(),
                    None => format!("{level}{indentation}"),
                };
            }
        }
        let edited = format!("{edited_indentation}{edited_code}");
        if edited == line {
            // The edit changed nothing (no word to replace, no indentation in `language`).
            format!("{line} {}", self.word(words))
        } else {
            edited
        }
    }

    /// [`FILE_WORDS_COUNT`] distinct words of the vocabulary.
    fn file_words(&mut self) -> Vec<String> {
        index::sample(&mut self.rng, self.vocabulary.len(), FILE_WORDS_COUNT)
            .into_iter()
            .map(|word_index| self.vocabulary[word_index].clone())
            .collect()
    }

    /// A new text file in `language` of `lines_count` lines, plus [`EMBEDDED_BLOCKS_COUNT`]
    /// embedded blocks if `language` has them.
    fn new_text_file(&mut self, language: Language, lines_count: usize) -> Content {
        let words = self.file_words();
        let mut lines = self.lines(language, &words, lines_count);
        if language.embedded_language().is_some() {
            let mut positions =
                index::sample(&mut self.rng, lines_count + 1, EMBEDDED_BLOCKS_COUNT).into_vec();
            // From the end, so that the positions before stay valid.
            positions.sort_unstable_by(|left, right| right.cmp(left));
            for position in positions {
                let level = self.rng.random_range(0..=MAX_INDENTATION_LEVEL);
                let block = self.embedded_block(language, &words, level);
                lines.splice(position..position, block);
            }
        }
        Content::Text(TextFile { language, words, lines })
    }

    fn base_files(&mut self) {
        for (directory, language, files_count) in BASE_FILES {
            for file_index in 0..files_count {
                let lines_count = self.rng.random_range(BASE_LINES_COUNTS);
                let path = self.new_path(directory, language, file_index);
                let content = self.new_text_file(language, lines_count);
                self.files.insert(path, content);
            }
        }
        let binary = (0..BINARY_SIZE).map(|_| self.rng.random()).collect();
        self.files
            .insert(BINARY_PATH.to_string(), Content::Binary(binary));
    }

    /// A path in `directory` for a file in `language`, unused by the other files, made unique
    /// by `suffix` if the random name is used.
    fn new_path(&mut self, directory: &str, language: Language, suffix: usize) -> String {
        let extension = language.extension();
        let vocabulary = self.vocabulary.clone();
        let name = format!("{}_{}", self.word(&vocabulary), self.word(&vocabulary));
        let path = format!("{directory}/{name}.{extension}");
        if self.files.contains_key(&path) {
            format!("{directory}/{name}_{suffix}.{extension}")
        } else {
            path
        }
    }

    fn text_paths(&self) -> Vec<String> {
        self.files
            .iter()
            .filter(|(_, content)| matches!(content, Content::Text(..)))
            .map(|(path, _)| path.clone())
            .collect()
    }

    fn text_file_mut(&mut self, path: &str) -> &mut TextFile {
        match self.files.get_mut(path) {
            Some(Content::Text(text_file)) => text_file,
            _ => panic!("{path} is not a text file"),
        }
    }

    fn text_lines_mut(&mut self, path: &str) -> &mut Vec<String> {
        &mut self.text_file_mut(path).lines
    }

    fn text_file_at(&self, path: &str) -> &TextFile {
        match self.files.get(path) {
            Some(Content::Text(text_file)) => text_file,
            _ => panic!("{path} is not a text file"),
        }
    }

    fn language_of(&self, path: &str) -> Language {
        self.text_file_at(path).language
    }

    /// The text files that `changes` don't write yet, so that a change doesn't hide or undo
    /// another one of the same revision.
    fn unwritten_text_paths(&self, changes: &Changes) -> Vec<String> {
        self.text_paths()
            .into_iter()
            .filter(|path| !changes.written_paths.contains(path))
            .collect()
    }

    /// The line ranges of the embedded blocks of the file at `path` that later changes left
    /// recognizable: an opening line, at least [`EMBEDDED_LINES_COUNTS`] lines of code with the
    /// prefix, and the closing line if it follows them.
    fn embedded_blocks(&self, path: &str) -> Vec<RangeInclusive<usize>> {
        let TextFile { language, lines, .. } = self.text_file_at(path);
        lines
            .iter()
            .enumerate()
            .filter(|(_, line)| language.is_embedded_opening(line.trim_start()))
            .filter_map(|(opening_index, opening)| {
                let code_prefix = format!(
                    "{}{}",
                    indentation_of(opening),
                    language.embedded_code_prefix()
                );
                let code_lines_count = lines[opening_index + 1..]
                    .iter()
                    .take_while(|line| is_embedded_code(line, *language, &code_prefix))
                    .count();
                if code_lines_count < *EMBEDDED_LINES_COUNTS.start() {
                    return None;
                }
                let last_code_index = opening_index + code_lines_count;
                let closing_lines_count = lines
                    .get(last_code_index + 1)
                    .filter(|line| language.is_embedded_closing(line.trim_start()))
                    .map_or(0, |_| 1);
                Some(opening_index..=last_code_index + closing_lines_count)
            })
            .collect()
    }

    /// Replace a word in some of `lines` (at most [`MAX_LIGHTLY_EDITED_LINES_COUNT`], and fewer
    /// than half of the lines with words), so that the block stays recognizable. Return the
    /// number of edited lines.
    fn edit_lightly(&mut self, lines: &mut [String], words: &[String]) -> usize {
        let candidate_indexes = (0..lines.len())
            .filter(|line_index| !self.word_ranges(&lines[*line_index]).is_empty())
            .collect::<Vec<_>>();
        let max_edited_lines_count =
            MAX_LIGHTLY_EDITED_LINES_COUNT.min(candidate_indexes.len().saturating_sub(1) / 2);
        if max_edited_lines_count == 0 {
            return 0;
        }
        let edited_lines_count = self.rng.random_range(1..=max_edited_lines_count);
        let edited_indexes =
            index::sample(&mut self.rng, candidate_indexes.len(), edited_lines_count);
        for candidate_index in edited_indexes {
            let line = &mut lines[candidate_indexes[candidate_index]];
            let range = self
                .word_ranges(line)
                .choose(&mut self.rng)
                .unwrap()
                .clone();
            let replacements = words
                .iter()
                .filter(|word| **word != line[range.clone()])
                .collect::<Vec<_>>();
            line.replace_range(range, replacements.choose(&mut self.rng).unwrap());
        }
        edited_lines_count
    }

    /// The language and the words of the text file at `path`.
    fn language_and_words(&mut self, path: &str) -> (Language, Vec<String>) {
        let text_file = self.text_file_mut(path);
        (text_file.language, text_file.words.clone())
    }

    /// The removed and added lines counts of a hunk: mostly small, sometimes large. A hunk that
    /// both removes and adds lines adds at least as many as it removes.
    fn hunk_lines_counts(&mut self) -> (usize, usize) {
        let draw: f64 = self.rng.random();
        let lines_count = self.skewed_count(HUNK_LINES_SCALE, MAX_HUNK_LINES_COUNT);
        if draw < PURE_ADDITION_PROBABILITY {
            (0, lines_count)
        } else if draw < PURE_ADDITION_PROBABILITY + PURE_REMOVAL_PROBABILITY {
            (lines_count, 0)
        } else {
            let extra_lines_count = self.skewed_count(HUNK_LINES_SCALE, MAX_HUNK_LINES_COUNT) / 2;
            (lines_count, lines_count + extra_lines_count)
        }
    }

    /// Replace `removed_lines_count` lines of the file at `path` by `added_lines_count` lines at
    /// a random position, some of the added lines being edited removed lines. Return the number
    /// of changed lines.
    fn hunk(&mut self, path: &str, removed_lines_count: usize, added_lines_count: usize) -> usize {
        let (language, words) = self.language_and_words(path);
        let lines_count = self.text_lines_mut(path).len();
        let removed_lines_count = removed_lines_count.min(lines_count);
        let start = self.rng.random_range(0..=lines_count - removed_lines_count);
        let removed_lines = self
            .text_lines_mut(path)
            .drain(start..start + removed_lines_count)
            .collect::<Vec<_>>();
        let added_lines = (0..added_lines_count)
            .map(|line_index| match removed_lines.get(line_index) {
                Some(removed_line) if self.rng.random_bool(EDITED_LINE_PROBABILITY) => {
                    self.edited(removed_line, language, &words)
                }
                _ => self.line(language, &words),
            })
            .collect::<Vec<_>>();
        self.text_lines_mut(path).splice(start..start, added_lines);
        removed_lines_count + added_lines_count
    }

    /// Move a block of lines of the file at `from` to a random position of the file at `to`
    /// (possibly the same), lightly edited (see [`Generator::edit_lightly()`]) if `edit` is set.
    /// Return the number of changed lines.
    fn move_block(&mut self, from: &str, to: &str, edit: Edit, max_lines_count: usize) -> usize {
        let (_, words) = self.language_and_words(from);
        let lines_count = self.text_lines_mut(from).len();
        let moved_lines_count = self
            .rng
            .random_range(MOVED_LINES_COUNTS)
            .min(lines_count)
            .min(max_lines_count / 2);
        let start = self.rng.random_range(0..=lines_count - moved_lines_count);
        let mut block = self
            .text_lines_mut(from)
            .drain(start..start + moved_lines_count)
            .collect::<Vec<_>>();
        let edited_lines_count = match edit {
            Edit::None => 0,
            Edit::Light => self.edit_lightly(&mut block, &words),
        };
        let destination_lines_count = self.text_lines_mut(to).len();
        // Within a file, far from where the block was: `git` shows the shorter of the block and
        // of the lines it moves over as moved.
        let far_destinations = (0..=destination_lines_count)
            .filter(|destination| {
                from != to || destination.abs_diff(start) >= 2 * moved_lines_count
            })
            .collect::<Vec<_>>();
        let destination = match far_destinations.choose(&mut self.rng) {
            Some(destination) => *destination,
            None => self.rng.random_range(0..=destination_lines_count),
        };
        self.text_lines_mut(to)
            .splice(destination..destination, block);
        2 * moved_lines_count + edited_lines_count
    }

    /// Change the files for a revision, making `guaranteed_changes` and random ones, and return
    /// the paths of the files to write and to remove.
    fn revision(&mut self, guaranteed_changes: &[GuaranteedChange]) -> Changes {
        let mut changes = Changes::default();
        let mut remaining_count = self.rng.random_range(GENERATED_CHANGED_LINES_COUNTS);
        for change in guaranteed_changes {
            let changed_lines_count =
                self.guaranteed_change(*change, remaining_count, &mut changes);
            remaining_count = remaining_count.saturating_sub(changed_lines_count);
        }
        if self.rng.random_bool(FILE_CHANGE_PROBABILITY) {
            let changed_lines_count = self.delete_file(MAX_DELETED_LINES_COUNT, &mut changes);
            remaining_count = remaining_count.saturating_sub(changed_lines_count);
        }
        if self.rng.random_bool(FILE_CHANGE_PROBABILITY) {
            let changed_lines_count = self.rename_file(&mut changes);
            remaining_count = remaining_count.saturating_sub(changed_lines_count);
        }
        if self.rng.random_bool(FILE_CHANGE_PROBABILITY) {
            let changed_lines_count = self.new_file(remaining_count, &mut changes);
            remaining_count = remaining_count.saturating_sub(changed_lines_count);
        }
        self.modify_files(remaining_count, &mut changes);
        changes
    }

    /// Make `change`, changing at most about `max_lines_count` lines. Return the number of
    /// changed lines.
    fn guaranteed_change(
        &mut self,
        change: GuaranteedChange,
        max_lines_count: usize,
        changes: &mut Changes,
    ) -> usize {
        let max_moved_lines_count = *MOVED_LINES_COUNTS.end();
        match change {
            GuaranteedChange::BinaryFile => {
                let byte_index = self.rng.random_range(0..BINARY_SIZE);
                let byte = self.rng.random();
                match self.files.get_mut(BINARY_PATH) {
                    Some(Content::Binary(bytes)) => bytes[byte_index] = byte,
                    _ => panic!("{BINARY_PATH} is not a binary file"),
                }
                changes.written_paths.insert(BINARY_PATH.to_string());
                0
            }
            GuaranteedChange::NewFile => self.new_file(max_lines_count, changes),
            GuaranteedChange::DeletedFile => {
                // The smallest file, if none is small enough.
                let smallest_lines_count = self
                    .unwritten_text_paths(changes)
                    .iter()
                    .filter(|path| self.embedded_blocks(path).is_empty())
                    .map(|path| self.text_file_at(path).lines.len())
                    .min()
                    .unwrap();
                let max_lines_count = smallest_lines_count.max(MAX_DELETED_LINES_COUNT);
                self.delete_file(max_lines_count, changes)
            }
            GuaranteedChange::RenamedFile => self.rename_file(changes),
            GuaranteedChange::MoveWithinFile(edit) => {
                let unwritten_paths = self.unwritten_text_paths(changes);
                // Long enough for the block to move far.
                let long_paths = unwritten_paths
                    .iter()
                    .filter(|path| self.text_file_at(path).lines.len() >= 5 * max_moved_lines_count)
                    .collect::<Vec<_>>();
                let path = match long_paths.choose(&mut self.rng) {
                    Some(path) => path.to_string(),
                    None => unwritten_paths.choose(&mut self.rng).unwrap().clone(),
                };
                changes.written_paths.insert(path.clone());
                let max_lines_count = max_lines_count.max(2 * max_moved_lines_count);
                self.move_block(&path, &path, edit, max_lines_count)
            }
            GuaranteedChange::MoveBetweenFiles(edit) => {
                let unwritten_paths = self.unwritten_text_paths(changes);
                let path_pairs = unwritten_paths
                    .iter()
                    .flat_map(|from| unwritten_paths.iter().map(move |to| (from, to)))
                    .filter(|(from, to)| {
                        from != to
                            && self.language_of(from) == self.language_of(to)
                            && self.text_file_at(from).lines.len() >= max_moved_lines_count
                    })
                    .collect::<Vec<_>>();
                let (from, to) = path_pairs.choose(&mut self.rng).unwrap();
                let (from, to) = (from.to_string(), to.to_string());
                changes.written_paths.insert(from.clone());
                changes.written_paths.insert(to.clone());
                let max_lines_count = max_lines_count.max(2 * max_moved_lines_count);
                self.move_block(&from, &to, edit, max_lines_count)
            }
            GuaranteedChange::MoveEmbeddedCode(language, edit) => {
                self.move_embedded_code(language, edit, changes)
            }
        }
    }

    /// Add a file of a random type of at most `max_lines_count` lines (plus its embedded
    /// blocks). Return its lines count.
    fn new_file(&mut self, max_lines_count: usize, changes: &mut Changes) -> usize {
        let (directory, language, _) = *BASE_FILES.choose(&mut self.rng).unwrap();
        let lines_count = self
            .rng
            .random_range(NEW_LINES_COUNTS)
            .min(max_lines_count / 2);
        let path = self.new_path(directory, language, self.files.len());
        let content = self.new_text_file(language, lines_count);
        self.files.insert(path.clone(), content);
        changes.written_paths.insert(path.clone());
        self.text_file_at(&path).lines.len()
    }

    /// Move the code of an embedded block of a file in `language` to a dedicated file: an inline
    /// script to a JavaScript file, a doc comment example to a test function of a test file. Its
    /// lines lose the prefix of the block and change indentation, like in a real move, and are
    /// lightly edited (see [`Generator::edit_lightly()`]) if `edit` is set. Return the number of
    /// changed lines.
    fn move_embedded_code(
        &mut self,
        language: Language,
        edit: Edit,
        changes: &mut Changes,
    ) -> usize {
        let sources = self
            .text_paths()
            .into_iter()
            .filter(|path| self.language_of(path) == language)
            .flat_map(|path| {
                self.embedded_blocks(&path)
                    .into_iter()
                    .map(move |block| (path.clone(), block))
            })
            .collect::<Vec<_>>();
        let (path, block) = sources
            .choose(&mut self.rng)
            .unwrap_or_else(|| panic!("no intact embedded block in a {language:?} file"))
            .clone();
        let words = self.text_file_at(&path).words.clone();
        let removed_lines = self
            .text_lines_mut(&path)
            .drain(block.clone())
            .collect::<Vec<_>>();
        let indentation = indentation_of(&removed_lines[0]).to_string();
        let code_prefix = format!("{indentation}{}", language.embedded_code_prefix());
        let mut code_lines = removed_lines[1..]
            .iter()
            .take_while(|line| is_embedded_code(line, language, &code_prefix))
            .map(|line| line[code_prefix.len()..].to_string())
            .collect::<Vec<_>>();
        if edit == Edit::Light {
            self.edit_lightly(&mut code_lines, &words);
        }
        changes.written_paths.insert(path.clone());
        let embedded_language = language.embedded_language().unwrap();
        let (destination, added_lines) = if language == Language::Html {
            let destination = self.dedicated_file("web", embedded_language, changes);
            let name = destination.rsplit_once('/').unwrap().1;
            let script = format!("{indentation}<script src=\"{name}\"></script>");
            self.text_lines_mut(&path).insert(*block.start(), script);
            (destination, code_lines)
        } else {
            let destination = self.dedicated_file(TESTS_DIRECTORY, language, changes);
            let (w1, w2) = (self.word(&words), self.word(&words));
            let test_indentation = language.indentation(1);
            let (header, footer) = match language {
                Language::Rust => (
                    vec!["#[test]".to_string(), format!("fn {w1}_{w2}() {{")],
                    vec!["}".to_string()],
                ),
                _ => (vec![format!("def test_{w1}_{w2}():")], Vec::new()),
            };
            // A blank line separates it from the previous test function.
            let separator = if self.text_file_at(&destination).lines.is_empty() {
                None
            } else {
                Some(String::new())
            };
            let test_lines = separator
                .into_iter()
                .chain(header)
                .chain(
                    code_lines
                        .iter()
                        .map(|line| format!("{test_indentation}{line}")),
                )
                .chain(footer)
                .collect();
            (destination, test_lines)
        };
        let added_lines_count = added_lines.len();
        let destination_lines_count = self.text_lines_mut(&destination).len();
        let position = if language == Language::Html {
            self.rng.random_range(0..=destination_lines_count)
        } else {
            // A test function goes after the others.
            destination_lines_count
        };
        self.text_lines_mut(&destination)
            .splice(position..position, added_lines);
        let script_lines_count = usize::from(language == Language::Html);
        removed_lines.len() + script_lines_count + added_lines_count
    }

    /// A file in `language` of `directory`, new (empty) if there is none. A new test file of
    /// Python is named `test_*.py`, so that `pytest` finds it.
    fn dedicated_file(
        &mut self,
        directory: &str,
        language: Language,
        changes: &mut Changes,
    ) -> String {
        let directory_prefix = format!("{directory}/");
        let existing_paths = self
            .text_paths()
            .into_iter()
            .filter(|path| {
                path.starts_with(&directory_prefix) && self.language_of(path) == language
            })
            .collect::<Vec<_>>();
        let path = match existing_paths.choose(&mut self.rng) {
            Some(path) => path.clone(),
            None => {
                let path = self.new_path(directory, language, self.files.len());
                let path = if directory == TESTS_DIRECTORY && language == Language::Python {
                    path.replacen(&directory_prefix, &format!("{directory_prefix}test_"), 1)
                } else {
                    path
                };
                let words = self.file_words();
                let text_file = TextFile { language, words, lines: Vec::new() };
                self.files.insert(path.clone(), Content::Text(text_file));
                path
            }
        };
        changes.written_paths.insert(path.clone());
        path
    }

    /// Delete a text file of at most `max_lines_count` lines without embedded blocks (kept for
    /// the moves of embedded code), if there is one. Return its lines count.
    fn delete_file(&mut self, max_lines_count: usize, changes: &mut Changes) -> usize {
        let candidates = self
            .unwritten_text_paths(changes)
            .into_iter()
            .filter(|path| {
                self.text_file_at(path).lines.len() <= max_lines_count
                    && self.embedded_blocks(path).is_empty()
            })
            .collect::<Vec<_>>();
        let Some(path) = candidates.choose(&mut self.rng) else {
            return 0;
        };
        let Some(Content::Text(text_file)) = self.files.remove(path) else {
            unreachable!("the candidates are text files");
        };
        changes.removed_paths.insert(path.clone());
        text_file.lines.len()
    }

    /// Rename a text file, with a few small hunks. Return the number of changed lines.
    fn rename_file(&mut self, changes: &mut Changes) -> usize {
        let path = self
            .unwritten_text_paths(changes)
            .choose(&mut self.rng)
            .unwrap()
            .clone();
        let language = self.language_of(&path);
        let directory = path.rsplit_once('/').unwrap().0.to_string();
        let new_path = self.new_path(&directory, language, self.files.len());
        let content = self.files.remove(&path).unwrap();
        self.files.insert(new_path.clone(), content);
        let hunks_count = self.rng.random_range(1..=2);
        let changed_lines_count = (0..hunks_count)
            .map(|_| {
                let removed_lines_count = self.rng.random_range(0..=MAX_RENAMED_HUNK_LINES_COUNT);
                let added_lines_count = self.rng.random_range(1..=MAX_RENAMED_HUNK_LINES_COUNT);
                self.hunk(&new_path, removed_lines_count, added_lines_count)
            })
            .sum();
        changes.removed_paths.insert(path);
        changes.written_paths.insert(new_path);
        changed_lines_count
    }

    /// Modify text files with hunks and moved blocks, until `remaining_count` lines are changed.
    fn modify_files(&mut self, mut remaining_count: usize, changes: &mut Changes) {
        let mut candidates = self.unwritten_text_paths(changes);
        let touched_files_count = self.rng.random_range(TOUCHED_FILES_COUNTS);
        let mut hunks_counts = BTreeMap::<String, usize>::new();
        while remaining_count > 0 {
            let available = hunks_counts
                .iter()
                .filter(|(_, hunks_count)| **hunks_count < MAX_HUNKS_COUNT)
                .map(|(path, _)| path.clone())
                .collect::<Vec<_>>();
            let path = if available.is_empty() || hunks_counts.len() < touched_files_count {
                if candidates.is_empty() {
                    break;
                }
                let candidate_index = self.rng.random_range(0..candidates.len());
                candidates.swap_remove(candidate_index)
            } else {
                available.choose(&mut self.rng).unwrap().clone()
            };
            let changed_lines_count = if self.rng.random_bool(MOVE_PROBABILITY) {
                let language = self.language_of(&path);
                let destination = hunks_counts
                    .keys()
                    .filter(|destination| self.language_of(destination) == language)
                    .cloned()
                    .collect::<Vec<_>>()
                    .choose(&mut self.rng)
                    .cloned()
                    .unwrap_or_else(|| path.clone());
                let edit = *[Edit::None, Edit::Light].choose(&mut self.rng).unwrap();
                let changed_lines_count =
                    self.move_block(&path, &destination, edit, remaining_count);
                changes.written_paths.insert(destination);
                changed_lines_count
            } else {
                let (removed_lines_count, added_lines_count) = self.hunk_lines_counts();
                let removed_lines_count = removed_lines_count.min(remaining_count);
                let added_lines_count =
                    added_lines_count.min(remaining_count - removed_lines_count);
                self.hunk(&path, removed_lines_count, added_lines_count)
            };
            *hunks_counts.entry(path.clone()).or_default() += 1;
            changes.written_paths.insert(path);
            remaining_count = remaining_count.saturating_sub(changed_lines_count);
        }
    }

    fn write(&self, git: &Git, path: &str) {
        match &self.files[path] {
            Content::Text(text_file) => {
                let text = text_file
                    .lines
                    .iter()
                    .map(|line| format!("{line}\n"))
                    .collect::<String>();
                git.write(path, text.as_bytes());
            }
            Content::Binary(bytes) => git.write(path, bytes),
        }
    }

    /// Write the files that `changes` write, and remove those they remove, in the working tree.
    fn apply(&self, git: &Git, changes: &Changes) {
        changes
            .removed_paths
            .iter()
            .for_each(|path| git.remove(path));
        changes
            .written_paths
            .iter()
            .for_each(|path| self.write(git, path));
    }

    fn write_all(&self, git: &Git) {
        self.files.keys().for_each(|path| self.write(git, path));
    }
}

/// An edit of a line, so that the line-diff pairs it with the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineEdit {
    /// [`REPLACED_WORDS_COUNTS`] of its words replaced.
    ReplaceWords,
    InsertWord,
    RemoveWord,
    /// Its indentation level changed by one.
    Reindent,
}

/// The edits of a line, equally likely.
const LINE_EDITS: [LineEdit; 4] = [
    LineEdit::ReplaceWords,
    LineEdit::InsertWord,
    LineEdit::RemoveWord,
    LineEdit::Reindent,
];

/// A change made by at least one revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GuaranteedChange {
    BinaryFile,
    NewFile,
    DeletedFile,
    RenamedFile,
    MoveWithinFile(Edit),
    MoveBetweenFiles(Edit),
    /// The code of an embedded block of a file in this language moved to a dedicated file (see
    /// [`Generator::move_embedded_code()`]).
    MoveEmbeddedCode(Language, Edit),
}

/// The changes made by at least one revision each.
const GUARANTEED_CHANGES: [GuaranteedChange; 14] = [
    GuaranteedChange::BinaryFile,
    GuaranteedChange::NewFile,
    GuaranteedChange::DeletedFile,
    GuaranteedChange::RenamedFile,
    GuaranteedChange::MoveWithinFile(Edit::None),
    GuaranteedChange::MoveWithinFile(Edit::Light),
    GuaranteedChange::MoveBetweenFiles(Edit::None),
    GuaranteedChange::MoveBetweenFiles(Edit::Light),
    GuaranteedChange::MoveEmbeddedCode(Language::Html, Edit::None),
    GuaranteedChange::MoveEmbeddedCode(Language::Html, Edit::Light),
    GuaranteedChange::MoveEmbeddedCode(Language::Rust, Edit::None),
    GuaranteedChange::MoveEmbeddedCode(Language::Rust, Edit::Light),
    GuaranteedChange::MoveEmbeddedCode(Language::Python, Edit::None),
    GuaranteedChange::MoveEmbeddedCode(Language::Python, Edit::Light),
];

/// Whether `line` is a line of code of an embedded block of a file in `language`, whose lines of
/// code start with `code_prefix`.
fn is_embedded_code(line: &str, language: Language, code_prefix: &str) -> bool {
    line.starts_with(code_prefix) && !language.is_embedded_closing(line.trim_start())
}

/// The indentation of `line`.
fn indentation_of(line: &str) -> &str {
    &line[..line.len() - line.trim_start().len()]
}

/// Whether the lines of a moved block are edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edit {
    None,
    /// Some of its lines (see [`Generator::edit_lightly()`]) get a word replaced.
    Light,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stdout of `git log --reverse $format @{upstream}..`, split by revision: each revision
    /// is preceded by a line containing only `REVISION`.
    fn log_by_revision(git: &Git, args: &[&str]) -> Vec<String> {
        let args = ["log", "--reverse", "--format=tformat:REVISION"]
            .iter()
            .chain(args)
            .chain(&["@{upstream}.."])
            .copied()
            .collect::<Vec<_>>();
        git.stdout_of(&args)
            .split("REVISION\n")
            .skip(1)
            .map(str::to_string)
            .collect()
    }

    fn generated() -> Git {
        generated_with(Uncommitted::None)
    }

    fn generated_with(uncommitted: Uncommitted) -> Git {
        let git = Git::on_branch();
        generate(&git, uncommitted);
        git
    }

    /// The insertions + deletions of `git diff $args --numstat`.
    fn changed_lines_count(git: &Git, args: &[&str]) -> usize {
        let args = ["diff", "--numstat"]
            .iter()
            .chain(args)
            .copied()
            .collect::<Vec<_>>();
        git.stdout_of(&args)
            .lines()
            .filter_map(|line| {
                let mut columns = line.split('\t');
                let insertions = columns.next()?.parse::<usize>().ok()?;
                let deletions = columns.next()?.parse::<usize>().ok()?;
                Some(insertions + deletions)
            })
            .sum()
    }

    #[test]
    fn uncommitted_changes_are_staged_unstaged_and_untracked_as_asked() {
        Uncommitted::ALL.iter().for_each(|uncommitted| {
            let git = generated_with(*uncommitted);
            let staged_count = changed_lines_count(&git, &["--cached"]);
            let unstaged_count = changed_lines_count(&git, &[]);
            let untracked_paths = git.stdout_of(&["ls-files", "--others", "--exclude-standard"]);
            let expected_count = |is_expected| {
                if is_expected {
                    CHANGED_LINES_COUNTS
                } else {
                    0..=0
                }
            };
            assert!(
                expected_count(uncommitted.has_index()).contains(&staged_count),
                "{uncommitted:?}: {staged_count} staged changed lines"
            );
            assert!(
                expected_count(uncommitted.has_working_directory()).contains(&unstaged_count),
                "{uncommitted:?}: {unstaged_count} unstaged changed lines"
            );
            assert_eq!(
                !untracked_paths.is_empty(),
                uncommitted.has_working_directory(),
                "{uncommitted:?}: untracked files: {untracked_paths:?}"
            );
        });
    }

    #[test]
    fn indexed_files_are_older_than_the_index() {
        let git = generated_with(Uncommitted::Both);
        let index_seconds = std::fs::metadata(git.path().join(".git/index"))
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let entries = git.stdout_of(&["ls-files", "--debug"]);
        let mtimes_seconds = entries
            .lines()
            .filter_map(|line| line.trim_start().strip_prefix("mtime: "))
            .map(|mtime| mtime.split(':').next().unwrap().parse::<u64>().unwrap())
            .collect::<Vec<_>>();
        assert!(!mtimes_seconds.is_empty());
        assert!(
            mtimes_seconds.iter().all(|&mtime| mtime < index_seconds),
            "index at {index_seconds}, files at {mtimes_seconds:?}"
        );
    }

    #[test]
    fn each_revision_changes_about_1000_lines() {
        let git = generated();
        let numstats = log_by_revision(&git, &["--numstat", "-M"]);
        assert_eq!(numstats.len(), REVISIONS_COUNT);
        for numstat in numstats {
            let changed_lines_count = numstat
                .lines()
                .filter_map(|line| {
                    let mut columns = line.split('\t');
                    let insertions = columns.next()?.parse::<usize>().ok()?;
                    let deletions = columns.next()?.parse::<usize>().ok()?;
                    Some(insertions + deletions)
                })
                .sum::<usize>();
            assert!(
                CHANGED_LINES_COUNTS.contains(&changed_lines_count),
                "{changed_lines_count} changed lines:\n{numstat}"
            );
        }
    }

    #[test]
    fn revisions_add_delete_rename_and_change_a_binary_file() {
        let git = generated();
        let statuses = log_by_revision(&git, &["--name-status", "-M"]).concat();
        let numstats = log_by_revision(&git, &["--numstat", "-M"]).concat();
        ["A\t", "D\t", "R"].iter().for_each(|status| {
            assert!(
                statuses.lines().any(|line| line.starts_with(status)),
                "no {status:?} in:\n{statuses}"
            )
        });
        assert!(numstats.contains(&format!("-\t-\t{BINARY_PATH}")));
    }

    /// The shortest moved block searched by the tests.
    const MIN_MOVED_LINES_COUNT: usize = *MOVED_LINES_COUNTS.start();

    /// Consecutive removed, or consecutive added, lines of a file.
    struct Run {
        path: String,
        lines: Vec<String>,
    }

    /// The runs of removed lines, and the runs of added lines, of `patch` (made with
    /// `--unified=0`).
    fn runs(patch: &str) -> (Vec<Run>, Vec<Run>) {
        let (mut removed_runs, mut added_runs) = (Vec::<Run>::new(), Vec::<Run>::new());
        let (mut old_path, mut new_path) = (String::new(), String::new());
        let mut previous_kind = None;
        let mut in_hunk = false;
        for line in patch.lines() {
            if line.starts_with("diff --git") {
                in_hunk = false;
            } else if !in_hunk && let Some(path) = line.strip_prefix("--- a/") {
                old_path = path.to_string();
            } else if !in_hunk && let Some(path) = line.strip_prefix("+++ b/") {
                new_path = path.to_string();
            } else if line.starts_with("@@") {
                in_hunk = true;
                previous_kind = None;
                continue;
            }
            let kind = line.chars().next().filter(|_| in_hunk);
            let (runs, path) = match kind {
                Some('-') => (&mut removed_runs, &old_path),
                Some('+') => (&mut added_runs, &new_path),
                _ => {
                    previous_kind = None;
                    continue;
                }
            };
            if previous_kind != kind {
                runs.push(Run { path: path.clone(), lines: Vec::new() });
            }
            runs.last_mut().unwrap().lines.push(line[1..].to_string());
            previous_kind = kind;
        }
        (removed_runs, added_runs)
    }

    /// `line` without its indentation, and without the prefix of a doc comment line (`///`) or
    /// of a doctest line (`>>>`, `...`).
    fn normalized(line: &str) -> &str {
        let code = line.trim();
        ["/// ", "///", ">>> ", "... "]
            .iter()
            .find_map(|prefix| code.strip_prefix(prefix))
            .unwrap_or(code)
    }

    /// Whether `removed` and `added` differ, but share at least half of the words of the longest.
    fn lightly_edited(removed: &str, added: &str) -> bool {
        let words = |line: &str| {
            line.split(|character: char| !character.is_alphanumeric())
                .filter(|word| !word.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        };
        let (removed_words, mut added_words) = (words(removed), words(added));
        let longest_count = removed_words.len().max(added_words.len());
        let common_count = removed_words
            .iter()
            .filter(|word| {
                let position = added_words
                    .iter()
                    .position(|added_word| added_word == *word);
                position
                    .map(|position| added_words.swap_remove(position))
                    .is_some()
            })
            .count();
        removed != added && 2 * common_count >= longest_count
    }

    /// A block of lines removed from `from` and added to `to` by the same revision, in the same
    /// order.
    #[derive(Debug)]
    struct MovedBlock {
        from: String,
        to: String,
        edit: Edit,
        /// Whether some of its lines changed like in a real move (indentation, doc comment or
        /// doctest prefix), besides the edits.
        reshaped: bool,
    }

    /// The moved blocks between `removed` and `added`: at least [`MIN_MOVED_LINES_COUNT`]
    /// consecutive lines, each identical once [`normalized()`] or lightly edited, with more
    /// identical non-blank lines than edited ones (so that unrelated lines don't make a block).
    fn moved_blocks_between(removed: &Run, added: &Run) -> Vec<MovedBlock> {
        let offsets = -(removed.lines.len() as isize)..added.lines.len() as isize;
        offsets
            .flat_map(|offset| {
                let line_pairs = (0..removed.lines.len())
                    .filter_map(|removed_index| {
                        let added_index = usize::try_from(removed_index as isize + offset).ok()?;
                        Some((&removed.lines[removed_index], added.lines.get(added_index)?))
                    })
                    .collect::<Vec<_>>();
                line_pairs
                    .split(|(removed_line, added_line)| {
                        normalized(removed_line) != normalized(added_line)
                            && !lightly_edited(normalized(removed_line), normalized(added_line))
                    })
                    .filter(|segment| segment.len() >= MIN_MOVED_LINES_COUNT)
                    .filter_map(|segment| {
                        let identical_count = segment
                            .iter()
                            .filter(|(removed_line, added_line)| {
                                !removed_line.trim().is_empty()
                                    && normalized(removed_line) == normalized(added_line)
                            })
                            .count();
                        let edited_count = segment
                            .iter()
                            .filter(|(removed_line, added_line)| {
                                normalized(removed_line) != normalized(added_line)
                            })
                            .count();
                        if identical_count <= edited_count {
                            return None;
                        }
                        Some(MovedBlock {
                            from: removed.path.clone(),
                            to: added.path.clone(),
                            edit: if edited_count == 0 {
                                Edit::None
                            } else {
                                Edit::Light
                            },
                            reshaped: segment.iter().any(|(removed_line, added_line)| {
                                normalized(removed_line) == normalized(added_line)
                                    && removed_line != added_line
                            }),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// The moved blocks of all the revisions.
    fn moved_blocks(git: &Git) -> Vec<MovedBlock> {
        log_by_revision(git, &["-p", "-M", "--unified=0"])
            .iter()
            .flat_map(|patch| {
                let (removed_runs, added_runs) = runs(patch);
                removed_runs
                    .iter()
                    .flat_map(|removed| {
                        added_runs
                            .iter()
                            .flat_map(|added| moved_blocks_between(removed, added))
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// Assert that `blocks` contain, for each edit, a block matching `is_expected`.
    fn assert_moved_with_and_without_light_edits(
        blocks: &[MovedBlock],
        is_expected: impl Fn(&MovedBlock) -> bool,
    ) {
        [Edit::None, Edit::Light].iter().for_each(|edit| {
            assert!(
                blocks
                    .iter()
                    .any(|block| block.edit == *edit && is_expected(block)),
                "no such block with {edit:?} edit in:\n{blocks:#?}"
            )
        });
    }

    fn extension(path: &str) -> &str {
        path.rsplit_once('.').map_or("", |(_, extension)| extension)
    }

    #[test]
    fn revisions_move_blocks_within_a_file() {
        let blocks = moved_blocks(&generated());
        assert_moved_with_and_without_light_edits(&blocks, |block| {
            block.from == block.to && !block.reshaped
        });
    }

    #[test]
    fn revisions_move_blocks_between_files() {
        let blocks = moved_blocks(&generated());
        assert_moved_with_and_without_light_edits(&blocks, |block| {
            block.from != block.to
                && extension(&block.from) == extension(&block.to)
                && !block.reshaped
        });
    }

    #[test]
    fn revisions_move_inline_scripts_of_html_files_to_javascript_files() {
        let blocks = moved_blocks(&generated());
        assert_moved_with_and_without_light_edits(&blocks, |block| {
            extension(&block.from) == Language::Html.extension()
                && extension(&block.to) == Language::JavaScript.extension()
                && block.reshaped
        });
    }

    #[test]
    fn revisions_move_doc_comments_to_test_files() {
        let blocks = moved_blocks(&generated());
        [Language::Rust, Language::Python]
            .iter()
            .for_each(|language| {
                assert_moved_with_and_without_light_edits(&blocks, |block| {
                    extension(&block.from) == language.extension()
                        && !block.from.starts_with(TESTS_DIRECTORY)
                        && extension(&block.to) == language.extension()
                        && block.to.starts_with(TESTS_DIRECTORY)
                        && block.reshaped
                })
            });
    }

    #[test]
    fn base_contains_files_of_every_language() {
        let git = generated();
        let paths = git.stdout_of(&["ls-tree", "-r", "--name-only", "@{upstream}"]);
        BASE_FILES.iter().for_each(|(_, language, _)| {
            let extension = format!(".{}", language.extension());
            assert!(
                paths.lines().any(|path| path.ends_with(&extension)),
                "no {extension} file in:\n{paths}"
            )
        });
    }

    #[test]
    fn repository_is_the_same_each_time() {
        assert_eq!(generated().hash_of("HEAD"), generated().hash_of("HEAD"));
    }
}
