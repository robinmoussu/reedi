//! Position of the cursor and of the screen in the displayed rows.

use crate::settings::SCROLLOFF;

/// Minimal number of rows kept between the cursor and the top/bottom of a screen `height` rows
/// tall, which is smaller on a small screen.
fn scrolloff(height: usize) -> usize {
    SCROLLOFF.min(height.saturating_sub(1) / 2)
}

/// Number of rows the cursor moves alone during `count` moves of `half` rows, once the screen,
/// moved by `scrolled_count` rows, cannot move anymore: each move scrolls the screen if it still
/// can (possibly by fewer rows), otherwise it moves the cursor.
fn cursor_only_count(scrolled_count: usize, half: usize, count: usize) -> usize {
    if half == 0 {
        return 0;
    }
    count
        .saturating_sub(scrolled_count.div_ceil(half))
        .saturating_mul(half)
}

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

    /// Move the cursor `rows_count` rows down, stopping on the last of `len` rows.
    pub fn down_by(&mut self, rows_count: usize, len: usize) {
        self.cursor = self
            .cursor
            .saturating_add(rows_count)
            .min(len.saturating_sub(1));
    }

    /// Move the cursor `rows_count` rows up, stopping on the first row.
    pub fn up_by(&mut self, rows_count: usize) {
        self.cursor = self.cursor.saturating_sub(rows_count);
    }

    /// Move the cursor to the row `cursor` (or to the last of `len` rows if it is further), then
    /// scroll like [`Viewport::scroll()`].
    pub fn jump(&mut self, cursor: usize, len: usize, height: usize) {
        self.cursor = cursor.min(len.saturating_sub(1));
        self.scroll(len, height);
    }

    /// Move the cursor to the row `cursor` (or to the last of `len` rows if it is further), and
    /// scroll so that it is `scrolloff(height)` rows below the top of the screen, `height` rows
    /// tall, unless that would display space before the first row or after the last row.
    pub fn jump_and_scroll_to_top_margin(&mut self, cursor: usize, len: usize, height: usize) {
        self.cursor = cursor.min(len.saturating_sub(1));
        self.top = self
            .cursor
            .saturating_sub(scrolloff(height))
            .min(len.saturating_sub(height));
    }

    /// Scroll as little as possible so that the cursor is displayed on the screen, `height` rows
    /// tall, and there is no empty space after the last of `len` rows. Unlike
    /// [`Viewport::scroll()`], the cursor can be inside the scrolloff margin.
    pub fn fit(&mut self, len: usize, height: usize) {
        let height = height.max(1);
        let lowest = (self.cursor + 1).saturating_sub(height);
        self.top = self
            .top
            .clamp(lowest, self.cursor)
            .min(len.saturating_sub(height));
    }

    /// Keep the cursor on the screen, `height` rows tall, after it is resized or the `len` rows are
    /// replaced: a cursor below the screen moves up to its last row, and the screen scrolls up if
    /// there are empty rows after the last row. Otherwise nothing moves, even inside the scrolloff
    /// margin.
    pub fn resize(&mut self, len: usize, height: usize) {
        let height = height.max(1);
        self.top = self.top.min(len.saturating_sub(height));
        self.cursor = self.cursor.min(self.top + height - 1);
    }

    /// Move the cursor `rows_count` rows down (`j`), one row at a time. When the cursor would move
    /// into (or further into) the bottom scrolloff margin of the screen, `height` rows tall, the
    /// screen moves with it instead, unless the last of `len` rows is displayed.
    pub fn move_down(&mut self, rows_count: usize, len: usize, height: usize) {
        let height = height.max(1);
        let lowest_allowed = height - 1 - scrolloff(height);
        let distance = self.cursor.saturating_sub(self.top);
        let reaching_margin_count = rows_count.min(lowest_allowed.saturating_sub(distance));
        self.top += (rows_count - reaching_margin_count)
            .min(len.saturating_sub(height).saturating_sub(self.top));
        self.down_by(rows_count, len);
    }

    /// Move the cursor `rows_count` rows up (`k`), one row at a time. When the cursor would move
    /// into (or further into) the top scrolloff margin of the screen, `height` rows tall, the
    /// screen moves with it instead, unless the first row is displayed.
    pub fn move_up(&mut self, rows_count: usize, height: usize) {
        let height = height.max(1);
        let distance = self.cursor.saturating_sub(self.top);
        let reaching_margin_count = rows_count.min(distance.saturating_sub(scrolloff(height)));
        self.top -= (rows_count - reaching_margin_count).min(self.top);
        self.up_by(rows_count);
    }

    /// Move the screen `lines_count` lines down (`ctrl+e`), one line at a time, stopping when its
    /// last row is the last of `len` rows, like `less`.
    ///
    /// For each line, the cursor moves one row down, like `j`. The screen moves with it, so that the
    /// cursor stays on its row of the screen, unless the cursor is inside the scrolloff margin
    /// (less than `scrolloff(height)` rows below the top of the screen) or the screen cannot move
    /// anymore.
    pub fn scroll_down(&mut self, lines_count: usize, len: usize, height: usize) {
        let height = height.max(1);
        let distance = self.cursor.saturating_sub(self.top);
        let leaving_margin_count = lines_count.min(scrolloff(height).saturating_sub(distance));
        self.top += (lines_count - leaving_margin_count)
            .min(len.saturating_sub(height).saturating_sub(self.top));
        self.down_by(lines_count, len);
    }

    /// Move the screen `lines_count` lines up (`ctrl+y`), one line at a time, stopping when its
    /// first row is the first row, like `less`.
    ///
    /// For each line, the cursor moves one row up, like `k`. The screen moves with it, so that the
    /// cursor stays on its row of the screen, unless the cursor is inside the scrolloff margin
    /// (less than `scrolloff(height)` rows above the bottom of the screen) or the screen cannot
    /// move anymore.
    pub fn scroll_up(&mut self, lines_count: usize, height: usize) {
        let height = height.max(1);
        let lowest_allowed = height - 1 - scrolloff(height);
        let distance = self.cursor.saturating_sub(self.top);
        let leaving_margin_count = lines_count.min(distance.saturating_sub(lowest_allowed));
        self.top -= (lines_count - leaving_margin_count).min(self.top);
        self.up_by(lines_count);
    }

    /// Move half the screen (`height / 2` rows, `count` times) down (`d`), ignoring the scrolloff
    /// margin. Each time, the screen and the cursor move together, by fewer rows if the screen
    /// reaches the last of `len` rows, or, if the last row is already displayed, the cursor moves
    /// alone, stopping on the last row.
    pub fn half_page_down(&mut self, count: usize, len: usize, height: usize) {
        let half = height / 2;
        let scrolled_count = len
            .saturating_sub(height)
            .saturating_sub(self.top)
            .min(half.saturating_mul(count));
        self.top += scrolled_count;
        self.down_by(
            scrolled_count.saturating_add(cursor_only_count(scrolled_count, half, count)),
            len,
        );
    }

    /// Move half the screen (`height / 2` rows, `count` times) up (`u`), ignoring the scrolloff
    /// margin. Each time, the screen and the cursor move together, by fewer rows if the screen
    /// reaches the first row, or, if the first row is already displayed, the cursor moves alone,
    /// stopping on the first row.
    pub fn half_page_up(&mut self, count: usize, height: usize) {
        let half = height / 2;
        let scrolled_count = self.top.min(half.saturating_mul(count));
        self.top -= scrolled_count;
        self.up_by(scrolled_count.saturating_add(cursor_only_count(scrolled_count, half, count)));
    }

    /// Move the cursor to the row `cursor`, keeping it at the same screen position (unless the
    /// screen would have to start before the first row).
    pub fn move_cursor(&mut self, cursor: usize) {
        self.top = (self.top + cursor).saturating_sub(self.cursor);
        self.cursor = cursor;
    }

    /// Scroll as little as possible so that the screen, `height` rows tall, displays
    /// `scrolloff(height)` rows above and below the cursor (unless the first or last of `len` rows
    /// is reached), and no empty space after the last row.
    ///
    /// Scrolling as little as possible keeps the cursor at the same screen position when `height`
    /// changes.
    pub fn scroll(&mut self, len: usize, height: usize) {
        let height = height.max(1);
        let scrolloff = scrolloff(height);
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

    #[test]
    fn cursor_stops_on_the_first_and_last_rows() {
        let mut viewport = Viewport { cursor: 1, top: 0 };
        viewport.up();
        viewport.up();
        assert_eq!(viewport.cursor, 0);
        viewport.down(/* len */ 3);
        viewport.down(/* len */ 3);
        viewport.down(/* len */ 3);
        assert_eq!(viewport.cursor, 2);
    }

    #[test]
    fn screen_does_not_scroll_when_the_cursor_is_far_from_the_edges() {
        let mut viewport = Viewport { cursor: 15, top: 10 };
        viewport.scroll(/* len */ 100, /* height */ 20);
        assert_eq!(viewport.top, 10);
    }

    #[test]
    fn screen_scrolls_down_to_keep_scrolloff_rows_below_the_cursor() {
        let mut viewport = Viewport { cursor: 25, top: 10 };
        viewport.scroll(/* len */ 100, /* height */ 20);
        // Rows 11 to 30 are displayed, the cursor is 5 rows above the bottom.
        assert_eq!(viewport.top, 11);
    }

    #[test]
    fn screen_scrolls_up_to_keep_scrolloff_rows_above_the_cursor() {
        let mut viewport = Viewport { cursor: 14, top: 10 };
        viewport.scroll(/* len */ 100, /* height */ 20);
        assert_eq!(viewport.top, 9);
    }

    #[test]
    fn scrolloff_is_ignored_at_the_beginning() {
        let mut viewport = Viewport { cursor: 2, top: 0 };
        viewport.scroll(/* len */ 100, /* height */ 20);
        assert_eq!(viewport.top, 0);
    }

    #[test]
    fn scrolloff_is_ignored_at_the_end() {
        let mut viewport = Viewport { cursor: 99, top: 80 };
        viewport.scroll(/* len */ 100, /* height */ 20);
        assert_eq!(viewport.top, 80);
    }

    #[test]
    fn screen_does_not_display_empty_space_after_the_last_row() {
        let mut viewport = Viewport { cursor: 90, top: 85 };
        viewport.scroll(/* len */ 100, /* height */ 20);
        assert_eq!(viewport.top, 80);
    }

    #[test]
    fn screen_does_not_scroll_when_everything_fits() {
        let mut viewport = Viewport { cursor: 9, top: 0 };
        viewport.scroll(/* len */ 10, /* height */ 20);
        assert_eq!(viewport.top, 0);
    }

    #[test]
    fn scrolloff_is_reduced_on_a_small_screen() {
        // With a height of 5, the cursor stays in the middle row.
        let mut viewport = Viewport { cursor: 10, top: 0 };
        viewport.scroll(/* len */ 100, /* height */ 5);
        assert_eq!(viewport.top, 8);
    }

    #[test]
    fn resizing_keeps_the_cursor_at_the_same_screen_position() {
        let mut viewport = Viewport { cursor: 30, top: 20 };
        viewport.scroll(/* len */ 100, /* height */ 30);
        assert_eq!(viewport.top, 20);
        viewport.scroll(/* len */ 100, /* height */ 20);
        assert_eq!(viewport.top, 20);
    }

    #[test]
    fn resizing_moves_the_screen_when_the_cursor_would_be_too_close_to_the_bottom() {
        let mut viewport = Viewport { cursor: 40, top: 20 };
        viewport.scroll(/* len */ 100, /* height */ 30);
        viewport.scroll(/* len */ 100, /* height */ 20);
        // Rows 26 to 45 are displayed, the cursor is 5 rows above the bottom.
        assert_eq!(viewport.top, 26);
    }

    #[test]
    fn moved_cursor_stays_at_the_same_screen_position() {
        let mut viewport = Viewport { cursor: 30, top: 20 };
        viewport.move_cursor(40);
        assert_eq!(viewport, Viewport { cursor: 40, top: 30 });
        viewport.move_cursor(35);
        assert_eq!(viewport, Viewport { cursor: 35, top: 25 });
    }

    #[test]
    fn moved_cursor_cannot_go_above_the_first_row() {
        let mut viewport = Viewport { cursor: 30, top: 20 };
        viewport.move_cursor(5);
        assert_eq!(viewport, Viewport { cursor: 5, top: 0 });
    }

    #[test]
    fn zero_height_does_not_panic() {
        let mut viewport = Viewport { cursor: 10, top: 0 };
        viewport.scroll(/* len */ 100, /* height */ 0);
        assert_eq!(viewport.top, 10);
    }

    #[test]
    fn cursor_moves_by_several_rows() {
        let mut viewport = Viewport { cursor: 10, top: 0 };
        viewport.down_by(/* rows_count */ 5, /* len */ 100);
        assert_eq!(viewport.cursor, 15);
        viewport.up_by(7);
        assert_eq!(viewport.cursor, 8);
    }

    #[test]
    fn cursor_moved_by_several_rows_stops_on_the_first_and_last_rows() {
        let mut viewport = Viewport { cursor: 10, top: 0 };
        viewport.down_by(/* rows_count */ 1000, /* len */ 100);
        assert_eq!(viewport.cursor, 99);
        viewport.up_by(1000);
        assert_eq!(viewport.cursor, 0);
        viewport.down_by(usize::MAX, /* len */ 100);
        assert_eq!(viewport.cursor, 99);
        viewport.up_by(usize::MAX);
        assert_eq!(viewport.cursor, 0);
    }

    #[test]
    fn cursor_does_not_move_without_rows() {
        let mut viewport = Viewport { cursor: 0, top: 0 };
        viewport.down_by(/* rows_count */ 3, /* len */ 0);
        assert_eq!(viewport.cursor, 0);
    }

    #[test]
    fn resize_moves_nothing_when_the_cursor_stays_on_the_screen() {
        // The cursor is the 19th row of the screen, inside the scrolloff margin.
        let mut viewport = Viewport { cursor: 28, top: 10 };
        viewport.resize(/* len */ 100, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 28, top: 10 });
    }

    #[test]
    fn resize_moves_a_cursor_below_the_screen_to_its_last_row() {
        // The cursor was the 31st row of a screen at least 31 rows tall.
        let mut viewport = Viewport { cursor: 40, top: 10 };
        viewport.resize(/* len */ 100, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 29, top: 10 });
    }

    #[test]
    fn resize_scrolls_up_to_fill_the_empty_rows_after_the_last_row() {
        // The screen displayed the last 20 rows.
        let mut viewport = Viewport { cursor: 90, top: 80 };
        viewport.resize(/* len */ 100, /* height */ 30);
        assert_eq!(viewport, Viewport { cursor: 90, top: 70 });
    }

    #[test]
    fn resize_with_a_zero_height_does_not_panic() {
        let mut viewport = Viewport { cursor: 10, top: 10 };
        viewport.resize(/* len */ 100, /* height */ 0);
        assert_eq!(viewport, Viewport { cursor: 10, top: 10 });
    }

    #[test]
    fn fit_does_not_scroll_when_the_cursor_is_inside_the_scrolloff_margin() {
        // The cursor is the 3rd row of the screen, and the 18th row of the screen below.
        let mut viewport = Viewport { cursor: 12, top: 10 };
        viewport.fit(/* len */ 100, /* height */ 20);
        assert_eq!(viewport.top, 10);
        let mut viewport = Viewport { cursor: 28, top: 10 };
        viewport.fit(/* len */ 100, /* height */ 20);
        assert_eq!(viewport.top, 10);
    }

    #[test]
    fn fit_scrolls_up_to_display_the_cursor() {
        let mut viewport = Viewport { cursor: 7, top: 10 };
        viewport.fit(/* len */ 100, /* height */ 20);
        assert_eq!(viewport.top, 7);
    }

    #[test]
    fn fit_scrolls_down_to_display_the_cursor() {
        let mut viewport = Viewport { cursor: 40, top: 10 };
        viewport.fit(/* len */ 100, /* height */ 20);
        // Rows 21 to 40 are displayed.
        assert_eq!(viewport.top, 21);
    }

    #[test]
    fn fit_does_not_display_empty_space_after_the_last_row() {
        let mut viewport = Viewport { cursor: 90, top: 95 };
        viewport.fit(/* len */ 100, /* height */ 20);
        assert_eq!(viewport.top, 80);
    }

    #[test]
    fn fit_does_not_scroll_when_everything_fits() {
        let mut viewport = Viewport { cursor: 9, top: 3 };
        viewport.fit(/* len */ 10, /* height */ 20);
        assert_eq!(viewport.top, 0);
    }

    #[test]
    fn fit_with_a_zero_height_does_not_panic() {
        let mut viewport = Viewport { cursor: 10, top: 0 };
        viewport.fit(/* len */ 100, /* height */ 0);
        assert_eq!(viewport.top, 10);
    }

    #[test]
    fn jump_scrolls_as_little_as_possible_to_keep_the_scrolloff() {
        let mut viewport = Viewport { cursor: 0, top: 0 };
        viewport.jump(
            /* cursor */ 50, /* len */ 100, /* height */ 20,
        );
        // Rows 36 to 55 are displayed, the cursor is 5 rows above the bottom.
        assert_eq!(viewport, Viewport { cursor: 50, top: 36 });
        viewport.jump(
            /* cursor */ 0, /* len */ 100, /* height */ 20,
        );
        assert_eq!(viewport, Viewport { cursor: 0, top: 0 });
    }

    #[test]
    fn jump_does_not_scroll_when_the_cursor_stays_far_from_the_edges() {
        let mut viewport = Viewport { cursor: 15, top: 10 };
        viewport.jump(
            /* cursor */ 20, /* len */ 100, /* height */ 20,
        );
        assert_eq!(viewport, Viewport { cursor: 20, top: 10 });
    }

    #[test]
    fn jump_stops_on_the_last_row() {
        let mut viewport = Viewport { cursor: 0, top: 0 };
        viewport.jump(
            /* cursor */ 1000, /* len */ 100, /* height */ 20,
        );
        assert_eq!(viewport, Viewport { cursor: 99, top: 80 });
    }

    #[test]
    fn jump_and_scroll_to_top_margin_puts_the_cursor_scrolloff_rows_below_the_top_of_the_screen() {
        let mut viewport = Viewport { cursor: 0, top: 0 };
        viewport.jump_and_scroll_to_top_margin(
            /* cursor */ 50, /* len */ 100, /* height */ 20,
        );
        assert_eq!(viewport, Viewport { cursor: 50, top: 45 });
        viewport.jump_and_scroll_to_top_margin(
            /* cursor */ 10, /* len */ 100, /* height */ 20,
        );
        assert_eq!(viewport, Viewport { cursor: 10, top: 5 });
    }

    #[test]
    fn jump_and_scroll_to_top_margin_scrolls_even_when_the_cursor_does_not_move() {
        let mut viewport = Viewport { cursor: 30, top: 20 };
        viewport.jump_and_scroll_to_top_margin(
            /* cursor */ 30, /* len */ 100, /* height */ 20,
        );
        assert_eq!(viewport, Viewport { cursor: 30, top: 25 });
    }

    #[test]
    fn jump_and_scroll_to_top_margin_does_not_display_space_before_the_first_row() {
        let mut viewport = Viewport { cursor: 50, top: 40 };
        viewport.jump_and_scroll_to_top_margin(
            /* cursor */ 2, /* len */ 100, /* height */ 20,
        );
        assert_eq!(viewport, Viewport { cursor: 2, top: 0 });
    }

    #[test]
    fn jump_and_scroll_to_top_margin_uses_a_smaller_scrolloff_on_a_small_screen() {
        // The scrolloff is 2 rows on a screen of 5 rows.
        let mut viewport = Viewport { cursor: 0, top: 0 };
        viewport.jump_and_scroll_to_top_margin(
            /* cursor */ 10, /* len */ 100, /* height */ 5,
        );
        assert_eq!(viewport, Viewport { cursor: 10, top: 8 });
    }

    #[test]
    fn jump_and_scroll_to_top_margin_does_not_display_empty_space_after_the_last_row() {
        let mut viewport = Viewport { cursor: 0, top: 0 };
        viewport.jump_and_scroll_to_top_margin(
            /* cursor */ 90, /* len */ 100, /* height */ 20,
        );
        assert_eq!(viewport, Viewport { cursor: 90, top: 80 });
    }

    #[test]
    fn jump_and_scroll_to_top_margin_stops_on_the_last_row() {
        let mut viewport = Viewport { cursor: 0, top: 0 };
        viewport.jump_and_scroll_to_top_margin(
            /* cursor */ 1000, /* len */ 100, /* height */ 20,
        );
        assert_eq!(viewport, Viewport { cursor: 99, top: 80 });
    }

    #[test]
    fn jump_and_scroll_to_top_margin_does_not_scroll_when_everything_fits() {
        let mut viewport = Viewport { cursor: 0, top: 0 };
        viewport.jump_and_scroll_to_top_margin(
            /* cursor */ 5, /* len */ 10, /* height */ 20,
        );
        assert_eq!(viewport, Viewport { cursor: 5, top: 0 });
    }

    #[test]
    fn move_down_only_moves_the_cursor_outside_the_margins() {
        let mut viewport = Viewport { cursor: 20, top: 10 };
        viewport.move_down(
            /* rows_count */ 1, /* len */ 100, /* height */ 20,
        );
        assert_eq!(viewport, Viewport { cursor: 21, top: 10 });
    }

    #[test]
    fn move_down_moves_the_screen_with_a_cursor_reaching_the_bottom_margin() {
        // The cursor is the 15th row of the screen: the next one is inside the margin.
        let mut viewport = Viewport { cursor: 24, top: 10 };
        viewport.move_down(
            /* rows_count */ 1, /* len */ 100, /* height */ 20,
        );
        assert_eq!(viewport, Viewport { cursor: 25, top: 11 });
    }

    #[test]
    fn move_down_moves_the_screen_with_a_cursor_inside_the_bottom_margin() {
        // The cursor is the 19th row of the screen.
        let mut viewport = Viewport { cursor: 28, top: 10 };
        viewport.move_down(
            /* rows_count */ 1, /* len */ 100, /* height */ 20,
        );
        assert_eq!(viewport, Viewport { cursor: 29, top: 11 });
    }

    #[test]
    fn move_down_only_moves_a_cursor_inside_the_top_margin() {
        // The cursor is the 3rd row of the screen.
        let mut viewport = Viewport { cursor: 12, top: 10 };
        viewport.move_down(
            /* rows_count */ 1, /* len */ 100, /* height */ 20,
        );
        assert_eq!(viewport, Viewport { cursor: 13, top: 10 });
    }

    #[test]
    fn move_down_only_moves_the_cursor_when_the_last_row_is_displayed() {
        let mut viewport = Viewport { cursor: 90, top: 80 };
        viewport.move_down(
            /* rows_count */ 1, /* len */ 100, /* height */ 20,
        );
        assert_eq!(viewport, Viewport { cursor: 91, top: 80 });
    }

    #[test]
    fn move_down_moves_the_screen_once_the_cursor_reaches_the_bottom_margin() {
        // The cursor is the 13th row of the screen: it moves alone to the 15th, then with the
        // screen.
        let mut viewport = Viewport { cursor: 22, top: 10 };
        viewport.move_down(
            /* rows_count */ 5, /* len */ 100, /* height */ 20,
        );
        assert_eq!(viewport, Viewport { cursor: 27, top: 13 });
    }

    #[test]
    fn move_down_by_more_rows_than_available_stops_on_the_last_row() {
        let mut viewport = Viewport { cursor: 50, top: 40 };
        viewport.move_down(
            /* rows_count */ 1000, /* len */ 100, /* height */ 20,
        );
        assert_eq!(viewport, Viewport { cursor: 99, top: 80 });
    }

    #[test]
    fn move_up_only_moves_the_cursor_outside_the_margins() {
        let mut viewport = Viewport { cursor: 20, top: 10 };
        viewport.move_up(/* rows_count */ 1, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 19, top: 10 });
    }

    #[test]
    fn move_up_moves_the_screen_with_a_cursor_reaching_the_top_margin() {
        // The cursor is the 6th row of the screen: the previous one is inside the margin.
        let mut viewport = Viewport { cursor: 15, top: 10 };
        viewport.move_up(/* rows_count */ 1, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 14, top: 9 });
    }

    #[test]
    fn move_up_moves_the_screen_with_a_cursor_inside_the_top_margin() {
        // The cursor is the 3rd row of the screen.
        let mut viewport = Viewport { cursor: 12, top: 10 };
        viewport.move_up(/* rows_count */ 1, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 11, top: 9 });
    }

    #[test]
    fn move_up_only_moves_a_cursor_inside_the_bottom_margin() {
        // The cursor is the 19th row of the screen.
        let mut viewport = Viewport { cursor: 28, top: 10 };
        viewport.move_up(/* rows_count */ 1, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 27, top: 10 });
    }

    #[test]
    fn move_up_only_moves_the_cursor_when_the_first_row_is_displayed() {
        let mut viewport = Viewport { cursor: 10, top: 0 };
        viewport.move_up(/* rows_count */ 1, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 9, top: 0 });
    }

    #[test]
    fn move_up_moves_the_screen_once_the_cursor_reaches_the_top_margin() {
        // The cursor is the 8th row of the screen: it moves alone to the 6th, then with the
        // screen.
        let mut viewport = Viewport { cursor: 17, top: 10 };
        viewport.move_up(/* rows_count */ 5, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 12, top: 7 });
    }

    #[test]
    fn move_up_by_more_rows_than_available_stops_on_the_first_row() {
        let mut viewport = Viewport { cursor: 50, top: 40 };
        viewport.move_up(/* rows_count */ 1000, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 0, top: 0 });
    }

    #[test]
    fn scroll_down_moves_the_screen_and_the_cursor_far_from_the_top() {
        let mut viewport = Viewport { cursor: 20, top: 10 };
        viewport.scroll_down(
            /* lines_count */ 1, /* len */ 100, /* height */ 20,
        );
        assert_eq!(viewport, Viewport { cursor: 21, top: 11 });
    }

    #[test]
    fn scroll_down_moves_a_cursor_inside_the_scrolloff_margin_like_j() {
        // The cursor is the 3rd row of the screen.
        let mut viewport = Viewport { cursor: 2, top: 0 };
        viewport.scroll_down(
            /* lines_count */ 1, /* len */ 100, /* height */ 20,
        );
        assert_eq!(viewport, Viewport { cursor: 3, top: 0 });
    }

    #[test]
    fn scroll_down_scrolls_once_the_cursor_is_out_of_the_scrolloff_margin() {
        // The cursor is the 4th row of the screen: it moves to the 6th, then it moves with the
        // screen.
        let mut viewport = Viewport { cursor: 3, top: 0 };
        viewport.scroll_down(
            /* lines_count */ 3, /* len */ 100, /* height */ 20,
        );
        assert_eq!(viewport, Viewport { cursor: 6, top: 1 });
    }

    #[test]
    fn scroll_down_moves_a_cursor_inside_the_bottom_margin_with_the_screen() {
        // The cursor is the 19th row of the screen.
        let mut viewport = Viewport { cursor: 28, top: 10 };
        viewport.scroll_down(
            /* lines_count */ 1, /* len */ 100, /* height */ 20,
        );
        assert_eq!(viewport, Viewport { cursor: 29, top: 11 });
    }

    #[test]
    fn scroll_down_only_moves_the_cursor_when_the_last_row_is_displayed() {
        let mut viewport = Viewport { cursor: 90, top: 79 };
        viewport.scroll_down(
            /* lines_count */ 1, /* len */ 100, /* height */ 20,
        );
        assert_eq!(viewport, Viewport { cursor: 91, top: 80 });
        viewport.scroll_down(
            /* lines_count */ 1, /* len */ 100, /* height */ 20,
        );
        assert_eq!(viewport, Viewport { cursor: 92, top: 80 });
    }

    #[test]
    fn scroll_down_by_more_lines_than_available_stops_on_the_last_row() {
        let mut viewport = Viewport { cursor: 50, top: 40 };
        viewport.scroll_down(
            /* lines_count */ 1000, /* len */ 100, /* height */ 20,
        );
        assert_eq!(viewport, Viewport { cursor: 99, top: 80 });
        viewport.scroll_down(usize::MAX, /* len */ 100, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 99, top: 80 });
    }

    #[test]
    fn scroll_down_only_moves_the_cursor_when_everything_fits() {
        // The cursor is the 8th row of the screen, outside the scrolloff margin.
        let mut viewport = Viewport { cursor: 7, top: 0 };
        viewport.scroll_down(
            /* lines_count */ 1, /* len */ 10, /* height */ 20,
        );
        assert_eq!(viewport, Viewport { cursor: 8, top: 0 });
    }

    #[test]
    fn scroll_down_uses_a_smaller_scrolloff_on_a_small_screen() {
        // The scrolloff is 3 rows on a screen of 7 rows: the cursor is on the 4th row of the
        // screen, so it moves with the screen.
        let mut viewport = Viewport { cursor: 13, top: 10 };
        viewport.scroll_down(
            /* lines_count */ 1, /* len */ 100, /* height */ 7,
        );
        assert_eq!(viewport, Viewport { cursor: 14, top: 11 });
    }

    #[test]
    fn scroll_up_moves_the_screen_and_the_cursor_far_from_the_bottom() {
        let mut viewport = Viewport { cursor: 20, top: 10 };
        viewport.scroll_up(/* lines_count */ 1, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 19, top: 9 });
    }

    #[test]
    fn scroll_up_moves_a_cursor_inside_the_bottom_margin_like_k() {
        // The cursor is the 19th row of the screen.
        let mut viewport = Viewport { cursor: 28, top: 10 };
        viewport.scroll_up(/* lines_count */ 1, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 27, top: 10 });
    }

    #[test]
    fn scroll_up_scrolls_once_the_cursor_is_out_of_the_bottom_margin() {
        // The cursor is the 17th row of the screen: it moves to the 15th, then it moves with the
        // screen.
        let mut viewport = Viewport { cursor: 26, top: 10 };
        viewport.scroll_up(/* lines_count */ 3, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 23, top: 9 });
    }

    #[test]
    fn scroll_up_only_moves_the_cursor_when_the_first_row_is_displayed() {
        let mut viewport = Viewport { cursor: 10, top: 1 };
        viewport.scroll_up(/* lines_count */ 1, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 9, top: 0 });
        viewport.scroll_up(/* lines_count */ 1, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 8, top: 0 });
    }

    #[test]
    fn scroll_up_by_more_lines_than_available_stops_on_the_first_row() {
        let mut viewport = Viewport { cursor: 20, top: 10 };
        viewport.scroll_up(/* lines_count */ 1000, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 0, top: 0 });
        viewport.scroll_up(usize::MAX, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 0, top: 0 });
    }

    #[test]
    fn half_page_down_moves_the_screen_and_the_cursor_by_half_the_height() {
        let mut viewport = Viewport { cursor: 20, top: 10 };
        viewport.half_page_down(/* count */ 1, /* len */ 100, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 30, top: 20 });
    }

    #[test]
    fn half_page_up_moves_the_screen_and_the_cursor_by_half_the_height() {
        let mut viewport = Viewport { cursor: 40, top: 30 };
        viewport.half_page_up(/* count */ 1, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 30, top: 20 });
    }

    #[test]
    fn half_page_rounds_the_half_of_an_odd_height_down() {
        let mut viewport = Viewport { cursor: 20, top: 10 };
        viewport.half_page_down(/* count */ 1, /* len */ 100, /* height */ 21);
        assert_eq!(viewport, Viewport { cursor: 30, top: 20 });
        let mut viewport = Viewport { cursor: 40, top: 30 };
        viewport.half_page_up(/* count */ 1, /* height */ 21);
        assert_eq!(viewport, Viewport { cursor: 30, top: 20 });
    }

    #[test]
    fn half_page_is_repeated() {
        let mut viewport = Viewport { cursor: 20, top: 10 };
        viewport.half_page_down(/* count */ 3, /* len */ 100, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 50, top: 40 });
        let mut viewport = Viewport { cursor: 40, top: 30 };
        viewport.half_page_up(/* count */ 2, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 20, top: 10 });
    }

    #[test]
    fn half_page_leaves_a_cursor_inside_the_scrolloff_margin() {
        // The cursor is the 3rd row of the screen.
        let mut viewport = Viewport { cursor: 12, top: 10 };
        viewport.half_page_down(/* count */ 1, /* len */ 100, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 22, top: 20 });
    }

    #[test]
    fn half_page_down_moves_the_cursor_alone_when_the_last_row_is_displayed() {
        let mut viewport = Viewport { cursor: 85, top: 80 };
        viewport.half_page_down(/* count */ 1, /* len */ 100, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 95, top: 80 });
        viewport.half_page_down(/* count */ 1, /* len */ 100, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 99, top: 80 });
        viewport.half_page_down(usize::MAX, /* len */ 100, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 99, top: 80 });
    }

    #[test]
    fn half_page_up_moves_the_cursor_alone_when_the_first_row_is_displayed() {
        let mut viewport = Viewport { cursor: 15, top: 0 };
        viewport.half_page_up(/* count */ 1, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 5, top: 0 });
        viewport.half_page_up(/* count */ 1, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 0, top: 0 });
        viewport.half_page_up(usize::MAX, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 0, top: 0 });
    }

    #[test]
    fn half_page_down_scrolls_by_fewer_rows_then_moves_the_cursor_alone_when_the_screen_stops() {
        // Only 5 rows are left below the screen.
        let mut viewport = Viewport { cursor: 85, top: 75 };
        viewport.half_page_down(/* count */ 1, /* len */ 100, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 90, top: 80 });
        let mut viewport = Viewport { cursor: 75, top: 75 };
        viewport.half_page_down(/* count */ 2, /* len */ 100, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 90, top: 80 });
    }

    #[test]
    fn half_page_up_scrolls_by_fewer_rows_then_moves_the_cursor_alone_when_the_screen_stops() {
        // Only 3 rows are left above the screen.
        let mut viewport = Viewport { cursor: 15, top: 3 };
        viewport.half_page_up(/* count */ 1, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 12, top: 0 });
        let mut viewport = Viewport { cursor: 15, top: 3 };
        viewport.half_page_up(/* count */ 2, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 2, top: 0 });
    }

    #[test]
    fn half_page_moves_the_cursor_alone_when_everything_fits() {
        let mut viewport = Viewport { cursor: 3, top: 0 };
        viewport.half_page_down(/* count */ 1, /* len */ 20, /* height */ 20);
        assert_eq!(viewport, Viewport { cursor: 13, top: 0 });
    }

    #[test]
    fn half_page_does_nothing_when_the_screen_is_one_row_tall() {
        let mut viewport = Viewport { cursor: 30, top: 30 };
        viewport.half_page_down(/* count */ 1, /* len */ 100, /* height */ 1);
        viewport.half_page_up(/* count */ 1, /* height */ 1);
        assert_eq!(viewport, Viewport { cursor: 30, top: 30 });
    }
}
