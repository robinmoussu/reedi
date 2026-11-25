/// Parse a duration like `1h30m`.
pub fn parse_duration(text: &str) -> Result<Duration, ParseError> {
    parse_components(text).map(|components| components.into_iter().sum())
}
