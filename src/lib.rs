//! `reedi` is a review editor: a TUI to review (and edit) unpushed commits.

mod app;
pub mod cli;
pub mod config;
pub mod date;
pub mod diff;
pub mod fake;
pub mod git;
pub mod gix_compat;
pub mod invisible;
pub mod model;
pub mod pairing;
pub mod settings;
#[cfg(test)]
mod test_git;
pub mod theme;
pub mod view;
pub mod viewport;
pub mod wrap;

pub use app::run;
