//! The cleanup of commit messages.

use gix::bstr::BStr;
use gix::bstr::BString;
use gix::bstr::ByteSlice;

/// The start of the comment lines of the commit messages, if `core.commentChar` doesn't set it.
const DEFAULT_COMMENT_PREFIX: &str = "#";

/// The value of `core.commentChar` asking git to pick a character not used by the message.
const AUTO_COMMENT_CHAR: &str = "auto";

/// `message`, cleaned up like `git commit` does by default (`git stripspace --strip-comments`):
/// without its comment lines (`core.commentChar`), without trailing whitespace, without blank lines
/// at its start and end, and with consecutive blank lines merged.
pub fn stripped(repository: &gix::Repository, message: &BStr) -> BString {
    stripped_like_git(message, &comment_prefix(repository))
}

/// The start of the comment lines of the commit messages of `repository` (`core.commentChar`).
///
/// With `auto`, git picks a character when it writes a message, which cannot be known when
/// reading it: the default one is used.
pub fn comment_prefix(repository: &gix::Repository) -> String {
    let config = repository.config_snapshot();
    match config.string("core.commentChar") {
        Some(prefix) if prefix.as_slice() != AUTO_COMMENT_CHAR.as_bytes() && !prefix.is_empty() => {
            prefix.to_str_lossy().into_owned()
        }
        _ => DEFAULT_COMMENT_PREFIX.to_string(),
    }
}

/// Like [`stripped()`], the comment lines being those starting with `comment_prefix`.
fn stripped_like_git(message: &BStr, comment_prefix: &str) -> BString {
    let lines = message
        .lines()
        .filter(|line| !line.starts_with(comment_prefix.as_bytes()))
        .map(|line| line.trim_end());
    let mut cleaned = BString::default();
    let mut blank_pending = false;
    for line in lines {
        if line.is_empty() {
            blank_pending = !cleaned.is_empty();
            continue;
        }
        if blank_pending {
            cleaned.push(b'\n');
            blank_pending = false;
        }
        cleaned.extend_from_slice(line);
        cleaned.push(b'\n');
    }
    cleaned
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_trailing_whitespace_and_extra_blank_lines_are_removed() {
        let message = "\n# Comment.\nSubject  \n\n\n# Comment.\n\nBody.\n\n# Conflicts:\n";
        assert_eq!(
            stripped_like_git(message.into(), DEFAULT_COMMENT_PREFIX),
            "Subject\n\nBody.\n"
        );
    }
}
