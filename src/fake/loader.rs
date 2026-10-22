//! Loads the fake commits from the plain text files of `etc/fake_commits/`, embedded in the
//! binary.

use std::collections::BTreeMap;
use std::error::Error;
use std::ffi::OsStr;
use std::fmt;

use gix::bstr::BStr;
use gix::date::Time;
use include_dir::Dir;
use include_dir::include_dir;
use serde::Deserialize;

use crate::date::DateParseError;
use crate::diff;
use crate::diff::FileState;
use crate::model::Commit;
use crate::model::Decoration;
use crate::model::FileDiff;

/// The name of the file of a fake commit holding everything but its files.
pub const METADATA_FILE: &str = "metadata.toml";

/// The format of the date of a commit, like `git log --date=iso`. Unlike the formats accepted by
/// `git commit --date`, it is always absolute, like the dates stored by git.
const DATE_FORMAT: &str = "%Y-%m-%d %H:%M:%S %z";

/// The number of hexadecimal characters of a full hash (SHA-1).
const HASH_LEN: usize = 40;

/// The attributes of `.gitattributes` that make a file binary, and the one that makes it text even
/// if it contains a NUL byte.
const BINARY_ATTRIBUTES: [&str; 2] = ["binary", "-diff"];
const TEXT_ATTRIBUTE: &str = "diff";

/// The fake commits, one folder each.
static FAKE_COMMITS: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/etc/fake_commits");

/// The number of characters of the short hash of all the fake commits.
pub const HASH_ABBREV_LEN: usize = 7;

/// Which version of a file a file of a fake commit contains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    Before,
    After,
}

/// The name of a file of a fake commit, `$id_$state_$filename`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileName {
    /// The index of the pair of files, starting from 1.
    pub id: usize,
    pub version: Version,
    /// The path of the file in the fake commit (`__` being the separator in the name).
    pub path: String,
}

/// A file of the folder of a fake commit.
#[derive(Debug, Clone, Copy)]
pub struct RawFile<'a> {
    pub name: &'a str,
    pub content: &'a [u8],
}

/// The folder of a fake commit.
#[derive(Debug, Clone)]
pub struct Folder<'a> {
    /// `${reverse_order}_${descriptive_commit_name}`
    pub name: &'a str,
    pub files: Vec<RawFile<'a>>,
}

/// The files of a fake commit are invalid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FakeDataError {
    /// The name of a folder is not `${reverse_order}_${descriptive_commit_name}`.
    InvalidFolderName(String),
    /// A file is neither the metadata nor named `$id_$state_$filename`.
    InvalidFileName(String),
    MissingMetadata,
    /// The metadata is not valid TOML, has an unknown or missing key, or an invalid value.
    InvalidMetadata(String),
    /// The date is not absolute, in the `git log --date=iso` format (`2026-10-12 14:32:07 +0200`).
    InvalidDate(DateParseError),
    /// Several files have the same `id` and the same version.
    DuplicateFile {
        id: usize,
        version: Version,
    },
    /// The ids of the files start at 1 and have no gap, but none has this id.
    MissingId(usize),
    /// A key of the `gitattributes` table of the metadata is not the name of a file.
    UnknownGitattributesFile(String),
}

impl fmt::Display for FakeDataError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            FakeDataError::InvalidFolderName(name) => write!(f, "invalid folder name: {name}"),
            FakeDataError::InvalidFileName(name) => write!(f, "invalid file name: {name}"),
            FakeDataError::MissingMetadata => write!(f, "missing {METADATA_FILE}"),
            FakeDataError::InvalidMetadata(reason) => {
                write!(f, "invalid {METADATA_FILE}: {reason}")
            }
            FakeDataError::InvalidDate(error) => write!(f, "{error}"),
            FakeDataError::DuplicateFile { id, version } => {
                write!(f, "several {version:?} files with the id {id}")
            }
            FakeDataError::MissingId(id) => write!(f, "no file with the id {id}"),
            FakeDataError::UnknownGitattributesFile(name) => {
                write!(f, "gitattributes of a file that does not exist: {name}")
            }
        }
    }
}

impl Error for FakeDataError {}

/// The content of `metadata.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    hash: String,
    author: String,
    email: String,
    date: String,
    subject: String,
    body: String,
    #[serde(default)]
    decorations: Decorations,
    /// The attributes (like in a `.gitattributes` file) of the files, by file name.
    #[serde(default)]
    gitattributes: BTreeMap<String, String>,
}

/// The refs pointing to the commit.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Decorations {
    /// The branch `HEAD` is attached to (empty if it is detached), if `HEAD` points to the commit.
    head: Option<String>,
    #[serde(default)]
    local_branches: Vec<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    remote_branches: Vec<String>,
}

impl Decorations {
    /// In the order of `git log --decorate`.
    fn into_model(self) -> Vec<Decoration> {
        let head = self.head.map(|branch| {
            Decoration::Head(if branch.is_empty() {
                None
            } else {
                Some(branch)
            })
        });
        head.into_iter()
            .chain(self.local_branches.into_iter().map(Decoration::LocalBranch))
            .chain(self.tags.into_iter().map(Decoration::Tag))
            .chain(
                self.remote_branches
                    .into_iter()
                    .map(Decoration::RemoteBranch),
            )
            .collect()
    }
}

/// The index and the description of the commit of the folder `name`.
pub fn parse_folder_name(name: &str) -> Result<(usize, &str), FakeDataError> {
    let invalid = || FakeDataError::InvalidFolderName(name.to_string());
    let (index, description) = name.split_at_checked(3).ok_or_else(invalid)?;
    let digits = index.strip_suffix('_').ok_or_else(invalid)?;
    if digits.len() != 2
        || !digits.bytes().all(|byte| byte.is_ascii_digit())
        || description.is_empty()
    {
        return Err(invalid());
    }
    Ok((digits.parse().map_err(|_| invalid())?, description))
}

/// The id, the version and the path of the file `name`.
pub fn parse_file_name(name: &str) -> Result<FileName, FakeDataError> {
    let invalid = || FakeDataError::InvalidFileName(name.to_string());
    let (id, rest) = name.split_at_checked(3).ok_or_else(invalid)?;
    let digits = id.strip_suffix('_').ok_or_else(invalid)?;
    if digits.len() != 2 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid());
    }
    let id: usize = digits.parse().map_err(|_| invalid())?;
    let (version, path) = if let Some(path) = rest.strip_prefix("before_") {
        (Version::Before, path)
    } else if let Some(path) = rest.strip_prefix("after_") {
        (Version::After, path)
    } else {
        return Err(invalid());
    };
    if id == 0 || path.is_empty() {
        return Err(invalid());
    }
    Ok(FileName {
        id,
        version,
        path: path.replace("__", "/"),
    })
}

/// `text`, an absolute date in the `git log --date=iso` format.
fn parse_date(text: &str) -> Result<Time, FakeDataError> {
    let invalid = || {
        FakeDataError::InvalidDate(DateParseError {
            input: text.to_string(),
        })
    };
    let parsed = jiff::fmt::strtime::parse(DATE_FORMAT, text).map_err(|_| invalid())?;
    let timestamp = parsed.to_timestamp().map_err(|_| invalid())?;
    let offset = parsed.offset().ok_or_else(invalid)?;
    Ok(Time::new(timestamp.as_second(), offset.seconds()))
}

/// Whether `attributes` (the attributes of a line of a `.gitattributes` file) make a file binary
/// (`Some(true)`), text (`Some(false)`), or let git decide from its content (`None`).
fn is_binary(attributes: &str) -> Option<bool> {
    let mut attributes = attributes.split_whitespace();
    if attributes
        .clone()
        .any(|attribute| BINARY_ATTRIBUTES.contains(&attribute))
    {
        Some(true)
    } else if attributes.any(|attribute| attribute == TEXT_ATTRIBUTE) {
        Some(false)
    } else {
        None
    }
}

/// The files of the folder of a fake commit with the same id: its before and after versions.
#[derive(Default)]
struct Pair<'a> {
    before: Option<RawFile<'a>>,
    after: Option<RawFile<'a>>,
    before_path: String,
    after_path: String,
}

/// The state of the version `file` of a file at `path`, if the version exists.
fn state<'a>(file: &Option<RawFile<'a>>, path: &'a str) -> Option<FileState<'a>> {
    file.map(|file| FileState {
        path: BStr::new(path),
        content: file.content,
    })
}

/// The commit made of `files`, the content of its folder.
pub fn commit(files: &[RawFile]) -> Result<Commit, FakeDataError> {
    let metadata = files
        .iter()
        .find(|file| file.name == METADATA_FILE)
        .ok_or(FakeDataError::MissingMetadata)?;
    let mut pairs: BTreeMap<usize, Pair> = BTreeMap::new();
    for file in files.iter().filter(|file| file.name != METADATA_FILE) {
        let name = parse_file_name(file.name)?;
        let pair = pairs.entry(name.id).or_default();
        let (slot, path) = match name.version {
            Version::Before => (&mut pair.before, &mut pair.before_path),
            Version::After => (&mut pair.after, &mut pair.after_path),
        };
        if slot.replace(*file).is_some() {
            return Err(FakeDataError::DuplicateFile {
                id: name.id,
                version: name.version,
            });
        }
        *path = name.path;
    }
    let last_id = pairs.keys().next_back().copied().unwrap_or(0);
    if let Some(missing) = (1..=last_id).find(|id| !pairs.contains_key(id)) {
        return Err(FakeDataError::MissingId(missing));
    }

    let invalid = |reason: String| FakeDataError::InvalidMetadata(reason);
    let text = std::str::from_utf8(metadata.content).map_err(|error| invalid(error.to_string()))?;
    let metadata: Metadata = toml::from_str(text).map_err(|error| invalid(error.to_string()))?;
    if metadata.hash.len() != HASH_LEN
        || !metadata.hash.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(invalid(format!(
            "the hash must have {HASH_LEN} hexadecimal characters"
        )));
    }
    let date = parse_date(&metadata.date)?;
    if let Some(unknown) = metadata
        .gitattributes
        .keys()
        .find(|name| !files.iter().any(|file| file.name == name.as_str()))
    {
        return Err(FakeDataError::UnknownGitattributesFile(unknown.clone()));
    }

    let attributes = |file: &Option<RawFile>| {
        file.and_then(|file| metadata.gitattributes.get(file.name))
            .and_then(|attributes| is_binary(attributes))
    };
    let files: Vec<FileDiff> = pairs
        .values()
        .map(|pair| {
            let has_nul =
                |file: &Option<RawFile>| file.is_some_and(|file| file.content.contains(&0));
            // Like git: the attributes decide, otherwise a NUL byte makes a file binary.
            let declared = [attributes(&pair.before), attributes(&pair.after)];
            let binary = if declared.contains(&Some(true)) {
                true
            } else if declared.contains(&Some(false)) {
                false
            } else {
                has_nul(&pair.before) || has_nul(&pair.after)
            };
            diff::file(
                state(&pair.before, &pair.before_path),
                state(&pair.after, &pair.after_path),
                binary,
                diff::CONTEXT,
            )
        })
        .collect();
    Ok(Commit {
        hash: metadata.hash,
        hash_abbrev_len: HASH_ABBREV_LEN,
        subject: metadata.subject,
        author: metadata.author,
        email: metadata.email,
        date,
        decorations: metadata.decorations.into_model(),
        body: metadata.body,
        files,
    })
}

/// The folders of the fake commits embedded in the binary, sorted by name (so by index).
pub fn embedded() -> Vec<Folder<'static>> {
    let name = |path: &'static std::path::Path| {
        path.file_name()
            .and_then(OsStr::to_str)
            .expect("the names of the fake data are valid UTF-8")
    };
    let mut folders: Vec<_> = FAKE_COMMITS
        .dirs()
        .map(|dir| Folder {
            name: name(dir.path()),
            files: dir
                .files()
                .map(|file| RawFile {
                    name: name(file.path()),
                    content: file.contents(),
                })
                .collect(),
        })
        .collect();
    folders.sort_by_key(|folder| folder.name);
    folders
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Decoration;
    use crate::model::FileChange;
    use crate::model::FileContentType;
    use gix::date::Time;

    const HASH: &str = "0123456789abcdef0123456789abcdef01234567";

    /// The metadata of a valid commit, followed by `extra`.
    fn metadata(extra: &str) -> String {
        format!(
            "# A comment about this fake commit.\n\
             hash = \"{HASH}\"\n\
             author = \"Alice Martin\"\n\
             email = \"alice.martin@example.org\"\n\
             date = \"2026-10-12 14:32:07 +0200\"\n\
             subject = \"config: rename\"\n\
             body = \"Line 1.\\nLine 2.\"\n\
             {extra}"
        )
    }

    /// The commit of the folder made of the metadata `metadata` and the files `(name, content)`.
    fn load(metadata: &str, files: &[(&str, &[u8])]) -> Result<Commit, FakeDataError> {
        let metadata = RawFile {
            name: METADATA_FILE,
            content: metadata.as_bytes(),
        };
        let files: Vec<_> = std::iter::once(metadata)
            .chain(
                files
                    .iter()
                    .map(|(name, content)| RawFile { name, content }),
            )
            .collect();
        commit(&files)
    }

    #[test]
    fn folder_name_is_the_index_then_the_description() {
        assert_eq!(
            parse_folder_name("00_scrolloff_rename"),
            Ok((0, "scrolloff_rename"))
        );
        assert_eq!(
            parse_folder_name("12_file_changes"),
            Ok((12, "file_changes"))
        );
    }

    #[test]
    fn invalid_folder_names_are_errors() {
        ["", "00", "00_", "_x", "0_x", "000_x", "ab_x", "00-x"]
            .into_iter()
            .for_each(|name| {
                assert_eq!(
                    parse_folder_name(name),
                    Err(FakeDataError::InvalidFolderName(name.to_string())),
                    "{name:?}"
                );
            });
    }

    #[test]
    fn file_name_is_the_id_then_the_version_then_the_path() {
        let name = |id, version, path: &str| FileName {
            id,
            version,
            path: path.to_string(),
        };
        assert_eq!(
            parse_file_name("01_before_src__main.rs"),
            Ok(name(1, Version::Before, "src/main.rs"))
        );
        assert_eq!(
            parse_file_name("12_after_my_file.rs"),
            Ok(name(12, Version::After, "my_file.rs"))
        );
    }

    #[test]
    fn invalid_file_names_are_errors() {
        let invalid = [
            "",
            "a.txt",
            "1_before_a",
            "00_before_a",
            "01_middle_a",
            "01_before_",
            "01_before",
            "ab_after_a",
        ];
        invalid.into_iter().for_each(|name| {
            assert_eq!(
                parse_file_name(name),
                Err(FakeDataError::InvalidFileName(name.to_string())),
                "{name:?}"
            );
        });
    }

    #[test]
    fn metadata_is_the_information_of_the_commit() {
        let extra = "[decorations]\n\
                     head = \"main\"\n\
                     local_branches = [\"dev\"]\n\
                     tags = [\"v1.0\"]\n\
                     remote_branches = [\"origin/main\"]\n";
        let commit = load(&metadata(extra), &[]).unwrap();
        assert_eq!(commit.hash, HASH);
        assert_eq!(commit.hash_abbrev_len, HASH_ABBREV_LEN);
        assert_eq!(commit.author, "Alice Martin");
        assert_eq!(commit.email, "alice.martin@example.org");
        assert_eq!(commit.subject, "config: rename");
        assert_eq!(commit.body, "Line 1.\nLine 2.");
        assert_eq!(commit.date, Time::new(1_791_808_327, 7200));
        // In the order of `git log --decorate`.
        assert_eq!(
            commit.decorations,
            [
                Decoration::Head(Some("main".to_string())),
                Decoration::LocalBranch("dev".to_string()),
                Decoration::Tag("v1.0".to_string()),
                Decoration::RemoteBranch("origin/main".to_string()),
            ]
        );
        assert!(commit.files.is_empty());
    }

    #[test]
    fn decorations_are_optional_and_head_can_be_detached() {
        assert!(load(&metadata(""), &[]).unwrap().decorations.is_empty());
        let detached = load(&metadata("[decorations]\nhead = \"\"\n"), &[]).unwrap();
        assert_eq!(detached.decorations, [Decoration::Head(None)]);
    }

    #[test]
    fn date_is_absolute_with_its_own_offset() {
        let date = |date: &str| metadata("").replace("2026-10-12 14:32:07 +0200", date);
        assert_eq!(
            load(&date("2026-10-12 14:32:07 -0400"), &[]).unwrap().date,
            Time::new(1_791_829_927, -4 * 3600)
        );
    }

    /// Only the dates that git stores in a commit are accepted: the ones with an offset.
    #[test]
    fn dates_that_are_not_absolute_are_errors() {
        let invalid = [
            "2026-10-12 14:32:07",
            "2026-10-12T14:32:07+02:00",
            "1791808327 +0200",
            "3 hours ago",
            "yesterday",
            "not a date",
        ];
        invalid.into_iter().for_each(|date| {
            let metadata = metadata("").replace("2026-10-12 14:32:07 +0200", date);
            assert_eq!(
                load(&metadata, &[]).unwrap_err(),
                FakeDataError::InvalidDate(DateParseError {
                    input: date.to_string()
                }),
                "{date:?}"
            );
        });
    }

    #[test]
    fn invalid_metadata_is_an_error() {
        let invalid = [
            metadata("colour = \"red\"\n"),
            metadata("").replace("hash = ", "unknown_hash = "),
            metadata("").replace(HASH, "abc123"),
            "this is not toml".to_string(),
        ];
        invalid.into_iter().for_each(|metadata| {
            let error = load(&metadata, &[]).unwrap_err();
            assert!(
                matches!(error, FakeDataError::InvalidMetadata(_)),
                "{error:?}"
            );
        });
    }

    #[test]
    fn missing_metadata_is_an_error() {
        let files = [RawFile {
            name: "01_after_a.txt",
            content: b"a\n",
        }];
        assert_eq!(commit(&files).unwrap_err(), FakeDataError::MissingMetadata);
    }

    #[test]
    fn files_are_paired_and_displayed_in_the_order_of_their_id() {
        let files: &[(&str, &[u8])] = &[
            ("04_after_new.rs", b"fn main() {}\n"),
            ("02_after_added.txt", b"added\n"),
            ("03_before_deleted.txt", b"deleted\n"),
            ("01_after_src__a.rs", b"after\n"),
            ("04_before_old.rs", b"fn main() {}\n"),
            ("01_before_src__a.rs", b"before\n"),
        ];
        let commit = load(&metadata(""), files).unwrap();
        let files: Vec<_> = commit
            .files
            .iter()
            .map(|file| (file.path.to_string(), file.change.clone()))
            .collect();
        assert_eq!(
            files,
            [
                ("src/a.rs".to_string(), FileChange::Modified),
                ("added.txt".to_string(), FileChange::Added),
                ("deleted.txt".to_string(), FileChange::Deleted),
                (
                    "new.rs".to_string(),
                    FileChange::Renamed {
                        from: "old.rs".into()
                    }
                ),
            ]
        );
    }

    #[test]
    fn the_bytes_of_the_files_are_used_as_they_are() {
        let files: &[(&str, &[u8])] = &[
            ("01_before_a.txt", b"one\r\ntwo\r\n"),
            ("01_after_a.txt", b"one\r\ntwo"),
        ];
        let commit = load(&metadata(""), files).unwrap();
        let lines = commit.files[0].chunks.iter().flat_map(|chunk| &chunk.lines);
        assert!(lines.clone().any(|line| line.text.ends_with('\r')));
        assert!(lines.clone().any(|line| line.missing_newline));
    }

    #[test]
    fn files_with_the_same_id_and_version_are_an_error() {
        let files: &[(&str, &[u8])] = &[("01_before_a.txt", b"a\n"), ("01_before_b.txt", b"b\n")];
        assert_eq!(
            load(&metadata(""), files).unwrap_err(),
            FakeDataError::DuplicateFile {
                id: 1,
                version: Version::Before
            }
        );
    }

    #[test]
    fn ids_start_at_1_and_have_no_gap() {
        let gap: &[(&str, &[u8])] = &[("01_after_a.txt", b"a\n"), ("03_after_b.txt", b"b\n")];
        assert_eq!(
            load(&metadata(""), gap).unwrap_err(),
            FakeDataError::MissingId(2)
        );
        let start: &[(&str, &[u8])] = &[("02_after_a.txt", b"a\n")];
        assert_eq!(
            load(&metadata(""), start).unwrap_err(),
            FakeDataError::MissingId(1)
        );
    }

    #[test]
    fn unknown_files_are_an_error() {
        let files: &[(&str, &[u8])] = &[("notes.txt", b"a\n")];
        assert_eq!(
            load(&metadata(""), files).unwrap_err(),
            FakeDataError::InvalidFileName("notes.txt".to_string())
        );
    }

    #[test]
    fn gitattributes_tell_which_files_are_binary() {
        let files: &[(&str, &[u8])] = &[
            ("01_before_a.dat", b"before\n"),
            ("01_after_a.dat", b"after\n"),
            ("02_before_b.dat", b"before\n"),
            ("02_after_b.dat", b"after\n"),
            ("03_before_c.txt", b"before\n"),
            ("03_after_c.txt", b"after\n"),
        ];
        // The attributes of a single version are enough, and are read like `.gitattributes` does.
        let extra = "[gitattributes]\n\
                     \"01_after_a.dat\" = \"binary\"\n\
                     \"02_before_b.dat\" = \"-text -diff\"\n\
                     \"03_after_c.txt\" = \"text eol=lf\"\n";
        let commit = load(&metadata(extra), files).unwrap();
        let contents: Vec<_> = commit.files.iter().map(|file| file.content).collect();
        assert_eq!(
            contents,
            [
                FileContentType::Binary,
                FileContentType::Binary,
                FileContentType::Text
            ]
        );
        assert!(commit.files[0].chunks.is_empty());
    }

    #[test]
    fn files_with_a_nul_byte_are_binary_without_gitattributes() {
        let files: &[(&str, &[u8])] = &[("01_after_a.dat", b"a\0b\n")];
        let commit = load(&metadata(""), files).unwrap();
        assert_eq!(commit.files[0].content, FileContentType::Binary);
    }

    #[test]
    fn diff_attribute_makes_a_file_with_a_nul_byte_text() {
        let files: &[(&str, &[u8])] = &[("01_after_a.txt", b"a\0b\n")];
        let extra = "[gitattributes]\n\"01_after_a.txt\" = \"diff\"\n";
        let commit = load(&metadata(extra), files).unwrap();
        assert_eq!(commit.files[0].content, FileContentType::Text);
    }

    #[test]
    fn files_that_are_not_utf8_are_reported_as_such() {
        let files: &[(&str, &[u8])] = &[("01_after_a.txt", b"caf\xe9\n")];
        let commit = load(&metadata(""), files).unwrap();
        assert_eq!(commit.files[0].content, FileContentType::InvalidUtf8);
    }

    #[test]
    fn gitattributes_of_unknown_files_are_an_error() {
        let files: &[(&str, &[u8])] = &[("01_after_a.txt", b"a\n")];
        let extra = "[gitattributes]\n\"01_after_b.txt\" = \"binary\"\n";
        assert_eq!(
            load(&metadata(extra), files).unwrap_err(),
            FakeDataError::UnknownGitattributesFile("01_after_b.txt".to_string())
        );
    }
}
