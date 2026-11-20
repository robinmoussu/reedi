Measuring the performance
=========================

The measurements use a generated repository matching the performance target of the README: 10
unpushed revisions of about 1000 changed lines each, always the same.

Tools
-----

On Arch Linux:

```
sudo pacman -S just perf inferno valgrind kcachegrind heaptrack
paru -S hotspot
```

`just perf` and `just flamegraph` need `sudo sysctl kernel.perf_event_paranoid=1` (until the next
reboot), and say so when it is missing.

Measuring: the benchmarks
-------------------------

`just bench` runs the benchmarks of `benches/performance.rs`, which print the duration and the
allocations of:

- `startup`: what `reedi` computes before its first frame (loading the revisions, wrapping their
  rows, drawing the frame), at 80×24 and 200×50. Reading the settings and preparing the terminal
  are not included.
- `displayed_revisions`: loading the revisions.
- `rewrap`: wrapping the rows again (resizing, dragging the separator), at 80 and 200 columns.
- `frame`: drawing a frame (scrolling), at 80×24 and 200×50. Above 16 ms, scrolling is not fluid.

`cargo bench --bench performance -- frame` runs only the benchmarks whose name contains `frame`.

Investigating: the profilers
----------------------------

Each target builds `reedi` with the `profiling` cargo profile (optimized, with symbols) and frame
pointers (in `target/frame-pointers`), creates the generated repository in
`target/performance-repository` if it is missing, runs `reedi` on it under the profiler, then opens
the report (written in `target/`) in its viewer.

By default, `reedi` replays the keys `99999jq` (to the last row, then quit), so that two runs do
the same thing. Other keys are written in Vim's key notation: `just perf '<C-d><C-d>q'`. With
`just perf ''`, you use `reedi` yourself and quit with `q`.

| Target | Measures | Viewer | Use it to |
|---|---|---|---|
| `just perf` | CPU time, sampled | Hotspot | find which functions take the time |
| `just flamegraph` | the same | a flamegraph (SVG) in the browser | see at a glance where the time goes |
| `just callgrind` | instructions executed, exactly (tens of times slower) | KCachegrind | compare two versions: the counts don't vary between runs |
| `just heaptrack` | allocations and memory | `heaptrack_gui` | find what allocates, and the peak memory |

`just flamegraph` takes one to two minutes to find the functions inlined by the compiler.

`perf` only samples while `reedi` runs on a CPU: waiting (for the terminal, for instance) is
invisible. Hotspot can record it too, with its off-CPU profiling option.

`just generate <directory>` creates the generated repository elsewhere, to run `reedi` on it by
hand.
