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
            }
        }
    }
}

fn single_decoration_spans(decoration: &Decoration) -> Vec<Span<'_>> {
    match decoration {
        Decoration::Head(None) => vec![Span::styled("HEAD", HEAD)],
        Decoration::Head(Some(branch)) => vec![
            Span::styled("HEAD", HEAD),
            Span::styled(" -> ", DECORATION_PUNCTUATION),
            Span::styled(branch.as_str(), LOCAL_BRANCH),
        ],
        Decoration::LocalBranch(branch) => vec![Span::styled(branch.as_str(), LOCAL_BRANCH)],
        Decoration::RemoteBranch(branch) => vec![Span::styled(branch.as_str(), REMOTE_BRANCH)],
        Decoration::Tag(tag) => vec![Span::styled("tag: ", TAG), Span::styled(tag.as_str(), TAG)],
    }
}

fn decoration_spans(decorations: &[Decoration]) -> Vec<Span<'_>> {
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
fn header(commit: &Commit) -> Line<'_> {
    let mut spans = vec![
        Span::styled(commit.hash.as_str(), HASH),
        Span::raw(" "),
        Span::raw(commit.subject.as_str()),
        Span::raw(" "),
        Span::styled(commit.author.as_str(), AUTHOR),
        Span::raw(" "),
        Span::styled(commit.date.strftime(DATE_FORMAT).to_string(), DATE),
    ];
    if !commit.decorations.is_empty() {
        spans.extend(decoration_spans(&commit.decorations));
    }
    Line::from(spans)
}

/// `│ $line │ $code` for the `index`-th line of `side`.
fn half_line(side: &Side, index: usize, width: usize) -> Line<'_> {
    let number = side.line_number(index);
    Line::from(vec![
        Span::styled(format!("│ {number:>width$} │ "), LINE_NUMBER),
        Span::raw(side.lines[index].as_str()),
    ])
}

fn chunk_rows(chunk: &Chunk) -> impl Iterator<Item = Row<'_>> {
    let width = chunk.line_number_width();
    (0..chunk.before.lines.len()).map(move |index| Row::Split {
        left: half_line(&chunk.before, index, width),
        right: half_line(&chunk.after, index, width),
    })
}

fn file_rows(file: &FileDiff) -> Vec<Row<'_>> {
    let mut rows = vec![
        Row::Text(Line::styled(file.path.as_str(), FILE)),
        Row::Rule(FILE),
    ];
    rows.extend(file.chunks.iter().enumerate().flat_map(|(i, chunk)| {
        let separator = (i > 0).then(Row::blank);
        separator.into_iter().chain(chunk_rows(chunk))
    }));
    rows
}

/// All the rows displaying `commit`, including the separator below it.
pub fn commit_rows(commit: &Commit) -> Vec<Row<'_>> {
    let mut rows = vec![Row::Text(header(commit))];
    if !commit.body.is_empty() {
        rows.push(Row::blank());
        rows.extend(commit.body.lines().map(|line| Row::Text(Line::raw(line))));
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
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::fake;

    /// The most recent fake commit.
    fn newest() -> Commit {
        fake::commits().pop().unwrap()
    }

    fn render(width: u16, height: u16, viewport: Viewport) -> Terminal<TestBackend> {
        let commits = fake::commits();
        let rows = rows(&commits);
        let mut viewport = viewport;
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| draw(frame, &rows, &mut viewport, Mode::Review))
            .unwrap();
        terminal
    }

    fn text(line: &Line) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn full_screen() {
        insta::assert_snapshot!(render(110, 26, Viewport::default()).backend());
    }

    #[test]
    fn too_small_screen_is_cropped() {
        insta::assert_snapshot!(render(50, 8, Viewport::default()).backend());
    }

    #[test]
    fn scrolled_to_the_end() {
        let len = rows(&fake::commits()).len();
        let viewport = Viewport {
            cursor: len - 1,
            top: 0,
        };
        insta::assert_snapshot!(render(110, 26, viewport).backend());
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
        let terminal = render(110, 26, viewport);
        let buffer = terminal.backend().buffer();
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
        let terminal = render(110, 26, viewport);
        let buffer = terminal.backend().buffer();
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
        assert_eq!(rows[2], Row::Text(Line::styled("src/config.rs", FILE)));
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
        assert_eq!(buf, Buffer::with_lines(["LLLLLLLLLLRRRRRRRRRRR"]));
    }

    #[test]
    fn line_numbers_are_aligned_per_chunk() {
        let commit = newest();
        let rows: Vec<_> = chunk_rows(&commit.files[0].chunks[1]).collect();
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
        let terminal = render(110, 26, Viewport::default());
        let buffer = terminal.backend().buffer();
        let status_bar: String = (0..8).map(|x| buffer[(x, 25)].symbol()).collect();
        assert_eq!(status_bar, " Review ");
        assert_eq!(buffer[(1, 25)].bg, Mode::Review.accent());
    }
}
