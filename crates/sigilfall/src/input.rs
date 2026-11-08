//! Keymap: turns key presses into commands for the application.
//!
//! Exposes one keymap per kind of screen: [`command_for`] on the game
//! screen ([`Command`]), [`trace_command_for`] while a sigil is traced
//! ([`TraceCommand`]), [`aim_command_for`] while its direction is asked
//! ([`AimCommand`]) and [`verdict_command_for`] while the verdict of a cast
//! is shown ([`VerdictCommand`]). Only key presses count: crossterm also
//! reports key releases on Windows, and treating them as input would play
//! every key twice. Ctrl+C quits from every screen, since raw mode turns it
//! into a plain key. This module decides nothing about the game itself; the
//! `app` module submits the resulting actions to `sigilfall_core`.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use sigilfall_core::action::Action;
use sigilfall_core::geometry::Direction;
use sigilfall_core::sigil::Node;

/// What the player asked for with one key press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Perform a game action on the player's turn.
    Act(Action),
    /// Open the sigil overlay to trace a sigil.
    Trace,
    /// Leave the game.
    Quit,
}

/// What the player asked for with one key press while tracing a sigil.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceCommand {
    /// Link this node to the trace.
    Link(Node),
    /// Remove the last node linked.
    Undo,
    /// Validate the trace.
    Cast,
    /// Close the overlay without casting.
    Cancel,
    /// Leave the game.
    Quit,
}

/// What the player asked for with one key press while the direction of a
/// sigil is asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AimCommand {
    /// Cast the sigil in this direction.
    Aim(Direction),
    /// Go back to the trace, which is kept.
    Back,
    /// Leave the game.
    Quit,
}

/// What the player asked for with one key press while the verdict of a
/// cast is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerdictCommand {
    /// Close the verdict and go back to the game.
    Continue,
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
/// - trace a sigil: `t`, as in the mock-ups of `docs/design.md` (the
///   digits are taken by movement, and `t` is not a vi key);
/// - quit: `q`, Escape or Ctrl+C (raw mode turns Ctrl+C into a plain key).
///
/// Releases and repeats are ignored (`None`), so each physical press counts
/// once. Shift is accepted, because some keyboard layouts need it for `.` or
/// the digit row; any other key combined with Ctrl or Alt is ignored.
pub fn command_for(key: KeyEvent) -> Option<Command> {
    let code = match plain_press(key)? {
        Press::Quit => return Some(Command::Quit),
        Press::Code(code) => code,
    };
    match code {
        KeyCode::Char('q') | KeyCode::Esc => Some(Command::Quit),
        KeyCode::Char('t') => Some(Command::Trace),
        KeyCode::Char('.' | '5') | KeyCode::KeypadBegin => Some(Command::Act(Action::Wait)),
        code => direction_for(code).map(|direction| Command::Act(Action::Move(direction))),
    }
}

/// Returns the command bound to `key` while a sigil is traced, or `None`
/// when the key does nothing there.
///
/// Bindings (decision D7):
/// - link a node: the digits `1` to `9`, from the keypad or the top row,
///   numbered like the keypad and the grid; with Num Lock off, the keys the
///   keypad sends instead (Home, Up, Page Up, Left, centre, Right, End,
///   Down, Page Down) link the node under them, so tracing on the keypad
///   never depends on Num Lock;
/// - undo the last node: Backspace;
/// - cast: Enter;
/// - cancel: Escape;
/// - quit: Ctrl+C only, so that `q` pressed by mistake loses nothing.
///
/// Releases and repeats are ignored, Shift is accepted (some layouts need
/// it for the digit row), and keys with Alt or Ctrl, Ctrl+C aside, do
/// nothing.
pub fn trace_command_for(key: KeyEvent) -> Option<TraceCommand> {
    let code = match plain_press(key)? {
        Press::Quit => return Some(TraceCommand::Quit),
        Press::Code(code) => code,
    };
    match code {
        KeyCode::Enter => Some(TraceCommand::Cast),
        KeyCode::Backspace => Some(TraceCommand::Undo),
        KeyCode::Esc => Some(TraceCommand::Cancel),
        code => node_for(code).map(TraceCommand::Link),
    }
}

/// Returns the command bound to `key` while the direction of a sigil is
/// asked, or `None` when the key does nothing there.
///
/// Bindings: the movement keys of [`command_for`] aim in their direction
/// (arrows, keypad digits, vi keys); Escape goes back to the trace; Ctrl+C
/// quits. The wait keys do nothing: a direction is needed.
pub fn aim_command_for(key: KeyEvent) -> Option<AimCommand> {
    let code = match plain_press(key)? {
        Press::Quit => return Some(AimCommand::Quit),
        Press::Code(code) => code,
    };
    match code {
        KeyCode::Esc => Some(AimCommand::Back),
        code => direction_for(code).map(AimCommand::Aim),
    }
}

/// Returns the command bound to `key` while the verdict of a cast is shown.
///
/// Any key press closes the verdict (Enter is the one the key help names),
/// without doing anything else, so a key pressed while reading is never
/// played in the game; Ctrl+C quits. Releases, repeats and keys with Alt
/// give `None`.
pub fn verdict_command_for(key: KeyEvent) -> Option<VerdictCommand> {
    match plain_press(key)? {
        Press::Quit => Some(VerdictCommand::Quit),
        Press::Code(_) => Some(VerdictCommand::Continue),
    }
}

/// A key press that may mean something, once the modifiers are checked.
enum Press {
    /// Ctrl+C: leave the game, from any screen.
    Quit,
    /// A key pressed alone or with Shift.
    Code(KeyCode),
}

/// Sorts out the presses every keymap ignores: releases, repeats, keys
/// with Alt, and keys with Ctrl other than Ctrl+C (`None`). Ctrl+C gives
/// [`Press::Quit`]; any other key, alone or with Shift, its code.
fn plain_press(key: KeyEvent) -> Option<Press> {
    if key.kind != KeyEventKind::Press {
        return None;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return (key.code == KeyCode::Char('c')).then_some(Press::Quit);
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        return None;
    }
    Some(Press::Code(key.code))
}

/// Returns the grid node a key links while tracing, or `None` for any other
/// key: the digits `1` to `9`, and the keys the keypad sends with Num Lock
/// off, which stand where the digits are.
const fn node_for(code: KeyCode) -> Option<Node> {
    let digit = match code {
        KeyCode::Char(digit @ '1'..='9') => digit as u8 - b'0',
        KeyCode::End => 1,
        KeyCode::Down => 2,
        KeyCode::PageDown => 3,
        KeyCode::Left => 4,
        KeyCode::KeypadBegin => 5,
        KeyCode::Right => 6,
        KeyCode::Home => 7,
        KeyCode::Up => 8,
        KeyCode::PageUp => 9,
        _ => return None,
    };
    Node::new(digit)
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
    fn t_opens_the_sigil_overlay() {
        assert_eq!(command_for(press(KeyCode::Char('t'))), Some(Command::Trace));
        assert_eq!(
            command_for(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::ALT)),
            None
        );
    }

    /// The node numbered `digit`.
    fn node(digit: u8) -> Node {
        Node::new(digit).expect("a digit from 1 to 9")
    }

    #[test]
    fn while_tracing_digits_and_the_keypad_without_num_lock_link_their_node() {
        let navigation = [
            (1, KeyCode::End),
            (2, KeyCode::Down),
            (3, KeyCode::PageDown),
            (4, KeyCode::Left),
            (5, KeyCode::KeypadBegin),
            (6, KeyCode::Right),
            (7, KeyCode::Home),
            (8, KeyCode::Up),
            (9, KeyCode::PageUp),
        ];
        for (digit, key) in navigation {
            let expected = Some(TraceCommand::Link(node(digit)));
            let char_key = KeyCode::Char(char::from(b'0' + digit));
            assert_eq!(trace_command_for(press(char_key)), expected, "{digit}");
            let shifted = KeyEvent::new(char_key, KeyModifiers::SHIFT);
            assert_eq!(trace_command_for(shifted), expected, "Shift+{digit}");
            assert_eq!(trace_command_for(press(key)), expected, "{key:?}");
        }
        assert_eq!(trace_command_for(press(KeyCode::Char('0'))), None);
    }

    #[test]
    fn while_tracing_enter_casts_backspace_undoes_and_escape_cancels() {
        assert_eq!(
            trace_command_for(press(KeyCode::Enter)),
            Some(TraceCommand::Cast)
        );
        assert_eq!(
            trace_command_for(press(KeyCode::Backspace)),
            Some(TraceCommand::Undo)
        );
        assert_eq!(
            trace_command_for(press(KeyCode::Esc)),
            Some(TraceCommand::Cancel)
        );
    }

    #[test]
    fn while_tracing_only_ctrl_c_quits_and_other_keys_do_nothing() {
        assert_eq!(
            trace_command_for(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(TraceCommand::Quit)
        );
        for key in [
            press(KeyCode::Char('q')),
            press(KeyCode::Char('t')),
            press(KeyCode::Char('.')),
            KeyEvent::new(KeyCode::Char('5'), KeyModifiers::ALT),
            KeyEvent::new_with_kind(
                KeyCode::Char('5'),
                KeyModifiers::NONE,
                KeyEventKind::Release,
            ),
            KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::NONE, KeyEventKind::Repeat),
        ] {
            assert_eq!(trace_command_for(key), None, "{key:?}");
        }
    }

    #[test]
    fn while_aiming_movement_keys_aim_and_escape_goes_back() {
        for (code, direction) in [
            (KeyCode::Right, Direction::East),
            (KeyCode::Char('7'), Direction::NorthWest),
            (KeyCode::Char('j'), Direction::South),
            (KeyCode::PageDown, Direction::SouthEast),
        ] {
            assert_eq!(
                aim_command_for(press(code)),
                Some(AimCommand::Aim(direction)),
                "{code:?}"
            );
        }
        assert_eq!(aim_command_for(press(KeyCode::Esc)), Some(AimCommand::Back));
        for code in [
            KeyCode::Char('5'),
            KeyCode::Char('.'),
            KeyCode::Enter,
            KeyCode::Char('q'),
        ] {
            assert_eq!(aim_command_for(press(code)), None, "{code:?}");
        }
    }

    #[test]
    fn a_verdict_closes_on_any_press_and_ctrl_c_quits() {
        for code in [
            KeyCode::Enter,
            KeyCode::Char('q'),
            KeyCode::Right,
            KeyCode::Esc,
        ] {
            assert_eq!(
                verdict_command_for(press(code)),
                Some(VerdictCommand::Continue),
                "{code:?}"
            );
        }
        assert_eq!(
            verdict_command_for(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(VerdictCommand::Quit)
        );
        let release =
            KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::NONE, KeyEventKind::Release);
        assert_eq!(verdict_command_for(release), None);
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
