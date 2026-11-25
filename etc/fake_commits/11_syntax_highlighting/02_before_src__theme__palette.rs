use std::collections::HashMap;

/// The colors of a theme, by name.
pub struct Palette {
    colors: HashMap<String, (u8, u8, u8)>,
}

impl Palette {
    /// The palette of the default dark theme.
    pub fn dark() -> Self {
        let colors = [
            ("background", (0x27, 0x28, 0x22)),
            ("foreground", (0xf8, 0xf8, 0xf2)),
            ("comment", (0x75, 0x71, 0x5e)),
        ];
        Self {
            colors: colors
                .into_iter()
                .map(|(name, rgb)| (name.to_string(), rgb))
                .collect(),
        }
    }

    /// The color `name`, if the palette has it.
    pub fn color(&self, name: &str) -> Option<(u8, u8, u8)> {
        self.colors.get(name).copied()
    }
}
