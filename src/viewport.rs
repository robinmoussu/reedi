//! Position of the cursor and of the screen in the displayed rows.

/// Minimal number of rows kept between the cursor and the top/bottom of the screen.
const SCROLLOFF: usize = 5;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Viewport {
    /// Row of the cursor.
    pub cursor: usize,
    /// First row displayed on the screen.
    pub top: usize,
}

impl Viewport {
    /// Move the cursor one row down, unless it is on the last of `len` rows.
    pub fn down(&mut self, len: usize) {
        if self.cursor + 1 < len {
            self.cursor += 1;
        }
    }

    /// Move the cursor one row up, unless it is on the first row.
    pub fn up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    /// Scroll as little as possible so that the screen, `height` rows tall, displays `SCROLLOFF`
    /// rows above and below the cursor (unless the first or last of `len` rows is reached), and no
    /// empty space after the last row.
    ///
    /// Scrolling as little as possible keeps the cursor at the same screen position when `height`
    /// changes.
    pub fn scroll(&mut self, len: usize, height: usize) {
        let height = height.max(1);
        let scrolloff = SCROLLOFF.min((height - 1) / 2);
        let lowest = (self.cursor + scrolloff + 1).saturating_sub(height);
        let highest = self.cursor.saturating_sub(scrolloff);
        self.top = self
            .top
            .clamp(lowest, highest)
            .min(len.saturating_sub(height));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn viewport(cursor: usize, top: usize) -> Viewport {
        Viewport { cursor, top }
    }

    #[test]
    fn cursor_stops_on_the_first_and_last_rows() {
        let mut viewport = viewport(1, 0);
        viewport.up();
        viewport.up();
        assert_eq!(viewport.cursor, 0);
        viewport.down(3);
        viewport.down(3);
        viewport.down(3);
        assert_eq!(viewport.cursor, 2);
    }

    #[test]
    fn screen_does_not_scroll_when_the_cursor_is_far_from_the_edges() {
        let mut viewport = viewport(15, 10);
        viewport.scroll(100, 20);
        assert_eq!(viewport.top, 10);
    }

    #[test]
    fn screen_scrolls_down_to_keep_scrolloff_rows_below_the_cursor() {
        let mut viewport = viewport(25, 10);
        viewport.scroll(100, 20);
        // Rows 11 to 30 are displayed, the cursor is 5 rows above the bottom.
        assert_eq!(viewport.top, 11);
    }

    #[test]
    fn screen_scrolls_up_to_keep_scrolloff_rows_above_the_cursor() {
        let mut viewport = viewport(14, 10);
        viewport.scroll(100, 20);
        assert_eq!(viewport.top, 9);
    }

    #[test]
    fn scrolloff_is_ignored_at_the_beginning() {
        let mut viewport = viewport(2, 0);
        viewport.scroll(100, 20);
        assert_eq!(viewport.top, 0);
    }

    #[test]
    fn scrolloff_is_ignored_at_the_end() {
        let mut viewport = viewport(99, 80);
        viewport.scroll(100, 20);
        assert_eq!(viewport.top, 80);
    }

    #[test]
    fn screen_does_not_display_empty_space_after_the_last_row() {
        let mut viewport = viewport(90, 85);
        viewport.scroll(100, 20);
        assert_eq!(viewport.top, 80);
    }

    #[test]
    fn screen_does_not_scroll_when_everything_fits() {
        let mut viewport = viewport(9, 0);
        viewport.scroll(10, 20);
        assert_eq!(viewport.top, 0);
    }

    #[test]
    fn scrolloff_is_reduced_on_a_small_screen() {
        // With a height of 5, the cursor stays in the middle row.
        let mut viewport = viewport(10, 0);
        viewport.scroll(100, 5);
        assert_eq!(viewport.top, 8);
    }

    #[test]
    fn resizing_keeps_the_cursor_at_the_same_screen_position() {
        let mut viewport = viewport(30, 20);
        viewport.scroll(100, 30);
        assert_eq!(viewport.top, 20);
        viewport.scroll(100, 20);
        assert_eq!(viewport.top, 20);
    }

    #[test]
    fn resizing_moves_the_screen_when_the_cursor_would_be_too_close_to_the_bottom() {
        let mut viewport = viewport(40, 20);
        viewport.scroll(100, 30);
        viewport.scroll(100, 20);
        // Rows 26 to 45 are displayed, the cursor is 5 rows above the bottom.
        assert_eq!(viewport.top, 26);
    }

    #[test]
    fn zero_height_does_not_panic() {
        let mut viewport = viewport(10, 0);
        viewport.scroll(100, 0);
        assert_eq!(viewport.top, 10);
    }
}
