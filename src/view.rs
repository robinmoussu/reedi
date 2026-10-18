//! Rendering of the commits and of the status bar.

use std::iter;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Widget;
use unicode_width::UnicodeWidthStr;

use crate::invisible::FinalCr;
use crate::invisible::Kind;
use crate::model::Chunk;
use crate::model::Commit;
use crate::model::Decoration;
use crate::model::FileDiff;
use crate::model::Side;
use crate::viewport::Viewport;
use crate::wrap;
use crate::wrap::Segment;
use crate::wrap::TooNarrow;

/// Format used to display `Commit::date`.
const DATE_FORMAT: &str = "%d/%m/%Y";

// Colors used by git by default.
const HASH: Style = Style::new().fg(Color::Yellow);
const AUTHOR: Style = Style::new().fg(Color::Green);
const DATE: Style = Style::new().fg(Color::Blue);
const DECORATION_PUNCTUATION: Style = Style::new().fg(Color::Yellow);
const HEAD: Style = Style::new().fg(Color::Cyan).bold();
const LOCAL_BRANCH: Style = Style::new().fg(Color::Green).bold();
const REMOTE_BRANCH: Style = Style::new().fg(Color::Red).bold();
const TAG: Style = Style::new().fg(Color::Yellow).bold();

const FILE: Style = Style::new().fg(Color::Blue);
const LINE_NUMBER: Style = Style::new().fg(Color::Blue);
const COMMIT_SEPARATOR: Style = Style::new().fg(Color::Red);
const ERROR: Style = Style::new().fg(Color::Red).bold();

/// Replacements of the tabs, and of the spaces and U+2800 next to a break, that are not part of
/// the trailing whitespace.
const DIMMED_WHITESPACE: Style = Style::new().fg(Color::DarkGray);
/// Replacements of the trailing whitespace, including the tabs.
const TRAILING_WHITESPACE: Style = Style::new().fg(Color::Red).bold();
/// Replacements of the other invisible characters.
const INVISIBLE: Style = Style::new().fg(Color::Red).bold();

/// Displayed in the last column of each half of a diff row followed by a continuation row of the
/// same line.
const WRAP_MARKER: &str = "↵";

/// Displayed in place of a chunk whose lines cannot be wrapped.
const CHUNK_TOO_NARROW: &str = "This chunk cannot be displayed: the terminal is too narrow.";

/// Put after the first character of each row and after the last character of each column.
///
/// Terminals reordering bidirectional text do it for the whole row. Without it, the punctuation
/// and numbers next to a right-to-left text (like Persian) would be moved with it, even from
/// another column, and a row starting with a right-to-left text may be displayed right-to-left.
const LEFT_TO_RIGHT_MARK: char = '\u{200e}';

/// Background of the row of the cursor.
const CURSOR: Style = Style::new().bg(Color::Indexed(235));

/// The interaction mode, displayed in the status bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Review,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Review => "Review",
        }
    }

    /// Background color of the mode name in the status bar.
    pub fn accent(self) -> Color {
        match self {
            Mode::Review => Color::Blue,
        }
    }
}

/// What a row displays.
#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    Text(Line<'static>),
    /// A horizontal line spanning the whole width.
    Rule(Style),
    /// A row split 50/50 (the right half is bigger if the width is odd).
    Split {
        left: Line<'static>,
        right: Line<'static>,
    },
}

/// Position, in bytes, of the first character displayed on a row in its line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// The row is not split.
    Whole(usize),
    /// For each half, if it displays text.
    Split {
        left: Option<usize>,
        right: Option<usize>,
    },
}

/// The part of a row an [`Anchor`] is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Half {
    Whole,
    Left,
    Right,
}

/// The first character displayed on a row, which doesn't depend on the width of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Anchor {
    /// See [`Row::line`].
    pub line: usize,
    pub half: Half,
    /// Position of the character in its line, in bytes.
    pub offset: usize,
}

/// A single screen line.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub content: Content,
    /// Index of the line displayed (partially if it is wrapped) by the row, among all the lines:
    /// the headers, the lines of the commit messages and of the chunks, the file names, the blank
    /// lines and the rules. It doesn't depend on the width of the screen.
    pub line: usize,
    pub start: Start,
}

impl Row {
    /// Position of the first character displayed by `half` of the row, if any.
    fn offset(&self, half: Half) -> Option<usize> {
        match (self.start, half) {
            (Start::Whole(offset), Half::Whole) => Some(offset),
            (Start::Split { left, .. }, Half::Left) => left,
            (Start::Split { right, .. }, Half::Right) => right,
            _ => None,
        }
    }

    /// The first character displayed on the row: the one of the right half of a split row if it
    /// displays text, otherwise the one of the left half.
    pub fn anchor(&self) -> Anchor {
        let (half, offset) = match self.start {
            Start::Whole(offset) => (Half::Whole, offset),
            Start::Split {
                right: Some(offset),
                ..
            } => (Half::Right, offset),
            Start::Split { left, .. } => (Half::Left, left.unwrap_or(0)),
        };
        Anchor {
            line: self.line,
            half,
            offset,
        }
    }

    /// Render the row on the first line of `area`, cropping it if too wide.
    fn render(&self, area: Rect, buf: &mut Buffer) {
        match &self.content {
            Content::Text(line) => {
                buf.set_line(area.x, area.y, line, area.width);
                if area.width > 0 {
                    mark_left_to_right(buf, area.x, area.y);
                }
            }
            Content::Rule(style) => {
                let rule = "─".repeat(area.width.into());
                buf.set_string(area.x, area.y, rule, *style);
            }
            Content::Split { left, right } => {
                let left_width = area.width / 2;
                let right_width = area.width - left_width;
                buf.set_line(area.x, area.y, left, left_width);
                buf.set_line(area.x + left_width, area.y, right, right_width);
                if left_width > 0 {
                    mark_left_to_right(buf, area.x, area.y);
                    mark_left_to_right(buf, area.x + left_width - 1, area.y);
                }
            }
        }
    }
}

/// Index of the row of `rows` displaying `anchor`.
///
/// If the line of `anchor` is not displayed anymore (like the lines of a chunk replaced by an
/// error message), the first row of the next displayed line.
pub fn find(rows: &[Row], anchor: Anchor) -> usize {
    rows.iter()
        .rposition(|row| {
            row.line == anchor.line
                && row
                    .offset(anchor.half)
                    .is_some_and(|offset| offset <= anchor.offset)
        })
        .or_else(|| rows.iter().position(|row| row.line >= anchor.line))
        .unwrap_or(rows.len().saturating_sub(1))
}

/// A part of a line, displayed with `style`.
#[derive(Debug, Clone)]
struct Part {
    text: String,
    style: Style,
    /// See [`Segment::markup`].
    markup: bool,
}

impl Part {
    /// A part of the displayed data, like a field of the commit.
    fn content(text: &str, style: Style) -> Self {
        Self {
            text: text.to_string(),
            style,
            markup: false,
        }
    }

    /// Markup (see [`Segment::markup`]): separators, labels and messages.
    fn markup(text: &str, style: Style) -> Self {
        Self {
            text: text.to_string(),
            style,
            markup: true,
        }
    }
}

/// The style of a piece of `kind`.
fn kind_style(kind: Kind) -> Style {
    match kind {
        Kind::Text => Style::new(),
        Kind::DimmedWhitespace => DIMMED_WHITESPACE,
        Kind::TrailingWhitespace => TRAILING_WHITESPACE,
        Kind::Replacement => INVISIBLE,
    }
}

/// The spans displaying the line made of `parts` on rows of `width` columns, with the position of
/// their first character and the number of columns they use. Its invisible characters are
/// replaced, the style of each replacement being the style of its part, patched with the style of
/// its kind.
///
/// A carriage return ending a part is replaced if `final_cr` is [`FinalCr::Shown`], and hidden
/// otherwise.
fn wrapped(
    parts: &[Part],
    final_cr: FinalCr,
    width: usize,
) -> Result<Vec<(usize, usize, Vec<Span<'static>>)>, TooNarrow> {
    let segments: Vec<_> = parts
        .iter()
        .map(|part| Segment {
            text: &part.text,
            markup: part.markup,
        })
        .collect();
    let rows = wrap::wrap(&segments, final_cr, width)?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let spans = row
                .pieces
                .into_iter()
                .map(|(segment, piece)| {
                    let style = parts[segment].style.patch(kind_style(piece.kind));
                    Span::styled(piece.text, style)
                })
                .collect();
            (row.start, row.columns_count, spans)
        })
        .collect())
}

/// Put `LEFT_TO_RIGHT_MARK` after the character displayed at `x`, `y`, if it is in `buf`.
///
/// Terminals attach a zero-width character to the cell before the cursor, and the cursor jumps
/// over the cells that are not modified. So the mark is appended to the symbol of the cell of the
/// character, which is then always written right before it.
fn mark_left_to_right(buf: &mut Buffer, x: u16, y: u16) {
    // The cells covered by a wide character are never written, its own cell is the one before.
    let covered = x > buf.area.x
        && buf
            .cell((x - 1, y))
            .is_some_and(|cell| cell.symbol().width() > 1);
    let x = if covered { x - 1 } else { x };
    if let Some(cell) = buf.cell_mut((x, y)) {
        let symbol = format!("{}{LEFT_TO_RIGHT_MARK}", cell.symbol());
        cell.set_symbol(&symbol);
    }
}

fn single_decoration_parts(decoration: &Decoration) -> Vec<Part> {
    match decoration {
        Decoration::Head(None) => vec![Part::markup("HEAD", HEAD)],
        Decoration::Head(Some(branch)) => vec![
            Part::markup("HEAD", HEAD),
            Part::markup(" -> ", DECORATION_PUNCTUATION),
            Part::content(branch, LOCAL_BRANCH),
        ],
        Decoration::LocalBranch(branch) => vec![Part::content(branch, LOCAL_BRANCH)],
        Decoration::RemoteBranch(branch) => vec![Part::content(branch, REMOTE_BRANCH)],
        Decoration::Tag(tag) => vec![Part::markup("tag: ", TAG), Part::content(tag, TAG)],
    }
}

/// ` ($decorations)`, or nothing if there are no decorations.
fn decoration_parts(decorations: &[Decoration]) -> Vec<Part> {
    if decorations.is_empty() {
        return Vec::new();
    }
    let decorations = decorations.iter().enumerate().flat_map(|(i, decoration)| {
        let separator = if i > 0 {
            Some(Part::markup(", ", DECORATION_PUNCTUATION))
        } else {
            None
        };
        separator
            .into_iter()
            .chain(single_decoration_parts(decoration))
    });
    iter::once(Part::markup(" (", DECORATION_PUNCTUATION))
        .chain(decorations)
        .chain(iter::once(Part::markup(")", DECORATION_PUNCTUATION)))
        .collect()
}

/// `$hash $commit_subject $author $date ($tags, $branches)`
///
/// The trailing whitespace of each field is the one ending the field.
fn header_parts(commit: &Commit) -> Vec<Part> {
    let date = commit.date.strftime(DATE_FORMAT).to_string();
    let space = || Part::markup(" ", Style::new());
    [
        Part::content(&commit.hash, HASH),
        space(),
        Part::content(&commit.subject, Style::new()),
        space(),
        Part::content(&commit.author, AUTHOR),
        space(),
        Part::content(&date, DATE),
    ]
    .into_iter()
    .chain(decoration_parts(&commit.decorations))
    .collect()
}

/// The line number column of a diff row: `│ $line │ `, or `│     │ ` without `number`.
fn number_column(number: Option<usize>, width: usize) -> Span<'static> {
    let number = number.map(|number| number.to_string()).unwrap_or_default();
    Span::styled(format!("│ {number:>width$} │ "), LINE_NUMBER)
}

/// The rows displaying the `index`-th line of `side` in a half of a split row, `width` columns
/// wide, with the position of their first character. The line number column is `number_width`
/// wide.
///
/// A carriage return ending the line is replaced if `final_cr` is [`FinalCr::Shown`], and hidden
/// otherwise.
fn half_rows(
    side: &Side,
    index: usize,
    number_width: usize,
    width: usize,
    final_cr: FinalCr,
) -> Result<Vec<(usize, Line<'static>)>, TooNarrow> {
    // `│ `, the number, ` │ `, then the text area, then the column of the marker.
    let text_width = width.saturating_sub(number_width + 6);
    let code = Part::content(&side.lines[index], Style::new());
    let rows = wrapped(&[code], final_cr, text_width)?;
    let last = rows.len() - 1;
    Ok(rows
        .into_iter()
        .enumerate()
        .map(|(i, (start, columns_count, spans))| {
            let number = if i == 0 {
                Some(side.line_number(index))
            } else {
                None
            };
            let marker = if i < last {
                vec![
                    Span::raw(" ".repeat(text_width.saturating_sub(columns_count))),
                    Span::styled(WRAP_MARKER, LINE_NUMBER),
                ]
            } else {
                Vec::new()
            };
            let spans: Vec<_> = iter::once(number_column(number, number_width))
                .chain(spans)
                .chain(marker)
                .collect();
            (start, Line::from(spans))
        })
        .collect())
}

/// The rows being built. `line` counts the lines added so far (a wrapped line is added as several
/// rows, all with the same [`Row::line`]).
struct Rows {
    rows: Vec<Row>,
    line: usize,
    width: u16,
}

impl Rows {
    fn new(width: u16) -> Self {
        Self {
            rows: Vec::new(),
            line: 0,
            width,
        }
    }

    /// Add a line made of a single row.
    fn single(&mut self, content: Content) {
        self.rows.push(Row {
            content,
            line: self.line,
            start: Start::Whole(0),
        });
        self.line += 1;
    }

    fn blank(&mut self) {
        self.single(Content::Text(Line::default()));
    }

    fn rule(&mut self, style: Style) {
        self.single(Content::Rule(style));
    }

    /// Add a line made of `parts`, using the whole width. A carriage return ending a part is
    /// replaced.
    fn text(&mut self, parts: &[Part]) -> Result<(), TooNarrow> {
        let rows = wrapped(parts, FinalCr::Shown, self.width.into())?;
        let line = self.line;
        self.rows
            .extend(rows.into_iter().map(|(start, _, spans)| Row {
                content: Content::Text(Line::from(spans)),
                line,
                start: Start::Whole(start),
            }));
        self.line += 1;
        Ok(())
    }

    /// Add the lines of `chunk`, or an error message in their place if they cannot be wrapped.
    ///
    /// A carriage return ending a line is replaced if `final_cr` is [`FinalCr::Shown`], and
    /// hidden otherwise.
    fn chunk(&mut self, chunk: &Chunk, final_cr: FinalCr) -> Result<(), TooNarrow> {
        let len = chunk.before.lines.len();
        match self.chunk_rows(chunk, final_cr) {
            Ok(rows) => {
                self.rows.extend(rows);
                self.line += len;
            }
            Err(_) => {
                self.text(&[Part::markup(CHUNK_TOO_NARROW, ERROR)])?;
                // The error message takes the index of the first line.
                self.line += len.saturating_sub(1);
            }
        }
        Ok(())
    }

    /// The rows displaying the lines of `chunk`. If one half of a line has more rows than the
    /// other, the other one is padded with rows displaying only the line number column.
    fn chunk_rows(&self, chunk: &Chunk, final_cr: FinalCr) -> Result<Vec<Row>, TooNarrow> {
        let number_width = chunk.line_number_width();
        let left_width = usize::from(self.width / 2);
        let right_width = usize::from(self.width) - left_width;
        let lines = (0..chunk.before.lines.len()).map(|index| {
            let left = half_rows(&chunk.before, index, number_width, left_width, final_cr)?;
            let right = half_rows(&chunk.after, index, number_width, right_width, final_cr)?;
            let len = left.len().max(right.len());
            let padded = |rows: Vec<(usize, Line<'static>)>| {
                rows.into_iter()
                    .map(|(start, line)| (Some(start), line))
                    .chain(iter::repeat_with(|| {
                        (None, Line::from(number_column(None, number_width)))
                    }))
                    .take(len)
            };
            let line = self.line + index;
            Ok(padded(left)
                .zip(padded(right))
                .map(|((left_start, left), (right_start, right))| Row {
                    content: Content::Split { left, right },
                    line,
                    start: Start::Split {
                        left: left_start,
                        right: right_start,
                    },
                })
                .collect::<Vec<_>>())
        });
        lines
            .collect::<Result<Vec<_>, _>>()
            .map(|lines| lines.into_iter().flatten().collect())
    }

    fn file(&mut self, file: &FileDiff) -> Result<(), TooNarrow> {
        let final_cr = if file.has_inconsistent_line_endings() {
            FinalCr::Shown
        } else {
            FinalCr::Hidden
        };
        self.blank();
        self.text(&[Part::content(&file.path, FILE)])?;
        self.rule(FILE);
        file.chunks.iter().enumerate().try_for_each(|(i, chunk)| {
            if i > 0 {
                self.blank();
            }
            self.chunk(chunk, final_cr)
        })
    }

    /// Add `commit`, including the separator below it.
    fn commit(&mut self, commit: &Commit) -> Result<(), TooNarrow> {
        self.text(&header_parts(commit))?;
        if !commit.body.is_empty() {
            self.blank();
            // Unlike `str::lines`, a carriage return ending a line is kept, to be displayed.
            commit
                .body
                .split_terminator('\n')
                .try_for_each(|line| self.text(&[Part::content(line, Style::new())]))?;
        }
        commit.files.iter().try_for_each(|file| self.file(file))?;
        self.blank();
        self.rule(COMMIT_SEPARATOR);
        Ok(())
    }
}

/// All the rows displaying `commits` on a screen `width` columns wide.
///
/// Fails if a text area is too narrow to display a grapheme cluster of its line, unless it is in
/// a chunk, which is then replaced by an error message.
pub fn rows(commits: &[Commit], width: u16) -> Result<Vec<Row>, TooNarrow> {
    let mut rows = Rows::new(width);
    commits.iter().try_for_each(|commit| rows.commit(commit))?;
    Ok(rows.rows)
}

/// Rows displayed from `viewport.top`, cropped if they don't fit, with the row of the cursor
/// highlighted.
pub struct RowsView<'a> {
    pub rows: &'a [Row],
    pub viewport: Viewport,
}

impl Widget for RowsView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.rows
            .iter()
            .enumerate()
            .skip(self.viewport.top)
            .zip(area.top()..area.bottom())
            .for_each(|((index, row), y)| {
                let area = Rect {
                    y,
                    height: 1,
                    ..area
                };
                row.render(area, buf);
                if index == self.viewport.cursor {
                    buf.set_style(area, CURSOR);
                }
            });
    }
}

pub struct StatusBar {
    pub mode: Mode,
}

impl Widget for StatusBar {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mode = Span::styled(
            format!(" {} ", self.mode.name()),
            Style::new().fg(Color::Black).bg(self.mode.accent()).bold(),
        );
        Line::from(mode).render(area, buf);
    }
}

/// Draw `rows`, with the status bar on the last line of the screen.
///
/// `viewport` is first scrolled to fit the screen.
pub fn draw(frame: &mut Frame, rows: &[Row], viewport: &mut Viewport, mode: Mode) {
    let [content, status_bar] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());
    viewport.scroll(rows.len(), content.height.into());
    let viewport = *viewport;
    frame.render_widget(RowsView { rows, viewport }, content);
    frame.render_widget(StatusBar { mode }, status_bar);
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::Backend;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::fake;

    /// The most recent fake commit.
    fn newest() -> Commit {
        fake::commits().pop().unwrap()
    }

    /// The fake commit with long lines.
    fn wrapping() -> Commit {
        fake::commits().remove(0)
    }

    /// The fake commit with all kinds of invisible characters.
    fn invisible() -> Commit {
        fake::commits().remove(1)
    }

    fn commit_rows(commit: &Commit, width: u16) -> Vec<Row> {
        let mut rows = Rows::new(width);
        rows.commit(commit).unwrap();
        rows.rows
    }

    /// The rows of `file`, starting with the blank row above the file name.
    fn file_rows(file: &FileDiff, width: u16) -> Vec<Row> {
        let mut rows = Rows::new(width);
        rows.file(file).unwrap();
        rows.rows
    }

    /// A row of the first line.
    fn row(content: Content) -> Row {
        Row {
            content,
            line: 0,
            start: Start::Whole(0),
        }
    }

    fn text(line: &Line) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    fn text_of_row(row: &Row) -> (String, String) {
        match &row.content {
            Content::Text(line) => (text(line), String::new()),
            Content::Split { left, right } => (text(left), text(right)),
            Content::Rule(_) => (String::new(), String::new()),
        }
    }

    /// The text of the first row of `rows` whose left half starts with `prefix`.
    fn row_starting_with(rows: &[Row], prefix: &str) -> (String, String) {
        rows.iter()
            .map(text_of_row)
            .find(|(left, _)| left.starts_with(prefix))
            .unwrap()
    }

    /// The header of `commit`, on a single row.
    fn header(commit: &Commit) -> Line<'static> {
        let (_, _, spans) = wrapped(&header_parts(commit), FinalCr::Shown, 1000)
            .unwrap()
            .remove(0);
        Line::from(spans)
    }

    /// The style of the first span of `line` whose content is `content`.
    fn style_in(line: &Line, content: &str) -> Style {
        line.spans
            .iter()
            .find(|span| span.content == content)
            .unwrap()
            .style
    }

    /// `buffer`, without the left-to-right marks, which are invisible in the snapshots.
    fn without_left_to_right_marks(buffer: &Buffer) -> Buffer {
        let mut buffer = buffer.clone();
        buffer.content.iter_mut().for_each(|cell| {
            let symbol = cell.symbol().replace(LEFT_TO_RIGHT_MARK, "");
            cell.set_symbol(&symbol);
        });
        buffer
    }

    /// The screen displaying the fake commits, without the left-to-right marks.
    fn render(width: u16, height: u16, viewport: Viewport) -> TestBackend {
        let commits = fake::commits();
        let rows = rows(&commits, width).unwrap();
        let mut viewport = viewport;
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| draw(frame, &rows, &mut viewport, Mode::Review))
            .unwrap();
        let buffer = without_left_to_right_marks(terminal.backend().buffer());
        let mut backend = TestBackend::new(width, height);
        let cells = buffer.content.iter().enumerate().map(|(i, cell)| {
            let (x, y) = (i % usize::from(width), i / usize::from(width));
            (x as u16, y as u16, cell)
        });
        backend.draw(cells).unwrap();
        backend
    }

    #[test]
    fn full_screen() {
        insta::assert_snapshot!(render(110, 26, Viewport::default()));
    }

    #[test]
    fn small_screen_is_wrapped_and_cropped() {
        insta::assert_snapshot!(render(50, 8, Viewport::default()));
    }

    #[test]
    fn scrolled_to_the_end() {
        let len = rows(&fake::commits(), 110).unwrap().len();
        let viewport = Viewport {
            cursor: len - 1,
            top: 0,
        };
        insta::assert_snapshot!(render(110, 26, viewport));
    }

    #[test]
    fn long_lines_are_wrapped() {
        insta::assert_snapshot!(render(110, 60, Viewport::default()));
    }

    #[test]
    fn long_lines_are_wrapped_on_a_narrow_screen() {
        insta::assert_snapshot!(render(70, 80, Viewport::default()));
    }

    #[test]
    fn invisible_characters_after_the_first_screen() {
        let start = commit_rows(&wrapping(), 110).len();
        let viewport = Viewport {
            cursor: start + 30,
            top: start + 20,
        };
        insta::assert_snapshot!(render(110, 26, viewport));
    }

    #[test]
    fn rows_of_all_commits_are_displayed_one_after_the_other() {
        let commits = fake::commits();
        let all_rows: Vec<_> = rows(&commits, 110)
            .unwrap()
            .into_iter()
            .map(|row| row.content)
            .collect();
        let expected: Vec<_> = commits
            .iter()
            .flat_map(|commit| commit_rows(commit, 110))
            .map(|row| row.content)
            .collect();
        assert_eq!(all_rows, expected);
    }

    #[test]
    fn lines_are_numbered_consecutively() {
        let rows = rows(&fake::commits(), 110).unwrap();
        assert_eq!(rows[0].line, 0);
        assert!(
            rows.windows(2)
                .all(|rows| rows[1].line == rows[0].line || rows[1].line == rows[0].line + 1)
        );
    }

    #[test]
    fn cursor_row_is_highlighted() {
        let viewport = Viewport { cursor: 2, top: 0 };
        let backend = render(110, 26, viewport);
        let buffer = backend.buffer();
        assert!((0..110).all(|x| buffer[(x, 2)].bg == Color::Indexed(235)));
        assert!((0..110).all(|x| buffer[(x, 1)].bg == Color::Reset));
        assert!((0..110).all(|x| buffer[(x, 3)].bg == Color::Reset));
    }

    #[test]
    fn cursor_row_is_highlighted_when_scrolled() {
        let viewport = Viewport {
            cursor: 30,
            top: 20,
        };
        let backend = render(110, 26, viewport);
        let buffer = backend.buffer();
        assert_eq!(buffer[(0, 10)].bg, Color::Indexed(235));
        assert_eq!(buffer[(0, 9)].bg, Color::Reset);
    }

    #[test]
    fn header_is_colored_like_git() {
        let commit = newest();
        let header = header(&commit);
        assert_eq!(
            text(&header),
            "3f9a2c1 config: rename scroll_off to scrolloff Alice Martin 12/10/2026 \
             (HEAD -> main, tag: v1.0, origin/main)"
        );
        assert_eq!(style_in(&header, "3f9a2c1"), HASH);
        assert_eq!(style_in(&header, "Alice Martin"), AUTHOR);
        assert_eq!(style_in(&header, "12/10/2026"), DATE);
        assert_eq!(style_in(&header, "HEAD"), HEAD);
        assert_eq!(style_in(&header, " -> "), DECORATION_PUNCTUATION);
        assert_eq!(style_in(&header, "main"), LOCAL_BRANCH);
        assert_eq!(style_in(&header, "v1.0"), TAG);
        assert_eq!(style_in(&header, "origin/main"), REMOTE_BRANCH);
    }

    #[test]
    fn decorations_are_omitted_when_empty() {
        let mut commit = newest();
        commit.decorations.clear();
        assert!(text(&header(&commit)).ends_with("12/10/2026"));
    }

    #[test]
    fn decorations_without_head() {
        let decorations = [
            Decoration::LocalBranch("feature".to_string()),
            Decoration::Head(None),
        ];
        let text: String = decoration_parts(&decorations)
            .into_iter()
            .map(|part| part.text)
            .collect();
        assert_eq!(text, " (feature, HEAD)");
    }

    #[test]
    fn body_is_omitted_when_empty() {
        let mut commit = newest();
        commit.body.clear();
        let rows = commit_rows(&commit, 110);
        assert_eq!(rows[1].content, Content::Text(Line::default()));
        assert_eq!(text_of_row(&rows[2]).0, "src/config.rs");
    }

    #[test]
    fn split_right_half_is_bigger_when_width_is_odd() {
        let row = row(Content::Split {
            left: Line::raw("LLLLLLLLLLLLLLL"),
            right: Line::raw("RRRRRRRRRRRRRRR"),
        });
        let area = Rect::new(0, 0, 21, 1);
        let mut buf = Buffer::empty(area);
        row.render(area, &mut buf);
        assert_eq!(
            without_left_to_right_marks(&buf),
            Buffer::with_lines(["LLLLLLLLLLRRRRRRRRRRR"])
        );
    }

    #[test]
    fn rows_and_columns_have_a_left_to_right_mark() {
        let area = Rect::new(0, 0, 21, 1);
        let symbols = |content: Content| {
            let mut buf = Buffer::empty(area);
            row(content).render(area, &mut buf);
            buf.content
                .iter()
                .map(|cell| cell.symbol().to_string())
                .collect::<Vec<_>>()
        };
        let split = symbols(Content::Split {
            left: Line::raw("L"),
            right: Line::raw("R"),
        });
        assert_eq!(split[0], "L\u{200e}");
        assert_eq!(split[9], " \u{200e}");
        assert_eq!(split[1], " ");
        assert_eq!(split[10], "R");
        let text = symbols(Content::Text(Line::raw("T")));
        assert_eq!(text[0], "T\u{200e}");
        assert_eq!(text[9], " ");
    }

    #[test]
    fn left_to_right_mark_is_after_a_wide_character_ending_a_column() {
        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        // `日` is 2 columns wide, on the last 2 columns of the left half.
        let row = row(Content::Split {
            left: Line::raw("LLLLLLLL日"),
            right: Line::raw("R"),
        });
        row.render(area, &mut buf);
        assert_eq!(buf[(8, 0)].symbol(), "日\u{200e}");
        assert_eq!(buf[(9, 0)].symbol(), " ");
    }

    #[test]
    fn line_numbers_are_aligned_per_chunk() {
        let commit = newest();
        let rows = Rows::new(110)
            .chunk_rows(&commit.files[0].chunks[1], FinalCr::Hidden)
            .unwrap();
        let Content::Split { left, right } = &rows[0].content else {
            panic!("chunk rows are split");
        };
        assert_eq!(text(left), "│  97 │ impl Default for Config {");
        assert_eq!(text(right), "│  97 │ impl Default for Config {");
        assert_eq!(left.spans[0].style, LINE_NUMBER);
    }

    /// A chunk of a single line, whose left half is wider than its right half.
    fn chunk(before: &str, after: &str) -> Chunk {
        let side = |line: &str| Side {
            first_line: 1,
            lines: vec![line.to_string()],
        };
        Chunk {
            before: side(before),
            after: side(after),
        }
    }

    #[test]
    fn wrapped_diff_lines_have_a_marker_and_no_line_number_on_continuation_rows() {
        // Each half is 12 columns wide: 6 for the line number column, 5 for the text, and 1 for
        // the marker.
        let rows = Rows::new(24)
            .chunk_rows(&chunk("aaaa bbbb cccc", "dd"), FinalCr::Hidden)
            .unwrap();
        let texts: Vec<_> = rows.iter().map(text_of_row).collect();
        assert_eq!(
            texts,
            [
                ("│ 1 │ aaaa ↵".to_string(), "│ 1 │ dd".to_string()),
                ("│   │ bbbb ↵".to_string(), "│   │ ".to_string()),
                ("│   │ cccc".to_string(), "│   │ ".to_string()),
            ]
        );
        let Content::Split { left, .. } = &rows[0].content else {
            panic!("chunk rows are split");
        };
        assert_eq!(style_in(left, WRAP_MARKER), LINE_NUMBER);
        let area = Rect::new(0, 0, 24, 1);
        let mut buf = Buffer::empty(area);
        rows[0].render(area, &mut buf);
        let buf = without_left_to_right_marks(&buf);
        // In the last column of the left half.
        assert_eq!(buf[(11, 0)].symbol(), WRAP_MARKER);
    }

    #[test]
    fn marker_is_in_the_last_column_of_the_right_half() {
        let rows = Rows::new(24)
            .chunk_rows(&chunk("a", "aaaa bbbb"), FinalCr::Hidden)
            .unwrap();
        let area = Rect::new(0, 0, 24, 1);
        let mut buf = Buffer::empty(area);
        rows[0].render(area, &mut buf);
        assert_eq!(buf[(23, 0)].symbol(), WRAP_MARKER);
    }

    #[test]
    fn rows_of_a_diff_line_start_at_their_first_character() {
        let rows = Rows::new(24)
            .chunk_rows(&chunk("aaaa bbbb cccc", "dd"), FinalCr::Hidden)
            .unwrap();
        let starts: Vec<_> = rows.iter().map(|row| row.start).collect();
        assert_eq!(
            starts,
            [
                Start::Split {
                    left: Some(0),
                    right: Some(0)
                },
                Start::Split {
                    left: Some(5),
                    right: None
                },
                Start::Split {
                    left: Some(10),
                    right: None
                },
            ]
        );
        assert!(rows.iter().all(|row| row.line == 0));
    }

    #[test]
    fn header_body_and_file_names_are_wrapped_without_marker() {
        let rows = commit_rows(&wrapping(), 110);
        let (header, continuation) = (text_of_row(&rows[0]).0, text_of_row(&rows[1]).0);
        assert_eq!(
            header,
            "b0d4e2a view: wrap the lines too wide for the screen instead of cropping them at its \
             edge Chloé Bernard "
        );
        assert_eq!(continuation, "01/10/2026 (tag: v0.2, origin/line-wrapping)");
        assert_eq!((rows[0].line, rows[1].line), (0, 0));
        // The first line of the body.
        assert_eq!(rows[3].line, rows[4].line);
        let mut commit = newest();
        commit.files[0].path = "src/a/very/long/path/to/the/config.rs".to_string();
        let rows = commit_rows(&commit, 20);
        let names: Vec<_> = rows
            .iter()
            .filter(|row| row.line == rows[0].line + 5)
            .map(|row| text_of_row(row).0)
            .collect();
        assert_eq!(names, ["src/a/very/long/", "path/to/the/", "config.rs"]);
        assert!(rows.iter().all(|row| match &row.content {
            Content::Text(line) => !text(line).contains(WRAP_MARKER),
            _ => true,
        }));
    }

    #[test]
    fn header_is_broken_inside_a_field_keeping_its_style() {
        let rows = commit_rows(&newest(), 93);
        let (first, second) = (&rows[0].content, &rows[1].content);
        let (Content::Text(first), Content::Text(second)) = (first, second) else {
            panic!("the header is not split");
        };
        // The space of the markup before the break is not replaced.
        assert!(text(first).ends_with("(HEAD -> main, tag: "));
        assert_eq!(first.spans.last().unwrap().style, TAG);
        assert_eq!(text(second), "v1.0, origin/main)");
        assert_eq!(second.spans[0].style, TAG);
    }

    #[test]
    fn tab_stops_of_the_header_are_counted_from_the_start_of_the_row() {
        let mut commit = invisible();
        commit.subject = "xy".to_string();
        commit.author = "a\tb".to_string();
        // The tab is at column 12.
        assert_eq!(text(&header(&commit)), "c7e05b9 xy a⇥   b 05/10/2026");
    }

    #[test]
    fn chunk_too_narrow_is_replaced_by_an_error_message() {
        let commit = newest();
        // The text area of each half has no column.
        let rows = commit_rows(&commit, 12);
        assert!(
            rows.iter()
                .all(|row| !matches!(row.content, Content::Split { .. }))
        );
        let errors: Vec<_> = rows
            .iter()
            .filter_map(|row| match &row.content {
                Content::Text(line) if line.spans.first()?.style == ERROR => Some(row.line),
                _ => None,
            })
            .collect();
        assert!(!errors.is_empty());
        // The lines after the chunks keep the same index.
        let wide = commit_rows(&commit, 110);
        assert_eq!(rows.last().unwrap().line, wide.last().unwrap().line);
    }

    #[test]
    fn chunk_too_narrow_for_the_error_message_is_an_error() {
        let commit = newest();
        let error = Rows::new(0)
            .chunk(&commit.files[0].chunks[0], FinalCr::Hidden)
            .unwrap_err();
        assert_eq!(error.width, 0);
    }

    #[test]
    fn text_area_too_narrow_outside_of_a_chunk_is_an_error() {
        let mut commit = newest();
        commit.author = "日本".to_string();
        let error = rows(&[commit], 1).unwrap_err();
        assert_eq!(
            error,
            TooNarrow {
                width: 1,
                cluster: "日".to_string()
            }
        );
        assert!(rows(&fake::commits(), 0).is_err());
    }

    #[test]
    fn anchor_is_on_the_right_half_if_it_displays_text() {
        let rows = Rows::new(24)
            .chunk_rows(&chunk("aaaa bbbb", "cccc dddd"), FinalCr::Hidden)
            .unwrap();
        assert_eq!(
            rows[1].anchor(),
            Anchor {
                line: 0,
                half: Half::Right,
                offset: 5
            }
        );
        let rows = Rows::new(24)
            .chunk_rows(&chunk("aaaa bbbb", "dd"), FinalCr::Hidden)
            .unwrap();
        assert_eq!(
            rows[1].anchor(),
            Anchor {
                line: 0,
                half: Half::Left,
                offset: 5
            }
        );
    }

    #[test]
    fn find_returns_the_row_displaying_the_first_character_of_a_row() {
        let commits = fake::commits();
        let wide = rows(&commits, 110).unwrap();
        let narrow = rows(&commits, 60).unwrap();
        narrow.iter().for_each(|row| {
            let anchor = row.anchor();
            let i = find(&wide, anchor);
            assert_eq!(wide[i].line, anchor.line);
            // The found row starts at or before the anchor, and the next one after it.
            assert!(wide[i].offset(anchor.half).unwrap() <= anchor.offset);
            let next = wide
                .get(i + 1)
                .filter(|next| next.line == anchor.line)
                .and_then(|next| next.offset(anchor.half));
            assert!(next.is_none_or(|offset| offset > anchor.offset));
        });
        // Each row is found back on its own screen.
        assert!(
            narrow
                .iter()
                .enumerate()
                .all(|(i, row)| find(&narrow, row.anchor()) == i)
        );
    }

    #[test]
    fn find_returns_the_next_line_when_the_line_is_not_displayed() {
        let commit = newest();
        let wide = commit_rows(&commit, 110);
        let narrow = commit_rows(&commit, 12);
        let code = wide
            .iter()
            .find(|row| matches!(row.content, Content::Split { .. }))
            .unwrap();
        let found = &narrow[find(&narrow, code.anchor())];
        assert!(found.line >= code.line);
    }

    #[test]
    fn rules_and_file_name_are_colored() {
        let commit = newest();
        let rows = commit_rows(&commit, 110);
        let area = Rect::new(0, 0, 110, 22);
        let mut buffer = Buffer::empty(area);
        RowsView {
            rows: &rows,
            viewport: Viewport::default(),
        }
        .render(area, &mut buffer);
        let buffer = without_left_to_right_marks(&buffer);
        // File name, then the rule below it.
        assert_eq!(buffer[(0, 5)].symbol(), "s");
        assert_eq!(buffer[(0, 5)].fg, Color::Blue);
        assert_eq!(buffer[(109, 6)].symbol(), "─");
        assert_eq!(buffer[(109, 6)].fg, Color::Blue);
        // Separator below the commit.
        assert_eq!(buffer[(109, 21)].symbol(), "─");
        assert_eq!(buffer[(109, 21)].fg, Color::Red);
    }

    #[test]
    fn status_bar_displays_the_mode() {
        let backend = render(110, 26, Viewport::default());
        let buffer = backend.buffer();
        let status_bar: String = (0..8).map(|x| buffer[(x, 25)].symbol()).collect();
        assert_eq!(status_bar, " Review ");
        assert_eq!(buffer[(1, 25)].bg, Mode::Review.accent());
    }

    #[test]
    fn invisible_characters_of_the_header_are_replaced_per_field() {
        let mut commit = invisible();
        commit.decorations = vec![Decoration::LocalBranch("fix\u{200b} ".to_string())];
        let header = header(&commit);
        assert_eq!(
            text(&header),
            "c7e05b9 lint: add fixtures for⌁ invisible characters∙ Zoé⍽Durand 05/10/2026 (fix⌁∙)"
        );
        assert_eq!(style_in(&header, "⍽"), AUTHOR.patch(INVISIBLE));
        assert_eq!(style_in(&header, "⌁"), LOCAL_BRANCH.patch(INVISIBLE));
        assert_eq!(
            style_in(&header, "∙"),
            LOCAL_BRANCH.patch(TRAILING_WHITESPACE)
        );
    }

    #[test]
    fn replacements_are_bold_red_and_tabs_dim_gray() {
        let commit = invisible();
        let rows = commit_rows(&commit, 110);
        let Content::Text(body) = &rows[2].content else {
            panic!("the body is not split");
        };
        assert_eq!(style_in(body, "⍽"), INVISIBLE);
        assert_eq!(style_in(body, "∙"), TRAILING_WHITESPACE);
        let makefile = file_rows(&commit.files[2], 110);
        let Content::Split { right, .. } = &makefile[7].content else {
            panic!("chunk rows are split");
        };
        assert_eq!(text(right), "│ 5 │ ⇥   cc $(CFLAGS) -o app main.c");
        assert_eq!(style_in(right, "⇥   "), DIMMED_WHITESPACE);
        let Content::Split { right, .. } = &makefile[4].content else {
            panic!("chunk rows are split");
        };
        // A trailing tab is trailing whitespace.
        assert_eq!(text(right), "│ 2 │ all: build⇥ ");
        assert_eq!(style_in(right, "⇥ "), TRAILING_WHITESPACE);
    }

    #[test]
    fn tab_stops_are_counted_from_the_beginning_of_the_code() {
        let commit = invisible();
        let makefile = file_rows(&commit.files[2], 110);
        // `CFLAGS` is 6 columns wide, the next tab stop is at column 8.
        assert_eq!(text_of_row(&makefile[3]).1, "│ 1 │ CFLAGS⇥ = -O2");
    }

    #[test]
    fn carriage_returns_are_hidden_when_line_endings_are_consistent() {
        let commit = invisible();
        let emoji = file_rows(&commit.files[1], 110);
        let (left, right) = row_starting_with(&emoji, "│ 7 │");
        assert_eq!(left, "│ 7 │ const JOINED: &str = \"ab\";");
        assert_eq!(right, "│ 7 │ const JOINED: &str = \"a⌁b⌁\";");
    }

    #[test]
    fn carriage_returns_are_shown_when_line_endings_are_inconsistent() {
        let commit = invisible();
        let script = file_rows(&commit.files[3], 110);
        let (left, right) = row_starting_with(&script, "│ 3 │");
        assert_eq!(left, "│ 3 │ exit /b 0␍");
        assert_eq!(right, "│ 3 │ exit /b 0");
    }

    #[test]
    fn carriage_returns_of_the_body_are_shown() {
        let commit = invisible();
        let rows = commit_rows(&commit, 110);
        assert_eq!(
            text_of_row(&rows[3]).0,
            "The batch script now always uses CRLF line endings.␍"
        );
    }

    #[test]
    fn invisible_characters_of_the_file_name_are_replaced() {
        let mut commit = invisible();
        commit.files[0].path = "a\u{a0}b".to_string();
        let rows = commit_rows(&commit, 110);
        assert_eq!(text_of_row(&rows[5]).0, "a⍽b");
    }
}
