//! Snapshots of what only the whole screen shows: the error modal displayed over the commits.
//!
//! They only contain the text (the styles have their own unit tests), without the status bar
//! (which has its own unit tests too).

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use reedi::date::DateDisplay;
use reedi::fake;
use reedi::model::Commit;
use reedi::model::Revision;
use reedi::test_support::backend_without_left_to_right_marks;
use reedi::test_support::unknown_settings_error_modal;
use reedi::theme::Theme;
use reedi::view;
use reedi::view::ErrorModal;
use reedi::view::Mode;
use reedi::viewport::Viewport;

/// The text of the screen `width` × `height` displaying `commit` from `viewport` and
/// `error_modal` over it, without the left-to-right marks, and without its last row, the status
/// bar.
fn screen(
    commit: Commit,
    width: u16,
    height: u16,
    viewport: Viewport,
    error_modal: &ErrorModal,
) -> String {
    let revisions = [Revision::Commit(commit)];
    let theme = Theme::monochrome();
    let rows = view::rows(&revisions, width, &theme, &DateDisplay::for_snapshots()).unwrap();
    let mut viewport = viewport;
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| {
            view::draw(
                frame,
                &revisions,
                &rows,
                &mut viewport,
                Mode::Review,
                /* pending_count */ None,
                Some(error_modal),
                &theme,
            )
        })
        .unwrap();
    let screen = backend_without_left_to_right_marks(terminal.backend().buffer()).to_string();
    let mut rows: Vec<_> = screen.lines().collect();
    rows.pop();
    rows.join("\n")
}

/// A fake commit with a single file, so that the order of the files of a commit doesn't change the
/// snapshots, and taller than the screen of the snapshots.
fn commit_behind_the_error_modal() -> Commit {
    fake::scrolloff_rename()
}

#[test]
fn error_modal_is_displayed_over_the_commits() {
    let error_modal = unknown_settings_error_modal();
    let viewport = Viewport { cursor: 5, top: 0 };
    insta::assert_snapshot!(screen(
        commit_behind_the_error_modal(),
        /* width */ 110,
        /* height */ 26,
        viewport,
        &error_modal
    ));
}

#[test]
fn long_lines_of_the_error_modal_are_wrapped() {
    let error_modal = unknown_settings_error_modal();
    let viewport = Viewport { cursor: 5, top: 0 };
    insta::assert_snapshot!(screen(
        commit_behind_the_error_modal(),
        /* width */ 40,
        /* height */ 12,
        viewport,
        &error_modal
    ));
}
