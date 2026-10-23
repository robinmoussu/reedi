//! Position of the cursor and of the screen in the displayed rows.

/// Minimal number of rows kept between the cursor and the top/bottom of the screen.
const SCROLLOFF: usize = 5;

/// Minimal number of rows kept between the cursor and the top/bottom of a screen `height` rows
/// tall, which is smaller on a small screen.
fn scrolloff(height: usize) -> usize {
    SCROLLOFF.min(height.saturating_sub(1) / 2)
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

    /// Move the cursor `rows` rows down, stopping on the last of `len` rows.
    pub fn down_by(&mut self, rows: usize, len: usize) {
        self.cursor = self.cursor.saturating_add(rows).min(len.saturating_sub(1));
    }

    /// Move the cursor `rows` rows up, stopping on the first row.
    pub fn up_by(&mut self, rows: usize) {
        self.cursor = self.cursor.saturating_sub(rows);
    }

    /// Move the cursor to the row `cursor` (or to the last of `len` rows if it is further), then
    /// scroll like [`Viewport::scroll`].
    pub fn jump(&mut self, cursor: usize, len: usize, height: usize) {
        self.cursor = cursor.min(len.saturating_sub(1));
        self.scroll(len, height);
    }

    /// Scroll as little as possible so that the cursor is displayed on the screen, `height` rows
    /// tall, and there is no empty space after the last of `len` rows. Unlike
    /// [`Viewport::scroll`], the cursor can be inside the scrolloff margin.
    pub fn fit(&mut self, len: usize, height: usize) {
        let height = height.max(1);
        let lowest = (self.cursor + 1).saturating_sub(height);
        self.top = self
            .top
            .clamp(lowest, self.cursor)
            .min(len.saturating_sub(height));
    }

    /// Move the screen `lines` lines down (`ctrl+e`), one line at a time, stopping when its last
    /// row is the last of `len` rows, like `less`.
    ///
    /// For each line, the cursor stays on its row, unless it is at most `SCROLLOFF` rows below the
    /// top of the screen (inside the margin, or about to enter it): it then moves down with the
    /// screen, so that it keeps its distance from the top.
    pub fn scroll_down(&mut self, lines: usize, len: usize, height: usize) {
        let height = height.max(1);
        let lines = lines.min(len.saturating_sub(height).saturating_sub(self.top));
        // The cursor stays on its row until it is `scrolloff` rows from the top, then it moves
        // with the screen.
        let distance = self.cursor.saturating_sub(self.top);
        let staying = lines.min(distance.saturating_sub(scrolloff(height)));
        self.top += lines;
        self.cursor += lines - staying;
    }

    /// Move the screen `lines` lines up (`ctrl+y`), one line at a time, stopping when its first row
    /// is the first row, like `less`.
    ///
    /// For each line, the cursor stays on its row, unless it is at most `SCROLLOFF` rows above the
    /// bottom of the screen (inside the margin, or about to enter it): it then moves up with the
    /// screen, so that it keeps its distance from the bottom.
    pub fn scroll_up(&mut self, lines: usize, height: usize) {
        let height = height.max(1);
        let lines = lines.min(self.top);
        // The cursor stays on its row until it is `scrolloff` rows from the bottom, then it moves
        // with the screen.
        let distance = self.cursor.saturating_sub(self.top);
        let lowest_allowed = height - 1 - scrolloff(height);
        let staying = lines.min(lowest_allowed.saturating_sub(distance));
        self.top -= lines;
        self.cursor = self.cursor.saturating_sub(lines - staying);
    }

    /// Move half the screen (`height / 2` rows, `times` times) down (`d`): the screen scrolls like
    /// with [`Viewport::scroll_down`], and the cursor moves down by the same number of rows, like
    /// with [`Viewport::down_by`], instead of following the scroll. Nothing moves anymore when the
    /// last of `len` rows is displayed, and a scroll that would go further than that is shortened.
    pub fn half_page_down(&mut self, times: usize, len: usize, height: usize) {
        let rows = (height / 2).saturating_mul(times);
        let rows = rows.min(len.saturating_sub(height).saturating_sub(self.top));
        if rows > 0 {
            self.top += rows;
            self.down_by(rows, len);
            self.scroll(len, height);
        }
    }

    /// Move half the screen (`height / 2` rows, `times` times) up (`u`): the screen scrolls like
    /// with [`Viewport::scroll_up`], and the cursor moves up by the same number of rows, like with
    /// [`Viewport::up_by`], instead of following the scroll. Nothing moves anymore when the first
    /// row is displayed, and a scroll that would go further than that is shortened.
    pub fn half_page_up(&mut self, times: usize, len: usize, height: usize) {
        let rows = (height / 2).saturating_mul(times);
        let rows = rows.min(self.top);
        if rows > 0 {
            self.top -= rows;
            self.up_by(rows);
            self.scroll(len, height);
        }
    }

    /// Move the cursor to the row `cursor`, keeping it at the same screen position (unless the
    /// screen would have to start before the first row).
    pub fn move_cursor(&mut self, cursor: usize) {
        self.top = (self.top + cursor).saturating_sub(self.cursor);
        self.cursor = cursor;
    }

    /// Scroll as little as possible so that the screen, `height` rows tall, displays `SCROLLOFF`
    /// rows above and below the cursor (unless the first or last of `len` rows is reached), and no
    /// empty space after the last row.
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
    fn moved_cursor_stays_at_the_same_screen_position() {
        let mut viewport = viewport(30, 20);
        viewport.move_cursor(40);
        assert_eq!(
            viewport,
            Viewport {
                cursor: 40,
                top: 30
            }
        );
        viewport.move_cursor(35);
        assert_eq!(
            viewport,
            Viewport {
                cursor: 35,
                top: 25
            }
        );
    }

    #[test]
    fn moved_cursor_cannot_go_above_the_first_row() {
        let mut viewport = viewport(30, 20);
        viewport.move_cursor(5);
        assert_eq!(viewport, Viewport { cursor: 5, top: 0 });
    }

    #[test]
    fn zero_height_does_not_panic() {
        let mut viewport = viewport(10, 0);
        viewport.scroll(100, 0);
        assert_eq!(viewport.top, 10);
    }

    #[test]
    fn cursor_moves_by_several_rows() {
        let mut viewport = viewport(10, 0);
        viewport.down_by(5, 100);
        assert_eq!(viewport.cursor, 15);
        viewport.up_by(7);
        assert_eq!(viewport.cursor, 8);
    }

    #[test]
    fn cursor_moved_by_several_rows_stops_on_the_first_and_last_rows() {
        let mut viewport = viewport(10, 0);
        viewport.down_by(1000, 100);
        assert_eq!(viewport.cursor, 99);
        viewport.up_by(1000);
        assert_eq!(viewport.cursor, 0);
        viewport.down_by(usize::MAX, 100);
        assert_eq!(viewport.cursor, 99);
        viewport.up_by(usize::MAX);
        assert_eq!(viewport.cursor, 0);
    }

    #[test]
    fn cursor_does_not_move_without_rows() {
        let mut viewport = viewport(0, 0);
        viewport.down_by(3, 0);
        assert_eq!(viewport.cursor, 0);
    }

    #[test]
    fn fit_does_not_scroll_when_the_cursor_is_inside_the_scrolloff_margin() {
        // The cursor is the 3rd row of the screen, and the 18th row of the screen below.
        let mut viewport = viewport(12, 10);
        viewport.fit(100, 20);
        assert_eq!(viewport.top, 10);
        let mut viewport = self::viewport(28, 10);
        viewport.fit(100, 20);
        assert_eq!(viewport.top, 10);
    }

    #[test]
    fn fit_scrolls_up_to_display_the_cursor() {
        let mut viewport = viewport(7, 10);
        viewport.fit(100, 20);
        assert_eq!(viewport.top, 7);
    }

    #[test]
    fn fit_scrolls_down_to_display_the_cursor() {
        let mut viewport = viewport(40, 10);
        viewport.fit(100, 20);
        // Rows 21 to 40 are displayed.
        assert_eq!(viewport.top, 21);
    }

    #[test]
    fn fit_does_not_display_empty_space_after_the_last_row() {
        let mut viewport = viewport(90, 95);
        viewport.fit(100, 20);
        assert_eq!(viewport.top, 80);
    }

    #[test]
    fn fit_does_not_scroll_when_everything_fits() {
        let mut viewport = viewport(9, 3);
        viewport.fit(10, 20);
        assert_eq!(viewport.top, 0);
    }

    #[test]
    fn fit_with_a_zero_height_does_not_panic() {
        let mut viewport = viewport(10, 0);
        viewport.fit(100, 0);
        assert_eq!(viewport.top, 10);
    }

    #[test]
    fn jump_scrolls_as_little_as_possible_to_keep_the_scrolloff() {
        let mut viewport = viewport(0, 0);
        viewport.jump(50, 100, 20);
        // Rows 36 to 55 are displayed, the cursor is 5 rows above the bottom.
        assert_eq!(
            viewport,
            Viewport {
                cursor: 50,
                top: 36
            }
        );
        viewport.jump(0, 100, 20);
        assert_eq!(viewport, Viewport { cursor: 0, top: 0 });
    }

    #[test]
    fn jump_does_not_scroll_when_the_cursor_stays_far_from_the_edges() {
        let mut viewport = viewport(15, 10);
        viewport.jump(20, 100, 20);
        assert_eq!(
            viewport,
            Viewport {
                cursor: 20,
                top: 10
            }
        );
    }

    #[test]
    fn jump_stops_on_the_last_row() {
        let mut viewport = viewport(0, 0);
        viewport.jump(1000, 100, 20);
        assert_eq!(
            viewport,
            Viewport {
                cursor: 99,
                top: 80
            }
        );
    }

    #[test]
    fn scroll_down_moves_the_screen_and_not_the_cursor_far_from_the_top() {
        let mut viewport = viewport(20, 10);
        viewport.scroll_down(1, 100, 20);
        assert_eq!(
            viewport,
            Viewport {
                cursor: 20,
                top: 11
            }
        );
    }

    #[test]
    fn scroll_down_moves_the_cursor_once_it_is_scrolloff_rows_from_the_top() {
        // The cursor is the 10th row of the screen: it stays on its row until it is the 6th, then
        // it moves with the screen.
        let mut viewport = viewport(20, 10);
        viewport.scroll_down(5, 100, 20);
        assert_eq!(
            viewport,
            Viewport {
                cursor: 20,
                top: 15
            }
        );
        viewport.scroll_down(1, 100, 20);
        assert_eq!(
            viewport,
            Viewport {
                cursor: 21,
                top: 16
            }
        );
        viewport.scroll_down(1, 100, 20);
        assert_eq!(
            viewport,
            Viewport {
                cursor: 22,
                top: 17
            }
        );
    }

    #[test]
    fn scroll_down_keeps_the_distance_of_a_cursor_inside_the_scrolloff_margin() {
        // The cursor is the 3rd row of the screen, and doesn't jump to the 6th.
        let mut viewport = viewport(12, 10);
        viewport.scroll_down(1, 100, 20);
        assert_eq!(
            viewport,
            Viewport {
                cursor: 13,
                top: 11
            }
        );
    }

    #[test]
    fn scroll_down_and_up_with_a_cursor_inside_the_scrolloff_margin() {
        let mut viewport = viewport(12, 10);
        viewport.scroll_down(1, 100, 20);
        // Scrolling up leaves the cursor on its row, which is now the 4th row of the screen.
        viewport.scroll_up(1, 20);
        assert_eq!(
            viewport,
            Viewport {
                cursor: 13,
                top: 10
            }
        );
        // Scrolling down again keeps the cursor on the 4th row of the screen.
        viewport.scroll_down(1, 100, 20);
        assert_eq!(
            viewport,
            Viewport {
                cursor: 14,
                top: 11
            }
        );
    }

    #[test]
    fn scroll_down_does_not_move_a_cursor_inside_the_bottom_margin() {
        // The cursor is the 19th row of the screen.
        let mut viewport = viewport(28, 10);
        viewport.scroll_down(1, 100, 20);
        assert_eq!(
            viewport,
            Viewport {
                cursor: 28,
                top: 11
            }
        );
    }

    #[test]
    fn scroll_down_stops_when_the_last_row_is_displayed() {
        let mut viewport = viewport(90, 79);
        viewport.scroll_down(1, 100, 20);
        assert_eq!(
            viewport,
            Viewport {
                cursor: 90,
                top: 80
            }
        );
        viewport.scroll_down(1, 100, 20);
        assert_eq!(
            viewport,
            Viewport {
                cursor: 90,
                top: 80
            }
        );
    }

    #[test]
    fn scroll_down_by_more_lines_than_available_stops_at_the_end() {
        let mut viewport = viewport(50, 40);
        viewport.scroll_down(1000, 100, 20);
        // The cursor was the 11th row of the screen: 5 lines are scrolled with it staying on its
        // row, then it moves with the screen for the 35 remaining ones.
        assert_eq!(
            viewport,
            Viewport {
                cursor: 85,
                top: 80
            }
        );
        viewport.scroll_down(usize::MAX, 100, 20);
        assert_eq!(
            viewport,
            Viewport {
                cursor: 85,
                top: 80
            }
        );
    }

    #[test]
    fn scroll_down_does_nothing_when_everything_fits() {
        let mut viewport = viewport(3, 0);
        viewport.scroll_down(5, 10, 20);
        assert_eq!(viewport, Viewport { cursor: 3, top: 0 });
    }

    #[test]
    fn scroll_down_uses_a_smaller_scrolloff_on_a_small_screen() {
        // The scrolloff is 3 rows on a screen of 7 rows: the cursor is on the 4th row of the
        // screen, so it moves with the screen.
        let mut viewport = viewport(13, 10);
        viewport.scroll_down(1, 100, 7);
        assert_eq!(
            viewport,
            Viewport {
                cursor: 14,
                top: 11
            }
        );
    }

    #[test]
    fn scroll_up_moves_the_screen_and_not_the_cursor_far_from_the_bottom() {
        let mut viewport = viewport(20, 10);
        viewport.scroll_up(1, 20);
        assert_eq!(viewport, Viewport { cursor: 20, top: 9 });
    }

    #[test]
    fn scroll_up_moves_the_cursor_once_it_is_scrolloff_rows_from_the_bottom() {
        // The cursor is the 10th row of the screen: it stays on its row until it is the 15th, then
        // it moves with the screen.
        let mut viewport = viewport(20, 10);
        viewport.scroll_up(4, 20);
        assert_eq!(viewport, Viewport { cursor: 20, top: 6 });
        viewport.scroll_up(1, 20);
        assert_eq!(viewport, Viewport { cursor: 19, top: 5 });
    }

    #[test]
    fn scroll_up_keeps_the_distance_of_a_cursor_inside_the_bottom_margin() {
        // The cursor is the 19th row of the screen.
        let mut viewport = viewport(28, 10);
        viewport.scroll_up(1, 20);
        assert_eq!(viewport, Viewport { cursor: 27, top: 9 });
    }

    #[test]
    fn scroll_up_stops_when_the_first_row_is_displayed() {
        let mut viewport = viewport(10, 1);
        viewport.scroll_up(1, 20);
        assert_eq!(viewport, Viewport { cursor: 10, top: 0 });
        viewport.scroll_up(1, 20);
        assert_eq!(viewport, Viewport { cursor: 10, top: 0 });
    }

    #[test]
    fn scroll_up_by_more_lines_than_available_stops_at_the_beginning() {
        let mut viewport = viewport(20, 10);
        viewport.scroll_up(1000, 20);
        // 4 lines are scrolled with the cursor staying on its row, then it moves with the screen
        // for the 6 remaining ones.
        assert_eq!(viewport, Viewport { cursor: 14, top: 0 });
        viewport.scroll_up(usize::MAX, 20);
        assert_eq!(viewport, Viewport { cursor: 14, top: 0 });
    }

    #[test]
    fn half_page_down_moves_the_screen_and_the_cursor_by_half_the_height() {
        let mut viewport = viewport(20, 10);
        viewport.half_page_down(1, 100, 20);
        assert_eq!(
            viewport,
            Viewport {
                cursor: 30,
                top: 20
            }
        );
    }

    #[test]
    fn half_page_up_moves_the_screen_and_the_cursor_by_half_the_height() {
        let mut viewport = viewport(40, 30);
        viewport.half_page_up(1, 100, 20);
        assert_eq!(
            viewport,
            Viewport {
                cursor: 30,
                top: 20
            }
        );
    }

    #[test]
    fn half_page_rounds_the_half_of_an_odd_height_down() {
        let mut viewport = viewport(20, 10);
        viewport.half_page_down(1, 100, 21);
        assert_eq!(
            viewport,
            Viewport {
                cursor: 30,
                top: 20
            }
        );
        let mut viewport = self::viewport(40, 30);
        viewport.half_page_up(1, 100, 21);
        assert_eq!(
            viewport,
            Viewport {
                cursor: 30,
                top: 20
            }
        );
    }

    #[test]
    fn half_page_is_repeated() {
        let mut viewport = viewport(20, 10);
        viewport.half_page_down(3, 100, 20);
        assert_eq!(
            viewport,
            Viewport {
                cursor: 50,
                top: 40
            }
        );
        let mut viewport = self::viewport(40, 30);
        viewport.half_page_up(2, 100, 20);
        assert_eq!(
            viewport,
            Viewport {
                cursor: 20,
                top: 10
            }
        );
    }

    #[test]
    fn half_page_keeps_the_scrolloff_of_a_cursor_inside_the_margin() {
        // The cursor is the 3rd row of the screen: it is moved with the screen, then the screen
        // scrolls back to keep the scrolloff, like with `j`.
        let mut viewport = viewport(12, 10);
        viewport.half_page_down(1, 100, 20);
        assert_eq!(
            viewport,
            Viewport {
                cursor: 22,
                top: 17
            }
        );
    }

    #[test]
    fn half_page_down_does_nothing_when_the_last_row_is_displayed() {
        [(95, 80), (85, 80), (80, 80)]
            .into_iter()
            .for_each(|(cursor, top)| {
                let mut viewport = viewport(cursor, top);
                viewport.half_page_down(1, 100, 20);
                assert_eq!(viewport, Viewport { cursor, top });
                viewport.half_page_down(usize::MAX, 100, 20);
                assert_eq!(viewport, Viewport { cursor, top });
            });
    }

    #[test]
    fn half_page_up_does_nothing_when_the_first_row_is_displayed() {
        [(0, 0), (5, 0), (10, 0)]
            .into_iter()
            .for_each(|(cursor, top)| {
                let mut viewport = viewport(cursor, top);
                viewport.half_page_up(1, 100, 20);
                assert_eq!(viewport, Viewport { cursor, top });
                viewport.half_page_up(usize::MAX, 100, 20);
                assert_eq!(viewport, Viewport { cursor, top });
            });
    }

    #[test]
    fn half_page_down_moves_the_cursor_by_the_scrolled_rows_when_the_screen_stops() {
        // Only 5 rows are left below the screen.
        let mut viewport = viewport(85, 75);
        viewport.half_page_down(1, 100, 20);
        assert_eq!(
            viewport,
            Viewport {
                cursor: 90,
                top: 80
            }
        );
        let mut viewport = self::viewport(20, 10);
        viewport.half_page_down(usize::MAX, 100, 20);
        assert_eq!(
            viewport,
            Viewport {
                cursor: 90,
                top: 80
            }
        );
    }

    #[test]
    fn half_page_up_moves_the_cursor_by_the_scrolled_rows_when_the_screen_stops() {
        // Only 3 rows are left above the screen.
        let mut viewport = viewport(15, 3);
        viewport.half_page_up(1, 100, 20);
        assert_eq!(viewport, Viewport { cursor: 12, top: 0 });
        let mut viewport = self::viewport(20, 10);
        viewport.half_page_up(usize::MAX, 100, 20);
        assert_eq!(viewport, Viewport { cursor: 10, top: 0 });
    }

    #[test]
    fn half_page_does_nothing_when_everything_fits_or_the_screen_is_one_row_tall() {
        let mut viewport = viewport(3, 0);
        viewport.half_page_down(1, 10, 20);
        assert_eq!(viewport, Viewport { cursor: 3, top: 0 });
        let mut viewport = self::viewport(30, 30);
        viewport.half_page_down(1, 100, 1);
        viewport.half_page_up(1, 100, 1);
        assert_eq!(
            viewport,
            Viewport {
                cursor: 30,
                top: 30
            }
        );
    }
}
