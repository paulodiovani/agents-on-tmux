mod backends;
mod frontends;

use std::fmt::Display;

use backends::config::Config;
use clap::Parser;

#[derive(Parser)]
#[command(name = "aot", about = "Agents on tmux", version)]
struct Cli {
    /// Launch only the terminal UI
    #[arg(long, conflicts_with = "no_tui", default_missing_value = "true", num_args = 0..=1, require_equals = true)]
    tui: Option<bool>,

    /// Do not launch the terminal UI pane
    #[arg(long, default_missing_value = "true", num_args = 0..=1, require_equals = true)]
    no_tui: Option<bool>,

    /// Enable Nerd Font icons
    #[arg(long, env = "NERD_FONT", value_parser = parse_bool, default_missing_value = "true", num_args = 0..=1, require_equals = true)]
    nerd_font: Option<bool>,

    /// Enable Font Awesome icons
    #[arg(long, env = "FONT_AWESOME", value_parser = parse_bool, default_missing_value = "true", num_args = 0..=1, require_equals = true)]
    font_awesome: Option<bool>,
}

impl Display for Cli {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(tui) = self.tui {
            write!(f, "--tui={}", tui)?;
        }
        if let Some(no_tui) = self.no_tui {
            write!(f, " --no-tui={}", no_tui)?;
        }
        if let Some(nerd_font) = self.nerd_font {
            write!(f, " --nerd-font={}", nerd_font)?;
        }
        if let Some(font_awesome) = self.font_awesome {
            write!(f, " --font-awesome={}", font_awesome)?;
        }
        Ok(())
    }
}

impl From<&Cli> for Config {
    fn from(cli: &Cli) -> Self {
        Self {
            tui: cli.tui,
            no_tui: cli.no_tui,
            nerd_font: cli.nerd_font,
            font_awesome: cli.font_awesome,
        }
    }
}

fn parse_bool(value: &str) -> Result<bool, String> {
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(format!("expected boolean value, got '{value}'")),
    }
}

fn main() -> anyhow::Result<()> {
    use backends::tmux::{SESSION_NAME, Tmux, TmuxDriver, TmuxError, detect_parent_session};
    let config = Config::parse()?;
    let cli = Cli::parse();
    let config = config.merge(&cli);

    backends::agents::set_icon_fonts(
        config.nerd_font.unwrap_or(false),
        config.font_awesome.unwrap_or(false),
    );

    let parent_session = detect_parent_session()?;
    if parent_session == SESSION_NAME {
        return Err(TmuxError::InsideOwnSession(parent_session).into());
    }
    let parent_driver = TmuxDriver::new(&parent_session);

    let nested_driver = TmuxDriver::new(SESSION_NAME);
    nested_driver.create_session_if_not_exists()?;

    if config.tui.unwrap_or(false) {
        let terminal = ratatui::init();
        let mut app =
            frontends::tui::app::App::new(Box::new(nested_driver), Box::new(parent_driver))?;
        app.run(terminal)?;
        ratatui::restore();
    } else {
        let exe = std::env::current_exe()?;
        if !config.no_tui.unwrap_or(false) {
            let command = format!("{} --tui=true{}", exe.to_string_lossy(), cli);
            parent_driver.split_window(&command)?;
        }
        nested_driver.attach_session()?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_icon_env<T>(
        nerd_font: Option<&str>,
        font_awesome: Option<&str>,
        test: impl FnOnce() -> T,
    ) -> T {
        let _guard = ENV_LOCK.lock().unwrap();

        unsafe {
            if let Some(value) = nerd_font {
                std::env::set_var("NERD_FONT", value);
            } else {
                std::env::remove_var("NERD_FONT");
            }

            if let Some(value) = font_awesome {
                std::env::set_var("FONT_AWESOME", value);
            } else {
                std::env::remove_var("FONT_AWESOME");
            }
        }

        let result = test();

        unsafe {
            std::env::remove_var("NERD_FONT");
            std::env::remove_var("FONT_AWESOME");
        }

        result
    }

    #[test]
    fn test_cli_display_without_icon_flags() {
        let cli = with_icon_env(None, None, || Cli::parse_from(["aot"]));
        assert_eq!(format!("{}", cli), "");
    }

    #[test]
    fn test_cli_display_with_nerd_font_flag() {
        let cli = with_icon_env(None, None, || Cli::parse_from(["aot", "--nerd-font"]));
        assert_eq!(format!("{}", cli), " --nerd-font=true");
    }

    #[test]
    fn test_cli_display_with_font_awesome_flag() {
        let cli = with_icon_env(None, None, || Cli::parse_from(["aot", "--font-awesome"]));
        assert_eq!(format!("{}", cli), " --font-awesome=true");
    }

    #[test]
    fn test_cli_display_with_both_icon_flags() {
        let cli = with_icon_env(None, None, || {
            Cli::parse_from(["aot", "--nerd-font", "--font-awesome"])
        });
        assert_eq!(format!("{}", cli), " --nerd-font=true --font-awesome=true");
    }

    #[test]
    fn test_cli_display_with_explicit_false_values() {
        let cli = with_icon_env(None, None, || {
            Cli::parse_from(["aot", "--nerd-font=false", "--font-awesome=false"])
        });
        assert_eq!(
            format!("{}", cli),
            " --nerd-font=false --font-awesome=false"
        );
    }

    #[test]
    fn test_tui_and_no_tui_conflict() {
        assert!(Cli::try_parse_from(["aot", "--tui", "--no-tui"]).is_err());
    }

    #[test]
    fn test_nerd_font_env_sets_cli_option() {
        let cli = with_icon_env(Some("1"), None, || Cli::parse_from(["aot"]));
        assert_eq!(cli.nerd_font, Some(true));
        assert_eq!(cli.font_awesome, None);
    }

    #[test]
    fn test_font_awesome_env_sets_cli_option() {
        let cli = with_icon_env(None, Some("1"), || Cli::parse_from(["aot"]));
        assert_eq!(cli.nerd_font, None);
        assert_eq!(cli.font_awesome, Some(true));
    }

    #[test]
    fn test_from_cli_to_config() {
        let cli = with_icon_env(None, None, || {
            Cli::parse_from(["aot", "--tui", "--nerd-font"])
        });
        let config: Config = (&cli).into();
        assert_eq!(config.tui, Some(true));
        assert_eq!(config.no_tui, None);
        assert_eq!(config.nerd_font, Some(true));
        assert_eq!(config.font_awesome, None);
    }
}
