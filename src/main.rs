use std::error::Error;
use std::path::PathBuf;
use std::process::ExitCode;

use reedi::config;
use reedi::config::UnknownKey;
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
    let (settings, unknown_keys) = settings()?;
    reedi::run(&settings, unknown_keys)?;
    Ok(())
}

/// The path of the config file, and its unknown keys (at least one).
type UnknownKeys = (PathBuf, Vec<UnknownKey>);

/// The settings, read from the config file, and its path and unknown keys, if there are any.
///
/// If the colorscheme is unset, the background of the terminal is detected, before entering the
/// alternate screen.
///
/// Fails if the config file cannot be used.
fn settings() -> Result<(Settings, Option<UnknownKeys>), Box<dyn Error>> {
    let path = config::file_path()
        .ok_or("cannot locate the config file: no $XDG_CONFIG_HOME nor home directory")?;
    let file = config::read(&path)?;
    let settings = Settings {
        theme: Theme::choose(file.config.colorscheme, theme::detect),
    };
    let unknown_keys = if file.unknown_keys.is_empty() {
        None
    } else {
        Some((path, file.unknown_keys))
    };
    Ok((settings, unknown_keys))
}
