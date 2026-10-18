//! Replacement of the invisible characters by visible ones.

use std::iter;

use icu_properties::CodePointSetDataBorrowed;
use icu_properties::props::DefaultIgnorableCodePoint;
use icu_properties::props::ExtendedPictographic;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Number of columns between two tab stops.
const TAB_WIDTH: usize = 4;

/// Replacement of a tab, followed by spaces up to the next tab stop.
const TAB: char = '⇥';
const TRAILING_SPACE: char = '∙';
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
    /// Replacement of a tab that is not part of the trailing whitespace.
    Tab,
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

/// How a single character is displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shown {
    Kept,
    /// Expanded up to the next tab stop.
    Tab,
    Replaced(char),
}

fn is_non_ascii_space(c: char) -> bool {
    matches!(
        c,
        '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}'
    )
}

fn is_whitespace(c: char) -> bool {
    c == ' ' || c == '\t' || is_non_ascii_space(c)
}

/// Whether `c` can be part of an emoji: zero-width joiner, variation selectors and tag
/// characters.
fn joins_emoji(c: char) -> bool {
    matches!(
        c,
        '\u{200d}' | '\u{fe00}'..='\u{fe0f}' | '\u{e0100}'..='\u{e01ef}' | '\u{e0000}'..='\u{e007f}'
    )
}

/// How `c` is displayed, if it is inside a grapheme cluster starting with an emoji (`in_emoji`),
/// and if it is part of the trailing whitespace (`trailing`).
fn shown(c: char, in_emoji: bool, trailing: bool) -> Shown {
    match c {
        '\t' => Shown::Tab,
        ' ' if trailing => Shown::Replaced(TRAILING_SPACE),
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
struct Pieces {
    pieces: Vec<Piece>,
    column: usize,
}

impl Pieces {
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

    fn push_cluster(&mut self, cluster: &str, trailing: bool) {
        let in_emoji = cluster.chars().next().is_some_and(|c| EMOJI.contains(c));
        let shown: Vec<_> = cluster
            .chars()
            .map(|c| shown(c, in_emoji, trailing))
            .collect();
        // Kept whole, so that its width is the one of the cluster, not the sum of its characters.
        if shown.iter().all(|shown| *shown == Shown::Kept) {
            self.push(Kind::Text, cluster);
            return;
        }
        cluster
            .chars()
            .zip(shown)
            .for_each(|(c, shown)| match shown {
                Shown::Kept => self.push_char(Kind::Text, c),
                Shown::Tab => {
                    let kind = if trailing {
                        Kind::TrailingWhitespace
                    } else {
                        Kind::Tab
                    };
                    let spaces = TAB_WIDTH - self.column % TAB_WIDTH - 1;
                    let tab: String = iter::once(TAB).chain(iter::repeat_n(' ', spaces)).collect();
                    self.push(kind, &tab);
                }
                Shown::Replaced(replacement) => {
                    let kind = if trailing {
                        Kind::TrailingWhitespace
                    } else {
                        Kind::Replacement
                    };
                    self.push_char(kind, replacement)
                }
            });
    }
}

/// Display `line` with its invisible characters replaced.
///
/// Tabs are expanded up to the next tab stop, the columns being counted from the beginning of
/// `line`. A carriage return ending `line` is replaced if `final_cr` is [`FinalCr::Shown`], and
/// hidden otherwise.
pub fn display(line: &str, final_cr: FinalCr) -> Vec<Piece> {
    let (content, ends_with_cr) = match line.strip_suffix('\r') {
        Some(content) => (content, true),
        None => (line, false),
    };
    // Grapheme clusters, what is displayed as a single character.
    let clusters: Vec<&str> = content.graphemes(true).collect();
    let trailing_len = clusters
        .iter()
        .rev()
        .take_while(|cluster| cluster.chars().all(is_whitespace))
        .count();
    let trailing_start = clusters.len() - trailing_len;
    let mut pieces = Pieces {
        pieces: Vec::new(),
        column: 0,
    };
    clusters
        .iter()
        .enumerate()
        .for_each(|(i, cluster)| pieces.push_cluster(cluster, i >= trailing_start));
    if ends_with_cr && final_cr == FinalCr::Shown {
        pieces.push_char(Kind::Replacement, CARRIAGE_RETURN);
    }
    pieces.pieces
}

#[cfg(test)]
mod tests {
    use super::*;

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
                piece("⇥   ", Kind::Tab),
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
