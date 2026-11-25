/// Split `line` into rows of at most `width` columns.
pub fn wrap(line: &str, width: usize) -> Vec<&str> {
    let mut rows = Vec::new();
    let mut rest = line;
    while rest.len() > width {
        let (row, next) = rest.split_at(width);
        rows.push(row);
        rest = next;
    }
    rows.push(rest);
    rows
}
