//! Realistic fake data, used until real git data is read. It is read from the files of
//! `etc/fake_commits/`, embedded in the binary.

mod loader;

use crate::model::Commit;

/// The descriptions of the folders of the fake commits that can be read by name.
const FILE_CHANGES: &str = "file_changes";
const LINE_WRAPPING: &str = "line_wrapping";
const INVISIBLE_CHARACTERS: &str = "invisible_characters";
const SCROLLOFF_RENAME: &str = "scrolloff_rename";

/// All the fake commits, in topological order (oldest first): the folders of
/// `etc/fake_commits/`, from the highest index to `00`.
pub fn all() -> Vec<Commit> {
    loader::embedded().iter().rev().map(commit_of).collect()
}

fn commit_of(folder: &loader::Folder) -> Commit {
    loader::commit(&folder.files).unwrap_or_else(|error| panic!("{}: {error}", folder.name))
}

/// The fake commit whose folder is described by `description`.
fn named(description: &str) -> Commit {
    let folders = loader::embedded();
    let folder = folders
        .iter()
        .find(|folder| {
            loader::parse_folder_name(folder.name).is_ok_and(|(_, other)| other == description)
        })
        .unwrap_or_else(|| panic!("no folder of fake commit is described by {description}"));
    commit_of(folder)
}

/// A commit with all kinds of file changes: added, deleted, renamed, binary and not valid UTF-8
/// files, files without newline at their end, and blocks of lines that are only removed or only
/// added.
pub fn file_changes() -> Commit {
    named(FILE_CHANGES)
}

/// A commit whose lines are too wide for a screen 110 columns wide, in all the areas: header,
/// body, and code (including wide characters, tabs, trailing whitespace and right-to-left text).
pub fn line_wrapping() -> Commit {
    named(LINE_WRAPPING)
}

/// A commit with all kinds of invisible characters, in all the areas: subject, author, body, code
/// with consistent and inconsistent line endings.
pub fn invisible_characters() -> Commit {
    named(INVISIBLE_CHARACTERS)
}

/// A commit modifying two chunks of the same file.
pub fn scrolloff_rename() -> Commit {
    named(SCROLLOFF_RENAME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::FileChange;
    use crate::model::FileContentType;
    use crate::model::LineKind;

    #[test]
    fn folders_are_numbered_from_00_without_gap() {
        let indexes: Vec<_> = loader::embedded()
            .iter()
            .map(|folder| loader::parse_folder_name(folder.name).unwrap().0)
            .collect();
        assert!(!indexes.is_empty());
        assert_eq!(indexes, (0..indexes.len()).collect::<Vec<_>>());
    }

    #[test]
    fn all_loads_every_folder() {
        assert_eq!(all().len(), loader::embedded().len());
    }

    #[test]
    fn named_functions_return_a_commit_of_all() {
        let hashes: Vec<_> = all().into_iter().map(|commit| commit.hash).collect();
        [
            file_changes(),
            line_wrapping(),
            invisible_characters(),
            scrolloff_rename(),
        ]
        .into_iter()
        .for_each(|commit| assert!(hashes.contains(&commit.hash), "{}", commit.subject));
    }

    #[test]
    fn fake_descriptions_are_different() {
        let descriptions: Vec<_> = loader::embedded()
            .iter()
            .map(|folder| loader::parse_folder_name(folder.name).unwrap().1)
            .collect();
        let unique: std::collections::HashSet<_> = descriptions.iter().collect();
        assert_eq!(unique.len(), descriptions.len());
    }

    #[test]
    fn fake_hashes_are_different() {
        let hashes: Vec<_> = all().into_iter().map(|commit| commit.hash).collect();
        let unique: std::collections::HashSet<_> = hashes.iter().collect();
        assert_eq!(unique.len(), hashes.len());
    }

    fn repository_file(path: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
        std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path:?}: {error}"))
    }

    /// Compared with `git check-attr`: git doesn't convert nor warn about the fake data.
    #[test]
    fn git_does_not_modify_the_fake_data() {
        let folders = loader::embedded();
        let paths: Vec<_> = folders
            .iter()
            .flat_map(|folder| {
                folder
                    .files
                    .iter()
                    .map(|file| format!("etc/fake_commits/{}/{}", folder.name, file.name))
            })
            .collect();
        assert!(!paths.is_empty());
        let output = std::process::Command::new("git")
            .args(["check-attr", "text", "whitespace", "--"])
            .args(&paths)
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .unwrap();
        let attributes = String::from_utf8(output.stdout).unwrap();
        paths.iter().for_each(|path| {
            assert!(
                attributes.contains(&format!("{path}: text: unset")),
                "{path}"
            );
            assert!(
                attributes.contains(&format!("{path}: whitespace: unset")),
                "{path}"
            );
        });
    }

    #[test]
    fn a_single_glob_of_gitattributes_covers_the_fake_data() {
        let gitattributes = repository_file(".gitattributes");
        let globs: Vec<_> = gitattributes
            .lines()
            .filter(|line| line.contains("etc/fake_commits"))
            .collect();
        assert_eq!(globs.len(), 1, "{globs:?}");
    }

    #[test]
    fn editors_do_not_modify_the_fake_data() {
        let editorconfig = repository_file(".editorconfig");
        let headers: Vec<_> = editorconfig
            .lines()
            .filter(|line| line.contains("etc/fake_commits"))
            .collect();
        assert_eq!(headers, ["[etc/fake_commits/**]"]);
        let section: Vec<_> = editorconfig
            .lines()
            .skip_while(|line| *line != "[etc/fake_commits/**]")
            .skip(1)
            .take_while(|line| !line.starts_with('['))
            .collect();
        [
            "charset",
            "end_of_line",
            "indent_style",
            "insert_final_newline",
            "trim_trailing_whitespace",
        ]
        .into_iter()
        .for_each(|property| {
            assert!(
                section.contains(&format!("{property} = unset").as_str()),
                "{property}"
            );
        });
    }

    #[test]
    fn fake_hashes_are_full_hashes() {
        all().iter().for_each(|commit| {
            let hash = commit.hash.to_string();
            assert_eq!(hash.len(), 40, "{hash}");
            assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()), "{hash}");
        });
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
