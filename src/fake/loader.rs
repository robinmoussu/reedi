//! Loads the fake commits, index and working directory from the plain text files of
//! `etc/fake_commits/`, embedded in the binary.

use std::collections::BTreeMap;
use std::error::Error;
use std::ffi::OsStr;
use std::fmt;

use gix::ObjectId;
use gix::bstr::BStr;
use gix::date::Time;
use gix::refs::Category;
use include_dir::Dir;
use include_dir::include_dir;
use reediif::diff;
use reediif::diff::DiffAs;
use reediif::diff::FileState;
use reediif::model::FileDiff;
use reediif::model::LineKind;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::date::DateParseError;
use crate::model::CONFLICT_MARKER;
use crate::model::Commit;
use crate::model::Decoration;
use crate::model::Index;
use crate::model::NextCommit;
use crate::model::WorkingDirectory;
use crate::settings::CONTEXT;

/// The name of the file of a fake commit holding everything but its files.
pub const METADATA_FILE: &str = "metadata.toml";

/// The folder of the fake index.
pub const INDEX_FOLDER: &str = "index";
/// The folder of the fake working directory.
pub const WORKING_DIRECTORY_FOLDER: &str = "working_directory";

/// The format of the date of a commit, like `git log --date=iso`. Unlike the formats accepted by
/// `git commit --date`, it is always absolute, like the dates stored by git.
const DATE_FORMAT: &str = "%Y-%m-%d %H:%M:%S %z";

/// The number of digits of the index starting the names of the folders and files of the fake
/// commits (`01_`).
const INDEX_DIGITS_COUNT: usize = 2;

/// The attributes of `.gitattributes` that make a file binary.
const BINARY_ATTRIBUTES: [&str; 2] = ["binary", "-diff"];
/// The attribute of `.gitattributes` that makes a file text, even if it contains a NUL byte.
const TEXT_ATTRIBUTE: &str = "diff";

/// The fake data: a folder per fake commit, and the folders of the index and of the working
/// directory.
static FAKE_DATA: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/etc/fake_commits");

/// The number of characters of the short hash of all the fake commits.
pub const HASH_ABBREV_LEN: usize = 7;

/// Which version of a file a file of a fake commit contains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    Before,
    After,
}

/// The name of a file of a fake commit, `$id_$version_$path`.
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
    /// `$index_$description`
    pub name: &'a str,
    pub files: Vec<RawFile<'a>>,
}

/// The files of a fake commit are invalid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FakeDataError {
    /// The name of a folder is not `$index_$description`.
    #[cfg(any(test, feature = "test-support"))]
    InvalidFolderName(String),
    /// A file is neither the metadata nor named `$id_$version_$path`.
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
    /// The folder of the working directory has a metadata file.
    UnexpectedMetadata,
}

impl fmt::Display for FakeDataError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            #[cfg(any(test, feature = "test-support"))]
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
            FakeDataError::UnexpectedMetadata => write!(f, "unexpected {METADATA_FILE}"),
        }
    }
}

impl Error for FakeDataError {}

/// The content of `metadata.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    hash: String,
    /// The hashes of the parents, in order.
    parents: Vec<String>,
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

/// The content of the `metadata.toml` of the index.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexMetadata {
    /// The branch HEAD is attached to (`main`), missing if HEAD is detached.
    branch: Option<String>,
    subject: String,
    #[serde(default)]
    body: String,
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
    /// Convert to the decorations of the model, in the order of `git log --decorate`.
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

/// The index starting `name` (`INDEX_DIGITS_COUNT` digits followed by `_`), and the rest of
/// `name`.
fn split_index(name: &str) -> Option<(usize, &str)> {
    let digits = name.get(..INDEX_DIGITS_COUNT)?;
    let rest = name.get(INDEX_DIGITS_COUNT..)?.strip_prefix('_')?;
    if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some((digits.parse().ok()?, rest))
}

/// The index and the description of the commit of the folder `name`.
#[cfg(any(test, feature = "test-support"))]
pub fn parse_folder_name(name: &str) -> Result<(usize, &str), FakeDataError> {
    match split_index(name) {
        Some((index, description)) if !description.is_empty() => Ok((index, description)),
        _ => Err(FakeDataError::InvalidFolderName(name.to_string())),
    }
}

/// The id, the version and the path of the file `name`.
pub fn parse_file_name(name: &str) -> Result<FileName, FakeDataError> {
    let invalid = || FakeDataError::InvalidFileName(name.to_string());
    let (id, rest) = split_index(name).ok_or_else(invalid)?;
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
    let invalid = || FakeDataError::InvalidDate(DateParseError { input: text.to_string() });
    let parsed = jiff::fmt::strtime::parse(DATE_FORMAT, text).map_err(|_| invalid())?;
    let timestamp = parsed.to_timestamp().map_err(|_| invalid())?;
    let offset = parsed.offset().ok_or_else(invalid)?;
    Ok(Time::new(timestamp.as_second(), offset.seconds()))
}

/// Whether `attributes` (the attributes of a line of a `.gitattributes` file) make a file binary,
/// text, or let git decide from its content ([`DiffAs::Auto`]).
fn diff_as(attributes: &str) -> DiffAs {
    let mut attributes = attributes.split_whitespace();
    if attributes
        .clone()
        .any(|attribute| BINARY_ATTRIBUTES.contains(&attribute))
    {
        DiffAs::Binary
    } else if attributes.any(|attribute| attribute == TEXT_ATTRIBUTE) {
        DiffAs::Text
    } else {
        DiffAs::Auto
    }
}

/// The files of the folder of a fake commit with the same id: its before and after versions.
#[derive(Default)]
struct Versions<'a> {
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

/// The metadata of the folder made of `files`.
fn metadata<T: DeserializeOwned>(files: &[RawFile]) -> Result<T, FakeDataError> {
    let metadata = files
        .iter()
        .find(|file| file.name == METADATA_FILE)
        .ok_or(FakeDataError::MissingMetadata)?;
    let invalid = |reason: String| FakeDataError::InvalidMetadata(reason);
    let text = std::str::from_utf8(metadata.content).map_err(|error| invalid(error.to_string()))?;
    toml::from_str(text).map_err(|error| invalid(error.to_string()))
}

/// The commit hash `hash`, in hexadecimal.
fn object_id(hash: &str) -> Result<ObjectId, FakeDataError> {
    ObjectId::from_hex(hash.as_bytes())
        .map_err(|error| FakeDataError::InvalidMetadata(error.to_string()))
}

/// The diffs of the files of the folder made of `files` (but its metadata), paired by id and
/// sorted by path like git, diffed according to `gitattributes` (the attributes of the files, by
/// file name).
fn file_diffs(
    files: &[RawFile],
    gitattributes: &BTreeMap<String, String>,
) -> Result<Vec<FileDiff>, FakeDataError> {
    let mut versions_by_id: BTreeMap<usize, Versions> = BTreeMap::new();
    for file in files.iter().filter(|file| file.name != METADATA_FILE) {
        let name = parse_file_name(file.name)?;
        let versions = versions_by_id.entry(name.id).or_default();
        let (slot, path) = match name.version {
            Version::Before => (&mut versions.before, &mut versions.before_path),
            Version::After => (&mut versions.after, &mut versions.after_path),
        };
        if slot.replace(*file).is_some() {
            return Err(FakeDataError::DuplicateFile { id: name.id, version: name.version });
        }
        *path = name.path;
    }
    let last_id = versions_by_id.keys().next_back().copied().unwrap_or(0);
    if let Some(missing) = (1..=last_id).find(|id| !versions_by_id.contains_key(id)) {
        return Err(FakeDataError::MissingId(missing));
    }
    if let Some(unknown) = gitattributes
        .keys()
        .find(|name| !files.iter().any(|file| file.name == name.as_str()))
    {
        return Err(FakeDataError::UnknownGitattributesFile(unknown.clone()));
    }

    let diff_as_of = |file: &Option<RawFile>| {
        file.and_then(|file| gitattributes.get(file.name))
            .map_or(DiffAs::Auto, |attributes| diff_as(attributes))
    };
    let mut files: Vec<FileDiff> = versions_by_id
        .values()
        .map(|versions| {
            let diff_as =
                DiffAs::of_versions(diff_as_of(&versions.before), diff_as_of(&versions.after));
            diff::file(
                /* before */ state(&versions.before, &versions.before_path),
                /* after */ state(&versions.after, &versions.after_path),
                diff_as,
                CONTEXT,
            )
        })
        .collect();
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

/// The commit made of `files`, the content of its folder.
pub fn commit(files: &[RawFile]) -> Result<Commit, FakeDataError> {
    let metadata: Metadata = metadata(files)?;
    let hash = object_id(&metadata.hash)?;
    let parent_ids = metadata
        .parents
        .iter()
        .map(|parent| object_id(parent))
        .collect::<Result<_, _>>()?;
    let date = parse_date(&metadata.date)?;
    Ok(Commit {
        hash,
        hash_abbrev_len: HASH_ABBREV_LEN,
        subject: metadata.subject,
        author: metadata.author,
        email: metadata.email,
        date,
        decorations: metadata.decorations.into_model(),
        body: metadata.body,
        parent_ids,
        files: file_diffs(files, &metadata.gitattributes)?,
    })
}

/// Whether `file` adds a conflict marker: its after version has a `<<<<<<<` line that its before
/// version doesn't have.
fn adds_a_conflict_marker(file: &FileDiff) -> bool {
    file.hunks
        .iter()
        .flat_map(|hunk| &hunk.lines)
        .any(|line| line.kind == LineKind::Added && line.text.starts_with(CONFLICT_MARKER))
}

/// The index and the working directory made of `index_files` and `working_directory_files`, the
/// content of their folders.
///
/// The files of the working directory that add a conflict marker are unmerged in the index.
pub fn index_and_working_directory(
    index_files: &[RawFile],
    working_directory_files: &[RawFile],
) -> Result<(Index, WorkingDirectory), FakeDataError> {
    let metadata: IndexMetadata = metadata(index_files)?;
    if working_directory_files
        .iter()
        .any(|file| file.name == METADATA_FILE)
    {
        return Err(FakeDataError::UnexpectedMetadata);
    }
    let next_commit = metadata
        .branch
        .map(|branch| {
            Category::LocalBranch
                .to_full_name(branch.as_str())
                .map_err(|error| FakeDataError::InvalidMetadata(error.to_string()))
        })
        .transpose()?
        .map_or(NextCommit::DetachedHead, NextCommit::Branch);
    let working_directory = WorkingDirectory {
        files: file_diffs(working_directory_files, &BTreeMap::new())?,
    };
    let mut files = file_diffs(index_files, &BTreeMap::new())?;
    files.extend(
        working_directory
            .files
            .iter()
            .filter(|file| adds_a_conflict_marker(file))
            .map(|file| diff::unmerged(file.path.clone())),
    );
    files.sort_by(|left, right| left.path.cmp(&right.path));
    let index = Index {
        next_commit,
        subject: metadata.subject,
        body: metadata.body,
        files,
    };
    Ok((index, working_directory))
}

/// The folders of the fake commits (`$index_$description`) embedded in the binary, sorted by name
/// (so by index).
pub fn commit_folders() -> Vec<Folder<'static>> {
    embedded()
        .into_iter()
        .filter(|folder| split_index(folder.name).is_some())
        .collect()
}

/// The folder `name` embedded in the binary.
pub fn folder(name: &str) -> Folder<'static> {
    embedded()
        .into_iter()
        .find(|folder| folder.name == name)
        .unwrap_or_else(|| panic!("no folder {name} in the fake data"))
}

/// The folders of the fake data embedded in the binary, sorted by name.
pub fn embedded() -> Vec<Folder<'static>> {
    let name = |path: &'static std::path::Path| {
        path.file_name()
            .and_then(OsStr::to_str)
            .expect("the names of the fake data are valid UTF-8")
    };
    let mut folders: Vec<_> = FAKE_DATA
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
    use crate::test_support::file_at;
    use crate::test_support::single;
    use gix::date::Time;
    use reediif::model::FileChange;
    use reediif::model::FileContentType;

    const HASH: &str = "0123456789abcdef0123456789abcdef01234567";
    const PARENT_HASH: &str = "89abcdef0123456789abcdef0123456789abcdef";

    /// The date of `metadata()`, which the tests of the dates replace.
    const DEFAULT_DATE: &str = "2026-10-12 14:32:07 +0200";

    /// The metadata of a valid commit, followed by `extra`.
    fn metadata(extra: &str) -> String {
        format!(
            "# A comment about this fake commit.\n\
             hash = \"{HASH}\"\n\
             parents = [\"{PARENT_HASH}\"]\n\
             author = \"Alice Martin\"\n\
             email = \"alice.martin@example.org\"\n\
             date = \"{DEFAULT_DATE}\"\n\
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
        for name in ["", "00", "00_", "_x", "0_x", "000_x", "ab_x", "00-x"] {
            assert_eq!(
                parse_folder_name(name),
                Err(FakeDataError::InvalidFolderName(name.to_string())),
                "{name:?}"
            );
        }
    }

    #[test]
    fn file_name_is_the_id_then_the_version_then_the_path() {
        let name = |id, version, path: &str| FileName { id, version, path: path.to_string() };
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
        for name in invalid {
            assert_eq!(
                parse_file_name(name),
                Err(FakeDataError::InvalidFileName(name.to_string())),
                "{name:?}"
            );
        }
    }

    #[test]
    fn metadata_is_the_information_of_the_commit() {
        let extra = "[decorations]\n\
                     head = \"main\"\n\
                     local_branches = [\"dev\"]\n\
                     tags = [\"v1.0\"]\n\
                     remote_branches = [\"origin/main\"]\n";
        let commit = load(&metadata(extra), &[]).unwrap();
        assert_eq!(commit.hash.to_string(), HASH);
        assert_eq!(commit.short_hash().to_string(), &HASH[..HASH_ABBREV_LEN]);
        assert_eq!(commit.author, "Alice Martin");
        assert_eq!(commit.email, "alice.martin@example.org");
        assert_eq!(commit.subject, "config: rename");
        assert_eq!(commit.body, "Line 1.\nLine 2.");
        assert_eq!(commit.date, Time::new(1_791_808_327, 7200));
        assert_eq!(
            commit.parent_ids,
            [ObjectId::from_hex(PARENT_HASH.as_bytes()).unwrap()]
        );
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
        let date = |date: &str| metadata("").replace(DEFAULT_DATE, date);
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
        for date in invalid {
            let metadata = metadata("").replace(DEFAULT_DATE, date);
            assert_eq!(
                load(&metadata, &[]).unwrap_err(),
                FakeDataError::InvalidDate(DateParseError { input: date.to_string() }),
                "{date:?}"
            );
        }
    }

    #[test]
    fn invalid_metadata_is_an_error() {
        let invalid = [
            metadata("colour = \"red\"\n"),
            metadata("").replace("hash = ", "unknown_hash = "),
            metadata("").replace(HASH, "abc123"),
            "this is not toml".to_string(),
        ];
        for metadata in invalid {
            let error = load(&metadata, &[]).unwrap_err();
            assert!(
                matches!(error, FakeDataError::InvalidMetadata(_)),
                "{error:?}"
            );
        }
    }

    #[test]
    fn missing_metadata_is_an_error() {
        let files = [RawFile { name: "01_after_a.txt", content: b"a\n" }];
        assert_eq!(commit(&files).unwrap_err(), FakeDataError::MissingMetadata);
    }

    /// The order of the ids differs from the order of the paths.
    #[test]
    fn files_are_paired_by_id_and_sorted_by_path_like_git() {
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
        // A renamed file is sorted by its new path, like in `git log -p`.
        assert_eq!(
            files,
            [
                ("added.txt".to_string(), FileChange::Added),
                ("deleted.txt".to_string(), FileChange::Deleted),
                (
                    "new.rs".to_string(),
                    FileChange::Renamed { from: "old.rs".into() }
                ),
                ("src/a.rs".to_string(), FileChange::Modified),
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
        let lines = single(&commit.files)
            .hunks
            .iter()
            .flat_map(|hunk| &hunk.lines);
        assert!(lines.clone().any(|line| line.text.ends_with('\r')));
        assert!(lines.clone().any(|line| line.missing_newline));
    }

    #[test]
    fn files_with_the_same_id_and_version_are_an_error() {
        let files: &[(&str, &[u8])] = &[("01_before_a.txt", b"a\n"), ("01_before_b.txt", b"b\n")];
        assert_eq!(
            load(&metadata(""), files).unwrap_err(),
            FakeDataError::DuplicateFile { id: 1, version: Version::Before }
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
        // The attributes of a single version are enough, and are read like git reads
        // `.gitattributes`.
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
        assert!(file_at(&commit, "a.dat").hunks.is_empty());
    }

    #[test]
    fn files_with_a_nul_byte_are_binary_without_gitattributes() {
        let files: &[(&str, &[u8])] = &[("01_after_a.dat", b"a\0b\n")];
        let commit = load(&metadata(""), files).unwrap();
        assert_eq!(single(&commit.files).content, FileContentType::Binary);
    }

    #[test]
    fn diff_attribute_makes_a_file_with_a_nul_byte_text() {
        let files: &[(&str, &[u8])] = &[("01_after_a.txt", b"a\0b\n")];
        let extra = "[gitattributes]\n\"01_after_a.txt\" = \"diff\"\n";
        let commit = load(&metadata(extra), files).unwrap();
        assert_eq!(single(&commit.files).content, FileContentType::Text);
    }

    #[test]
    fn files_that_are_not_utf8_are_reported_as_such() {
        let files: &[(&str, &[u8])] = &[("01_after_a.txt", b"caf\xe9\n")];
        let commit = load(&metadata(""), files).unwrap();
        assert_eq!(single(&commit.files).content, FileContentType::InvalidUtf8);
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

    /// The metadata of an index on `main`.
    const INDEX_METADATA: RawFile = RawFile {
        name: METADATA_FILE,
        content: b"branch = \"main\"\nsubject = \"Subject\"\nbody = \"Body.\"\n",
    };

    #[test]
    fn index_metadata_is_the_branch_and_the_next_commit_message() {
        let (index, _) = index_and_working_directory(&[INDEX_METADATA], &[]).unwrap();
        assert_eq!(
            index.next_commit,
            NextCommit::Branch(gix::refs::FullName::try_from("refs/heads/main").unwrap())
        );
        assert_eq!(index.subject, "Subject");
        assert_eq!(index.body, "Body.");
    }

    #[test]
    fn index_without_branch_is_detached_and_its_body_is_optional() {
        let metadata = RawFile {
            name: METADATA_FILE,
            content: b"subject = \"Subject\"\n",
        };
        let (index, _) = index_and_working_directory(&[metadata], &[]).unwrap();
        assert_eq!(index.next_commit, NextCommit::DetachedHead);
        assert_eq!(index.body, "");
    }

    /// The `before` version of a file of the working directory where `b.txt` is unmerged: its
    /// version in HEAD.
    const UNMERGED_BEFORE: RawFile = RawFile {
        name: "01_before_b.txt",
        content: b"head\n",
    };
    /// The `after` version of the same file, which adds a conflict.
    const UNMERGED_AFTER: RawFile = RawFile {
        name: "01_after_b.txt",
        content: b"<<<<<<< HEAD\nhead\n=======\nother\n>>>>>>> other\n",
    };

    /// A file adds a conflict marker if its `after` version has a `<<<<<<<` line that its
    /// `before` version doesn't have: `c.txt` has the same marker in both versions, so it is not
    /// unmerged. `a.txt` and `z.txt` are around `b.txt` in the order of the paths.
    #[test]
    fn conflict_markers_added_in_the_working_directory_make_their_file_unmerged_in_the_index() {
        let index_files = [
            INDEX_METADATA,
            RawFile { name: "01_after_a.txt", content: b"a\n" },
            RawFile { name: "02_after_z.txt", content: b"z\n" },
        ];
        let working_directory_files = [
            UNMERGED_BEFORE,
            UNMERGED_AFTER,
            RawFile {
                name: "02_before_c.txt",
                content: b"<<<<<<< kept\nbefore\n",
            },
            RawFile {
                name: "02_after_c.txt",
                content: b"<<<<<<< kept\nafter\n",
            },
        ];
        let (index, _) =
            index_and_working_directory(&index_files, &working_directory_files).unwrap();
        let files: Vec<_> = index
            .files
            .iter()
            .map(|file| (file.path.to_string(), file.content))
            .collect();
        assert_eq!(
            files,
            [
                ("a.txt".to_string(), FileContentType::Text),
                ("b.txt".to_string(), FileContentType::Unmerged),
                ("z.txt".to_string(), FileContentType::Text),
            ]
        );
    }

    /// Unlike in the index, where they have no diff.
    #[test]
    fn unmerged_files_are_diffed_in_the_working_directory() {
        let (_, working_directory) =
            index_and_working_directory(&[INDEX_METADATA], &[UNMERGED_BEFORE, UNMERGED_AFTER])
                .unwrap();
        let expected = diff::file(
            /* before */ state(&Some(UNMERGED_BEFORE), "b.txt"),
            /* after */ state(&Some(UNMERGED_AFTER), "b.txt"),
            DiffAs::Auto,
            CONTEXT,
        );
        let [file] = &working_directory.files[..] else {
            panic!("{} files instead of 1", working_directory.files.len());
        };
        assert_eq!(file.content, FileContentType::Text);
        assert_eq!(file.hunks, expected.hunks);
    }

    #[test]
    fn working_directory_with_metadata_is_an_error() {
        let metadata = RawFile { name: METADATA_FILE, content: b"" };
        assert_eq!(
            index_and_working_directory(&[INDEX_METADATA], &[metadata]).unwrap_err(),
            FakeDataError::UnexpectedMetadata
        );
    }

    #[test]
    fn index_without_metadata_is_an_error() {
        assert_eq!(
            index_and_working_directory(&[], &[]).unwrap_err(),
            FakeDataError::MissingMetadata
        );
    }
}
