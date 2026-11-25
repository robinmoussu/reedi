//! Benchmarks of `reedi` on the repository matching the performance target of the README (see
//! [`performance_repository`]), with their allocations count. After the table of divan, the fewest
//! instructions executed by a call of each bench are printed, one `bench <bench> instructions
//! <count>` line per bench, if the system allows counting them.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::OnceLock;

use clap::Parser;
use divan::AllocProfiler;
use divan::Bencher;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use reedi::cli::Cli;
use reedi::date::DateDisplay;
use reedi::log;
use reedi::model::Revision;
use reedi::test_git::Directory;
use reedi::test_git::Git;
use reedi::test_support::NOT_CANCELLED;
use reedi::test_support::performance_repository;
use reedi::test_support::performance_repository::Uncommitted;
use reedi::theme::Theme;
use reedi::view;
use reedi::view::Mode;
use reedi::view::PairsCache;
use reedi::view::Row;
use reedi::view::SplitRatio;
use reedi::view::StickyFileNameCache;
use reedi::viewport::Viewport;

#[global_allocator]
static ALLOCATOR: AllocProfiler = AllocProfiler::system();

/// The directory of the benchmarked repository: `$REEDI_PERFORMANCE_REPOSITORY`, or else a
/// generated repository, removed at the end of [`main()`].
static REPOSITORY_PATH: OnceLock<PathBuf> = OnceLock::new();

/// The revisions of the generated repository, loaded by the first benchmark needing them.
static REVISIONS: LazyLock<Vec<Revision>> = LazyLock::new(load_revisions);

/// The fewest instructions executed by a call of each bench.
static INSTRUCTIONS: Mutex<BTreeMap<Bench, u64>> = Mutex::new(BTreeMap::new());

#[cfg(target_os = "linux")]
thread_local! {
    /// The counter of the instructions executed in user space by the thread and by the threads it
    /// starts, if the system allows counting them.
    static COUNTER: Option<std::cell::RefCell<perf_event::Counter>> = {
        let counter = perf_event::Builder::new(perf_event::events::Hardware::INSTRUCTIONS)
            .exclude_kernel(true)
            .exclude_hv(true)
            .inherit(true)
            .enabled(true)
            .build();
        match counter {
            Ok(counter) => Some(std::cell::RefCell::new(counter)),
            Err(error) => {
                eprintln!("The instructions are not counted: {error}");
                None
            }
        }
    };
}

/// A bench, with its argument, named as in the table of divan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Bench {
    Startup(Screen),
    DisplayedRevisions,
    Rewrap(u16),
    Frame(Screen),
}

impl fmt::Display for Bench {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Bench::Startup(screen) => write!(f, "startup {screen}"),
            Bench::DisplayedRevisions => write!(f, "displayed_revisions"),
            Bench::Rewrap(width) => write!(f, "rewrap {width}"),
            Bench::Frame(screen) => write!(f, "frame {screen}"),
        }
    }
}

/// The widths the rows are wrapped for.
const WIDTHS: [u16; 2] = [80, 200];

/// The screens the frames are drawn on.
const SCREENS: [Screen; 2] = [
    Screen { width: 80, height: 24 },
    Screen { width: 200, height: 50 },
];

/// The size of a terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Screen {
    width: u16,
    height: u16,
}

impl fmt::Display for Screen {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

fn main() {
    let generated_directory;
    let path = match std::env::var_os("REEDI_PERFORMANCE_REPOSITORY") {
        Some(path) => PathBuf::from(path),
        None => {
            generated_directory = tempfile::tempdir().unwrap();
            let path = generated_directory.path().to_path_buf();
            let git = Git::on_branch_in(Directory::Persistent(path.clone()));
            performance_repository::generate(&git, Uncommitted::None);
            path
        }
    };
    REPOSITORY_PATH.set(path).unwrap();
    divan::main();
    for (bench, instructions) in INSTRUCTIONS.lock().unwrap().iter() {
        println!("bench {bench} instructions {instructions}");
    }
}

/// Call `measured`, the code measured by `bench`, keeping the instructions it executes (with those
/// of the threads it starts) if they are fewer than those of the previous calls.
fn counted<T>(bench: Bench, measured: impl FnOnce() -> T) -> T {
    let (result, instructions) = instructions_of(measured);
    if let Some(instructions) = instructions {
        INSTRUCTIONS
            .lock()
            .unwrap()
            .entry(bench)
            .and_modify(|fewest| *fewest = (*fewest).min(instructions))
            .or_insert(instructions);
    }
    result
}

/// Call `measured`, and return its result with the instructions it executes (with those of the
/// threads it starts), if the system allows counting them.
#[cfg(target_os = "linux")]
fn instructions_of<T>(measured: impl FnOnce() -> T) -> (T, Option<u64>) {
    COUNTER.with(|counter| {
        let Some(counter) = counter else {
            return (measured(), None);
        };
        let before = counter.borrow_mut().read().unwrap();
        let result = measured();
        // The instructions of the threads started by `measured` are added to the counter when they
        // end, which they did before `measured` returned.
        let after = counter.borrow_mut().read().unwrap();
        (result, Some(after - before))
    })
}

#[cfg(not(target_os = "linux"))]
fn instructions_of<T>(measured: impl FnOnce() -> T) -> (T, Option<u64>) {
    (measured(), None)
}

fn load_revisions() -> Vec<Revision> {
    let cli = Cli::parse_from(["reedi"]);
    log::displayed_revisions(&cli, REPOSITORY_PATH.get().unwrap()).unwrap()
}

/// Width of the left half of the split rows on a screen `width` columns wide, as at startup.
fn left_width(revisions: &[Revision], width: u16) -> u16 {
    SplitRatio::default().left_width(width, view::min_half_width(revisions))
}

/// The rows of `revisions`, with the pairs of `pairs_cache`.
fn rows(revisions: &[Revision], pairs_cache: &PairsCache, width: u16, left_width: u16) -> Vec<Row> {
    view::rows(
        revisions,
        pairs_cache,
        width,
        left_width,
        &Theme::dark(),
        &DateDisplay::for_snapshots(),
        &NOT_CANCELLED,
    )
    .unwrap()
}

/// Draw the first frame displaying `rows`, whose split rows have a left half `left_width` columns
/// wide, on a new terminal of the size of `screen`.
fn draw(revisions: &[Revision], rows: &[Row], left_width: u16, screen: Screen) {
    let mut terminal = Terminal::new(TestBackend::new(screen.width, screen.height)).unwrap();
    terminal
        .draw(|frame| {
            view::draw(
                frame,
                revisions,
                rows,
                &mut Viewport::default(),
                left_width,
                Mode::Review,
                /* pending_count */ None,
                /* error_modal */ None,
                &Theme::dark(),
                &mut StickyFileNameCache::default(),
            )
        })
        .unwrap();
}

/// What `reedi` computes before displaying its first frame: the loading of the revisions, their
/// rows, and the frame. Reading the settings and preparing the terminal are not included.
#[divan::bench(args = SCREENS, sample_count = 10, sample_size = 1)]
fn startup(screen: Screen) {
    counted(Bench::Startup(screen), || {
        let revisions = load_revisions();
        let left_width = left_width(&revisions, screen.width);
        let rows = rows(
            &revisions,
            &PairsCache::new(&revisions),
            screen.width,
            left_width,
        );
        draw(&revisions, &rows, left_width, screen);
    });
}

#[divan::bench(sample_count = 10, sample_size = 1)]
fn displayed_revisions() -> Vec<Revision> {
    counted(Bench::DisplayedRevisions, load_revisions)
}

/// The rewrap, when the terminal is resized or the separator is dragged: the pairs are those of the
/// first wrap.
#[divan::bench(args = WIDTHS, sample_count = 10, sample_size = 1)]
fn rewrap(bencher: Bencher, width: u16) {
    let left_width = left_width(&REVISIONS, width);
    let pairs_cache = PairsCache::new(&REVISIONS);
    rows(&REVISIONS, &pairs_cache, width, left_width);
    bencher.bench(|| {
        counted(Bench::Rewrap(width), || {
            rows(&REVISIONS, &pairs_cache, width, left_width)
        })
    });
}

#[divan::bench(args = SCREENS)]
fn frame(bencher: Bencher, screen: Screen) {
    let left_width = left_width(&REVISIONS, screen.width);
    let rows = rows(
        &REVISIONS,
        &PairsCache::new(&REVISIONS),
        screen.width,
        left_width,
    );
    bencher.bench(|| {
        counted(Bench::Frame(screen), || {
            draw(&REVISIONS, &rows, left_width, screen)
        })
    });
}
