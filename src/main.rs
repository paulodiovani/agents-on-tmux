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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    use backends::Tmux;
    let cli = Cli::parse();

    let parent_session = backends::detect_parent_session()?;
    let parent_driver = backends::TmuxDriver::new(&parent_session);

    let nested_driver = backends::TmuxDriver::new(backends::SESSION_NAME);
    nested_driver.create_session_if_not_exists()?;

    if cli.tui {
        let terminal = ratatui::init();
        let mut app = frontends::App::new(&nested_driver)?;
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
