//! What `crossterm` doesn't provide yet (see `compatibility.md`).

use std::fmt;
#[cfg(windows)]
use std::io;

use crossterm::Command;

/// The shape of the mouse pointer, named like the CSS `cursor` property.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PointerShape {
    /// The shape chosen by the terminal.
    #[default]
    Default,
    /// A horizontal double arrow, for moving a vertical separator.
    ColResize,
}

impl PointerShape {
    fn name(self) -> &'static str {
        match self {
            PointerShape::Default => "default",
            PointerShape::ColResize => "col-resize",
        }
    }
}

/// Set the shape of the mouse pointer over the terminal (`OSC 22`). Terminals that don't support
/// it ignore it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetPointerShape(pub PointerShape);

impl Command for SetPointerShape {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b]22;{}\x1b\\", self.0.name())
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_pointer_shape_is_osc_22_with_the_css_name_of_the_shape() {
        let mut sequence = String::new();
        SetPointerShape(PointerShape::ColResize)
            .write_ansi(&mut sequence)
            .unwrap();
        assert_eq!(sequence, "\x1b]22;col-resize\x1b\\");
    }
}
