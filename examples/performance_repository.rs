//! Generate the repository of the performance measurements (see
//! [`reedi::test_support::performance_repository`]) in the directory given as first argument,
//! which must not exist. The second argument, if any, names its uncommitted changes (see
//! [`Uncommitted`]).

use std::path::PathBuf;
use std::process::ExitCode;

use reedi::test_git::Directory;
use reedi::test_git::Git;
use reedi::test_support::performance_repository;
use reedi::test_support::performance_repository::Uncommitted;

fn main() -> ExitCode {
    let usage = "usage: performance_repository <directory> [none|index|working-directory|both]";
    let Some(directory) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("{usage}");
        return ExitCode::FAILURE;
    };
    let uncommitted = match std::env::args().nth(2) {
        None => Uncommitted::None,
        Some(name) => match Uncommitted::from_name(&name) {
            Some(uncommitted) => uncommitted,
            None => {
                eprintln!("{usage}");
                return ExitCode::FAILURE;
            }
        },
    };
    if directory.exists() {
        eprintln!("{} already exists", directory.display());
        return ExitCode::FAILURE;
    }
    let git = Git::on_branch_in(Directory::Persistent(directory));
    performance_repository::generate(&git, uncommitted);
    ExitCode::SUCCESS
}
