//! `reedi` is a review editor: a TUI to review (and edit) unpushed commits, or uncommitted
//! changes.

#![forbid(unsafe_code)]

mod app;
pub mod cli;
pub mod config;
pub mod date;
pub mod fake;
pub mod git;
pub mod invisible;
pub mod log;
pub mod model;
pub mod settings;
#[cfg(any(test, feature = "test-support"))]
pub mod test_git;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
pub mod theme;
pub mod view;
pub mod viewport;
pub mod wrap;

pub use app::Timing;
pub use app::run;
