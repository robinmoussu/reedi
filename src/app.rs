//! Terminal handling and event loop.

use std::io;

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

use crate::fake;
use crate::model::Commit;
use crate::view;
use crate::view::Mode;
use crate::view::Row;
use crate::viewport::Viewport;
use crate::wrap::TooNarrow;

/// What a key press does in review mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Down,
    Up,
    Quit,
    Suspend,
}

/// Display the commits until `q` or `ctrl+c` is pressed.
///
/// Like `less`, the alternate screen is used, and the screen is redrawn when its size changes.
/// Fails if a text area outside of the chunks is too narrow to display a grapheme cluster of its
/// line.
pub fn run() -> io::Result<()> {
    let commits = fake::commits();
    let mut rows = Vec::new();
    let mut width = None;
    let mut viewport = Viewport::default();
    ratatui::run(|terminal| {
        loop {
            let screen_width = terminal.size()?.width;
            if width != Some(screen_width) {
                rows = rewrap(&commits, &rows, &mut viewport, screen_width)
                    .map_err(io::Error::other)?;
                width = Some(screen_width);
            }
            terminal.draw(|frame| view::draw(frame, &rows, &mut viewport, Mode::Review))?;
            match action(&event::read()?) {
                Some(Action::Down) => viewport.down(rows.len()),
                Some(Action::Up) => viewport.up(),
                Some(Action::Quit) => return Ok(()),
                Some(Action::Suspend) => suspend(terminal)?,
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
) -> Result<Vec<Row>, TooNarrow> {
    let new_rows = view::rows(commits, width)?;
    if let Some(row) = rows.get(viewport.cursor) {
        viewport.move_cursor(view::find(&new_rows, row.anchor()));
    }
    Ok(new_rows)
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
        let commits = fake::commits();
        let rows = view::rows(&commits, 110).unwrap();
        let mut viewport = Viewport::default();
        viewport.down(rows.len());
        // The header of the first commit is displayed on 2 rows.
        assert_eq!(viewport.cursor, 1);
        assert_eq!(rows[1].line, rows[0].line);
    }

    #[test]
    fn first_rows_keep_the_cursor_on_the_first_row() {
        let mut viewport = Viewport::default();
        let rows = rewrap(&fake::commits(), &[], &mut viewport, 110).unwrap();
        assert!(!rows.is_empty());
        assert_eq!(viewport, Viewport::default());
    }

    #[test]
    fn rewrapping_keeps_the_cursor_on_the_first_character_of_its_row() {
        let commits = fake::commits();
        let narrow = view::rows(&commits, 60).unwrap();
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
        let wide = rewrap(&commits, &narrow, &mut viewport, 110).unwrap();
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
        assert!(rewrap(&fake::commits(), &[], &mut viewport, 0).is_err());
    }
}
