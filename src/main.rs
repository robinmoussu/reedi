#![forbid(unsafe_code)]

use std::error::Error;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use reedi::cli;
use reedi::cli::Cli;
use reedi::config;
use reedi::config::UnknownSetting;
use reedi::date::DateDisplay;
use reedi::date::DateFormat;
use reedi::date::Environment;
use reedi::git;
use reedi::log;
use reedi::settings::Settings;
use reedi::theme;
use reedi::theme::Theme;
use reedi_gix_compat::locale::Locale;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("reedi: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    if cli.help_hidden {
        print!("{}", cli::hidden_help());
        return Ok(());
    }
    let revisions = log::displayed_revisions(&cli, &std::env::current_dir()?)?;
    let (settings, unknown_settings) = settings(&cli)?;
    let sent_keys = cli.send_keys.unwrap_or_default().0;
    reedi::run(&revisions, &settings, unknown_settings, sent_keys)?;
    Ok(())
}

/// The path of the config file, and its unrecognized settings (at least one).
type UnknownSettings = (PathBuf, Vec<UnknownSetting>);

/// The settings, read from the command line and the config file, and the path and unrecognized
/// settings of the config file, if there are any.
///
/// If the colorscheme is unset, the background of the terminal is detected, before entering the
/// alternate screen.
///
/// Fails if the config file cannot be used.
fn settings(cli: &Cli) -> Result<(Settings, Option<UnknownSettings>), Box<dyn Error>> {
    let path = config::file_path()
        .ok_or("cannot locate the config file: no $XDG_CONFIG_HOME nor home directory")?;
    let file = config::read(&path)?;
    let settings = Settings {
        theme: Theme::choose(file.config.colorscheme, theme::detect),
        dates: dates(cli)?,
    };
    let unknown_settings = if file.unknown_settings.is_empty() {
        None
    } else {
        Some((path, file.unknown_settings))
    };
    Ok((settings, unknown_settings))
}

/// How dates are displayed: with `--date-format`, otherwise the `log.date` git setting, otherwise
/// `human`.
fn dates(cli: &Cli) -> Result<DateDisplay, Box<dyn Error>> {
    let format = match &cli.date_format {
        Some(format) => format.clone(),
        None => git::log_date(&std::env::current_dir()?)?.unwrap_or(DateFormat::HUMAN),
    };
    Ok(DateDisplay {
        format,
        environment: Environment {
            now: cli.now()?,
            locale: Locale::from_environment(),
        },
    })
}
