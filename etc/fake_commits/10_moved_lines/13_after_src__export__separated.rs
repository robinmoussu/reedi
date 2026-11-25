pub fn export_with_separator(rows: &[Row], separator: char) -> String {
    let header = columns.iter().map(|column| column.title()).collect::<Vec<_>>();
    let escaped = |field: &str| field.replace('"', "\"\"");
    write_rows(header, rows, escaped, separator)
}
