/// Parse a duration like `1h30m`.
///
/// ```
/// let duration = parse_duration("1h30m").unwrap();
/// assert_eq!(duration, Duration::from_secs(5400));
/// ```
///
/// An unknown unit is an error:
///
/// ```
/// let error = parse_duration("3 fortnights").unwrap_err();
/// assert_eq!(error, ParseError::UnknownUnit("fortnights".to_string()));
/// ```
pub fn parse_duration(text: &str) -> Result<Duration, ParseError> {
    parse_components(text).map(|components| components.into_iter().sum())
}
