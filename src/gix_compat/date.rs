//! The `relative` and `human` date formats of git (`show_date_relative` and `show_date_normal` in
//! git's `date.c`), which `gix` doesn't have, and the parsing of dates (`approxidate`), which
//! `gix` only partially has.

use std::process::Command;

use gix::date::Time;
use jiff::Timestamp;
use jiff::Zoned;
use jiff::tz::Offset;
use jiff::tz::TimeZone;

use crate::gix_compat::locale::Locale;

/// The English names of the weekdays and months, never translated by git, starting with Sunday
/// and January.
pub const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
pub const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// `time` relative to `now`, like `git log --date=relative`: `3 hours ago`, translated with
/// `locale`.
pub fn relative(time: Time, now: &Zoned, locale: &Locale) -> String {
    let Ok(mut diff) = u64::try_from(now.timestamp().as_second() - time.seconds) else {
        return locale.translate("in the future");
    };
    let count =
        |singular: &str, plural: &str, count: u64| locale.translate_plural(singular, plural, count);
    if diff < 90 {
        return count("%lu second ago", "%lu seconds ago", diff);
    }
    // Minutes.
    diff = (diff + 30) / 60;
    if diff < 90 {
        return count("%lu minute ago", "%lu minutes ago", diff);
    }
    // Hours.
    diff = (diff + 30) / 60;
    if diff < 36 {
        return count("%lu hour ago", "%lu hours ago", diff);
    }
    // Days.
    diff = (diff + 12) / 24;
    if diff < 14 {
        return count("%lu day ago", "%lu days ago", diff);
    }
    // Weeks, for the past 10 weeks or so.
    if diff < 70 {
        return count("%lu week ago", "%lu weeks ago", (diff + 3) / 7);
    }
    // Months, for the past 12 months or so.
    if diff < 365 {
        return count("%lu month ago", "%lu months ago", (diff + 15) / 30);
    }
    // Years and months, for 5 years or so.
    if diff < 1825 {
        let total_months = (diff * 12 * 2 + 365) / (365 * 2);
        let (years, months) = (total_months / 12, total_months % 12);
        let years_text = count("%lu year", "%lu years", years);
        if months == 0 {
            return count("%lu year ago", "%lu years ago", years);
        }
        return count("%s, %lu month ago", "%s, %lu months ago", months).replacen(
            "%s",
            &years_text,
            1,
        );
    }
    // Only years.
    count("%lu year ago", "%lu years ago", (diff + 183) / 365)
}

/// `offset` as git stores it in a number: `+0200` is 200.
pub fn hhmm(offset_seconds: i32) -> i32 {
    let minutes = offset_seconds.abs() / 60;
    offset_seconds.signum() * (minutes / 60 * 100 + minutes % 60)
}

/// `time` displayed like `git log --date=human`, `now` being the current date and timezone:
/// relative for recent dates, otherwise without the parts that are the same as `now`. If `local`,
/// `time` is displayed in the timezone of `now`.
pub fn human(time: Time, now: &Zoned, local: bool, locale: &Locale) -> String {
    let timezone = if local {
        now.time_zone().clone()
    } else {
        Offset::from_seconds(time.offset).map_or(TimeZone::UTC, TimeZone::fixed)
    };
    let date = Timestamp::from_second(time.seconds)
        .unwrap_or_default()
        .to_zoned(timezone);

    let same_year = date.year() == now.year();
    let same_month = same_year && date.month() == now.month();
    let mut hide_date = false;
    let mut hide_weekday = false;
    if same_month && date.day() == now.day() {
        hide_date = true;
        hide_weekday = true;
    } else if same_month && date.day() < now.day() && date.day() + 5 > now.day() {
        // A few days ago: only the weekday.
        hide_date = true;
    }
    // Today: only the relative time.
    if hide_weekday {
        return relative(time, now, locale);
    }

    let hide_timezone =
        local || hhmm(date.offset().seconds()) == hhmm(now.offset().seconds()) || !hide_date;
    let mut text = String::new();
    if same_year {
        let weekday = usize::try_from(date.weekday().to_sunday_zero_offset()).unwrap_or_default();
        text += &format!("{} ", WEEKDAYS[weekday]);
    }
    if !hide_date {
        let month = usize::try_from(date.month() - 1).unwrap_or_default();
        text += &format!("{} {} ", MONTHS[month], date.day());
    }
    if same_year {
        text += &format!("{:02}:{:02}", date.hour(), date.minute());
    } else {
        text.truncate(text.trim_end().len());
        text += &format!(" {}", date.year());
    }
    if !hide_timezone {
        text += &format!(" {:+05}", hhmm(date.offset().seconds()));
    }
    text
}

/// `input` parsed by `git`, like `git commit --date` does (`approxidate` in git's `date.c`, which
/// handles dates like `noon`, or without an offset), or `None` if git refuses it. Relative dates
/// and dates without an offset are resolved with `now` and its timezone.
///
/// `git commit` is the only command to parse a date this way, so a commit is made in a temporary
/// repository.
pub fn parse(input: &str, now: &Zoned) -> Option<Time> {
    let repository = tempfile::tempdir().ok()?;
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(repository.path())
            .env_clear()
            .env("PATH", std::env::var_os("PATH")?)
            .env("HOME", repository.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "reedi")
            .env("GIT_AUTHOR_EMAIL", "reedi@localhost")
            .env("GIT_COMMITTER_NAME", "reedi")
            .env("GIT_COMMITTER_EMAIL", "reedi@localhost")
            .env("TZ", posix_timezone(now))
            .env("GIT_TEST_DATE_NOW", now.timestamp().as_second().to_string())
            .output()
            .ok()
            .filter(|output| output.status.success())
    };
    git(&["init", "--quiet", "--template="])?;
    let date = format!("--date={input}");
    git(&[
        "commit",
        "--quiet",
        "--allow-empty",
        "--no-gpg-sign",
        "--message=fake",
        &date,
    ])?;
    let output = git(&["log", "-1", "--format=%ad", "--date=raw"])?;
    // `$seconds $offset`
    let raw = String::from_utf8(output.stdout).ok()?;
    let (seconds, offset) = raw.trim_end().split_once(' ')?;
    let sign = if offset.starts_with('-') { -1 } else { 1 };
    let hours: i32 = offset.get(1..3)?.parse().ok()?;
    let minutes: i32 = offset.get(3..5)?.parse().ok()?;
    Some(Time {
        seconds: seconds.parse().ok()?,
        offset: sign * (hours * 60 + minutes) * 60,
    })
}

/// The timezone of `now`, as a value of `TZ`: its IANA name, or a POSIX timezone with its fixed
/// offset (whose sign is inverted: `<+0200>-2:00` is UTC+2).
fn posix_timezone(now: &Zoned) -> String {
    if let Some(name) = now.time_zone().iana_name() {
        return name.to_string();
    }
    let seconds = now.offset().seconds();
    let minutes = seconds.abs() / 60;
    let sign = if seconds < 0 { '-' } else { '+' };
    let inverted = if seconds < 0 { '+' } else { '-' };
    let (hours, minutes) = (minutes / 60, minutes % 60);
    format!("<{sign}{hours:02}{minutes:02}>{inverted}{hours}:{minutes:02}")
}
