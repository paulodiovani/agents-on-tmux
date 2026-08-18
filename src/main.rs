mod backends;
mod frontends;

use std::fmt::Display;

use backends::config::{Config, DEFAULT_TUI_WIDTH};
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

    /// TUI panel width in columns, only when the panel is split (default: 35)
    #[arg(long, value_parser = clap::value_parser!(u16).range(1..))]
    tui_width: Option<u16>,

    /// Run as the split side panel (internal: set by the launcher)
    #[arg(long, hide = true)]
    split_panel: bool,

    /// Enable Nerd Font icons
    #[arg(long, env = "NERD_FONT", value_parser = parse_bool, default_missing_value = "true", num_args = 0..=1, require_equals = true)]
    nerd_font: Option<bool>,

    /// Enable Font Awesome icons
    #[arg(long, env = "FONT_AWESOME", value_parser = parse_bool, default_missing_value = "true", num_args = 0..=1, require_equals = true)]
    font_awesome: Option<bool>,

    /// Enable debug logging to a file
    #[arg(long, env = "AOT_DEBUG", value_parser = parse_bool, default_missing_value = "true", num_args = 0..=1, require_equals = true)]
    debug: Option<bool>,
}

// Implement Display so we can extract the cli options to forward to TUI side-panel
impl Display for Cli {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(tui) = self.tui {
            write!(f, "--tui={}", tui)?;
        }
        if let Some(no_tui) = self.no_tui {
            write!(f, " --no-tui={}", no_tui)?;
        }
        if let Some(tui_width) = self.tui_width {
            write!(f, " --tui-width={}", tui_width)?;
        }
        if let Some(nerd_font) = self.nerd_font {
            write!(f, " --nerd-font={}", nerd_font)?;
        }
        if let Some(font_awesome) = self.font_awesome {
            write!(f, " --font-awesome={}", font_awesome)?;
        }
        if let Some(debug) = self.debug {
            write!(f, " --debug={}", debug)?;
        }
        Ok(())
    }
}

impl From<&Cli> for Config {
    fn from(cli: &Cli) -> Self {
        Self {
            tui: cli.tui,
            no_tui: cli.no_tui,
            tui_width: cli.tui_width,
            nerd_font: cli.nerd_font,
            font_awesome: cli.font_awesome,
            debug: cli.debug,
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

/// Builds the side-panel identity (pane id, width) for the TUI. `Some` only
/// when launched as the split panel; the pane id comes from TMUX_PANE, which
/// tmux itself sets in every pane process.
fn panel_config(split_panel: bool, tui_width: Option<u16>) -> Option<(String, u16)> {
    if !split_panel {
        return None;
    }
    let pane_id = std::env::var("TMUX_PANE").ok()?;
    Some((pane_id, tui_width.unwrap_or(DEFAULT_TUI_WIDTH)))
}

fn main() -> anyhow::Result<()> {
    use backends::tmux::{SESSION_NAME, Tmux, TmuxDriver, TmuxError, detect_parent_session};
    let config = Config::parse()?;
    let cli = Cli::parse();
    let config = config.merge(&cli);

    // Initialize before ratatui takes over the screen; the logger only ever
    // writes to this file, never to stdout/stderr.
    if config.debug.unwrap_or(false) {
        let path = dirs::cache_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("aot");
        let _ = std::fs::create_dir_all(&path);
        let _ = backends::logger::init(&path.join("aot.log"));
        backends::logger::info("main: starting aot");
    }

    frontends::tui::icons::set_icon_fonts(
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
        backends::logger::info("main: starting tui");
        let terminal = ratatui::init();
        // Only the split panel enforces its width; a plain `aot --tui` must not.
        let panel = panel_config(cli.split_panel, config.tui_width);
        let mut app =
            frontends::tui::app::App::new(Box::new(nested_driver), Box::new(parent_driver), panel)?;
        app.run(terminal)?;
        ratatui::restore();
    } else {
        let exe = std::env::current_exe()?;
        if !config.no_tui.unwrap_or(false) {
            // --split-panel marks the child as the side panel so it enforces
            // its width; the width itself travels via the forwarded CLI flags.
            let command = format!("{} --tui=true --split-panel{}", exe.to_string_lossy(), cli);
            let width = config.tui_width.unwrap_or(DEFAULT_TUI_WIDTH);
            parent_driver.split_window(&command, width)?;
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

            // Ambient AOT_DEBUG would leak into Cli parsing and Display output.
            std::env::remove_var("AOT_DEBUG");
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
            Cli::parse_from(["aot", "--tui", "--nerd-font", "--tui-width", "50"])
        });
        let config: Config = (&cli).into();
        assert_eq!(config.tui, Some(true));
        assert_eq!(config.no_tui, None);
        assert_eq!(config.nerd_font, Some(true));
        assert_eq!(config.font_awesome, None);
        assert_eq!(config.debug, None);
        assert_eq!(config.tui_width, Some(50));
    }

    #[test]
    fn test_tui_width_flag() {
        let cli = with_icon_env(None, None, || Cli::parse_from(["aot", "--tui-width", "50"]));
        assert_eq!(cli.tui_width, Some(50));
    }

    #[test]
    fn test_tui_width_rejects_zero() {
        assert!(Cli::try_parse_from(["aot", "--tui-width", "0"]).is_err());
    }

    #[test]
    fn test_tui_width_forwarded_to_tui_command() {
        // The width must survive the hop to the TUI child process: pane
        // processes are spawned by the tmux server, not by aot.
        let cli = with_icon_env(None, None, || Cli::parse_from(["aot", "--tui-width", "50"]));
        assert_eq!(format!("{}", cli), " --tui-width=50");
    }

    #[test]
    fn test_split_panel_flag_is_hidden_and_parsed() {
        let cli = with_icon_env(None, None, || Cli::parse_from(["aot", "--split-panel"]));
        assert!(cli.split_panel);

        let cli = with_icon_env(None, None, || Cli::parse_from(["aot"]));
        assert!(!cli.split_panel);

        use clap::CommandFactory;
        let help = Cli::command().render_long_help().to_string();
        assert!(!help.contains("split-panel"));
    }

    #[test]
    fn test_panel_config_not_split_panel() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("TMUX_PANE", "%5") };
        assert_eq!(panel_config(false, Some(50)), None);
        unsafe { std::env::remove_var("TMUX_PANE") };
    }

    #[test]
    fn test_panel_config_split_panel_default_width() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("TMUX_PANE", "%5") };
        assert_eq!(panel_config(true, None), Some(("%5".to_string(), 35)));
        unsafe { std::env::remove_var("TMUX_PANE") };
    }

    #[test]
    fn test_panel_config_split_panel_custom_width() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("TMUX_PANE", "%7") };
        assert_eq!(panel_config(true, Some(50)), Some(("%7".to_string(), 50)));
        unsafe { std::env::remove_var("TMUX_PANE") };
    }

    #[test]
    fn test_panel_config_split_panel_without_tmux_pane() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::remove_var("TMUX_PANE") };
        assert_eq!(panel_config(true, None), None);
    }

    #[test]
    fn test_tui_width_help_shows_default() {
        use clap::CommandFactory;
        let help = Cli::command().render_long_help().to_string();
        assert!(help.contains("--tui-width"));
        assert!(help.contains("default: 35"));
    }

    #[test]
    fn test_debug_flag() {
        let cli = with_icon_env(None, None, || Cli::parse_from(["aot", "--debug"]));
        assert_eq!(cli.debug, Some(true));
    }

    #[test]
    fn test_debug_env_var() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("AOT_DEBUG", "1") };
        let cli = Cli::parse_from(["aot"]);
        let config = Config::from(&cli);
        unsafe { std::env::remove_var("AOT_DEBUG") };
        assert_eq!(config.debug, Some(true));
    }

    #[test]
    fn test_cli_display_with_debug_flag() {
        let cli = with_icon_env(None, None, || Cli::parse_from(["aot", "--debug"]));
        assert_eq!(format!("{}", cli), " --debug=true");
    }
}
