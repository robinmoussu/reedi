#!/usr/bin/env -S cargo +nightly -Zscript --quiet
---
[package]
edition = "2024"
---
//! `etc/check.rs <base> <tree>`: compare the performance of the tree `tree` with `base`, with the
//! results saved by `etc/measure.rs`, on the repositories as they are, without measuring anything
//! (for the `commit-msg` hook, see `etc/profiling.md`). The exit status is 1 if a threshold or a
//! target is broken, and 2 if the results of a side aren't saved.

use std::path::PathBuf;
use std::process::Command;
use std::process::ExitCode;

/// The scripts measuring and comparing, relative to the working tree.
const MEASURE_SCRIPT: &str = "etc/measure.rs";
const COMPARE_SCRIPT: &str = "etc/compare.rs";
/// The exit status when the results of a side aren't saved.
const NOT_MEASURED: u8 = 2;

fn main() -> ExitCode {
    let [base, tree] = match std::env::args().skip(1).collect::<Vec<_>>().try_into() {
        Ok(arguments) => arguments,
        Err(_) => {
            eprintln!("usage: etc/check.rs <base> <tree>");
            return ExitCode::FAILURE;
        }
    };
    let root = output(Command::new("git").args(["rev-parse", "--show-toplevel"]));
    std::env::set_current_dir(root).unwrap();
    let (base_files, new_files) = (result_files(&base), result_files(&tree));
    if !base_files
        .iter()
        .chain(&new_files)
        .all(|file| file.exists())
    {
        eprintln!(
            "The performance of this commit or of its parent isn't measured: run `just compare` first."
        );
        return ExitCode::from(NOT_MEASURED);
    }
    let pairs = base_files
        .into_iter()
        .zip(new_files)
        .flat_map(|(base, new)| [base, new]);
    let status = Command::new(COMPARE_SCRIPT)
        .arg("--results")
        .args(pairs)
        .status()
        .unwrap();
    if status.success() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The stdout of `command`, without its final newline. Panics if it fails.
fn output(command: &mut Command) -> String {
    let output = command.output().unwrap();
    assert!(output.status.success(), "{command:?} failed");
    String::from_utf8(output.stdout)
        .unwrap()
        .trim_end()
        .to_string()
}

/// The result files of `revision`, one per measured repository.
fn result_files(revision: &str) -> Vec<PathBuf> {
    output(Command::new(MEASURE_SCRIPT).args(["--result-files", revision]))
        .lines()
        .map(PathBuf::from)
        .collect()
}
