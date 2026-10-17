//! `reedi` is a review editor: a TUI to review (and edit) unpushed commits.

mod app;
pub mod fake;
pub mod model;
pub mod view;

pub use app::run;
