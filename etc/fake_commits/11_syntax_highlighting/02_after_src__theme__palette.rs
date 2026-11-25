use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;

/// The colors of a theme, by name.
pub struct Palette {
    colors: HashMap<String, (u8, u8, u8)>,
}

impl Palette {
    /// The palette read from `path`: one `name #rrggbb` color per line.
    pub fn read(path: &Path) -> io::Result<Self> {
        let colors = fs::read_to_string(path)?
            .lines()
            .filter_map(|line| {
                let (name, hex) = line.split_once(' ')?;
                let rgb = u32::from_str_radix(hex.strip_prefix('#')?, 16).ok()?;
                Some((name.to_string(), ((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)))
            })
            .collect();
        Ok(Self { colors })
    }

    /// The color `name`, if the palette has it.
    pub fn color(&self, name: &str) -> Option<(u8, u8, u8)> {
        self.colors.get(name).copied()
    }
}
