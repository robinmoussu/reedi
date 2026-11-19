//! Generate the repository of the performance measurements (see
//! [`reedi::test_support::performance_repository`]) in the directory given as argument, which
//! must not exist.

use std::path::PathBuf;
use std::process::ExitCode;

use reedi::test_git::Directory;
use reedi::test_git::Git;
use reedi::test_support::performance_repository;

fn main() -> ExitCode {
    let Some(directory) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: performance_repository <directory>");
        return ExitCode::FAILURE;
    };
    if directory.exists() {
        eprintln!("{} already exists", directory.display());
        return ExitCode::FAILURE;
    }
    let git = Git::on_branch_in(Directory::Persistent(directory));
    performance_repository::generate(&git);
    ExitCode::SUCCESS
}
