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
    let driver = backends::TmuxDriver;
    driver.create_session_if_not_exists()?;

    if cli.tui {
        let terminal = ratatui::init();
        let mut app = frontends::App::new(&driver)?;
        app.run(terminal, &driver)?;
        ratatui::restore();
    } else {
        driver.attach_session()?;
    }

    Ok(())
}
