What `gix` doesn't provide yet
==============================

Each of these is technical debt: when a new version of `gix` provides it, the associated code
should be removed.

`relative` and `human` date formats
-----------------------------------

- Used by: `--date-format`, and the `log.date` git setting
- Missing in `gix`: `gix::date::Time::format` has no `relative` nor `human` format
- Replaced by: `date.rs`, a port of `show_date_relative` and `show_date_normal` from git's `date.c`,
  translated with git's own translations (see below)

`strftime` with the locale of the environment
---------------------------------------------

- Used by: the `format:<strftime>` and `format-local:<strftime>` date formats
- Missing in `gix`: `gix::date::time::CustomFormat` only accepts patterns known at compile time
  (`&'static str`), and formats them with `jiff`, which doesn't use the locale (`%A` is always
  `Monday`, never `lundi`)
- Replaced by: `locale.rs`, which calls the `strftime` of the C library with the locale of the
  environment, like git

Translations of git
-------------------

- Used by: the `relative` and `human` date formats (`3 hours ago` is `il y a 3 heures` in French)
- Missing in `gix`: no translation
- Replaced by: `locale.rs`, which uses the gettext of the C library with the translations installed
  with git (the `git` text domain). Without them, messages are in English, like git

Parsing of dates
----------------

- Used by: the dates of the fake commits, `--fake-now`
- Missing in `gix`: `gix::date::parse` doesn't implement `approxidate` of git: it has no `noon`,
  `12/10/2026 14:32`, and dates without an offset are read as UTC instead of the current timezone
- Replaced by: `date.rs`, which runs `git commit --date` in a temporary repository (with the current
  date and timezone in `GIT_TEST_DATE_NOW` and `TZ`), the only git command parsing dates this way,
  then reads the date of the commit
