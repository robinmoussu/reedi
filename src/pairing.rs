//! Pairing of the removed and added lines of a chunk, like `delta` (with its default settings)
//! does: a removed line is displayed next to an added line only if they are similar enough.

use std::iter;
use std::sync::LazyLock;

use regex::Regex;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::model::Chunk;
use crate::model::DiffLine;
use crate::model::LineKind;

/// Maximum distance (between 0 and 1, see [`distance`]) between a removed and an added line for
/// them to be paired.
const MAX_LINE_DISTANCE: f64 = 0.6;

/// Lines are split into words (matching this regex), and the characters between them.
static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\w+").expect("the regex is valid"));

/// Before computing the distance between two lines, each tab is replaced by this many spaces.
const TAB_WIDTH: usize = 8;

const DELETION_COST: usize = 2;
const INSERTION_COST: usize = 2;
/// Extra cost for starting a new group of deleted or inserted tokens.
const INITIAL_MISMATCH_PENALTY: usize = 1;

/// A row of a chunk: the indexes (in [`Chunk::lines`]) of the lines displayed in each half.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pair {
    pub before: Option<usize>,
    pub after: Option<usize>,
}

/// The rows of `chunk`, in order.
///
/// An unchanged line is displayed in both halves. The removed and added lines between two
/// unchanged lines are paired greedily: each removed line is paired with the first following added
/// line that is similar enough, and the added lines skipped to find it are unpaired. A removed line
/// with no similar added line is unpaired.
pub fn pairs(chunk: &Chunk) -> Vec<Pair> {
    let lines: Vec<_> = chunk.lines.iter().enumerate().collect();
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
            let unchanged = indexes(LineKind::Unchanged).into_iter().map(|index| Pair {
                before: Some(index),
                after: Some(index),
            });
            let modified = pair_modified(
                &chunk.lines,
                &indexes(LineKind::Removed),
                &indexes(LineKind::Added),
            );
            unchanged.chain(modified)
        })
        .collect()
}

/// The rows displaying the `removed` and `added` lines (indexes in `lines`), see [`pairs`].
fn pair_modified(lines: &[DiffLine], removed: &[usize], added: &[usize]) -> Vec<Pair> {
    let mut next = 0;
    let mut pairs: Vec<_> = removed
        .iter()
        .flat_map(|&before| {
            let similar = added[next..].iter().position(|&after| {
                distance(&lines[before].text, &lines[after].text) <= MAX_LINE_DISTANCE
            });
            let rows: Vec<_> = match similar {
                Some(skipped) => added[next..next + skipped]
                    .iter()
                    .map(|&after| Pair {
                        before: None,
                        after: Some(after),
                    })
                    .chain(iter::once(Pair {
                        before: Some(before),
                        after: Some(added[next + skipped]),
                    }))
                    .collect(),
                None => vec![Pair {
                    before: Some(before),
                    after: None,
                }],
            };
            next += similar.map_or(0, |skipped| skipped + 1);
            rows
        })
        .collect();
    pairs.extend(added[next..].iter().map(|&after| Pair {
        before: None,
        after: Some(after),
    }));
    pairs
}

/// The tokens of `line`, aligned by [`operations`]: its words, and each grapheme cluster between
/// them, after one (or two, if the line doesn't start with a word) empty tokens.
fn tokenize(line: &str) -> Vec<&str> {
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
        .flat_map(|((end, start), word)| line[end..start].graphemes(true).chain(word));
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

/// A cell of the Needleman-Wunsch / Wagner-Fischer table.
#[derive(Debug, Clone, Copy)]
struct Cell {
    /// Index of the cell this one comes from.
    parent: usize,
    operation: Operation,
    cost: usize,
}

/// The operations transforming `x` into `y`, with the minimal cost (in which consecutive
/// deletions or insertions cost less), each one with the number of consecutive tokens it applies
/// to.
fn operations(x: &[&str], y: &[&str]) -> Vec<(Operation, usize)> {
    // `x` is along the top of the table, `y` along the left side. The cell of `x[i]` and `y[j]` is
    // in the column `i + 1` and the row `j + 1`.
    let columns = x.len() + 1;
    let index = |i: usize, j: usize| j * columns + i;
    let mut table = vec![
        Cell {
            parent: 0,
            operation: Operation::NoOp,
            cost: 0,
        };
        columns * (y.len() + 1)
    ];
    (1..columns).for_each(|i| {
        table[index(i, 0)] = Cell {
            parent: 0,
            operation: Operation::Deletion,
            cost: i * DELETION_COST + INITIAL_MISMATCH_PENALTY,
        };
    });
    (1..=y.len()).for_each(|j| {
        table[index(0, j)] = Cell {
            parent: 0,
            operation: Operation::Insertion,
            cost: j * INSERTION_COST + INITIAL_MISMATCH_PENALTY,
        };
    });
    let mismatch_cost = |table: &[Cell], parent: usize, cost: usize| {
        let penalty = if table[parent].operation == Operation::NoOp {
            INITIAL_MISMATCH_PENALTY
        } else {
            0
        };
        table[parent].cost + cost + penalty
    };
    x.iter().enumerate().for_each(|(i, x_i)| {
        y.iter().enumerate().for_each(|(j, y_j)| {
            let (left, diagonal, up) = (index(i, j + 1), index(i, j), index(i + 1, j));
            // If several candidates have the same cost, the first one is chosen. Insertions and
            // deletions are considered before matches, to group the modifications together.
            let candidates = [
                Cell {
                    parent: up,
                    operation: Operation::Insertion,
                    cost: mismatch_cost(&table, up, INSERTION_COST),
                },
                Cell {
                    parent: left,
                    operation: Operation::Deletion,
                    cost: mismatch_cost(&table, left, DELETION_COST),
                },
                Cell {
                    parent: diagonal,
                    operation: Operation::NoOp,
                    cost: if x_i == y_j {
                        table[diagonal].cost
                    } else {
                        usize::MAX
                    },
                },
            ];
            table[index(i + 1, j + 1)] = candidates
                .into_iter()
                .min_by_key(|cell| cell.cost)
                .expect("there are candidates");
        });
    });
    // The operations are read backwards, up to the first cell whose parent is the first one.
    let mut cell = table[index(x.len(), y.len())];
    let mut operations = vec![cell.operation];
    while cell.parent != 0 {
        cell = table[cell.parent];
        operations.push(cell.operation);
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
/// lines (ignoring the whitespace at the edges of each group of tokens).
fn distance(before: &str, after: &str) -> f64 {
    let prepare = |line: &str| line.replace('\t', &" ".repeat(TAB_WIDTH)) + "\n";
    let (before, after) = (prepare(before), prepare(after));
    let (x, y) = (tokenize(&before), tokenize(&after));
    let width = |tokens: &[&str]| tokens.concat().trim().width();
    let (_, _, changed, total) = operations(&x, &y).into_iter().fold(
        (0, 0, 0, 0),
        |(x_offset, y_offset, changed, total), (operation, n)| match operation {
            Operation::Deletion => {
                let width = width(&x[x_offset..x_offset + n]);
                (x_offset + n, y_offset, changed + width, total + width)
            }
            Operation::Insertion => {
                let width = width(&y[y_offset..y_offset + n]);
                (x_offset, y_offset + n, changed + width, total + width)
            }
            Operation::NoOp => {
                let width = width(&x[x_offset..x_offset + n]);
                (x_offset + n, y_offset + n, changed, total + 2 * width)
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

    fn line(kind: LineKind, text: &str) -> DiffLine {
        DiffLine {
            kind,
            text: text.to_string(),
            missing_newline: false,
        }
    }

    fn chunk(lines: &[(LineKind, &str)]) -> Chunk {
        Chunk {
            before_first_line: 1,
            after_first_line: 1,
            lines: lines.iter().map(|(kind, text)| line(*kind, text)).collect(),
        }
    }

    /// The pairs, as `(before, after)` tuples.
    fn pairs_of(lines: &[(LineKind, &str)]) -> Vec<(Option<usize>, Option<usize>)> {
        pairs(&chunk(lines))
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
    fn distance_of_identical_lines_is_zero() {
        assert_eq!(distance("let a = b;", "let a = b;"), 0.0);
        // Whitespace doesn't count.
        assert_eq!(distance("a b", "a  b\t"), 0.0);
    }

    #[test]
    fn distance_of_lines_with_nothing_in_common_is_one() {
        assert_eq!(distance("abc", "def"), 1.0);
    }

    #[test]
    fn distance_is_the_proportion_of_modified_text() {
        // `let ` and ` = 1;` are kept (3 + 4 columns, in both lines), `a` is replaced by `b`.
        assert_eq!(distance("let a = 1;", "let b = 1;"), 2.0 / 16.0);
    }

    #[test]
    fn unchanged_lines_are_paired_with_themselves() {
        assert_eq!(
            pairs_of(&[(Unchanged, "a"), (Unchanged, "b")]),
            [(Some(0), Some(0)), (Some(1), Some(1))]
        );
    }

    #[test]
    fn similar_lines_are_paired() {
        assert_eq!(
            pairs_of(&[
                (Unchanged, "fn a() {"),
                (Removed, "    let x = 1;"),
                (Removed, "    let y = 2;"),
                (Added, "    let x = 10;"),
                (Added, "    let y = 20;"),
                (Unchanged, "}"),
            ]),
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
            pairs_of(&[(Removed, "abc"), (Added, "def")]),
            [(Some(0), None), (None, Some(1))]
        );
    }

    #[test]
    fn added_lines_skipped_to_find_a_similar_one_are_unpaired() {
        assert_eq!(
            pairs_of(&[
                (Removed, "let value = compute(input);"),
                (Added, "// The input is validated first."),
                (Added, "let value = compute(&input);"),
                (Added, "return value;"),
            ]),
            [(None, Some(1)), (Some(0), Some(2)), (None, Some(3))]
        );
    }

    #[test]
    fn removed_and_added_only_lines_are_unpaired() {
        assert_eq!(
            pairs_of(&[
                (Unchanged, "a"),
                (Removed, "b"),
                (Unchanged, "c"),
                (Added, "d")
            ]),
            [
                (Some(0), Some(0)),
                (Some(1), None),
                (Some(2), Some(2)),
                (None, Some(3))
            ]
        );
    }
}
