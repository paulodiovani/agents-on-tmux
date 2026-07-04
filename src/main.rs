mod backends;
mod frontends;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    use backends::Tmux;
    let driver = backends::TmuxDriver;
    driver.create_session_if_not_exists()?;
    let terminal = ratatui::init();
    let mut app = frontends::App::new(&driver)?;
    app.run(terminal, &driver)?;
    ratatui::restore();
    Ok(())
}
