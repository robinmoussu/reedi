#[test]
fn hours_and_minutes_are_added() {
    let duration = parse_duration("1h30m").unwrap();
    assert_eq!(duration, Duration::from_secs(5400));
}

#[test]
fn unknown_units_are_errors() {
    let error = parse_duration("3 fortnights").unwrap_err();
    assert_eq!(error, ParseError::UnknownUnit(String::from("fortnights")));
}
