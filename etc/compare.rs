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
//! exceeded, in which case the exit status is 1.

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

/// The degradation of a measure above which a warning is printed, as a fraction of its base value.
const THRESHOLDS: [(&str, f64); 5] = [
    ("instructions", 0.01),
    ("cpu_time_s", 0.05),
    ("wall_time_s", 0.05),
    ("peak_memory_kb", 0.05),
    // The median time of a bench, a wall time.
    ("median_s", 0.05),
];

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
        for (measure, new_value) in new_measures.as_object().unwrap_or(&empty) {
            let base_value = base_measures.and_then(|measures| measures[measure].as_f64());
            let new_value = new_value.as_f64();
            let warnings = warnings(group, measure, base_value, new_value);
            broken |= !warnings.is_empty();
            println!(
                "{measure:<24} {:>14} {:>14} {:>9}  {}",
                formatted(base_value),
                formatted(new_value),
                difference(base_value, new_value).map_or("–".to_string(), |difference| format!(
                    "{:+.1} %",
                    difference * 100.0
                )),
                warnings.join(", ")
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

/// The warnings of `measure` of `group` going from `base` to `new`: its threshold broken, and the
/// target it exceeds.
fn warnings(group: &str, measure: &str, base: Option<f64>, new: Option<f64>) -> Vec<String> {
    let threshold = THRESHOLDS
        .iter()
        .find(|(name, _)| *name == measure)
        .map(|(_, threshold)| *threshold);
    let degradation = match (threshold, difference(base, new)) {
        (Some(threshold), Some(difference)) if difference > threshold => Some(format!(
            "WARNING: degraded by more than {:.0} %",
            threshold * 100.0
        )),
        _ => None,
    };
    let is_open = group.split(' ').any(|word| word == OPEN_SCENARIO);
    let exceeded = TARGETS
        .iter()
        .filter(|(name, _, scenarios)| {
            *name == measure && (*scenarios == Scenarios::All || is_open)
        })
        .find(|(_, target, _)| new.is_some_and(|new| new > *target))
        .map(|(_, target, _)| format!("WARNING: above the target of the README ({target})"));
    degradation.into_iter().chain(exceeded).collect()
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

    #[test]
    fn instructions_warn_above_1_percent() {
        assert!(warnings("200x50 scroll", "instructions", Some(100.0), Some(100.5)).is_empty());
        assert_eq!(
            warnings("200x50 scroll", "instructions", Some(100.0), Some(102.0)).len(),
            1
        );
    }

    #[test]
    fn times_warn_above_5_percent() {
        assert!(warnings("200x50 scroll", "cpu_time_s", Some(1.0), Some(1.04)).is_empty());
        assert_eq!(
            warnings("200x50 scroll", "cpu_time_s", Some(1.0), Some(1.06)).len(),
            1
        );
    }

    #[test]
    fn improvements_dont_warn() {
        assert!(warnings("200x50 scroll", "instructions", Some(100.0), Some(50.0)).is_empty());
    }

    #[test]
    fn missing_measures_dont_warn() {
        assert!(warnings("200x50 scroll", "instructions", None, Some(100.0)).is_empty());
        assert_eq!(difference(None, Some(1.0)), None);
    }

    #[test]
    fn zero_base_has_no_difference() {
        assert_eq!(difference(Some(0.0), Some(1.0)), None);
        assert!(warnings("200x50 scroll", "instructions", Some(0.0), Some(1.0)).is_empty());
    }

    #[test]
    fn targets_warn_even_without_base() {
        let warnings = warnings("200x50 scroll", "first_full_screen_ms", None, Some(250.0));
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn time_to_open_is_only_the_wall_time_of_opening() {
        assert_eq!(
            warnings("200x50 open", "wall_time_s", None, Some(1.5)).len(),
            1
        );
        assert!(warnings("200x50 scroll", "wall_time_s", None, Some(1.5)).is_empty());
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
