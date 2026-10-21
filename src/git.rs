//! The settings of `reedi` read from the git configuration.

use std::error::Error;
use std::fmt;
use std::path::Path;

use gix::bstr::BString;
use gix::bstr::ByteSlice;

use crate::date::DateFormat;
use crate::date::DateFormatError;

/// The key of the setting of the format of dates.
const LOG_DATE_KEY: &str = "log.date";

/// The git configuration cannot be read, or one of its values is invalid.
#[derive(Debug)]
pub enum GitConfigError {
    Unreadable(String),
    /// The value of `log.date`.
    InvalidDateFormat(DateFormatError),
}

impl fmt::Display for GitConfigError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            GitConfigError::Unreadable(reason) => {
                write!(f, "cannot read the git configuration: {reason}")
            }
            GitConfigError::InvalidDateFormat(error) => {
                write!(f, "invalid `log.date` in the git configuration: {error}")
            }
        }
    }
}

impl Error for GitConfigError {}

/// The `log.date` setting, read like `git log` does from `directory`: from the configuration of
/// its repository, or only the global and system configuration if it is not in a repository.
pub fn log_date(directory: &Path) -> Result<Option<DateFormat>, GitConfigError> {
    match gix::discover(directory) {
        Ok(repository) => log_date_of(&repository),
        Err(error) if error.is_not_found() => {
            let config = gix::config::File::from_globals()
                .map_err(|error| GitConfigError::Unreadable(error.to_string()))?;
            parse_log_date(config.string(LOG_DATE_KEY))
        }
        Err(error) => Err(GitConfigError::Unreadable(error.to_string())),
    }
}

/// The `log.date` setting of the configuration of `repository`.
fn log_date_of(repository: &gix::Repository) -> Result<Option<DateFormat>, GitConfigError> {
    parse_log_date(repository.config_snapshot().string(LOG_DATE_KEY))
}

fn parse_log_date(value: Option<BString>) -> Result<Option<DateFormat>, GitConfigError> {
    value
        .map(|value| {
            value
                .to_str_lossy()
                .parse()
                .map_err(GitConfigError::InvalidDateFormat)
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use gix::date::Time;
    use jiff::Timestamp;
    use jiff::tz::TimeZone;

    use super::*;
    use crate::date::DateStyle;
    use crate::date::Environment;
    use crate::date::display;
    use crate::gix_compat::locale::Locale;
    use crate::test_git::Git;

    /// `2026-10-12 14:32:07 +0200`.
    const TIME: Time = Time {
        seconds: 1_791_808_327,
        offset: 7200,
    };

    /// A new repository whose only commit is at `TIME` and whose configuration has
    /// `log.date = $value` (if any), opened without the global and system configuration (so that
    /// the tests don't depend on the configuration of the user).
    fn repository(value: Option<&str>) -> (Git, gix::Repository) {
        let now: Timestamp = "2026-10-12T17:32:07+02:00".parse().unwrap();
        let git = Git::new(
            &now.to_zoned(TimeZone::get("America/New_York").unwrap()),
            "C",
        );
        git.commit_at(TIME);
        if let Some(value) = value {
            git.set_config("log.date", value);
        }
        let repository = gix::open_opts(git.path(), gix::open::Options::isolated()).unwrap();
        (git, repository)
    }

    /// `log.date` is interpreted like `git log` does if it is read as the format of the dates it
    /// displays.
    #[test]
    fn log_date_is_read_like_git_log() {
        let values = [
            "iso-local",
            "rfc",
            "short",
            "unix",
            "raw",
            "format:%d/%m/%Y",
        ];
        values.into_iter().for_each(|value| {
            let (git, repository) = repository(Some(value));
            let format = log_date_of(&repository).unwrap().unwrap();
            let environment = Environment {
                now: "2026-10-12T17:32:07+02:00"
                    .parse::<Timestamp>()
                    .unwrap()
                    .to_zoned(TimeZone::get("America/New_York").unwrap()),
                locale: Locale::c(),
            };
            assert_eq!(
                display(TIME, &format, &environment),
                git.author_date(None).unwrap(),
                "log.date = {value}"
            );
        });
    }

    #[test]
    fn log_date_is_read_from_the_repository() {
        let (_git, repository) = repository(Some("iso-local"));
        let iso_local = DateFormat {
            style: DateStyle::Iso,
            local: true,
        };
        assert_eq!(log_date_of(&repository).unwrap(), Some(iso_local));
    }

    #[test]
    fn log_date_is_unset() {
        let (_git, repository) = repository(None);
        assert_eq!(log_date_of(&repository).unwrap(), None);
    }

    /// An invalid `log.date` is refused by `git log` too.
    #[test]
    fn invalid_log_date_is_an_error() {
        let (git, repository) = repository(Some("bogus"));
        assert!(git.author_date(None).is_err());
        let error = log_date_of(&repository).unwrap_err();
        assert!(matches!(
            error,
            GitConfigError::InvalidDateFormat(DateFormatError::Unknown(_))
        ));
    }
}
