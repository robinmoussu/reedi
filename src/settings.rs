//! The settings of `reedi`, read once at startup.

use crate::date::DateDisplay;
use crate::theme::Theme;

/// Number of columns between two tab stops.
pub const TAB_WIDTH: usize = 4;

/// Number of unchanged lines displayed before and after the modified lines.
pub const CONTEXT: usize = 3;

/// Maximum distance (between 0 and 1, see [`crate::pairing::pairs()`]) between a removed and an
/// added line for them to be displayed side by side.
pub const MAX_PAIRING_DISTANCE: f64 = 0.6;

/// Minimal number of rows kept between the cursor and the top/bottom of the screen.
pub const SCROLLOFF: usize = 5;

/// All the settings, given to [`crate::run()`]. Each function only receives the settings it uses.
#[derive(Debug)]
pub struct Settings {
    pub theme: Theme,
    pub dates: DateDisplay,
}
