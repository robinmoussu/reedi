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

use crate::invisible;
use crate::invisible::FinalCr;
use crate::invisible::Kind;
use crate::model::Chunk;
use crate::model::Commit;
use crate::model::Decoration;
use crate::model::FileDiff;
use crate::model::Side;
use crate::viewport::Viewport;

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

/// Replacement of a tab that is not part of the trailing whitespace.
const TAB: Style = Style::new().fg(Color::DarkGray);
/// Replacements of the trailing whitespace, including the tabs.
const TRAILING_WHITESPACE: Style = Style::new().fg(Color::Red).bold();
/// Replacements of the other invisible characters.
const INVISIBLE: Style = Style::new().fg(Color::Red).bold();

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

/// A single screen line.
#[derive(Debug, Clone, PartialEq)]
pub enum Row<'a> {
    Text(Line<'a>),
    /// A horizontal line spanning the whole width.
    Rule(Style),
    /// A line split 50/50 (the right half is bigger if the width is odd).
    Split {
        left: Line<'a>,
        right: Line<'a>,
    },
}

impl Row<'_> {
    fn blank() -> Self {
        Row::Text(Line::default())
    }

    /// Render the row on the first line of `area`, cropping it if too wide.
    fn render(&self, area: Rect, buf: &mut Buffer) {
        match self {
            Row::Text(line) => {
                buf.set_line(area.x, area.y, line, area.width);
                if area.width > 0 {
                    mark_left_to_right(buf, area.x, area.y);
                }
            }
            Row::Rule(style) => {
                let rule = "─".repeat(area.width.into());
                buf.set_string(area.x, area.y, rule, *style);
            }
            Row::Split { left, right } => {
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

/// Spans displaying `text` with `style`, with its invisible characters replaced. The style of
/// each replacement is `style`, patched with the style of its kind.
///
/// A carriage return ending `text` is replaced if `final_cr` is [`FinalCr::Shown`], and hidden
/// otherwise.
fn reveal(text: &str, style: Style, final_cr: FinalCr) -> Vec<Span<'_>> {
    invisible::display(text, final_cr)
        .into_iter()
        .map(|piece| {
            let kind = match piece.kind {
                Kind::Text => Style::new(),
                Kind::Tab => TAB,
                Kind::TrailingWhitespace => TRAILING_WHITESPACE,
                Kind::Replacement => INVISIBLE,
            };
            Span::styled(piece.text, style.patch(kind))
        })
        .collect()
}

/// A line displaying `text` with `style`, with its invisible characters replaced.
fn revealed_line(text: &str, style: Style) -> Line<'_> {
    Line::from(reveal(text, style, FinalCr::Shown))
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

fn single_decoration_spans(decoration: &Decoration) -> Vec<Span<'_>> {
    match decoration {
        Decoration::Head(None) => vec![Span::styled("HEAD", HEAD)],
        Decoration::Head(Some(branch)) => [
            Span::styled("HEAD", HEAD),
            Span::styled(" -> ", DECORATION_PUNCTUATION),
        ]
        .into_iter()
        .chain(reveal(branch, LOCAL_BRANCH, FinalCr::Shown))
        .collect(),
        Decoration::LocalBranch(branch) => reveal(branch, LOCAL_BRANCH, FinalCr::Shown),
        Decoration::RemoteBranch(branch) => reveal(branch, REMOTE_BRANCH, FinalCr::Shown),
        Decoration::Tag(tag) => iter::once(Span::styled("tag: ", TAG))
            .chain(reveal(tag, TAG, FinalCr::Shown))
            .collect(),
    }
}

/// ` ($decorations)`, or nothing if there are no decorations.
fn decoration_spans(decorations: &[Decoration]) -> Vec<Span<'_>> {
    if decorations.is_empty() {
        return Vec::new();
    }
    let decorations = decorations.iter().enumerate().flat_map(|(i, decoration)| {
        let separator = (i > 0).then(|| Span::styled(", ", DECORATION_PUNCTUATION));
        separator
            .into_iter()
            .chain(single_decoration_spans(decoration))
    });
    iter::once(Span::styled(" (", DECORATION_PUNCTUATION))
        .chain(decorations)
        .chain(iter::once(Span::styled(")", DECORATION_PUNCTUATION)))
        .collect()
}

/// `$hash $commit_subject $author $date ($tags, $branches)`
///
/// The invisible characters of each field (except the date, which has none) are replaced, the
/// trailing whitespace being the one ending the field.
fn header(commit: &Commit) -> Line<'_> {
    let date = commit.date.strftime(DATE_FORMAT).to_string();
    let spans: Vec<_> = reveal(&commit.hash, HASH, FinalCr::Shown)
        .into_iter()
        .chain(iter::once(Span::raw(" ")))
        .chain(reveal(&commit.subject, Style::new(), FinalCr::Shown))
        .chain(iter::once(Span::raw(" ")))
        .chain(reveal(&commit.author, AUTHOR, FinalCr::Shown))
        .chain(iter::once(Span::raw(" ")))
        .chain(iter::once(Span::styled(date, DATE)))
        .chain(decoration_spans(&commit.decorations))
        .collect();
    Line::from(spans)
}

/// `│ $line │ $code` for the `index`-th line of `side`.
///
/// A carriage return ending `$code` is replaced if `final_cr` is [`FinalCr::Shown`], and hidden
/// otherwise.
fn half_line(side: &Side, index: usize, width: usize, final_cr: FinalCr) -> Line<'_> {
    let number = side.line_number(index);
    let number = Span::styled(format!("│ {number:>width$} │ "), LINE_NUMBER);
    let code = reveal(&side.lines[index], Style::new(), final_cr);
    Line::from(iter::once(number).chain(code).collect::<Vec<_>>())
}

fn chunk_rows(chunk: &Chunk, final_cr: FinalCr) -> impl Iterator<Item = Row<'_>> {
    let width = chunk.line_number_width();
    (0..chunk.before.lines.len()).map(move |index| Row::Split {
        left: half_line(&chunk.before, index, width, final_cr),
        right: half_line(&chunk.after, index, width, final_cr),
    })
}

fn file_rows(file: &FileDiff) -> Vec<Row<'_>> {
    let final_cr = if file.has_inconsistent_line_endings() {
        FinalCr::Shown
    } else {
        FinalCr::Hidden
    };
    let mut rows = vec![Row::Text(revealed_line(&file.path, FILE)), Row::Rule(FILE)];
    rows.extend(file.chunks.iter().enumerate().flat_map(|(i, chunk)| {
        let separator = (i > 0).then(Row::blank);
        separator.into_iter().chain(chunk_rows(chunk, final_cr))
    }));
    rows
}

/// All the rows displaying `commit`, including the separator below it.
pub fn commit_rows(commit: &Commit) -> Vec<Row<'_>> {
    let mut rows = vec![Row::Text(header(commit))];
    if !commit.body.is_empty() {
        rows.push(Row::blank());
        // Unlike `str::lines`, a carriage return ending a line is kept, to be displayed.
        rows.extend(
            commit
                .body
                .split_terminator('\n')
                .map(|line| Row::Text(revealed_line(line, Style::new()))),
        );
    }
    rows.extend(
        commit
            .files
            .iter()
            .flat_map(|file| iter::once(Row::blank()).chain(file_rows(file))),
    );
    rows.push(Row::blank());
    rows.push(Row::Rule(COMMIT_SEPARATOR));
    rows
}

/// All the rows displaying `commits`.
pub fn rows(commits: &[Commit]) -> Vec<Row<'_>> {
    commits.iter().flat_map(commit_rows).collect()
}

/// Rows displayed from `viewport.top`, cropped if they don't fit, with the row of the cursor
/// highlighted.
pub struct RowsView<'a> {
    pub rows: &'a [Row<'a>],
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

    /// The fake commit with all kinds of invisible characters.
    fn invisible() -> Commit {
        fake::commits().remove(0)
    }

    fn text_of_row(row: &Row) -> (String, String) {
        match row {
            Row::Text(line) => (text(line), String::new()),
            Row::Split { left, right } => (text(left), text(right)),
            Row::Rule(_) => (String::new(), String::new()),
        }
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
        let rows = rows(&commits);
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

    fn text(line: &Line) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn full_screen() {
        insta::assert_snapshot!(render(110, 26, Viewport::default()));
    }

    #[test]
    fn too_small_screen_is_cropped() {
        insta::assert_snapshot!(render(50, 8, Viewport::default()));
    }

    #[test]
    fn scrolled_to_the_end() {
        let len = rows(&fake::commits()).len();
        let viewport = Viewport {
            cursor: len - 1,
            top: 0,
        };
        insta::assert_snapshot!(render(110, 26, viewport));
    }

    #[test]
    fn invisible_characters_after_the_first_screen() {
        let viewport = Viewport {
            cursor: 30,
            top: 20,
        };
        insta::assert_snapshot!(render(110, 26, viewport));
    }

    #[test]
    fn rows_of_all_commits_are_displayed_one_after_the_other() {
        let commits = fake::commits();
        let all_rows = rows(&commits);
        let expected: Vec<_> = commits.iter().flat_map(commit_rows).collect();
        assert_eq!(all_rows, expected);
        assert_eq!(all_rows[0], Row::Text(header(&commits[0])));
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
        let style_of = |content: &str| {
            header
                .spans
                .iter()
                .find(|span| span.content == content)
                .unwrap()
                .style
        };
        assert_eq!(style_of("3f9a2c1"), HASH);
        assert_eq!(style_of("Alice Martin"), AUTHOR);
        assert_eq!(style_of("12/10/2026"), DATE);
        assert_eq!(style_of("HEAD"), HEAD);
        assert_eq!(style_of(" -> "), DECORATION_PUNCTUATION);
        assert_eq!(style_of("main"), LOCAL_BRANCH);
        assert_eq!(style_of("v1.0"), TAG);
        assert_eq!(style_of("origin/main"), REMOTE_BRANCH);
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
        let line = Line::from(decoration_spans(&decorations));
        assert_eq!(text(&line), " (feature, HEAD)");
    }

    #[test]
    fn body_is_omitted_when_empty() {
        let mut commit = newest();
        commit.body.clear();
        let rows = commit_rows(&commit);
        assert_eq!(rows[1], Row::blank());
        assert_eq!(rows[2], Row::Text(revealed_line("src/config.rs", FILE)));
    }

    #[test]
    fn split_right_half_is_bigger_when_width_is_odd() {
        let row = Row::Split {
            left: Line::raw("LLLLLLLLLLLLLLL"),
            right: Line::raw("RRRRRRRRRRRRRRR"),
        };
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
        let symbols = |row: Row| {
            let mut buf = Buffer::empty(area);
            row.render(area, &mut buf);
            buf.content
                .iter()
                .map(|cell| cell.symbol().to_string())
                .collect::<Vec<_>>()
        };
        let split = symbols(Row::Split {
            left: Line::raw("L"),
            right: Line::raw("R"),
        });
        assert_eq!(split[0], "L\u{200e}");
        assert_eq!(split[9], " \u{200e}");
        assert_eq!(split[1], " ");
        assert_eq!(split[10], "R");
        let text = symbols(Row::Text(Line::raw("T")));
        assert_eq!(text[0], "T\u{200e}");
        assert_eq!(text[9], " ");
    }

    #[test]
    fn left_to_right_mark_is_after_a_wide_character_ending_a_column() {
        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        // `日` is 2 columns wide, on the last 2 columns of the left half.
        let row = Row::Split {
            left: Line::raw("LLLLLLLL日"),
            right: Line::raw("R"),
        };
        row.render(area, &mut buf);
        assert_eq!(buf[(8, 0)].symbol(), "日\u{200e}");
        assert_eq!(buf[(9, 0)].symbol(), " ");
    }

    #[test]
    fn line_numbers_are_aligned_per_chunk() {
        let commit = newest();
        let rows: Vec<_> = chunk_rows(&commit.files[0].chunks[1], FinalCr::Hidden).collect();
        let Row::Split { left, right } = &rows[0] else {
            panic!("chunk rows are split");
        };
        assert_eq!(text(left), "│  97 │ impl Default for Config {");
        assert_eq!(text(right), "│  97 │ impl Default for Config {");
        assert_eq!(left.spans[0].style, LINE_NUMBER);
    }

    #[test]
    fn rules_and_file_name_are_colored() {
        let commit = newest();
        let rows = commit_rows(&commit);
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
        let rows = commit_rows(&commit);
        let Row::Text(body) = &rows[2] else {
            panic!("the body is not split");
        };
        assert_eq!(style_in(body, "⍽"), INVISIBLE);
        assert_eq!(style_in(body, "∙"), TRAILING_WHITESPACE);
        let makefile = file_rows(&commit.files[2]);
        let Row::Split { right, .. } = &makefile[6] else {
            panic!("chunk rows are split");
        };
        assert_eq!(text(right), "│ 5 │ ⇥   cc $(CFLAGS) -o app main.c");
        assert_eq!(style_in(right, "⇥   "), TAB);
        let Row::Split { right, .. } = &makefile[3] else {
            panic!("chunk rows are split");
        };
        // A trailing tab is trailing whitespace.
        assert_eq!(text(right), "│ 2 │ all: build⇥ ");
        assert_eq!(style_in(right, "⇥ "), TRAILING_WHITESPACE);
    }

    #[test]
    fn tab_stops_are_counted_from_the_beginning_of_the_code() {
        let commit = invisible();
        let makefile = file_rows(&commit.files[2]);
        // `CFLAGS` is 6 columns wide, the next tab stop is at column 8.
        assert_eq!(text_of_row(&makefile[2]).1, "│ 1 │ CFLAGS⇥ = -O2");
    }

    #[test]
    fn carriage_returns_are_hidden_when_line_endings_are_consistent() {
        let commit = invisible();
        let emoji = file_rows(&commit.files[1]);
        let (left, right) = text_of_row(&emoji[6]);
        assert_eq!(left, "│ 7 │ const JOINED: &str = \"ab\";");
        assert_eq!(right, "│ 7 │ const JOINED: &str = \"a⌁b⌁\";");
    }

    #[test]
    fn carriage_returns_are_shown_when_line_endings_are_inconsistent() {
        let commit = invisible();
        let script = file_rows(&commit.files[3]);
        let (left, right) = text_of_row(&script[4]);
        assert_eq!(left, "│ 3 │ exit /b 0␍");
        assert_eq!(right, "│ 3 │ exit /b 0");
    }

    #[test]
    fn carriage_returns_of_the_body_are_shown() {
        let commit = invisible();
        let rows = commit_rows(&commit);
        assert_eq!(
            text_of_row(&rows[3]).0,
            "The batch script now always uses CRLF line endings.␍"
        );
    }

    #[test]
    fn invisible_characters_of_the_file_name_are_replaced() {
        let mut commit = invisible();
        commit.files[0].path = "a\u{a0}b".to_string();
        let rows = commit_rows(&commit);
        assert_eq!(text_of_row(&rows[5]).0, "a⍽b");
    }
}
