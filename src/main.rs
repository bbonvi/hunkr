use std::io::{self, Stdout};
use std::time::Duration;

use anyhow::Context;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use hunkr::app::App;

fn main() -> anyhow::Result<()> {
    let mut terminal = init_terminal().context("failed to initialize terminal")?;
    let mut app = App::bootstrap()?;

    let result = run_event_loop(&mut terminal, &mut app);
    restore_terminal(&mut terminal).context("failed to restore terminal")?;
    result
}

fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
) -> anyhow::Result<()> {
    loop {
        terminal
            .draw(|frame| app.draw(frame))
            .context("failed to render frame")?;

        if app.should_quit() {
            break;
        }

        if event::poll(Duration::from_millis(800)).context("event poll failed")? {
            let evt = event::read().context("event read failed")?;
            app.handle_event(evt);
        } else {
            app.tick();
        }
    }

    Ok(())
}

fn init_terminal() -> anyhow::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode().context("failed to enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
        .context("failed to enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend).context("failed to create terminal")
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> anyhow::Result<()> {
    disable_raw_mode().context("failed to disable raw mode")?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )
    .context("failed to leave alternate screen")?;
    terminal.show_cursor().context("failed to show cursor")?;
    Ok(())
}
