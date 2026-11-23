//! The command line options of `reedi`.

use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use clap::ArgGroup;
use clap::CommandFactory;
use clap::Parser;
use jiff::Timestamp;
use jiff::Zoned;
use jiff::fmt::strtime;
use jiff::tz::TimeZone;
use modalkit_key::parse::parse_macro_str;
use ratatui::crossterm::event::KeyEvent;

use crate::date;

use crate::date::DateFormat;
use crate::date::DateParseError;

/// The options displaying fake data, which exclude each other.
const FAKE_DATA_OPTIONS: &str = "fake_data";

/// `reedi` is a review editor: a TUI to review (and edit) unpushed commits, or uncommitted changes.
#[derive(Debug, Parser)]
#[command(version, group(ArgGroup::new(FAKE_DATA_OPTIONS).args(["fake", "fake_dirty_directory"])))]
pub struct Cli {
    /// Display the fake commits instead of those of the repository.
    #[arg(long, hide = true)]
    pub fake: bool,
    /// Display the fake index and working directory instead of those of the repository.
    #[arg(long, hide = true)]
    pub fake_dirty_directory: bool,
    /// The current date (any format of `git commit --date`), used by the `relative` and `human`
    /// date formats.
    #[arg(long, hide = true, requires = FAKE_DATA_OPTIONS)]
    pub fake_now: Option<String>,
    /// The current timezone (IANA name or fixed offset), used by the `-local` and `human` date
    /// formats, and by `--fake-now` when it has no offset.
    #[arg(long, hide = true, requires = FAKE_DATA_OPTIONS)]
    pub fake_timezone: Option<String>,
    /// How dates are displayed (any value of `git log --date=`). Defaults to the `log.date` git
    /// setting, then to `human`.
    #[arg(long)]
    pub date_format: Option<DateFormat>,
    /// Keys pressed once the rows are displayed, before those of the terminal, in Vim's key
    /// notation (`3jG<C-d>q`).
    #[arg(long, hide = true)]
    pub send_keys: Option<Keys>,
    /// Print the help of the hidden options.
    #[arg(long, hide = true)]
    pub help_hidden: bool,
    /// Write to this file, in milliseconds, the time from the start to the first full screen, and
    /// the duration of each refresh after a key.
    #[arg(long, hide = true)]
    pub timing: Option<PathBuf>,
    /// Draw after every key, even when the next key is already waiting, so that `--timing`
    /// measures every refresh.
    #[arg(long, hide = true)]
    pub draw_every_key: bool,
}

/// The help of the hidden options only.
pub fn hidden_help() -> String {
    Cli::command()
        .disable_help_flag(true)
        .disable_version_flag(true)
        .help_template("Hidden options:\n{options}")
        .mut_args(|arg| {
            let is_hidden = arg.is_hide_set();
            arg.hide(!is_hidden)
        })
        .render_help()
        .to_string()
}

/// A sequence of keys, written in Vim's key notation (`3jG<C-d>q`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Keys(pub Vec<KeyEvent>);

impl FromStr for Keys {
    type Err = InvalidKeys;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (_, keys) =
            parse_macro_str(text).map_err(|_| InvalidKeys { input: text.to_string() })?;
        Ok(Keys(keys.into_iter().map(KeyEvent::from).collect()))
    }
}

/// A text is not a sequence of keys in Vim's key notation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidKeys {
    pub input: String,
}

impl fmt::Display for InvalidKeys {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "invalid keys (expected Vim's key notation, like 3jG<C-d>q): {}",
            self.input
        )
    }
}

impl Error for InvalidKeys {}

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
        let timestamp = Timestamp::from_second(time.seconds)
            .map_err(|_| CliError::InvalidNow(DateParseError { input: text.clone() }))?;
        Ok(timestamp.to_zoned(timezone))
    }
}

/// An IANA name (`Europe/Paris`), or a fixed offset (`+02:00`, `-0530`, `+02`).
fn parse_timezone(text: &str) -> Result<TimeZone, CliError> {
    if let Ok(timezone) = TimeZone::get(text) {
        return Ok(timezone);
    }
    // `%z` parses `-0530`, and `%:::z` parses `+02:00` and `+02`.
    ["%z", "%:::z"]
        .into_iter()
        .find_map(|format| strtime::parse(format, text).ok()?.offset())
        .map(TimeZone::fixed)
        .ok_or_else(|| CliError::InvalidTimezone(text.to_string()))
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
    use clap::error::ErrorKind;
    use ratatui::crossterm::event::KeyCode;
    use ratatui::crossterm::event::KeyModifiers;

    use super::*;
    use crate::date::DateStyle;
    use reedi_gix_compat::date::DisplayedIn;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(["reedi"].iter().chain(args))
    }

    #[test]
    fn options_are_unset_by_default() {
        let cli = parse(&[]).unwrap();
        assert!(!cli.fake);
        assert!(!cli.fake_dirty_directory);
        assert_eq!(cli.date_format, None);
        assert_eq!(cli.send_keys, None);
        assert_eq!(cli.timing, None);
        assert!(!cli.draw_every_key);
    }

    #[test]
    fn timing_is_the_path_of_a_file() {
        let cli = parse(&["--timing", "timing.json"]).unwrap();
        assert_eq!(cli.timing, Some(PathBuf::from("timing.json")));
    }

    #[test]
    fn date_format_is_parsed_like_git() {
        let cli = parse(&["--date-format", "iso-local"]).unwrap();
        let iso_local = DateFormat {
            style: DateStyle::Iso,
            displayed_in: DisplayedIn::LocalTimezone,
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
    fn fake_dirty_directory_conflicts_with_fake() {
        let error = parse(&["--fake", "--fake-dirty-directory"]).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::ArgumentConflict);
    }

    #[test]
    fn fake_now_and_timezone_require_a_fake_option() {
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
        let fake_dirty_directory = [
            "--fake-dirty-directory",
            "--fake-now",
            "yesterday",
            "--fake-timezone",
            "UTC",
        ];
        assert!(parse(&fake_dirty_directory).is_ok());
    }

    #[test]
    fn only_date_format_is_visible_in_the_help() {
        let help = Cli::command().render_help().to_string();
        assert!(help.contains("--date-format"));
        assert!(!help.contains("--fake"));
        assert!(!help.contains("--send-keys"));
    }

    #[test]
    fn hidden_help_contains_only_the_hidden_options() {
        let help = hidden_help();
        Cli::command().get_arguments().for_each(|arg| {
            let option = format!("--{}", arg.get_long().unwrap());
            assert_eq!(
                help.contains(&option),
                arg.is_hide_set(),
                "{option} in:\n{help}"
            );
        });
        assert!(!help.contains("--help "));
        assert!(!help.contains("--version"));
    }

    #[test]
    fn hidden_help_is_only_the_list_of_hidden_options() {
        assert!(hidden_help().starts_with("Hidden options:\n      --"));
    }

    #[test]
    fn send_keys_are_parsed_like_vim() {
        let cli = parse(&["--send-keys", "3G<C-d>q"]).unwrap();
        let key = |c, modifiers| KeyEvent::new(KeyCode::Char(c), modifiers);
        let keys = vec![
            key('3', KeyModifiers::NONE),
            key('G', KeyModifiers::NONE),
            key('d', KeyModifiers::CONTROL),
            key('q', KeyModifiers::NONE),
        ];
        assert_eq!(cli.send_keys, Some(Keys(keys)));
    }

    /// Any other text is a sequence of keys: like in Vim, an unknown `<…>` is the keys of its
    /// characters.
    #[test]
    fn empty_send_keys_is_an_error() {
        let error = parse(&["--send-keys", ""]).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::ValueValidation);
        assert!(error.to_string().contains("invalid keys"));
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
        let invalid = DateParseError { input: "not a date".to_string() };
        assert_eq!(
            parse(&args).unwrap().now(),
            Err(CliError::InvalidNow(invalid))
        );
    }
}
