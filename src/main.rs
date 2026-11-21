mod backends;
mod frontends;

use std::path::PathBuf;
use std::sync::Arc;

use backends::config::{Config, LaunchMode};
use backends::process::SystemProcesses;
use backends::signals::OnSignal;
use backends::tmux::{SESSION_NAME, SOCKET_NAME, Tmux, TmuxDriver, TmuxError};
use clap::{Parser, Subcommand};
use frontends::mcp::{McpHandler, McpServer};
use frontends::tui::app::App;

use crate::backends::logger;

/// Process-wide effects of launching aot, so the launch flow can run against fakes.
#[cfg_attr(test, mockall::automock)]
trait Launcher {
    /// Driver for the tmux session aot was started from.
    #[allow(clippy::needless_lifetimes)]
    fn parent_session<'a>(&self, tmux_env: Option<&'a str>) -> Result<Box<dyn Tmux>, TmuxError>;
    /// Driver for aot's own session, on its own tmux server.
    fn nested_session(&self) -> Box<dyn Tmux>;
    /// Starts the MCP server; `None` when another aot owns it or it failed.
    fn start_mcp_server(&self) -> Option<McpServer>;
    /// Installs the signal handlers; failures are logged.
    fn install_signals(&self, policy: OnSignal);
    /// Runs the TUI in this terminal until it quits.
    fn run_tui(
        &self,
        nested: Box<dyn Tmux>,
        parent: Box<dyn Tmux>,
        pane_id: Option<String>,
        panel_width: Option<u16>,
    ) -> anyhow::Result<()>;
    /// Path of the running aot executable.
    fn current_exe(&self) -> std::io::Result<PathBuf>;
}

#[derive(Subcommand, Debug, PartialEq)]
enum Command {
    /// Run the MCP stdio server for agents (connects to the running aot)
    Mcp,
}

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

/// Launches aot against the real tmux, terminal and process.
struct SystemLauncher;

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

impl Launcher for SystemLauncher {
    fn parent_session(&self, tmux_env: Option<&str>) -> Result<Box<dyn Tmux>, TmuxError> {
        Ok(Box::new(TmuxDriver::for_current_session(tmux_env)?))
    }

    fn nested_session(&self) -> Box<dyn Tmux> {
        Box::new(TmuxDriver::new_with_socket(SESSION_NAME, SOCKET_NAME))
    }

    fn start_mcp_server(&self) -> Option<McpServer> {
        let path = frontends::mcp::socket_path();
        let tmux = Arc::new(TmuxDriver::new_with_socket(SESSION_NAME, SOCKET_NAME));
        let default_dir = std::env::current_dir().unwrap_or_else(|_| std::env::temp_dir());
        let handler = McpHandler::new(frontends::mcp::all_tools(tmux, default_dir));
        match McpServer::start(&path, Arc::new(handler)) {
            Ok(Some(server)) => {
                logger::info!("main: MCP server listening on {}", path.display());
                Some(server)
            }
            Ok(None) => {
                logger::info!("main: MCP socket owned by another aot instance; not starting");
                None
            }
            Err(error) => {
                logger::error!("main: MCP server failed to start: {error}");
                None
            }
        }
    }

    fn install_signals(&self, policy: OnSignal) {
        if let Err(error) = backends::signals::install(policy) {
            logger::error!("main: signal install failed: {error}");
        }
    }

    fn run_tui(
        &self,
        nested: Box<dyn Tmux>,
        parent: Box<dyn Tmux>,
        pane_id: Option<String>,
        panel_width: Option<u16>,
    ) -> anyhow::Result<()> {
        let mut app = App::new(
            nested,
            parent,
            Box::new(SystemProcesses),
            pane_id,
            panel_width,
        )?;
        let terminal = ratatui::init();
        let _ = crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture);
        let result = app.run(terminal);
        let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableMouseCapture);
        if let Err(error) = ratatui::try_restore() {
            logger::error!("main: failed to restore terminal: {error}");
        }
        result
    }

    fn current_exe(&self) -> std::io::Result<PathBuf> {
        std::env::current_exe()
    }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if cli.command == Some(Command::Mcp) {
        // The shim must keep serving the agent even when the log is unusable.
        if cli.debug.unwrap_or(false) {
            let _ = init_logging("shim");
        }
        let path = frontends::mcp::socket_path();
        let mut stdout = std::io::stdout();
        let result = frontends::mcp::run_shim(
            &path,
            cli.tui_pane.as_deref(),
            std::io::stdin(),
            &mut stdout,
        );
        if let Err(error) = &result {
            logger::error!("shim: {error}");
        }
        return Ok(result?);
    }

    let config = Config::parse()?.merge(&cli);

    // Logger setup must precede any tmux session/thread creation, so a fatal
    // --debug failure leaves no server, session, or thread behind.
    if config.debug.unwrap_or(false) {
        init_logging(log_role(config.launch_mode()))?;
        logger::info!("main: starting aot {}", env!("CARGO_PKG_VERSION"));
        logger::info!("main: config {config:?}");
        logger::info!("main: launch mode {:?}", config.launch_mode());
    }

    // ratatui::init() installs its own terminal-restoring panic hook and
    // requires other hooks to be installed first.
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        logger::error!("panic: {info}");
        previous_hook(info);
    }));

    frontends::tui::icons::set_icon_fonts(
        config.nerd_font.unwrap_or(false),
        config.font_awesome.unwrap_or(false),
    );

    run(&cli, &config, &SystemLauncher)?;

    if let Some((signo, _name)) = backends::signals::received() {
        std::process::exit(128 + signo);
    }

    Ok(())
}

/// Ensures aot's session exists, starts the MCP server and launches the
/// interface the config asks for, returning when it ends.
fn run(cli: &Cli, config: &Config, launcher: &dyn Launcher) -> anyhow::Result<()> {
    let tmux_env = config.tmux_env.as_deref();
    let parent = launcher.parent_session(tmux_env)?;
    let nested = launcher.nested_session();
    nested.create_session_if_not_exists(tmux_env)?;

    let _mcp_server = launcher.start_mcp_server();

    let pane_id = config.tui_pane.clone();
    if pane_id.is_none() {
        logger::debug!("main: TMUX_PANE not set; focus tracking disabled");
    }

    match config.launch_mode() {
        LaunchMode::NoTui => {
            // Attaching blocks, so signals must end the process right away.
            launcher.install_signals(OnSignal::Exit);
            nested.attach_session()?;
        }
        LaunchMode::TuiOnly { width } => {
            logger::info!("main: starting tui");
            launcher.install_signals(OnSignal::Shutdown);
            launcher.run_tui(nested, parent, pane_id, width)?;
        }
        LaunchMode::Split { width } => {
            launcher.install_signals(OnSignal::Exit);
            let exe = launcher.current_exe()?;
            let command = split_command(&exe.to_string_lossy(), cli, width);
            parent.split_window(&command, width)?;
            nested.attach_session()?;
        }
    }

    Ok(())
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

/// Role tag that identifies this process's lines in the shared log file.
fn log_role(mode: LaunchMode) -> &'static str {
    match mode {
        LaunchMode::TuiOnly { .. } => "tui",
        LaunchMode::Split { .. } | LaunchMode::NoTui => "main",
    }
}

/// Starts file logging under the user cache dir.
fn init_logging(role: &'static str) -> std::io::Result<()> {
    let path = dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("aot");
    std::fs::create_dir_all(&path)?;
    logger::init(&path.join("aot.log"), role)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::tmux::MockTmux;
    use mockall::predicate::eq;

    const TMUX_ENV: &str = "/tmp/tmux-1000/default,1,0";

    fn config(args: &[&str]) -> (Cli, Config) {
        let cli = Cli::parse_from(args);
        let mut config: Config = (&cli).into();
        config.tmux_env = Some(TMUX_ENV.to_string());
        (cli, config)
    }

    /// A nested session that gets created, then optionally attached.
    fn nested_session(attaches: bool) -> MockTmux {
        let mut nested = MockTmux::new();
        nested
            .expect_create_session_if_not_exists()
            .withf(|env| *env == Some(TMUX_ENV))
            .times(1)
            .returning(|_| Ok(()));
        nested
            .expect_attach_session()
            .times(usize::from(attaches))
            .returning(|| Ok(()));
        nested
    }

    fn launcher(parent: MockTmux, nested: MockTmux) -> MockLauncher {
        let mut launcher = MockLauncher::new();
        launcher
            .expect_parent_session()
            .withf(|env| *env == Some(TMUX_ENV))
            .times(1)
            .return_once(move |_| Ok(Box::new(parent)));
        launcher
            .expect_nested_session()
            .times(1)
            .return_once(move || Box::new(nested));
        launcher
            .expect_start_mcp_server()
            .times(1)
            .returning(|| None);
        launcher
    }

    #[test]
    fn test_run_without_tui_attaches_nested_session() {
        let (cli, config) = config(&["aot", "--no-tui"]);
        let mut launcher = launcher(MockTmux::new(), nested_session(true));
        launcher
            .expect_install_signals()
            .with(eq(OnSignal::Exit))
            .times(1)
            .return_const(());

        run(&cli, &config, &launcher).unwrap();
    }

    #[test]
    fn test_run_tui_only_runs_tui_in_this_pane() {
        let (cli, mut config) = config(&["aot", "--tui", "--tui-width=40"]);
        config.tui_pane = Some("%3".to_string());
        let mut launcher = launcher(MockTmux::new(), nested_session(false));
        launcher
            .expect_install_signals()
            .with(eq(OnSignal::Shutdown))
            .times(1)
            .return_const(());
        launcher
            .expect_run_tui()
            .withf(|_, _, pane_id, width| pane_id.as_deref() == Some("%3") && *width == Some(40))
            .times(1)
            .returning(|_, _, _, _| Ok(()));

        run(&cli, &config, &launcher).unwrap();
    }

    #[test]
    fn test_run_split_launches_tui_beside_and_attaches() {
        let (cli, config) = config(&["aot", "--nerd-font"]);
        let mut parent = MockTmux::new();
        parent
            .expect_split_window()
            .with(
                eq("/usr/bin/aot --nerd-font=true --tui=true --tui-width=35"),
                eq(35),
            )
            .times(1)
            .returning(|_, _| Ok("%9".to_string()));
        let mut launcher = launcher(parent, nested_session(true));
        launcher
            .expect_install_signals()
            .with(eq(OnSignal::Exit))
            .times(1)
            .return_const(());
        launcher
            .expect_current_exe()
            .returning(|| Ok(PathBuf::from("/usr/bin/aot")));

        run(&cli, &config, &launcher).unwrap();
    }

    #[test]
    fn test_run_fails_outside_tmux() {
        let (cli, mut config) = config(&["aot"]);
        config.tmux_env = None;
        let mut launcher = MockLauncher::new();
        launcher
            .expect_parent_session()
            .returning(|_| Err(TmuxError::NotInsideTmux));
        launcher.expect_nested_session().never();

        let error = run(&cli, &config, &launcher).unwrap_err();

        assert!(matches!(
            error.downcast_ref::<TmuxError>(),
            Some(TmuxError::NotInsideTmux)
        ));
    }

    #[test]
    fn test_run_stops_when_session_cannot_be_created() {
        let (cli, config) = config(&["aot"]);
        let mut nested = MockTmux::new();
        nested
            .expect_create_session_if_not_exists()
            .returning(|_| Err(TmuxError::InsideOwnServer(SOCKET_NAME.to_string())));
        let mut launcher = MockLauncher::new();
        launcher
            .expect_parent_session()
            .return_once(|_| Ok(Box::new(MockTmux::new())));
        launcher
            .expect_nested_session()
            .return_once(move || Box::new(nested));
        launcher.expect_start_mcp_server().never();
        launcher.expect_install_signals().never();

        assert!(run(&cli, &config, &launcher).is_err());
    }

    #[test]
    fn test_run_reports_split_failure() {
        let (cli, config) = config(&["aot"]);
        let mut parent = MockTmux::new();
        parent.expect_split_window().returning(|_, _| {
            Err(TmuxError::CommandFailed {
                message: "split-window failed".to_string(),
                stderr: String::new(),
                code: Some(1),
            })
        });
        let mut launcher = launcher(parent, nested_session(false));
        launcher.expect_install_signals().return_const(());
        launcher
            .expect_current_exe()
            .returning(|| Ok(PathBuf::from("aot")));

        assert!(run(&cli, &config, &launcher).is_err());
    }

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
    fn test_log_role_by_launch_mode() {
        assert_eq!(log_role(LaunchMode::TuiOnly { width: None }), "tui");
        assert_eq!(log_role(LaunchMode::Split { width: 35 }), "main");
        assert_eq!(log_role(LaunchMode::NoTui), "main");
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
