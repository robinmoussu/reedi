pub fn export_csv(rows: &[Row]) -> String {
    export_with_separator(rows, ',')
}
