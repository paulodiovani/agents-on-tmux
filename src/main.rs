mod backends;
mod frontends;

use clap::Parser;

#[derive(Parser)]
#[command(name = "aot", about = "Agents on tmux")]
struct Cli {
    /// Launch the terminal UI
    #[arg(long)]
    tui: bool,
}

fn main() -> anyhow::Result<()> {
    use backends::tmux::{SESSION_NAME, Tmux, TmuxDriver, TmuxError, detect_parent_session};
    let cli = Cli::parse();

    let parent_session = detect_parent_session()?;
    if parent_session == SESSION_NAME {
        return Err(TmuxError::InsideOwnSession(parent_session).into());
    }
    let parent_driver = TmuxDriver::new(&parent_session);

    let nested_driver = TmuxDriver::new(SESSION_NAME);
    nested_driver.create_session_if_not_exists()?;

    if cli.tui {
        let terminal = ratatui::init();
        let mut app = frontends::tui::app::App::new(&nested_driver)?;
        app.run(terminal, &nested_driver)?;
        ratatui::restore();
    } else {
        let exe = std::env::current_exe()?;
        let command = format!("{} --tui", exe.to_string_lossy());
        parent_driver.split_window(&command)?;
        nested_driver.attach_session()?;
    }

    Ok(())
}
