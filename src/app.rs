//! Terminal handling and event loop.

use std::io;

use ratatui::crossterm::event;
use ratatui::crossterm::event::Event;

use crate::fake;
use crate::view;
use crate::view::Mode;

/// Display the commit until a key is pressed.
///
/// Like `less`, the alternate screen is used, and the screen is redrawn when its size changes.
pub fn run() -> io::Result<()> {
    let commit = fake::commit();
    ratatui::run(|terminal| {
        loop {
            terminal.draw(|frame| view::draw(frame, &commit, Mode::Review))?;
            if should_exit(&event::read()?) {
                return Ok(());
            }
        }
    })
}

fn should_exit(event: &Event) -> bool {
    event.is_key_press()
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::KeyCode;
    use ratatui::crossterm::event::KeyEvent;
    use ratatui::crossterm::event::KeyEventKind;
    use ratatui::crossterm::event::KeyModifiers;

    use super::*;

    fn key(kind: KeyEventKind) -> Event {
        Event::Key(KeyEvent::new_with_kind(
            KeyCode::Char('a'),
            KeyModifiers::NONE,
            kind,
        ))
    }

    #[test]
    fn any_key_press_exits() {
        assert!(should_exit(&key(KeyEventKind::Press)));
    }

    #[test]
    fn key_release_does_not_exit() {
        assert!(!should_exit(&key(KeyEventKind::Release)));
    }

    #[test]
    fn resize_does_not_exit() {
        assert!(!should_exit(&Event::Resize(80, 24)));
    }
}
