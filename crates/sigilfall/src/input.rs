//! Keymap: turns key presses into commands for the application.
//!
//! Exposes [`Command`] and [`command_for`]. Only key presses count: crossterm
//! also reports key releases on Windows, and treating them as input would
//! play every key twice. This module decides nothing about the game itself;
//! the resulting [`Command::Act`] is submitted to `sigilfall_core` by the
//! `app` module.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use sigilfall_core::action::Action;
use sigilfall_core::geometry::Direction;

/// What the player asked for with one key press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Perform a game action on the player's turn.
    Act(Action),
    /// Leave the game.
    Quit,
}

/// Returns the command bound to `key`, or `None` when the key does nothing.
///
/// Bindings:
/// - moves: arrow keys, the keypad digits around 5 (`7 8 9 / 4 6 / 1 2 3`,
///   with or without Num Lock, where Home, Page Up, End and Page Down are the
///   diagonals) and the vi keys `h j k l y u b n`;
/// - wait: `.`, `5` or the keypad centre key without Num Lock;
/// - quit: `q`, Escape or Ctrl+C (raw mode turns Ctrl+C into a plain key).
///
/// Releases and repeats are ignored (`None`), so each physical press counts
/// once. Shift is accepted, because some keyboard layouts need it for `.` or
/// the digit row; any other key combined with Ctrl or Alt is ignored.
pub fn command_for(key: KeyEvent) -> Option<Command> {
    if key.kind != KeyEventKind::Press {
        return None;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return (key.code == KeyCode::Char('c')).then_some(Command::Quit);
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        return None;
    }
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => Some(Command::Quit),
        KeyCode::Char('.' | '5') | KeyCode::KeypadBegin => Some(Command::Act(Action::Wait)),
        code => direction_for(code).map(|direction| Command::Act(Action::Move(direction))),
    }
}

/// Returns the direction a movement key points to, or `None` for any other
/// key.
///
/// Digits follow the keypad layout, so `8` is up and `3` is down-right; the
/// navigation keys are what the keypad sends with Num Lock off.
const fn direction_for(code: KeyCode) -> Option<Direction> {
    let direction = match code {
        KeyCode::Up | KeyCode::Char('k' | '8') => Direction::North,
        KeyCode::PageUp | KeyCode::Char('u' | '9') => Direction::NorthEast,
        KeyCode::Right | KeyCode::Char('l' | '6') => Direction::East,
        KeyCode::PageDown | KeyCode::Char('n' | '3') => Direction::SouthEast,
        KeyCode::Down | KeyCode::Char('j' | '2') => Direction::South,
        KeyCode::End | KeyCode::Char('b' | '1') => Direction::SouthWest,
        KeyCode::Left | KeyCode::Char('h' | '4') => Direction::West,
        KeyCode::Home | KeyCode::Char('y' | '7') => Direction::NorthWest,
        _ => return None,
    };
    Some(direction)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A key press of `code` without modifiers.
    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// The command expected for a move in `direction`.
    const fn step(direction: Direction) -> Option<Command> {
        Some(Command::Act(Action::Move(direction)))
    }

    #[test]
    fn every_direction_has_an_arrow_or_navigation_key_a_digit_and_a_vi_key() {
        let bindings = [
            (Direction::North, KeyCode::Up, '8', 'k'),
            (Direction::NorthEast, KeyCode::PageUp, '9', 'u'),
            (Direction::East, KeyCode::Right, '6', 'l'),
            (Direction::SouthEast, KeyCode::PageDown, '3', 'n'),
            (Direction::South, KeyCode::Down, '2', 'j'),
            (Direction::SouthWest, KeyCode::End, '1', 'b'),
            (Direction::West, KeyCode::Left, '4', 'h'),
            (Direction::NorthWest, KeyCode::Home, '7', 'y'),
        ];

        for (direction, navigation, digit, vi) in bindings {
            for code in [navigation, KeyCode::Char(digit), KeyCode::Char(vi)] {
                assert_eq!(command_for(press(code)), step(direction), "{code:?}");
            }
        }
    }

    #[test]
    fn keypad_digits_move_like_the_digit_row() {
        let keypad_eight = KeyEvent::new_with_kind_and_state(
            KeyCode::Char('8'),
            KeyModifiers::NONE,
            KeyEventKind::Press,
            ratatui::crossterm::event::KeyEventState::KEYPAD,
        );

        assert_eq!(command_for(keypad_eight), step(Direction::North));
    }

    #[test]
    fn dot_five_and_keypad_centre_wait() {
        for code in [KeyCode::Char('.'), KeyCode::Char('5'), KeyCode::KeypadBegin] {
            assert_eq!(
                command_for(press(code)),
                Some(Command::Act(Action::Wait)),
                "{code:?}"
            );
        }
    }

    #[test]
    fn q_escape_and_ctrl_c_quit() {
        assert_eq!(command_for(press(KeyCode::Char('q'))), Some(Command::Quit));
        assert_eq!(command_for(press(KeyCode::Esc)), Some(Command::Quit));
        assert_eq!(
            command_for(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(Command::Quit)
        );
    }

    #[test]
    fn releases_and_repeats_are_ignored() {
        for kind in [KeyEventKind::Release, KeyEventKind::Repeat] {
            for code in [KeyCode::Right, KeyCode::Char('.'), KeyCode::Char('q')] {
                let key = KeyEvent::new_with_kind(code, KeyModifiers::NONE, kind);
                assert_eq!(command_for(key), None, "{kind:?} {code:?}");
            }
        }
    }

    #[test]
    fn shift_is_accepted_for_layouts_that_need_it() {
        let shifted_dot = KeyEvent::new(KeyCode::Char('.'), KeyModifiers::SHIFT);

        assert_eq!(command_for(shifted_dot), Some(Command::Act(Action::Wait)));
    }

    #[test]
    fn keys_with_ctrl_or_alt_and_unbound_keys_do_nothing() {
        for key in [
            KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL),
            KeyEvent::new(KeyCode::Left, KeyModifiers::ALT),
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::ALT),
            press(KeyCode::Char('z')),
            press(KeyCode::Char('K')),
            press(KeyCode::Enter),
            press(KeyCode::F(1)),
        ] {
            assert_eq!(command_for(key), None, "{key:?}");
        }
    }
}
