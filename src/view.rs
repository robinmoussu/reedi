//! Rendering of the commits and of the status bar.

use std::iter;
use std::path::PathBuf;

use gix::bstr::ByteSlice;
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::BorderType;
use ratatui::widgets::Clear;
use ratatui::widgets::Padding;
use ratatui::widgets::Widget;
use unicode_width::UnicodeWidthStr;

use crate::config::UnknownKey;
use crate::invisible::FinalCr;
use crate::invisible::Kind;
use crate::model::Chunk;
use crate::model::Commit;
use crate::model::Decoration;
use crate::model::DiffLine;
use crate::model::FileChange;
use crate::model::FileContentType;
use crate::model::FileDiff;
use crate::model::LineKind;
use crate::model::LineNumbers;
use crate::pairing;
use crate::theme::Theme;
use crate::viewport::Viewport;
use crate::wrap;
use crate::wrap::Segment;
use crate::wrap::TooNarrow;

/// Format used to display `Commit::date`.
const DATE_FORMAT: &str = "%d/%m/%Y";

/// Displayed in the last column of each half of a diff row followed by a continuation row of the
/// same line.
const WRAP_MARKER: &str = "↵";

/// Displayed below the last rows of a chunk, in the half of the version of the file that doesn't
/// end with a newline, if the other version does.
const MISSING_NEWLINE: &str = "\\ No newline at end of file";

/// Displayed in place of a chunk whose lines cannot be wrapped.
const CHUNK_TOO_NARROW: &str = "This chunk cannot be displayed: the terminal is too narrow.";

/// Put after the first character of each row and after the last character of each column.
///
/// Terminals reordering bidirectional text do it for the whole row. Without it, the punctuation
/// and numbers next to a right-to-left text (like Persian) would be moved with it, even from
/// another column, and a row starting with a right-to-left text may be displayed right-to-left.
const LEFT_TO_RIGHT_MARK: char = '\u{200e}';

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
                set_line(buf, area.x, area.y, line, area.width);
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
                set_line(buf, area.x, area.y, left, left_width);
                set_line(buf, area.x + left_width, area.y, right, right_width);
                if left_width > 0 {
                    mark_left_to_right(buf, area.x, area.y);
                    mark_left_to_right(buf, area.x + left_width - 1, area.y);
                }
            }
        }
    }
}

/// Render `line` at `x`, `y`, cropped to `width` columns. The columns after its end (up to `width`)
/// have the background of the style of `line`, if any.
fn set_line(buf: &mut Buffer, x: u16, y: u16, line: &Line, width: u16) {
    let (end, _) = buf.set_line(x, y, line, width);
    if let Some(background) = line.style.bg {
        let rest = Rect::new(end, y, (x + width).saturating_sub(end), 1);
        buf.set_style(rest.intersection(buf.area), Style::new().bg(background));
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
fn kind_style(kind: Kind, theme: &Theme) -> Style {
    match kind {
        Kind::Text => Style::new(),
        Kind::DimmedWhitespace => theme.dimmed_whitespace,
        Kind::TrailingWhitespace => theme.trailing_whitespace,
        Kind::Replacement => theme.invisible,
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
    theme: &Theme,
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
                    let style = parts[segment].style.patch(kind_style(piece.kind, theme));
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

fn single_decoration_parts(decoration: &Decoration, theme: &Theme) -> Vec<Part> {
    match decoration {
        Decoration::Head(None) => vec![Part::markup("HEAD", theme.head)],
        Decoration::Head(Some(branch)) => vec![
            Part::markup("HEAD", theme.head),
            Part::markup(" -> ", theme.decoration_punctuation),
            Part::content(branch, theme.local_branch),
        ],
        Decoration::LocalBranch(branch) => vec![Part::content(branch, theme.local_branch)],
        Decoration::RemoteBranch(branch) => vec![Part::content(branch, theme.remote_branch)],
        Decoration::Tag(tag) => vec![
            Part::markup("tag: ", theme.tag),
            Part::content(tag, theme.tag),
        ],
    }
}

/// ` ($decorations)`, or nothing if there are no decorations.
fn decoration_parts(decorations: &[Decoration], theme: &Theme) -> Vec<Part> {
    if decorations.is_empty() {
        return Vec::new();
    }
    let decorations = decorations.iter().enumerate().flat_map(|(i, decoration)| {
        let separator = if i > 0 {
            Some(Part::markup(", ", theme.decoration_punctuation))
        } else {
            None
        };
        separator
            .into_iter()
            .chain(single_decoration_parts(decoration, theme))
    });
    iter::once(Part::markup(" (", theme.decoration_punctuation))
        .chain(decorations)
        .chain(iter::once(Part::markup(")", theme.decoration_punctuation)))
        .collect()
}

/// `$hash $commit_subject $author $date ($tags, $branches)`
///
/// The trailing whitespace of each field is the one ending the field.
fn header_parts(commit: &Commit, theme: &Theme) -> Vec<Part> {
    let date = commit.date.strftime(DATE_FORMAT).to_string();
    let space = || Part::markup(" ", Style::new());
    [
        Part::content(&commit.hash, theme.hash),
        space(),
        Part::content(&commit.subject, Style::new()),
        space(),
        Part::content(&commit.author, theme.author),
        space(),
        Part::content(&date, theme.date),
    ]
    .into_iter()
    .chain(decoration_parts(&commit.decorations, theme))
    .collect()
}

/// The line number column of a diff row: `│ $line │ `, with `$line` displayed with its style, or
/// `│     │ ` without `number`.
fn number_column(
    number: Option<(usize, Style)>,
    width: usize,
    theme: &Theme,
) -> Vec<Span<'static>> {
    match number {
        Some((number, style)) => vec![
            Span::styled("│ ", theme.line_number),
            Span::styled(format!("{number:>width$}"), style),
            Span::styled(" │ ", theme.line_number),
        ],
        None => vec![Span::styled(
            format!("│ {:width$} │ ", ""),
            theme.line_number,
        )],
    }
}

/// A half of a row displaying only the line number column, without number.
fn filler(number_width: usize, theme: &Theme) -> Line<'static> {
    Line::from(number_column(None, number_width, theme))
}

/// The style of the text area of a line of `kind`: a background if it is modified.
fn line_style(kind: LineKind, theme: &Theme) -> Style {
    match kind {
        LineKind::Unchanged => Style::new(),
        LineKind::Removed => theme.removed_line,
        LineKind::Added => theme.added_line,
    }
}

/// The kind of the line of `chunk` that is the last one of a version of the file not ending with a
/// newline, while the other version does (see [`Chunk::shows_missing_newline`]), if any.
fn missing_newline_kind(chunk: &Chunk) -> Option<LineKind> {
    (0..chunk.lines.len())
        .find(|&index| chunk.shows_missing_newline(index))
        .map(|index| chunk.lines[index].kind)
}

/// The style of the number of a line of `kind`.
fn line_number_style(kind: LineKind, theme: &Theme) -> Style {
    match kind {
        LineKind::Unchanged => theme.unchanged_line_number,
        LineKind::Removed => theme.removed_line_number,
        LineKind::Added => theme.added_line_number,
    }
}

/// The rows displaying `line` (whose line number in its version of the file is `line_number`) in a
/// half of a split row, `width` columns wide, with the position of their first character. The line
/// number column is `number_width` wide.
///
/// If the line is modified, its text area (up to the end of the half) has the background of its
/// kind.
///
/// A carriage return ending the line is replaced if `final_cr` is [`FinalCr::Shown`], and hidden
/// otherwise.
fn half_rows(
    line: &DiffLine,
    line_number: usize,
    number_width: usize,
    width: usize,
    final_cr: FinalCr,
    theme: &Theme,
) -> Result<Vec<(usize, Line<'static>)>, TooNarrow> {
    let text_width = text_width(width, number_width);
    let background = line_style(line.kind, theme);
    let code = Part::content(&line.text, background);
    let rows = wrapped(&[code], final_cr, text_width, theme)?;
    let last = rows.len() - 1;
    Ok(rows
        .into_iter()
        .enumerate()
        .map(|(i, (start, columns_count, spans))| {
            let number = if i == 0 {
                Some((line_number, line_number_style(line.kind, theme)))
            } else {
                None
            };
            let marker = if i < last {
                vec![
                    Span::raw(" ".repeat(text_width.saturating_sub(columns_count))),
                    Span::styled(WRAP_MARKER, theme.marker),
                ]
            } else {
                Vec::new()
            };
            let spans: Vec<_> = number_column(number, number_width, theme)
                .into_iter()
                .chain(spans)
                .chain(marker)
                .collect();
            (start, Line::from(spans).style(background))
        })
        .collect())
}

/// The rows displaying `MISSING_NEWLINE` in a half of a split row, `width` columns wide, with the
/// position of their first character. The line number column is `number_width` wide.
fn missing_newline_rows(
    number_width: usize,
    width: usize,
    theme: &Theme,
) -> Result<Vec<(usize, Line<'static>)>, TooNarrow> {
    let message = Part::markup(MISSING_NEWLINE, theme.error);
    let rows = wrapped(
        &[message],
        FinalCr::Shown,
        text_width(width, number_width),
        theme,
    )?;
    Ok(rows
        .into_iter()
        .map(|(start, _, spans)| {
            let spans: Vec<_> = number_column(None, number_width, theme)
                .into_iter()
                .chain(spans)
                .collect();
            (start, Line::from(spans))
        })
        .collect())
}

/// Width of the text area of a half of a split row, `width` columns wide, whose line number column
/// is `number_width` wide.
fn text_width(width: usize, number_width: usize) -> usize {
    // `│ `, the number, ` │ `, then the text area, then the column of the marker.
    width.saturating_sub(number_width + 6)
}

/// `$file`, with a label if the file is not only modified (like `delta`), and a suffix if it has
/// no chunks because of its content.
fn file_parts(file: &FileDiff, theme: &Theme) -> Vec<Part> {
    // Invalid UTF-8 is replaced by `�`.
    let path = Part::content(&file.path.to_str_lossy(), theme.file);
    let name = match &file.change {
        FileChange::Modified => vec![path],
        FileChange::Added => vec![Part::markup("added: ", theme.file), path],
        FileChange::Deleted => vec![Part::markup("removed: ", theme.file), path],
        FileChange::Renamed { from } => vec![
            Part::markup("renamed: ", theme.file),
            Part::content(&from.to_str_lossy(), theme.file),
            Part::markup(" ⟶   ", theme.file),
            path,
        ],
    };
    let suffix = match file.content {
        FileContentType::Text => None,
        FileContentType::Binary => Some(Part::markup(" (binary file)", theme.file)),
        FileContentType::InvalidUtf8 => Some(Part::markup(" (invalid utf-8 content)", theme.error)),
    };
    name.into_iter().chain(suffix).collect()
}

/// The rows being built. `line` counts the lines added so far (a wrapped line is added as several
/// rows, all with the same [`Row::line`]).
struct Rows<'a> {
    rows: Vec<Row>,
    line: usize,
    width: u16,
    theme: &'a Theme,
}

impl<'a> Rows<'a> {
    fn new(width: u16, theme: &'a Theme) -> Self {
        Self {
            rows: Vec::new(),
            line: 0,
            width,
            theme,
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
        let rows = wrapped(parts, FinalCr::Shown, self.width.into(), self.theme)?;
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
        // Each pair of lines displayed side by side is a line (see `Row::line`), and so is
        // `MISSING_NEWLINE`.
        let len = pairing::pairs(chunk).len() + usize::from(missing_newline_kind(chunk).is_some());
        match self.chunk_rows(chunk, final_cr) {
            Ok(rows) => {
                self.rows.extend(rows);
                self.line += len;
            }
            Err(_) => {
                self.text(&[Part::markup(CHUNK_TOO_NARROW, self.theme.error)])?;
                // The error message takes the index of the first line.
                self.line += len.saturating_sub(1);
            }
        }
        Ok(())
    }

    /// The rows displaying the lines of `chunk`, the removed and added lines being paired like
    /// `delta` does. If one half of a pair has more rows than the other, or has no line, the other
    /// one is padded with rows displaying only the line number column.
    ///
    /// If a version of the file doesn't end with a newline, but the other one does, they are
    /// followed by `MISSING_NEWLINE`, in the half of this version.
    fn chunk_rows(&self, chunk: &Chunk, final_cr: FinalCr) -> Result<Vec<Row>, TooNarrow> {
        let number_width = chunk.line_number_width();
        let numbers = chunk.line_numbers();
        let left_width = usize::from(self.width / 2);
        let right_width = usize::from(self.width) - left_width;
        let half = |index: Option<usize>, number: fn(&LineNumbers) -> Option<usize>, width| {
            index.map_or(Ok(Vec::new()), |index| {
                half_rows(
                    &chunk.lines[index],
                    number(&numbers[index]).expect("the line is in this version"),
                    number_width,
                    width,
                    final_cr,
                    self.theme,
                )
            })
        };
        let mut lines = pairing::pairs(chunk)
            .into_iter()
            .map(|pair| {
                let left = half(pair.before, |numbers| numbers.before, left_width)?;
                let right = half(pair.after, |numbers| numbers.after, right_width)?;
                Ok((left, right))
            })
            .collect::<Result<Vec<_>, TooNarrow>>()?;
        match missing_newline_kind(chunk) {
            Some(LineKind::Removed) => {
                lines.push((
                    missing_newline_rows(number_width, left_width, self.theme)?,
                    Vec::new(),
                ));
            }
            Some(LineKind::Added) => {
                lines.push((
                    Vec::new(),
                    missing_newline_rows(number_width, right_width, self.theme)?,
                ));
            }
            Some(LineKind::Unchanged) | None => {}
        }
        Ok(lines
            .into_iter()
            .enumerate()
            .flat_map(|(index, (left, right))| {
                let len = left.len().max(right.len());
                let padded = |rows: Vec<(usize, Line<'static>)>| {
                    rows.into_iter()
                        .map(|(start, line)| (Some(start), line))
                        .chain(iter::repeat_with(|| {
                            (None, filler(number_width, self.theme))
                        }))
                        .take(len)
                };
                let line = self.line + index;
                padded(left)
                    .zip(padded(right))
                    .map(move |((left_start, left), (right_start, right))| Row {
                        content: Content::Split { left, right },
                        line,
                        start: Start::Split {
                            left: left_start,
                            right: right_start,
                        },
                    })
                    .collect::<Vec<_>>()
            })
            .collect())
    }

    fn file(&mut self, file: &FileDiff) -> Result<(), TooNarrow> {
        let final_cr = if file.has_inconsistent_line_endings() {
            FinalCr::Shown
        } else {
            FinalCr::Hidden
        };
        self.blank();
        self.text(&file_parts(file, self.theme))?;
        self.rule(self.theme.file);
        file.chunks.iter().enumerate().try_for_each(|(i, chunk)| {
            if i > 0 {
                self.blank();
            }
            self.chunk(chunk, final_cr)
        })
    }

    /// Add `commit`, including the separator below it.
    fn commit(&mut self, commit: &Commit) -> Result<(), TooNarrow> {
        self.text(&header_parts(commit, self.theme))?;
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
        self.rule(self.theme.commit_separator);
        Ok(())
    }
}

/// All the rows displaying `commits` on a screen `width` columns wide.
///
/// Fails if a text area is too narrow to display a grapheme cluster of its line, unless it is in
/// a chunk, which is then replaced by an error message.
pub fn rows(commits: &[Commit], width: u16, theme: &Theme) -> Result<Vec<Row>, TooNarrow> {
    let mut rows = Rows::new(width, theme);
    commits.iter().try_for_each(|commit| rows.commit(commit))?;
    Ok(rows.rows)
}

/// Rows displayed from `viewport.top`, cropped if they don't fit, with the row of the cursor
/// highlighted.
pub struct RowsView<'a> {
    pub rows: &'a [Row],
    pub viewport: Viewport,
    pub theme: &'a Theme,
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
                    buf.set_style(area, self.theme.cursor);
                }
            });
    }
}

pub struct StatusBar<'a> {
    pub mode: Mode,
    pub theme: &'a Theme,
}

impl Widget for StatusBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let style = match self.mode {
            Mode::Review => self.theme.review_mode,
        };
        let mode = Span::styled(format!(" {} ", self.mode.name()), style);
        Line::from(mode).render(area, buf);
    }
}

/// A message displayed over the commits, until a key is pressed.
#[derive(Debug, Clone, PartialEq)]
pub enum ErrorModal {
    /// The keys of the config file at `path` that are not settings.
    UnknownKeys {
        path: PathBuf,
        unknown_keys: Vec<UnknownKey>,
    },
}

impl ErrorModal {
    fn title(&self) -> String {
        match self {
            ErrorModal::UnknownKeys { path, .. } => {
                format!("Unknown key in config file: {}", path.display())
            }
        }
    }

    fn lines(&self) -> Vec<String> {
        match self {
            ErrorModal::UnknownKeys { unknown_keys, .. } => unknown_keys
                .iter()
                .map(|unknown_key| format!("line {}: {unknown_key}", unknown_key.line))
                .collect(),
        }
    }
}

/// Displayed centered in the bottom border of the modals.
const CLOSE_MODAL: &str = "Press any key to close";

/// `error_modal` centered in the area, with a border and a blank row above and below its content,
/// as wide as its content (at most the width of the area), its long lines being wrapped. The border
/// has heavy lines and the error style, and the title the error style in reverse video.
/// `CLOSE_MODAL` is in the bottom border.
struct ErrorModalView<'a> {
    error_modal: &'a ErrorModal,
    theme: &'a Theme,
}

impl Widget for ErrorModalView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let title = format!(" {} ", self.error_modal.title());
        let close = format!(" {CLOSE_MODAL} ");
        // The border and the padding, on each side.
        let max_width = usize::from(area.width).saturating_sub(4);
        // Like the chunks, the content is not displayed if it is too narrow to be wrapped.
        let rows: Vec<_> = self
            .error_modal
            .lines()
            .iter()
            .map(|line| {
                let line = Part::content(line, Style::new());
                wrapped(&[line], FinalCr::Shown, max_width, self.theme)
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|rows| rows.into_iter().flatten().collect())
            .unwrap_or_default();
        let content_width = rows
            .iter()
            .map(|(_, columns_count, _)| *columns_count)
            .chain([title.width(), close.width()])
            .max()
            .unwrap_or(0)
            .min(max_width);
        let width = u16::try_from(content_width + 4)
            .unwrap_or(u16::MAX)
            .min(area.width);
        // The border and the padding, on each side.
        let height = u16::try_from(rows.len() + 4)
            .unwrap_or(u16::MAX)
            .min(area.height);
        let error_modal_area = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        let block = Block::bordered()
            .border_type(BorderType::Thick)
            .title(Line::styled(title, self.theme.error.reversed()))
            .border_style(self.theme.error)
            .title_bottom(Line::styled(close, self.theme.error).centered())
            .padding(Padding::uniform(1));
        let inner = block.inner(error_modal_area);
        Clear.render(error_modal_area, buf);
        block.render(error_modal_area, buf);
        rows.into_iter()
            .zip(inner.top()..inner.bottom())
            .for_each(|((_, _, spans), y)| {
                buf.set_line(inner.x, y, &Line::from(spans), inner.width);
                if inner.width > 0 {
                    mark_left_to_right(buf, inner.x, y);
                }
            });
    }
}

/// Draw `rows`, with the status bar on the last line of the screen, and `error_modal` over them.
///
/// `viewport` is first scrolled to fit the screen.
pub fn draw(
    frame: &mut Frame,
    rows: &[Row],
    viewport: &mut Viewport,
    mode: Mode,
    error_modal: Option<&ErrorModal>,
    theme: &Theme,
) {
    let [content, status_bar] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());
    viewport.scroll(rows.len(), content.height.into());
    let viewport = *viewport;
    frame.render_widget(
        RowsView {
            rows,
            viewport,
            theme,
        },
        content,
    );
    frame.render_widget(StatusBar { mode, theme }, status_bar);
    if let Some(error_modal) = error_modal {
        frame.render_widget(ErrorModalView { error_modal, theme }, frame.area());
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::Backend;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;
    use ratatui::style::Modifier;

    use super::*;
    use crate::fake::FakeCommit;
    use crate::theme::Theme;

    fn commit_rows(commit: &Commit, width: u16, theme: &Theme) -> Vec<Row> {
        let mut rows = Rows::new(width, theme);
        rows.commit(commit).unwrap();
        rows.rows
    }

    /// Index of the first row of the fake commit `commit`, on a screen `width` columns wide.
    fn first_row(commit: &Commit, width: u16) -> usize {
        FakeCommit::all()
            .iter()
            .take_while(|other| other.hash != commit.hash)
            .map(|other| commit_rows(other, width, &Theme::monochrome()).len())
            .sum()
    }

    /// A viewport whose first row is the first row of the fake `commit`.
    fn viewport_at(commit: &Commit, width: u16) -> Viewport {
        let top = first_row(commit, width);
        Viewport {
            cursor: top + 5,
            top,
        }
    }

    /// The rows of `file`, starting with the blank row above the file name.
    fn file_rows(file: &FileDiff, width: u16, theme: &Theme) -> Vec<Row> {
        let mut rows = Rows::new(width, theme);
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
    fn header(commit: &Commit, theme: &Theme) -> Line<'static> {
        let (_, _, spans) = wrapped(&header_parts(commit, theme), FinalCr::Shown, 1000, theme)
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

    /// The themes with colors.
    fn themes() -> [Theme; 2] {
        [Theme::dark(), Theme::light()]
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
    fn render(width: u16, height: u16, viewport: Viewport, theme: &Theme) -> TestBackend {
        render_with_error_modal(width, height, viewport, None, theme)
    }

    /// The screen displaying the fake commits and `error_modal`, without the left-to-right marks.
    fn render_with_error_modal(
        width: u16,
        height: u16,
        viewport: Viewport,
        error_modal: Option<&ErrorModal>,
        theme: &Theme,
    ) -> TestBackend {
        let commits = FakeCommit::all();
        let rows = rows(&commits, width, theme).unwrap();
        let mut viewport = viewport;
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &rows,
                    &mut viewport,
                    Mode::Review,
                    error_modal,
                    theme,
                )
            })
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
        insta::assert_snapshot!(render(110, 26, Viewport::default(), &Theme::monochrome()));
    }

    #[test]
    fn small_screen_is_wrapped_and_cropped() {
        insta::assert_snapshot!(render(50, 8, Viewport::default(), &Theme::monochrome()));
    }

    #[test]
    fn scrolled_to_the_end() {
        let len = rows(&FakeCommit::all(), 110, &Theme::monochrome())
            .unwrap()
            .len();
        let viewport = Viewport {
            cursor: len - 1,
            top: 0,
        };
        insta::assert_snapshot!(render(110, 26, viewport, &Theme::monochrome()));
    }

    #[test]
    fn all_kinds_of_file_changes_are_displayed() {
        let commit = FakeCommit::FileChanges.commit();
        let height = commit_rows(&commit, 110, &Theme::monochrome()).len() + 1;
        let viewport = viewport_at(&commit, 110);
        insta::assert_snapshot!(render(110, height as u16, viewport, &Theme::monochrome()));
    }

    #[test]
    fn long_lines_are_wrapped() {
        insta::assert_snapshot!(render(
            110,
            60,
            viewport_at(&FakeCommit::LineWrapping.commit(), 110),
            &Theme::monochrome()
        ));
    }

    #[test]
    fn long_lines_are_wrapped_on_a_narrow_screen() {
        insta::assert_snapshot!(render(
            70,
            80,
            viewport_at(&FakeCommit::LineWrapping.commit(), 70),
            &Theme::monochrome()
        ));
    }

    #[test]
    fn invisible_characters_after_the_first_screen() {
        let start = first_row(&FakeCommit::InvisibleCharacters.commit(), 110);
        let viewport = Viewport {
            cursor: start + 30,
            top: start + 20,
        };
        insta::assert_snapshot!(render(110, 26, viewport, &Theme::monochrome()));
    }

    #[test]
    fn rows_of_all_commits_are_displayed_one_after_the_other() {
        let commits = FakeCommit::all();
        let all_rows: Vec<_> = rows(&commits, 110, &Theme::monochrome())
            .unwrap()
            .into_iter()
            .map(|row| row.content)
            .collect();
        let expected: Vec<_> = commits
            .iter()
            .flat_map(|commit| commit_rows(commit, 110, &Theme::monochrome()))
            .map(|row| row.content)
            .collect();
        assert_eq!(all_rows, expected);
    }

    #[test]
    fn lines_are_numbered_consecutively() {
        let rows = rows(&FakeCommit::all(), 110, &Theme::monochrome()).unwrap();
        assert_eq!(rows[0].line, 0);
        assert!(
            rows.windows(2)
                .all(|rows| rows[1].line == rows[0].line || rows[1].line == rows[0].line + 1)
        );
    }

    #[test]
    fn cursor_row_is_highlighted() {
        let viewport = Viewport { cursor: 2, top: 0 };
        themes().iter().for_each(|theme| {
            let backend = render(110, 26, viewport, theme);
            let buffer = backend.buffer();
            let cursor = theme.cursor.bg.unwrap();
            assert!((0..110).all(|x| buffer[(x, 2)].bg == cursor));
            assert!((0..110).all(|x| buffer[(x, 1)].bg == Color::Reset));
            assert!((0..110).all(|x| buffer[(x, 3)].bg == Color::Reset));
        });
    }

    #[test]
    fn cursor_row_is_highlighted_when_scrolled() {
        let viewport = Viewport {
            cursor: 30,
            top: 20,
        };
        themes().iter().for_each(|theme| {
            let backend = render(110, 26, viewport, theme);
            let buffer = backend.buffer();
            assert_eq!(buffer[(0, 10)].bg, theme.cursor.bg.unwrap());
            assert_eq!(buffer[(0, 9)].bg, Color::Reset);
        });
    }

    #[test]
    fn cursor_row_is_in_reverse_video_without_colors() {
        let viewport = Viewport { cursor: 2, top: 0 };
        let backend = render(110, 26, viewport, &Theme::monochrome());
        let buffer = backend.buffer();
        assert!((0..110).all(|x| buffer[(x, 2)].modifier.contains(Modifier::REVERSED)));
        assert!((0..110).all(|x| !buffer[(x, 1)].modifier.contains(Modifier::REVERSED)));
        assert!((0..110).all(|x| !buffer[(x, 3)].modifier.contains(Modifier::REVERSED)));
    }

    #[test]
    fn header_is_colored_like_git() {
        let commit = FakeCommit::newest();
        themes().iter().for_each(|theme| {
            let header = header(&commit, theme);
            assert_eq!(
                text(&header),
                "3f9a2c1 config: rename scroll_off to scrolloff Alice Martin 12/10/2026 \
                 (HEAD -> main, tag: v1.0, origin/main)"
            );
            assert_eq!(style_in(&header, "3f9a2c1"), theme.hash);
            assert_eq!(style_in(&header, "Alice Martin"), theme.author);
            assert_eq!(style_in(&header, "12/10/2026"), theme.date);
            assert_eq!(style_in(&header, "HEAD"), theme.head);
            assert_eq!(style_in(&header, " -> "), theme.decoration_punctuation);
            assert_eq!(style_in(&header, "main"), theme.local_branch);
            assert_eq!(style_in(&header, "v1.0"), theme.tag);
            assert_eq!(style_in(&header, "origin/main"), theme.remote_branch);
        });
    }

    #[test]
    fn decorations_are_omitted_when_empty() {
        let mut commit = FakeCommit::newest();
        commit.decorations.clear();
        assert!(text(&header(&commit, &Theme::monochrome())).ends_with("12/10/2026"));
    }

    #[test]
    fn decorations_without_head() {
        let decorations = [
            Decoration::LocalBranch("feature".to_string()),
            Decoration::Head(None),
        ];
        let text: String = decoration_parts(&decorations, &Theme::monochrome())
            .into_iter()
            .map(|part| part.text)
            .collect();
        assert_eq!(text, " (feature, HEAD)");
    }

    #[test]
    fn body_is_omitted_when_empty() {
        let mut commit = FakeCommit::newest();
        commit.body.clear();
        let rows = commit_rows(&commit, 110, &Theme::monochrome());
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
        let commit = FakeCommit::newest();
        themes().iter().for_each(|theme| {
            let rows = Rows::new(110, theme)
                .chunk_rows(&commit.files[0].chunks[1], FinalCr::Hidden)
                .unwrap();
            let Content::Split { left, right } = &rows[0].content else {
                panic!("chunk rows are split");
            };
            assert_eq!(text(left), "│  98 │     fn default() -> Self {");
            assert_eq!(text(right), "│  98 │     fn default() -> Self {");
            assert_eq!(left.spans[0].style, theme.line_number);
        });
    }

    /// A chunk replacing the line `before` by the line `after`, displayed side by side if they are
    /// similar enough.
    fn chunk(before: &str, after: &str) -> Chunk {
        let line = |kind, text: &str| DiffLine {
            kind,
            text: text.to_string(),
            missing_newline: false,
        };
        Chunk {
            before_first_line: 1,
            after_first_line: 1,
            lines: vec![
                line(LineKind::Removed, before),
                line(LineKind::Added, after),
            ],
        }
    }

    #[test]
    fn wrapped_diff_lines_have_a_marker_and_no_line_number_on_continuation_rows() {
        themes().iter().for_each(|theme| {
            // Each half is 12 columns wide: 6 for the line number column, 5 for the text, and 1
            // for the marker.
            let rows = Rows::new(24, theme)
                .chunk_rows(&chunk("aaaa bbbb cccc", "aaaa"), FinalCr::Hidden)
                .unwrap();
            let texts: Vec<_> = rows.iter().map(text_of_row).collect();
            assert_eq!(
                texts,
                [
                    ("│ 1 │ aaaa ↵".to_string(), "│ 1 │ aaaa".to_string()),
                    ("│   │ bbbb ↵".to_string(), "│   │ ".to_string()),
                    ("│   │ cccc".to_string(), "│   │ ".to_string()),
                ]
            );
            let Content::Split { left, .. } = &rows[0].content else {
                panic!("chunk rows are split");
            };
            assert_eq!(style_in(left, WRAP_MARKER), theme.marker);
            let area = Rect::new(0, 0, 24, 1);
            let mut buf = Buffer::empty(area);
            rows[0].render(area, &mut buf);
            let buf = without_left_to_right_marks(&buf);
            // In the last column of the left half.
            assert_eq!(buf[(11, 0)].symbol(), WRAP_MARKER);
        });
    }

    #[test]
    fn marker_is_in_the_last_column_of_the_right_half() {
        let rows = Rows::new(24, &Theme::monochrome())
            .chunk_rows(&chunk("aaaa", "aaaa bbbb"), FinalCr::Hidden)
            .unwrap();
        let area = Rect::new(0, 0, 24, 1);
        let mut buf = Buffer::empty(area);
        rows[0].render(area, &mut buf);
        assert_eq!(buf[(23, 0)].symbol(), WRAP_MARKER);
    }

    #[test]
    fn unpaired_lines_have_a_filler_on_the_other_side() {
        let rows = Rows::new(24, &Theme::monochrome())
            .chunk_rows(&chunk("aaaa", "bbbb"), FinalCr::Hidden)
            .unwrap();
        let texts: Vec<_> = rows.iter().map(text_of_row).collect();
        assert_eq!(
            texts,
            [
                ("│ 1 │ aaaa".to_string(), "│   │ ".to_string()),
                ("│   │ ".to_string(), "│ 1 │ bbbb".to_string()),
            ]
        );
        let starts: Vec<_> = rows.iter().map(|row| (row.line, row.start)).collect();
        assert_eq!(
            starts,
            [
                (
                    0,
                    Start::Split {
                        left: Some(0),
                        right: None
                    }
                ),
                (
                    1,
                    Start::Split {
                        left: None,
                        right: Some(0)
                    }
                ),
            ]
        );
    }

    #[test]
    fn line_numbers_are_colored_like_delta() {
        let mut chunk = chunk("aaaa bbbb", "aaaa");
        chunk.lines.insert(
            0,
            DiffLine {
                kind: LineKind::Unchanged,
                text: "z".to_string(),
                missing_newline: false,
            },
        );
        let halves = |row: &Row| match &row.content {
            Content::Split { left, right } => (left.clone(), right.clone()),
            _ => panic!("chunk rows are split"),
        };
        themes().iter().for_each(|theme| {
            let rows = Rows::new(40, theme)
                .chunk_rows(&chunk, FinalCr::Hidden)
                .unwrap();
            let (left, right) = halves(&rows[0]);
            assert_eq!(style_in(&left, "1"), theme.unchanged_line_number);
            assert_eq!(style_in(&right, "1"), theme.unchanged_line_number);
            let (left, right) = halves(&rows[1]);
            assert_eq!(style_in(&left, "2"), theme.removed_line_number);
            assert_eq!(style_in(&right, "2"), theme.added_line_number);
            // The borders keep their color.
            assert_eq!(style_in(&left, "│ "), theme.line_number);
            assert_eq!(style_in(&left, " │ "), theme.line_number);
        });
    }

    /// The background colors of the cells of `row`, rendered `width` columns wide.
    fn backgrounds(row: &Row, width: u16) -> Vec<Color> {
        let area = Rect::new(0, 0, width, 1);
        let mut buf = Buffer::empty(area);
        row.render(area, &mut buf);
        buf.content.iter().map(|cell| cell.bg).collect()
    }

    #[test]
    fn modified_lines_have_a_red_or_green_background_up_to_the_end_of_their_half() {
        themes().iter().for_each(|theme| {
            let removed = theme.removed_line.bg.unwrap();
            let added = theme.added_line.bg.unwrap();
            // Each half is 20 columns wide: 6 for the line number column, 13 for the text, and 1
            // for the marker.
            let rows = Rows::new(40, theme)
                .chunk_rows(&chunk("aaaa bbbb cccc dddd", "aaaa bbbb"), FinalCr::Hidden)
                .unwrap();
            let first = backgrounds(&rows[0], 40);
            assert!(first[..6].iter().all(|bg| *bg == Color::Reset));
            assert!(first[6..20].iter().all(|bg| *bg == removed));
            assert!(first[20..26].iter().all(|bg| *bg == Color::Reset));
            assert!(first[26..].iter().all(|bg| *bg == added));
            // The continuation row of the removed line, and the padding of the added line.
            let second = backgrounds(&rows[1], 40);
            assert!(second[..6].iter().all(|bg| *bg == Color::Reset));
            assert!(second[6..20].iter().all(|bg| *bg == removed));
            assert!(second[20..].iter().all(|bg| *bg == Color::Reset));
        });
    }

    #[test]
    fn replacements_of_modified_lines_keep_their_background() {
        themes().iter().for_each(|theme| {
            let rows = Rows::new(40, theme)
                .chunk_rows(&chunk("a b", "a\u{a0}b"), FinalCr::Hidden)
                .unwrap();
            let Content::Split { right, .. } = &rows[0].content else {
                panic!("chunk rows are split");
            };
            assert_eq!(style_in(right, "⍽").bg, theme.added_line.bg);
            assert_eq!(style_in(right, "⍽").fg, theme.invisible.fg);
        });
    }

    #[test]
    fn unchanged_lines_and_fillers_have_no_background() {
        let mut chunk = chunk("aaaa", "bbbb");
        chunk.lines.insert(
            0,
            DiffLine {
                kind: LineKind::Unchanged,
                text: "z".to_string(),
                missing_newline: false,
            },
        );
        themes().iter().for_each(|theme| {
            let rows = Rows::new(40, theme)
                .chunk_rows(&chunk, FinalCr::Hidden)
                .unwrap();
            assert!(
                backgrounds(&rows[0], 40)
                    .iter()
                    .all(|bg| *bg == Color::Reset)
            );
            // The removed line is unpaired, so the right half is a filler.
            assert!(
                backgrounds(&rows[1], 40)[20..]
                    .iter()
                    .all(|bg| *bg == Color::Reset)
            );
        });
    }

    #[test]
    fn missing_newline_is_displayed_below_the_last_rows_of_both_halves() {
        // Each half is 40 columns wide: the right line is wrapped, not the left one.
        let mut chunk = chunk("aaaa bbbb cccc", "aaaa bbbb cccc dddd eeee ffff gggg hhhh");
        chunk.lines[0].missing_newline = true;
        let rows = Rows::new(80, &Theme::monochrome())
            .chunk_rows(&chunk, FinalCr::Hidden)
            .unwrap();
        let texts: Vec<_> = rows.iter().map(text_of_row).collect();
        assert_eq!(
            texts,
            [
                (
                    "│ 1 │ aaaa bbbb cccc".to_string(),
                    "│ 1 │ aaaa bbbb cccc dddd eeee ffff    ↵".to_string()
                ),
                ("│   │ ".to_string(), "│   │ gggg hhhh".to_string()),
                (
                    "│   │ \\ No newline at end of file".to_string(),
                    "│   │ ".to_string()
                ),
            ]
        );
        // It is a line of its own.
        assert_eq!(rows[2].line, rows[1].line + 1);
        assert_eq!(
            rows[2].start,
            Start::Split {
                left: Some(0),
                right: None
            }
        );
    }

    #[test]
    fn missing_newline_has_the_error_color() {
        let mut chunk = chunk("aaaa", "aaaa bbbb");
        chunk.lines[0].missing_newline = true;
        themes().iter().for_each(|theme| {
            let rows = Rows::new(80, theme)
                .chunk_rows(&chunk, FinalCr::Hidden)
                .unwrap();
            let Content::Split { left, .. } = &rows[1].content else {
                panic!("chunk rows are split");
            };
            assert_eq!(style_in(left, MISSING_NEWLINE), theme.error);
        });
    }

    #[test]
    fn missing_newline_is_displayed_below_the_lines_added_after_it() {
        let mut chunk = chunk("aaaa", "aaaa");
        chunk.lines[0].missing_newline = true;
        chunk.lines.push(DiffLine {
            kind: LineKind::Added,
            text: "bbbb".to_string(),
            missing_newline: false,
        });
        let rows = Rows::new(80, &Theme::monochrome())
            .chunk_rows(&chunk, FinalCr::Hidden)
            .unwrap();
        let texts: Vec<_> = rows.iter().map(text_of_row).collect();
        assert_eq!(
            texts,
            [
                ("│ 1 │ aaaa".to_string(), "│ 1 │ aaaa".to_string()),
                ("│   │ ".to_string(), "│ 2 │ bbbb".to_string()),
                (
                    "│   │ \\ No newline at end of file".to_string(),
                    "│   │ ".to_string()
                ),
            ]
        );
    }

    #[test]
    fn missing_newline_is_counted_as_a_line_when_the_chunk_is_too_narrow() {
        let mut chunk = chunk("aaaa", "aaaa");
        chunk.lines[0].missing_newline = true;
        let theme = Theme::monochrome();
        let mut rows = Rows::new(12, &theme);
        rows.chunk(&chunk, FinalCr::Hidden).unwrap();
        // The pair of lines, and the line displaying `MISSING_NEWLINE`.
        assert_eq!(rows.line, 2);
    }

    #[test]
    fn invalid_utf8_suffix_has_the_error_color() {
        let file = FileDiff {
            path: "a".into(),
            change: FileChange::Modified,
            content: FileContentType::InvalidUtf8,
            chunks: Vec::new(),
        };
        themes().iter().for_each(|theme| {
            let Content::Text(name) = &file_rows(&file, 110, theme)[1].content else {
                panic!("the file name is not split");
            };
            assert_eq!(style_in(name, "a"), theme.file);
            assert_eq!(style_in(name, " (invalid utf-8 content)"), theme.error);
        });
    }

    #[test]
    fn file_name_has_a_label_and_a_suffix_like_delta() {
        let name = |change, content| {
            let file = FileDiff {
                path: "b".into(),
                change,
                content,
                chunks: Vec::new(),
            };
            text_of_row(&file_rows(&file, 110, &Theme::monochrome())[1]).0
        };
        let renamed = FileChange::Renamed { from: "a".into() };
        assert_eq!(name(FileChange::Modified, FileContentType::Text), "b");
        assert_eq!(name(FileChange::Added, FileContentType::Text), "added: b");
        assert_eq!(
            name(FileChange::Deleted, FileContentType::Text),
            "removed: b"
        );
        assert_eq!(name(renamed, FileContentType::Text), "renamed: a ⟶   b");
        assert_eq!(
            name(FileChange::Modified, FileContentType::Binary),
            "b (binary file)"
        );
        assert_eq!(
            name(FileChange::Added, FileContentType::InvalidUtf8),
            "added: b (invalid utf-8 content)"
        );
    }

    #[test]
    fn invalid_utf8_of_the_file_names_is_replaced() {
        let file = FileDiff {
            path: b"caf\xe9.txt".into(),
            change: FileChange::Renamed {
                from: b"\xff.txt".into(),
            },
            content: FileContentType::Text,
            chunks: Vec::new(),
        };
        let name = text_of_row(&file_rows(&file, 110, &Theme::monochrome())[1]).0;
        assert_eq!(name, "renamed: \u{fffd}.txt ⟶   caf\u{fffd}.txt");
    }

    #[test]
    fn rows_of_a_diff_line_start_at_their_first_character() {
        let rows = Rows::new(24, &Theme::monochrome())
            .chunk_rows(&chunk("aaaa bbbb cccc", "aaaa"), FinalCr::Hidden)
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
        let rows = commit_rows(
            &FakeCommit::LineWrapping.commit(),
            110,
            &Theme::monochrome(),
        );
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
        let mut commit = FakeCommit::newest();
        commit.files[0].path = "src/a/very/long/path/to/the/config.rs".into();
        let rows = commit_rows(&commit, 20, &Theme::monochrome());
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
        themes().iter().for_each(|theme| {
            let rows = commit_rows(&FakeCommit::newest(), 93, theme);
            let (first, second) = (&rows[0].content, &rows[1].content);
            let (Content::Text(first), Content::Text(second)) = (first, second) else {
                panic!("the header is not split");
            };
            // The space of the markup before the break is not replaced.
            assert!(text(first).ends_with("(HEAD -> main, tag: "));
            assert_eq!(first.spans.last().unwrap().style, theme.tag);
            assert_eq!(text(second), "v1.0, origin/main)");
            assert_eq!(second.spans[0].style, theme.tag);
        });
    }

    #[test]
    fn tab_stops_of_the_header_are_counted_from_the_start_of_the_row() {
        let mut commit = FakeCommit::InvisibleCharacters.commit();
        commit.subject = "xy".to_string();
        commit.author = "a\tb".to_string();
        // The tab is at column 12.
        assert_eq!(
            text(&header(&commit, &Theme::monochrome())),
            "c7e05b9 xy a⇥   b 05/10/2026"
        );
    }

    #[test]
    fn chunk_too_narrow_is_replaced_by_an_error_message() {
        let commit = FakeCommit::newest();
        themes().iter().for_each(|theme| {
            // The text area of each half has no column.
            let rows = commit_rows(&commit, 12, theme);
            assert!(
                rows.iter()
                    .all(|row| !matches!(row.content, Content::Split { .. }))
            );
            let errors: Vec<_> = rows
                .iter()
                .filter_map(|row| match &row.content {
                    Content::Text(line) if line.spans.first()?.style == theme.error => {
                        Some(row.line)
                    }
                    _ => None,
                })
                .collect();
            assert!(!errors.is_empty());
            // The lines after the chunks keep the same index.
            let wide = commit_rows(&commit, 110, theme);
            assert_eq!(rows.last().unwrap().line, wide.last().unwrap().line);
        });
    }

    #[test]
    fn chunk_too_narrow_for_the_error_message_is_an_error() {
        let commit = FakeCommit::newest();
        let error = Rows::new(0, &Theme::monochrome())
            .chunk(&commit.files[0].chunks[0], FinalCr::Hidden)
            .unwrap_err();
        assert_eq!(error.width, 0);
    }

    #[test]
    fn text_area_too_narrow_outside_of_a_chunk_is_an_error() {
        let mut commit = FakeCommit::newest();
        commit.author = "日本".to_string();
        let error = rows(&[commit], 1, &Theme::monochrome()).unwrap_err();
        assert_eq!(
            error,
            TooNarrow {
                width: 1,
                cluster: "日".to_string()
            }
        );
        assert!(rows(&FakeCommit::all(), 0, &Theme::monochrome()).is_err());
    }

    #[test]
    fn anchor_is_on_the_right_half_if_it_displays_text() {
        let rows = Rows::new(24, &Theme::monochrome())
            .chunk_rows(&chunk("aaaa bbbb", "aaaa dddd"), FinalCr::Hidden)
            .unwrap();
        assert_eq!(
            rows[1].anchor(),
            Anchor {
                line: 0,
                half: Half::Right,
                offset: 5
            }
        );
        let rows = Rows::new(24, &Theme::monochrome())
            .chunk_rows(&chunk("aaaa bbbb", "aaaa"), FinalCr::Hidden)
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
        let commits = FakeCommit::all();
        let wide = rows(&commits, 110, &Theme::monochrome()).unwrap();
        let narrow = rows(&commits, 60, &Theme::monochrome()).unwrap();
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
        let commit = FakeCommit::newest();
        let wide = commit_rows(&commit, 110, &Theme::monochrome());
        let narrow = commit_rows(&commit, 12, &Theme::monochrome());
        let code = wide
            .iter()
            .find(|row| matches!(row.content, Content::Split { .. }))
            .unwrap();
        let found = &narrow[find(&narrow, code.anchor())];
        assert!(found.line >= code.line);
    }

    #[test]
    fn rules_and_file_name_are_colored() {
        let commit = FakeCommit::newest();
        themes().iter().for_each(|theme| {
            let rows = commit_rows(&commit, 110, theme);
            let area = Rect::new(0, 0, 110, 26);
            let mut buffer = Buffer::empty(area);
            RowsView {
                rows: &rows,
                viewport: Viewport::default(),
                theme,
            }
            .render(area, &mut buffer);
            let buffer = without_left_to_right_marks(&buffer);
            // File name, then the rule below it.
            assert_eq!(buffer[(0, 5)].symbol(), "s");
            assert_eq!(buffer[(0, 5)].fg, theme.file.fg.unwrap());
            assert_eq!(buffer[(109, 6)].symbol(), "─");
            assert_eq!(buffer[(109, 6)].fg, theme.file.fg.unwrap());
            // Separator below the commit.
            assert_eq!(buffer[(109, 25)].symbol(), "─");
            assert_eq!(buffer[(109, 25)].fg, theme.commit_separator.fg.unwrap());
        });
    }

    #[test]
    fn status_bar_displays_the_mode() {
        themes().iter().for_each(|theme| {
            let backend = render(110, 26, Viewport::default(), theme);
            let buffer = backend.buffer();
            let status_bar: String = (0..8).map(|x| buffer[(x, 25)].symbol()).collect();
            assert_eq!(status_bar, " Review ");
            assert_eq!(buffer[(1, 25)].bg, theme.review_mode.bg.unwrap());
        });
    }

    #[test]
    fn mode_is_in_reverse_video_and_bold_without_colors() {
        let backend = render(110, 26, Viewport::default(), &Theme::monochrome());
        let buffer = backend.buffer();
        let status_bar: String = (0..8).map(|x| buffer[(x, 25)].symbol()).collect();
        assert_eq!(status_bar, " Review ");
        assert!((0..8).all(|x| {
            buffer[(x, 25)]
                .modifier
                .contains(Modifier::REVERSED | Modifier::BOLD)
        }));
    }

    /// All the rows of the fake commits on a single screen, with the cursor on the first row.
    fn render_all(theme: &Theme) -> TestBackend {
        let height = rows(&FakeCommit::all(), 110, theme).unwrap().len() + 1;
        render(110, height as u16, Viewport::default(), theme)
    }

    #[test]
    fn monochrome_theme_displays_no_color() {
        let backend = render_all(&Theme::monochrome());
        assert!(backend.buffer().content.iter().all(|cell| {
            cell.fg == Color::Reset
                && cell.bg == Color::Reset
                && cell.underline_color == Color::Reset
        }));
    }

    #[test]
    fn monochrome_theme_keeps_bold() {
        let bold = |backend: TestBackend| -> Vec<bool> {
            backend
                .buffer()
                .content
                .iter()
                .map(|cell| cell.modifier.contains(Modifier::BOLD))
                .collect()
        };
        assert_eq!(
            bold(render_all(&Theme::monochrome())),
            bold(render_all(&Theme::dark()))
        );
    }

    #[test]
    fn invisible_characters_of_the_header_are_replaced_per_field() {
        let mut commit = FakeCommit::InvisibleCharacters.commit();
        commit.decorations = vec![Decoration::LocalBranch("fix\u{200b} ".to_string())];
        themes().iter().for_each(|theme| {
            let header = header(&commit, theme);
            assert_eq!(
                text(&header),
                "c7e05b9 lint: add fixtures for⌁ invisible characters∙ Zoé⍽Durand 05/10/2026 \
                 (fix⌁∙)"
            );
            assert_eq!(style_in(&header, "⍽"), theme.author.patch(theme.invisible));
            assert_eq!(
                style_in(&header, "⌁"),
                theme.local_branch.patch(theme.invisible)
            );
            assert_eq!(
                style_in(&header, "∙"),
                theme.local_branch.patch(theme.trailing_whitespace)
            );
        });
    }

    #[test]
    fn replacements_are_bold_red_and_tabs_dim_gray() {
        let commit = FakeCommit::InvisibleCharacters.commit();
        themes().iter().for_each(|theme| {
            let rows = commit_rows(&commit, 110, theme);
            let Content::Text(body) = &rows[2].content else {
                panic!("the body is not split");
            };
            assert_eq!(style_in(body, "⍽"), theme.invisible);
            assert_eq!(style_in(body, "∙"), theme.trailing_whitespace);
            let makefile = file_rows(&commit.files[2], 110, theme);
            let Content::Split { right, .. } = &makefile[7].content else {
                panic!("chunk rows are split");
            };
            assert_eq!(text(right), "│ 5 │ ⇥   cc $(CFLAGS) -o app main.c");
            assert_eq!(style_in(right, "⇥   "), theme.dimmed_whitespace);
            let Content::Split { right, .. } = &makefile[4].content else {
                panic!("chunk rows are split");
            };
            // A trailing tab is trailing whitespace.
            assert_eq!(text(right), "│ 2 │ all: build⇥ ");
            assert_eq!(
                style_in(right, "⇥ "),
                theme.trailing_whitespace.patch(theme.added_line)
            );
        });
    }

    #[test]
    fn tab_stops_are_counted_from_the_beginning_of_the_code() {
        let commit = FakeCommit::InvisibleCharacters.commit();
        let makefile = file_rows(&commit.files[2], 110, &Theme::monochrome());
        // `CFLAGS` is 6 columns wide, the next tab stop is at column 8.
        assert_eq!(text_of_row(&makefile[3]).1, "│ 1 │ CFLAGS⇥ = -O2");
    }

    #[test]
    fn carriage_returns_are_hidden_when_line_endings_are_consistent() {
        let commit = FakeCommit::InvisibleCharacters.commit();
        let emoji = file_rows(&commit.files[1], 110, &Theme::monochrome());
        let (left, right) = row_starting_with(&emoji, "│ 7 │");
        assert_eq!(left, "│ 7 │ const JOINED: &str = \"ab\";");
        assert_eq!(right, "│ 7 │ const JOINED: &str = \"a⌁b⌁\";");
    }

    #[test]
    fn carriage_returns_are_shown_when_line_endings_are_inconsistent() {
        let commit = FakeCommit::InvisibleCharacters.commit();
        let script = file_rows(&commit.files[3], 110, &Theme::monochrome());
        let (left, right) = row_starting_with(&script, "│ 3 │");
        assert_eq!(left, "│ 3 │ exit /b 0␍");
        assert_eq!(right, "│ 3 │ exit /b 0");
    }

    #[test]
    fn carriage_returns_of_the_body_are_shown() {
        let commit = FakeCommit::InvisibleCharacters.commit();
        let rows = commit_rows(&commit, 110, &Theme::monochrome());
        assert_eq!(
            text_of_row(&rows[3]).0,
            "The batch script now always uses CRLF line endings.␍"
        );
    }

    #[test]
    fn invisible_characters_of_the_file_name_are_replaced() {
        let mut commit = FakeCommit::InvisibleCharacters.commit();
        commit.files[0].path = "a\u{a0}b".into();
        let rows = commit_rows(&commit, 110, &Theme::monochrome());
        assert_eq!(text_of_row(&rows[5]).0, "a⍽b");
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
    fn error_modal_is_displayed_over_the_commits() {
        let error_modal = unknown_keys_error_modal();
        insta::assert_snapshot!(render_with_error_modal(
            110,
            26,
            Viewport::default(),
            Some(&error_modal),
            &Theme::monochrome()
        ));
    }

    #[test]
    fn long_lines_of_the_error_modal_are_wrapped() {
        let error_modal = unknown_keys_error_modal();
        insta::assert_snapshot!(render_with_error_modal(
            40,
            12,
            Viewport::default(),
            Some(&error_modal),
            &Theme::monochrome()
        ));
    }

    /// The position of the top left corner of the error modal displayed on `backend`.
    fn error_modal_corner(backend: &TestBackend) -> (u16, u16) {
        let buffer = backend.buffer();
        let corner = buffer
            .content
            .iter()
            .position(|cell| cell.symbol() == "┏")
            .unwrap();
        buffer.pos_of(corner)
    }

    #[test]
    fn border_of_the_error_modal_has_the_error_style_and_its_title_in_reverse_video() {
        let error_modal = unknown_keys_error_modal();
        themes().iter().for_each(|theme| {
            let backend =
                render_with_error_modal(110, 26, Viewport::default(), Some(&error_modal), theme);
            let buffer = backend.buffer();
            let (x, y) = error_modal_corner(&backend);
            // The corner, and the left border below it.
            [(x, y), (x, y + 1)].into_iter().for_each(|position| {
                assert_eq!(buffer[position].fg, theme.error.fg.unwrap());
                assert_eq!(buffer[position].bg, Color::Reset);
                assert!(!buffer[position].modifier.contains(Modifier::REVERSED));
            });
            // The space before the title, then its first letter.
            assert_eq!(buffer[(x + 2, y)].symbol(), "U");
            [(x + 1, y), (x + 2, y)].into_iter().for_each(|position| {
                assert_eq!(buffer[position].fg, theme.error.fg.unwrap());
                assert!(
                    buffer[position]
                        .modifier
                        .contains(Modifier::REVERSED | Modifier::BOLD)
                );
            });
        });
    }

    #[test]
    fn title_of_the_error_modal_is_in_reverse_video_without_colors() {
        let error_modal = unknown_keys_error_modal();
        let backend = render_with_error_modal(
            110,
            26,
            Viewport::default(),
            Some(&error_modal),
            &Theme::monochrome(),
        );
        let buffer = backend.buffer();
        let (x, y) = error_modal_corner(&backend);
        assert!(!buffer[(x, y)].modifier.contains(Modifier::REVERSED));
        assert!(
            buffer[(x + 2, y)]
                .modifier
                .contains(Modifier::REVERSED | Modifier::BOLD)
        );
    }

    #[test]
    fn bottom_border_of_the_error_modal_tells_how_to_close_it() {
        let error_modal = unknown_keys_error_modal();
        themes().iter().for_each(|theme| {
            let backend =
                render_with_error_modal(110, 26, Viewport::default(), Some(&error_modal), theme);
            let buffer = backend.buffer();
            let (left, top) = error_modal_corner(&backend);
            let bottom = (top..26)
                .find(|&y| buffer[(left, y)].symbol() == "┗")
                .unwrap();
            let right = (left..110)
                .find(|&x| buffer[(x, bottom)].symbol() == "┛")
                .unwrap();
            let border: String = (left + 1..right)
                .map(|x| buffer[(x, bottom)].symbol())
                .collect();
            let (before, after) = border.split_once(" Press any key to close ").unwrap();
            // Centered.
            assert!(before.chars().count().abs_diff(after.chars().count()) <= 1);
            assert!(before.chars().chain(after.chars()).all(|c| c == '━'));
            let start = left + 1 + before.chars().count() as u16;
            (start..start + 24).for_each(|x| {
                assert_eq!(buffer[(x, bottom)].fg, theme.error.fg.unwrap());
                assert!(!buffer[(x, bottom)].modifier.contains(Modifier::REVERSED));
            });
        });
    }

    #[test]
    fn content_of_the_error_modal_has_a_blank_row_above_and_below() {
        let error_modal = unknown_keys_error_modal();
        let backend = render_with_error_modal(
            110,
            26,
            Viewport::default(),
            Some(&error_modal),
            &Theme::monochrome(),
        );
        let buffer = backend.buffer();
        let (left, top) = error_modal_corner(&backend);
        let right = (left..110)
            .find(|&x| buffer[(x, top)].symbol() == "┓")
            .unwrap();
        let bottom = (top..26)
            .find(|&y| buffer[(left, y)].symbol() == "┗")
            .unwrap();
        let row = |y| -> String { (left..=right).map(|x| buffer[(x, y)].symbol()).collect() };
        let blank = format!("┃{}┃", " ".repeat(usize::from(right - left - 1)));
        assert_eq!(row(top + 1), blank);
        assert!(row(top + 2).starts_with("┃ line 3: "));
        assert_eq!(row(bottom - 1), blank);
        // Just around the border, the commits are displayed.
        assert_ne!(buffer[(left - 1, top)].symbol(), " ");
    }

    #[test]
    fn inside_of_the_error_modal_has_the_default_background() {
        let error_modal = unknown_keys_error_modal();
        [Theme::dark(), Theme::light(), Theme::monochrome()]
            .iter()
            .for_each(|theme| {
                let backend = render_with_error_modal(
                    110,
                    26,
                    Viewport::default(),
                    Some(&error_modal),
                    theme,
                );
                let buffer = backend.buffer();
                let (x, y) = error_modal_corner(&backend);
                // The blank row, then the padding and the text below it.
                assert_eq!(buffer[(x + 2, y + 2)].symbol(), "l");
                [(x + 1, y + 1), (x + 1, y + 2), (x + 2, y + 2)]
                    .into_iter()
                    .for_each(|position| {
                        assert_eq!(buffer[position].bg, Color::Reset);
                        assert!(!buffer[position].modifier.contains(Modifier::REVERSED));
                    });
            });
    }
}
