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
        Self { text, markup: false }
    }
}

/// A row of a text area: the part of a line displayed on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextRow {
    /// Position of the first character displayed on the row, in bytes, in the concatenation of
    /// the segments of the line (without their hidden carriage returns).
    pub start_byte: usize,
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
    segment_index: usize,
    /// See [`Segment::markup`].
    markup: bool,
    cluster: Cluster,
    /// Whether there is a break opportunity right after it.
    break_after: bool,
}

/// Move `column` past `item` if it fits in `width` columns.
///
/// A tab whose spaces up to the next tab stop don't fit is displayed alone, and ends the row.
fn advance_if_fits(item: &Item, column: &mut usize, width: usize) -> bool {
    let item_width = item.cluster.width(*column);
    if *column + item_width <= width {
        *column += item_width;
        true
    } else if item.cluster.is_tab() && *column < width {
        *column = width;
        true
    } else {
        false
    }
}

/// Move `column` past the items at the start of `items` while they fit, as [`advance_if_fits`]
/// does, and return the number of items it moved past.
fn advance_while_fits(items: &[Item], column: &mut usize, width: usize) -> usize {
    items
        .iter()
        .take_while(|item| advance_if_fits(item, column, width))
        .count()
}

/// The number of items of `items` that fit on its first row of `width` columns.
///
/// The row ends after the last word that fits, the whitespace ending a word not needing to fit:
/// the whitespace that doesn't fit starts the next row. A word wider than `width` columns is
/// split after its last cluster that fits.
///
/// Returns `None` if the first item doesn't fit on the row.
fn items_count_fitting_in_first_row(items: &[Item], width: usize) -> Option<usize> {
    let mut column = 0;
    let mut word_start = 0;
    for word in items.split_inclusive(|item| item.break_after) {
        // The whitespace ending the word doesn't need to fit: what doesn't fit starts the next row.
        let body_len = word.len()
            - word
                .iter()
                .rev()
                .take_while(|item| {
                    matches!(
                        item.cluster.content,
                        Content::Contextual(Contextual::Space | Contextual::Tab)
                    )
                })
                .count();
        let (body, trailing_whitespace) = word.split_at(body_len);
        let body_width = body
            .iter()
            .fold(column, |column, item| column + item.cluster.width(column))
            - column;
        let len = if column + body_width <= width {
            column += body_width;
            let fitting_count = advance_while_fits(trailing_whitespace, &mut column, width);
            if fitting_count == trailing_whitespace.len() {
                word_start += word.len();
                continue;
            }
            word_start + body_len + fitting_count
        } else if word_start > 0 {
            word_start
        } else {
            // A word wider than the text area is split after its last cluster that fits.
            advance_while_fits(body, &mut column, width)
        };
        return if len > 0 { Some(len) } else { None };
    }
    Some(items.len())
}

/// Split `items` into the rows of a text area of `width` columns, each row holding as many
/// words as fit, and return the range of item indexes of each row.
///
/// If an item doesn't fit on an empty row, fails with its index.
fn split_into_rows(items: &[Item], width: usize) -> Result<Vec<Range<usize>>, usize> {
    let mut rows = Vec::new();
    let mut row_start = 0;
    loop {
        let len = items_count_fitting_in_first_row(&items[row_start..], width).ok_or(row_start)?;
        rows.push(row_start..row_start + len);
        row_start += len;
        if row_start == items.len() {
            return Ok(rows);
        }
    }
}

/// Number of spaces and U+2800 in the whitespace at the start of `items`.
fn blanks_count<'a>(items: impl Iterator<Item = &'a Item>) -> usize {
    items
        .take_while(|item| item.cluster.is_contextual())
        .filter(|item| !item.cluster.is_tab())
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
        let single = |blanks: Option<usize>| blanks == Some(1) && !item.cluster.is_tab();
        let ends_row = i >= self.ending_whitespace_start;
        let starts_row = i < self.leading_whitespace_end;
        if item.markup {
            Wrap::None
        } else if ends_row && single(self.blanks_after) {
            Wrap::Single
        } else if ends_row && (!item.cluster.is_tab() || i + 1 == row.len()) {
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

/// The row displaying `items` (its items, split by [`split_into_rows()`]) in a text area of `width`
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
                    .is_none_or(|(segment_index, _)| *segment_index != item.segment_index)
                {
                    groups.push((item.segment_index, Pieces::new(column)));
                }
                let (_, pieces) = groups.last_mut().expect("a group was just pushed");
                item.cluster
                    .push(pieces, breaks.wrap(items, i), width.saturating_sub(column));
                groups
            });
    TextRow {
        start_byte: items.first().map_or(0, |item| item.cluster.range.start),
        columns_count: groups.last().map_or(0, |(_, pieces)| pieces.column),
        pieces: groups
            .into_iter()
            .flat_map(|(segment_index, pieces)| {
                pieces
                    .pieces
                    .into_iter()
                    .map(move |piece| (segment_index, piece))
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
        .scan(0, |offset, (segment_index, text)| {
            let start = *offset;
            *offset += text.len();
            // The hidden carriage returns are already removed.
            let clusters = invisible::clusters(text, FinalCr::Shown);
            Some(
                clusters
                    .into_iter()
                    .map(move |cluster| (segment_index, start, cluster)),
            )
        })
        .flatten()
        .map(|(segment_index, start, cluster)| {
            let range = start + cluster.range.start..start + cluster.range.end;
            Item {
                segment_index,
                markup: segments[segment_index].markup,
                break_after: opportunities.binary_search(&range.end).is_ok(),
                cluster: Cluster {
                    range,
                    trailing: cluster.trailing && !segments[segment_index].markup,
                    ..cluster
                },
            }
        })
        .collect();
    let rows = split_into_rows(&items, width).map_err(|i| TooNarrow {
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
        Piece { text: text.to_string(), kind }
    }

    #[test]
    fn short_lines_are_not_wrapped() {
        assert_eq!(rows("let a = b;", /* width */ 10), ["let a = b;"]);
    }

    #[test]
    fn empty_lines_have_one_empty_row() {
        assert_eq!(rows("", /* width */ 10), [""]);
        // Even if the text area has no column.
        assert_eq!(rows("", /* width */ 0), [""]);
    }

    #[test]
    fn lines_are_broken_at_spaces_as_late_as_possible() {
        assert_eq!(
            rows("the quick brown fox", /* width */ 10),
            ["the quick ", "brown fox"]
        );
    }

    #[test]
    fn lines_are_broken_after_a_hyphen() {
        assert_eq!(
            rows("a well-known word", /* width */ 9),
            ["a well-", "known ", "word"]
        );
    }

    #[test]
    fn lines_are_not_broken_between_a_dot_and_a_letter() {
        assert_eq!(
            rows("see example.com", /* width */ 12),
            ["see ", "example.com"]
        );
    }

    #[test]
    fn words_wider_than_the_text_area_are_split() {
        assert_eq!(rows("abcdefgh", /* width */ 3), ["abc", "def", "gh"]);
    }

    #[test]
    fn words_are_split_between_grapheme_clusters() {
        assert_eq!(rows("abe\u{301}cd", /* width */ 3), ["abe\u{301}", "cd"]);
    }

    #[test]
    fn wide_characters_that_do_not_fit_go_to_the_next_row() {
        // `日` and `本` are 2 columns wide, with a break opportunity between them.
        assert_eq!(rows("a日本", /* width */ 4), ["a日", "本"]);
        // No break opportunity before `。`, so the word `日。` is split.
        assert_eq!(rows("日。", /* width */ 3), ["日", "。"]);
    }

    #[test]
    fn single_space_at_a_break_is_not_displayed() {
        // Moved to the next row.
        assert_eq!(rows("abc d", /* width */ 3), ["abc", " d"]);
        assert_eq!(
            pieces("ab cd", /* width */ 3),
            [
                vec![piece("ab ", Kind::Text)],
                vec![piece("cd", Kind::Text)]
            ]
        );
        // Unless they are trailing spaces.
        assert_eq!(
            pieces("ab  ", /* width */ 3)[0],
            [
                piece("ab", Kind::Text),
                piece("∙", Kind::TrailingWhitespace)
            ]
        );
    }

    #[test]
    fn spaces_before_a_break_are_dimmed_if_there_are_several() {
        assert_eq!(
            pieces("ab  cd", /* width */ 4),
            [
                vec![piece("ab", Kind::Text), piece("∙∙", Kind::DimmedWhitespace)],
                vec![piece("cd", Kind::Text)],
            ]
        );
    }

    #[test]
    fn spaces_that_do_not_fit_are_moved_to_the_next_row() {
        assert_eq!(rows("abc   def", /* width */ 5), ["abc∙∙", " def"]);
        // The spaces moved to the next row count as being around the break.
        assert_eq!(rows("abc  def", /* width */ 4), ["abc∙", " def"]);
    }

    #[test]
    fn trailing_whitespace_is_wrapped_and_highlighted() {
        assert_eq!(
            pieces("ab    ", /* width */ 3),
            [
                vec![
                    piece("ab", Kind::Text),
                    piece("∙", Kind::TrailingWhitespace)
                ],
                vec![piece("∙∙∙", Kind::TrailingWhitespace)],
            ]
        );
        assert_eq!(
            pieces("ab\t\t", /* width */ 4),
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
        assert_eq!(rows("ab\tcdef", /* width */ 6), ["ab⇥", "cdef"]);
        // Only `⇥` fits.
        assert_eq!(rows("abcde\tx", /* width */ 6), ["abcde⇥", "x"]);
        assert_eq!(
            pieces("ab\tcdef", /* width */ 6)[0],
            [piece("ab", Kind::Text), piece("⇥", Kind::DimmedWhitespace)]
        );
    }

    #[test]
    fn tab_after_a_break_reaches_the_next_tab_stop() {
        assert_eq!(rows("abcdef\tx", /* width */ 6), ["abcdef", "⇥   x"]);
        // Tab stops are counted from the start of the text area.
        assert_eq!(rows("abcdef g\tx", /* width */ 6), ["abcdef", " g⇥ x"]);
    }

    #[test]
    fn braille_blanks_at_a_break_are_dimmed_if_there_are_several() {
        assert_eq!(
            pieces("a\u{2800}\u{2800}cd", /* width */ 3)[0],
            [piece("a", Kind::Text), piece("∙∙", Kind::DimmedWhitespace)]
        );
        assert_eq!(
            pieces("abc\u{2800}\u{2800}d", /* width */ 3)[1],
            [piece("∙∙", Kind::DimmedWhitespace), piece("d", Kind::Text)]
        );
    }

    #[test]
    fn single_braille_blank_at_a_break_is_not_displayed() {
        assert_eq!(
            pieces("ab\u{2800}cd", /* width */ 3),
            [
                vec![piece("ab ", Kind::Text)],
                vec![piece("cd", Kind::Text)]
            ]
        );
        // Its column is left blank.
        assert_eq!(
            pieces("abc\u{2800}d", /* width */ 3)[1],
            [piece(" d", Kind::Text)]
        );
        assert_eq!(rows("a\u{2800}b", /* width */ 10), ["a\u{2800}b"]);
    }

    #[test]
    fn break_opportunities_are_computed_on_the_original_text() {
        // There is a break opportunity after a zero-width space, but not after `⌁`.
        assert_eq!(rows("ab\u{200b}cd", /* width */ 4), ["ab⌁", "cd"]);
    }

    #[test]
    fn segments_are_wrapped_as_a_single_line() {
        let segments = ["abc ", "de", "f gh"].map(Segment::new);
        let rows = wrap(&segments, FinalCr::Shown, /* width */ 8).unwrap();
        assert_eq!(
            rows,
            [
                TextRow {
                    start_byte: 0,
                    columns_count: 8,
                    pieces: vec![
                        (0, piece("abc", Kind::Text)),
                        (0, piece("∙", Kind::TrailingWhitespace)),
                        (1, piece("de", Kind::Text)),
                        (2, piece("f ", Kind::Text)),
                    ],
                },
                TextRow {
                    start_byte: 8,
                    columns_count: 2,
                    pieces: vec![(2, piece("gh", Kind::Text))],
                },
            ]
        );
    }

    #[test]
    fn tab_stops_are_counted_from_the_start_of_the_row_in_all_segments() {
        let rows = wrap(
            &["ab", "\tc"].map(Segment::new),
            FinalCr::Shown,
            /* width */ 20,
        )
        .unwrap();
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
        let rows = wrap(&segments, FinalCr::Shown, /* width */ 7).unwrap();
        assert_eq!(
            rows,
            [
                TextRow {
                    start_byte: 0,
                    columns_count: 7,
                    pieces: vec![
                        (0, piece("a", Kind::Text)),
                        (0, piece("∙", Kind::TrailingWhitespace)),
                        (1, piece(" (", Kind::Text)),
                        (2, piece("b  ", Kind::Text)),
                    ],
                },
                TextRow {
                    start_byte: 7,
                    columns_count: 4,
                    pieces: vec![(3, piece("  c ", Kind::Text))],
                },
            ]
        );
    }

    #[test]
    fn rows_start_at_their_first_character() {
        let starts: Vec<_> = wrap(
            &[Segment::new("the quick brown")],
            FinalCr::Shown,
            /* width */ 10,
        )
        .unwrap()
        .into_iter()
        .map(|row| row.start_byte)
        .collect();
        assert_eq!(starts, [0, 10]);
    }

    #[test]
    fn width_of_a_row_is_counted_per_grapheme_cluster() {
        // As a whole, `unicode-width` counts the lam-alef ligature `لا` as 1 column.
        let rows = wrap(&[Segment::new("سلام")], FinalCr::Shown, /* width */ 10).unwrap();
        assert_eq!(rows[0].columns_count, 4);
    }

    #[test]
    fn hidden_carriage_return_is_not_displayed() {
        let rows = wrap(
            &[Segment::new("ab\r")],
            FinalCr::Hidden,
            /* width */ 10,
        )
        .unwrap();
        assert_eq!(rows[0].pieces, [(0, piece("ab", Kind::Text))]);
        let rows = wrap(&[Segment::new("ab\r")], FinalCr::Shown, /* width */ 10).unwrap();
        assert_eq!(rows[0].pieces[1], (0, piece("␍", Kind::Replacement)));
    }

    #[test]
    fn too_narrow_text_area_is_an_error() {
        let error = wrap(&[Segment::new("a日")], FinalCr::Shown, /* width */ 1).unwrap_err();
        assert_eq!(error, TooNarrow { width: 1, cluster: "日".to_string() });
        assert_eq!(
            error.to_string(),
            "the terminal is too narrow to display \"日\" in a text area of 1 column"
        );
        assert!(wrap(&[Segment::new("a")], FinalCr::Shown, /* width */ 0).is_err());
        assert!(wrap(&[Segment::new(" ")], FinalCr::Shown, /* width */ 0).is_err());
    }
}
