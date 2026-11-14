mod backends;
mod frontends;

use std::sync::Arc;

use backends::config::{Config, LaunchMode};
use backends::tmux::{SESSION_NAME, SOCKET_NAME, Tmux, TmuxDriver, detect_parent_session};
use clap::{Parser, Subcommand};
use frontends::mcp::{McpHandler, McpServer};

use crate::backends::logger;

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
    #[arg(long, value_parser = clap::value_parser!(u16).range(1..), overrides_with = "tui_width", require_equals = true)]
    tui_width: Option<u16>,

    /// Enable Nerd Font icons
    #[arg(long, env = "NERD_FONT", value_parser = parse_bool, default_missing_value = "true", num_args = 0..=1, require_equals = true)]
    nerd_font: Option<bool>,

    /// Enable Font Awesome icons
    #[arg(long, env = "FONT_AWESOME", value_parser = parse_bool, default_missing_value = "true", num_args = 0..=1, require_equals = true)]
    font_awesome: Option<bool>,

    /// Enable debug logging to a file
    #[arg(long, env = "AOT_DEBUG", value_parser = parse_bool, default_missing_value = "true", num_args = 0..=1, require_equals = true)]
    debug: Option<bool>,

    /// TMUX environment variable (read from env only)
    #[arg(env = "TMUX", hide = true)]
    tmux_env: Option<String>,

    /// TUI pane ID (read from env only)
    #[arg(env = "TMUX_PANE", hide = true)]
    tui_pane: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug, PartialEq)]
enum Command {
    /// Run the MCP stdio server for agents (connects to the running aot)
    Mcp,
}

impl Cli {
    /// Options the TUI side-panel child must inherit from this invocation.
    fn forwarded_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(nerd_font) = self.nerd_font {
            args.push(format!("--nerd-font={nerd_font}"));
        }
        if let Some(font_awesome) = self.font_awesome {
            args.push(format!("--font-awesome={font_awesome}"));
        }
        if let Some(debug) = self.debug {
            args.push(format!("--debug={debug}"));
        }
        args
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
            tmux_env: cli.tmux_env.clone(),
            tui_pane: cli.tui_pane.clone(),
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

/// Shell command that launches the TUI side-panel in the parent session.
fn split_command(exe: &str, cli: &Cli, width: u16) -> String {
    let mut argv = vec![exe.to_string()];
    argv.extend(cli.forwarded_args());
    argv.push("--tui=true".to_string());
    argv.push(format!("--tui-width={width}"));
    backends::shell::join(&argv)
}

fn start_mcp_server() -> Option<McpServer> {
    let path = frontends::mcp::socket_path();
    let tmux = Arc::new(TmuxDriver::new_with_socket(SESSION_NAME, SOCKET_NAME));
    let default_dir = std::env::current_dir().unwrap_or_else(|_| std::env::temp_dir());
    let handler = McpHandler::new(frontends::mcp::all_tools(tmux, default_dir));
    match McpServer::start(&path, Arc::new(handler)) {
        Ok(Some(server)) => {
            logger::info(&format!("main: MCP server listening on {}", path.display()));
            Some(server)
        }
        Ok(None) => {
            logger::info("main: MCP socket owned by another aot instance; not starting");
            None
        }
        Err(error) => {
            logger::error(&format!("main: MCP server failed to start: {error}"));
            None
        }
    }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if cli.command == Some(Command::Mcp) {
        let path = frontends::mcp::socket_path();
        let mut stdout = std::io::stdout();
        frontends::mcp::run_shim(
            &path,
            cli.tui_pane.as_deref(),
            std::io::stdin(),
            &mut stdout,
        )?;
        return Ok(());
    }

    let config = Config::parse()?.merge(&cli);

    // Logger setup must precede any tmux session/thread creation, so a fatal
    // --debug failure leaves no server, session, or thread behind.
    if config.debug.unwrap_or(false) {
        let path = dirs::cache_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("aot");
        std::fs::create_dir_all(&path)?;
        backends::logger::init(&path.join("aot.log"))?;
        backends::logger::info("main: starting aot");
    }

    // Install panic hook before ratatui::init() so panics are logged to the
    // file and the terminal is restored. ratatui::init() installs its own
    // restore hook and requires other hooks to be installed first.
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        logger::error(&format!("panic: {info}"));
        previous_hook(info);
    }));

    frontends::tui::icons::set_icon_fonts(
        config.nerd_font.unwrap_or(false),
        config.font_awesome.unwrap_or(false),
    );

    let tmux_env = config.tmux_env.as_deref();
    let parent_session = detect_parent_session(tmux_env)?;
    logger::debug(&format!("main: parent session: {}", parent_session));

    let parent_driver = TmuxDriver::new(&parent_session);

    let nested_driver = TmuxDriver::new_with_socket(SESSION_NAME, SOCKET_NAME);
    nested_driver.create_session_if_not_exists(tmux_env)?;

    let mcp_server = start_mcp_server();

    let pane_id = config.tui_pane.clone();
    if pane_id.is_none() {
        logger::debug("main: TMUX_PANE not set; focus tracking disabled");
    }

    match config.launch_mode() {
        LaunchMode::NoTui => {
            // Blocking attach: signals must exit the process immediately.
            if let Err(error) = backends::signals::install(backends::signals::OnSignal::Exit) {
                logger::error(&format!("main: signal install failed: {error}"));
            }
            nested_driver.attach_session()?;
        }
        LaunchMode::TuiOnly { width } => {
            backends::logger::info("main: starting tui");
            // TUI mode: signals set a flag; the run loop polls it.
            if let Err(error) = backends::signals::install(backends::signals::OnSignal::Shutdown) {
                logger::error(&format!("main: signal install failed: {error}"));
            }
            let terminal = ratatui::init();
            let _ = crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture);
            let mut app = frontends::tui::app::App::new(
                Box::new(nested_driver),
                Box::new(parent_driver),
                pane_id,
                width,
            )?;
            let result = app.run(terminal);
            let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableMouseCapture);
            if let Err(error) = ratatui::try_restore() {
                logger::error(&format!("main: failed to restore terminal: {error}"));
            }
            result?;
        }
        LaunchMode::Split { width } => {
            // Blocking attach: signals must exit the process immediately.
            if let Err(error) = backends::signals::install(backends::signals::OnSignal::Exit) {
                logger::error(&format!("main: signal install failed: {error}"));
            }
            let exe = std::env::current_exe()?;
            let command = split_command(&exe.to_string_lossy(), &cli, width);
            parent_driver.split_window(&command, width)?;
            nested_driver.attach_session()?;
        }
    }

    drop(mcp_server);

    // If a signal was received (TUI mode), exit with 128 + signo.
    if let Some((signo, _name)) = backends::signals::received() {
        std::process::exit(128 + signo);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_display_without_icon_flags() {
        let cli = Cli::parse_from(["aot"]);
        assert!(cli.forwarded_args().is_empty());
    }

    #[test]
    fn test_cli_display_with_nerd_font_flag() {
        let cli = Cli::parse_from(["aot", "--nerd-font"]);
        assert_eq!(cli.forwarded_args(), ["--nerd-font=true"]);
    }

    #[test]
    fn test_cli_display_with_font_awesome_flag() {
        let cli = Cli::parse_from(["aot", "--font-awesome"]);
        assert_eq!(cli.forwarded_args(), ["--font-awesome=true"]);
    }

    #[test]
    fn test_cli_display_with_both_icon_flags() {
        let cli = Cli::parse_from(["aot", "--nerd-font", "--font-awesome"]);
        assert_eq!(
            cli.forwarded_args(),
            ["--nerd-font=true", "--font-awesome=true"]
        );
    }

    #[test]
    fn test_cli_display_with_explicit_false_values() {
        let cli = Cli::parse_from(["aot", "--nerd-font=false", "--font-awesome=false"]);
        assert_eq!(
            cli.forwarded_args(),
            ["--nerd-font=false", "--font-awesome=false"]
        );
    }

    #[test]
    fn test_tui_and_no_tui_conflict() {
        assert!(Cli::try_parse_from(["aot", "--tui", "--no-tui"]).is_err());
    }

    #[test]
    fn test_from_cli_to_config() {
        let cli = Cli::parse_from(["aot", "--tui", "--nerd-font", "--tui-width=50"]);
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
        let cli = Cli::parse_from(["aot", "--tui-width=50"]);
        assert_eq!(cli.tui_width, Some(50));
    }

    #[test]
    fn test_tui_width_rejects_zero() {
        assert!(Cli::try_parse_from(["aot", "--tui-width=0"]).is_err());
    }

    #[test]
    fn test_split_command_passes_width_to_tui() {
        let cli = Cli::parse_from(["aot", "--tui-width=50"]);
        assert_eq!(
            split_command("/usr/bin/aot", &cli, 50),
            "/usr/bin/aot --tui=true --tui-width=50"
        );
    }

    #[test]
    fn test_split_command_forwards_options() {
        let cli = Cli::parse_from(["aot", "--nerd-font", "--debug=false"]);
        assert_eq!(
            split_command("/usr/bin/aot", &cli, 35),
            "/usr/bin/aot --nerd-font=true --debug=false --tui=true --tui-width=35"
        );
    }

    #[test]
    fn test_split_command_quotes_exe_path() {
        let cli = Cli::parse_from(["aot"]);
        assert_eq!(
            split_command("/home/me/my apps/aot", &cli, 35),
            "'/home/me/my apps/aot' --tui=true --tui-width=35"
        );
    }

    #[test]
    fn test_split_command_drops_conflicting_tui_flags() {
        let cli = Cli::parse_from(["aot", "--no-tui=false"]);
        let command = split_command("aot", &cli, 40);
        let child = Cli::try_parse_from(command.split(' ')).unwrap();
        assert_eq!(child.tui, Some(true));
        assert_eq!(child.tui_width, Some(40));
    }

    #[test]
    fn test_tui_width_help_shows_default() {
        use clap::CommandFactory;
        let help = Cli::command().render_long_help().to_string();
        assert!(help.contains("--tui-width"));
        assert!(help.contains("default: 35"));
    }

    #[test]
    fn test_mcp_subcommand() {
        let cli = Cli::parse_from(["aot", "mcp"]);
        assert_eq!(cli.command, Some(Command::Mcp));
        assert!(cli.forwarded_args().is_empty());
    }

    #[test]
    fn test_no_subcommand_by_default() {
        let cli = Cli::parse_from(["aot", "--tui"]);
        assert_eq!(cli.command, None);
    }

    #[test]
    fn test_debug_flag() {
        let cli = Cli::parse_from(["aot", "--debug"]);
        assert_eq!(cli.debug, Some(true));
    }

    #[test]
    fn test_cli_display_with_debug_flag() {
        let cli = Cli::parse_from(["aot", "--debug"]);
        assert_eq!(cli.forwarded_args(), ["--debug=true"]);
    }
}
