//! The settings of `reedi`, read once at startup.

use crate::theme::Theme;

/// All the settings, given to [`crate::run`]. Each function only receives the settings it uses.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub theme: Theme,
}
