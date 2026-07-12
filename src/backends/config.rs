/// Application configuration options
#[derive(serde::Deserialize, Clone, Copy, Default)]
pub struct Config {
    #[serde(default)]
    pub tui: Option<bool>,
    #[serde(default)]
    pub no_tui: Option<bool>,
    #[serde(default)]
    pub nerd_font: Option<bool>,
    #[serde(default)]
    pub font_awesome: Option<bool>,
}

/// Possible errors when reading config file
#[derive(thiserror::Error, Debug)]
pub enum ConfigError {
    #[error("could not determine config directory")]
    ConfigDirNotFound,
    #[error("failed to read config file: {0}")]
    ReadError(#[from] std::io::Error),
    #[error("failed to parse config file: {0}")]
    ParseError(#[from] toml::de::Error),
}

impl Config {
    /// Parse a config file to a Config
    pub fn parse() -> Result<Self, ConfigError> {
        let path = dirs::config_dir()
            .ok_or(ConfigError::ConfigDirNotFound)?
            .join("aot")
            .join("aot.conf");

        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&path)?;
        Ok(toml::from_str(&content)?)
    }

    /// Merge two Config, the other takes precedence
    pub fn merge<C: Into<Self>>(self, other: C) -> Self {
        let other = other.into();
        Self {
            tui: other.tui.or(self.tui),
            no_tui: other.no_tui.or(self.no_tui),
            nerd_font: other.nerd_font.or(self.nerd_font),
            font_awesome: other.font_awesome.or(self.font_awesome),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_all_none() {
        let config = Config::default();
        assert_eq!(config.tui, None);
        assert_eq!(config.no_tui, None);
        assert_eq!(config.nerd_font, None);
        assert_eq!(config.font_awesome, None);
    }

    #[test]
    fn test_merge_other_overrides_self() {
        let base = Config {
            tui: Some(false),
            nerd_font: Some(false),
            no_tui: None,
            font_awesome: None,
        };
        let other = Config {
            tui: Some(true),
            nerd_font: None,
            no_tui: None,
            font_awesome: None,
        };
        let merged = base.merge(other);
        assert_eq!(merged.tui, Some(true));
        assert_eq!(merged.nerd_font, Some(false));
        assert_eq!(merged.no_tui, None);
        assert_eq!(merged.font_awesome, None);
    }

    #[test]
    fn test_merge_falls_back_to_self() {
        let base = Config {
            tui: None,
            no_tui: None,
            nerd_font: Some(true),
            font_awesome: Some(true),
        };
        let other = Config::default();
        let merged = base.merge(other);
        assert_eq!(merged.nerd_font, Some(true));
        assert_eq!(merged.font_awesome, Some(true));
    }

    #[test]
    fn test_merge_both_none_stays_none() {
        let merged = Config::default().merge(Config::default());
        assert_eq!(merged.tui, None);
        assert_eq!(merged.no_tui, None);
        assert_eq!(merged.nerd_font, None);
        assert_eq!(merged.font_awesome, None);
    }

    #[test]
    fn test_parse_valid_toml() {
        let dir = std::env::temp_dir().join("aot_test_config");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("aot.conf");
        std::fs::write(&path, "tui = true\nnerd_font = true\n").unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        assert_eq!(config.tui, Some(true));
        assert_eq!(config.nerd_font, Some(true));
        assert_eq!(config.no_tui, None);
        assert_eq!(config.font_awesome, None);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_parse_empty_toml_returns_defaults() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.tui, None);
        assert_eq!(config.no_tui, None);
        assert_eq!(config.nerd_font, None);
        assert_eq!(config.font_awesome, None);
    }

    #[test]
    fn test_parse_malformed_toml_returns_error() {
        let result: Result<Config, _> = toml::from_str("not valid {{{ toml");
        assert!(result.is_err());
    }
}
