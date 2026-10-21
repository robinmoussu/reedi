use std::error::Error;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use reedi::cli::Cli;
use reedi::config;
use reedi::config::UnknownKey;
use reedi::date::DateDisplay;
use reedi::date::DateFormat;
use reedi::date::Environment;
use reedi::git;
use reedi::gix_compat::locale::Locale;
use reedi::settings::Settings;
use reedi::theme;
use reedi::theme::Theme;

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
    let (settings, unknown_keys) = settings(&cli)?;
    reedi::run(&settings, unknown_keys)?;
    Ok(())
}

/// The path of the config file, and its unknown keys (at least one).
type UnknownKeys = (PathBuf, Vec<UnknownKey>);

/// The settings, read from the command line and the config file, and the path and unknown keys of
/// the config file, if there are any.
///
/// If the colorscheme is unset, the background of the terminal is detected, before entering the
/// alternate screen.
///
/// Fails if the config file cannot be used.
fn settings(cli: &Cli) -> Result<(Settings, Option<UnknownKeys>), Box<dyn Error>> {
    let path = config::file_path()
        .ok_or("cannot locate the config file: no $XDG_CONFIG_HOME nor home directory")?;
    let file = config::read(&path)?;
    let settings = Settings {
        theme: Theme::choose(file.config.colorscheme, theme::detect),
        dates: dates(cli)?,
    };
    let unknown_keys = if file.unknown_keys.is_empty() {
        None
    } else {
        Some((path, file.unknown_keys))
    };
    Ok((settings, unknown_keys))
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
