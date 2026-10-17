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

/// Rows displayed from the top of the area, cropped if they don't fit.
pub struct RowsView<'a> {
    pub rows: &'a [Row<'a>],
}

impl Widget for RowsView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.rows
            .iter()
            .zip(area.top()..area.bottom())
            .for_each(|(row, y)| {
                row.render(
                    Rect {
                        y,
                        height: 1,
                        ..area
                    },
                    buf,
                )
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

/// Draw `commit`, with the status bar on the last line of the screen.
pub fn draw(frame: &mut Frame, commit: &Commit, mode: Mode) {
    let [content, status_bar] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());
    let rows = commit_rows(commit);
    frame.render_widget(RowsView { rows: &rows }, content);
    frame.render_widget(StatusBar { mode }, status_bar);
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::fake;

    fn render(width: u16, height: u16) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| draw(frame, &fake::commit(), Mode::Review))
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
        insta::assert_snapshot!(render(110, 26).backend());
    }

    #[test]
    fn too_small_screen_is_cropped() {
        insta::assert_snapshot!(render(50, 8).backend());
    }

    #[test]
    fn header_is_colored_like_git() {
        let commit = fake::commit();
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
        let mut commit = fake::commit();
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
        let mut commit = fake::commit();
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
        let commit = fake::commit();
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
        let terminal = render(110, 26);
        let buffer = terminal.backend().buffer();
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
        let terminal = render(110, 26);
        let buffer = terminal.backend().buffer();
        let status_bar: String = (0..8).map(|x| buffer[(x, 25)].symbol()).collect();
        assert_eq!(status_bar, " Review ");
        assert_eq!(buffer[(1, 25)].bg, Mode::Review.accent());
    }
}
