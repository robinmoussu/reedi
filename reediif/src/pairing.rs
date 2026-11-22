//! Pairing of the removed and added lines of a hunk, similar to what `delta` does: a removed line
//! is displayed next to an added line only if they are similar enough.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::iter;
use std::sync::LazyLock;

use foldhash::HashMap;

use regex::Regex;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::model::DiffLine;
use crate::model::Hunk;
use crate::model::LineKind;

/// Lines are split into words (matching this regex), and the characters between them.
static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\w+").expect("the regex is valid"));

const DELETION_COST: usize = 2;
const INSERTION_COST: usize = 2;
/// Extra cost for starting a new group of deleted or inserted tokens.
const INITIAL_MISMATCH_PENALTY: usize = 1;

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

/// The tokens of `line`, aligned by [`operations()`]: its words, and each grapheme cluster between
/// them, after one (or two, if the line doesn't start with a word) empty tokens.
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
    let second_empty = if words.first().is_some_and(|word| word.start() == 0) || line.is_empty() {
        None
    } else {
        Some("")
    };
    let ends = iter::once(0).chain(words.iter().map(|word| word.end()));
    let starts = words
        .iter()
        .map(|word| word.start())
        .chain(iter::once(line.len()));
    let words = words
        .iter()
        .map(|word| Some(word.as_str()))
        .chain(iter::once(None));
    let tokens = ends
        .zip(starts)
        .zip(words)
        .flat_map(|((end, start), word)| {
            line[end..start]
                .graphemes(/* is_extended */ true)
                .chain(word)
        });
    iter::once("").chain(second_empty).chain(tokens).collect()
}

/// [`tokenize()`] for an ASCII line without carriage return, in which the words are made of
/// `[A-Za-z0-9_]`, and each other character is a grapheme cluster.
fn ascii_tokens(line: &str) -> Vec<&str> {
    let bytes = line.as_bytes();
    let is_word = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    let second_empty = if bytes.first().is_none_or(|&byte| is_word(byte)) {
        None
    } else {
        Some("")
    };
    let mut start = 0;
    let tokens = iter::from_fn(|| {
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
    });
    iter::once("").chain(second_empty).chain(tokens).collect()
}

/// An operation transforming a sequence of tokens into another one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Operation {
    /// The token is in both sequences.
    NoOp,
    Deletion,
    Insertion,
}

/// A cell of the Needleman-Wunsch / Wagner-Fischer table. The cell it comes from is deduced from
/// its operation (it is the first cell for the cells of the first row and of the first column).
#[derive(Debug, Clone, Copy)]
struct Cell {
    cost: u32,
    operation: Operation,
}

/// The operations transforming `x` into `y`, with the minimal cost (in which consecutive
/// deletions or insertions cost less), each one with the number of consecutive tokens it applies
/// to.
fn operations<T: PartialEq>(x: &[T], y: &[T]) -> Vec<(Operation, usize)> {
    thread_local! {
        /// The table of the last call, kept to reuse its allocation.
        static TABLE: RefCell<Vec<Cell>> = const { RefCell::new(Vec::new()) };
    }
    TABLE.with_borrow_mut(|table| operations_in(x, y, table))
}

/// [`operations()`], computed in `table`, whose previous content is ignored.
fn operations_in<T: PartialEq>(x: &[T], y: &[T], table: &mut Vec<Cell>) -> Vec<(Operation, usize)> {
    // `x` is along the top of the table, `y` along the left side. The cell of `x[i]` and `y[j]` is
    // in the column `i + 1` and the row `j + 1`.
    let columns = x.len() + 1;
    let index = |i: usize, j: usize| j * columns + i;
    let cost = |cost: usize| u32::try_from(cost).expect("the cost fits in u32");
    table.clear();
    table.resize(
        columns * (y.len() + 1),
        Cell { cost: 0, operation: Operation::NoOp },
    );
    for i in 1..columns {
        table[index(i, 0)] = Cell {
            cost: cost(i * DELETION_COST + INITIAL_MISMATCH_PENALTY),
            operation: Operation::Deletion,
        };
    }
    for j in 1..=y.len() {
        table[index(0, j)] = Cell {
            cost: cost(j * INSERTION_COST + INITIAL_MISMATCH_PENALTY),
            operation: Operation::Insertion,
        };
    }
    let mismatch_cost = |parent: Cell, mismatch_cost: usize| {
        let penalty = if parent.operation == Operation::NoOp {
            INITIAL_MISMATCH_PENALTY
        } else {
            0
        };
        parent.cost + cost(mismatch_cost + penalty)
    };
    // Row by row, so that the cells read are close to each other in memory.
    y.iter().enumerate().for_each(|(j, y_j)| {
        x.iter().enumerate().for_each(|(i, x_i)| {
            let (left, diagonal, up) = (
                table[index(i, j + 1)],
                table[index(i, j)],
                table[index(i + 1, j)],
            );
            // If several candidates have the same cost, the first one is chosen. Insertions and
            // deletions are considered before matches, to group the modifications together.
            let insertion = mismatch_cost(up, INSERTION_COST);
            let deletion = mismatch_cost(left, DELETION_COST);
            let no_op = if x_i == y_j { diagonal.cost } else { u32::MAX };
            table[index(i + 1, j + 1)] = if insertion <= deletion && insertion <= no_op {
                Cell {
                    cost: insertion,
                    operation: Operation::Insertion,
                }
            } else if deletion <= no_op {
                Cell {
                    cost: deletion,
                    operation: Operation::Deletion,
                }
            } else {
                Cell { cost: no_op, operation: Operation::NoOp }
            };
        });
    });
    let parent = |cell_index: usize| {
        let (i, j) = (cell_index % columns, cell_index / columns);
        if i == 0 || j == 0 {
            return 0;
        }
        match table[cell_index].operation {
            Operation::Insertion => index(i, j - 1),
            Operation::Deletion => index(i - 1, j),
            Operation::NoOp => index(i - 1, j - 1),
        }
    };
    // The operations are read backwards, up to the first cell whose parent is the first one.
    let mut cell_index = index(x.len(), y.len());
    let mut operations = vec![table[cell_index].operation];
    while parent(cell_index) != 0 {
        cell_index = parent(cell_index);
        operations.push(table[cell_index].operation);
    }
    operations.reverse();
    operations
        .chunk_by(|a, b| a == b)
        .map(|group| (group[0], group.len()))
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
    ids: Vec<u32>,
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
fn tokens_distance(x: &Tokenized, y: &Tokenized) -> f64 {
    let width = |tokens: &[&str]| tokens.concat().trim().width();
    let operations = operations(&x.ids, &y.ids);
    let (x, y) = (&x.tokens, &y.tokens);
    let (_, _, changed, total) = operations.into_iter().fold(
        (0, 0, 0, 0),
        |(x_offset, y_offset, changed, total), (operation, tokens_count)| match operation {
            Operation::Deletion => {
                let width = width(&x[x_offset..x_offset + tokens_count]);
                (
                    x_offset + tokens_count,
                    y_offset,
                    changed + width,
                    total + width,
                )
            }
            Operation::Insertion => {
                let width = width(&y[y_offset..y_offset + tokens_count]);
                (
                    x_offset,
                    y_offset + tokens_count,
                    changed + width,
                    total + width,
                )
            }
            Operation::NoOp => {
                let width = width(&x[x_offset..x_offset + tokens_count]);
                (
                    x_offset + tokens_count,
                    y_offset + tokens_count,
                    changed,
                    total + 2 * width,
                )
            }
        },
    );
    if total > 0 {
        changed as f64 / total as f64
    } else {
        0.0
    }
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
        assert_eq!(tokenize("ab cd\n"), ["", "ab", " ", "cd", "\n"]);
        assert_eq!(tokenize("  a.é\n"), ["", "", " ", " ", "a", ".", "é", "\n"]);
        assert_eq!(tokenize("!\n"), ["", "", "!", "\n"]);
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
