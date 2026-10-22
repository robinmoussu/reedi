//! The styles used by the rendering, for each colorscheme.

use ratatui::style::Color;
use ratatui::style::Style;
use terminal_colorsaurus::QueryOptions;
use terminal_colorsaurus::ThemeMode;

use crate::config::Colorscheme;

/// The background color of the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Background {
    Light,
    Dark,
}

/// All the styles used by the rendering.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub hash: Style,
    pub author: Style,
    pub date: Style,
    pub decoration_punctuation: Style,
    pub head: Style,
    pub local_branch: Style,
    pub remote_branch: Style,
    pub tag: Style,
    pub file: Style,
    /// The borders of the line number column.
    pub line_number: Style,
    pub removed_line_number: Style,
    pub added_line_number: Style,
    pub unchanged_line_number: Style,
    /// The `↵` marker of the wrapped lines of the diffs.
    pub marker: Style,
    /// The text area of the removed lines.
    pub removed_line: Style,
    /// The text area of the added lines.
    pub added_line: Style,
    pub commit_separator: Style,
    pub error: Style,
    /// Replacements of the tabs, and of the spaces and U+2800 next to a break, that are not part
    /// of the trailing whitespace.
    pub dimmed_whitespace: Style,
    /// Replacements of the trailing whitespace, including the tabs.
    pub trailing_whitespace: Style,
    /// Replacements of the other invisible characters.
    pub invisible: Style,
    /// The row of the cursor.
    pub cursor: Style,
    /// The name of the review mode, in the status bar.
    pub review_mode: Style,
    /// The status bar, behind the mode, the blank column after it and the content.
    pub status_bar: Style,
    /// The content of the status bar, after the mode.
    pub status_bar_content: Style,
}

impl Theme {
    /// The colors of git and of `delta` by default, for a dark background.
    pub fn dark() -> Self {
        Self {
            // Colors used by git by default.
            hash: Style::new().fg(Color::Yellow),
            author: Style::new().fg(Color::Green),
            date: Style::new().fg(Color::Blue),
            decoration_punctuation: Style::new().fg(Color::Yellow),
            head: Style::new().fg(Color::Cyan).bold(),
            local_branch: Style::new().fg(Color::Green).bold(),
            remote_branch: Style::new().fg(Color::Red).bold(),
            tag: Style::new().fg(Color::Yellow).bold(),
            file: Style::new().fg(Color::Blue),
            // The line number column never has the background of the modified lines.
            line_number: Style::new().fg(Color::Blue).bg(Color::Reset),
            // Colors used by `delta` by default.
            removed_line_number: Style::new().fg(Color::Indexed(88)).bg(Color::Reset),
            added_line_number: Style::new().fg(Color::Indexed(28)).bg(Color::Reset),
            unchanged_line_number: Style::new()
                .fg(Color::Rgb(0x44, 0x44, 0x44))
                .bg(Color::Reset),
            marker: Style::new().fg(Color::Blue),
            removed_line: Style::new().bg(Color::Rgb(0x3f, 0x00, 0x01)),
            added_line: Style::new().bg(Color::Rgb(0x00, 0x28, 0x00)),
            commit_separator: Style::new().fg(Color::Red),
            error: Style::new().fg(Color::LightRed).bold(),
            dimmed_whitespace: Style::new().fg(Color::DarkGray),
            trailing_whitespace: Style::new().fg(Color::Red).bold(),
            invisible: Style::new().fg(Color::Red).bold(),
            cursor: Style::new().bg(Color::Indexed(235)),
            review_mode: Style::new().fg(Color::Black).bg(Color::Blue).bold(),
            status_bar: Style::new(),
            status_bar_content: Style::new().fg(Color::Indexed(248)),
        }
    }

    /// The colors of git, and of `delta` by default for a light background.
    pub fn light() -> Self {
        Self {
            // Colors used by `delta` by default.
            removed_line_number: Style::new().fg(Color::Red).bg(Color::Reset),
            added_line_number: Style::new().fg(Color::Green).bg(Color::Reset),
            unchanged_line_number: Style::new()
                .fg(Color::Rgb(0xdd, 0xdd, 0xdd))
                .bg(Color::Reset),
            removed_line: Style::new().bg(Color::Rgb(0xff, 0xe0, 0xe0)),
            added_line: Style::new().bg(Color::Rgb(0xd0, 0xff, 0xd0)),
            dimmed_whitespace: Style::new().fg(Color::Indexed(250)),
            cursor: Style::new().bg(Color::Indexed(254)),
            status_bar: Style::new().bg(Color::Indexed(252)),
            status_bar_content: Style::new().fg(Color::Indexed(240)),
            // The colors of git are the same.
            ..Self::dark()
        }
    }

    /// No color, only the default foreground and background of the terminal. Bold is kept, and the
    /// row of the cursor and the mode use reverse video.
    pub fn monochrome() -> Self {
        Self {
            hash: Style::new(),
            author: Style::new(),
            date: Style::new(),
            decoration_punctuation: Style::new(),
            head: Style::new().bold(),
            local_branch: Style::new().bold(),
            remote_branch: Style::new().bold(),
            tag: Style::new().bold(),
            file: Style::new(),
            line_number: Style::new(),
            removed_line_number: Style::new(),
            added_line_number: Style::new(),
            unchanged_line_number: Style::new(),
            marker: Style::new(),
            removed_line: Style::new(),
            added_line: Style::new(),
            commit_separator: Style::new(),
            error: Style::new().bold(),
            dimmed_whitespace: Style::new(),
            trailing_whitespace: Style::new().bold(),
            invisible: Style::new().bold(),
            cursor: Style::new().reversed(),
            review_mode: Style::new().reversed().bold(),
            status_bar: Style::new(),
            status_bar_content: Style::new(),
        }
    }

    /// The theme of `colorscheme`, or if it is unset, the one matching the background of the
    /// terminal returned by `detect` (dark if it cannot be detected).
    pub fn choose(
        colorscheme: Option<Colorscheme>,
        detect: impl FnOnce() -> Option<Background>,
    ) -> Self {
        match colorscheme {
            Some(Colorscheme::Light) => Self::light(),
            Some(Colorscheme::Dark) => Self::dark(),
            Some(Colorscheme::Monochrome) => Self::monochrome(),
            None => match detect() {
                Some(Background::Light) => Self::light(),
                Some(Background::Dark) | None => Self::dark(),
            },
        }
    }
}

/// The background color of the terminal, if it can be detected.
pub fn detect() -> Option<Background> {
    match terminal_colorsaurus::theme_mode(QueryOptions::default()).ok()? {
        ThemeMode::Light => Some(Background::Light),
        ThemeMode::Dark => Some(Background::Dark),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colorscheme_is_used_without_detecting_the_background() {
        let undetected = || -> Option<Background> { panic!("the background is detected") };
        assert_eq!(
            Theme::choose(Some(Colorscheme::Light), undetected),
            Theme::light()
        );
        assert_eq!(
            Theme::choose(Some(Colorscheme::Dark), undetected),
            Theme::dark()
        );
        assert_eq!(
            Theme::choose(Some(Colorscheme::Monochrome), undetected),
            Theme::monochrome()
        );
    }

    #[test]
    fn unset_colorscheme_uses_the_background_of_the_terminal() {
        assert_eq!(
            Theme::choose(None, || Some(Background::Light)),
            Theme::light()
        );
        assert_eq!(
            Theme::choose(None, || Some(Background::Dark)),
            Theme::dark()
        );
    }

    #[test]
    fn theme_is_dark_if_the_background_cannot_be_detected() {
        assert_eq!(Theme::choose(None, || None), Theme::dark());
    }
}
