pub fn export_csv(rows: &[Row]) -> String {
    let header = columns.iter().map(|column| column.title()).collect::<Vec<_>>();
    let escaped = |field: &str| field.replace('"', "\"\"");
    write_rows(header, rows, escaped, ',')
}
