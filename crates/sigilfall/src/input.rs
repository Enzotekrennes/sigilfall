//! Keymap: turns key presses into commands for the application.
//!
//! Exposes one keymap per kind of screen: [`command_for`] on the game
//! screen ([`Command`]), [`trace_command_for`] while a sigil is traced
//! ([`TraceCommand`]), [`aim_command_for`] while its direction is asked
//! ([`AimCommand`]), [`verdict_command_for`] while the verdict of a cast
//! is shown ([`VerdictCommand`]), [`inventory_command_for`] while the
//! inventory is open ([`InventoryCommand`]), [`end_command_for`] on the end
//! screen ([`EndCommand`]), and for the screens before a game:
//! [`title_command_for`] on the title menu and [`confirm_command_for`] on
//! the confirmation before a saved game is abandoned ([`MenuCommand`]),
//! [`seed_command_for`] while a seed is typed ([`SeedCommand`]) and
//! [`options_command_for`] on the options screen ([`OptionsCommand`]).
//! Only key presses count: crossterm also reports key releases on Windows,
//! and treating them as input would play every key twice. Ctrl+C quits from
//! every screen, since raw mode turns it into a plain key; `q` quits from
//! the game, end and title screens only. Escape never quits: it closes an
//! overlay, a verdict or a sub-screen of the title, and does nothing on the
//! game, end and title screens, so a second press after closing one loses
//! nothing. This module decides nothing about the game itself; the `app`
//! module submits the resulting actions to `sigilfall_core`.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use sigilfall_core::action::Action;
use sigilfall_core::geometry::Direction;
use sigilfall_core::sigil::Node;

/// Second key that goes down the stairs, beside `>`: `D`, typed Shift+d.
///
/// `>` is the key of the genre, and the one the key help names. On an
/// AZERTY keyboard it is Shift and the `<` key next to it, as easy as Shift
/// and `.` on QWERTY; but some layouts, such as AZERTY on a laptop without
/// the `<` key, have no plain way to type it. `D` sits at the same place on
/// every Latin layout, reads as "descend", and its lower case is bound to
/// nothing on the game screen: `d` drops an item in the inventory, so a
/// player pressing it once too many after a drop never leaves the floor by
/// mistake.
pub const DESCEND_ALTERNATIVE_KEY: char = 'D';

/// What the player asked for with one key press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Perform a game action on the player's turn.
    Act(Action),
    /// Open the sigil overlay to trace a sigil.
    Trace,
    /// Open the inventory.
    Inventory,
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

/// What the player asked for with one key press while the inventory is
/// open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryCommand {
    /// Choose the item above the chosen one, or the last from the first.
    Previous,
    /// Choose the item below the chosen one, or the first from the last.
    Next,
    /// Choose the first item.
    First,
    /// Choose the last item.
    Last,
    /// Choose the item in this slot, counted from 0.
    Slot(usize),
    /// Use the chosen item.
    Use,
    /// Drop the chosen item on the player's cell.
    Drop,
    /// Close the inventory without doing anything.
    Close,
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

/// What the player asked for with one key press on the end screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndCommand {
    /// Go back to the title screen (Enter).
    Title,
    /// Leave the game (`q`).
    Quit,
    /// Leave the game at once (Ctrl+C): unlike the other keys, it is never
    /// held back while the end screen settles.
    Interrupt,
}

/// What the player asked for with one key press on a menu: the title menu
/// or the confirmation before a saved game is abandoned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuCommand {
    /// Choose the entry above, or the last from the first.
    Previous,
    /// Choose the entry below, or the first from the last.
    Next,
    /// Choose the first entry.
    First,
    /// Choose the last entry.
    Last,
    /// Do what the chosen entry says.
    Confirm,
    /// Leave the menu without choosing (the confirmation only).
    Back,
    /// Leave the game.
    Quit,
}

/// What the player asked for with one key press while typing a seed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedCommand {
    /// Add this character at the end of the seed.
    Type(char),
    /// Remove the last character.
    Erase,
    /// Start a game on the seed typed.
    Submit,
    /// Go back to the title menu.
    Back,
    /// Leave the game.
    Quit,
}

/// What the player asked for with one key press on the options screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionsCommand {
    /// Choose the entry above, or the last from the first.
    Previous,
    /// Choose the entry below, or the first from the last.
    Next,
    /// Choose the first entry.
    First,
    /// Choose the last entry.
    Last,
    /// Give the chosen option its previous value, wrapping around.
    PreviousValue,
    /// Give the chosen option its next value, wrapping around.
    NextValue,
    /// Next value on an option; back to the title on "Back".
    Confirm,
    /// Go back to the title menu.
    Back,
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
/// - pick up the item underfoot: `g`, as in the mock-up, or `,`, the key
///   of other roguelikes; neither is a vi key;
/// - open the inventory: `i`;
/// - go down the stairs: `>`, the key of the genre, or
///   [`DESCEND_ALTERNATIVE_KEY`]; the game refuses it off the stairs, at no
///   cost;
/// - quit: `q` or Ctrl+C (raw mode turns Ctrl+C into a plain key).
///
/// Escape is bound to nothing here: it closes the overlays, and a player
/// who presses it once too many after closing one must not lose the game.
/// Releases and repeats are ignored (`None`), so each physical press counts
/// once. Shift is accepted, because some keyboard layouts need it for `.` or
/// the digit row; any other key combined with Ctrl or Alt is ignored.
pub fn command_for(key: KeyEvent) -> Option<Command> {
    let code = match plain_press(key)? {
        Press::Quit => return Some(Command::Quit),
        Press::Code(code) => code,
    };
    match code {
        KeyCode::Char('q') => Some(Command::Quit),
        KeyCode::Char('t') => Some(Command::Trace),
        KeyCode::Char('i') => Some(Command::Inventory),
        KeyCode::Char('>' | DESCEND_ALTERNATIVE_KEY) => Some(Command::Act(Action::Descend)),
        KeyCode::Char('g' | ',') => Some(Command::Act(Action::PickUp)),
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

/// Returns the command bound to `key` while the inventory is open, or
/// `None` when the key does nothing there.
///
/// Bindings:
/// - choose an item: Up and Down (or `k` and `j`), which wrap around the
///   list; Home and End for the first and the last; a digit for the item
///   in its slot, `1` for the first and `0` for the tenth
///   (`item::INVENTORY_CAPACITY`), as the list numbers them;
/// - use the chosen item: Enter or `u`;
/// - drop it: `d`;
/// - close: Escape;
/// - quit: Ctrl+C only, so that `q` pressed by mistake loses nothing.
///
/// Releases and repeats are ignored, Shift is accepted (some layouts need
/// it for the digit row), and keys with Alt or Ctrl, Ctrl+C aside, do
/// nothing.
pub fn inventory_command_for(key: KeyEvent) -> Option<InventoryCommand> {
    let code = match plain_press(key)? {
        Press::Quit => return Some(InventoryCommand::Quit),
        Press::Code(code) => code,
    };
    let command = match code {
        KeyCode::Up | KeyCode::Char('k') => InventoryCommand::Previous,
        KeyCode::Down | KeyCode::Char('j') => InventoryCommand::Next,
        KeyCode::Home => InventoryCommand::First,
        KeyCode::End => InventoryCommand::Last,
        KeyCode::Char('0') => InventoryCommand::Slot(9),
        KeyCode::Char(digit @ '1'..='9') => InventoryCommand::Slot(usize::from(digit as u8 - b'1')),
        KeyCode::Enter | KeyCode::Char('u') => InventoryCommand::Use,
        KeyCode::Char('d') => InventoryCommand::Drop,
        KeyCode::Esc => InventoryCommand::Close,
        _ => return None,
    };
    Some(command)
}

/// Returns the command bound to `key` on the end screen, or `None` when
/// the key does nothing there.
///
/// Bindings: Enter goes back to the title screen; `q` quits
/// ([`EndCommand::Quit`]) and so does Ctrl+C ([`EndCommand::Interrupt`]),
/// told apart because only Ctrl+C works during the grace period of the end
/// screen (`app::END_SCREEN_GRACE`). Escape does nothing, like every other
/// key, so a press left over from the game never leaves the summary.
/// Releases, repeats and keys with Alt or Ctrl, Ctrl+C aside, give `None`.
pub fn end_command_for(key: KeyEvent) -> Option<EndCommand> {
    match plain_press(key)? {
        Press::Quit => Some(EndCommand::Interrupt),
        Press::Code(KeyCode::Char('q')) => Some(EndCommand::Quit),
        Press::Code(KeyCode::Enter) => Some(EndCommand::Title),
        Press::Code(_) => None,
    }
}

/// Returns the command bound to `key` on the title menu, or `None` when
/// the key does nothing there.
///
/// Bindings: Up and Down (or `k` and `j`), which wrap around the menu;
/// Home and End; Enter confirms the chosen entry; `q` and Ctrl+C quit.
/// Escape does nothing: the title is the first screen, there is nothing to
/// go back to. Releases, repeats and keys with Alt or Ctrl, Ctrl+C aside,
/// give `None`.
pub fn title_command_for(key: KeyEvent) -> Option<MenuCommand> {
    match plain_press(key)? {
        Press::Quit | Press::Code(KeyCode::Char('q')) => Some(MenuCommand::Quit),
        Press::Code(KeyCode::Esc) => None,
        Press::Code(code) => menu_move_for(code),
    }
}

/// Returns the command bound to `key` on the confirmation shown before a
/// saved game is abandoned, or `None` when the key does nothing there.
///
/// Bindings: the moves of [`title_command_for`] and Enter; Escape goes back
/// and keeps the saved game; Ctrl+C quits, but not `q`, which a player
/// could press meaning "no".
pub fn confirm_command_for(key: KeyEvent) -> Option<MenuCommand> {
    match plain_press(key)? {
        Press::Quit => Some(MenuCommand::Quit),
        Press::Code(KeyCode::Esc) => Some(MenuCommand::Back),
        Press::Code(code) => menu_move_for(code),
    }
}

/// Returns the command bound to `key` while a seed is typed, or `None`
/// when the key does nothing there.
///
/// Bindings: any character, Shift included, is typed (letters such as `q`
/// too, so nothing typed quits); Backspace erases the last one; Enter
/// starts the game; Escape goes back to the title menu; Ctrl+C quits.
/// Releases, repeats and keys with Alt or Ctrl, Ctrl+C aside, give `None`.
pub fn seed_command_for(key: KeyEvent) -> Option<SeedCommand> {
    let code = match plain_press(key)? {
        Press::Quit => return Some(SeedCommand::Quit),
        Press::Code(code) => code,
    };
    match code {
        KeyCode::Char(typed) => Some(SeedCommand::Type(typed)),
        KeyCode::Backspace => Some(SeedCommand::Erase),
        KeyCode::Enter => Some(SeedCommand::Submit),
        KeyCode::Esc => Some(SeedCommand::Back),
        _ => None,
    }
}

/// Returns the command bound to `key` on the options screen, or `None`
/// when the key does nothing there.
///
/// Bindings: Up and Down (or `k` and `j`), Home and End choose an entry;
/// Left and Right (or `h` and `l`) change the chosen option's value, as do
/// Enter and Space, which go back to the title on "Back"; Escape goes back
/// to the title; Ctrl+C quits, but not `q`. Releases, repeats and keys with
/// Alt or Ctrl, Ctrl+C aside, give `None`.
pub fn options_command_for(key: KeyEvent) -> Option<OptionsCommand> {
    let code = match plain_press(key)? {
        Press::Quit => return Some(OptionsCommand::Quit),
        Press::Code(code) => code,
    };
    let command = match code {
        KeyCode::Up | KeyCode::Char('k') => OptionsCommand::Previous,
        KeyCode::Down | KeyCode::Char('j') => OptionsCommand::Next,
        KeyCode::Home => OptionsCommand::First,
        KeyCode::End => OptionsCommand::Last,
        KeyCode::Left | KeyCode::Char('h') => OptionsCommand::PreviousValue,
        KeyCode::Right | KeyCode::Char('l') => OptionsCommand::NextValue,
        KeyCode::Enter | KeyCode::Char(' ') => OptionsCommand::Confirm,
        KeyCode::Esc => OptionsCommand::Back,
        _ => return None,
    };
    Some(command)
}

/// The moves and the confirmation shared by the menus: Up and Down (or `k`
/// and `j`), Home, End and Enter; `None` for any other key.
const fn menu_move_for(code: KeyCode) -> Option<MenuCommand> {
    let command = match code {
        KeyCode::Up | KeyCode::Char('k') => MenuCommand::Previous,
        KeyCode::Down | KeyCode::Char('j') => MenuCommand::Next,
        KeyCode::Home => MenuCommand::First,
        KeyCode::End => MenuCommand::Last,
        KeyCode::Enter => MenuCommand::Confirm,
        _ => return None,
    };
    Some(command)
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
    fn q_and_ctrl_c_quit_and_escape_does_nothing() {
        assert_eq!(command_for(press(KeyCode::Char('q'))), Some(Command::Quit));
        assert_eq!(command_for(press(KeyCode::Esc)), None);
        assert_eq!(
            command_for(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(Command::Quit)
        );
    }

    #[test]
    fn g_and_comma_pick_up_and_i_opens_the_inventory() {
        for code in [KeyCode::Char('g'), KeyCode::Char(',')] {
            assert_eq!(
                command_for(press(code)),
                Some(Command::Act(Action::PickUp)),
                "{code:?}"
            );
        }
        assert_eq!(
            command_for(press(KeyCode::Char('i'))),
            Some(Command::Inventory)
        );
        assert_eq!(
            command_for(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::ALT)),
            None
        );
    }

    #[test]
    fn greater_than_and_shift_d_descend_but_d_and_less_than_do_nothing() {
        let descend = Some(Command::Act(Action::Descend));
        // `>` arrives with Shift on most layouts, `D` always does.
        for key in [
            press(KeyCode::Char('>')),
            KeyEvent::new(KeyCode::Char('>'), KeyModifiers::SHIFT),
            KeyEvent::new(KeyCode::Char('D'), KeyModifiers::SHIFT),
        ] {
            assert_eq!(command_for(key), descend, "{key:?}");
        }
        for key in [
            press(KeyCode::Char('d')),
            press(KeyCode::Char('<')),
            KeyEvent::new(KeyCode::Char('>'), KeyModifiers::ALT),
            KeyEvent::new_with_kind(
                KeyCode::Char('>'),
                KeyModifiers::NONE,
                KeyEventKind::Release,
            ),
        ] {
            assert_eq!(command_for(key), None, "{key:?}");
        }
    }

    #[test]
    fn in_the_inventory_arrows_vi_keys_and_digits_choose_an_item() {
        for (code, expected) in [
            (KeyCode::Up, InventoryCommand::Previous),
            (KeyCode::Char('k'), InventoryCommand::Previous),
            (KeyCode::Down, InventoryCommand::Next),
            (KeyCode::Char('j'), InventoryCommand::Next),
            (KeyCode::Home, InventoryCommand::First),
            (KeyCode::End, InventoryCommand::Last),
            (KeyCode::Char('1'), InventoryCommand::Slot(0)),
            (KeyCode::Char('9'), InventoryCommand::Slot(8)),
            (KeyCode::Char('0'), InventoryCommand::Slot(9)),
        ] {
            assert_eq!(
                inventory_command_for(press(code)),
                Some(expected),
                "{code:?}"
            );
        }
        let shifted = KeyEvent::new(KeyCode::Char('3'), KeyModifiers::SHIFT);
        assert_eq!(
            inventory_command_for(shifted),
            Some(InventoryCommand::Slot(2))
        );
    }

    #[test]
    fn in_the_inventory_enter_and_u_use_d_drops_and_escape_closes() {
        for (code, expected) in [
            (KeyCode::Enter, InventoryCommand::Use),
            (KeyCode::Char('u'), InventoryCommand::Use),
            (KeyCode::Char('d'), InventoryCommand::Drop),
            (KeyCode::Esc, InventoryCommand::Close),
        ] {
            assert_eq!(
                inventory_command_for(press(code)),
                Some(expected),
                "{code:?}"
            );
        }
    }

    #[test]
    fn in_the_inventory_only_ctrl_c_quits_and_other_keys_do_nothing() {
        assert_eq!(
            inventory_command_for(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(InventoryCommand::Quit)
        );
        for key in [
            press(KeyCode::Char('q')),
            press(KeyCode::Char('i')),
            press(KeyCode::Char('g')),
            press(KeyCode::Left),
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::ALT),
            KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::NONE, KeyEventKind::Release),
            KeyEvent::new_with_kind(KeyCode::Down, KeyModifiers::NONE, KeyEventKind::Repeat),
        ] {
            assert_eq!(inventory_command_for(key), None, "{key:?}");
        }
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

    /// Ctrl+C, which quits from every screen.
    fn ctrl_c() -> KeyEvent {
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
    }

    #[test]
    fn the_end_screen_goes_back_to_the_title_on_enter_and_quits_on_q() {
        assert_eq!(
            end_command_for(press(KeyCode::Enter)),
            Some(EndCommand::Title)
        );
        assert_eq!(
            end_command_for(press(KeyCode::Char('q'))),
            Some(EndCommand::Quit)
        );
        assert_eq!(end_command_for(ctrl_c()), Some(EndCommand::Interrupt));
        for code in [KeyCode::Esc, KeyCode::Char('.'), KeyCode::Right] {
            assert_eq!(end_command_for(press(code)), None, "{code:?}");
        }
    }

    #[test]
    fn the_title_menu_moves_confirms_and_quits_on_q_but_not_on_escape() {
        for (code, command) in [
            (KeyCode::Up, MenuCommand::Previous),
            (KeyCode::Char('k'), MenuCommand::Previous),
            (KeyCode::Down, MenuCommand::Next),
            (KeyCode::Char('j'), MenuCommand::Next),
            (KeyCode::Home, MenuCommand::First),
            (KeyCode::End, MenuCommand::Last),
            (KeyCode::Enter, MenuCommand::Confirm),
            (KeyCode::Char('q'), MenuCommand::Quit),
        ] {
            assert_eq!(title_command_for(press(code)), Some(command), "{code:?}");
        }
        assert_eq!(title_command_for(ctrl_c()), Some(MenuCommand::Quit));
        assert_eq!(title_command_for(press(KeyCode::Esc)), None);
    }

    #[test]
    fn the_confirmation_goes_back_on_escape_and_ignores_q() {
        assert_eq!(
            confirm_command_for(press(KeyCode::Esc)),
            Some(MenuCommand::Back)
        );
        assert_eq!(
            confirm_command_for(press(KeyCode::Enter)),
            Some(MenuCommand::Confirm)
        );
        assert_eq!(confirm_command_for(press(KeyCode::Char('q'))), None);
        assert_eq!(confirm_command_for(ctrl_c()), Some(MenuCommand::Quit));
    }

    #[test]
    fn every_character_is_typed_into_the_seed_even_q() {
        for typed in ['7', 'F', 'a', '-', 'q', ' '] {
            assert_eq!(
                seed_command_for(press(KeyCode::Char(typed))),
                Some(SeedCommand::Type(typed))
            );
        }
        let shifted = KeyEvent::new(KeyCode::Char('F'), KeyModifiers::SHIFT);
        assert_eq!(seed_command_for(shifted), Some(SeedCommand::Type('F')));
        assert_eq!(
            seed_command_for(press(KeyCode::Backspace)),
            Some(SeedCommand::Erase)
        );
        assert_eq!(
            seed_command_for(press(KeyCode::Enter)),
            Some(SeedCommand::Submit)
        );
        assert_eq!(
            seed_command_for(press(KeyCode::Esc)),
            Some(SeedCommand::Back)
        );
        assert_eq!(seed_command_for(ctrl_c()), Some(SeedCommand::Quit));
        assert_eq!(seed_command_for(press(KeyCode::Left)), None);
    }

    #[test]
    fn the_options_screen_moves_changes_values_and_goes_back() {
        for (code, command) in [
            (KeyCode::Up, OptionsCommand::Previous),
            (KeyCode::Down, OptionsCommand::Next),
            (KeyCode::Home, OptionsCommand::First),
            (KeyCode::End, OptionsCommand::Last),
            (KeyCode::Left, OptionsCommand::PreviousValue),
            (KeyCode::Char('h'), OptionsCommand::PreviousValue),
            (KeyCode::Right, OptionsCommand::NextValue),
            (KeyCode::Char('l'), OptionsCommand::NextValue),
            (KeyCode::Enter, OptionsCommand::Confirm),
            (KeyCode::Char(' '), OptionsCommand::Confirm),
            (KeyCode::Esc, OptionsCommand::Back),
        ] {
            assert_eq!(options_command_for(press(code)), Some(command), "{code:?}");
        }
        assert_eq!(options_command_for(press(KeyCode::Char('q'))), None);
        assert_eq!(options_command_for(ctrl_c()), Some(OptionsCommand::Quit));
    }

    #[test]
    fn the_menu_keymaps_ignore_releases() {
        let release =
            KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::NONE, KeyEventKind::Release);

        assert_eq!(title_command_for(release), None);
        assert_eq!(confirm_command_for(release), None);
        assert_eq!(seed_command_for(release), None);
        assert_eq!(options_command_for(release), None);
        assert_eq!(end_command_for(release), None);
    }
}
