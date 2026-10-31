//! The `relative` and `human` date formats of git (`show_date_relative` and `show_date_normal` in
//! git's `date.c`), which `gix` doesn't have, and the parsing of dates (`approxidate`), which
//! `gix` only partially has.

use std::process::Command;

use gix::date::Time;
use jiff::Timestamp;
use jiff::Zoned;
use jiff::fmt::strtime;
use jiff::tz::Offset;
use jiff::tz::TimeZone;

use crate::gix_compat::locale::Locale;

/// The `strftime` format of a date as git stores it (`--date=raw`): `1791808327 +0200`.
pub const RAW_FORMAT: &str = "%s %z";

/// `time` relative to `now`, like `git log --date=relative`: `3 hours ago`, translated with
/// `locale`.
pub fn relative(time: Time, now: &Zoned, locale: &Locale) -> String {
    let Ok(seconds) = u64::try_from(now.timestamp().as_second() - time.seconds) else {
        return locale.translate("in the future");
    };
    let translated =
        |singular: &str, plural: &str, count: u64| locale.translate_plural(singular, plural, count);
    if seconds < 90 {
        return translated("%lu second ago", "%lu seconds ago", seconds);
    }
    let minutes = (seconds + 30) / 60;
    if minutes < 90 {
        return translated("%lu minute ago", "%lu minutes ago", minutes);
    }
    let hours = (minutes + 30) / 60;
    if hours < 36 {
        return translated("%lu hour ago", "%lu hours ago", hours);
    }
    let days = (hours + 12) / 24;
    if days < 14 {
        return translated("%lu day ago", "%lu days ago", days);
    }
    // Weeks, for the past 10 weeks or so.
    if days < 70 {
        return translated("%lu week ago", "%lu weeks ago", (days + 3) / 7);
    }
    // Months, for the past 12 months or so.
    if days < 365 {
        return translated("%lu month ago", "%lu months ago", (days + 15) / 30);
    }
    // Years and months, for 5 years or so.
    if days < 1825 {
        let total_months = (days * 12 * 2 + 365) / (365 * 2);
        let (years, months) = (total_months / 12, total_months % 12);
        let years_text = translated("%lu year", "%lu years", years);
        if months == 0 {
            return translated("%lu year ago", "%lu years ago", years);
        }
        return translated("%s, %lu month ago", "%s, %lu months ago", months).replacen(
            "%s",
            &years_text,
            1,
        );
    }
    // Only years.
    translated("%lu year ago", "%lu years ago", (days + 183) / 365)
}

/// `time` displayed like `git log --date=human`, `now` being the current date and timezone:
/// relative for recent dates, otherwise without the parts that are the same as `now`. If `local`,
/// `time` is displayed in the timezone of `now`.
pub fn human(time: Time, now: &Zoned, local: bool, locale: &Locale) -> String {
    let date = to_zoned(time, now, local);

    let same_year = date.year() == now.year();
    let same_month = same_year && date.month() == now.month();
    // Today: only the relative time.
    if same_month && date.day() == now.day() {
        return relative(time, now, locale);
    }
    // A few days ago: only the weekday.
    let hide_date = same_month && date.day() < now.day() && date.day() + 5 > now.day();

    let hide_timezone = local || date.offset() == now.offset() || !hide_date;
    let mut text = String::new();
    // The names of the weekdays and months of `jiff` are in English, like the ones of git, which
    // never translates them.
    if same_year {
        text += &date.strftime("%a ").to_string();
    }
    if !hide_date {
        text += &date.strftime("%b %-d ").to_string();
    }
    if same_year {
        text += &date.strftime("%H:%M").to_string();
    } else {
        text.truncate(text.trim_end().len());
        text += &date.strftime(" %Y").to_string();
    }
    if !hide_timezone {
        text += &date.strftime(" %z").to_string();
    }
    text
}

/// `time` in the timezone of `now` if `local`, otherwise with its own offset.
pub fn to_zoned(time: Time, now: &Zoned, local: bool) -> Zoned {
    let timezone = if local {
        now.time_zone().clone()
    } else {
        Offset::from_seconds(time.offset).map_or(TimeZone::UTC, TimeZone::fixed)
    };
    Timestamp::from_second(time.seconds)
        .unwrap_or_default()
        .to_zoned(timezone)
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
    parse_raw(String::from_utf8(output.stdout).ok()?.trim_end())
}

/// `text`, a date as git stores it (see [`RAW_FORMAT`]).
pub fn parse_raw(text: &str) -> Option<Time> {
    let parsed = strtime::parse(RAW_FORMAT, text).ok()?;
    Some(Time {
        seconds: parsed.to_timestamp().ok()?.as_second(),
        offset: parsed.offset()?.seconds(),
    })
}

/// The timezone of `now`, as a value of `TZ`: its IANA name, or a POSIX timezone with its fixed
/// offset (whose sign is inverted: `<+0200>-2:00` is UTC+2).
fn posix_timezone(now: &Zoned) -> String {
    if let Some(name) = now.time_zone().iana_name() {
        return name.to_string();
    }
    let inverted = now.with_time_zone(TimeZone::fixed(-now.offset()));
    format!("<{}>{}", now.strftime("%z"), inverted.strftime("%:z"))
}
