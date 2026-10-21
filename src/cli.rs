//! The command line options of `reedi`.

use std::error::Error;
use std::fmt;

use clap::Parser;
use jiff::Timestamp;
use jiff::Zoned;
use jiff::tz::Offset;
use jiff::tz::TimeZone;

use crate::date;

use crate::date::DateFormat;
use crate::date::DateParseError;

/// `reedi` is a review editor: a TUI to review (and edit) unpushed commits.
#[derive(Debug, Parser)]
#[command(version)]
pub struct Cli {
    /// Display the fake commits (the default for now).
    #[arg(long, hide = true)]
    pub fake: bool,
    /// The current date (any format of `git commit --date`), to resolve the relative dates and
    /// the dates without offset of the fake commits, and to display the `relative`, `human` and
    /// `local` date formats.
    #[arg(long, hide = true, requires = "fake")]
    pub fake_now: Option<String>,
    /// The current timezone (IANA name or fixed offset), like `--fake-now`.
    #[arg(long, hide = true, requires = "fake")]
    pub fake_timezone: Option<String>,
    /// How dates are displayed (any value of `git log --date=`). Defaults to the `log.date` git
    /// setting, then to `human`.
    #[arg(long)]
    pub date_format: Option<DateFormat>,
}

/// An option of the command line is invalid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    /// `--fake-timezone` is neither an IANA name nor a fixed offset.
    InvalidTimezone(String),
    /// `--fake-now` is not a date that `git commit --date` accepts.
    InvalidNow(DateParseError),
}

impl Cli {
    /// The current date: `--fake-now` in `--fake-timezone` if they are set, otherwise the date and
    /// timezone of the system.
    pub fn now(&self) -> Result<Zoned, CliError> {
        let timezone = match &self.fake_timezone {
            Some(text) => parse_timezone(text)?,
            None => TimeZone::system(),
        };
        let now = Timestamp::now().to_zoned(timezone.clone());
        let Some(text) = &self.fake_now else {
            return Ok(now);
        };
        let time = date::parse(text, &now).map_err(CliError::InvalidNow)?;
        let timestamp = Timestamp::from_second(time.seconds).map_err(|_| {
            CliError::InvalidNow(DateParseError {
                input: text.clone(),
            })
        })?;
        Ok(timestamp.to_zoned(timezone))
    }
}

/// An IANA name (`Europe/Paris`), or a fixed offset (`+02:00`, `-0530`, `+02`).
fn parse_timezone(text: &str) -> Result<TimeZone, CliError> {
    if let Ok(timezone) = TimeZone::get(text) {
        return Ok(timezone);
    }
    let invalid = || CliError::InvalidTimezone(text.to_string());
    let sign = match text.chars().next() {
        Some('+') => 1,
        Some('-') => -1,
        _ => return Err(invalid()),
    };
    let digits = text[1..].replacen(':', "", 1);
    if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid());
    }
    let (hours, minutes) = match digits.len() {
        2 => (digits.as_str(), "00"),
        4 => digits.split_at(2),
        _ => return Err(invalid()),
    };
    let seconds =
        sign * (hours.parse::<i32>().unwrap() * 3600 + minutes.parse::<i32>().unwrap() * 60);
    Offset::from_seconds(seconds)
        .map(TimeZone::fixed)
        .map_err(|_| invalid())
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            CliError::InvalidTimezone(text) => write!(
                f,
                "invalid --fake-timezone: {text} (expected an IANA name or an offset like +02:00)"
            ),
            CliError::InvalidNow(error) => write!(f, "invalid --fake-now: {error}"),
        }
    }
}

impl Error for CliError {}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;
    use clap::error::ErrorKind;

    use super::*;
    use crate::date::DateStyle;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(["reedi"].iter().chain(args))
    }

    #[test]
    fn no_option() {
        let cli = parse(&[]).unwrap();
        assert!(!cli.fake);
        assert_eq!(cli.date_format, None);
    }

    #[test]
    fn date_format_is_parsed_like_git() {
        let cli = parse(&["--date-format", "iso-local"]).unwrap();
        let iso_local = DateFormat {
            style: DateStyle::Iso,
            local: true,
        };
        assert_eq!(cli.date_format, Some(iso_local));
    }

    #[test]
    fn invalid_date_format_is_an_error() {
        let error = parse(&["--date-format", "bogus"]).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::ValueValidation);
        assert!(error.to_string().contains("unknown date format bogus"));
    }

    #[test]
    fn fake_now_and_timezone_require_fake() {
        let now = parse(&["--fake-now", "yesterday"]).unwrap_err();
        assert_eq!(now.kind(), ErrorKind::MissingRequiredArgument);
        let timezone = parse(&["--fake-timezone", "Europe/Paris"]).unwrap_err();
        assert_eq!(timezone.kind(), ErrorKind::MissingRequiredArgument);
        let fake = [
            "--fake",
            "--fake-now",
            "yesterday",
            "--fake-timezone",
            "UTC",
        ];
        assert!(parse(&fake).is_ok());
    }

    #[test]
    fn only_date_format_is_visible_in_the_help() {
        let help = Cli::command().render_help().to_string();
        assert!(help.contains("--date-format"));
        assert!(!help.contains("--fake"));
    }

    #[test]
    fn fake_now_is_in_the_fake_timezone() {
        let args = [
            "--fake",
            "--fake-now",
            "2026-10-12 14:32:07 +0200",
            "--fake-timezone",
            "America/New_York",
        ];
        let now = parse(&args).unwrap().now().unwrap();
        assert_eq!(now.timestamp().as_second(), 1_791_808_327);
        assert_eq!(now.time_zone().iana_name(), Some("America/New_York"));
    }

    #[test]
    fn fake_now_without_offset_is_in_the_fake_timezone() {
        let args = [
            "--fake",
            "--fake-now",
            "2026-10-12 14:32:07",
            "--fake-timezone",
            "+02:00",
        ];
        let now = parse(&args).unwrap().now().unwrap();
        assert_eq!(now.timestamp().as_second(), 1_791_808_327);
        assert_eq!(now.offset().seconds(), 7200);
    }

    #[test]
    fn invalid_fake_timezone_is_an_error() {
        let args = ["--fake", "--fake-timezone", "Mars/Olympus"];
        assert_eq!(
            parse(&args).unwrap().now(),
            Err(CliError::InvalidTimezone("Mars/Olympus".to_string()))
        );
    }

    #[test]
    fn invalid_fake_now_is_an_error() {
        let args = ["--fake", "--fake-now", "not a date"];
        let invalid = DateParseError {
            input: "not a date".to_string(),
        };
        assert_eq!(
            parse(&args).unwrap().now(),
            Err(CliError::InvalidNow(invalid))
        );
    }
}
