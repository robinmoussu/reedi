//! Runs the real `git`, to validate that what is documented as done "like git" is.

use std::path::Path;
use std::process::Command;
use std::process::Output;

use gix::date::Time;
use jiff::Zoned;
use tempfile::TempDir;

/// A new repository, and the environment `git` runs in. The configuration of the user is not
/// read, so that the tests don't depend on it.
pub struct Git {
    directory: TempDir,
    /// The IANA name of the current timezone (`TZ`).
    timezone: String,
    /// The current date, in seconds since the epoch (`GIT_TEST_DATE_NOW`).
    now: i64,
    /// `LC_ALL`.
    locale: String,
}

impl Git {
    /// An empty repository, where the current date is `now` and the locale is `locale` (`C`, or
    /// the name of an installed locale like `fr_FR.UTF-8`).
    pub fn new(now: &Zoned, locale: &str) -> Self {
        let git = Git {
            directory: tempfile::tempdir().unwrap(),
            timezone: now.time_zone().iana_name().unwrap().to_string(),
            now: now.timestamp().as_second(),
            locale: locale.to_string(),
        };
        assert!(git.run(&["init", "--quiet"]).status.success());
        git
    }

    pub fn path(&self) -> &Path {
        self.directory.path()
    }

    /// `git $args`, whatever its exit status.
    pub fn run(&self, args: &[&str]) -> Output {
        Command::new("git")
            .args(args)
            .current_dir(self.path())
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap())
            .env("HOME", self.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Author")
            .env("GIT_AUTHOR_EMAIL", "author@example.com")
            .env("GIT_COMMITTER_NAME", "Committer")
            .env("GIT_COMMITTER_EMAIL", "committer@example.com")
            .env("TZ", &self.timezone)
            .env("GIT_TEST_DATE_NOW", self.now.to_string())
            .env("LC_ALL", &self.locale)
            .output()
            .unwrap()
    }

    /// `git config $key $value`.
    pub fn set_config(&self, key: &str, value: &str) {
        assert!(self.run(&["config", key, value]).status.success());
    }

    /// `git commit --date=$date`, whose stderr is the error if git refuses `date`.
    pub fn commit_with_date(&self, date: &str) -> Result<(), String> {
        let output = self.run(&[
            "commit",
            "--quiet",
            "--allow-empty",
            "--no-gpg-sign",
            "--message=fake",
            &format!("--date={date}"),
        ]);
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8(output.stderr).unwrap())
        }
    }

    /// A commit authored at `time`.
    pub fn commit_at(&self, time: Time) {
        let sign = if time.offset < 0 { '-' } else { '+' };
        let minutes = time.offset.abs() / 60;
        let date = format!(
            "{} {sign}{:02}{:02}",
            time.seconds,
            minutes / 60,
            minutes % 60
        );
        self.commit_with_date(&date).unwrap();
    }

    /// The author date of the last commit, displayed by `git log` with `--date=$format` (or with
    /// its default if `format` is `None`), whose stderr is the error if git refuses `format`.
    pub fn author_date(&self, format: Option<&str>) -> Result<String, String> {
        let date = format.map(|format| format!("--date={format}"));
        let args = ["log", "-1", "--format=%ad"]
            .into_iter()
            .chain(date.as_deref());
        let output = self.run(&args.collect::<Vec<_>>());
        if output.status.success() {
            let stdout = String::from_utf8(output.stdout).unwrap();
            Ok(stdout.strip_suffix('\n').unwrap().to_string())
        } else {
            Err(String::from_utf8(output.stderr).unwrap())
        }
    }

    /// The author date of the last commit, as stored (`--date=raw`).
    pub fn author_time(&self) -> Time {
        let raw = self.author_date(Some("raw")).unwrap();
        let (seconds, offset) = raw.split_once(' ').unwrap();
        let sign = if offset.starts_with('-') { -1 } else { 1 };
        let hours: i32 = offset[1..3].parse().unwrap();
        let minutes: i32 = offset[3..5].parse().unwrap();
        Time {
            seconds: seconds.parse().unwrap(),
            offset: sign * (hours * 60 + minutes) * 60,
        }
    }
}
