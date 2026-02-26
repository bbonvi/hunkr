use anyhow::Context;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io::{self, Stdout};

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
        if app.should_quit() {
            break;
        }

        if app.needs_redraw() {
            terminal
                .draw(|frame| app.draw(frame))
                .context("failed to render frame")?;
            app.mark_drawn();
        }

        if event::poll(app.poll_timeout()).context("event poll failed")? {
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
    let mut terminal = Terminal::new(backend).context("failed to create terminal")?;
    terminal.hide_cursor().context("failed to hide cursor")?;
    Ok(terminal)
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
