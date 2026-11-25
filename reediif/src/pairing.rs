//! Pairing of the removed and added lines of a hunk, similar to what `delta` does: a removed line
//! is displayed next to an added line only if they are similar enough.

use std::cmp::Ordering;
use std::iter;
use std::ops::Range;
use std::sync::LazyLock;

use foldhash::HashMap;
use gix_imara_diff::Algorithm;
use gix_imara_diff::Diff;
use gix_imara_diff::Token;

use regex::Regex;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::model::DiffLine;
use crate::model::Hunk;
use crate::model::LineKind;

/// Lines are split into words (matching this regex), and the characters between them.
static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\w+").expect("the regex is valid"));

/// A row of a hunk: the indexes (in [`Hunk::lines`]) of the lines displayed in each half.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pair {
    pub before: Option<usize>,
    pub after: Option<usize>,
}

/// The rows of `hunk`, in order.
///
/// An unchanged line is displayed in both halves. The removed and added lines between two
/// unchanged lines are paired greedily: each removed line is paired with the first following added
/// line that is similar enough, and the added lines skipped to find it are unpaired. A removed line
/// with no similar added line is unpaired.
///
/// Two lines are similar enough if their distance is at most `max_distance`: the distance is the
/// proportion of their text that is modified, between 0 (identical) and 1 (nothing in common), with
/// each tab counting as `tab_width` spaces.
pub fn pairs(hunk: &Hunk, max_distance: f64, tab_width: usize) -> Vec<Pair> {
    let lines: Vec<_> = hunk.lines.iter().enumerate().collect();
    lines
        .chunk_by(|(_, a), (_, b)| {
            (a.kind == LineKind::Unchanged) == (b.kind == LineKind::Unchanged)
        })
        .flat_map(|group| {
            let indexes = |kind| {
                group
                    .iter()
                    .filter(move |(_, line)| line.kind == kind)
                    .map(|(index, _)| *index)
                    .collect::<Vec<_>>()
            };
            let unchanged = indexes(LineKind::Unchanged)
                .into_iter()
                .map(|index| Pair { before: Some(index), after: Some(index) });
            let modified = pair_modified(
                &hunk.lines,
                &indexes(LineKind::Removed),
                &indexes(LineKind::Added),
                max_distance,
                tab_width,
            );
            unchanged.chain(modified)
        })
        .collect()
}

/// The rows displaying the `removed` and `added` lines (indexes in `lines`), see [`pairs()`].
fn pair_modified(
    lines: &[DiffLine],
    removed: &[usize],
    added: &[usize],
    max_distance: f64,
    tab_width: usize,
) -> Vec<Pair> {
    // Each line is compared with several others: it is prepared and tokenized once.
    let prepare = |indexes: &[usize]| {
        indexes
            .iter()
            .map(|&index| prepared(&lines[index].text, tab_width))
            .collect::<Vec<_>>()
    };
    let (removed_texts, added_texts) = (prepare(removed), prepare(added));
    let mut ids = HashMap::default();
    let removed_tokens: Vec<_> = removed_texts
        .iter()
        .map(|text| tokenized(text, &mut ids))
        .collect();
    let added_tokens: Vec<_> = added_texts
        .iter()
        .map(|text| tokenized(text, &mut ids))
        .collect();
    let mut next = 0;
    let mut pairs: Vec<_> = removed
        .iter()
        .zip(&removed_tokens)
        .flat_map(|(&before, before_tokens)| {
            let similar = added_tokens[next..]
                .iter()
                .position(|after_tokens| is_similar(before_tokens, after_tokens, max_distance));
            let rows: Vec<_> = match similar {
                Some(skipped) => added[next..next + skipped]
                    .iter()
                    .map(|&after| Pair { before: None, after: Some(after) })
                    .chain(iter::once(Pair {
                        before: Some(before),
                        after: Some(added[next + skipped]),
                    }))
                    .collect(),
                None => vec![Pair { before: Some(before), after: None }],
            };
            next += similar.map_or(0, |skipped| skipped + 1);
            rows
        })
        .collect();
    pairs.extend(
        added[next..]
            .iter()
            .map(|&after| Pair { before: None, after: Some(after) }),
    );
    pairs
}

/// The tokens of `line`: its words, and each grapheme cluster between them.
fn tokenize(line: &str) -> Vec<&str> {
    if line.is_ascii() && !line.contains('\r') {
        ascii_tokens(line)
    } else {
        unicode_tokens(line)
    }
}

/// [`tokenize()`] for any line.
fn unicode_tokens(line: &str) -> Vec<&str> {
    let words: Vec<_> = WORD.find_iter(line).collect();
    let ends = iter::once(0).chain(words.iter().map(|word| word.end()));
    let starts = words
        .iter()
        .map(|word| word.start())
        .chain(iter::once(line.len()));
    let words = words
        .iter()
        .map(|word| Some(word.as_str()))
        .chain(iter::once(None));
    ends.zip(starts)
        .zip(words)
        .flat_map(|((end, start), word)| {
            line[end..start]
                .graphemes(/* is_extended */ true)
                .chain(word)
        })
        .collect()
}

/// [`tokenize()`] for an ASCII line without carriage return, in which the words are made of
/// `[A-Za-z0-9_]`, and each other character is a grapheme cluster.
fn ascii_tokens(line: &str) -> Vec<&str> {
    let bytes = line.as_bytes();
    let is_word = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    let mut start = 0;
    iter::from_fn(|| {
        let first = *bytes.get(start)?;
        let len = if is_word(first) {
            bytes[start..]
                .iter()
                .take_while(|&&byte| is_word(byte))
                .count()
        } else {
            1
        };
        let token = &line[start..start + len];
        start += len;
        Some(token)
    })
    .collect()
}

/// The distance between `before` and `after`, between 0 (identical) and 1 (nothing in common).
///
/// It is the width of the deleted and inserted text, divided by the width of all the text of both
/// lines (ignoring the whitespace at the edges of each group of tokens). Each tab counts as
/// `tab_width` spaces.
#[cfg(test)]
fn distance(before: &str, after: &str, tab_width: usize) -> f64 {
    compare(before, after, tab_width, tokens_distance)
}

/// The result of `comparison` on `before` and `after`, prepared and tokenized with `tab_width`.
#[cfg(test)]
fn compare<T>(
    before: &str,
    after: &str,
    tab_width: usize,
    comparison: impl Fn(&Tokenized, &Tokenized) -> T,
) -> T {
    let (before, after) = (prepared(before, tab_width), prepared(after, tab_width));
    let mut ids = HashMap::default();
    comparison(&tokenized(&before, &mut ids), &tokenized(&after, &mut ids))
}

/// `line` as it is tokenized: with each tab replaced by `tab_width` spaces, and followed by a
/// newline.
fn prepared(line: &str, tab_width: usize) -> String {
    line.replace('\t', &" ".repeat(tab_width)) + "\n"
}

/// A prepared line (see [`prepared()`]), tokenized for its comparisons with other lines.
struct Tokenized<'a> {
    tokens: Vec<&'a str>,
    /// The tokens, interned: equal tokens have the same id.
    ids: Vec<Token>,
    /// Width of the tokens without whitespace, which the distance never trims.
    nonblank_width: usize,
    /// The id and the width of each token, and whether it has whitespace, sorted by id.
    sorted: Vec<(u32, usize, bool)>,
}

/// `text` tokenized, its tokens interned in `ids`.
fn tokenized<'a>(text: &'a str, ids: &mut HashMap<&'a str, u32>) -> Tokenized<'a> {
    let tokens = tokenize(text);
    let token_ids = tokens
        .iter()
        .map(|token| {
            let next_id = u32::try_from(ids.len()).expect("fewer tokens than u32::MAX");
            *ids.entry(*token).or_insert(next_id)
        })
        .collect::<Vec<_>>();
    let mut sorted: Vec<_> = tokens
        .iter()
        .zip(&token_ids)
        .map(|(token, id)| (*id, token.width(), token.chars().any(char::is_whitespace)))
        .collect();
    let token_ids = token_ids.into_iter().map(Token).collect();
    sorted.sort_unstable();
    let nonblank_width = sorted
        .iter()
        .filter(|(_, _, has_whitespace)| !has_whitespace)
        .map(|(_, width, _)| width)
        .sum();
    Tokenized {
        tokens,
        ids: token_ids,
        nonblank_width,
        sorted,
    }
}

/// The width of the tokens `x` and `y` (see [`Tokenized::sorted`]) have in common, counted as many
/// times as they are in both, and of those without whitespace.
fn common_widths(x: &[(u32, usize, bool)], y: &[(u32, usize, bool)]) -> (usize, usize) {
    let (mut i, mut j, mut common, mut common_nonblank) = (0, 0, 0, 0);
    while i < x.len() && j < y.len() {
        match x[i].0.cmp(&y[j].0) {
            Ordering::Less => i += 1,
            Ordering::Greater => j += 1,
            Ordering::Equal => {
                let (_, width, has_whitespace) = x[i];
                common += width;
                if !has_whitespace {
                    common_nonblank += width;
                }
                i += 1;
                j += 1;
            }
        }
    }
    (common, common_nonblank)
}

/// Whether the [`distance()`] between the lines tokenized as `x` and `y` is at most
/// `max_distance`.
fn is_similar(x: &Tokenized, y: &Tokenized, max_distance: f64) -> bool {
    // A lower bound of the distance, much cheaper to compute: the kept tokens are common to both
    // lines, so at least the tokens without whitespace that aren't common are changed, and at most
    // the common tokens are kept.
    let (common, common_nonblank) = common_widths(&x.sorted, &y.sorted);
    let changed = x.nonblank_width + y.nonblank_width - 2 * common_nonblank;
    let kept = common;
    if changed > 0 && changed as f64 / (changed + 2 * kept) as f64 > max_distance {
        return false;
    }
    tokens_distance(x, y) <= max_distance
}

/// The [`distance()`] between the lines tokenized as `x` and `y`.
///
/// The kept tokens are found by `gix-imara-diff`, with the Myers algorithm, like the lines of the
/// hunks.
fn tokens_distance(x: &Tokenized, y: &Tokenized) -> f64 {
    let width = |tokens: &[&str]| tokens.concat().trim().width();
    let mut diff = Diff::default();
    // `num_tokens` is only used by the histogram algorithm.
    diff.compute_with(Algorithm::Myers, &x.ids, &y.ids, /* num_tokens */ 0);
    let (kept_start, changed, kept) =
        diff.hunks()
            .fold((0, 0, 0), |(kept_start, changed, kept), hunk| {
                let (before, after) = (indexes(&hunk.before), indexes(&hunk.after));
                (
                    before.end,
                    changed + width(&x.tokens[before.clone()]) + width(&y.tokens[after]),
                    kept + width(&x.tokens[kept_start..before.start]),
                )
            });
    let kept = kept + width(&x.tokens[kept_start..]);
    let total = changed + 2 * kept;
    if total > 0 {
        changed as f64 / total as f64
    } else {
        0.0
    }
}

/// The indexes of the tokens of a range of `gix-imara-diff`.
fn indexes(range: &Range<u32>) -> Range<usize> {
    range.start as usize..range.end as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LineKind::Added;
    use crate::model::LineKind::Removed;
    use crate::model::LineKind::Unchanged;
    use crate::test_support::line;

    /// No line of these tests has a tab between non-whitespace characters, so the tab width doesn't
    /// change their distances.
    const TAB_WIDTH: usize = 4;

    /// The lines of these tests are similar if at least half of their text is kept.
    const MAX_DISTANCE: f64 = 0.5;

    fn hunk(lines: &[(LineKind, &str)]) -> Hunk {
        Hunk {
            before_first_line: 1,
            after_first_line: 1,
            lines: lines.iter().map(|(kind, text)| line(*kind, text)).collect(),
        }
    }

    /// The pairs, as `(before, after)` tuples.
    fn pairs_of(
        max_distance: f64,
        lines: &[(LineKind, &str)],
    ) -> Vec<(Option<usize>, Option<usize>)> {
        pairs(&hunk(lines), max_distance, TAB_WIDTH)
            .into_iter()
            .map(|pair| (pair.before, pair.after))
            .collect()
    }

    #[test]
    fn tokens_are_words_and_grapheme_clusters() {
        assert_eq!(tokenize("ab cd\n"), ["ab", " ", "cd", "\n"]);
        assert_eq!(tokenize("  a.é\n"), [" ", " ", "a", ".", "é", "\n"]);
        assert_eq!(tokenize("!\n"), ["!", "\n"]);
    }

    #[test]
    fn lines_are_similar_if_their_distance_is_at_most_the_maximum() {
        // Lines of close widths, with few tokens in common, and of widths different enough for the
        // distance to be above the maximum whatever their content.
        let lines = [
            "let a = b;",
            "let c = d;",
            "fn main() {}",
            "x",
            "let a = b; // a comment much longer than the code before it",
            "\tlet a = b;",
        ];
        for before in lines {
            for after in lines {
                let similar = compare(before, after, TAB_WIDTH, |x, y| {
                    is_similar(x, y, MAX_DISTANCE)
                });
                let distance = distance(before, after, TAB_WIDTH);
                assert_eq!(similar, distance <= MAX_DISTANCE, "{before:?}, {after:?}");
            }
        }
    }

    #[test]
    fn ascii_lines_are_tokenized_like_any_line() {
        let all_but_carriage_return: String = (0..=127_u8)
            .map(char::from)
            .filter(|&c| c != '\r')
            .collect();
        for line in [
            "",
            "a",
            " a",
            "let a_1 = b2(c);\n",
            all_but_carriage_return.as_str(),
        ] {
            assert_eq!(ascii_tokens(line), unicode_tokens(line), "{line:?}");
        }
    }

    #[test]
    fn distance_of_identical_lines_is_zero() {
        assert_eq!(distance("let a = b;", "let a = b;", TAB_WIDTH), 0.0);
        // Whitespace doesn't count.
        assert_eq!(distance("a b", "a  b\t", TAB_WIDTH), 0.0);
    }

    #[test]
    fn distance_of_lines_with_nothing_in_common_is_one() {
        assert_eq!(distance("abc", "def", TAB_WIDTH), 1.0);
    }

    #[test]
    fn distance_is_the_proportion_of_modified_text() {
        // `let ` and ` = 1;` are kept (3 + 4 columns, in both lines), `a` is replaced by `b`.
        assert_eq!(distance("let a = 1;", "let b = 1;", TAB_WIDTH), 2.0 / 16.0);
    }

    #[test]
    fn unchanged_lines_are_paired_with_themselves() {
        assert_eq!(
            // Even if no modification is similar enough.
            pairs_of(
                /* max_distance */ 0.0,
                &[(Unchanged, "a"), (Unchanged, "b")]
            ),
            [(Some(0), Some(0)), (Some(1), Some(1))]
        );
    }

    #[test]
    fn similar_lines_are_paired() {
        assert_eq!(
            pairs_of(
                MAX_DISTANCE,
                &[
                    (Unchanged, "fn a() {"),
                    (Removed, "    let x = 1;"),
                    (Removed, "    let y = 2;"),
                    (Added, "    let x = 10;"),
                    (Added, "    let y = 20;"),
                    (Unchanged, "}"),
                ]
            ),
            [
                (Some(0), Some(0)),
                (Some(1), Some(3)),
                (Some(2), Some(4)),
                (Some(5), Some(5))
            ]
        );
    }

    #[test]
    fn dissimilar_lines_are_unpaired() {
        assert_eq!(
            pairs_of(MAX_DISTANCE, &[(Removed, "abc"), (Added, "def")]),
            [(Some(0), None), (None, Some(1))]
        );
    }

    #[test]
    fn added_lines_skipped_to_find_a_similar_one_are_unpaired() {
        assert_eq!(
            pairs_of(
                MAX_DISTANCE,
                &[
                    (Removed, "let value = compute(input);"),
                    (Added, "// The input is validated first."),
                    (Added, "let value = compute(&input);"),
                    (Added, "return value;"),
                ]
            ),
            [(None, Some(1)), (Some(0), Some(2)), (None, Some(3))]
        );
    }

    #[test]
    fn removed_and_added_only_lines_are_unpaired() {
        assert_eq!(
            // Even if any two lines are similar enough.
            pairs_of(
                /* max_distance */ 1.0,
                &[
                    (Unchanged, "a"),
                    (Removed, "b"),
                    (Unchanged, "c"),
                    (Added, "d")
                ]
            ),
            [
                (Some(0), Some(0)),
                (Some(1), None),
                (Some(2), Some(2)),
                (None, Some(3))
            ]
        );
    }
}
