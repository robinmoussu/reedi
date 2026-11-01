//! Helpers shared by the unit tests and the integration tests.

use ratatui::backend::Backend;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;

use crate::config::UnknownSetting;
use crate::date::DateDisplay;
use crate::fake;
use crate::model::Commit;
use crate::theme::Theme;
use crate::view;
use crate::view::ErrorModal;
use crate::view::LEFT_TO_RIGHT_MARK;
use crate::view::Row;
use crate::viewport::Viewport;

pub fn viewport(cursor: usize, top: usize) -> Viewport {
    Viewport { cursor, top }
}

/// 3 fake commits displayed on at least 6 rows each, for the tests displaying several commits
/// (some of them move the cursor 3 rows below a header, inside the same commit).
pub fn short_commits() -> [Commit; 3] {
    [
        fake::ltr_then_rtl_summary(),
        fake::rtl_summary(),
        fake::rtl_then_ltr_summary(),
    ]
}

/// All the rows of `commits`, on a screen `width` columns wide.
pub fn rows_of(commits: &[Commit], width: u16) -> Vec<Row> {
    view::rows(
        commits,
        width,
        &Theme::monochrome(),
        &DateDisplay::for_snapshots(),
    )
    .unwrap()
}

/// Index of the first row of the header of each commit of `commits`, on a screen `width` columns
/// wide.
pub fn header_row_indexes(commits: &[Commit], width: u16) -> Vec<usize> {
    commits
        .iter()
        .scan(0, |next_header_row_index, commit| {
            let header_row_index = *next_header_row_index;
            *next_header_row_index += rows_of(std::slice::from_ref(commit), width).len();
            Some(header_row_index)
        })
        .collect()
}

/// An error modal displaying 2 unrecognized settings of the config file.
pub fn unknown_settings_error_modal() -> ErrorModal {
    let unknown_setting = |key: &str, value: &str, line_number| UnknownSetting {
        key: key.to_string(),
        value: value.parse().unwrap(),
        line_number,
    };
    ErrorModal::UnknownSettings {
        path: "/home/alice/.config/reedi/config.toml".into(),
        unknown_settings: vec![
            unknown_setting("scrolloff", "3", 3),
            unknown_setting("keys", "{ quit = \"q\", down = \"j\", up = \"k\" }", 5),
        ],
    }
}

/// `buffer`, without the left-to-right marks, which are invisible in the snapshots.
pub fn without_left_to_right_marks(buffer: &Buffer) -> Buffer {
    let mut buffer = buffer.clone();
    for cell in &mut buffer.content {
        let symbol = cell.symbol().replace(LEFT_TO_RIGHT_MARK, "");
        cell.set_symbol(&symbol);
    }
    buffer
}

/// A backend displaying `buffer`, without the left-to-right marks.
pub fn backend_without_left_to_right_marks(buffer: &Buffer) -> TestBackend {
    let buffer = without_left_to_right_marks(buffer);
    let width = buffer.area.width;
    let mut backend = TestBackend::new(width, buffer.area.height);
    let cells = buffer.content.iter().enumerate().map(|(cell_index, cell)| {
        let (x, y) = (
            cell_index % usize::from(width),
            cell_index / usize::from(width),
        );
        (x as u16, y as u16, cell)
    });
    backend.draw(cells).unwrap();
    backend
}
