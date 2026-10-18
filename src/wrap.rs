//! Display of the lines too wide for their text area on several rows.

use std::error::Error;
use std::fmt;
use std::ops::Range;

use icu_segmenter::LineSegmenter;
use icu_segmenter::options::LineBreakOptions;

use crate::invisible;
use crate::invisible::Cluster;
use crate::invisible::Content;
use crate::invisible::Contextual;
use crate::invisible::FinalCr;
use crate::invisible::Piece;
use crate::invisible::Pieces;
use crate::invisible::Wrap;

/// A part of a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment<'a> {
    pub text: &'a str,
    /// Whether the segment is markup: text structuring the display of the data (like the spaces
    /// between the fields of the header), not part of the data. It contains no invisible
    /// character other than spaces, which are never replaced, even next to a break or at the end
    /// of the line.
    pub markup: bool,
}

impl<'a> Segment<'a> {
    /// A segment that is not markup.
    pub fn new(text: &'a str) -> Self {
        Self {
            text,
            markup: false,
        }
    }
}

/// A row of a text area: the part of a line displayed on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextRow {
    /// Position of the first character displayed on the row, in bytes, in the concatenation of
    /// the segments of the line (without their hidden carriage returns).
    pub start: usize,
    /// Number of columns used by the pieces (at most the width of the text area).
    pub columns_count: usize,
    /// The displayed pieces, with the index of their segment.
    pub pieces: Vec<(usize, Piece)>,
}

/// A text area too narrow to display a grapheme cluster of its line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TooNarrow {
    /// Width of the text area.
    pub width: usize,
    pub cluster: String,
}

impl fmt::Display for TooNarrow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let unit = if self.width == 1 { "column" } else { "columns" };
        write!(
            f,
            "the terminal is too narrow to display {:?} in a text area of {} {unit}",
            self.cluster, self.width
        )
    }
}

impl Error for TooNarrow {}

/// A cluster, with the index of its segment.
#[derive(Debug, Clone)]
struct Item {
    segment: usize,
    /// See [`Segment::markup`].
    markup: bool,
    cluster: Cluster,
    /// Whether there is a break opportunity right after it.
    break_after: bool,
}

/// Place `item` at `column` if it fits in `width` columns, and move `column` after it.
///
/// A tab whose spaces up to the next tab stop don't fit is displayed alone, and ends the row.
fn fits(item: &Item, column: &mut usize, width: usize) -> bool {
    let item_width = item.cluster.width(*column);
    if *column + item_width <= width {
        *column += item_width;
        true
    } else if matches!(item.cluster.content, Content::Contextual(Contextual::Tab))
        && *column < width
    {
        *column = width;
        true
    } else {
        false
    }
}

/// The items displayed on each row, the line being broken as late as possible.
///
/// Fails with the index of an item that doesn't fit on an empty row.
fn layout(items: &[Item], width: usize) -> Result<Vec<Range<usize>>, usize> {
    let mut rows = Vec::new();
    let mut row_start = 0;
    let mut column = 0;
    let mut i = 0;
    while i < items.len() {
        let word_end = items[i..]
            .iter()
            .position(|item| item.break_after)
            .map_or(items.len(), |end| i + end + 1);
        // The spaces and tabs ending a word don't need to fit: they are moved to the next row.
        let body_end = word_end
            - items[i..word_end]
                .iter()
                .rev()
                .take_while(|item| {
                    matches!(
                        item.cluster.content,
                        Content::Contextual(Contextual::Space | Contextual::Tab)
                    )
                })
                .count();
        let body_width = items[i..body_end]
            .iter()
            .fold(column, |column, item| column + item.cluster.width(column))
            - column;
        let ends_row = if column + body_width <= width {
            column += body_width;
            i = body_end;
            while i < word_end && fits(&items[i], &mut column, width) {
                i += 1;
            }
            i < word_end
        } else if i > row_start {
            true
        } else {
            // A word wider than the text area is split after its last cluster that fits.
            while i < body_end && fits(&items[i], &mut column, width) {
                i += 1;
            }
            true
        };
        if ends_row {
            if i == row_start {
                return Err(i);
            }
            rows.push(row_start..i);
            row_start = i;
            column = 0;
        }
    }
    rows.push(row_start..items.len());
    Ok(rows)
}

/// Number of spaces and U+2800 in the whitespace at the start of `items`.
fn blanks_count<'a>(items: impl Iterator<Item = &'a Item>) -> usize {
    items
        .take_while(|item| item.cluster.is_contextual())
        .filter(|item| !matches!(item.cluster.content, Content::Contextual(Contextual::Tab)))
        .count()
}

/// Number of whitespace clusters at the start of `items`.
fn whitespace_count<'a>(items: impl Iterator<Item = &'a Item>) -> usize {
    items
        .take_while(|item| item.cluster.is_contextual())
        .count()
}

/// The whitespace of a row next to the breaks before and after it.
struct RowBreaks {
    /// Number of spaces and U+2800 around the break before the row, if it is a continuation row.
    blanks_before: Option<usize>,
    /// Number of spaces and U+2800 around the break after the row, if it is followed by a
    /// continuation row.
    blanks_after: Option<usize>,
    /// End of the whitespace starting the row, if it is a continuation row (`0` otherwise).
    leading_whitespace_end: usize,
    /// Start of the whitespace ending the row, if it is followed by a continuation row (the
    /// length of the row otherwise).
    ending_whitespace_start: usize,
}

impl RowBreaks {
    fn new(row: &[Item], blanks_before: Option<usize>, blanks_after: Option<usize>) -> Self {
        let leading_whitespace_end = if blanks_before.is_some() {
            whitespace_count(row.iter())
        } else {
            0
        };
        let ending_whitespace_start = if blanks_after.is_some() {
            row.len() - whitespace_count(row.iter().rev())
        } else {
            row.len()
        };
        Self {
            blanks_before,
            blanks_after,
            leading_whitespace_end,
            ending_whitespace_start,
        }
    }

    /// Where the `i`-th item of `row` is relative to the breaks.
    ///
    /// The whitespace ending the row takes precedence over the one starting it (when the row is
    /// only whitespace), except for a tab that is not the last item: only the tab that is the last
    /// character before the break is displayed alone.
    fn wrap(&self, row: &[Item], i: usize) -> Wrap {
        let item = &row[i];
        // The only space or U+2800 around a break.
        let single = |blanks: Option<usize>| {
            blanks == Some(1)
                && !matches!(item.cluster.content, Content::Contextual(Contextual::Tab))
        };
        let ends_row = i >= self.ending_whitespace_start;
        let starts_row = i < self.leading_whitespace_end;
        if item.markup {
            Wrap::None
        } else if ends_row && single(self.blanks_after) {
            Wrap::Single
        } else if ends_row
            && (!matches!(item.cluster.content, Content::Contextual(Contextual::Tab))
                || i + 1 == row.len())
        {
            Wrap::BeforeBreak
        } else if starts_row && single(self.blanks_before) {
            Wrap::Single
        } else if starts_row {
            Wrap::AfterBreak
        } else {
            Wrap::None
        }
    }
}

/// The row displaying `items` (the items placed on it by [`layout`]) in a text area of `width`
/// columns.
///
/// `blanks_before` is the number of spaces and U+2800 around the break before the row, if it is a
/// continuation row, and `blanks_after` around the break after it, if it is followed by a
/// continuation row.
fn text_row(
    items: &[Item],
    blanks_before: Option<usize>,
    blanks_after: Option<usize>,
    width: usize,
) -> TextRow {
    let breaks = RowBreaks::new(items, blanks_before, blanks_after);
    let groups =
        items
            .iter()
            .enumerate()
            .fold(Vec::<(usize, Pieces)>::new(), |mut groups, (i, item)| {
                let column = groups.last().map_or(0, |(_, pieces)| pieces.column);
                if groups
                    .last()
                    .is_none_or(|(segment, _)| *segment != item.segment)
                {
                    groups.push((item.segment, Pieces::new(column)));
                }
                let (_, pieces) = groups.last_mut().expect("a group was just pushed");
                item.cluster
                    .push(pieces, breaks.wrap(items, i), width.saturating_sub(column));
                groups
            });
    TextRow {
        start: items.first().map_or(0, |item| item.cluster.range.start),
        columns_count: groups.last().map_or(0, |(_, pieces)| pieces.column),
        pieces: groups
            .into_iter()
            .flat_map(|(segment, pieces)| {
                pieces.pieces.into_iter().map(move |piece| (segment, piece))
            })
            .collect(),
    }
}

/// Display the line made of `segments` on rows of `width` columns, with its invisible characters
/// replaced. A line has always at least one row.
///
/// The line is broken at the break opportunities of the Unicode line breaking algorithm (UAX
/// #14), computed on the original text. A carriage return ending a segment is replaced if
/// `final_cr` is [`FinalCr::Shown`], and hidden otherwise.
pub fn wrap(
    segments: &[Segment],
    final_cr: FinalCr,
    width: usize,
) -> Result<Vec<TextRow>, TooNarrow> {
    let texts: Vec<&str> = segments
        .iter()
        .map(|segment| match final_cr {
            FinalCr::Shown => segment.text,
            FinalCr::Hidden => segment.text.strip_suffix('\r').unwrap_or(segment.text),
        })
        .collect();
    let text = texts.concat();
    let opportunities: Vec<usize> =
        LineSegmenter::new_for_non_complex_scripts(LineBreakOptions::default())
            .segment_str(&text)
            .collect();
    let items: Vec<Item> = texts
        .iter()
        .enumerate()
        .scan(0, |offset, (segment, text)| {
            let start = *offset;
            *offset += text.len();
            // The hidden carriage returns are already removed.
            let clusters = invisible::clusters(text, FinalCr::Shown);
            Some(
                clusters
                    .into_iter()
                    .map(move |cluster| (segment, start, cluster)),
            )
        })
        .flatten()
        .map(|(segment, start, cluster)| {
            let range = start + cluster.range.start..start + cluster.range.end;
            Item {
                segment,
                markup: segments[segment].markup,
                break_after: opportunities.binary_search(&range.end).is_ok(),
                cluster: Cluster {
                    range,
                    trailing: cluster.trailing && !segments[segment].markup,
                    ..cluster
                },
            }
        })
        .collect();
    let rows = layout(&items, width).map_err(|i| TooNarrow {
        width,
        cluster: text[items[i].cluster.range.clone()].to_string(),
    })?;
    let rows: Vec<&[Item]> = rows.into_iter().map(|range| &items[range]).collect();
    // Number of spaces and U+2800 around each break: ending the row before it, and starting the
    // row after it.
    let around_breaks: Vec<usize> = rows
        .windows(2)
        .map(|rows| blanks_count(rows[0].iter().rev()) + blanks_count(rows[1].iter()))
        .collect();
    Ok(rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let before = i.checked_sub(1).map(|i| around_breaks[i]);
            text_row(row, before, around_breaks.get(i).copied(), width)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::invisible::Kind;

    /// The text of each row displaying `line`.
    fn rows(line: &str, width: usize) -> Vec<String> {
        wrap(&[Segment::new(line)], FinalCr::Shown, width)
            .unwrap()
            .into_iter()
            .map(|row| {
                row.pieces
                    .into_iter()
                    .map(|(_, piece)| piece.text)
                    .collect()
            })
            .collect()
    }

    /// The pieces of each row displaying `line`.
    fn pieces(line: &str, width: usize) -> Vec<Vec<Piece>> {
        wrap(&[Segment::new(line)], FinalCr::Shown, width)
            .unwrap()
            .into_iter()
            .map(|row| row.pieces.into_iter().map(|(_, piece)| piece).collect())
            .collect()
    }

    fn piece(text: &str, kind: Kind) -> Piece {
        Piece {
            text: text.to_string(),
            kind,
        }
    }

    #[test]
    fn short_lines_are_not_wrapped() {
        assert_eq!(rows("let a = b;", 10), ["let a = b;"]);
    }

    #[test]
    fn empty_lines_have_one_empty_row() {
        assert_eq!(rows("", 10), [""]);
        // Even if the text area has no column.
        assert_eq!(rows("", 0), [""]);
    }

    #[test]
    fn lines_are_broken_at_spaces_as_late_as_possible() {
        assert_eq!(rows("the quick brown fox", 10), ["the quick ", "brown fox"]);
    }

    #[test]
    fn lines_are_broken_after_a_hyphen() {
        assert_eq!(rows("a well-known word", 9), ["a well-", "known ", "word"]);
    }

    #[test]
    fn lines_are_not_broken_between_a_dot_and_a_letter() {
        assert_eq!(rows("see example.com", 12), ["see ", "example.com"]);
    }

    #[test]
    fn words_wider_than_the_text_area_are_split() {
        assert_eq!(rows("abcdefgh", 3), ["abc", "def", "gh"]);
    }

    #[test]
    fn words_are_split_between_grapheme_clusters() {
        assert_eq!(rows("abe\u{301}cd", 3), ["abe\u{301}", "cd"]);
    }

    #[test]
    fn wide_characters_that_do_not_fit_go_to_the_next_row() {
        // `日` and `本` are 2 columns wide, with a break opportunity between them.
        assert_eq!(rows("a日本", 4), ["a日", "本"]);
        // No break opportunity before `。`, so the word `日。` is split.
        assert_eq!(rows("日。", 3), ["日", "。"]);
    }

    #[test]
    fn single_space_at_a_break_is_not_displayed() {
        // Moved to the next row.
        assert_eq!(rows("abc d", 3), ["abc", " d"]);
        assert_eq!(
            pieces("ab cd", 3),
            [
                vec![piece("ab ", Kind::Text)],
                vec![piece("cd", Kind::Text)]
            ]
        );
        // Unless they are trailing spaces.
        assert_eq!(
            pieces("ab  ", 3)[0],
            [
                piece("ab", Kind::Text),
                piece("∙", Kind::TrailingWhitespace)
            ]
        );
    }

    #[test]
    fn spaces_before_a_break_are_dimmed_if_there_are_several() {
        assert_eq!(
            pieces("ab  cd", 4),
            [
                vec![piece("ab", Kind::Text), piece("∙∙", Kind::DimmedWhitespace)],
                vec![piece("cd", Kind::Text)],
            ]
        );
    }

    #[test]
    fn spaces_that_do_not_fit_are_moved_to_the_next_row() {
        assert_eq!(rows("abc   def", 5), ["abc∙∙", " def"]);
        // The spaces moved to the next row count as being around the break.
        assert_eq!(rows("abc  def", 4), ["abc∙", " def"]);
    }

    #[test]
    fn trailing_whitespace_is_wrapped_and_highlighted() {
        assert_eq!(
            pieces("ab    ", 3),
            [
                vec![
                    piece("ab", Kind::Text),
                    piece("∙", Kind::TrailingWhitespace)
                ],
                vec![piece("∙∙∙", Kind::TrailingWhitespace)],
            ]
        );
        assert_eq!(
            pieces("ab\t\t", 4),
            [
                vec![
                    piece("ab", Kind::Text),
                    piece("⇥", Kind::TrailingWhitespace)
                ],
                vec![piece("⇥   ", Kind::TrailingWhitespace)],
            ]
        );
    }

    #[test]
    fn tab_before_a_break_is_displayed_alone() {
        // The tab fits, but `cdef` doesn't.
        assert_eq!(rows("ab\tcdef", 6), ["ab⇥", "cdef"]);
        // Only `⇥` fits.
        assert_eq!(rows("abcde\tx", 6), ["abcde⇥", "x"]);
        assert_eq!(
            pieces("ab\tcdef", 6)[0],
            [piece("ab", Kind::Text), piece("⇥", Kind::DimmedWhitespace)]
        );
    }

    #[test]
    fn tab_after_a_break_reaches_the_next_tab_stop() {
        assert_eq!(rows("abcdef\tx", 6), ["abcdef", "⇥   x"]);
        // Tab stops are counted from the start of the text area.
        assert_eq!(rows("abcdef g\tx", 6), ["abcdef", " g⇥ x"]);
    }

    #[test]
    fn braille_blanks_at_a_break_are_dimmed_if_there_are_several() {
        assert_eq!(
            pieces("a\u{2800}\u{2800}cd", 3)[0],
            [piece("a", Kind::Text), piece("∙∙", Kind::DimmedWhitespace)]
        );
        assert_eq!(
            pieces("abc\u{2800}\u{2800}d", 3)[1],
            [piece("∙∙", Kind::DimmedWhitespace), piece("d", Kind::Text)]
        );
    }

    #[test]
    fn single_braille_blank_at_a_break_is_not_displayed() {
        assert_eq!(
            pieces("ab\u{2800}cd", 3),
            [
                vec![piece("ab ", Kind::Text)],
                vec![piece("cd", Kind::Text)]
            ]
        );
        // Its column is left blank.
        assert_eq!(pieces("abc\u{2800}d", 3)[1], [piece(" d", Kind::Text)]);
        assert_eq!(rows("a\u{2800}b", 10), ["a\u{2800}b"]);
    }

    #[test]
    fn break_opportunities_are_computed_on_the_original_text() {
        // There is a break opportunity after a zero-width space, but not after `⌁`.
        assert_eq!(rows("ab\u{200b}cd", 4), ["ab⌁", "cd"]);
    }

    #[test]
    fn segments_are_wrapped_as_a_single_line() {
        let segments = ["abc ", "de", "f gh"].map(Segment::new);
        let rows = wrap(&segments, FinalCr::Shown, 8).unwrap();
        assert_eq!(
            rows,
            [
                TextRow {
                    start: 0,
                    columns_count: 8,
                    pieces: vec![
                        (0, piece("abc", Kind::Text)),
                        (0, piece("∙", Kind::TrailingWhitespace)),
                        (1, piece("de", Kind::Text)),
                        (2, piece("f ", Kind::Text)),
                    ],
                },
                TextRow {
                    start: 8,
                    columns_count: 2,
                    pieces: vec![(2, piece("gh", Kind::Text))],
                },
            ]
        );
    }

    #[test]
    fn tab_stops_are_counted_from_the_start_of_the_row_in_all_segments() {
        let rows = wrap(&["ab", "\tc"].map(Segment::new), FinalCr::Shown, 20).unwrap();
        assert_eq!(rows[0].pieces[1], (1, piece("⇥ ", Kind::DimmedWhitespace)));
    }

    /// A segment that is markup.
    fn markup(text: &str) -> Segment<'_> {
        Segment { text, markup: true }
    }

    #[test]
    fn spaces_of_markup_are_never_replaced() {
        // At the end of a segment, of the line, and before and after a break.
        let segments = [
            Segment::new("a "),
            markup(" ("),
            markup("b  "),
            markup("  c "),
        ];
        let rows = wrap(&segments, FinalCr::Shown, 7).unwrap();
        assert_eq!(
            rows,
            [
                TextRow {
                    start: 0,
                    columns_count: 7,
                    pieces: vec![
                        (0, piece("a", Kind::Text)),
                        (0, piece("∙", Kind::TrailingWhitespace)),
                        (1, piece(" (", Kind::Text)),
                        (2, piece("b  ", Kind::Text)),
                    ],
                },
                TextRow {
                    start: 7,
                    columns_count: 4,
                    pieces: vec![(3, piece("  c ", Kind::Text))],
                },
            ]
        );
    }

    #[test]
    fn rows_start_at_their_first_character() {
        let starts: Vec<_> = wrap(&[Segment::new("the quick brown")], FinalCr::Shown, 10)
            .unwrap()
            .into_iter()
            .map(|row| row.start)
            .collect();
        assert_eq!(starts, [0, 10]);
    }

    #[test]
    fn width_of_a_row_is_counted_per_grapheme_cluster() {
        // As a whole, `unicode-width` counts the lam-alef ligature `لا` as 1 column.
        let rows = wrap(&[Segment::new("سلام")], FinalCr::Shown, 10).unwrap();
        assert_eq!(rows[0].columns_count, 4);
    }

    #[test]
    fn hidden_carriage_return_is_not_displayed() {
        let rows = wrap(&[Segment::new("ab\r")], FinalCr::Hidden, 10).unwrap();
        assert_eq!(rows[0].pieces, [(0, piece("ab", Kind::Text))]);
        let rows = wrap(&[Segment::new("ab\r")], FinalCr::Shown, 10).unwrap();
        assert_eq!(rows[0].pieces[1], (0, piece("␍", Kind::Replacement)));
    }

    #[test]
    fn too_narrow_text_area_is_an_error() {
        let error = wrap(&[Segment::new("a日")], FinalCr::Shown, 1).unwrap_err();
        assert_eq!(
            error,
            TooNarrow {
                width: 1,
                cluster: "日".to_string()
            }
        );
        assert_eq!(
            error.to_string(),
            "the terminal is too narrow to display \"日\" in a text area of 1 column"
        );
        assert!(wrap(&[Segment::new("a")], FinalCr::Shown, 0).is_err());
        assert!(wrap(&[Segment::new(" ")], FinalCr::Shown, 0).is_err());
    }
}
