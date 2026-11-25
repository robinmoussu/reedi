#!/usr/bin/env -S cargo +nightly -Zscript --quiet
---
[package]
edition = "2024"

[dependencies]
clap = { version = "4.6.7", features = ["derive"] }
serde_json = "1.0.154"
---
//! `etc/compare.rs [base]`: display the performance differences between the working tree and
//! `base` (default: `HEAD`), with the results saved by `etc/measure.rs`: the working tree is
//! measured if its results aren't saved yet, and `base` after a confirmation (see
//! `etc/profiling.md`).
//!
//! `etc/compare.rs --results <base> <new> [<base> <new>…]`: compare result files, each the
//! measurements of a revision on one repository.
//!
//! Print which parts of the identities differ, a table of the measures, and a warning for each
//! measure that degrades by more than its threshold and for each target of the README that is
//! exceeded, in which case the exit status is 1. A time that degrades by more than its threshold
//! only gets a note if the difference is at most `MIN_TIME_DIFFERENCE_S`, or, in a scenario or a
//! bench that counts instructions, if they don't degrade by more than their threshold: times vary
//! between runs of the same binary.

use std::fs;
use std::io;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::process::ExitCode;
use std::process::Stdio;

use clap::Parser;

use serde_json::Map;
use serde_json::Value;

/// The script measuring a revision, relative to the working tree.
const MEASURE_SCRIPT: &str = "etc/measure.rs";

const INSTRUCTIONS_THRESHOLD: f64 = 0.01;

/// The degradation of a measure above which a remark is printed, as a fraction of its base value,
/// and what the measure is.
const THRESHOLDS: [(&str, f64, Kind); 5] = [
    ("instructions", INSTRUCTIONS_THRESHOLD, Kind::Stable),
    ("cpu_time_s", 0.05, Kind::Time),
    ("wall_time_s", 0.05, Kind::Time),
    ("peak_memory_kb", 0.05, Kind::Stable),
    // The fastest time of a bench, a wall time.
    ("fastest_s", 0.05, Kind::Time),
];

/// The difference of a time, in seconds, up to which its degradation only gets a note.
const MIN_TIME_DIFFERENCE_S: f64 = 0.005;

/// Whether a measure varies between runs of the same binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Stable,
    /// A time, in seconds, which varies.
    Time,
}

/// What is printed after a measure.
#[derive(Debug, PartialEq)]
enum Remark {
    /// A broken threshold or an exceeded target, which makes the exit status 1.
    Warning(String),
    /// A time degraded by more than its threshold, by too little to make the exit status 1.
    Note(String),
}

impl Remark {
    /// Whether the remark makes the exit status 1.
    fn blocks(&self) -> bool {
        matches!(self, Remark::Warning(_))
    }

    fn text(&self) -> &str {
        match self {
            Remark::Warning(text) | Remark::Note(text) => text,
        }
    }
}

/// The scenario that only opens `reedi` then quits, whose wall time is the time to open.
const OPEN_SCENARIO: &str = "open";

/// The targets of the README: the measure, the maximal value, and whether it only applies to
/// [`OPEN_SCENARIO`].
const TARGETS: [(&str, f64, Scenarios); 4] = [
    ("peak_memory_kb", 500.0 * 1024.0, Scenarios::All),
    ("wall_time_s", 1.0, Scenarios::Open),
    ("first_full_screen_ms", 200.0, Scenarios::All),
    ("longest_refresh_ms", 500.0, Scenarios::All),
];

/// The scenarios a target applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scenarios {
    All,
    Open,
}

#[derive(Debug, Parser)]
struct Arguments {
    /// Compare these result files, by pairs of a base and a new one, instead of revisions.
    #[arg(long, num_args = 2.., conflicts_with = "base")]
    results: Vec<PathBuf>,
    /// The revision compared with the working tree.
    base: Option<String>,
}

fn main() -> ExitCode {
    let arguments = Arguments::parse();
    let root = output(Command::new("git").args(["rev-parse", "--show-toplevel"]));
    std::env::set_current_dir(root).unwrap();
    let paths = if arguments.results.is_empty() {
        let base = arguments.base.as_deref().unwrap_or("HEAD");
        match measured_pairs(base) {
            Some(paths) => paths,
            None => return ExitCode::FAILURE,
        }
    } else {
        arguments.results
    };
    if paths.len() % 2 != 0 {
        eprintln!("--results takes pairs of files");
        return ExitCode::FAILURE;
    }
    let mut broken = false;
    for pair in paths.chunks(2) {
        let [base, new] = [&pair[0], &pair[1]].map(|path| read(path));
        broken |= print_comparison(&base, &new);
    }
    if broken {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// The stdout of `command`, without its final newline. Panics if it fails.
fn output(command: &mut Command) -> String {
    let output = command.stderr(Stdio::inherit()).output().unwrap();
    assert!(output.status.success(), "{command:?} failed");
    String::from_utf8(output.stdout)
        .unwrap()
        .trim_end()
        .to_string()
}

/// Run `etc/measure.rs` with `args`, its output shown. Panics if it fails.
fn measure(args: &[&str]) {
    let status = Command::new(MEASURE_SCRIPT).args(args).status().unwrap();
    assert!(status.success(), "{MEASURE_SCRIPT} {args:?} failed");
}

/// The result files of `revision` (`None`: the working tree), one per measured repository.
fn result_files(revision: Option<&str>) -> Vec<PathBuf> {
    output(
        Command::new(MEASURE_SCRIPT)
            .arg("--result-files")
            .args(revision),
    )
    .lines()
    .map(PathBuf::from)
    .collect()
}

/// The result files of `base` and of the working tree, by pairs, measuring the working tree if
/// its results aren't saved, and `base` if the user confirms it. `None` if the user doesn't.
fn measured_pairs(base: &str) -> Option<Vec<PathBuf>> {
    measure(&["--generate"]);
    let mut new_files = result_files(None);
    if !new_files.iter().all(|file| file.exists()) {
        measure(&["--no-generate"]);
        new_files = result_files(None);
    }
    let mut base_files = result_files(Some(base));
    if !base_files.iter().all(|file| file.exists()) {
        print!("Build and measure {base}? [Y/n] ");
        io::stdout().flush().unwrap();
        let mut answer = String::new();
        io::stdin().read_line(&mut answer).unwrap();
        if answer.trim().eq_ignore_ascii_case("n") {
            return None;
        }
        measure(&["--no-generate", base]);
        base_files = result_files(Some(base));
    }
    Some(
        base_files
            .into_iter()
            .zip(new_files)
            .flat_map(|(base, new)| [base, new])
            .collect(),
    )
}

fn read(path: &PathBuf) -> Value {
    let text =
        fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// Print the comparison of the measurements `base` and `new` of one repository. Return whether a
/// threshold or a target is broken.
fn print_comparison(base: &Value, new: &Value) -> bool {
    let repository = new["repository"].as_str().unwrap_or("?");
    println!("== repository: {repository}");
    let differences = identity_differences(&base["identity"], &new["identity"], "");
    println!("identity differs in: {}", differences.join(", "));
    let mut broken = false;
    let empty = Map::new();
    let groups =
        |measurements: &Value| measurements["groups"].as_object().unwrap_or(&empty).clone();
    let (base_groups, new_groups) = (groups(base), groups(new));
    for (group, new_measures) in &new_groups {
        println!("\n{group:<24} {:>14} {:>14} {:>9}", "base", "new", "diff");
        let base_measures = base_groups.get(group);
        let base_value_of =
            |measure: &str| base_measures.and_then(|measures| measures[measure].as_f64());
        let instructions_difference = difference(
            base_value_of("instructions"),
            new_measures["instructions"].as_f64(),
        );
        for (measure, new_value) in new_measures.as_object().unwrap_or(&empty) {
            let base_value = base_value_of(measure);
            let new_value = new_value.as_f64();
            let remarks = remarks(
                group,
                measure,
                base_value,
                new_value,
                instructions_difference,
            );
            broken |= remarks.iter().any(Remark::blocks);
            println!(
                "{measure:<24} {:>14} {:>14} {:>9}  {}",
                formatted(base_value),
                formatted(new_value),
                difference(base_value, new_value).map_or("–".to_string(), |difference| format!(
                    "{:+.1} %",
                    difference * 100.0
                )),
                remarks
                    .iter()
                    .map(Remark::text)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
    println!();
    broken
}

/// The dotted paths of the fields that differ between the identities `base` and `new`, below
/// `prefix`.
fn identity_differences(base: &Value, new: &Value, prefix: &str) -> Vec<String> {
    match (base, new) {
        (Value::Object(base_fields), Value::Object(new_fields)) => {
            let mut names = base_fields
                .keys()
                .chain(new_fields.keys())
                .collect::<Vec<_>>();
            names.sort();
            names.dedup();
            names
                .into_iter()
                .flat_map(|name| {
                    let path = if prefix.is_empty() {
                        name.clone()
                    } else {
                        format!("{prefix}.{name}")
                    };
                    let missing = Value::Null;
                    identity_differences(
                        base_fields.get(name).unwrap_or(&missing),
                        new_fields.get(name).unwrap_or(&missing),
                        &path,
                    )
                })
                .collect()
        }
        _ if base == new => Vec::new(),
        _ => vec![prefix.to_string()],
    }
}

/// The relative difference from `base` to `new`, if both are known and `base` isn't 0.
fn difference(base: Option<f64>, new: Option<f64>) -> Option<f64> {
    match (base, new) {
        (Some(base), Some(new)) if base != 0.0 => Some((new - base) / base),
        _ => None,
    }
}

/// The remarks on `measure` of `group` going from `base` to `new`: its threshold broken, and the
/// target it exceeds. `instructions_difference` is the relative difference of the instructions of
/// `group`, if it counts them.
fn remarks(
    group: &str,
    measure: &str,
    base: Option<f64>,
    new: Option<f64>,
    instructions_difference: Option<f64>,
) -> Vec<Remark> {
    let degradation = degradation(measure, base, new, instructions_difference);
    let is_open = group.split(' ').any(|word| word == OPEN_SCENARIO);
    let exceeded = TARGETS
        .iter()
        .filter(|(name, _, scenarios)| {
            *name == measure && (*scenarios == Scenarios::All || is_open)
        })
        .find(|(_, target, _)| new.is_some_and(|new| new > *target))
        .map(|(_, target, _)| {
            Remark::Warning(format!(
                "WARNING: above the target of the README ({target})"
            ))
        });
    degradation.into_iter().chain(exceeded).collect()
}

/// The remark on `measure` going from `base` to `new`, if it degrades by more than its threshold.
/// `instructions_difference` is the relative difference of the instructions of its scenario, if it
/// counts them.
fn degradation(
    measure: &str,
    base: Option<f64>,
    new: Option<f64>,
    instructions_difference: Option<f64>,
) -> Option<Remark> {
    let &(_, threshold, kind) = THRESHOLDS.iter().find(|(name, _, _)| *name == measure)?;
    let (base, new) = base.zip(new)?;
    if !difference(Some(base), Some(new)).is_some_and(|difference| difference > threshold) {
        return None;
    }
    let text = format!("degraded by more than {:.0} %", threshold * 100.0);
    let stable_instructions =
        instructions_difference.is_some_and(|difference| difference <= INSTRUCTIONS_THRESHOLD);
    Some(match kind {
        Kind::Time if new - base <= MIN_TIME_DIFFERENCE_S => Remark::Note(format!(
            "note: {text} (less than {} ms)",
            MIN_TIME_DIFFERENCE_S * 1000.0
        )),
        Kind::Time if stable_instructions => Remark::Note(format!(
            "note: {text} (instructions within {:.0} %)",
            INSTRUCTIONS_THRESHOLD * 100.0
        )),
        Kind::Time | Kind::Stable => Remark::Warning(format!("WARNING: {text}")),
    })
}

fn formatted(value: Option<f64>) -> String {
    match value {
        Some(value) if value.fract() == 0.0 => format!("{value}"),
        Some(value) => format!("{value:.3}"),
        None => "–".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn warning(text: &str) -> Remark {
        Remark::Warning(text.to_string())
    }

    fn note(text: &str) -> Remark {
        Remark::Note(text.to_string())
    }

    #[test]
    fn instructions_warn_above_1_percent() {
        assert!(
            remarks(
                "200x50 scroll",
                "instructions",
                Some(100.0),
                Some(100.5),
                None
            )
            .is_empty()
        );
        assert_eq!(
            remarks(
                "200x50 scroll",
                "instructions",
                Some(100.0),
                Some(102.0),
                None
            )
            .len(),
            1
        );
    }

    #[test]
    fn times_warn_above_5_percent() {
        // A bench, which doesn't count instructions, with differences above
        // `MIN_TIME_DIFFERENCE_S`.
        let remarks = |new| {
            remarks(
                "bench startup 80x24",
                "fastest_s",
                Some(1.0),
                Some(new),
                None,
            )
        };
        assert!(remarks(1.04).is_empty());
        assert_eq!(
            remarks(1.06),
            [warning("WARNING: degraded by more than 5 %")]
        );
    }

    #[test]
    fn times_only_get_a_note_if_they_degrade_by_at_most_5_ms() {
        // A bench, which doesn't count instructions, degraded by 100 %.
        assert_eq!(
            remarks(
                "bench frame 80x24",
                "fastest_s",
                Some(0.0004),
                Some(0.0008),
                None
            ),
            [note("note: degraded by more than 5 % (less than 5 ms)")]
        );
        assert_eq!(
            remarks(
                "bench frame 80x24",
                "fastest_s",
                Some(0.010),
                Some(0.020),
                None
            ),
            [warning("WARNING: degraded by more than 5 %")]
        );
    }

    #[test]
    fn times_only_get_a_note_if_the_instructions_degrade_by_at_most_1_percent() {
        // 60 ms more, above `MIN_TIME_DIFFERENCE_S`.
        let remarks = |instructions_difference| {
            remarks(
                "200x50 scroll",
                "cpu_time_s",
                Some(1.0),
                Some(1.06),
                Some(instructions_difference),
            )
        };
        assert_eq!(
            remarks(0.01),
            [note(
                "note: degraded by more than 5 % (instructions within 1 %)"
            )]
        );
        assert_eq!(
            remarks(0.02),
            [warning("WARNING: degraded by more than 5 %")]
        );
    }

    #[test]
    fn times_degraded_by_at_most_5_ms_only_get_a_note_even_if_the_instructions_degrade() {
        // 10 % and 0.5 ms more, with instructions degraded by 2 %.
        assert_eq!(
            remarks(
                "200x50 scroll",
                "wall_time_s",
                Some(0.005),
                Some(0.0055),
                Some(0.02)
            ),
            [note("note: degraded by more than 5 % (less than 5 ms)")]
        );
    }

    #[test]
    fn measures_that_are_not_times_warn_even_if_the_instructions_dont_degrade() {
        assert_eq!(
            remarks(
                "200x50 scroll",
                "peak_memory_kb",
                Some(100.0),
                Some(110.0),
                Some(0.0)
            ),
            [warning("WARNING: degraded by more than 5 %")]
        );
    }

    #[test]
    fn only_warnings_block() {
        assert!(warning("WARNING: degraded by more than 5 %").blocks());
        assert!(!note("note: degraded by more than 5 % (less than 5 ms)").blocks());
    }

    #[test]
    fn improvements_dont_warn() {
        assert!(
            remarks(
                "200x50 scroll",
                "instructions",
                Some(100.0),
                Some(50.0),
                None
            )
            .is_empty()
        );
    }

    #[test]
    fn missing_measures_dont_warn() {
        assert!(remarks("200x50 scroll", "instructions", None, Some(100.0), None).is_empty());
        assert_eq!(difference(None, Some(1.0)), None);
    }

    #[test]
    fn zero_base_has_no_difference() {
        assert_eq!(difference(Some(0.0), Some(1.0)), None);
        assert!(remarks("200x50 scroll", "instructions", Some(0.0), Some(1.0), None).is_empty());
    }

    #[test]
    fn targets_warn_even_without_base() {
        let remarks = remarks(
            "200x50 scroll",
            "first_full_screen_ms",
            None,
            Some(250.0),
            None,
        );
        assert_eq!(remarks.len(), 1);
    }

    #[test]
    fn time_to_open_is_only_the_wall_time_of_opening() {
        assert_eq!(
            remarks("200x50 open", "wall_time_s", None, Some(1.5), None).len(),
            1
        );
        assert!(remarks("200x50 scroll", "wall_time_s", None, Some(1.5), None).is_empty());
    }

    #[test]
    fn identity_differences_name_the_changed_parts() {
        let base = json!({"source": "a", "repository": {"head": "b", "index": "c"}, "script": "d"});
        let new = json!({"source": "e", "repository": {"head": "b", "index": "f"}, "script": "d"});
        assert_eq!(
            identity_differences(&base, &new, ""),
            ["repository.index", "source"]
        );
    }
}
