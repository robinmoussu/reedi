//! The settings of `reedi`, read once at startup.

use crate::date::DateDisplay;
use crate::theme::Theme;

/// All the settings, given to [`crate::run`]. Each function only receives the settings it uses.
#[derive(Debug)]
pub struct Settings {
    pub theme: Theme,
    pub dates: DateDisplay,
}
