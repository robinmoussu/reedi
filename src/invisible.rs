//! Replacement of the invisible characters by visible ones.

use std::iter;
use std::ops::Range;

use icu_properties::CodePointSetDataBorrowed;
use icu_properties::props::DefaultIgnorableCodePoint;
use icu_properties::props::ExtendedPictographic;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Number of columns between two tab stops.
const TAB_WIDTH: usize = 4;

/// Replacement of a tab, followed by spaces up to the next tab stop.
const TAB: char = '⇥';
/// Replacement of the trailing spaces, and of the spaces before a break.
const TRAILING_AND_WRAPPING_SPACE: char = '∙';
/// Replacement of U+2800 (braille pattern blank) when it is part of the trailing whitespace, or at
/// the end or at the beginning of a row because of a break.
const TRAILING_AND_WRAPPING_BRAILLE_BLANK: char = '∙';
/// Replacement of the spaces other than U+0020.
const NON_ASCII_SPACE: char = '⍽';
/// Replacement of a carriage return ending a line, when it is displayed.
const CARRIAGE_RETURN: char = '␍';
const DELETE: char = '␡';
/// Replacement of the line and paragraph separators.
const LINE_SEPARATOR: char = '␤';
/// Replacement of the bidirectional text controls.
const BIDI_CONTROL: char = '⇄';
/// Replacement of the zero-width characters, soft hyphen, variation selectors and tag characters.
const ZERO_WIDTH: char = '⌁';
/// Replacement of all the other invisible characters.
const OTHER: char = '⍰';

const BRAILLE_BLANK: &str = "\u{2800}";

const DEFAULT_IGNORABLE: CodePointSetDataBorrowed<'static> =
    CodePointSetDataBorrowed::new::<DefaultIgnorableCodePoint>();
const EMOJI: CodePointSetDataBorrowed<'static> =
    CodePointSetDataBorrowed::new::<ExtendedPictographic>();

/// Whether a carriage return ending a line is replaced or hidden.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalCr {
    Shown,
    Hidden,
}

/// What a piece of displayed text is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Text displayed as is.
    Text,
    /// Replacement of a tab, or of a space or U+2800 next to a break, that is not part of the
    /// trailing whitespace.
    DimmedWhitespace,
    /// Replacement of the trailing whitespace, including the tabs.
    TrailingWhitespace,
    /// Replacement of any other invisible character.
    Replacement,
}

/// Consecutive displayed characters of the same kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub text: String,
    pub kind: Kind,
}

/// Where a cluster is relative to the breaks of a line displayed on several rows. Only
/// [`Content::Contextual`] is displayed differently next to a break.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wrap {
    /// Not next to a break.
    None,
    /// In the whitespace ending a row that is followed by a continuation row of the same line.
    BeforeBreak,
    /// In the whitespace starting a continuation row.
    AfterBreak,
    /// The only space or U+2800 around a break (ending the row before it or starting the row
    /// after it). Will not be displayed
    Single,
}

/// How a grapheme cluster is displayed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    /// Displayed the same way wherever it is.
    Fixed(Vec<Piece>),
    /// Displayed depending on its column and on the breaks around it.
    Contextual(Contextual),
}

/// A character whose display depends on its context
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Contextual {
    /// A space (U+0020).
    Space,
    /// U+2800, braille pattern blank.
    BrailleBlank,
    /// A tab, expanded up to the next tab stop.
    Tab,
}

/// A grapheme cluster (what is displayed as a single character) of a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cluster {
    /// Position in the line, in bytes.
    pub range: Range<usize>,
    pub content: Content,
    /// Whether it is part of the trailing whitespace of the line.
    pub trailing: bool,
}

impl Cluster {
    /// Whether it is displayed depending on its context (see [`Contextual`]).
    pub fn is_contextual(&self) -> bool {
        matches!(self.content, Content::Contextual(_))
    }

    /// Number of columns used when displayed at `column`, if it is not next to a break.
    ///
    /// Next to a break, it never uses more columns (see [`Cluster::push`]). This is what allows
    /// to choose the breaks before knowing where the clusters are relative to them.
    pub fn width(&self, column: usize) -> usize {
        match &self.content {
            Content::Fixed(pieces) => pieces.iter().map(|piece| piece.text.width()).sum(),
            Content::Contextual(Contextual::Space | Contextual::BrailleBlank) => 1,
            Content::Contextual(Contextual::Tab) => TAB_WIDTH - column % TAB_WIDTH,
        }
    }

    /// Display the cluster after `pieces`.
    ///
    /// Unless trailing, the only space or U+2800 around a break is not displayed: its column is left
    /// blank. A tab is cut to the `room` columns left in the text area, and displayed alone
    /// (without the spaces up to the next tab stop) before a break.
    pub fn push(&self, pieces: &mut Pieces, wrap: Wrap, room: usize) {
        let whitespace = match &self.content {
            Content::Fixed(fixed) => {
                fixed
                    .iter()
                    .for_each(|piece| pieces.push(piece.kind, &piece.text));
                return;
            }
            Content::Contextual(whitespace) => *whitespace,
        };
        let replaced = if self.trailing {
            Kind::TrailingWhitespace
        } else {
            Kind::DimmedWhitespace
        };
        match (whitespace, wrap) {
            (Contextual::Space, _) if self.trailing => {
                pieces.push_char(replaced, TRAILING_AND_WRAPPING_SPACE)
            }
            (Contextual::BrailleBlank, _) if self.trailing => {
                pieces.push_char(replaced, TRAILING_AND_WRAPPING_BRAILLE_BLANK)
            }
            (Contextual::Space | Contextual::BrailleBlank, Wrap::Single) => {
                pieces.push(Kind::Text, " ")
            }
            (Contextual::Space, Wrap::BeforeBreak) => {
                pieces.push_char(replaced, TRAILING_AND_WRAPPING_SPACE)
            }
            (Contextual::BrailleBlank, Wrap::BeforeBreak | Wrap::AfterBreak) => {
                pieces.push_char(replaced, TRAILING_AND_WRAPPING_BRAILLE_BLANK)
            }
            (Contextual::Space, Wrap::None | Wrap::AfterBreak) => pieces.push(Kind::Text, " "),
            (Contextual::BrailleBlank, Wrap::None) => pieces.push(Kind::Text, BRAILLE_BLANK),
            (Contextual::Tab, _) => {
                let width = if wrap == Wrap::BeforeBreak {
                    1
                } else {
                    self.width(pieces.column).min(room).max(1)
                };
                let tab: String = iter::once(TAB)
                    .chain(iter::repeat_n(' ', width - 1))
                    .collect();
                pieces.push(replaced, &tab);
            }
        }
    }
}

/// How a single character of a cluster that is not whitespace is displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shown {
    Kept,
    Replaced(char),
}

fn is_non_ascii_space(c: char) -> bool {
    matches!(
        c,
        '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}'
    )
}

/// Whether `c` can be part of the trailing whitespace.
fn is_whitespace(c: char) -> bool {
    c == ' ' || c == '\t' || c == '\u{2800}' || is_non_ascii_space(c)
}

/// Whether `c` can be part of an emoji: zero-width joiner, variation selectors and tag
/// characters.
fn joins_emoji(c: char) -> bool {
    matches!(
        c,
        '\u{200d}' | '\u{fe00}'..='\u{fe0f}' | '\u{e0100}'..='\u{e01ef}' | '\u{e0000}'..='\u{e007f}'
    )
}

/// How `c` is displayed, if it is inside a grapheme cluster starting with an emoji (`in_emoji`).
fn shown(c: char, in_emoji: bool) -> Shown {
    match c {
        // Line feed and zero-width non-joiner.
        '\n' | '\u{200c}' => Shown::Kept,
        _ if in_emoji && joins_emoji(c) => Shown::Kept,
        _ if is_non_ascii_space(c) => Shown::Replaced(NON_ASCII_SPACE),
        // Control pictures are in the same order as the control characters, from U+2400.
        '\u{0}'..='\u{1f}' => Shown::Replaced(
            char::from_u32(0x2400 + u32::from(c)).expect("control pictures are valid characters"),
        ),
        '\u{7f}' => Shown::Replaced(DELETE),
        '\u{85}' | '\u{2028}' | '\u{2029}' => Shown::Replaced(LINE_SEPARATOR),
        '\u{61c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' => {
            Shown::Replaced(BIDI_CONTROL)
        }
        '\u{ad}' | '\u{34f}' | '\u{180e}' | '\u{200b}' | '\u{2060}' | '\u{feff}' => {
            Shown::Replaced(ZERO_WIDTH)
        }
        _ if joins_emoji(c) => Shown::Replaced(ZERO_WIDTH),
        // C1 control characters and Hangul fillers.
        '\u{80}'..='\u{9f}' | '\u{115f}' | '\u{1160}' | '\u{3164}' | '\u{ffa0}' => {
            Shown::Replaced(OTHER)
        }
        _ if DEFAULT_IGNORABLE.contains(c) => Shown::Replaced(OTHER),
        _ => Shown::Kept,
    }
}

/// Pieces of displayed text, with the column reached after them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pieces {
    pub pieces: Vec<Piece>,
    pub column: usize,
}

impl Pieces {
    /// No pieces yet, the next one being displayed at `column`.
    pub fn new(column: usize) -> Self {
        Self {
            pieces: Vec::new(),
            column,
        }
    }

    fn push(&mut self, kind: Kind, text: &str) {
        self.column += text.width();
        match self.pieces.last_mut() {
            Some(last) if last.kind == kind => last.text.push_str(text),
            _ => self.pieces.push(Piece {
                text: text.to_string(),
                kind,
            }),
        }
    }

    fn push_char(&mut self, kind: Kind, c: char) {
        self.push(kind, c.encode_utf8(&mut [0; 4]));
    }
}

/// The pieces displaying `cluster`, which is not whitespace.
fn fixed(cluster: &str, trailing: bool) -> Vec<Piece> {
    let in_emoji = cluster.chars().next().is_some_and(|c| EMOJI.contains(c));
    let shown: Vec<_> = cluster.chars().map(|c| shown(c, in_emoji)).collect();
    let mut pieces = Pieces::new(0);
    // Kept whole, so that its width is the one of the cluster, not the sum of its characters.
    if shown.iter().all(|shown| *shown == Shown::Kept) {
        pieces.push(Kind::Text, cluster);
        return pieces.pieces;
    }
    let replaced = if trailing {
        Kind::TrailingWhitespace
    } else {
        Kind::Replacement
    };
    cluster
        .chars()
        .zip(shown)
        .for_each(|(c, shown)| match shown {
            Shown::Kept => pieces.push_char(Kind::Text, c),
            Shown::Replaced(replacement) => pieces.push_char(replaced, replacement),
        });
    pieces.pieces
}

/// The clusters of `line`.
///
/// A carriage return ending `line` is a cluster replaced by `␍` if `final_cr` is
/// [`FinalCr::Shown`], and is hidden (not part of any cluster) otherwise.
pub fn clusters(line: &str, final_cr: FinalCr) -> Vec<Cluster> {
    let (content, ends_with_cr) = match line.strip_suffix('\r') {
        Some(content) => (content, true),
        None => (line, false),
    };
    // Grapheme clusters (what is displayed as a single character), with their position in the
    // line.
    let graphemes: Vec<(usize, &str)> = content.grapheme_indices(true).collect();
    let trailing_len = graphemes
        .iter()
        .rev()
        .take_while(|(_, grapheme)| grapheme.chars().all(is_whitespace))
        .count();
    let trailing_start = graphemes.len() - trailing_len;
    let clusters = graphemes.iter().enumerate().map(|(i, &(start, grapheme))| {
        let trailing = i >= trailing_start;
        let content = match grapheme {
            " " => Content::Contextual(Contextual::Space),
            BRAILLE_BLANK => Content::Contextual(Contextual::BrailleBlank),
            "\t" => Content::Contextual(Contextual::Tab),
            _ => Content::Fixed(fixed(grapheme, trailing)),
        };
        Cluster {
            range: start..start + grapheme.len(),
            content,
            trailing,
        }
    });
    let final_cr = if ends_with_cr && final_cr == FinalCr::Shown {
        Some(Cluster {
            range: content.len()..line.len(),
            content: Content::Fixed(vec![Piece {
                text: CARRIAGE_RETURN.to_string(),
                kind: Kind::Replacement,
            }]),
            trailing: false,
        })
    } else {
        None
    };
    clusters.chain(final_cr).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Display `line` on a single row.
    ///
    /// A carriage return ending `line` is replaced if `final_cr` is [`FinalCr::Shown`], and hidden
    /// otherwise.
    fn display(line: &str, final_cr: FinalCr) -> Vec<Piece> {
        let mut pieces = Pieces::new(0);
        clusters(line, final_cr)
            .iter()
            .for_each(|cluster| cluster.push(&mut pieces, Wrap::None, usize::MAX));
        pieces.pieces
    }

    /// The pieces displaying the `index`-th cluster of `line`, at `column`.
    fn cluster_at(line: &str, index: usize, column: usize, wrap: Wrap, room: usize) -> Vec<Piece> {
        let cluster = clusters(line, FinalCr::Shown).remove(index);
        let mut pieces = Pieces::new(column);
        cluster.push(&mut pieces, wrap, room);
        pieces.pieces
    }

    /// The displayed text of `line`, with the carriage return ending it shown.
    fn text(line: &str) -> String {
        display(line, FinalCr::Shown)
            .into_iter()
            .map(|piece| piece.text)
            .collect()
    }

    fn piece(text: &str, kind: Kind) -> Piece {
        Piece {
            text: text.to_string(),
            kind,
        }
    }

    #[test]
    fn visible_text_is_kept() {
        assert_eq!(
            display("let a = b;", FinalCr::Shown),
            [piece("let a = b;", Kind::Text)]
        );
        assert_eq!(display("", FinalCr::Shown), []);
    }

    #[test]
    fn tabs_reach_the_next_tab_stop() {
        assert_eq!(text("\tx"), "⇥   x");
        assert_eq!(text("a\tx"), "a⇥  x");
        assert_eq!(text("abc\tx"), "abc⇥x");
        assert_eq!(text("abcd\tx"), "abcd⇥   x");
        assert_eq!(text("\t\tx"), "⇥   ⇥   x");
    }

    #[test]
    fn tab_stops_use_the_displayed_width() {
        // `日` is 2 columns wide.
        assert_eq!(text("日\tx"), "日⇥ x");
        // The replacement of the zero-width space is 1 column wide.
        assert_eq!(text("\u{200b}\tx"), "⌁⇥  x");
    }

    #[test]
    fn tabs_are_their_own_kind_unless_trailing() {
        assert_eq!(
            display("\tx\t", FinalCr::Shown),
            [
                piece("⇥   ", Kind::DimmedWhitespace),
                piece("x", Kind::Text),
                piece("⇥  ", Kind::TrailingWhitespace),
            ]
        );
    }

    #[test]
    fn trailing_spaces_are_replaced() {
        assert_eq!(
            display("a b  ", FinalCr::Shown),
            [
                piece("a b", Kind::Text),
                piece("∙∙", Kind::TrailingWhitespace)
            ]
        );
    }

    #[test]
    fn trailing_braille_blank_is_trailing_whitespace() {
        assert_eq!(
            display("a\u{2800} \u{2800}", FinalCr::Shown),
            [
                piece("a", Kind::Text),
                piece("∙∙∙", Kind::TrailingWhitespace)
            ]
        );
    }

    #[test]
    fn braille_blank_is_kept_elsewhere() {
        assert_eq!(text("a\u{2800}b"), "a\u{2800}b");
    }

    #[test]
    fn whitespace_has_its_own_content() {
        let contents: Vec<_> = clusters("a \u{2800}\tb", FinalCr::Shown)
            .into_iter()
            .map(|cluster| cluster.content)
            .collect();
        assert_eq!(
            contents,
            [
                Content::Fixed(vec![piece("a", Kind::Text)]),
                Content::Contextual(Contextual::Space),
                Content::Contextual(Contextual::BrailleBlank),
                Content::Contextual(Contextual::Tab),
                Content::Fixed(vec![piece("b", Kind::Text)]),
            ]
        );
    }

    #[test]
    fn space_is_dimmed_before_a_break() {
        let space = |wrap| cluster_at(" x", 0, 0, wrap, usize::MAX);
        assert_eq!(space(Wrap::None), [piece(" ", Kind::Text)]);
        // A space moved to the next row is displayed normally.
        assert_eq!(space(Wrap::AfterBreak), [piece(" ", Kind::Text)]);
        assert_eq!(
            space(Wrap::BeforeBreak),
            [piece("∙", Kind::DimmedWhitespace)]
        );
        // Not displayed: its column is left blank.
        assert_eq!(space(Wrap::Single), [piece(" ", Kind::Text)]);
    }

    #[test]
    fn braille_blank_is_dimmed_next_to_a_break() {
        let blank = |wrap| cluster_at("\u{2800}x", 0, 0, wrap, usize::MAX);
        assert_eq!(blank(Wrap::None), [piece("\u{2800}", Kind::Text)]);
        assert_eq!(
            blank(Wrap::AfterBreak),
            [piece("∙", Kind::DimmedWhitespace)]
        );
        assert_eq!(
            blank(Wrap::BeforeBreak),
            [piece("∙", Kind::DimmedWhitespace)]
        );
        // Not displayed: its column is left blank.
        assert_eq!(blank(Wrap::Single), [piece(" ", Kind::Text)]);
    }

    #[test]
    fn trailing_whitespace_is_replaced_even_if_single_at_a_break() {
        assert_eq!(
            cluster_at("a ", 1, 1, Wrap::Single, usize::MAX),
            [piece("∙", Kind::TrailingWhitespace)]
        );
    }

    #[test]
    fn trailing_whitespace_next_to_a_break_stays_trailing_whitespace() {
        assert_eq!(
            cluster_at("a ", 1, 1, Wrap::BeforeBreak, usize::MAX),
            [piece("∙", Kind::TrailingWhitespace)]
        );
        assert_eq!(
            cluster_at("a\u{2800}", 1, 0, Wrap::AfterBreak, usize::MAX),
            [piece("∙", Kind::TrailingWhitespace)]
        );
        assert_eq!(
            cluster_at("a\t", 1, 1, Wrap::BeforeBreak, usize::MAX),
            [piece("⇥", Kind::TrailingWhitespace)]
        );
    }

    #[test]
    fn tab_before_a_break_is_displayed_alone() {
        assert_eq!(
            cluster_at("\tx", 0, 0, Wrap::BeforeBreak, usize::MAX),
            [piece("⇥", Kind::DimmedWhitespace)]
        );
        // After a break, it reaches the next tab stop.
        assert_eq!(
            cluster_at("\tx", 0, 1, Wrap::AfterBreak, usize::MAX),
            [piece("⇥  ", Kind::DimmedWhitespace)]
        );
    }

    #[test]
    fn tab_is_cut_to_the_room_left() {
        assert_eq!(
            cluster_at("\tx", 0, 0, Wrap::None, 2),
            [piece("⇥ ", Kind::DimmedWhitespace)]
        );
    }

    #[test]
    fn cluster_width_depends_on_the_column_for_tabs_only() {
        let clusters = clusters("日\t", FinalCr::Shown);
        assert_eq!(clusters[0].width(3), 2);
        assert_eq!(clusters[1].width(0), 4);
        assert_eq!(clusters[1].width(3), 1);
    }

    #[test]
    fn whitespace_is_not_wider_next_to_a_break() {
        let wraps = [
            Wrap::None,
            Wrap::BeforeBreak,
            Wrap::AfterBreak,
            Wrap::Single,
        ];
        // The whitespace is the first cluster, and is trailing whitespace in the last three lines.
        [" x", "\u{2800}x", "\tx", " ", "\u{2800}", "\t"]
            .into_iter()
            .for_each(|line| {
                let cluster = clusters(line, FinalCr::Shown).remove(0);
                (0..TAB_WIDTH).for_each(|column| {
                    wraps.into_iter().for_each(|wrap| {
                        let mut pieces = Pieces::new(column);
                        cluster.push(&mut pieces, wrap, usize::MAX);
                        assert!(
                            pieces.column - column <= cluster.width(column),
                            "{line:?} at column {column}, {wrap:?}"
                        );
                    });
                });
            });
    }

    #[test]
    fn whitespace_only_lines_are_trailing_whitespace() {
        assert_eq!(
            display(" \t\u{a0} ", FinalCr::Shown),
            [piece("∙⇥  ⍽∙", Kind::TrailingWhitespace)]
        );
    }

    #[test]
    fn final_carriage_return_is_shown_only_if_requested() {
        assert_eq!(text("a\r"), "a␍");
        assert_eq!(display("a\r", FinalCr::Hidden), [piece("a", Kind::Text)]);
    }

    #[test]
    fn trailing_whitespace_is_before_the_final_carriage_return() {
        assert_eq!(text("a \r"), "a∙␍");
        let hidden: String = display("a \r", FinalCr::Hidden)
            .into_iter()
            .map(|piece| piece.text)
            .collect();
        assert_eq!(hidden, "a∙");
    }

    #[test]
    fn carriage_returns_inside_a_line_are_always_shown() {
        assert_eq!(
            display("a\rb", FinalCr::Hidden),
            [
                piece("a", Kind::Text),
                piece("␍", Kind::Replacement),
                piece("b", Kind::Text),
            ]
        );
    }

    #[test]
    fn non_ascii_spaces_are_replaced() {
        assert_eq!(
            text("a\u{a0}b\u{1680}c\u{2000}d\u{200a}e\u{202f}f\u{205f}g\u{3000}h"),
            "a⍽b⍽c⍽d⍽e⍽f⍽g⍽h"
        );
    }

    #[test]
    fn control_characters_are_replaced_by_their_control_picture() {
        assert_eq!(text("\0\u{1b}[31mred\u{7}\u{1f}x"), "␀␛[31mred␇␟x");
        assert_eq!(text("a\u{7f}"), "a␡");
    }

    #[test]
    fn line_and_paragraph_separators_are_replaced() {
        assert_eq!(text("a\u{2028}b\u{85}c\u{2029}d"), "a␤b␤c␤d");
    }

    #[test]
    fn bidirectional_text_controls_are_replaced() {
        assert_eq!(
            text(
                "\u{202a}\u{202b}\u{202c}\u{202d}\u{202e}\u{2066}\u{2067}\u{2068}\u{2069}\u{200e}\u{200f}\u{61c}"
            ),
            "⇄⇄⇄⇄⇄⇄⇄⇄⇄⇄⇄⇄"
        );
    }

    #[test]
    fn zero_width_characters_are_replaced() {
        assert_eq!(
            text("a\u{200b}b\u{200d}c\u{2060}d\u{feff}e\u{ad}f\u{180e}g\u{34f}h"),
            "a⌁b⌁c⌁d⌁e⌁f⌁g⌁h"
        );
    }

    #[test]
    fn variation_selectors_and_tags_outside_an_emoji_are_replaced() {
        assert_eq!(text("a\u{fe0f}b\u{e0100}c\u{e0041}\u{e007f}"), "a⌁b⌁c⌁⌁");
    }

    #[test]
    fn emoji_sequences_are_kept() {
        let family = "👨\u{200d}👩\u{200d}👧";
        let heart = "❤\u{fe0f}";
        let scotland = "🏴\u{e0067}\u{e0062}\u{e0073}\u{e0063}\u{e0074}\u{e007f}";
        [family, heart, scotland].into_iter().for_each(|emoji| {
            assert_eq!(display(emoji, FinalCr::Shown), [piece(emoji, Kind::Text)])
        });
    }

    #[test]
    fn zero_width_non_joiner_is_kept() {
        let persian = "می\u{200c}خواهم";
        assert_eq!(text(persian), persian);
    }

    #[test]
    fn other_invisible_characters_are_replaced() {
        // C1 control character, Hangul fillers, invisible plus (a default ignorable code point).
        assert_eq!(
            text("\u{9b}a\u{115f}\u{1160}\u{3164}\u{ffa0}b\u{2064}"),
            "⍰a⍰⍰⍰⍰b⍰"
        );
    }

    #[test]
    fn replacements_are_grouped() {
        assert_eq!(
            display("a\u{200b}\u{202e}b", FinalCr::Shown),
            [
                piece("a", Kind::Text),
                piece("⌁⇄", Kind::Replacement),
                piece("b", Kind::Text),
            ]
        );
    }
}
