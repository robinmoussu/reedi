//! Terminal handling and event loop.

use std::io;
use std::path::PathBuf;

use nix::sys::signal;
use nix::sys::signal::Signal;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event;
use ratatui::crossterm::event::Event;
use ratatui::crossterm::event::KeyCode;
use ratatui::crossterm::event::KeyModifiers;
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal;
use ratatui::crossterm::terminal::EnterAlternateScreen;

use crate::config::UnknownKey;
use crate::date::DateDisplay;
use crate::fake;
use crate::model::Commit;
use crate::settings::Settings;
use crate::theme::Theme;
use crate::view;
use crate::view::ErrorModal;
use crate::view::Mode;
use crate::view::Row;
use crate::viewport::Viewport;
use crate::wrap::TooNarrow;

/// What a key press does in review mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Down,
    Up,
    /// `ctrl+e`.
    ScrollDown,
    /// `ctrl+y`.
    ScrollUp,
    /// `d` or `ctrl+d`.
    HalfPageDown,
    /// `u` or `ctrl+u`.
    HalfPageUp,
    /// `g`.
    First,
    /// `G`.
    Last,
    /// `J` or `ctrl+down`.
    NextCommit,
    /// `K` or `ctrl+up`.
    PreviousCommit,
    /// A digit of the number repeating the next command.
    Digit(u8),
    Quit,
    Suspend,
}

/// The number typed before a command, which repeats it (or is the row `g` and `G` go to).
#[derive(Debug, Default)]
struct Count(Option<usize>);

impl Count {
    /// The number typed so far, if any. Its leading zeros are ignored, so it is never 0.
    fn pending(&self) -> Option<usize> {
        self.0
    }

    /// Update the number with `event`. Return the action `event` triggers with the number typed
    /// before it, if any, which is then cleared. A digit is added to the number and triggers
    /// nothing. Any other key press cancels the number; key releases and other events (like
    /// resizing the screen) don't.
    fn apply(&mut self, event: &Event) -> Option<(Action, Option<usize>)> {
        let Event::Key(key) = event else {
            return None;
        };
        if !key.is_press() {
            return None;
        }
        match action(event) {
            Some(Action::Digit(digit)) => {
                self.push(digit);
                None
            }
            Some(action) => Some((action, self.0.take())),
            None => {
                self.0 = None;
                None
            }
        }
    }

    /// Add `digit` to the number, unless it is a leading zero. The number saturates.
    fn push(&mut self, digit: u8) {
        if self.0.is_some() || digit != 0 {
            self.0 = Some(
                self.0
                    .unwrap_or(0)
                    .saturating_mul(10)
                    .saturating_add(usize::from(digit)),
            );
        }
    }
}

/// Apply the movement `action`, repeated `count` times (or the number it is given, for `First`
/// and `Last`), to `viewport` on the `rows`, displayed on a screen `height` rows tall (without
/// the status bar). Other actions do nothing.
fn navigate(
    action: Action,
    count: Option<usize>,
    viewport: &mut Viewport,
    rows: &[Row],
    height: usize,
) {
    let len = rows.len();
    let times = count.unwrap_or(1);
    match action {
        Action::Down => {
            viewport.down_by(times, len);
            viewport.scroll(len, height);
        }
        Action::Up => {
            viewport.up_by(times);
            viewport.scroll(len, height);
        }
        Action::ScrollDown => viewport.scroll_down(times, len, height),
        Action::ScrollUp => viewport.scroll_up(times, height),
        Action::HalfPageDown => viewport.half_page_down(times, len, height),
        Action::HalfPageUp => viewport.half_page_up(times, len, height),
        Action::First => viewport.jump(times.saturating_sub(1), len, height),
        Action::Last => {
            let row = count.map_or(len.saturating_sub(1), |row| row.saturating_sub(1));
            viewport.jump(row, len, height);
        }
        Action::NextCommit => jump_to_commit(
            view::next_commit(rows, viewport.cursor, times),
            viewport,
            len,
            height,
        ),
        Action::PreviousCommit => jump_to_commit(
            view::previous_commit(rows, viewport.cursor, times),
            viewport,
            len,
            height,
        ),
        Action::Digit(_) | Action::Quit | Action::Suspend => {}
    }
}

/// Move the cursor to the row `row`, unless it is already on it.
fn jump_to_commit(row: usize, viewport: &mut Viewport, len: usize, height: usize) {
    if row != viewport.cursor {
        viewport.jump(row, len, height);
    }
}

/// Display the commits with `settings` until `q` or `ctrl+c` is pressed, and the `unknown_keys` of
/// the config file (its path, and at least one key) in an error modal over them, if any, until a
/// key is pressed.
///
/// Like `less`, the alternate screen is used, and the screen is redrawn when its size changes.
/// Fails if a text area outside of the chunks is too narrow to display a grapheme cluster of its
/// line.
pub fn run(
    settings: &Settings,
    unknown_keys: Option<(PathBuf, Vec<UnknownKey>)>,
) -> io::Result<()> {
    let mut error_modal =
        unknown_keys.map(|(path, unknown_keys)| ErrorModal::UnknownKeys { path, unknown_keys });
    let commits = fake::all();
    let mut rows = Vec::new();
    let mut size = None;
    let mut viewport = Viewport::default();
    let mut count = Count::default();
    ratatui::run(|terminal| {
        loop {
            let screen = terminal.size()?;
            let height = usize::from(screen.height.saturating_sub(view::STATUS_BAR_HEIGHT));
            if size != Some(screen) {
                if size.map(|size| size.width) != Some(screen.width) {
                    rows = rewrap(
                        &commits,
                        &rows,
                        &mut viewport,
                        screen.width,
                        &settings.theme,
                        &settings.dates,
                    )
                    .map_err(io::Error::other)?;
                }
                viewport.scroll(rows.len(), height);
                size = Some(screen);
            }
            terminal.draw(|frame| {
                view::draw(
                    frame,
                    &commits,
                    &rows,
                    &mut viewport,
                    Mode::Review,
                    count.pending(),
                    error_modal.as_ref(),
                    &settings.theme,
                )
            })?;
            let event = event::read()?;
            let command = if error_modal.is_some() {
                handle(&event, &mut error_modal);
                None
            } else {
                count.apply(&event)
            };
            match command {
                Some((Action::Quit, _)) => return Ok(()),
                Some((Action::Suspend, _)) => suspend(terminal)?,
                Some((action, count)) => navigate(action, count, &mut viewport, &rows, height),
                None => {}
            }
        }
    })
}

/// The rows displaying `commits` on a screen `width` columns wide, replacing `rows`.
///
/// The cursor goes, at the same screen position, on the row displaying the character that was the
/// first of its row (see [`Row::anchor`]).
fn rewrap(
    commits: &[Commit],
    rows: &[Row],
    viewport: &mut Viewport,
    width: u16,
    theme: &Theme,
    dates: &DateDisplay,
) -> Result<Vec<Row>, TooNarrow> {
    let new_rows = view::rows(commits, width, theme, dates)?;
    if let Some(row) = rows.get(viewport.cursor) {
        viewport.move_cursor(view::find(&new_rows, row.anchor()));
    }
    Ok(new_rows)
}

/// The action triggered by `event`, if any. While `error_modal` is displayed, a key press only
/// closes it.
fn handle(event: &Event, error_modal: &mut Option<ErrorModal>) -> Option<Action> {
    match error_modal {
        Some(_) => {
            if matches!(event, Event::Key(key) if key.is_press()) {
                *error_modal = None;
            }
            None
        }
        None => action(event),
    }
}

/// The action triggered by `event`, if any. All other keys are inactive.
fn action(event: &Event) -> Option<Action> {
    let Event::Key(key) = event else {
        return None;
    };
    if !key.is_press() {
        return None;
    }
    match (key.code, key.modifiers) {
        (KeyCode::Char('j') | KeyCode::Down, KeyModifiers::NONE) => Some(Action::Down),
        (KeyCode::Char('k') | KeyCode::Up, KeyModifiers::NONE) => Some(Action::Up),
        (KeyCode::Char('e'), KeyModifiers::CONTROL) => Some(Action::ScrollDown),
        (KeyCode::Char('y'), KeyModifiers::CONTROL) => Some(Action::ScrollUp),
        (KeyCode::Char('d'), KeyModifiers::NONE | KeyModifiers::CONTROL) => {
            Some(Action::HalfPageDown)
        }
        (KeyCode::Char('u'), KeyModifiers::NONE | KeyModifiers::CONTROL) => {
            Some(Action::HalfPageUp)
        }
        (KeyCode::Char('g'), KeyModifiers::NONE) => Some(Action::First),
        (KeyCode::Char('G'), KeyModifiers::NONE | KeyModifiers::SHIFT) => Some(Action::Last),
        (KeyCode::Char('J'), KeyModifiers::NONE | KeyModifiers::SHIFT)
        | (KeyCode::Down, KeyModifiers::CONTROL) => Some(Action::NextCommit),
        (KeyCode::Char('K'), KeyModifiers::NONE | KeyModifiers::SHIFT)
        | (KeyCode::Up, KeyModifiers::CONTROL) => Some(Action::PreviousCommit),
        (KeyCode::Char(c @ '0'..='9'), KeyModifiers::NONE) => Some(Action::Digit(c as u8 - b'0')),
        (KeyCode::Char('q'), KeyModifiers::NONE) => Some(Action::Quit),
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => Some(Action::Quit),
        (KeyCode::Char('z'), KeyModifiers::CONTROL) => Some(Action::Suspend),
        _ => None,
    }
}

/// Put the application in the background until it is resumed (e.g. with `fg`).
///
/// In raw mode, `ctrl+z` is received as a key instead of sending `SIGTSTP`, so the terminal is
/// restored and `SIGTSTP` is sent manually.
fn suspend(terminal: &mut DefaultTerminal) -> io::Result<()> {
    ratatui::try_restore()?;
    signal::raise(Signal::SIGTSTP)?;
    // The process is stopped until it receives `SIGCONT`.
    terminal::enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    // The content of the alternate screen may have been modified while suspended.
    terminal.clear()
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::KeyEvent;
    use ratatui::crossterm::event::KeyEventKind;

    use super::*;
    use crate::view::Start;

    fn press(code: KeyCode, modifiers: KeyModifiers) -> Event {
        Event::Key(KeyEvent::new_with_kind(
            code,
            modifiers,
            KeyEventKind::Press,
        ))
    }

    fn key(code: KeyCode) -> Event {
        press(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> Event {
        press(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn j_and_down_move_down() {
        assert_eq!(action(&key(KeyCode::Char('j'))), Some(Action::Down));
        assert_eq!(action(&key(KeyCode::Down)), Some(Action::Down));
    }

    #[test]
    fn k_and_up_move_up() {
        assert_eq!(action(&key(KeyCode::Char('k'))), Some(Action::Up));
        assert_eq!(action(&key(KeyCode::Up)), Some(Action::Up));
    }

    #[test]
    fn q_and_ctrl_c_quit() {
        assert_eq!(action(&key(KeyCode::Char('q'))), Some(Action::Quit));
        assert_eq!(action(&ctrl('c')), Some(Action::Quit));
    }

    #[test]
    fn ctrl_z_suspends() {
        assert_eq!(action(&ctrl('z')), Some(Action::Suspend));
    }

    #[test]
    fn other_keys_are_inactive() {
        assert_eq!(action(&key(KeyCode::Char('a'))), None);
        assert_eq!(action(&key(KeyCode::Enter)), None);
        assert_eq!(action(&ctrl('j')), None);
        assert_eq!(
            action(&press(KeyCode::Char('Q'), KeyModifiers::SHIFT)),
            None
        );
    }

    #[test]
    fn key_release_is_inactive() {
        let release = Event::Key(KeyEvent::new_with_kind(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        ));
        assert_eq!(action(&release), None);
    }

    #[test]
    fn resize_is_inactive() {
        assert_eq!(action(&Event::Resize(80, 24)), None);
    }

    #[test]
    fn cursor_moves_row_by_row() {
        let commits = vec![fake::file_changes()];
        let rows = view::rows(
            &commits,
            110,
            &Theme::monochrome(),
            &DateDisplay::for_snapshots(),
        )
        .unwrap();
        let mut viewport = Viewport::default();
        viewport.down(rows.len());
        // The header of the first commit is displayed on 2 rows.
        assert_eq!(viewport.cursor, 1);
        assert_eq!(rows[1].line, rows[0].line);
    }

    #[test]
    fn first_rows_keep_the_cursor_on_the_first_row() {
        let mut viewport = Viewport::default();
        let rows = rewrap(
            &fake::all(),
            &[],
            &mut viewport,
            110,
            &Theme::monochrome(),
            &DateDisplay::for_snapshots(),
        )
        .unwrap();
        assert!(!rows.is_empty());
        assert_eq!(viewport, Viewport::default());
    }

    #[test]
    fn rewrapping_keeps_the_cursor_on_the_first_character_of_its_row() {
        let commits = vec![fake::file_changes()];
        let narrow = view::rows(
            &commits,
            60,
            &Theme::monochrome(),
            &DateDisplay::for_snapshots(),
        )
        .unwrap();
        // The second row of the first line of the body (the header is the line 0, then a blank
        // line).
        let cursor = narrow
            .iter()
            .position(|row| row.line == 2 && row.start != Start::Whole(0))
            .unwrap();
        let mut viewport = Viewport {
            cursor,
            top: cursor - 3,
        };
        let wide = rewrap(
            &commits,
            &narrow,
            &mut viewport,
            110,
            &Theme::monochrome(),
            &DateDisplay::for_snapshots(),
        )
        .unwrap();
        // The first line of the body is displayed on a single row.
        assert_eq!(wide[viewport.cursor].line, 2);
        assert_eq!(viewport.cursor, view::find(&wide, narrow[cursor].anchor()));
        // At the same screen position.
        assert_eq!(viewport.cursor - viewport.top, 3);
    }

    #[test]
    // A chunk that's too narrow does not stop the application, but it will be replaced by the
    // CHUNK_TOO_NARROW message
    fn rewrapping_to_a_too_narrow_screen_is_an_error() {
        let mut viewport = Viewport::default();
        assert!(
            rewrap(
                &fake::all(),
                &[],
                &mut viewport,
                0,
                &Theme::monochrome(),
                &DateDisplay::for_snapshots()
            )
            .is_err()
        );
    }

    fn error_modal() -> Option<ErrorModal> {
        Some(ErrorModal::UnknownKeys {
            path: "config.toml".into(),
            unknown_keys: Vec::new(),
        })
    }

    #[test]
    fn key_press_only_closes_the_error_modal() {
        let mut error_modal = error_modal();
        assert_eq!(handle(&key(KeyCode::Char('q')), &mut error_modal), None);
        assert_eq!(error_modal, None);
    }

    #[test]
    fn error_modal_stays_open_until_a_key_is_pressed() {
        let mut error_modal = error_modal();
        let release = Event::Key(KeyEvent::new_with_kind(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        ));
        assert_eq!(handle(&release, &mut error_modal), None);
        assert_eq!(handle(&Event::Resize(80, 24), &mut error_modal), None);
        assert!(error_modal.is_some());
    }

    #[test]
    fn keys_are_active_without_error_modal() {
        let mut error_modal = None;
        assert_eq!(
            handle(&key(KeyCode::Char('q')), &mut error_modal),
            Some(Action::Quit)
        );
    }

    #[test]
    fn ctrl_e_and_ctrl_y_scroll_the_screen() {
        assert_eq!(action(&ctrl('e')), Some(Action::ScrollDown));
        assert_eq!(action(&ctrl('y')), Some(Action::ScrollUp));
    }

    #[test]
    fn d_and_u_move_half_a_screen() {
        assert_eq!(action(&key(KeyCode::Char('d'))), Some(Action::HalfPageDown));
        assert_eq!(action(&key(KeyCode::Char('u'))), Some(Action::HalfPageUp));
    }

    #[test]
    fn ctrl_d_and_ctrl_u_are_the_same_as_d_and_u() {
        assert_eq!(action(&ctrl('d')), Some(Action::HalfPageDown));
        assert_eq!(action(&ctrl('u')), Some(Action::HalfPageUp));
    }

    #[test]
    fn g_goes_to_the_first_row_and_shift_g_to_the_last_row() {
        assert_eq!(action(&key(KeyCode::Char('g'))), Some(Action::First));
        assert_eq!(action(&key(KeyCode::Char('G'))), Some(Action::Last));
        assert_eq!(
            action(&press(KeyCode::Char('G'), KeyModifiers::SHIFT)),
            Some(Action::Last)
        );
    }

    #[test]
    fn shift_j_and_ctrl_down_go_to_the_next_commit() {
        assert_eq!(
            action(&press(KeyCode::Char('J'), KeyModifiers::SHIFT)),
            Some(Action::NextCommit)
        );
        assert_eq!(action(&key(KeyCode::Char('J'))), Some(Action::NextCommit));
        assert_eq!(
            action(&press(KeyCode::Down, KeyModifiers::CONTROL)),
            Some(Action::NextCommit)
        );
    }

    #[test]
    fn shift_k_and_ctrl_up_go_to_the_previous_commit() {
        assert_eq!(
            action(&press(KeyCode::Char('K'), KeyModifiers::SHIFT)),
            Some(Action::PreviousCommit)
        );
        assert_eq!(
            action(&key(KeyCode::Char('K'))),
            Some(Action::PreviousCommit)
        );
        assert_eq!(
            action(&press(KeyCode::Up, KeyModifiers::CONTROL)),
            Some(Action::PreviousCommit)
        );
    }

    #[test]
    fn digits_are_actions() {
        ('0'..='9').for_each(|c| {
            let digit = c.to_digit(10).unwrap() as u8;
            assert_eq!(action(&key(KeyCode::Char(c))), Some(Action::Digit(digit)));
        });
    }

    fn typed(count: &mut Count, keys: &str) {
        keys.chars()
            .for_each(|c| assert_eq!(count.apply(&key(KeyCode::Char(c))), None, "{c}"));
    }

    #[test]
    fn typed_digits_build_a_pending_number() {
        let mut count = Count::default();
        assert_eq!(count.pending(), None);
        typed(&mut count, "1");
        assert_eq!(count.pending(), Some(1));
        typed(&mut count, "2");
        assert_eq!(count.pending(), Some(12));
    }

    #[test]
    fn the_pending_number_is_given_to_the_next_command_and_cleared() {
        let mut count = Count::default();
        typed(&mut count, "5");
        assert_eq!(
            count.apply(&key(KeyCode::Char('j'))),
            Some((Action::Down, Some(5)))
        );
        assert_eq!(count.pending(), None);
        assert_eq!(
            count.apply(&key(KeyCode::Char('j'))),
            Some((Action::Down, None))
        );
    }

    #[test]
    fn a_command_without_number_has_none() {
        let mut count = Count::default();
        assert_eq!(
            count.apply(&key(KeyCode::Char('G'))),
            Some((Action::Last, None))
        );
    }

    #[test]
    fn a_leading_zero_is_ignored() {
        let mut count = Count::default();
        typed(&mut count, "0");
        assert_eq!(count.pending(), None);
        typed(&mut count, "05");
        assert_eq!(count.pending(), Some(5));
        typed(&mut count, "0");
        assert_eq!(count.pending(), Some(50));
    }

    #[test]
    fn zero_alone_does_nothing() {
        let mut count = Count::default();
        typed(&mut count, "0");
        assert_eq!(
            count.apply(&key(KeyCode::Char('j'))),
            Some((Action::Down, None))
        );
    }

    #[test]
    fn a_too_large_number_saturates() {
        let mut count = Count::default();
        typed(&mut count, &"9".repeat(40));
        assert_eq!(count.pending(), Some(usize::MAX));
        assert_eq!(
            count.apply(&key(KeyCode::Char('j'))),
            Some((Action::Down, Some(usize::MAX)))
        );
    }

    #[test]
    fn any_other_key_cancels_the_pending_number() {
        [
            key(KeyCode::Esc),
            key(KeyCode::Enter),
            key(KeyCode::Tab),
            press(KeyCode::BackTab, KeyModifiers::SHIFT),
            key(KeyCode::Char('a')),
            ctrl('j'),
        ]
        .iter()
        .for_each(|event| {
            let mut count = Count::default();
            typed(&mut count, "5");
            assert_eq!(count.apply(event), None, "{event:?}");
            assert_eq!(count.pending(), None, "{event:?}");
            assert_eq!(
                count.apply(&key(KeyCode::Char('j'))),
                Some((Action::Down, None)),
                "{event:?}"
            );
        });
    }

    #[test]
    fn the_number_is_given_to_every_movement() {
        let shift = |c| press(KeyCode::Char(c), KeyModifiers::SHIFT);
        [
            (key(KeyCode::Char('j')), Action::Down),
            (key(KeyCode::Down), Action::Down),
            (key(KeyCode::Char('k')), Action::Up),
            (key(KeyCode::Up), Action::Up),
            (ctrl('e'), Action::ScrollDown),
            (ctrl('y'), Action::ScrollUp),
            (key(KeyCode::Char('d')), Action::HalfPageDown),
            (key(KeyCode::Char('u')), Action::HalfPageUp),
            (ctrl('d'), Action::HalfPageDown),
            (ctrl('u'), Action::HalfPageUp),
            (key(KeyCode::Char('g')), Action::First),
            (shift('G'), Action::Last),
            (shift('J'), Action::NextCommit),
            (shift('K'), Action::PreviousCommit),
            (
                press(KeyCode::Down, KeyModifiers::CONTROL),
                Action::NextCommit,
            ),
            (
                press(KeyCode::Up, KeyModifiers::CONTROL),
                Action::PreviousCommit,
            ),
        ]
        .into_iter()
        .for_each(|(event, expected)| {
            let mut count = Count::default();
            typed(&mut count, "3");
            assert_eq!(count.apply(&event), Some((expected, Some(3))), "{event:?}");
        });
    }

    #[test]
    fn key_releases_and_other_events_keep_the_pending_number() {
        let release = Event::Key(KeyEvent::new_with_kind(
            KeyCode::Char('j'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        ));
        let mut count = Count::default();
        typed(&mut count, "5");
        assert_eq!(count.apply(&release), None);
        assert_eq!(count.apply(&Event::Resize(80, 24)), None);
        assert_eq!(count.pending(), Some(5));
    }

    /// Height of the screen, without the status bar.
    const HEIGHT: usize = 20;

    fn all_rows() -> Vec<Row> {
        view::rows(
            &fake::all(),
            110,
            &Theme::monochrome(),
            &DateDisplay::for_snapshots(),
        )
        .unwrap()
    }

    /// Row of the header of each fake commit.
    fn headers() -> Vec<usize> {
        fake::all()
            .iter()
            .scan(0, |start, commit| {
                let first = *start;
                *start += view::rows(
                    std::slice::from_ref(commit),
                    110,
                    &Theme::monochrome(),
                    &DateDisplay::for_snapshots(),
                )
                .unwrap()
                .len();
                Some(first)
            })
            .collect()
    }

    /// `viewport` after `action` repeated `count` times.
    fn navigated(action: Action, count: Option<usize>, viewport: Viewport) -> Viewport {
        let mut viewport = viewport;
        navigate(action, count, &mut viewport, &all_rows(), HEIGHT);
        viewport
    }

    fn viewport(cursor: usize, top: usize) -> Viewport {
        Viewport { cursor, top }
    }

    #[test]
    fn navigate_down_and_up_move_the_cursor_row_by_row() {
        assert_eq!(navigated(Action::Down, None, viewport(3, 0)).cursor, 4);
        assert_eq!(navigated(Action::Up, None, viewport(3, 0)).cursor, 2);
    }

    #[test]
    fn navigate_down_and_up_are_repeated() {
        assert_eq!(navigated(Action::Down, Some(5), viewport(3, 0)).cursor, 8);
        assert_eq!(navigated(Action::Up, Some(2), viewport(3, 0)).cursor, 1);
    }

    #[test]
    fn navigate_down_and_up_stop_at_the_first_and_last_rows() {
        let last = all_rows().len() - 1;
        assert_eq!(
            navigated(Action::Down, Some(usize::MAX), viewport(3, 0)).cursor,
            last
        );
        assert_eq!(
            navigated(Action::Up, Some(usize::MAX), viewport(30, 20)).cursor,
            0
        );
    }

    #[test]
    fn navigate_down_scrolls_to_keep_the_scrolloff() {
        // The 15th row is the 15th row of the screen: the 6th row below it is not displayed.
        assert_eq!(
            navigated(Action::Down, None, viewport(14, 0)),
            viewport(15, 1)
        );
        assert_eq!(
            navigated(Action::Down, Some(30), viewport(0, 0)),
            viewport(30, 16)
        );
    }

    #[test]
    fn navigate_up_scrolls_to_keep_the_scrolloff() {
        assert_eq!(navigated(Action::Up, None, viewport(10, 5)), viewport(9, 4));
    }

    #[test]
    fn navigate_scroll_down_and_up_do_not_apply_the_scrolloff_to_the_cursor() {
        // The cursor is the 3rd row of the screen: it keeps its distance from the top.
        assert_eq!(
            navigated(Action::ScrollDown, None, viewport(12, 10)),
            viewport(13, 11)
        );
        // Then ctrl+y leaves it on its row, now the 4th one of the screen.
        assert_eq!(
            navigated(Action::ScrollUp, None, viewport(13, 11)),
            viewport(13, 10)
        );
    }

    #[test]
    fn navigate_scroll_down_and_up_are_repeated() {
        assert_eq!(
            navigated(Action::ScrollDown, Some(3), viewport(20, 10)),
            viewport(20, 13)
        );
        assert_eq!(
            navigated(Action::ScrollUp, Some(3), viewport(20, 10)),
            viewport(20, 7)
        );
    }

    #[test]
    fn navigate_scroll_stops_at_both_ends() {
        let len = all_rows().len();
        let end = viewport(len - 5, len - HEIGHT);
        assert_eq!(navigated(Action::ScrollDown, Some(10), end), end);
        let start = viewport(5, 0);
        assert_eq!(navigated(Action::ScrollUp, Some(10), start), start);
    }

    #[test]
    fn navigate_half_page_moves_the_screen_and_the_cursor_by_half_the_height() {
        assert_eq!(
            navigated(Action::HalfPageDown, None, viewport(20, 10)),
            viewport(30, 20)
        );
        assert_eq!(
            navigated(Action::HalfPageUp, None, viewport(40, 30)),
            viewport(30, 20)
        );
    }

    #[test]
    fn navigate_half_page_keeps_the_cursor_at_the_same_screen_position() {
        // Half of a screen of 40 rows is 20 rows.
        let mut viewport = viewport(20, 10);
        navigate(Action::HalfPageDown, None, &mut viewport, &all_rows(), 40);
        assert_eq!(viewport, self::viewport(40, 30));
    }

    #[test]
    fn navigate_half_page_rounds_the_half_of_an_odd_height_down() {
        let mut viewport = viewport(20, 10);
        navigate(Action::HalfPageDown, None, &mut viewport, &all_rows(), 21);
        assert_eq!(viewport, self::viewport(30, 20));
    }

    #[test]
    fn navigate_half_page_is_repeated() {
        assert_eq!(
            navigated(Action::HalfPageDown, Some(3), viewport(20, 10)),
            viewport(50, 40)
        );
        assert_eq!(
            navigated(Action::HalfPageUp, Some(2), viewport(40, 30)),
            viewport(20, 10)
        );
    }

    #[test]
    fn navigate_half_page_stops_when_the_last_or_first_row_is_displayed() {
        let len = all_rows().len();
        // The cursor is not moved anymore, even if it is not on the last row.
        let end = viewport(len - 5, len - HEIGHT);
        assert_eq!(navigated(Action::HalfPageDown, None, end), end);
        assert_eq!(navigated(Action::HalfPageDown, Some(3), end), end);
        let start = viewport(5, 0);
        assert_eq!(navigated(Action::HalfPageUp, None, start), start);
        assert_eq!(navigated(Action::HalfPageUp, Some(3), start), start);
    }

    #[test]
    fn navigate_half_page_moves_the_cursor_by_the_scrolled_rows_if_the_screen_stops() {
        let len = all_rows().len();
        // Only 3 rows are left below the screen.
        let top = len - HEIGHT - 3;
        assert_eq!(
            navigated(Action::HalfPageDown, None, viewport(top + 10, top)),
            viewport(len - HEIGHT + 10, len - HEIGHT)
        );
        assert_eq!(
            navigated(Action::HalfPageDown, Some(usize::MAX), viewport(20, 10)),
            viewport(len - 10, len - HEIGHT)
        );
        assert_eq!(
            navigated(Action::HalfPageUp, None, viewport(15, 3)),
            viewport(12, 0)
        );
        assert_eq!(
            navigated(Action::HalfPageUp, Some(usize::MAX), viewport(20, 10)),
            viewport(10, 0)
        );
    }

    #[test]
    fn navigate_first_goes_to_the_first_row() {
        assert_eq!(
            navigated(Action::First, None, viewport(50, 40)),
            viewport(0, 0)
        );
    }

    #[test]
    fn navigate_first_with_a_number_goes_to_that_row_counted_from_the_top() {
        assert_eq!(
            navigated(Action::First, Some(1), viewport(50, 40)),
            viewport(0, 0)
        );
        assert_eq!(
            navigated(Action::First, Some(5), viewport(50, 40)),
            viewport(4, 0)
        );
        assert_eq!(
            navigated(Action::First, Some(30), viewport(0, 0)),
            viewport(29, 15)
        );
    }

    #[test]
    fn navigate_first_with_a_too_large_number_goes_to_the_last_row() {
        let len = all_rows().len();
        assert_eq!(
            navigated(Action::First, Some(len + 1), viewport(0, 0)),
            viewport(len - 1, len - HEIGHT)
        );
        assert_eq!(
            navigated(Action::First, Some(usize::MAX), viewport(0, 0)),
            viewport(len - 1, len - HEIGHT)
        );
    }

    #[test]
    fn navigate_last_goes_to_the_last_row_and_scrolls_just_enough_to_display_it() {
        let len = all_rows().len();
        assert_eq!(
            navigated(Action::Last, None, viewport(0, 0)),
            viewport(len - 1, len - HEIGHT)
        );
        // Already displayed: the screen doesn't move more than needed.
        let almost = viewport(len - 10, len - HEIGHT);
        assert_eq!(
            navigated(Action::Last, None, almost),
            viewport(len - 1, len - HEIGHT)
        );
    }

    #[test]
    fn navigate_last_with_a_number_goes_to_that_row_counted_from_the_top() {
        assert_eq!(
            navigated(Action::Last, Some(1), viewport(50, 40)),
            viewport(0, 0)
        );
        assert_eq!(
            navigated(Action::Last, Some(5), viewport(50, 40)),
            viewport(4, 0)
        );
        // Like `g`.
        assert_eq!(
            navigated(Action::Last, Some(30), viewport(0, 0)),
            navigated(Action::First, Some(30), viewport(0, 0))
        );
    }

    #[test]
    fn navigate_last_with_a_too_large_number_goes_to_the_last_row() {
        let len = all_rows().len();
        assert_eq!(
            navigated(Action::Last, Some(len + 1), viewport(0, 0)),
            viewport(len - 1, len - HEIGHT)
        );
        assert_eq!(
            navigated(Action::Last, Some(usize::MAX), viewport(0, 0)),
            viewport(len - 1, len - HEIGHT)
        );
    }

    #[test]
    fn navigate_next_commit_goes_to_the_header_of_the_next_commit() {
        let headers = headers();
        let rows = all_rows();
        let viewport = navigated(Action::NextCommit, None, viewport(0, 0));
        let mut expected = Viewport::default();
        expected.jump(headers[1], rows.len(), HEIGHT);
        assert_eq!(viewport, expected);
        assert_eq!(
            navigated(Action::NextCommit, None, self::viewport(headers[2] + 3, 0)).cursor,
            headers[3]
        );
    }

    #[test]
    fn navigate_next_commit_from_a_separator_goes_to_the_commit_below_its_own() {
        let headers = headers();
        assert_eq!(
            navigated(Action::NextCommit, None, viewport(headers[4] - 1, 0)).cursor,
            headers[5]
        );
    }

    #[test]
    fn navigate_next_commit_is_repeated_and_stops_on_the_last_commit() {
        let headers = headers();
        assert_eq!(
            navigated(Action::NextCommit, Some(3), viewport(0, 0)).cursor,
            headers[3]
        );
        assert_eq!(
            navigated(Action::NextCommit, Some(usize::MAX), viewport(0, 0)).cursor,
            *headers.last().unwrap()
        );
    }

    #[test]
    fn navigate_next_commit_does_nothing_on_the_last_commit() {
        let last = *headers().last().unwrap();
        let before = viewport(last + 2, last - 5);
        assert_eq!(navigated(Action::NextCommit, None, before), before);
    }

    #[test]
    fn navigate_previous_commit_goes_to_the_header_then_to_the_previous_commit() {
        let headers = headers();
        let middle = viewport(headers[3] + 3, headers[3]);
        let middle = navigated(Action::PreviousCommit, None, middle);
        assert_eq!(middle.cursor, headers[3]);
        assert_eq!(
            navigated(Action::PreviousCommit, None, middle).cursor,
            headers[2]
        );
    }

    #[test]
    fn navigate_previous_commit_from_a_separator_goes_to_the_commit_above_its_own() {
        let headers = headers();
        assert_eq!(
            navigated(Action::PreviousCommit, None, viewport(headers[4] - 1, 0)).cursor,
            headers[3]
        );
    }

    #[test]
    fn navigate_previous_commit_is_repeated_and_stops_on_the_first_commit() {
        let headers = headers();
        let start = viewport(headers[5], headers[5] - 3);
        assert_eq!(
            navigated(Action::PreviousCommit, Some(3), start).cursor,
            headers[2]
        );
        assert_eq!(
            navigated(Action::PreviousCommit, Some(usize::MAX), start).cursor,
            0
        );
    }

    #[test]
    fn navigate_previous_commit_does_nothing_on_the_header_of_the_first_commit() {
        let before = viewport(0, 0);
        assert_eq!(navigated(Action::PreviousCommit, None, before), before);
    }

    #[test]
    fn navigate_jumps_between_commits_scroll_to_keep_the_scrolloff() {
        let headers = headers();
        let viewport = navigated(Action::NextCommit, None, viewport(0, 0));
        assert!(
            viewport.top <= viewport.cursor.saturating_sub(5),
            "{viewport:?}"
        );
        assert!(viewport.cursor + 5 < viewport.top + HEIGHT, "{viewport:?}");
        assert_eq!(viewport.cursor, headers[1]);
    }

    #[test]
    fn navigate_ignores_actions_that_are_not_movements() {
        let before = viewport(20, 10);
        [Action::Quit, Action::Suspend, Action::Digit(3)]
            .into_iter()
            .for_each(|action| assert_eq!(navigated(action, None, before), before));
    }

    #[test]
    fn navigate_without_rows_does_nothing() {
        [
            Action::Down,
            Action::Last,
            Action::NextCommit,
            Action::HalfPageDown,
        ]
        .into_iter()
        .for_each(|action| {
            let mut viewport = Viewport::default();
            navigate(action, Some(3), &mut viewport, &[], HEIGHT);
            assert_eq!(viewport, Viewport::default(), "{action:?}");
        });
    }
}
