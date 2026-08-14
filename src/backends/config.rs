/// The 16 colors of the terminal's own palette, the only ones aot ever names.
/// Colors are never chosen absolutely: the terminal theme decides what each slot
/// looks like, so aot recolors itself when the user changes the theme.
#[derive(serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnsiColor {
    Black,
    Blue,
    Cyan,
    #[serde(alias = "bright_black")]
    DarkGray,
    Gray,
    Green,
    #[serde(alias = "bright_blue")]
    LightBlue,
    #[serde(alias = "bright_cyan")]
    LightCyan,
    #[serde(alias = "bright_green")]
    LightGreen,
    #[serde(alias = "bright_magenta")]
    LightMagenta,
    #[serde(alias = "bright_red")]
    LightRed,
    #[serde(alias = "bright_yellow")]
    LightYellow,
    Magenta,
    Red,
    #[serde(alias = "bright_white")]
    White,
    Yellow,
}

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
    #[serde(default)]
    pub debug: Option<bool>,
    #[serde(default)]
    pub accent_color: Option<AnsiColor>,
    #[serde(default)]
    pub selection_bg: Option<AnsiColor>,
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
            debug: other.debug.or(self.debug),
            accent_color: other.accent_color.or(self.accent_color),
            selection_bg: other.selection_bg.or(self.selection_bg),
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
        assert_eq!(config.debug, None);
        assert_eq!(config.accent_color, None);
        assert_eq!(config.selection_bg, None);
    }

    #[test]
    fn test_merge_other_overrides_self() {
        let base = Config {
            tui: Some(false),
            nerd_font: Some(false),
            no_tui: None,
            font_awesome: None,
            debug: Some(false),
            accent_color: Some(AnsiColor::Blue),
            selection_bg: None,
        };
        let other = Config {
            tui: Some(true),
            nerd_font: None,
            no_tui: None,
            font_awesome: None,
            debug: Some(true),
            accent_color: Some(AnsiColor::Magenta),
            selection_bg: None,
        };
        let merged = base.merge(other);
        assert_eq!(merged.tui, Some(true));
        assert_eq!(merged.nerd_font, Some(false));
        assert_eq!(merged.no_tui, None);
        assert_eq!(merged.font_awesome, None);
        assert_eq!(merged.debug, Some(true));
        assert_eq!(merged.accent_color, Some(AnsiColor::Magenta));
    }

    #[test]
    fn test_merge_falls_back_to_self() {
        let base = Config {
            tui: None,
            no_tui: None,
            nerd_font: Some(true),
            font_awesome: Some(true),
            debug: Some(true),
            accent_color: Some(AnsiColor::Red),
            selection_bg: Some(AnsiColor::DarkGray),
        };
        let other = Config::default();
        let merged = base.merge(other);
        assert_eq!(merged.nerd_font, Some(true));
        assert_eq!(merged.font_awesome, Some(true));
        assert_eq!(merged.debug, Some(true));
        assert_eq!(merged.accent_color, Some(AnsiColor::Red));
        assert_eq!(merged.selection_bg, Some(AnsiColor::DarkGray));
    }

    #[test]
    fn test_merge_both_none_stays_none() {
        let merged = Config::default().merge(Config::default());
        assert_eq!(merged.tui, None);
        assert_eq!(merged.no_tui, None);
        assert_eq!(merged.nerd_font, None);
        assert_eq!(merged.font_awesome, None);
        assert_eq!(merged.debug, None);
        assert_eq!(merged.accent_color, None);
        assert_eq!(merged.selection_bg, None);
    }

    #[test]
    fn test_parse_colors_from_toml() {
        let config: Config =
            toml::from_str("accent_color = \"magenta\"\nselection_bg = \"dark_gray\"\n").unwrap();
        assert_eq!(config.accent_color, Some(AnsiColor::Magenta));
        assert_eq!(config.selection_bg, Some(AnsiColor::DarkGray));
    }

    #[test]
    fn test_parse_color_aliases() {
        let config: Config = toml::from_str("selection_bg = \"bright_black\"\n").unwrap();
        assert_eq!(config.selection_bg, Some(AnsiColor::DarkGray));
    }

    #[test]
    fn test_parse_unknown_color_returns_error() {
        let result: Result<Config, _> = toml::from_str("accent_color = \"#ff8800\"\n");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_valid_toml() {
        let dir = std::env::temp_dir().join("aot_test_config");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("aot.conf");
        std::fs::write(&path, "tui = true\nnerd_font = true\ndebug = true\n").unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        assert_eq!(config.tui, Some(true));
        assert_eq!(config.nerd_font, Some(true));
        assert_eq!(config.no_tui, None);
        assert_eq!(config.font_awesome, None);
        assert_eq!(config.debug, Some(true));
        assert_eq!(config.accent_color, None);
        assert_eq!(config.selection_bg, None);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_parse_empty_toml_returns_defaults() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.tui, None);
        assert_eq!(config.no_tui, None);
        assert_eq!(config.nerd_font, None);
        assert_eq!(config.font_awesome, None);
        assert_eq!(config.debug, None);
    }

    #[test]
    fn test_parse_malformed_toml_returns_error() {
        let result: Result<Config, _> = toml::from_str("not valid {{{ toml");
        assert!(result.is_err());
    }
}
