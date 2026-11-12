//! The settings of `reedi`, read once at startup.

use crate::date::DateDisplay;
use crate::theme::Theme;

/// Number of columns between two tab stops.
pub const TAB_WIDTH: usize = 4;

/// All the settings, given to [`crate::run()`]. Each function only receives the settings it uses.
#[derive(Debug)]
pub struct Settings {
    pub theme: Theme,
    pub dates: DateDisplay,
}
