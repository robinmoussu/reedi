//! `reedi` is a review editor: a TUI to review (and edit) unpushed commits.

mod app;
pub mod fake;
pub mod invisible;
pub mod model;
pub mod view;
pub mod viewport;
pub mod wrap;

pub use app::run;
