pub fn export_tsv(rows: &[Row]) -> String {
    let header = columns.iter().map(|column| column.title()).collect::<Vec<_>>();
    let escaped = |field: &str| field.replace('"', "\"\"");
    write_rows(header, rows, escaped, '\t')
}
