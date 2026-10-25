//! Snapshots of the screen, each one displaying a single fake commit, so that they only change
//! when this commit or the rendering changes.
//!
//! They only contain the text (the styles have their own unit tests), without the status bar
//! (which has its own unit tests too).

use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::backend::TestBackend;
use reedi::config::UnknownKey;
use reedi::date::DateDisplay;
use reedi::fake;
use reedi::model::Commit;
use reedi::theme::Theme;
use reedi::view;
use reedi::view::ErrorModal;
use reedi::view::Mode;
use reedi::viewport::Viewport;

/// Invisible in the snapshots, so removed from them.
const LEFT_TO_RIGHT_MARK: char = '\u{200e}';

/// The text of the screen `width` × `height` displaying `commit` and `error_modal` from
/// `viewport`, without the left-to-right marks, and without its last row, the status bar.
fn screen(
    commit: Commit,
    width: u16,
    height: u16,
    viewport: Viewport,
    error_modal: Option<&ErrorModal>,
) -> String {
    let commits = [commit];
    let theme = Theme::monochrome();
    let rows = view::rows(&commits, width, &theme, &DateDisplay::for_snapshots()).unwrap();
    let mut viewport = viewport;
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| {
            view::draw(
                frame,
                &commits,
                &rows,
                &mut viewport,
                Mode::Review,
                /* pending_count */ None,
                error_modal,
                &theme,
            )
        })
        .unwrap();
    let mut buffer = terminal.backend().buffer().clone();
    buffer.content.iter_mut().for_each(|cell| {
        let symbol = cell.symbol().replace(LEFT_TO_RIGHT_MARK, "");
        cell.set_symbol(&symbol);
    });
    let mut backend = TestBackend::new(width, height);
    let cells = buffer.content.iter().enumerate().map(|(i, cell)| {
        let (x, y) = (i % usize::from(width), i / usize::from(width));
        (x as u16, y as u16, cell)
    });
    backend.draw(cells).unwrap();
    let screen = backend.to_string();
    let mut rows: Vec<_> = screen.lines().collect();
    rows.pop();
    rows.join("\n")
}

/// The number of rows displaying `commit` on a screen `width` columns wide.
fn rows_count(commit: &Commit, width: u16) -> usize {
    view::rows(
        std::slice::from_ref(commit),
        width,
        &Theme::monochrome(),
        &DateDisplay::for_snapshots(),
    )
    .unwrap()
    .len()
}

/// An error modal displaying 2 unknown keys of the config file.
fn unknown_keys_error_modal() -> ErrorModal {
    let unknown_key = |key: &str, value: &str, line| UnknownKey {
        key: key.to_string(),
        value: value.parse().unwrap(),
        line,
    };
    ErrorModal::UnknownKeys {
        path: "/home/alice/.config/reedi/config.toml".into(),
        unknown_keys: vec![
            unknown_key("scrolloff", "3", 3),
            unknown_key("keys", "{ quit = \"q\", down = \"j\", up = \"k\" }", 5),
        ],
    }
}

#[test]
fn screen_scrolled_to_the_top_displays_the_header_the_body_and_the_first_file() {
    let viewport = Viewport { cursor: 5, top: 0 };
    insta::assert_snapshot!(screen(fake::file_changes(), 110, 26, viewport, None));
}

#[test]
fn small_screen_is_wrapped_and_cropped() {
    // The scrolloff is 3 rows on a screen of 7 rows.
    let viewport = Viewport { cursor: 3, top: 0 };
    insta::assert_snapshot!(screen(fake::file_changes(), 50, 8, viewport, None));
}

#[test]
fn screen_scrolled_to_the_end_displays_the_last_rows() {
    let commit = fake::scrolloff_rename();
    let viewport = Viewport {
        cursor: rows_count(&commit, 110) - 1,
        top: 0,
    };
    insta::assert_snapshot!(screen(commit, 110, 26, viewport, None));
}

#[test]
fn all_kinds_of_file_changes_are_displayed() {
    let commit = fake::file_changes();
    let height = rows_count(&commit, 110) + 1;
    let viewport = Viewport { cursor: 5, top: 0 };
    insta::assert_snapshot!(screen(commit, 110, height as u16, viewport, None));
}

#[test]
fn long_lines_are_wrapped() {
    let viewport = Viewport { cursor: 5, top: 0 };
    insta::assert_snapshot!(screen(fake::line_wrapping(), 110, 60, viewport, None));
}

#[test]
fn long_lines_are_wrapped_on_a_narrow_screen() {
    let viewport = Viewport { cursor: 5, top: 0 };
    insta::assert_snapshot!(screen(fake::line_wrapping(), 70, 80, viewport, None));
}

#[test]
fn invisible_characters_of_the_diff_lines_are_replaced() {
    let viewport = Viewport {
        cursor: 30,
        top: 20,
    };
    insta::assert_snapshot!(screen(
        fake::invisible_characters(),
        110,
        26,
        viewport,
        None
    ));
}

#[test]
fn error_modal_is_displayed_over_the_commits() {
    let error_modal = unknown_keys_error_modal();
    let viewport = Viewport { cursor: 5, top: 0 };
    insta::assert_snapshot!(screen(
        fake::file_changes(),
        110,
        26,
        viewport,
        Some(&error_modal)
    ));
}

#[test]
fn long_lines_of_the_error_modal_are_wrapped() {
    let error_modal = unknown_keys_error_modal();
    let viewport = Viewport { cursor: 5, top: 0 };
    insta::assert_snapshot!(screen(
        fake::file_changes(),
        40,
        12,
        viewport,
        Some(&error_modal)
    ));
}
