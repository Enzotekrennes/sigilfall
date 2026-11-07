//! The application: which screen is active, what keys do, and the event loop.
//!
//! Exposes [`App`] and [`State`]. Input handling ([`App::handle_key`]) and
//! drawing ([`App::render`]) never touch the real terminal, so tests drive
//! the whole application with simulated keys and render it into ratatui's
//! `TestBackend`. Only [`App::run`] reads the terminal; preparing and
//! restoring the terminal is left to `main`.

use anyhow::Context as _;
use ratatui::crossterm::event::{self, Event, KeyEvent};
use ratatui::{DefaultTerminal, Frame};
use sigilfall_core::game::Game;

use crate::input::{self, Command};
use crate::ui;

/// The screen the application is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// A game is in progress and keys control the player.
    Playing,
    /// The player asked to leave; the event loop stops.
    Quit,
}

/// The running application: a game and the screen showing it.
#[derive(Debug)]
pub struct App {
    /// The game in progress.
    game: Game,
    /// The active screen.
    state: State,
}

impl App {
    /// Starts the application on a new game, in [`State::Playing`].
    pub fn new() -> Self {
        Self {
            game: Game::new(),
            state: State::Playing,
        }
    }

    /// Returns the active screen, so tests can check it.
    #[cfg(test)]
    pub const fn state(&self) -> State {
        self.state
    }

    /// Returns the game in progress, so tests can check its state.
    #[cfg(test)]
    pub const fn game(&self) -> &Game {
        &self.game
    }

    /// Reacts to one key event.
    ///
    /// The key is translated by `input::command_for`: releases, repeats and
    /// unbound keys do nothing. A game action is submitted to the game, which
    /// may refuse it (a move into a wall) without any time passing. Once the
    /// application has quit, keys are ignored.
    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.state != State::Playing {
            return;
        }
        match input::command_for(key) {
            None => {}
            Some(Command::Quit) => self.state = State::Quit,
            Some(Command::Act(action)) => {
                // The events will feed the message log; there is no log yet,
                // and the map is drawn from the game state itself.
                self.game.submit(action);
            }
        }
    }

    /// Draws the active screen over the whole frame.
    ///
    /// Any frame size is accepted: below 80 × 24 a message asks for a larger
    /// terminal instead (see `ui::render`).
    pub fn render(&self, frame: &mut Frame) {
        ui::render(frame, &self.game);
    }

    /// Runs the event loop on `terminal` until the player quits: draw, wait
    /// for the next terminal event, handle it, repeat.
    ///
    /// A resize event needs no handling of its own: the next draw adapts to
    /// the new size.
    ///
    /// # Errors
    ///
    /// Returns an error if drawing or reading terminal input fails. The
    /// terminal is left as it is; restoring it is the caller's job.
    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> anyhow::Result<()> {
        while self.state == State::Playing {
            terminal
                .draw(|frame| self.render(frame))
                .context("cannot draw the game screen on the terminal")?;
            if let Event::Key(key) = event::read().context("cannot read terminal input")? {
                self.handle_key(key);
            }
        }
        Ok(())
    }
}

impl Default for App {
    /// Same as [`App::new`].
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    //! Test harness: drives [`App`] with simulated keys and renders it into
    //! a `TestBackend`, without any real terminal.

    use insta::assert_snapshot;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
    use sigilfall_core::game::PLAYER_START;
    use sigilfall_core::geometry::{Direction, Position};

    use super::*;

    /// A key press of `code`, as crossterm reports it.
    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// The release of `code`, which crossterm also reports on Windows.
    fn release(code: KeyCode) -> KeyEvent {
        KeyEvent::new_with_kind(code, KeyModifiers::NONE, KeyEventKind::Release)
    }

    /// Sends a press of each key in `codes` to `app`, in order.
    fn press_all(app: &mut App, codes: &[KeyCode]) {
        for &code in codes {
            app.handle_key(press(code));
        }
    }

    /// The player's current cell.
    fn player_position(app: &App) -> Position {
        app.game().player_actor().position()
    }

    /// Renders `app` once into a new `width` × `height` test terminal.
    fn render(app: &App, width: u16, height: u16) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
        terminal
    }

    #[test]
    fn new_game_shows_the_room_and_the_player() {
        let app = App::new();

        assert_eq!(app.state(), State::Playing);
        assert_snapshot!(render(&app, 80, 24).backend());
    }

    #[test]
    fn movement_keys_move_the_player_one_cell_each() {
        let mut app = App::new();
        let ticks = app.game().ticks();

        // Arrow, keypad digit, vi key, then a keypad diagonal without Num Lock.
        press_all(
            &mut app,
            &[
                KeyCode::Right,
                KeyCode::Char('2'),
                KeyCode::Char('l'),
                KeyCode::PageDown,
            ],
        );

        let expected = PLAYER_START
            .step(Direction::East)
            .step(Direction::South)
            .step(Direction::East)
            .step(Direction::SouthEast);
        assert_eq!(player_position(&app), expected);
        assert_eq!(app.game().ticks(), ticks + 4, "each move took one turn");
        assert_snapshot!(render(&app, 80, 24).backend());
    }

    #[test]
    fn waiting_lets_time_pass_without_moving() {
        let mut app = App::new();
        let ticks = app.game().ticks();

        press_all(&mut app, &[KeyCode::Char('.'), KeyCode::Char('5')]);

        assert_eq!(player_position(&app), PLAYER_START);
        assert_eq!(app.game().ticks(), ticks + 2);
    }

    #[test]
    fn wall_stops_the_player_and_no_time_passes() {
        let mut app = App::new();
        // The start is four cells right of the floor next to the left wall.
        press_all(&mut app, &[KeyCode::Char('h'); 4]);
        let at_wall = Position::new(1, PLAYER_START.y);
        assert_eq!(player_position(&app), at_wall);
        let ticks = app.game().ticks();

        press_all(&mut app, &[KeyCode::Left, KeyCode::Home, KeyCode::End]);

        assert_eq!(player_position(&app), at_wall);
        assert_eq!(app.game().ticks(), ticks, "blocked moves take no time");
        assert_eq!(app.state(), State::Playing);
    }

    #[test]
    fn press_then_release_counts_as_one_move() {
        let mut app = App::new();
        let ticks = app.game().ticks();

        app.handle_key(press(KeyCode::Right));
        app.handle_key(release(KeyCode::Right));

        assert_eq!(player_position(&app), PLAYER_START.step(Direction::East));
        assert_eq!(app.game().ticks(), ticks + 1);
    }

    #[test]
    fn q_quits() {
        let mut app = App::new();

        app.handle_key(press(KeyCode::Char('q')));

        assert_eq!(app.state(), State::Quit);
    }

    #[test]
    fn escape_quits_and_later_keys_are_ignored() {
        let mut app = App::new();

        press_all(&mut app, &[KeyCode::Esc, KeyCode::Right]);

        assert_eq!(app.state(), State::Quit);
        assert_eq!(player_position(&app), PLAYER_START);
    }

    #[test]
    fn releasing_q_does_not_quit() {
        let mut app = App::new();

        app.handle_key(release(KeyCode::Char('q')));

        assert_eq!(app.state(), State::Playing);
    }

    #[test]
    fn terminal_smaller_than_80_by_24_shows_a_resize_message() {
        let app = App::new();

        assert_snapshot!(render(&app, 60, 20).backend());
        for (width, height) in [(79, 24), (80, 23)] {
            let text = render(&app, width, height).backend().to_string();
            assert!(
                text.contains("Enlarge the terminal"),
                "{width} × {height}:\n{text}"
            );
            assert!(!text.contains('@'), "{width} × {height}:\n{text}");
        }
    }

    #[test]
    fn resizing_switches_between_message_and_game_without_panicking() {
        let app = App::new();
        let mut terminal = render(&app, 80, 24);

        for (width, height) in [(40, 10), (1, 1), (79, 30), (80, 24), (120, 40)] {
            terminal.backend_mut().resize(width, height);
            terminal.draw(|frame| app.render(frame)).unwrap();
            let text = terminal.backend().to_string();
            let fits = width >= ui::MIN_WIDTH && height >= ui::MIN_HEIGHT;
            assert_eq!(text.contains('@'), fits, "{width} × {height}:\n{text}");
        }
    }
}
