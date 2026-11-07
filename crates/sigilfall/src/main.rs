//! Entry point of the Sigilfall terminal game.
//!
//! This binary owns everything tied to the terminal: command line, input,
//! screens and rendering. Game rules live in the `sigilfall-core` crate.
//!
//! This file only prepares the terminal (raw mode, alternate screen, hidden
//! cursor), runs the [`app::App`] and puts the terminal back the way it was.
//! The terminal is restored on a normal exit, when the game returns an error
//! and when it panics, so the player's shell is never left unusable.

mod app;
mod input;
mod ui;

use std::io::{self, Stdout};

use anyhow::Context as _;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::cursor::{Hide, Show};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::{DefaultTerminal, Terminal};

use crate::app::App;

/// Runs the game in the current terminal until the player quits.
///
/// # Errors
///
/// Returns an error if the terminal cannot be prepared (for example when
/// standard output is not a terminal), if reading input or drawing fails
/// during the game, or if the terminal cannot be restored afterwards. The
/// terminal is restored before any error is reported.
fn main() -> anyhow::Result<()> {
    install_panic_hook();

    let outcome = enter_terminal().and_then(|mut terminal| App::new().run(&mut terminal));
    // Restore even when the game failed, so the error is printed on the
    // normal screen; the game's own error is the more useful one to report.
    let restored = restore_terminal().context(
        "cannot restore the terminal; run `reset` (Unix) or reopen the window if it misbehaves",
    );
    outcome?;
    restored
}

/// Switches the terminal to raw mode, enters the alternate screen, hides the
/// cursor and wraps standard output in a ratatui terminal.
///
/// # Errors
///
/// Returns an error if any of these steps fails, typically because standard
/// output is not an interactive terminal. Steps already applied are undone
/// by the caller through [`restore_terminal`].
fn enter_terminal() -> anyhow::Result<DefaultTerminal> {
    enable_raw_mode().context("cannot switch the terminal to raw mode: is this a terminal?")?;
    execute!(io::stdout(), EnterAlternateScreen, Hide)
        .context("cannot enter the alternate screen of the terminal")?;
    Terminal::new(CrosstermBackend::<Stdout>::new(io::stdout()))
        .context("cannot set up drawing on the terminal")
}

/// Puts the terminal back in its normal state: raw mode off, main screen
/// back, cursor visible.
///
/// Every step is attempted even if an earlier one fails, so as much as
/// possible is restored. Safe to call when the terminal was only partly
/// prepared or already restored.
///
/// # Errors
///
/// Returns the first error met, after all steps have been attempted.
fn restore_terminal() -> io::Result<()> {
    let raw_mode = disable_raw_mode();
    let screen = execute!(io::stdout(), LeaveAlternateScreen, Show);
    raw_mode.and(screen)
}

/// Makes every panic restore the terminal before its message is printed.
///
/// Without this, the panic message would be written to the alternate screen
/// in raw mode and vanish, leaving the shell without a cursor or line
/// editing. The previously installed hook (the standard one by default)
/// still prints the message and backtrace afterwards.
fn install_panic_hook() {
    let print_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Nothing more can be done about a failed restore while panicking;
        // the panic message below matters more.
        let _ = restore_terminal();
        print_panic(info);
    }));
}
