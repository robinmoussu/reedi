//! Dates, parsed and displayed like git does.

use std::error::Error;
use std::fmt;
use std::str::FromStr;

use gix::date::Time;
use jiff::Zoned;
use jiff::tz::Offset;
#[cfg(any(test, feature = "test-support"))]
use jiff::tz::TimeZone;

use crate::gix_compat;
use crate::gix_compat::date::DisplayedIn;
use crate::gix_compat::date::RAW_FORMAT;
use crate::gix_compat::date::to_zoned;
use crate::gix_compat::locale::Locale;

/// Prefix of the date formats that depend on whether the output is a terminal.
const AUTO_PREFIX: &str = "auto:";

/// How a date is displayed: the values of `git log --date=`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DateFormat {
    pub style: DateStyle,
    pub displayed_in: DisplayedIn,
}

/// The formats of `git log --date=`, without their `-local` suffix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DateStyle {
    /// `Mon Oct 12 14:32:07 2026 +0200` (`default`, and `local` which is `default-local`).
    Default,
    /// `2026-10-12 14:32:07 +0200` (`iso`, `iso8601`).
    Iso,
    /// `2026-10-12T14:32:07+02:00` (`iso-strict`, `iso8601-strict`).
    IsoStrict,
    /// `Mon, 12 Oct 2026 14:32:07 +0200` (`rfc`, `rfc2822`).
    Rfc,
    /// `2026-10-12`.
    Short,
    /// `3 hours ago`.
    Relative,
    /// `3 hours ago`, `Mon 14:32`, `Oct 12 2026`, … depending on how far the date is.
    Human,
    /// `1791808327`.
    Unix,
    /// `1791808327 +0200`.
    Raw,
    /// `format:<strftime>`.
    Strftime(String),
}

impl DateFormat {
    /// The format used when neither `--date-format` nor the `log.date` git setting is set.
    pub const HUMAN: DateFormat = DateFormat {
        style: DateStyle::Human,
        displayed_in: DisplayedIn::OriginalOffset,
    };
}

/// A value of `git log --date=` that git doesn't accept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DateFormatError {
    Unknown(String),
    /// `format` (or `format-local`) without `:<strftime>`.
    MissingColon(String),
}

impl fmt::Display for DateFormatError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            DateFormatError::Unknown(format) => write!(f, "unknown date format {format}"),
            DateFormatError::MissingColon(format) => {
                write!(f, "date format missing colon separator: {format}")
            }
        }
    }
}

impl Error for DateFormatError {}

impl FromStr for DateFormat {
    type Err = DateFormatError;

    /// Parses the values of `git log --date=`. `auto:<format>` is always `<format>`, since the
    /// output is always a terminal.
    fn from_str(format: &str) -> Result<Self, Self::Err> {
        let unknown = || DateFormatError::Unknown(format.to_string());
        let name = format.strip_prefix(AUTO_PREFIX).unwrap_or(format);
        // Before git 2.7.0, `local` was a format of its own. The `-local` suffix replaced it, and
        // `local` was kept as an alias of `default-local`.
        let name = if name == "local" {
            "default-local"
        } else {
            name
        };
        if let Some(pattern) = name.strip_prefix("format-local:") {
            return Ok(DateFormat {
                style: DateStyle::Strftime(pattern.to_string()),
                displayed_in: DisplayedIn::LocalTimezone,
            });
        }
        if let Some(pattern) = name.strip_prefix("format:") {
            return Ok(DateFormat {
                style: DateStyle::Strftime(pattern.to_string()),
                displayed_in: DisplayedIn::OriginalOffset,
            });
        }
        if name == "format" || name == "format-local" {
            return Err(DateFormatError::MissingColon(format.to_string()));
        }
        let (name, displayed_in) = match name.strip_suffix("-local") {
            Some(name) => (name, DisplayedIn::LocalTimezone),
            None => (name, DisplayedIn::OriginalOffset),
        };
        let style = match name {
            "default" => DateStyle::Default,
            "iso" | "iso8601" => DateStyle::Iso,
            "iso-strict" | "iso8601-strict" => DateStyle::IsoStrict,
            "rfc" | "rfc2822" => DateStyle::Rfc,
            "short" => DateStyle::Short,
            "relative" => DateStyle::Relative,
            "human" => DateStyle::Human,
            "unix" => DateStyle::Unix,
            "raw" => DateStyle::Raw,
            _ => return Err(unknown()),
        };
        Ok(DateFormat { style, displayed_in })
    }
}

/// A date that `git commit --date` doesn't accept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DateParseError {
    pub input: String,
}

impl fmt::Display for DateParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid date: {}", self.input)
    }
}

impl Error for DateParseError {}

/// What displaying a date depends on, besides its format.
#[derive(Debug)]
pub struct Environment {
    /// The current date, in the current timezone.
    pub now: Zoned,
    /// Used by the `strftime` formats, and to translate the `relative` and `human` formats.
    pub locale: Locale,
}

/// How the dates are displayed.
#[derive(Debug)]
pub struct DateDisplay {
    pub format: DateFormat,
    pub environment: Environment,
}

impl DateDisplay {
    /// `time`, displayed with `format`.
    pub fn display(&self, time: Time) -> String {
        display(time, &self.format, &self.environment)
    }

    /// The display used by the snapshots: `dd/mm/yyyy`, so that they don't depend on the current
    /// date, nor on the locale.
    #[cfg(any(test, feature = "test-support"))]
    pub fn for_snapshots() -> Self {
        DateDisplay {
            format: DateFormat {
                style: DateStyle::Strftime("%d/%m/%Y".to_string()),
                displayed_in: DisplayedIn::OriginalOffset,
            },
            environment: Environment { now: snapshot_now(), locale: Locale::c() },
        }
    }
}

/// The current date used by the snapshots.
#[cfg(any(test, feature = "test-support"))]
fn snapshot_now() -> Zoned {
    jiff::civil::date(2026, 10, 12)
        .at(
            /* hour */ 14, /* minute */ 32, /* second */ 7,
            /* subsec_nanosecond */ 0,
        )
        .to_zoned(TimeZone::UTC)
        .unwrap()
}

/// `input`, parsed like `git commit --date` does. Relative dates (like `yesterday`) and dates
/// without an offset are resolved with `now` and its timezone.
pub fn parse(input: &str, now: &Zoned) -> Result<Time, DateParseError> {
    gix_compat::date::parse(input, now).ok_or_else(|| DateParseError { input: input.to_string() })
}

/// `time`, displayed with `format` like `git log --date=` does.
pub fn display(time: Time, format: &DateFormat, environment: &Environment) -> String {
    let now = &environment.now;
    let zoned = to_zoned(time, now, format.displayed_in);
    // The names of the weekdays and months of `jiff` are in English, like the ones of git, which
    // never translates them.
    let formatted = |format: &str| zoned.strftime(format).to_string();
    match &format.style {
        DateStyle::Unix => time.seconds.to_string(),
        DateStyle::Raw => formatted(RAW_FORMAT),
        DateStyle::Relative => gix_compat::date::relative(time, now, &environment.locale),
        DateStyle::Human => {
            gix_compat::date::human(time, now, format.displayed_in, &environment.locale)
        }
        DateStyle::Short => formatted("%Y-%m-%d"),
        DateStyle::Iso => formatted("%Y-%m-%d %H:%M:%S %z"),
        DateStyle::IsoStrict if zoned.offset() == Offset::UTC => formatted("%Y-%m-%dT%H:%M:%SZ"),
        DateStyle::IsoStrict => formatted("%Y-%m-%dT%H:%M:%S%:z"),
        DateStyle::Rfc => formatted("%a, %-d %b %Y %H:%M:%S %z"),
        DateStyle::Default if format.displayed_in == DisplayedIn::LocalTimezone => {
            formatted("%a %b %-d %H:%M:%S %Y")
        }
        DateStyle::Default => formatted("%a %b %-d %H:%M:%S %Y %z"),
        DateStyle::Strftime(pattern) => {
            environment
                .locale
                .strftime(pattern, &zoned, format.displayed_in)
        }
    }
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;
    use jiff::tz::TimeZone;

    use super::*;
    use crate::test_git::Git;
    use DisplayedIn::LocalTimezone;
    use DisplayedIn::OriginalOffset;

    /// `2026-10-12 14:32:07 +0200`, the date used by all the display tests.
    const TIME: Time = Time { seconds: 1_791_808_327, offset: 7200 };

    /// `now` (RFC 3339) in `timezone` (IANA name).
    fn zoned(now: &str, timezone: &str) -> Zoned {
        let timezone = TimeZone::get(timezone).unwrap();
        now.parse::<Timestamp>().unwrap().to_zoned(timezone)
    }

    /// `now` (RFC 3339) in `timezone` (IANA name), with the C locale.
    fn environment(now: &str, timezone: &str) -> Environment {
        Environment {
            now: zoned(now, timezone),
            locale: Locale::c(),
        }
    }

    fn format(style: DateStyle, displayed_in: DisplayedIn) -> DateFormat {
        DateFormat { style, displayed_in }
    }

    fn strftime(pattern: &str, displayed_in: DisplayedIn) -> DateFormat {
        format(DateStyle::Strftime(pattern.to_string()), displayed_in)
    }

    /// A repository where the current date is `now` in `timezone`, whose only commit is at `TIME`.
    fn repository_with_a_commit_at_time(now: &str, timezone: &str, locale: &str) -> Git {
        let git = Git::new(&zoned(now, timezone), locale);
        git.commit_at(TIME);
        git
    }

    #[test]
    fn date_formats_of_git_are_accepted() {
        let formats = [
            ("default", format(DateStyle::Default, OriginalOffset)),
            ("default-local", format(DateStyle::Default, LocalTimezone)),
            ("local", format(DateStyle::Default, LocalTimezone)),
            ("iso", format(DateStyle::Iso, OriginalOffset)),
            ("iso-local", format(DateStyle::Iso, LocalTimezone)),
            ("iso8601", format(DateStyle::Iso, OriginalOffset)),
            ("iso8601-local", format(DateStyle::Iso, LocalTimezone)),
            ("iso-strict", format(DateStyle::IsoStrict, OriginalOffset)),
            (
                "iso-strict-local",
                format(DateStyle::IsoStrict, LocalTimezone),
            ),
            (
                "iso8601-strict",
                format(DateStyle::IsoStrict, OriginalOffset),
            ),
            ("rfc", format(DateStyle::Rfc, OriginalOffset)),
            ("rfc-local", format(DateStyle::Rfc, LocalTimezone)),
            ("rfc2822", format(DateStyle::Rfc, OriginalOffset)),
            ("short", format(DateStyle::Short, OriginalOffset)),
            ("short-local", format(DateStyle::Short, LocalTimezone)),
            ("relative", format(DateStyle::Relative, OriginalOffset)),
            ("relative-local", format(DateStyle::Relative, LocalTimezone)),
            ("human", format(DateStyle::Human, OriginalOffset)),
            ("human-local", format(DateStyle::Human, LocalTimezone)),
            ("unix", format(DateStyle::Unix, OriginalOffset)),
            ("unix-local", format(DateStyle::Unix, LocalTimezone)),
            ("raw", format(DateStyle::Raw, OriginalOffset)),
            ("raw-local", format(DateStyle::Raw, LocalTimezone)),
            ("format:%Y %Z", strftime("%Y %Z", OriginalOffset)),
            ("format-local:%Y %H %Z", strftime("%Y %H %Z", LocalTimezone)),
        ];
        let git =
            repository_with_a_commit_at_time("2026-10-12T17:32:07+02:00", "Europe/Paris", "C");
        for (text, expected) in formats {
            assert!(git.author_date(Some(text)).is_ok(), "git --date={text}");
            assert_eq!(text.parse(), Ok(expected), "--date={text}");
        }
    }

    #[test]
    fn auto_date_format_is_the_format_itself() {
        assert_eq!(
            "auto:iso".parse(),
            Ok(format(DateStyle::Iso, OriginalOffset))
        );
        assert_eq!("auto:human".parse(), Ok(DateFormat::HUMAN));
        assert_eq!("auto:format:%Y".parse(), Ok(strftime("%Y", OriginalOffset)));
    }

    /// The date formats that git refuses are errors, worded like git.
    #[test]
    fn date_formats_refused_by_git_are_errors() {
        let git =
            repository_with_a_commit_at_time("2026-10-12T17:32:07+02:00", "Europe/Paris", "C");
        let refused = [
            (
                "local-local",
                DateFormatError::Unknown("local-local".to_string()),
            ),
            ("normal", DateFormatError::Unknown("normal".to_string())),
            ("bogus", DateFormatError::Unknown("bogus".to_string())),
            ("ISO", DateFormatError::Unknown("ISO".to_string())),
            ("", DateFormatError::Unknown(String::new())),
            (
                "format",
                DateFormatError::MissingColon("format".to_string()),
            ),
            (
                "format-local",
                DateFormatError::MissingColon("format-local".to_string()),
            ),
        ];
        for (text, expected) in refused {
            let git_error = git.author_date(Some(text)).unwrap_err();
            assert_eq!(
                git_error,
                format!("fatal: {expected}\n"),
                "git --date={text}"
            );
            assert_eq!(text.parse::<DateFormat>(), Err(expected), "--date={text}");
        }
    }

    /// `date` (`%Y-%m-%d %H:%M:%S %z`).
    fn time(date: &str) -> Time {
        let zoned = Zoned::strptime("%Y-%m-%d %H:%M:%S %z", date).unwrap();
        Time {
            seconds: zoned.timestamp().as_second(),
            offset: zoned.offset().seconds(),
        }
    }

    /// The expected dates are the ones of `TZ=Europe/Paris GIT_TEST_DATE_NOW=… git commit
    /// --date=$input`, then `git log --date=raw`, or `None` if git refuses `input`.
    #[test]
    fn dates_are_parsed_like_git_commit() {
        let now = zoned("2026-10-21T02:30:00+02:00", "Europe/Paris");
        let dates = [
            (
                "2026-10-12 14:32:07 +0200",
                Some("2026-10-12 14:32:07 +0200"),
            ),
            (
                "2026-10-12T14:32:07+02:00",
                Some("2026-10-12 14:32:07 +0200"),
            ),
            (
                "Mon, 12 Oct 2026 14:32:07 +0200",
                Some("2026-10-12 14:32:07 +0200"),
            ),
            ("1791808327 +0200", Some("2026-10-12 14:32:07 +0200")),
            ("@1791808327 +0200", Some("2026-10-12 14:32:07 +0200")),
            // Without offset: in the timezone of `now`.
            ("2026-10-12 14:32:07", Some("2026-10-12 14:32:07 +0200")),
            // Month first, and in winter time.
            ("12/10/2026 14:32", Some("2026-12-10 14:32:00 +0100")),
            ("yesterday", Some("2026-10-20 02:30:00 +0200")),
            ("3 hours ago", Some("2026-10-20 23:30:00 +0200")),
            ("2 weeks ago", Some("2026-10-07 02:30:00 +0200")),
            // Noon is not reached yet today: the one of yesterday.
            ("noon", Some("2026-10-20 12:00:00 +0200")),
            ("not a date", None),
        ];
        for (input, expected) in dates {
            let expected = match expected {
                Some(date) => Ok(time(date)),
                None => Err(DateParseError { input: input.to_string() }),
            };
            assert_eq!(parse(input, &now), expected, "--date={input}");
        }
    }

    /// Compared with `LC_ALL=C TZ=America/New_York git log --date=$format`.
    #[test]
    fn dates_are_displayed_like_git_log() {
        let now = "2026-10-12T17:32:07+02:00";
        let git = repository_with_a_commit_at_time(now, "America/New_York", "C");
        let environment = environment(now, "America/New_York");
        let formats = [
            "default",
            "local",
            "iso",
            "iso-local",
            "iso-strict",
            "iso-strict-local",
            "rfc",
            "rfc-local",
            "short",
            "short-local",
            "relative",
            "relative-local",
            "human",
            "unix",
            "unix-local",
            "raw",
            "raw-local",
            "format:%d/%m/%Y",
            // `%Z` is only known in the current timezone.
            "format:%Y %Z",
            "format-local:%Y %H %Z",
            "format:%A %d %B %c",
        ];
        for text in formats {
            let format = text.parse().unwrap();
            assert_eq!(
                display(TIME, &format, &environment),
                git.author_date(Some(text)).unwrap(),
                "--date={text}"
            );
        }
    }

    /// Compared with `LC_ALL=C GIT_TEST_DATE_NOW=… git log --date=relative`, and `--date=human`,
    /// in `Europe/Paris` then in `America/New_York`.
    #[test]
    fn relative_and_human_dates_depend_on_now_like_git_log() {
        let nows = [
            // Seconds, minutes and hours ago.
            "2026-10-12T14:32:07+02:00",
            "2026-10-12T14:32:52+02:00",
            "2026-10-12T14:40:07+02:00",
            "2026-10-12T23:00:00+02:00",
            // Days, weeks, months and years ago.
            "2026-10-14T10:00:00+02:00",
            "2026-10-20T10:00:00+02:00",
            "2026-11-30T10:00:00+02:00",
            "2027-03-01T10:00:00+02:00",
            "2028-04-12T10:00:00+02:00",
            "2031-10-12T10:00:00+02:00",
            // Before the date, within and beyond a day.
            "2026-10-12T10:00:00+02:00",
            "2026-10-01T10:00:00+02:00",
        ];
        let formats = ["relative", "human"];
        for now in nows {
            for timezone in ["Europe/Paris", "America/New_York"] {
                let git = repository_with_a_commit_at_time(now, timezone, "C");
                let environment = environment(now, timezone);
                for text in formats {
                    let format = text.parse().unwrap();
                    assert_eq!(
                        display(TIME, &format, &environment),
                        git.author_date(Some(text)).unwrap(),
                        "--date={text}, now: {now} in {timezone}"
                    );
                }
            }
        }
    }

    /// Compared with `LC_ALL=fr_FR.UTF-8 git log --date=$format` (which is in English if the
    /// translations of git are not installed). Skipped if the French locale is not installed.
    #[test]
    fn dates_are_translated_like_git_log() {
        let Some(locale) = Locale::named("fr_FR.UTF-8") else {
            eprintln!("skipped: the fr_FR.UTF-8 locale is not installed");
            return;
        };
        let now = "2026-10-12T17:32:07+02:00";
        let git = repository_with_a_commit_at_time(now, "Europe/Paris", "fr_FR.UTF-8");
        let environment = Environment { now: zoned(now, "Europe/Paris"), locale };
        let formats = ["default", "rfc", "relative", "human", "format:%A %d %B %c"];
        for text in formats {
            let format = text.parse().unwrap();
            assert_eq!(
                display(TIME, &format, &environment),
                git.author_date(Some(text)).unwrap(),
                "--date={text}"
            );
        }
    }
}
