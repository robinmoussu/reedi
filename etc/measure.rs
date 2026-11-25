#!/usr/bin/env -S cargo +nightly -Zscript --quiet
---
[package]
edition = "2024"

[dependencies]
clap = { version = "4.6.7", features = ["derive"] }
reedi = { path = "..", features = ["test-support"] }
serde_json = "1.0.154"
tempfile = "3.27.0"
---
//! `etc/measure.rs [--generate | --result-files | --no-generate] [revision]`: measure the performance of
//! `revision` (default: the working tree) on the generated repositories, and save the results of
//! each repository in `target/performance/<key>.json` (see `etc/profiling.md`). Changing this
//! file changes the identity of the results.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::time::Instant;

use clap::Parser;
use reedi::test_git::Directory;
use reedi::test_git::Git;
use reedi::test_support::performance_repository;
use reedi::test_support::performance_repository::Uncommitted;
use serde_json::Map;
use serde_json::Value;
use serde_json::json;

/// The directory of the results, the builds and the repositories, relative to the working tree.
const PERFORMANCE_DIRECTORY: &str = "target/performance";
/// The target directory of `just perf`: same profile and `RUSTFLAGS`, so that its cache is shared.
const PROFILING_TARGET_DIRECTORY: &str = "target/frame-pointers";
const PROFILING_RUSTFLAGS: &str = "-C force-frame-pointers=yes";
/// The repositories measured: without uncommitted changes, and with both staged and unstaged
/// changes.
const MEASURED_REPOSITORIES: [Uncommitted; 2] = [Uncommitted::None, Uncommitted::Both];
/// The width and height of the terminals.
const TERMINALS: [(u16, u16); 2] = [(200, 50), (48, 24)];
/// The number of runs of each measure, of which the median is kept.
const RUNS_COUNT: usize = 5;
/// The number of half screens the screen scrolls after the cursor went down the whole screen, one
/// `j` per row, each of them drawn: 2.5 screens.
const SCROLLED_HALF_SCREENS_COUNT: usize = 5;
/// The file whose hash is part of the identity of the results.
const MEASUREMENT_SOURCE: &str = "etc/measure.rs";

#[derive(Debug, Parser)]
struct Arguments {
    /// Only generate the repositories again.
    #[arg(long, conflicts_with_all = ["result_files", "no_generate"])]
    generate: bool,
    /// Only print the path of the result file of each repository, without generating them.
    #[arg(long, conflicts_with = "no_generate")]
    result_files: bool,
    /// Measure on the repositories as they are, without generating them again.
    #[arg(long)]
    no_generate: bool,
    /// The revision to measure (any tree-ish), the working tree if absent.
    revision: Option<String>,
}

fn main() {
    let arguments = Arguments::parse();
    let root = PathBuf::from(output(
        Command::new("git").args(["rev-parse", "--show-toplevel"]),
    ));
    std::env::set_current_dir(&root).unwrap();
    let performance = root.join(PERFORMANCE_DIRECTORY);
    if !arguments.result_files && !arguments.no_generate {
        generate_repositories(&performance);
    }
    if arguments.generate {
        return;
    }
    let revision = arguments.revision.as_deref();
    let tree = source_tree(revision);
    if arguments.result_files {
        for uncommitted in MEASURED_REPOSITORIES {
            let repository = repository_path(&performance, uncommitted);
            println!(
                "{}",
                result_file(&performance, &identity(&tree, &repository)).display()
            );
        }
        return;
    }
    let sources = sources(&performance, &tree, revision);
    let binary = binary(&performance, &tree, &sources);
    let config = performance.join("config");
    fs::create_dir_all(config.join("reedi")).unwrap();
    // Without a colorscheme, reedi asks the terminal for its background color, and `script`
    // never answers.
    fs::write(config.join("reedi/config.toml"), "colorscheme = \"dark\"\n").unwrap();
    for uncommitted in MEASURED_REPOSITORIES {
        let repository = repository_path(&performance, uncommitted);
        let identity = identity(&tree, &repository);
        let mut groups = Map::new();
        for (width, height) in TERMINALS {
            eprintln!("measuring {}, {width}x{height}", uncommitted.name());
            let runs = (0..RUNS_COUNT)
                .map(|_| run(&binary, &repository, &config, (width, height)))
                .collect::<Vec<_>>();
            groups.insert(format!("{width}x{height} scroll"), medians(&runs));
        }
        eprintln!("benchmarking {}", uncommitted.name());
        groups.extend(benches(&sources, &repository));
        let results = json!({
            "identity": identity,
            "repository": uncommitted.name(),
            "groups": groups,
        });
        let path = result_file(&performance, &identity);
        fs::write(&path, serde_json::to_string_pretty(&results).unwrap()).unwrap();
        println!("{}", path.display());
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

fn git(args: &[&str]) -> String {
    output(Command::new("git").args(args))
}

fn repository_path(performance: &Path, uncommitted: Uncommitted) -> PathBuf {
    performance.join("repositories").join(uncommitted.name())
}

/// Generate the measured repositories again, with the generator of the working tree.
fn generate_repositories(performance: &Path) {
    for uncommitted in MEASURED_REPOSITORIES {
        let path = repository_path(performance, uncommitted);
        if path.exists() {
            fs::remove_dir_all(&path).unwrap();
        }
        let git = Git::on_branch_in(Directory::Persistent(path));
        performance_repository::generate(&git, uncommitted);
    }
}

/// The tree of the files of the index of the repository `directory`, with the changes of its
/// working tree: those of the tracked files only if `untracked` is `Untracked::Excluded`.
fn working_tree(directory: &Path, untracked: Untracked) -> String {
    let git_directory = output(
        Command::new("git")
            .current_dir(directory)
            .args(["rev-parse", "--absolute-git-dir"]),
    );
    let index = tempfile::NamedTempFile::new().unwrap();
    fs::copy(Path::new(&git_directory).join("index"), index.path()).unwrap();
    let add = match untracked {
        Untracked::Excluded => "--update",
        Untracked::Included => "--all",
    };
    let git_with_index = |args: &[&str]| {
        output(
            Command::new("git")
                .current_dir(directory)
                .env("GIT_INDEX_FILE", index.path())
                .args(args),
        )
    };
    git_with_index(&["add", add]);
    git_with_index(&["write-tree"])
}

/// Whether [`working_tree()`] includes the untracked files.
#[derive(Debug, Clone, Copy)]
enum Untracked {
    Excluded,
    Included,
}

/// The tree of `revision`, or of the tracked files of the working tree.
fn source_tree(revision: Option<&str>) -> String {
    match revision {
        Some(revision) => git(&["rev-parse", &format!("{revision}^{{tree}}")]),
        None => working_tree(Path::new("."), Untracked::Excluded),
    }
}

/// The identity of the repository at `path`: everything that changes what `reedi` displays.
fn repository_identity(path: &Path) -> Value {
    // The index of the repository being committed to, set by git for its hooks, is ignored.
    let git_in = |args: &[&str]| {
        output(
            Command::new("git")
                .current_dir(path)
                .env_remove("GIT_INDEX_FILE")
                .args(args),
        )
    };
    let config = git_in(&["config", "--list", "--local"]);
    let config_hash = output(
        Command::new("sh")
            .current_dir(path)
            .args(["-c", "git config --list --local | git hash-object --stdin"]),
    );
    debug_assert!(!config.is_empty());
    json!({
        "head": git_in(&["rev-parse", "HEAD"]),
        "upstream": git_in(&["rev-parse", "@{upstream}"]),
        "index": git_in(&["write-tree"]),
        "working_directory": working_tree(path, Untracked::Included),
        "config": config_hash,
    })
}

/// The identity of the results of the source `tree` on the repository at `repository`.
fn identity(tree: &str, repository: &Path) -> Value {
    json!({
        "source": tree,
        "repository": repository_identity(repository),
        "script": git(&["hash-object", MEASUREMENT_SOURCE]),
    })
}

/// The file of the results of `identity`, named by the hash of its JSON.
fn result_file(performance: &Path, identity: &Value) -> PathBuf {
    let mut child = Command::new("git")
        .args(["hash-object", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    serde_json::to_writer(child.stdin.take().unwrap(), identity).unwrap();
    let output = child.wait_with_output().unwrap();
    let key = String::from_utf8(output.stdout).unwrap();
    performance.join(format!("{}.json", key.trim_end()))
}

/// The directory of the sources of `tree`: the working tree, or an export of `revision` (with the
/// `modalkit` submodule at its recorded commit), so that nothing is stashed.
fn sources(performance: &Path, tree: &str, revision: Option<&str>) -> PathBuf {
    let Some(revision) = revision else {
        return PathBuf::from(".").canonicalize().unwrap();
    };
    let directory = performance.join("src").join(tree);
    if !directory.exists() {
        fs::create_dir_all(&directory).unwrap();
        // The spaces ending file names are removed: cargo misreads such paths in its dep-info
        // files, and would build the sources again each time. The files get the current time
        // (`-m`) rather than the one of the commit: the revisions share their build artifacts,
        // which cargo would otherwise reuse for a revision older than the last build.
        let export = |repository: &str, commit: &str, destination: &Path| {
            let command = format!(
                "git -C {repository} archive {commit} | tar -x -m --transform='s/ $//' -C {}",
                destination.display()
            );
            output(Command::new("sh").args(["-c", &command]));
        };
        export(".", revision, &directory);
        let modalkit = Command::new("git")
            .args([
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{revision}:modalkit"),
            ])
            .output()
            .unwrap();
        if modalkit.status.success() {
            let commit = String::from_utf8(modalkit.stdout).unwrap();
            export("modalkit", commit.trim(), &directory.join("modalkit"));
        }
    }
    directory
}

/// The `reedi` binary of `tree`, built from `sources` unless it is kept already.
fn binary(performance: &Path, tree: &str, sources: &Path) -> PathBuf {
    let binary = performance.join("bin").join(tree);
    if !binary.exists() {
        output(
            Command::new("cargo")
                .args(["build", "--profile", "profiling", "--manifest-path"])
                .arg(sources.join("Cargo.toml"))
                .env("RUSTFLAGS", PROFILING_RUSTFLAGS)
                .env("CARGO_TARGET_DIR", PROFILING_TARGET_DIRECTORY)
                .stdout(Stdio::inherit()),
        );
        fs::create_dir_all(binary.parent().unwrap()).unwrap();
        fs::copy(
            Path::new(PROFILING_TARGET_DIRECTORY).join("profiling/reedi"),
            &binary,
        )
        .unwrap();
    }
    binary.canonicalize().unwrap()
}

/// The measures of a run of `binary` in `repository`, on a terminal of size `(width, height)`,
/// moving the cursor down the screen, scrolling [`SCROLLED_HALF_SCREENS_COUNT`] half screens, then
/// quitting.
fn run(
    binary: &Path,
    repository: &Path,
    config: &Path,
    (width, height): (u16, u16),
) -> BTreeMap<&'static str, Option<f64>> {
    let [stat, memory, timing] = [(); 3].map(|()| tempfile::NamedTempFile::new().unwrap());
    let height = usize::from(height);
    let keys = format!(
        "{}q",
        "j".repeat(height + height * SCROLLED_HALF_SCREENS_COUNT / 2)
    );
    let command = format!(
        "stty cols {width} rows {height}; perf stat -x, -e instructions:u,task-clock -o {} -- \
         /usr/bin/time -f %M -o {} {} --send-keys {keys} --draw-every-key --timing {}",
        stat.path().display(),
        memory.path().display(),
        binary.display(),
        timing.path().display()
    );
    let start = Instant::now();
    let status = Command::new("script")
        .args(["--quiet", "--return", "--command", &command, "/dev/null"])
        .current_dir(repository)
        // The index of the repository being committed to, set by git for its hooks, is ignored.
        .env_remove("GIT_INDEX_FILE")
        .env("XDG_CONFIG_HOME", config)
        // So that `perf` writes its numbers with a decimal point, not with a comma like the
        // separator of its fields.
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .status()
        .unwrap();
    let wall_time = start.elapsed();
    assert!(status.success(), "{command} failed");
    let stat = fs::read_to_string(stat.path()).unwrap();
    let counter = |event: &str| {
        stat.lines().find_map(|line| {
            let fields = line.split(',').collect::<Vec<_>>();
            let is_event = fields.get(2).is_some_and(|name| name.starts_with(event));
            if is_event {
                fields[0].parse::<f64>().ok()
            } else {
                None
            }
        })
    };
    let memory = fs::read_to_string(memory.path()).unwrap();
    let timing: Value = serde_json::from_str(&fs::read_to_string(timing.path()).unwrap()).unwrap();
    let longest_refresh = timing["refresh_ms"]
        .as_array()
        .and_then(|refreshes| refreshes.iter().filter_map(Value::as_f64).reduce(f64::max));
    BTreeMap::from([
        ("instructions", counter("instructions")),
        (
            "cpu_time_s",
            counter("task-clock").map(|milliseconds| milliseconds / 1000.0),
        ),
        ("wall_time_s", Some(wall_time.as_secs_f64())),
        (
            "peak_memory_kb",
            memory
                .lines()
                .last()
                .and_then(|line| line.trim().parse().ok()),
        ),
        (
            "first_full_screen_ms",
            timing["first_full_screen_ms"].as_f64(),
        ),
        ("longest_refresh_ms", longest_refresh),
    ])
}

/// The median of each measure of `runs`, ignoring the runs without it.
fn medians(runs: &[BTreeMap<&'static str, Option<f64>>]) -> Value {
    let medians = runs[0]
        .keys()
        .map(|measure| {
            let mut values = runs
                .iter()
                .filter_map(|run| run[measure])
                .collect::<Vec<_>>();
            values.sort_by(f64::total_cmp);
            let median = values.get(values.len().saturating_sub(1) / 2).copied();
            (measure.to_string(), json!(median))
        })
        .collect::<Map<_, _>>();
    Value::Object(medians)
}

/// The groups of measures of the benches of `sources` on `repository`.
fn benches(sources: &Path, repository: &Path) -> Map<String, Value> {
    let table = output(
        Command::new("cargo")
            .args([
                "bench",
                "--quiet",
                "--bench",
                "performance",
                "--manifest-path",
            ])
            .arg(sources.join("Cargo.toml"))
            .env("REEDI_PERFORMANCE_REPOSITORY", repository)
            .env("RUSTFLAGS", PROFILING_RUSTFLAGS)
            .env("CARGO_TARGET_DIR", PROFILING_TARGET_DIRECTORY),
    );
    parse_benches(&table).unwrap_or_else(|error| panic!("{error}"))
}

/// The fastest time, median allocations count and median allocated bytes of each bench of the
/// `table` divan prints, and the instructions of the `bench <bench> instructions <count>` lines the
/// benches print after it. The fastest time varies less between runs than the median: the other
/// processes only make a sample slower. divan has no machine-readable output: fails if the columns
/// of the table are not the expected ones, rather than returning wrong numbers.
fn parse_benches(table: &str) -> Result<Map<String, Value>, String> {
    let mut lines = table.lines().skip_while(|line| !line.contains("fastest"));
    let header = lines.next().ok_or("no divan table")?;
    let columns = header.split('│').map(str::trim).collect::<Vec<_>>();
    if columns.get(1..) != Some(&["slowest", "median", "mean", "samples", "iters"][..])
        || !columns[0].ends_with("fastest")
    {
        return Err(format!("unexpected divan table: {header}"));
    }
    let mut benches = Map::new();
    let mut parent = "";
    let mut bench = String::new();
    let mut expected = Expected::Row;
    for line in lines {
        if let Some((bench, instructions)) = line.rsplit_once(" instructions ") {
            let instructions = instructions.parse::<f64>().map_err(|_| line)?;
            benches.get_mut(bench).ok_or(line)?["instructions"] = json!(instructions);
            continue;
        }
        let tree_drawn = line.trim_start_matches(['│', ' ']);
        let is_row = tree_drawn.starts_with(['├', '╰']);
        let is_nested = is_row && tree_drawn.len() != line.len();
        let columns = line
            .trim_start_matches(['│', '├', '╰', '─', ' '])
            .split('│')
            .map(str::trim)
            .collect::<Vec<_>>();
        let (first, median) = (columns[0], columns.get(2).copied().unwrap_or(""));
        match expected {
            _ if first == "alloc:" => expected = Expected::AllocationsCount,
            Expected::AllocationsCount => {
                benches[&bench]["allocations"] = json!(median.parse::<f64>().map_err(|_| line)?);
                expected = Expected::AllocatedBytes;
            }
            Expected::AllocatedBytes => {
                benches[&bench]["allocated_bytes"] = json!(quantity(median, BYTES).ok_or(line)?);
                expected = Expected::Row;
            }
            Expected::Row if is_row => {
                let (name, fastest) = first.split_once(' ').unwrap_or((first, ""));
                if median.is_empty() {
                    parent = name;
                    continue;
                }
                bench = if is_nested {
                    format!("bench {parent} {name}")
                } else {
                    format!("bench {name}")
                };
                let fastest_time = quantity(fastest.trim(), SECONDS).ok_or(line)?;
                benches.insert(bench.clone(), json!({"fastest_s": fastest_time}));
            }
            Expected::Row => {}
        }
    }
    if benches.is_empty() {
        return Err("no bench in the divan table".to_string());
    }
    Ok(benches)
}

/// The next line [`parse_benches()`] expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expected {
    /// A bench, or anything else to skip.
    Row,
    /// The line after `alloc:`.
    AllocationsCount,
    /// The line after the allocations count.
    AllocatedBytes,
}

/// The units of the durations divan prints, with their power of ten in seconds.
const SECONDS: &[(&str, i32)] = &[("s", 0), ("ms", -3), ("µs", -6), ("ns", -9)];
/// The units of the sizes divan prints, with their power of ten in bytes.
const BYTES: &[(&str, i32)] = &[("B", 0), ("KB", 3), ("MB", 6), ("GB", 9)];

/// The value of `text` (a number and one of `units`), in the base unit of `units`.
fn quantity(text: &str, units: &[(&str, i32)]) -> Option<f64> {
    let (number, unit) = text.split_once(' ')?;
    let (_, exponent) = units.iter().find(|(name, _)| *name == unit)?;
    let number = number.parse::<f64>().ok()?;
    // Divided rather than multiplied by a negative power, which isn't exact (85.28 ms would not
    // be 0.08528 s).
    if *exponent < 0 {
        Some(number / 10f64.powi(-exponent))
    } else {
        Some(number * 10f64.powi(*exponent))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A table of divan with a top-level bench, a group of benches, and a group of benches on
    /// the last branch of the tree.
    const TABLE: &str = "\
Timer precision: 32 ns
performance             fastest       │ slowest       │ median        │ mean          │ samples │ iters
├─ displayed_revisions  81.92 ms      │ 294.8 ms      │ 85.28 ms      │ 107.7 ms      │ 10      │ 10
│                       max alloc:    │               │               │               │         │
│                         14070       │ 14073         │ 14070         │ 14070         │         │
│                         1.758 MB    │ 1.759 MB      │ 1.758 MB      │ 1.758 MB      │         │
│                       alloc:        │               │               │               │         │
│                         21653       │ 21656         │ 21653         │ 21653         │         │
│                         31.05 MB    │ 31.05 MB      │ 31.05 MB      │ 31.05 MB      │         │
├─ frame                              │               │               │               │         │
│  ├─ 80x24             354.1 µs      │ 2.04 ms       │ 573.1 µs      │ 541.7 µs      │ 100     │ 100
│  │                    alloc:        │               │               │               │         │
│  │                      53          │ 53            │ 53            │ 54.53         │         │
│  │                      277.3 KB    │ 277.3 KB      │ 277.3 KB      │ 277.6 KB      │         │
╰─ startup                            │               │               │               │         │
   ╰─ 200x50            2.373 s       │ 2.697 s       │ 2.497 s       │ 2.5 s         │ 10      │ 10
                        alloc:        │               │               │               │         │
                          4952692     │ 4952692       │ 4952692       │ 4952692       │         │
                          847.4 MB    │ 847.4 MB      │ 847.4 MB      │ 847.4 MB      │         │
";

    #[test]
    fn bench_times_are_the_fastest_and_allocations_the_median() {
        let benches = parse_benches(TABLE).unwrap();
        assert_eq!(
            Value::Object(benches),
            json!({
                "bench displayed_revisions":
                    {"fastest_s": 0.08192, "allocations": 21653.0, "allocated_bytes": 31.05e6},
                // Divided, like `quantity()` does.
                "bench frame 80x24":
                    {"fastest_s": 354.1 / 1e6, "allocations": 53.0, "allocated_bytes": 277.3e3},
                "bench startup 200x50":
                    {"fastest_s": 2.373, "allocations": 4952692.0, "allocated_bytes": 847.4e6},
            })
        );
    }

    /// The lines the benches print after `TABLE`, for 2 of its 3 benches.
    const INSTRUCTIONS: &str = "\
bench displayed_revisions instructions 1000
bench frame 80x24 instructions 20
";

    #[test]
    fn instructions_are_read_from_the_lines_after_the_table() {
        let benches = parse_benches(&format!("{TABLE}{INSTRUCTIONS}")).unwrap();
        assert_eq!(
            benches["bench displayed_revisions"]["instructions"],
            json!(1000.0)
        );
        assert_eq!(benches["bench frame 80x24"]["instructions"], json!(20.0));
        assert_eq!(benches["bench startup 200x50"].get("instructions"), None);
    }

    #[test]
    fn instructions_of_a_bench_missing_from_the_table_are_an_error() {
        assert!(parse_benches(&format!("{TABLE}bench rewrap 80 instructions 10\n")).is_err());
    }

    #[test]
    fn unexpected_divan_table_is_an_error() {
        let table = TABLE.replace("│ median", "│ middle");
        assert!(
            parse_benches(&table)
                .unwrap_err()
                .starts_with("unexpected divan table")
        );
    }
}
