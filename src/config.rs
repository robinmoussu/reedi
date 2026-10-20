//! The config file: `$XDG_CONFIG_HOME/reedi/config.toml` (`~/.config/reedi/config.toml` if
//! unset).

use std::env;
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Deserializer;
use serde::de::DeserializeSeed;
use serde::de::IgnoredAny;
use serde::de::MapAccess;
use serde::de::Visitor;
use toml::Spanned;

/// The colorscheme set in the config file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Colorscheme {
    Light,
    Dark,
    Monochrome,
}

/// The settings of the config file. Unset settings are `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Config {
    pub colorscheme: Option<Colorscheme>,
}

/// A key of the config file that is not a setting, with its value.
#[derive(Debug, Clone, PartialEq)]
pub struct UnknownKey {
    pub key: String,
    pub value: toml::Value,
    /// The line of the key in the file, starting at 1.
    pub line: usize,
}

impl fmt::Display for UnknownKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "`{} = {}`", self.key, self.value)
    }
}

/// The content of a config file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConfigFile {
    pub config: Config,
    /// In the order of the file.
    pub unknown_keys: Vec<UnknownKey>,
}

/// The content of a config file, as deserialized.
#[derive(Deserialize)]
struct RawConfigFile {
    #[serde(flatten)]
    config: Config,
    /// The keys not deserialized in `config`.
    #[serde(flatten)]
    unknown_keys: toml::Table,
}

/// Deserializes the line of each key of `keys`, the keys not used by [`Config`] in the config file
/// whose text is `text`.
///
/// It is needed because the position of the keys is lost by `#[serde(flatten)]`.
struct UnknownKeys<'a> {
    keys: toml::Table,
    text: &'a str,
}

impl<'de> DeserializeSeed<'de> for UnknownKeys<'_> {
    type Value = Vec<UnknownKey>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for UnknownKeys<'_> {
    type Value = Vec<UnknownKey>;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a table of settings")
    }

    fn visit_map<A: MapAccess<'de>>(mut self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut unknown_keys = Vec::new();
        while let Some(key) = map.next_key::<Spanned<String>>()? {
            map.next_value::<IgnoredAny>()?;
            if let Some(value) = self.keys.remove(key.get_ref()) {
                unknown_keys.push(UnknownKey {
                    // The span of the key is in bytes.
                    line: self.text[..key.span().start].matches('\n').count() + 1,
                    key: key.into_inner(),
                    value,
                });
            }
        }
        Ok(unknown_keys)
    }
}

/// Why a config file cannot be used.
#[derive(Debug)]
pub enum Problem {
    Unreadable(io::Error),
    /// The file is not valid TOML, or a setting has an invalid value.
    Invalid(String),
}

/// Why the config file at `path` cannot be used.
#[derive(Debug)]
pub struct ConfigError {
    pub path: PathBuf,
    pub problem: Problem,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let path = self.path.display();
        match &self.problem {
            Problem::Unreadable(error) => write!(f, "cannot read the config file {path}: {error}"),
            Problem::Invalid(reason) => write!(f, "invalid config file {path}: {reason}"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// The path of the config file, from `$XDG_CONFIG_HOME` and the home directory, if it can be
/// located.
///
/// Like the XDG base directory specification, an empty `$XDG_CONFIG_HOME` is unset.
pub fn file_path() -> Option<PathBuf> {
    let config_home = match env::var_os("XDG_CONFIG_HOME") {
        Some(xdg_config_home) if !xdg_config_home.is_empty() => PathBuf::from(xdg_config_home),
        _ => env::home_dir()?.join(".config"),
    };
    Some(config_home.join("reedi").join("config.toml"))
}

/// The content of a config file whose text is `text`, or why it is invalid.
pub fn parse(text: &str) -> Result<ConfigFile, String> {
    let to_string = |error: toml::de::Error| error.to_string();
    let raw: RawConfigFile = toml::from_str(text).map_err(to_string)?;
    let unknown_keys = if raw.unknown_keys.is_empty() {
        Vec::new()
    } else {
        // Like `toml::from_str`, which cannot be given the keys to look for.
        UnknownKeys {
            keys: raw.unknown_keys,
            text,
        }
        .deserialize(toml::Deserializer::parse(text).map_err(to_string)?)
        .map_err(to_string)?
    };
    Ok(ConfigFile {
        config: raw.config,
        unknown_keys,
    })
}

/// The content of the config file at `path`. A missing file has no settings set.
pub fn read(path: &Path) -> Result<ConfigFile, ConfigError> {
    let error = |problem| ConfigError {
        path: path.to_path_buf(),
        problem,
    };
    match fs::read_to_string(path) {
        Ok(text) => parse(&text).map_err(|reason| error(Problem::Invalid(reason))),
        Err(io_error) if io_error.kind() == io::ErrorKind::NotFound => Ok(ConfigFile::default()),
        Err(io_error) => Err(error(Problem::Unreadable(io_error))),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process;

    use super::*;

    #[test]
    fn colorscheme_is_read() {
        let colorscheme = |text| parse(text).unwrap().config.colorscheme;
        assert_eq!(
            colorscheme("colorscheme = \"light\""),
            Some(Colorscheme::Light)
        );
        assert_eq!(
            colorscheme("colorscheme = \"dark\""),
            Some(Colorscheme::Dark)
        );
        assert_eq!(
            colorscheme("colorscheme = \"monochrome\""),
            Some(Colorscheme::Monochrome)
        );
    }

    #[test]
    fn colorscheme_is_unset_if_absent() {
        assert_eq!(parse(""), Ok(ConfigFile::default()));
    }

    #[test]
    fn invalid_colorscheme_is_an_error() {
        assert!(parse("colorscheme = \"blue\"").is_err());
        assert!(parse("colorscheme = 1").is_err());
    }

    #[test]
    fn invalid_toml_is_an_error() {
        assert!(parse("colorscheme =").is_err());
    }

    #[test]
    fn unknown_keys_are_reported() {
        let file = parse("scrolloff = 3\ncolorscheme = \"dark\"\n[keys]\nquit = \"q\"\n").unwrap();
        assert_eq!(file.config.colorscheme, Some(Colorscheme::Dark));
        let keys: Vec<_> = file
            .unknown_keys
            .iter()
            .map(|unknown_key| (unknown_key.line, unknown_key.to_string()))
            .collect();
        assert_eq!(
            keys,
            [
                (1, "`scrolloff = 3`".to_string()),
                (3, "`keys = { quit = \"q\" }`".to_string())
            ]
        );
    }

    /// A path in the temporary directory, unique to this process and to `name`.
    fn temporary(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("reedi-{}-{name}", process::id()))
    }

    #[test]
    fn missing_file_has_no_settings_set() {
        assert_eq!(read(&temporary("missing")).unwrap(), ConfigFile::default());
    }

    #[test]
    fn file_is_read() {
        let path = temporary("config.toml");
        fs::write(&path, "colorscheme = \"light\"\n").unwrap();
        let file = read(&path);
        fs::remove_file(&path).unwrap();
        assert_eq!(file.unwrap().config.colorscheme, Some(Colorscheme::Light));
    }

    #[test]
    fn unreadable_file_is_an_error_naming_the_file() {
        // A directory cannot be read as a file.
        let path = std::env::temp_dir();
        let error = read(&path).unwrap_err();
        assert_eq!(error.path, path);
        assert!(error.to_string().contains(&path.display().to_string()));
    }

    #[test]
    fn invalid_file_is_an_error_naming_the_file() {
        let path = temporary("invalid.toml");
        fs::write(&path, "colorscheme = \"blue\"\n").unwrap();
        let error = read(&path);
        fs::remove_file(&path).unwrap();
        let error = error.unwrap_err();
        assert_eq!(error.path, path);
        assert!(error.to_string().contains(&path.display().to_string()));
    }
}
