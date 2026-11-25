pub fn export_tsv(rows: &[Row]) -> String {
    export_with_separator(rows, '\t')
}
