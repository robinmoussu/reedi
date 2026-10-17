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
use crate::view;
use crate::view::Mode;
use crate::viewport::Viewport;

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
pub fn run() -> io::Result<()> {
    let commits = fake::commits();
    let rows = view::rows(&commits);
    let mut viewport = Viewport::default();
    ratatui::run(|terminal| {
        loop {
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
}
