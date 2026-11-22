# Targets measuring the performance of `reedi` (see `etc/profiling.md`).

repository := "target/performance-repository"
# Built with frame pointers, without which perf loses the call stacks. They change `RUSTFLAGS`,
# so they have their own target directory, so as not to rebuild the other builds each time.
profiling_target := justfile_directory() / "target/frame-pointers"
reedi := profiling_target / "profiling/reedi"
perf_report := justfile_directory() / "target/perf.data"
perf_record := "perf record --call-graph fp --output " + perf_report
flamegraph_report := justfile_directory() / "target/flamegraph.svg"
# perf's settings while making the flamegraph, replacing those of `~/.perfconfig`.
flamegraph_perf_config := justfile_directory() / "target/flamegraph.perfconfig"
callgrind_report := justfile_directory() / "target/callgrind.out"
# heaptrack appends `.zst`, the extension of its compression, to the name it is given.
heaptrack_report := justfile_directory() / "target/heaptrack"
# The build directory of the scripts of `etc/` (cargo scripts, built with nightly), shared by them.
scripts_target := justfile_directory() / "target/scripts"

# The keys replayed by the profilers: go to the last row, then quit.
default_keys := "99999jq"

# List the targets.
[private]
default:
    @just --list

# Create the generated repository in `directory`, unless it exists.
generate directory=repository:
    test -d {{ directory }} || cargo run --example performance_repository --features test-support -- {{ directory }}

# Run the benchmarks.
bench:
    cargo bench --bench performance

# Measure the performance of `revision` (default: the working tree) and save the results.
measure revision="":
    CARGO_TARGET_DIR={{ scripts_target }} etc/measure.rs {{ revision }}

# Display the performance differences between the working tree and `base`.
compare base="HEAD":
    CARGO_TARGET_DIR={{ scripts_target }} etc/compare.rs {{ base }}

# Make git use the hooks of `etc/hooks` (the `commit-msg` hook checks the performance).
install-hooks:
    git config core.hooksPath etc/hooks

# Profile the CPU time with perf, then open Hotspot (empty `keys`: use reedi by hand).
perf keys=default_keys: check-perf-event-paranoid (run-under perf_record keys)
    hotspot {{ perf_report }}

# Profile the CPU time with perf, then open its flamegraph (empty `keys`: use reedi by hand).
flamegraph keys=default_keys: check-perf-event-paranoid (run-under perf_record keys)
    # By default, perf stops waiting for `addr2line` (which finds the inlined functions) before it
    # has read the debug information of `reedi`, then restarts it: slower, with errors.
    printf '[core]\n\taddr2line-timeout = 30000\n' > {{ flamegraph_perf_config }}
    PERF_CONFIG={{ flamegraph_perf_config }} perf script --input {{ perf_report }} | inferno-collapse-perf | inferno-flamegraph > {{ flamegraph_report }}
    xdg-open {{ flamegraph_report }}

# Count the instructions with Callgrind, then open KCachegrind (empty `keys`: by hand).
callgrind keys=default_keys: (run-under ("valgrind --tool=callgrind --callgrind-out-file=" + callgrind_report) keys)
    kcachegrind {{ callgrind_report }}

# Profile the memory with heaptrack, then open heaptrack_gui (empty `keys`: by hand).
heaptrack keys=default_keys: (run-under ("heaptrack --output " + heaptrack_report) keys)
    heaptrack_gui {{ heaptrack_report }}.zst

# Fail, saying how to allow it, if perf can't record the kernel's samples.
[private]
check-perf-event-paranoid:
    #!/usr/bin/env bash
    if [ "$(cat /proc/sys/kernel/perf_event_paranoid)" -gt 1 ]; then
        echo "perf can't record the kernel's samples: run \`sudo sysctl kernel.perf_event_paranoid=1\`." >&2
        exit 1
    fi

# Run reedi, built with the `profiling` profile and frame pointers, under `tool` on the generated
# repository, replaying `keys` (if not empty).
[private]
run-under tool keys: generate
    RUSTFLAGS="-C force-frame-pointers=yes" CARGO_TARGET_DIR={{ profiling_target }} cargo build --profile profiling
    cd {{ repository }} && {{ tool }} {{ reedi }} {{ if keys == "" { "" } else { "--send-keys " + quote(keys) } }}
