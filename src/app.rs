//! Terminal handling and event loop.

use std::io;
use std::panic;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::thread;
use std::thread::Scope;
use std::time::Duration;

use nix::sys::signal;
use nix::sys::signal::Signal;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event;
use ratatui::crossterm::event::DisableMouseCapture;
use ratatui::crossterm::event::EnableMouseCapture;
use ratatui::crossterm::event::Event;
use ratatui::crossterm::event::KeyCode;
use ratatui::crossterm::event::KeyModifiers;
use ratatui::crossterm::event::MouseButton;
use ratatui::crossterm::event::MouseEvent;
use ratatui::crossterm::event::MouseEventKind;
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal;
use ratatui::crossterm::terminal::EnterAlternateScreen;
use reedi_crossterm_compat::PointerShape;
use reedi_crossterm_compat::SetPointerShape;

use crate::config::UnknownSetting;
use crate::date::DateDisplay;
use crate::model::Revision;
use crate::settings::Settings;
use crate::theme::Theme;
use crate::view;
use crate::view::Anchor;
use crate::view::Content;
use crate::view::ErrorModal;
use crate::view::Mode;
use crate::view::Row;
use crate::view::RowsError;
use crate::view::SplitRatio;
use crate::viewport::Viewport;
use crate::wrap::TooNarrow;

/// How long to wait for a terminal event before checking whether the rows being rewrapped are
/// built.
const REWRAP_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// What a key press or a mouse event does in review mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Down,
    Up,
    /// `ctrl+e` or mouse wheel down.
    ScrollDown,
    /// `ctrl+y` or mouse wheel up.
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
    NextRevision,
    /// `K` or `ctrl+up`.
    PreviousRevision,
    /// A left click on the row of this index.
    SetCursor(usize),
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
    /// nothing. Any other key press, and the mouse events other than moves, cancel the number; key
    /// releases, mouse moves and other events (like resizing the screen) don't.
    fn apply(&mut self, event: &Event) -> Option<(Action, Option<usize>)> {
        if let Event::Mouse(mouse) = event
            && mouse.kind != MouseEventKind::Moved
        {
            self.0 = None;
        }
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
    let repeat_count = count.unwrap_or(1);
    match action {
        Action::Down => viewport.move_down(repeat_count, len, height),
        Action::Up => viewport.move_up(repeat_count, height),
        Action::ScrollDown => viewport.scroll_down(repeat_count, len, height),
        Action::ScrollUp => viewport.scroll_up(repeat_count, height),
        Action::HalfPageDown => viewport.half_page_down(repeat_count, len, height),
        Action::HalfPageUp => viewport.half_page_up(repeat_count, height),
        Action::First => viewport.jump(repeat_count.saturating_sub(1), len, height),
        Action::Last => {
            let row_index = count.map_or(len.saturating_sub(1), |row_number| {
                row_number.saturating_sub(1)
            });
            viewport.jump(row_index, len, height);
        }
        Action::NextRevision => viewport.jump_and_scroll_to_top_margin(
            view::next_revision(rows, viewport.cursor, repeat_count),
            len,
            height,
        ),
        Action::PreviousRevision => viewport.jump_and_scroll_to_top_margin(
            view::previous_revision(rows, viewport.cursor, repeat_count),
            len,
            height,
        ),
        Action::SetCursor(row_index) => viewport.cursor = row_index,
        Action::Digit(_) | Action::Quit | Action::Suspend => {}
    }
}

/// Display `revisions` with `settings` until `q` or `ctrl+c` is pressed, and the
/// `unknown_settings` of the config file (its path, and at least one setting) in an error modal
/// over them, if any, until a key is pressed or the mouse is clicked.
///
/// Like `less`, the alternate screen is used, and the screen is redrawn when its size changes.
/// The rows are rewrapped in the background, the previous ones being displayed meanwhile. The
/// mouse events are captured while running. Fails if a text area outside of the hunks is too
/// narrow to display a grapheme cluster of its line.
pub fn run(
    revisions: &[Revision],
    settings: &Settings,
    unknown_settings: Option<(PathBuf, Vec<UnknownSetting>)>,
) -> io::Result<()> {
    let mut error_modal = unknown_settings
        .map(|(path, unknown_settings)| ErrorModal::UnknownSettings { path, unknown_settings });
    let mut rows = Vec::new();
    let mut size = None;
    let min_half_width = view::min_half_width(revisions);
    let mut mouse = Mouse::default();
    let mut displayed_pointer_shape = PointerShape::Default;
    let mut viewport = Viewport::default();
    let mut count = Count::default();
    let panic_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = release_mouse();
        panic_hook(info);
    }));
    execute!(io::stdout(), EnableMouseCapture)?;
    let result = ratatui::run(|terminal| {
        thread::scope(|scope| {
            let mut rewrapper = Rewrapper::new(scope, revisions, &settings.theme, &settings.dates);
            loop {
                let screen = terminal.size()?;
                let height = usize::from(screen.height.saturating_sub(view::STATUS_BAR_HEIGHT));
                let widths = Widths {
                    width: screen.width,
                    left_width: mouse
                        .displayed_ratio()
                        .left_width(screen.width, min_half_width),
                };
                let new_rows = rewrapper.poll_rows_for(widths).map_err(io::Error::other)?;
                let rows_replaced = new_rows.is_some();
                if let Some(new_rows) = new_rows {
                    replace_rows(&mut rows, new_rows, &mut viewport, mouse.drag_anchor);
                }
                if !rewrapper.is_running() {
                    mouse.forget_released_drag();
                }
                if rows_replaced || size != Some(screen) {
                    viewport.resize(rows.len(), height);
                    size = Some(screen);
                }
                terminal.draw(|frame| {
                    view::draw(
                        frame,
                        revisions,
                        &rows,
                        &mut viewport,
                        widths.left_width,
                        Mode::Review,
                        count.pending(),
                        error_modal.as_ref(),
                        &settings.theme,
                    )
                })?;
                if rewrapper.is_running() && !event::poll(REWRAP_POLL_INTERVAL)? {
                    continue;
                }
                let event = event::read()?;
                if error_modal.is_some() {
                    close_on_key_press_or_click(&event, &mut error_modal);
                    continue;
                }
                if let Event::Mouse(mouse_event) = event {
                    if let Some(action) = mouse.apply(&mouse_event, &rows, viewport, widths, height)
                    {
                        navigate(action, None, &mut viewport, &rows, height);
                    }
                    if mouse.pointer_shape != displayed_pointer_shape {
                        execute!(io::stdout(), SetPointerShape(mouse.pointer_shape))?;
                        displayed_pointer_shape = mouse.pointer_shape;
                    }
                }
                match count.apply(&event) {
                    Some((Action::Quit, _)) => return Ok(()),
                    Some((Action::Suspend, _)) => {
                        suspend(terminal)?;
                        displayed_pointer_shape = PointerShape::Default;
                    }
                    Some((action, count)) => navigate(action, count, &mut viewport, &rows, height),
                    None => {}
                }
            }
        })
    });
    release_mouse()?;
    result
}

/// Stop receiving the mouse events, and give the mouse pointer its default shape back.
fn release_mouse() -> io::Result<()> {
    execute!(
        io::stdout(),
        DisableMouseCapture,
        SetPointerShape(PointerShape::Default)
    )
}

/// The widths the rows are wrapped for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Widths {
    /// Width of the screen.
    width: u16,
    /// Width of the left half of the split rows.
    left_width: u16,
}

/// Rows being built in the background. Dropping it cancels it.
struct RewrapJob {
    widths: Widths,
    cancellation: Arc<AtomicBool>,
    result: Receiver<Result<Vec<Row>, RowsError>>,
}

impl RewrapJob {
    /// Start building, in a thread of `scope`, the rows displaying `revisions` for `widths`.
    fn spawn<'scope, 'env>(
        scope: &'scope Scope<'scope, 'env>,
        revisions: &'env [Revision],
        widths: Widths,
        theme: &'env Theme,
        dates: &'env DateDisplay,
    ) -> Self {
        let cancellation = Arc::new(AtomicBool::new(false));
        let (sender, result) = mpsc::channel();
        let thread_cancellation = Arc::clone(&cancellation);
        scope.spawn(move || {
            let rows = view::rows(
                revisions,
                widths.width,
                widths.left_width,
                theme,
                dates,
                &thread_cancellation,
            );
            // The job may have been dropped.
            let _ = sender.send(rows);
        });
        Self { widths, cancellation, result }
    }
}

impl Drop for RewrapJob {
    fn drop(&mut self) {
        self.cancellation.store(true, Ordering::Relaxed);
    }
}

/// Rewraps the rows in the background when the widths they are displayed with change.
struct Rewrapper<'scope, 'env> {
    scope: &'scope Scope<'scope, 'env>,
    revisions: &'env [Revision],
    theme: &'env Theme,
    dates: &'env DateDisplay,
    /// The widths the displayed rows are wrapped for, if any.
    wrapped_widths: Option<Widths>,
    job: Option<RewrapJob>,
}

impl<'scope, 'env> Rewrapper<'scope, 'env> {
    /// A rewrapper of the rows displaying `revisions`, in the threads of `scope`.
    fn new(
        scope: &'scope Scope<'scope, 'env>,
        revisions: &'env [Revision],
        theme: &'env Theme,
        dates: &'env DateDisplay,
    ) -> Self {
        Self {
            scope,
            revisions,
            theme,
            dates,
            wrapped_widths: None,
            job: None,
        }
    }

    /// Make progress on rewrapping the rows for `widths`, and return them once they are built, if
    /// they are not displayed yet. A running rewrap for other widths is cancelled.
    ///
    /// Fails if a text area outside of the hunks is too narrow to display a grapheme cluster of
    /// its line.
    fn poll_rows_for(&mut self, widths: Widths) -> Result<Option<Vec<Row>>, TooNarrow> {
        self.start_rewrapping_for(widths);
        let Some(result) = self.job.as_ref().and_then(|job| job.result.try_recv().ok()) else {
            return Ok(None);
        };
        self.job = None;
        match result {
            Ok(rows) => {
                self.wrapped_widths = Some(widths);
                Ok(Some(rows))
            }
            Err(RowsError::TooNarrow(error)) => Err(error),
            // Only a dropped job is cancelled.
            Err(RowsError::Cancelled) => Ok(None),
        }
    }

    /// Start rewrapping the rows for `widths`, unless the displayed rows are wrapped for them or
    /// are being rewrapped for them. A running rewrap for other widths is cancelled.
    fn start_rewrapping_for(&mut self, widths: Widths) {
        if self.wrapped_widths == Some(widths) {
            self.job = None;
        } else if self.job.as_ref().is_none_or(|job| job.widths != widths) {
            self.job = Some(RewrapJob::spawn(
                self.scope,
                self.revisions,
                widths,
                self.theme,
                self.dates,
            ));
        }
    }

    fn is_running(&self) -> bool {
        self.job.is_some()
    }
}

/// Replace `rows` by `new_rows`, displaying the same lines. The cursor goes, at the same screen
/// position, on the row displaying `anchor`, or if there is none, the character that was the
/// first of its row (see [`Row::anchor()`]).
fn replace_rows(
    rows: &mut Vec<Row>,
    new_rows: Vec<Row>,
    viewport: &mut Viewport,
    anchor: Option<Anchor>,
) {
    let anchor = anchor.or_else(|| rows.get(viewport.cursor).map(Row::anchor));
    if let Some(anchor) = anchor {
        viewport.move_cursor(view::find(&new_rows, anchor));
    }
    *rows = new_rows;
}

/// The ratio of the split rows, which is changed by dragging their separator with the mouse, and
/// the shape of the mouse pointer.
#[derive(Debug, Default)]
struct Mouse {
    ratio: SplitRatio,
    /// The ratio displayed while the separator is dragged, applied when it is released.
    dragged_ratio: Option<SplitRatio>,
    /// The character the cursor was on when the drag started, which it goes back on each time the
    /// rows are rewrapped, until they are wrapped for the applied ratio.
    drag_anchor: Option<Anchor>,
    pointer_shape: PointerShape,
}

impl Mouse {
    fn displayed_ratio(&self) -> SplitRatio {
        self.dragged_ratio.unwrap_or(self.ratio)
    }

    /// Apply `event` to `rows`, displayed from the row `viewport.top` with `widths`, on a screen
    /// `height` rows tall (without the status bar). Return the action triggered by the wheel or
    /// by a left click on a row, if any.
    ///
    /// Pressing the left button on the separator of a split row starts dragging it: the separator
    /// follows the mouse until the button is released. The pointer has the
    /// [`PointerShape::ColResize`] shape over the separator and while dragging it. Pressing it
    /// anywhere else on a row sets the cursor on that row.
    fn apply(
        &mut self,
        event: &MouseEvent,
        rows: &[Row],
        viewport: Viewport,
        widths: Widths,
        height: usize,
    ) -> Option<Action> {
        let row = usize::from(event.row);
        let header_rows_count =
            view::sticky_header(rows, viewport, height).map_or(0, |header| header.rule_row + 1);
        let is_on_separator = event.column == widths.left_width
            && row >= header_rows_count
            && row < height
            && rows
                .get(viewport.top + row)
                .is_some_and(|row| matches!(row.content, Content::Split { .. }));
        let action = match event.kind {
            MouseEventKind::Down(MouseButton::Left) if is_on_separator => {
                self.dragged_ratio = Some(self.ratio);
                self.drag_anchor = rows.get(viewport.cursor).map(Row::anchor);
                None
            }
            MouseEventKind::Drag(MouseButton::Left) if self.dragged_ratio.is_some() => {
                self.dragged_ratio = Some(SplitRatio::new(event.column, widths.width));
                None
            }
            MouseEventKind::Up(MouseButton::Left) if self.dragged_ratio.is_some() => {
                self.ratio = SplitRatio::new(event.column, widths.width);
                self.dragged_ratio = None;
                None
            }
            MouseEventKind::Down(MouseButton::Left)
                if row < height && viewport.top + row < rows.len() =>
            {
                Some(Action::SetCursor(viewport.top + row))
            }
            MouseEventKind::ScrollDown => Some(Action::ScrollDown),
            MouseEventKind::ScrollUp => Some(Action::ScrollUp),
            _ => None,
        };
        self.pointer_shape = if self.dragged_ratio.is_some() || is_on_separator {
            PointerShape::ColResize
        } else {
            PointerShape::Default
        };
        action
    }

    /// Forget the character the cursor was on when the drag started, unless the separator is still
    /// dragged. To call once the rows are wrapped for the applied ratio.
    fn forget_released_drag(&mut self) {
        if self.dragged_ratio.is_none() {
            self.drag_anchor = None;
        }
    }
}

/// Close `error_modal` if `event` is a key press or a mouse click.
fn close_on_key_press_or_click(event: &Event, error_modal: &mut Option<ErrorModal>) {
    let is_click =
        matches!(event, Event::Mouse(mouse) if matches!(mouse.kind, MouseEventKind::Down(_)));
    if matches!(event, Event::Key(key) if key.is_press()) || is_click {
        *error_modal = None;
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
        | (KeyCode::Down, KeyModifiers::CONTROL) => Some(Action::NextRevision),
        (KeyCode::Char('K'), KeyModifiers::NONE | KeyModifiers::SHIFT)
        | (KeyCode::Up, KeyModifiers::CONTROL) => Some(Action::PreviousRevision),
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
    release_mouse()?;
    ratatui::try_restore()?;
    signal::raise(Signal::SIGTSTP)?;
    // The process is stopped until it receives `SIGCONT`.
    terminal::enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    // The content of the alternate screen may have been modified while suspended.
    terminal.clear()
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::KeyEvent;
    use ratatui::crossterm::event::KeyEventKind;

    use super::*;
    use crate::fake;
    use crate::model::Commit;
    use crate::settings::SCROLLOFF;
    use crate::test_support::fifty_fifty_left_width;
    use crate::test_support::header_row_indexes;
    use crate::test_support::rows_of;
    use crate::test_support::rows_of_revisions;
    use crate::test_support::short_commits;
    use crate::view::AnchorOffset;

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
        assert_eq!(
            action(&Event::Resize(/* columns */ 80, /* rows */ 24)),
            None
        );
    }

    #[test]
    fn cursor_moves_row_by_row() {
        let commits = vec![fake::file_changes()];
        let rows = rows_of(&commits, /* width */ 110);
        let mut viewport = Viewport::default();
        viewport.down(rows.len());
        // The header of the first commit is displayed on 2 rows.
        assert_eq!(viewport.cursor, 1);
        assert_eq!(rows[1].line_index, rows[0].line_index);
    }

    /// The widths of a screen `width` columns wide, split 50/50.
    fn fifty_fifty(width: u16) -> Widths {
        Widths {
            width,
            left_width: fifty_fifty_left_width(width),
        }
    }

    /// Run `test` with a rewrapper of `revisions`.
    fn with_rewrapper(revisions: &[Revision], test: impl FnOnce(&mut Rewrapper)) {
        let theme = Theme::monochrome();
        let dates = DateDisplay::for_snapshots();
        thread::scope(|scope| test(&mut Rewrapper::new(scope, revisions, &theme, &dates)));
    }

    /// Poll `rewrapper` for the rows wrapped for `widths` until its rewrap is finished (see
    /// [`Rewrapper::poll_rows_for()`]).
    fn poll_rows_until_finished(
        rewrapper: &mut Rewrapper,
        widths: Widths,
    ) -> Result<Option<Vec<Row>>, TooNarrow> {
        loop {
            let rows = rewrapper.poll_rows_for(widths)?;
            if rows.is_some() || !rewrapper.is_running() {
                return Ok(rows);
            }
        }
    }

    /// The cancellation of the rewrap running in `rewrapper`.
    fn cancellation(rewrapper: &Rewrapper) -> Arc<AtomicBool> {
        let job = rewrapper.job.as_ref().expect("a rewrap is running");
        Arc::clone(&job.cancellation)
    }

    #[test]
    fn rows_are_polled_once_for_each_width() {
        // Its content does not matter.
        let revisions = [Revision::Commit(fake::ltr_then_rtl_summary())];
        with_rewrapper(&revisions, |rewrapper| {
            assert!(
                poll_rows_until_finished(rewrapper, fifty_fifty(/* width */ 110))
                    .unwrap()
                    .is_some()
            );
            assert!(
                rewrapper
                    .poll_rows_for(fifty_fifty(/* width */ 110))
                    .unwrap()
                    .is_none()
            );
            assert!(!rewrapper.is_running());
            assert!(
                poll_rows_until_finished(rewrapper, fifty_fifty(/* width */ 60))
                    .unwrap()
                    .is_some()
            );
        });
    }

    #[test]
    fn polling_the_rows_for_a_too_narrow_screen_is_an_error() {
        // Its content does not matter.
        let revisions = [Revision::Commit(fake::ltr_then_rtl_summary())];
        with_rewrapper(&revisions, |rewrapper| {
            assert!(poll_rows_until_finished(rewrapper, fifty_fifty(/* width */ 0)).is_err());
        });
    }

    #[test]
    fn rewrapping_for_the_widths_being_rewrapped_keeps_the_running_rewrap() {
        // Its content does not matter.
        let revisions = [Revision::Commit(fake::ltr_then_rtl_summary())];
        with_rewrapper(&revisions, |rewrapper| {
            rewrapper.start_rewrapping_for(fifty_fifty(/* width */ 110));
            let running = cancellation(rewrapper);
            rewrapper.start_rewrapping_for(fifty_fifty(/* width */ 110));
            assert!(Arc::ptr_eq(&running, &cancellation(rewrapper)));
            assert!(!running.load(Ordering::Relaxed));
        });
    }

    #[test]
    fn polling_the_rows_for_other_widths_cancels_the_running_rewrap() {
        // Its content does not matter.
        let revisions = [Revision::Commit(fake::ltr_then_rtl_summary())];
        with_rewrapper(&revisions, |rewrapper| {
            rewrapper.start_rewrapping_for(fifty_fifty(/* width */ 60));
            let running = cancellation(rewrapper);
            let rows = poll_rows_until_finished(rewrapper, fifty_fifty(/* width */ 110)).unwrap();
            assert!(running.load(Ordering::Relaxed));
            assert_eq!(rows, Some(rows_of_revisions(&revisions, /* width */ 110)));
        });
    }

    #[test]
    fn replacing_the_rows_at_start_up_keeps_the_cursor_on_the_first_row() {
        // Its content does not matter.
        let new_rows = rows_of(&[fake::ltr_then_rtl_summary()], /* width */ 110);
        let mut rows = Vec::new();
        let mut viewport = Viewport::default();
        replace_rows(&mut rows, new_rows, &mut viewport, /* anchor */ None);
        assert!(!rows.is_empty());
        assert_eq!(viewport, Viewport::default());
    }

    #[test]
    fn replacing_the_rows_keeps_the_cursor_on_the_first_character_of_its_row() {
        let commits = vec![fake::file_changes()];
        let narrow = rows_of(&commits, /* width */ 60);
        // The second row of the first line of the body (the header is the line 0, then a blank
        // line).
        let cursor = narrow
            .iter()
            .position(|row| row.line_index == 2 && row.anchor_offset != AnchorOffset::Whole(0))
            .unwrap();
        let mut rows = narrow.clone();
        let mut viewport = Viewport { cursor, top: cursor - 3 };
        replace_rows(
            &mut rows,
            rows_of(&commits, /* width */ 110),
            &mut viewport,
            /* anchor */ None,
        );
        // The first line of the body is displayed on a single row.
        assert_eq!(rows[viewport.cursor].line_index, 2);
        assert_eq!(viewport.cursor, view::find(&rows, narrow[cursor].anchor()));
        // At the same screen position.
        assert_eq!(viewport.cursor - viewport.top, 3);
    }

    #[test]
    fn replacing_the_rows_puts_the_cursor_on_the_given_anchor() {
        let commits = vec![fake::file_changes()];
        let narrow = rows_of(&commits, /* width */ 60);
        // A row of the body other than the one of the cursor, as the anchor of a drag that
        // started before the cursor moved there.
        let anchor = narrow[2].anchor();
        let mut rows = narrow.clone();
        let mut viewport = Viewport { cursor: 5, top: 0 };
        replace_rows(
            &mut rows,
            rows_of(&commits, /* width */ 110),
            &mut viewport,
            Some(anchor),
        );
        assert_eq!(viewport.cursor, view::find(&rows, anchor));
    }

    fn mouse_event(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    /// The rows of a commit with a hunk, and the screen row of one of its split rows, which fits
    /// on a screen `MOUSE_SCREEN_HEIGHT` rows tall displaying them from the first row.
    fn rows_with_a_split_row() -> (Vec<Row>, u16) {
        let rows = rows_of(&[fake::file_changes()], MOUSE_SCREEN_WIDTH);
        let split_row = rows
            .iter()
            .position(|row| matches!(row.content, Content::Split { .. }))
            .unwrap();
        assert!(split_row < MOUSE_SCREEN_HEIGHT);
        (rows, u16::try_from(split_row).unwrap())
    }

    const MOUSE_SCREEN_WIDTH: u16 = 110;
    const MOUSE_SCREEN_HEIGHT: usize = 40;

    /// Apply `event` to `mouse`, over `rows` displayed from the first row of a screen
    /// `MOUSE_SCREEN_WIDTH` × `MOUSE_SCREEN_HEIGHT`, with its split rows at their displayed ratio.
    fn apply(mouse: &mut Mouse, event: MouseEvent, rows: &[Row]) -> Option<Action> {
        apply_at(mouse, event, rows, Viewport::default())
    }

    /// Like [`apply()`], with `rows` displayed from the row `viewport.top`.
    fn apply_at(
        mouse: &mut Mouse,
        event: MouseEvent,
        rows: &[Row],
        viewport: Viewport,
    ) -> Option<Action> {
        let widths = Widths {
            width: MOUSE_SCREEN_WIDTH,
            left_width: displayed_left_width(mouse),
        };
        mouse.apply(&event, rows, viewport, widths, MOUSE_SCREEN_HEIGHT)
    }

    fn displayed_left_width(mouse: &Mouse) -> u16 {
        mouse
            .displayed_ratio()
            .left_width(MOUSE_SCREEN_WIDTH, /* min_half_width */ 0)
    }

    fn applied_left_width(mouse: &Mouse) -> u16 {
        mouse
            .ratio
            .left_width(MOUSE_SCREEN_WIDTH, /* min_half_width */ 0)
    }

    const LEFT: MouseButton = MouseButton::Left;

    /// The screen row of the header of the first commit, which is not split.
    const HEADER_ROW: u16 = 0;

    #[test]
    fn dragging_the_separator_moves_it_and_releasing_it_applies_the_ratio() {
        let (rows, split_row) = rows_with_a_split_row();
        let mut mouse = Mouse::default();
        let separator = displayed_left_width(&mouse);
        apply(
            &mut mouse,
            mouse_event(MouseEventKind::Down(LEFT), separator, split_row),
            &rows,
        );
        apply(
            &mut mouse,
            mouse_event(MouseEventKind::Drag(LEFT), 40, split_row),
            &rows,
        );
        assert_eq!(displayed_left_width(&mouse), 40);
        assert_eq!(applied_left_width(&mouse), separator);
        apply(
            &mut mouse,
            mouse_event(MouseEventKind::Up(LEFT), 30, split_row),
            &rows,
        );
        assert_eq!(displayed_left_width(&mouse), 30);
        assert_eq!(applied_left_width(&mouse), 30);
    }

    #[test]
    fn only_the_separator_of_a_displayed_split_row_can_be_dragged() {
        let (rows, split_row) = rows_with_a_split_row();
        let separator = displayed_left_width(&Mouse::default());
        let below_the_screen = u16::try_from(MOUSE_SCREEN_HEIGHT).unwrap();
        for (column, row) in [
            (separator - 1, split_row),
            (separator, HEADER_ROW),
            (separator, below_the_screen),
        ] {
            let mut mouse = Mouse::default();
            apply(
                &mut mouse,
                mouse_event(MouseEventKind::Down(LEFT), column, row),
                &rows,
            );
            apply(
                &mut mouse,
                mouse_event(MouseEventKind::Drag(LEFT), 40, split_row),
                &rows,
            );
            assert_eq!(displayed_left_width(&mouse), separator, "{column}, {row}");
        }
    }

    #[test]
    fn separator_cannot_be_dragged_over_the_sticky_file_name() {
        let (rows, split_row) = rows_with_a_split_row();
        // The sticky file name and its rule cover the first 2 screen rows, which display split
        // rows, like the row below them.
        let viewport = Viewport { cursor: 0, top: split_row.into() };
        assert!(
            rows[viewport.top..viewport.top + 3]
                .iter()
                .all(|row| matches!(row.content, Content::Split { .. }))
        );
        let separator = displayed_left_width(&Mouse::default());
        let is_dragged_after_press_on = |row| {
            let mut mouse = Mouse::default();
            apply_at(
                &mut mouse,
                mouse_event(MouseEventKind::Down(LEFT), separator, row),
                &rows,
                viewport,
            );
            mouse.dragged_ratio.is_some() && mouse.pointer_shape == PointerShape::ColResize
        };
        assert!(!is_dragged_after_press_on(0));
        assert!(!is_dragged_after_press_on(1));
        assert!(is_dragged_after_press_on(2));
    }

    #[test]
    fn mouse_wheel_scrolls_the_screen_over_the_sticky_file_name() {
        let (rows, split_row) = rows_with_a_split_row();
        let scroll = |kind| {
            apply_at(
                &mut Mouse::default(),
                mouse_event(kind, 0, 0),
                &rows,
                Viewport { cursor: 0, top: split_row.into() },
            )
        };
        assert_eq!(scroll(MouseEventKind::ScrollDown), Some(Action::ScrollDown));
        assert_eq!(scroll(MouseEventKind::ScrollUp), Some(Action::ScrollUp));
    }

    #[test]
    fn pointer_is_a_column_resize_one_over_the_separator_and_while_dragging_it() {
        let (rows, split_row) = rows_with_a_split_row();
        let mut mouse = Mouse::default();
        let separator = displayed_left_width(&mouse);
        apply(
            &mut mouse,
            mouse_event(MouseEventKind::Moved, separator, split_row),
            &rows,
        );
        assert_eq!(mouse.pointer_shape, PointerShape::ColResize);
        apply(
            &mut mouse,
            mouse_event(MouseEventKind::Moved, 40, split_row),
            &rows,
        );
        assert_eq!(mouse.pointer_shape, PointerShape::Default);
        apply(
            &mut mouse,
            mouse_event(MouseEventKind::Down(LEFT), separator, split_row),
            &rows,
        );
        // The separator follows the mouse with a delay: the next event is not over it.
        apply(
            &mut mouse,
            mouse_event(MouseEventKind::Drag(LEFT), 0, HEADER_ROW),
            &rows,
        );
        assert_eq!(mouse.pointer_shape, PointerShape::ColResize);
        apply(
            &mut mouse,
            mouse_event(MouseEventKind::Up(LEFT), 30, HEADER_ROW),
            &rows,
        );
        assert_eq!(mouse.pointer_shape, PointerShape::Default);
    }

    #[test]
    fn left_click_on_a_row_sets_the_cursor_on_it() {
        let (rows, _) = rows_with_a_split_row();
        let viewport = Viewport { cursor: 0, top: 2 };
        assert_eq!(
            apply_at(
                &mut Mouse::default(),
                mouse_event(MouseEventKind::Down(LEFT), 0, 3),
                &rows,
                viewport
            ),
            Some(Action::SetCursor(5))
        );
    }

    #[test]
    fn left_click_on_the_separator_does_not_set_the_cursor() {
        let (rows, split_row) = rows_with_a_split_row();
        let mut mouse = Mouse::default();
        let separator = displayed_left_width(&mouse);
        assert_eq!(
            apply(
                &mut mouse,
                mouse_event(MouseEventKind::Down(LEFT), separator, split_row),
                &rows
            ),
            None
        );
    }

    #[test]
    fn left_click_below_the_last_row_or_on_the_status_bar_does_nothing() {
        let (rows, _) = rows_with_a_split_row();
        // Only the last row is displayed, on the first row of the screen.
        let viewport = Viewport { cursor: 0, top: rows.len() - 1 };
        let status_bar_row = u16::try_from(MOUSE_SCREEN_HEIGHT).unwrap();
        for row in [1, status_bar_row] {
            assert_eq!(
                apply_at(
                    &mut Mouse::default(),
                    mouse_event(MouseEventKind::Down(LEFT), 0, row),
                    &rows,
                    viewport
                ),
                None,
                "{row}"
            );
        }
    }

    #[test]
    fn mouse_wheel_scrolls_the_screen_like_ctrl_e_and_ctrl_y() {
        let (rows, _) = rows_with_a_split_row();
        let mut mouse = Mouse::default();
        assert_eq!(
            apply(
                &mut mouse,
                mouse_event(MouseEventKind::ScrollDown, 0, 0),
                &rows
            ),
            Some(Action::ScrollDown)
        );
        assert_eq!(
            apply(
                &mut mouse,
                mouse_event(MouseEventKind::ScrollUp, 0, 0),
                &rows
            ),
            Some(Action::ScrollUp)
        );
    }

    #[test]
    fn drag_anchor_is_the_cursor_row_and_is_kept_until_the_drag_is_released() {
        let (rows, split_row) = rows_with_a_split_row();
        let mut mouse = Mouse::default();
        let separator = displayed_left_width(&mouse);
        apply(
            &mut mouse,
            mouse_event(MouseEventKind::Down(LEFT), separator, split_row),
            &rows,
        );
        assert_eq!(mouse.drag_anchor, Some(rows[0].anchor()));
        mouse.forget_released_drag();
        assert_eq!(mouse.drag_anchor, Some(rows[0].anchor()));
        apply(
            &mut mouse,
            mouse_event(MouseEventKind::Up(LEFT), 30, split_row),
            &rows,
        );
        assert_eq!(mouse.drag_anchor, Some(rows[0].anchor()));
        mouse.forget_released_drag();
        assert_eq!(mouse.drag_anchor, None);
    }

    fn error_modal() -> Option<ErrorModal> {
        Some(ErrorModal::UnknownSettings {
            path: "config.toml".into(),
            unknown_settings: Vec::new(),
        })
    }

    #[test]
    fn key_press_closes_the_error_modal() {
        let mut error_modal = error_modal();
        // `q` quits when no error modal is displayed: pressing it must close the modal, whatever
        // the action of the key.
        close_on_key_press_or_click(&key(KeyCode::Char('q')), &mut error_modal);
        assert_eq!(error_modal, None);
    }

    #[test]
    fn mouse_click_closes_the_error_modal() {
        let mut error_modal = error_modal();
        let click = Event::Mouse(mouse_event(MouseEventKind::Down(MouseButton::Right), 0, 0));
        close_on_key_press_or_click(&click, &mut error_modal);
        assert_eq!(error_modal, None);
    }

    #[test]
    fn error_modal_stays_open_until_a_key_is_pressed_or_the_mouse_is_clicked() {
        let mut error_modal = error_modal();
        let release = Event::Key(KeyEvent::new_with_kind(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        ));
        close_on_key_press_or_click(&release, &mut error_modal);
        close_on_key_press_or_click(
            &Event::Resize(/* columns */ 80, /* rows */ 24),
            &mut error_modal,
        );
        for kind in [
            MouseEventKind::Drag(LEFT),
            MouseEventKind::Up(LEFT),
            MouseEventKind::Moved,
            MouseEventKind::ScrollDown,
        ] {
            close_on_key_press_or_click(&Event::Mouse(mouse_event(kind, 0, 0)), &mut error_modal);
        }
        assert!(error_modal.is_some());
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
            Some(Action::NextRevision)
        );
        assert_eq!(action(&key(KeyCode::Char('J'))), Some(Action::NextRevision));
        assert_eq!(
            action(&press(KeyCode::Down, KeyModifiers::CONTROL)),
            Some(Action::NextRevision)
        );
    }

    #[test]
    fn shift_k_and_ctrl_up_go_to_the_previous_commit() {
        assert_eq!(
            action(&press(KeyCode::Char('K'), KeyModifiers::SHIFT)),
            Some(Action::PreviousRevision)
        );
        assert_eq!(
            action(&key(KeyCode::Char('K'))),
            Some(Action::PreviousRevision)
        );
        assert_eq!(
            action(&press(KeyCode::Up, KeyModifiers::CONTROL)),
            Some(Action::PreviousRevision)
        );
    }

    #[test]
    fn digits_are_actions() {
        for c in '0'..='9' {
            let digit = c.to_digit(10).unwrap() as u8;
            assert_eq!(action(&key(KeyCode::Char(c))), Some(Action::Digit(digit)));
        }
    }

    fn type_keys(count: &mut Count, keys: &str) {
        keys.chars()
            .for_each(|c| assert_eq!(count.apply(&key(KeyCode::Char(c))), None, "{c}"));
    }

    #[test]
    fn typed_digits_build_a_pending_number() {
        let mut count = Count::default();
        assert_eq!(count.pending(), None);
        type_keys(&mut count, "1");
        assert_eq!(count.pending(), Some(1));
        type_keys(&mut count, "2");
        assert_eq!(count.pending(), Some(12));
    }

    #[test]
    fn the_pending_number_is_given_to_the_next_command_and_cleared() {
        let mut count = Count::default();
        type_keys(&mut count, "5");
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
    fn command_without_number_has_no_count() {
        let mut count = Count::default();
        assert_eq!(
            count.apply(&key(KeyCode::Char('G'))),
            Some((Action::Last, None))
        );
    }

    #[test]
    fn a_leading_zero_is_ignored() {
        let mut count = Count::default();
        type_keys(&mut count, "0");
        assert_eq!(count.pending(), None);
        type_keys(&mut count, "05");
        assert_eq!(count.pending(), Some(5));
        type_keys(&mut count, "0");
        assert_eq!(count.pending(), Some(50));
    }

    #[test]
    fn zero_alone_does_nothing() {
        let mut count = Count::default();
        type_keys(&mut count, "0");
        assert_eq!(
            count.apply(&key(KeyCode::Char('j'))),
            Some((Action::Down, None))
        );
    }

    #[test]
    fn a_too_large_number_saturates() {
        let mut count = Count::default();
        type_keys(&mut count, &"9".repeat(40));
        assert_eq!(count.pending(), Some(usize::MAX));
        assert_eq!(
            count.apply(&key(KeyCode::Char('j'))),
            Some((Action::Down, Some(usize::MAX)))
        );
    }

    #[test]
    fn any_other_key_cancels_the_pending_number() {
        for event in &[
            key(KeyCode::Esc),
            key(KeyCode::Enter),
            key(KeyCode::Tab),
            press(KeyCode::BackTab, KeyModifiers::SHIFT),
            key(KeyCode::Char('a')),
            ctrl('j'),
        ] {
            let mut count = Count::default();
            type_keys(&mut count, "5");
            assert_eq!(count.apply(event), None, "{event:?}");
            assert_eq!(count.pending(), None, "{event:?}");
            assert_eq!(
                count.apply(&key(KeyCode::Char('j'))),
                Some((Action::Down, None)),
                "{event:?}"
            );
        }
    }

    #[test]
    fn the_number_is_given_to_every_movement() {
        let shift = |c| press(KeyCode::Char(c), KeyModifiers::SHIFT);
        for (event, expected) in [
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
            (shift('J'), Action::NextRevision),
            (shift('K'), Action::PreviousRevision),
            (
                press(KeyCode::Down, KeyModifiers::CONTROL),
                Action::NextRevision,
            ),
            (
                press(KeyCode::Up, KeyModifiers::CONTROL),
                Action::PreviousRevision,
            ),
        ] {
            let mut count = Count::default();
            type_keys(&mut count, "3");
            assert_eq!(count.apply(&event), Some((expected, Some(3))), "{event:?}");
        }
    }

    #[test]
    fn key_releases_mouse_moves_and_other_events_keep_the_pending_number() {
        let release = Event::Key(KeyEvent::new_with_kind(
            KeyCode::Char('j'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        ));
        let mut count = Count::default();
        type_keys(&mut count, "5");
        assert_eq!(count.apply(&release), None);
        assert_eq!(
            count.apply(&Event::Resize(/* columns */ 80, /* rows */ 24)),
            None
        );
        let mouse_move = Event::Mouse(mouse_event(MouseEventKind::Moved, 0, 0));
        assert_eq!(count.apply(&mouse_move), None);
        assert_eq!(count.pending(), Some(5));
    }

    #[test]
    fn mouse_events_other_than_moves_cancel_the_pending_number() {
        for kind in [
            MouseEventKind::Down(LEFT),
            MouseEventKind::Drag(LEFT),
            MouseEventKind::Up(LEFT),
            MouseEventKind::ScrollDown,
        ] {
            let mut count = Count::default();
            type_keys(&mut count, "5");
            assert_eq!(count.apply(&Event::Mouse(mouse_event(kind, 0, 0))), None);
            assert_eq!(count.pending(), None, "{kind:?}");
        }
    }

    /// Height of the screen, without the status bar.
    const HEIGHT: usize = 20;

    /// The rows of a fake commit displayed on at least 70 rows, for the tests moving inside a
    /// commit (the furthest one displays the rows 30 to 69, on a screen of 40 rows).
    fn rows_of_a_long_commit() -> Vec<Row> {
        rows_of(&[fake::file_changes()], /* width */ 110)
    }

    /// A fake commit displayed on at least `SCROLLOFF` rows followed by a commit displayed on at
    /// least 70 rows, for the tests where the screen scrolls the header row of the second commit
    /// to `SCROLLOFF` rows below its top (which needs at least `SCROLLOFF` rows above that header
    /// row and `HEIGHT` rows from it to the last row).
    fn short_then_long_commits() -> [Commit; 2] {
        [fake::ltr_then_rtl_summary(), fake::file_changes()]
    }

    /// `viewport` after `action` repeated `count` times on `rows`.
    fn navigated(
        rows: &[Row],
        action: Action,
        count: Option<usize>,
        viewport: Viewport,
    ) -> Viewport {
        let mut viewport = viewport;
        navigate(action, count, &mut viewport, rows, HEIGHT);
        viewport
    }

    #[test]
    fn navigate_set_cursor_moves_the_cursor_without_moving_the_screen() {
        let rows = rows_of_a_long_commit();
        // The row 12 is the 3rd row of the screen, inside the scrolloff margin.
        assert_eq!(
            navigated(
                &rows,
                Action::SetCursor(12),
                None,
                Viewport { cursor: 20, top: 10 }
            ),
            Viewport { cursor: 12, top: 10 }
        );
    }

    #[test]
    fn navigate_down_and_up_move_the_cursor_row_by_row() {
        let rows = rows_of_a_long_commit();
        assert_eq!(
            navigated(&rows, Action::Down, None, Viewport { cursor: 3, top: 0 }).cursor,
            4
        );
        assert_eq!(
            navigated(&rows, Action::Up, None, Viewport { cursor: 3, top: 0 }).cursor,
            2
        );
    }

    #[test]
    fn navigate_down_and_up_are_repeated() {
        let rows = rows_of_a_long_commit();
        assert_eq!(
            navigated(
                &rows,
                Action::Down,
                /* count */ Some(5),
                Viewport { cursor: 3, top: 0 }
            )
            .cursor,
            8
        );
        assert_eq!(
            navigated(
                &rows,
                Action::Up,
                /* count */ Some(2),
                Viewport { cursor: 3, top: 0 }
            )
            .cursor,
            1
        );
    }

    #[test]
    fn navigate_down_and_up_stop_at_the_first_and_last_rows() {
        let rows = rows_of_a_long_commit();
        let last_row_index = rows.len() - 1;
        assert_eq!(
            navigated(
                &rows,
                Action::Down,
                Some(usize::MAX),
                Viewport { cursor: 3, top: 0 }
            )
            .cursor,
            last_row_index
        );
        assert_eq!(
            navigated(
                &rows,
                Action::Up,
                Some(usize::MAX),
                Viewport { cursor: 30, top: 20 }
            )
            .cursor,
            0
        );
    }

    #[test]
    fn navigate_down_scrolls_to_keep_the_scrolloff() {
        let rows = rows_of_a_long_commit();
        // The 15th row is the 15th row of the screen: the 6th row below it is not displayed.
        assert_eq!(
            navigated(&rows, Action::Down, None, Viewport { cursor: 14, top: 0 }),
            Viewport { cursor: 15, top: 1 }
        );
        assert_eq!(
            navigated(
                &rows,
                Action::Down,
                /* count */ Some(30),
                Viewport { cursor: 0, top: 0 }
            ),
            Viewport { cursor: 30, top: 16 }
        );
    }

    #[test]
    fn navigate_up_scrolls_to_keep_the_scrolloff() {
        let rows = rows_of_a_long_commit();
        assert_eq!(
            navigated(&rows, Action::Up, None, Viewport { cursor: 10, top: 5 }),
            Viewport { cursor: 9, top: 4 }
        );
    }

    #[test]
    fn navigate_scroll_down_and_up_move_a_cursor_inside_the_scrolloff_margin_like_j_and_k() {
        let rows = rows_of_a_long_commit();
        let len = rows.len();
        // The cursor is the 3rd row of the screen.
        assert_eq!(
            navigated(
                &rows,
                Action::ScrollDown,
                None,
                Viewport { cursor: 2, top: 0 }
            ),
            Viewport { cursor: 3, top: 0 }
        );
        // The cursor is the 2nd row from the bottom of the screen.
        let top = len - HEIGHT;
        assert_eq!(
            navigated(
                &rows,
                Action::ScrollUp,
                None,
                Viewport { cursor: len - 2, top }
            ),
            Viewport { cursor: len - 3, top }
        );
    }

    #[test]
    fn navigate_scroll_down_and_up_are_repeated() {
        let rows = rows_of_a_long_commit();
        assert_eq!(
            navigated(
                &rows,
                Action::ScrollDown,
                /* count */ Some(3),
                Viewport { cursor: 20, top: 10 }
            ),
            Viewport { cursor: 23, top: 13 }
        );
        assert_eq!(
            navigated(
                &rows,
                Action::ScrollUp,
                /* count */ Some(3),
                Viewport { cursor: 20, top: 10 }
            ),
            Viewport { cursor: 17, top: 7 }
        );
    }

    #[test]
    fn navigate_scroll_only_moves_the_cursor_at_both_ends() {
        let rows = rows_of_a_long_commit();
        let len = rows.len();
        let top = len - HEIGHT;
        assert_eq!(
            navigated(
                &rows,
                Action::ScrollDown,
                /* count */ Some(2),
                Viewport { cursor: len - 5, top }
            ),
            Viewport { cursor: len - 3, top }
        );
        assert_eq!(
            navigated(
                &rows,
                Action::ScrollUp,
                /* count */ Some(2),
                Viewport { cursor: 5, top: 0 }
            ),
            Viewport { cursor: 3, top: 0 }
        );
    }

    #[test]
    fn navigate_half_page_moves_the_screen_and_the_cursor_by_half_the_height() {
        let rows = rows_of_a_long_commit();
        assert_eq!(
            navigated(
                &rows,
                Action::HalfPageDown,
                None,
                Viewport { cursor: 20, top: 10 }
            ),
            Viewport { cursor: 30, top: 20 }
        );
        assert_eq!(
            navigated(
                &rows,
                Action::HalfPageUp,
                None,
                Viewport { cursor: 40, top: 30 }
            ),
            Viewport { cursor: 30, top: 20 }
        );
    }

    #[test]
    fn navigate_half_page_keeps_the_cursor_at_the_same_screen_position() {
        let rows = rows_of_a_long_commit();
        // Half of a screen of 40 rows is 20 rows.
        let mut viewport = Viewport { cursor: 20, top: 10 };
        navigate(
            Action::HalfPageDown,
            None,
            &mut viewport,
            &rows,
            /* height */ 40,
        );
        assert_eq!(viewport, Viewport { cursor: 40, top: 30 });
    }

    #[test]
    fn navigate_half_page_rounds_the_half_of_an_odd_height_down() {
        let rows = rows_of_a_long_commit();
        let mut viewport = Viewport { cursor: 20, top: 10 };
        navigate(
            Action::HalfPageDown,
            None,
            &mut viewport,
            &rows,
            /* height */ 21,
        );
        assert_eq!(viewport, Viewport { cursor: 30, top: 20 });
    }

    #[test]
    fn navigate_half_page_is_repeated() {
        let rows = rows_of_a_long_commit();
        assert_eq!(
            navigated(
                &rows,
                Action::HalfPageDown,
                /* count */ Some(3),
                Viewport { cursor: 20, top: 10 }
            ),
            Viewport { cursor: 50, top: 40 }
        );
        assert_eq!(
            navigated(
                &rows,
                Action::HalfPageUp,
                /* count */ Some(2),
                Viewport { cursor: 40, top: 30 }
            ),
            Viewport { cursor: 20, top: 10 }
        );
    }

    #[test]
    fn navigate_half_page_moves_the_cursor_alone_when_the_last_or_first_row_is_displayed() {
        let rows = rows_of_a_long_commit();
        let len = rows.len();
        let top = len - HEIGHT;
        assert_eq!(
            navigated(
                &rows,
                Action::HalfPageDown,
                None,
                Viewport { cursor: len - 15, top }
            ),
            Viewport { cursor: len - 5, top }
        );
        assert_eq!(
            navigated(
                &rows,
                Action::HalfPageUp,
                None,
                Viewport { cursor: 15, top: 0 }
            ),
            Viewport { cursor: 5, top: 0 }
        );
    }

    #[test]
    fn navigate_half_page_scrolls_by_fewer_rows_then_moves_the_cursor_alone_if_the_screen_stops() {
        let rows = rows_of_a_long_commit();
        let len = rows.len();
        // Only 3 rows are left below the screen.
        let top = len - HEIGHT - 3;
        assert_eq!(
            navigated(
                &rows,
                Action::HalfPageDown,
                None,
                Viewport { cursor: top + 10, top }
            ),
            Viewport {
                cursor: len - HEIGHT + 10,
                top: len - HEIGHT
            }
        );
        assert_eq!(
            navigated(
                &rows,
                Action::HalfPageDown,
                Some(usize::MAX),
                Viewport { cursor: 20, top: 10 }
            ),
            Viewport { cursor: len - 1, top: len - HEIGHT }
        );
        assert_eq!(
            navigated(
                &rows,
                Action::HalfPageUp,
                None,
                Viewport { cursor: 15, top: 3 }
            ),
            Viewport { cursor: 12, top: 0 }
        );
        assert_eq!(
            navigated(
                &rows,
                Action::HalfPageUp,
                Some(usize::MAX),
                Viewport { cursor: 20, top: 10 }
            ),
            Viewport { cursor: 0, top: 0 }
        );
    }

    #[test]
    fn navigate_first_goes_to_the_first_row() {
        let rows = rows_of_a_long_commit();
        assert_eq!(
            navigated(&rows, Action::First, None, Viewport { cursor: 50, top: 40 }),
            Viewport { cursor: 0, top: 0 }
        );
    }

    #[test]
    fn navigate_first_with_a_number_goes_to_that_row_counted_from_the_top() {
        let rows = rows_of_a_long_commit();
        assert_eq!(
            navigated(
                &rows,
                Action::First,
                /* count */ Some(1),
                Viewport { cursor: 50, top: 40 }
            ),
            Viewport { cursor: 0, top: 0 }
        );
        assert_eq!(
            navigated(
                &rows,
                Action::First,
                /* count */ Some(5),
                Viewport { cursor: 50, top: 40 }
            ),
            Viewport { cursor: 4, top: 0 }
        );
        assert_eq!(
            navigated(
                &rows,
                Action::First,
                /* count */ Some(30),
                Viewport { cursor: 0, top: 0 }
            ),
            Viewport { cursor: 29, top: 15 }
        );
    }

    #[test]
    fn navigate_first_with_a_too_large_number_goes_to_the_last_row() {
        let rows = rows_of_a_long_commit();
        let len = rows.len();
        assert_eq!(
            navigated(
                &rows,
                Action::First,
                Some(len + 1),
                Viewport { cursor: 0, top: 0 }
            ),
            Viewport { cursor: len - 1, top: len - HEIGHT }
        );
        assert_eq!(
            navigated(
                &rows,
                Action::First,
                Some(usize::MAX),
                Viewport { cursor: 0, top: 0 }
            ),
            Viewport { cursor: len - 1, top: len - HEIGHT }
        );
    }

    #[test]
    fn navigate_last_goes_to_the_last_row_and_scrolls_just_enough_to_display_it() {
        let rows = rows_of_a_long_commit();
        let len = rows.len();
        assert_eq!(
            navigated(&rows, Action::Last, None, Viewport { cursor: 0, top: 0 }),
            Viewport { cursor: len - 1, top: len - HEIGHT }
        );
        // Already displayed: the screen doesn't move more than needed.
        let almost = Viewport { cursor: len - 10, top: len - HEIGHT };
        assert_eq!(
            navigated(&rows, Action::Last, None, almost),
            Viewport { cursor: len - 1, top: len - HEIGHT }
        );
    }

    #[test]
    fn navigate_last_with_a_number_goes_to_that_row_counted_from_the_top() {
        let rows = rows_of_a_long_commit();
        assert_eq!(
            navigated(
                &rows,
                Action::Last,
                /* count */ Some(1),
                Viewport { cursor: 50, top: 40 }
            ),
            Viewport { cursor: 0, top: 0 }
        );
        assert_eq!(
            navigated(
                &rows,
                Action::Last,
                /* count */ Some(5),
                Viewport { cursor: 50, top: 40 }
            ),
            Viewport { cursor: 4, top: 0 }
        );
        // Like `g`.
        assert_eq!(
            navigated(
                &rows,
                Action::Last,
                /* count */ Some(30),
                Viewport { cursor: 0, top: 0 }
            ),
            navigated(
                &rows,
                Action::First,
                /* count */ Some(30),
                Viewport { cursor: 0, top: 0 }
            )
        );
    }

    #[test]
    fn navigate_last_with_a_too_large_number_goes_to_the_last_row() {
        let rows = rows_of_a_long_commit();
        let len = rows.len();
        assert_eq!(
            navigated(
                &rows,
                Action::Last,
                Some(len + 1),
                Viewport { cursor: 0, top: 0 }
            ),
            Viewport { cursor: len - 1, top: len - HEIGHT }
        );
        assert_eq!(
            navigated(
                &rows,
                Action::Last,
                Some(usize::MAX),
                Viewport { cursor: 0, top: 0 }
            ),
            Viewport { cursor: len - 1, top: len - HEIGHT }
        );
    }

    #[test]
    fn navigate_next_revision_goes_to_the_header_of_the_next_commit() {
        let commits = short_commits();
        let rows = rows_of(&commits, /* width */ 110);
        let headers = header_row_indexes(&commits, /* width */ 110);
        assert_eq!(
            navigated(
                &rows,
                Action::NextRevision,
                None,
                Viewport { cursor: 0, top: 0 }
            )
            .cursor,
            headers[1]
        );
        assert_eq!(
            navigated(
                &rows,
                Action::NextRevision,
                None,
                Viewport { cursor: headers[1] + 3, top: 0 }
            )
            .cursor,
            headers[2]
        );
    }

    #[test]
    fn navigate_next_revision_from_a_separator_goes_to_the_commit_below_its_own() {
        let commits = short_commits();
        let rows = rows_of(&commits, /* width */ 110);
        let headers = header_row_indexes(&commits, /* width */ 110);
        assert_eq!(
            navigated(
                &rows,
                Action::NextRevision,
                None,
                Viewport { cursor: headers[1] - 1, top: 0 }
            )
            .cursor,
            headers[2]
        );
    }

    #[test]
    fn navigate_next_revision_is_repeated_and_stops_on_the_last_commit() {
        let commits = short_commits();
        let rows = rows_of(&commits, /* width */ 110);
        let headers = header_row_indexes(&commits, /* width */ 110);
        assert_eq!(
            navigated(
                &rows,
                Action::NextRevision,
                /* count */ Some(2),
                Viewport { cursor: 0, top: 0 }
            )
            .cursor,
            headers[2]
        );
        assert_eq!(
            navigated(
                &rows,
                Action::NextRevision,
                Some(usize::MAX),
                Viewport { cursor: 0, top: 0 }
            )
            .cursor,
            *headers.last().unwrap()
        );
    }

    #[test]
    fn navigate_next_revision_on_the_last_commit_only_scrolls_the_cursor_row_to_the_top_margin() {
        let commits = short_then_long_commits();
        let rows = rows_of(&commits, /* width */ 110);
        let headers = header_row_indexes(&commits, /* width */ 110);
        let before = Viewport { cursor: headers[1] + 10, top: headers[1] };
        assert_eq!(
            navigated(&rows, Action::NextRevision, None, before),
            Viewport {
                cursor: headers[1] + 10,
                top: headers[1] + 10 - SCROLLOFF
            }
        );
    }

    #[test]
    fn navigate_previous_revision_goes_to_the_header_then_to_the_previous_commit() {
        let commits = short_commits();
        let rows = rows_of(&commits, /* width */ 110);
        let headers = header_row_indexes(&commits, /* width */ 110);
        let middle = Viewport { cursor: headers[2] + 3, top: headers[2] };
        let middle = navigated(&rows, Action::PreviousRevision, None, middle);
        assert_eq!(middle.cursor, headers[2]);
        assert_eq!(
            navigated(&rows, Action::PreviousRevision, None, middle).cursor,
            headers[1]
        );
    }

    #[test]
    fn navigate_previous_revision_from_a_separator_goes_to_the_commit_above_its_own() {
        let commits = short_commits();
        let rows = rows_of(&commits, /* width */ 110);
        let headers = header_row_indexes(&commits, /* width */ 110);
        assert_eq!(
            navigated(
                &rows,
                Action::PreviousRevision,
                None,
                Viewport { cursor: headers[2] - 1, top: 0 }
            )
            .cursor,
            headers[1]
        );
    }

    #[test]
    fn navigate_previous_revision_is_repeated_and_stops_on_the_first_commit() {
        let commits = short_commits();
        let rows = rows_of(&commits, /* width */ 110);
        let headers = header_row_indexes(&commits, /* width */ 110);
        // The first one goes to the header of the commit, the next one to the previous commit.
        let start = Viewport { cursor: headers[2] + 3, top: headers[2] };
        assert_eq!(
            navigated(
                &rows,
                Action::PreviousRevision,
                /* count */ Some(2),
                start
            )
            .cursor,
            headers[1]
        );
        assert_eq!(
            navigated(&rows, Action::PreviousRevision, Some(usize::MAX), start).cursor,
            0
        );
    }

    #[test]
    fn navigate_previous_revision_does_nothing_on_the_header_of_the_first_commit() {
        let rows = rows_of(&short_commits(), /* width */ 110);
        let before = Viewport { cursor: 0, top: 0 };
        assert_eq!(
            navigated(&rows, Action::PreviousRevision, None, before),
            before
        );
    }

    #[test]
    fn navigate_next_and_previous_revision_go_between_the_index_and_the_working_directory() {
        let (index, working_directory) = fake::dirty_directory();
        let rows = rows_of_revisions(
            &[
                Revision::Index(index),
                Revision::WorkingDirectory(working_directory),
            ],
            /* width */ 110,
        );
        let working_directory_header = view::header_row(&rows, /* revision_index */ 1).unwrap();
        let on_index = Viewport { cursor: 0, top: 0 };
        let on_working_directory = navigated(&rows, Action::NextRevision, None, on_index);
        assert_eq!(on_working_directory.cursor, working_directory_header);
        assert_eq!(
            navigated(&rows, Action::PreviousRevision, None, on_working_directory).cursor,
            0
        );
    }

    #[test]
    fn navigate_next_revision_scrolls_its_header_row_to_the_top_margin() {
        let commits = short_then_long_commits();
        let rows = rows_of(&commits, /* width */ 110);
        let headers = header_row_indexes(&commits, /* width */ 110);
        assert_eq!(
            navigated(
                &rows,
                Action::NextRevision,
                None,
                Viewport { cursor: 0, top: 0 }
            ),
            Viewport {
                cursor: headers[1],
                top: headers[1] - SCROLLOFF
            }
        );
    }

    #[test]
    fn navigate_previous_revision_scrolls_its_header_row_to_the_top_margin() {
        let commits = short_then_long_commits();
        let rows = rows_of(&commits, /* width */ 110);
        let headers = header_row_indexes(&commits, /* width */ 110);
        let inside_the_long_commit = Viewport {
            cursor: headers[1] + 30,
            top: headers[1] + 20,
        };
        assert_eq!(
            navigated(
                &rows,
                Action::PreviousRevision,
                None,
                inside_the_long_commit
            ),
            Viewport {
                cursor: headers[1],
                top: headers[1] - SCROLLOFF
            }
        );
    }

    #[test]
    fn navigate_next_revision_does_not_display_empty_space_after_the_last_row() {
        // The last commit is displayed on fewer rows than the screen.
        let commits = short_commits();
        let rows = rows_of(&commits, /* width */ 110);
        let headers = header_row_indexes(&commits, /* width */ 110);
        assert_eq!(
            navigated(
                &rows,
                Action::NextRevision,
                /* count */ Some(2),
                Viewport { cursor: 0, top: 0 }
            ),
            Viewport {
                cursor: headers[2],
                top: rows.len() - HEIGHT
            }
        );
    }

    #[test]
    fn navigate_ignores_actions_that_are_not_movements() {
        let rows = rows_of_a_long_commit();
        let before = Viewport { cursor: 20, top: 10 };
        for action in [Action::Quit, Action::Suspend, Action::Digit(3)] {
            assert_eq!(navigated(&rows, action, None, before), before);
        }
    }

    #[test]
    fn navigate_without_rows_does_nothing() {
        for action in [
            Action::Down,
            Action::Last,
            Action::NextRevision,
            Action::HalfPageDown,
        ] {
            let mut viewport = Viewport::default();
            navigate(action, /* count */ Some(3), &mut viewport, &[], HEIGHT);
            assert_eq!(viewport, Viewport::default(), "{action:?}");
        }
    }
}
