//! Benchmarks of `reedi` on the repository matching the performance target of the README (see
//! [`performance_repository`]), with their allocations count.

use std::fmt;
use std::path::PathBuf;
use std::sync::LazyLock;
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
use reedi::theme::Theme;
use reedi::view;
use reedi::view::Mode;
use reedi::view::Row;
use reedi::view::SplitRatio;
use reedi::viewport::Viewport;

#[global_allocator]
static ALLOCATOR: AllocProfiler = AllocProfiler::system();

/// The directory of the generated repository, removed at the end of [`main()`].
static REPOSITORY_PATH: OnceLock<PathBuf> = OnceLock::new();

/// The revisions of the generated repository, loaded by the first benchmark needing them.
static REVISIONS: LazyLock<Vec<Revision>> = LazyLock::new(load_revisions);

/// The widths the rows are wrapped for.
const WIDTHS: [u16; 2] = [80, 200];

/// The screens the frames are drawn on.
const SCREENS: [Screen; 2] = [
    Screen { width: 80, height: 24 },
    Screen { width: 200, height: 50 },
];

/// The size of a terminal.
#[derive(Debug, Clone, Copy)]
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
    let directory = tempfile::tempdir().unwrap();
    let git = Git::on_branch_in(Directory::Persistent(directory.path().to_path_buf()));
    performance_repository::generate(&git);
    REPOSITORY_PATH.set(directory.path().to_path_buf()).unwrap();
    divan::main();
}

fn load_revisions() -> Vec<Revision> {
    let cli = Cli::parse_from(["reedi"]);
    log::displayed_revisions(&cli, REPOSITORY_PATH.get().unwrap()).unwrap()
}

/// Width of the left half of the split rows on a screen `width` columns wide, as at startup.
fn left_width(revisions: &[Revision], width: u16) -> u16 {
    SplitRatio::default().left_width(width, view::min_half_width(revisions))
}

fn rows(revisions: &[Revision], width: u16, left_width: u16) -> Vec<Row> {
    view::rows(
        revisions,
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
            )
        })
        .unwrap();
}

/// What `reedi` computes before displaying its first frame: the loading of the revisions, their
/// rows, and the frame. Reading the settings and preparing the terminal are not included.
#[divan::bench(args = SCREENS, sample_count = 10, sample_size = 1)]
fn startup(screen: Screen) {
    let revisions = load_revisions();
    let left_width = left_width(&revisions, screen.width);
    let rows = rows(&revisions, screen.width, left_width);
    draw(&revisions, &rows, left_width, screen);
}

#[divan::bench(sample_count = 10, sample_size = 1)]
fn displayed_revisions() -> Vec<Revision> {
    load_revisions()
}

/// The rewrap, when the terminal is resized or the separator is dragged.
#[divan::bench(args = WIDTHS, sample_count = 10, sample_size = 1)]
fn rewrap(bencher: Bencher, width: u16) {
    let left_width = left_width(&REVISIONS, width);
    bencher.bench(|| rows(&REVISIONS, width, left_width));
}

#[divan::bench(args = SCREENS)]
fn frame(bencher: Bencher, screen: Screen) {
    let left_width = left_width(&REVISIONS, screen.width);
    let rows = rows(&REVISIONS, screen.width, left_width);
    bencher.bench(|| draw(&REVISIONS, &rows, left_width, screen));
}
