//! Keymap: turns key presses into commands for the application, for the
//! player's keyboard layout.
//!
//! Exposes one keymap per kind of screen, each a table of bindings for a
//! [`Keyboard`] and a function that looks a key up in it: [`command_for`]
//! on the game screen ([`Command`], table [`game_bindings`]),
//! [`trace_command_for`] while a sigil is traced ([`TraceCommand`],
//! [`trace_bindings`]), [`aim_command_for`] while its direction is asked
//! ([`AimCommand`], [`aim_bindings`]), [`inventory_command_for`] while the
//! inventory is open ([`InventoryCommand`], [`inventory_bindings`]),
//! [`end_command_for`] on the end screen ([`EndCommand`],
//! [`end_bindings`]), and for the screens before a game:
//! [`title_command_for`] on the title menu and [`confirm_command_for`] on
//! the confirmation before a saved game is abandoned ([`MenuCommand`],
//! [`title_bindings`], [`confirm_bindings`]) and [`options_command_for`]
//! on the options screen ([`OptionsCommand`], [`options_bindings`]). Three
//! screens take any key and have no table: [`verdict_command_for`] while
//! the verdict of a cast is shown ([`VerdictCommand`]),
//! [`exploring_command_for`] while the floor is explored automatically
//! ([`ExploringCommand`]) and [`seed_command_for`] while a seed is typed
//! ([`SeedCommand`]). [`key_names`] gives the keys the key help names that
//! differ between layouts.
//!
//! Every table holds each key once, so no key does two things on one
//! screen of one layout (a test goes through them all). The layouts share
//! the arrows, the keypad (with or without Num Lock), the digits typed with
//! or without Shift and the vi keys `h j k l y u b n`; QWERTY adds WASD,
//! and AZERTY adds ZQSD and the top row typed without Shift
//! (`& é " ' ( - è _ ç à`), which stands for the digits `1` to `0` wherever
//! a digit counts. `q` and `d` move on AZERTY, so quitting and the letter
//! that goes down the stairs take other keys there (see [`quit_key`] and
//! [`descend_keys`]).
//!
//! Only key presses count: crossterm also reports key releases on Windows,
//! and treating them as input would play every key twice. Ctrl+C quits from
//! every screen, since raw mode turns it into a plain key; the quit key
//! quits from the game, end and title screens only. Escape never quits: it
//! closes an overlay, a verdict or a sub-screen of the title, and does
//! nothing on the game, end and title screens, so a second press after
//! closing one loses nothing. This module decides nothing about the game
//! itself; the `app` module submits the resulting actions to
//! `sigilfall_core`.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use sigilfall_core::action::Action;
use sigilfall_core::geometry::Direction;
use sigilfall_core::sigil::Node;

use crate::keyboard::Keyboard;

/// Key that explores the floor automatically, step by step: `x`.
///
/// `x` reads as "explore" and is the key Brogue gives the same command. It
/// is neither a vi movement key nor bound anywhere else on the game screen,
/// and it sits at the same place on QWERTY, AZERTY and QWERTZ keyboards.
/// `o`, the key of Dungeon Crawl, was the other candidate, but several
/// roguelikes (NetHack, Angband) open doors with it, which this game may
/// want one day.
pub const EXPLORE_KEY: char = 'x';

/// The top row of an AZERTY keyboard typed without Shift, from the key of
/// `0` to the key of `9`: entry `n` stands for the digit `n`, which the same
/// key gives with Shift.
const AZERTY_DIGIT_ROW: [char; 10] = ['à', '&', 'é', '"', '\'', '(', '-', 'è', '_', 'ç'];

/// Mark between the first and the last key of a row in the key help, as in
/// "&…ç" for the AZERTY keys of the digits `1` to `9`. A dash would read as
/// one of those keys (`-` is the key of `6`).
const RANGE_MARK: char = '…';

/// What the player asked for with one key press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Perform a game action on the player's turn.
    Act(Action),
    /// Open the sigil overlay to trace a sigil.
    Trace,
    /// Open the inventory.
    Inventory,
    /// Explore the floor automatically, one `Action::Explore` after the
    /// other, until something worth a look happens.
    Explore,
    /// Leave the game.
    Quit,
}

/// What the player asked for with one key press while the floor is being
/// explored automatically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExploringCommand {
    /// Stop exploring; the key does nothing else.
    Stop,
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
    /// Leave the game (the quit key of the layout).
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

/// One binding of a keymap: a key, as crossterm reports it once pressed
/// alone or with Shift, and what it does on that screen.
pub type Binding<C> = (KeyCode, C);

/// The keys the key help names that differ between layouts, written as
/// the help shows them. The other keys it names (`.`, `t`, `g`, `i`, `>`,
/// `d`, Enter, Escape, Backspace) are the same on both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyNames {
    /// The four letters that move, north, west, south and east: `wasd` or
    /// `zqsd`.
    pub movement: String,
    /// The keys that link the nodes `1` to `9` of the grid: `1-9`, or on
    /// AZERTY the top row without Shift, `&…ç`.
    pub nodes: String,
    /// The keys that choose an item by its slot, the first to the tenth:
    /// `1-0`, or on AZERTY `&…à`.
    pub slots: String,
    /// The key that saves and quits ([`quit_key`]).
    pub quit: String,
}

/// Returns the keys the key help names on `keyboard` (see [`KeyNames`]),
/// taken from the same values as the tables, so the help never names a key
/// the layout does not bind.
pub fn key_names(keyboard: Keyboard) -> KeyNames {
    let range = |first: u8, last: u8| match keyboard {
        Keyboard::Qwerty => format!("{first}-{last}"),
        Keyboard::Azerty => {
            let row = AZERTY_DIGIT_ROW;
            format!(
                "{}{RANGE_MARK}{}",
                row[usize::from(first)],
                row[usize::from(last)]
            )
        }
    };
    KeyNames {
        movement: movement_letters(keyboard).iter().collect(),
        nodes: range(1, 9),
        slots: range(1, 0),
        quit: quit_key(keyboard).to_string(),
    }
}

/// Returns the key that saves the game and quits on `keyboard`, from the
/// game, end and title screens: `q` on QWERTY; `p` on AZERTY, where `q`
/// moves west.
///
/// `p` sits under the right hand, away from ZQSD, so a stray press while
/// moving does not end the session; Escape stays free to close overlays,
/// and Shift+q was set aside because Caps Lock would make every step west a
/// quit.
pub const fn quit_key(keyboard: Keyboard) -> char {
    match keyboard {
        Keyboard::Qwerty => 'q',
        Keyboard::Azerty => 'p',
    }
}

/// Returns the keys that go down the stairs on `keyboard`, the one the key
/// help names first.
///
/// `>` is the key of the genre and the one the help names on both layouts:
/// Shift and `.` on QWERTY, Shift and the `<` key on AZERTY. Some layouts
/// have no plain way to type it, such as AZERTY on a laptop without the `<`
/// key, so a capital letter does the same on both layouts: `E` ("escalier",
/// or "exit" the floor). `D` was the letter before WASD and ZQSD, but `d`
/// now moves east on both, and Caps Lock would have turned a step east into
/// a descent; `e` is bound to nothing on the game screen. AZERTY also takes
/// `<`, the same key as `>` without Shift, since nothing goes up.
pub const fn descend_keys(keyboard: Keyboard) -> &'static [char] {
    match keyboard {
        Keyboard::Qwerty => &['>', 'E'],
        Keyboard::Azerty => &['>', '<', 'E'],
    }
}

/// Returns the letters that move north, west, south and east on
/// `keyboard`, in that order: WASD on QWERTY, ZQSD on AZERTY, the same
/// four keys under the left hand.
pub const fn movement_letters(keyboard: Keyboard) -> [char; 4] {
    match keyboard {
        Keyboard::Qwerty => ['w', 'a', 's', 'd'],
        Keyboard::Azerty => ['z', 'q', 's', 'd'],
    }
}

/// Returns the bindings of the game screen on `keyboard`.
///
/// Bindings:
/// - moves: the keys of [`direction_bindings`] (arrows, keypad, digits
///   around 5, vi keys, WASD or ZQSD, and on AZERTY the top row);
/// - wait: `.`, `5` (also `(` on AZERTY, the same key without Shift) or
///   the keypad centre key without Num Lock;
/// - trace a sigil: `t`, shown in the key help (the digits are taken by
///   movement, and `t` is not a vi key);
/// - pick up the item underfoot: `g`, shown in the key help, or `,`, the
///   key of other roguelikes; neither is a vi key;
/// - open the inventory: `i`;
/// - go down the stairs: [`descend_keys`]; the game refuses it off the
///   stairs, at no cost;
/// - explore automatically: [`EXPLORE_KEY`];
/// - quit: [`quit_key`].
///
/// Escape is bound to nothing here: it closes the overlays, and a player
/// who presses it once too many after closing one must not lose the game.
pub fn game_bindings(keyboard: Keyboard) -> Vec<Binding<Command>> {
    let mut bindings = vec![
        (KeyCode::Char(quit_key(keyboard)), Command::Quit),
        (KeyCode::Char('t'), Command::Trace),
        (KeyCode::Char('i'), Command::Inventory),
        (KeyCode::Char(EXPLORE_KEY), Command::Explore),
        (KeyCode::Char('g'), Command::Act(Action::PickUp)),
        (KeyCode::Char(','), Command::Act(Action::PickUp)),
        (KeyCode::Char('.'), Command::Act(Action::Wait)),
        (KeyCode::KeypadBegin, Command::Act(Action::Wait)),
    ];
    bindings.extend(
        descend_keys(keyboard)
            .iter()
            .map(|&key| (KeyCode::Char(key), Command::Act(Action::Descend))),
    );
    bindings.extend(
        digit_keys(keyboard, 5)
            .into_iter()
            .map(|code| (code, Command::Act(Action::Wait))),
    );
    bindings.extend(
        direction_bindings(keyboard)
            .into_iter()
            .map(|(code, direction)| (code, Command::Act(Action::Move(direction)))),
    );
    bindings
}

/// Returns the command bound to `key` on the game screen of `keyboard`
/// (see [`game_bindings`]), or `None` when the key does nothing.
///
/// Ctrl+C quits, since raw mode turns it into a plain key. Releases and
/// repeats are ignored (`None`), so each physical press counts once. Shift
/// is accepted, because some keyboard layouts need it for `.` or the digit
/// row; any other key combined with Ctrl or Alt is ignored.
pub fn command_for(keyboard: Keyboard, key: KeyEvent) -> Option<Command> {
    match plain_press(key)? {
        Press::Quit => Some(Command::Quit),
        Press::Code(code) => bound(&game_bindings(keyboard), code),
    }
}

/// Returns the command bound to `key` while the floor is explored
/// automatically, or `None` when the key does not count.
///
/// Any key pressed alone or with Shift stops the walk and does nothing
/// else, so a key meant to stop it never moves the player or quits by
/// mistake (the quit key included); Ctrl+C quits. Releases and repeats are
/// ignored, so the release of the explore key itself, which Windows
/// reports just after the walk starts, never stops it; keys with Alt or
/// Ctrl, Ctrl+C aside, are ignored like everywhere else. The same on every
/// layout.
pub fn exploring_command_for(key: KeyEvent) -> Option<ExploringCommand> {
    match plain_press(key)? {
        Press::Quit => Some(ExploringCommand::Quit),
        Press::Code(_) => Some(ExploringCommand::Stop),
    }
}

/// Returns the bindings of the sigil overlay while a trace is drawn on
/// `keyboard` (decision D7):
/// - link a node: the keys of the digits `1` to `9` ([`digit_keys`]:
///   the keypad, the top row with or without Shift), numbered like the
///   keypad and the grid; with Num Lock off, the keys the keypad sends
///   instead (Home, Up, Page Up, Left, centre, Right, End, Down, Page Down)
///   link the node under them, so tracing on the keypad never depends on
///   Num Lock;
/// - undo the last node: Backspace;
/// - cast: Enter;
/// - cancel: Escape.
///
/// No key quits but Ctrl+C, so that a quit key pressed by mistake loses
/// nothing.
pub fn trace_bindings(keyboard: Keyboard) -> Vec<Binding<TraceCommand>> {
    let mut bindings = vec![
        (KeyCode::Enter, TraceCommand::Cast),
        (KeyCode::Backspace, TraceCommand::Undo),
        (KeyCode::Esc, TraceCommand::Cancel),
    ];
    for (digit, navigation) in (1..=9).zip(NODE_NAVIGATION_KEYS) {
        let Some(node) = Node::new(digit) else {
            continue;
        };
        bindings.push((navigation, TraceCommand::Link(node)));
        bindings.extend(
            digit_keys(keyboard, digit)
                .into_iter()
                .map(|code| (code, TraceCommand::Link(node))),
        );
    }
    bindings
}

/// Returns the command bound to `key` while a sigil is traced on
/// `keyboard` (see [`trace_bindings`]), or `None` when the key does nothing
/// there.
///
/// Ctrl+C quits. Releases and repeats are ignored, Shift is accepted (some
/// layouts need it for the digit row), and keys with Alt or Ctrl, Ctrl+C
/// aside, do nothing.
pub fn trace_command_for(keyboard: Keyboard, key: KeyEvent) -> Option<TraceCommand> {
    match plain_press(key)? {
        Press::Quit => Some(TraceCommand::Quit),
        Press::Code(code) => bound(&trace_bindings(keyboard), code),
    }
}

/// Returns the bindings of the direction prompt of a sigil on `keyboard`:
/// the movement keys of the game screen ([`direction_bindings`]) aim in
/// their direction, and Escape goes back to the trace. The wait keys do
/// nothing: a direction is needed.
pub fn aim_bindings(keyboard: Keyboard) -> Vec<Binding<AimCommand>> {
    let mut bindings = vec![(KeyCode::Esc, AimCommand::Back)];
    bindings.extend(
        direction_bindings(keyboard)
            .into_iter()
            .map(|(code, direction)| (code, AimCommand::Aim(direction))),
    );
    bindings
}

/// Returns the command bound to `key` while the direction of a sigil is
/// asked on `keyboard` (see [`aim_bindings`]), or `None` when the key does
/// nothing there; Ctrl+C quits.
pub fn aim_command_for(keyboard: Keyboard, key: KeyEvent) -> Option<AimCommand> {
    match plain_press(key)? {
        Press::Quit => Some(AimCommand::Quit),
        Press::Code(code) => bound(&aim_bindings(keyboard), code),
    }
}

/// Returns the command bound to `key` while the verdict of a cast is shown.
///
/// Any key press closes the verdict (Enter is the one the key help names),
/// without doing anything else, so a key pressed while reading is never
/// played in the game; Ctrl+C quits. Releases, repeats and keys with Alt
/// give `None`. The same on every layout.
pub fn verdict_command_for(key: KeyEvent) -> Option<VerdictCommand> {
    match plain_press(key)? {
        Press::Quit => Some(VerdictCommand::Quit),
        Press::Code(_) => Some(VerdictCommand::Continue),
    }
}

/// Returns the bindings of the inventory on `keyboard`:
/// - choose an item: Up and Down, `k` and `j`, or the north and south
///   letters of [`movement_letters`] (`w s` or `z s`), which wrap around
///   the list; Home and End for the first and the last; the key of a digit
///   ([`digit_keys`]) for the item in its slot, `1` for the first and `0`
///   for the tenth (`item::INVENTORY_CAPACITY`), as the list numbers them;
/// - use the chosen item: Enter or `u`;
/// - drop it: `d`;
/// - close: Escape.
///
/// No key quits but Ctrl+C, so that a quit key pressed by mistake loses
/// nothing.
pub fn inventory_bindings(keyboard: Keyboard) -> Vec<Binding<InventoryCommand>> {
    let [north, _, south, _] = movement_letters(keyboard);
    let mut bindings = vec![
        (KeyCode::Up, InventoryCommand::Previous),
        (KeyCode::Char('k'), InventoryCommand::Previous),
        (KeyCode::Char(north), InventoryCommand::Previous),
        (KeyCode::Down, InventoryCommand::Next),
        (KeyCode::Char('j'), InventoryCommand::Next),
        (KeyCode::Char(south), InventoryCommand::Next),
        (KeyCode::Home, InventoryCommand::First),
        (KeyCode::End, InventoryCommand::Last),
        (KeyCode::Enter, InventoryCommand::Use),
        (KeyCode::Char('u'), InventoryCommand::Use),
        (KeyCode::Char('d'), InventoryCommand::Drop),
        (KeyCode::Esc, InventoryCommand::Close),
    ];
    for digit in 0..=9 {
        // The digits number the slots from 1, and 0 is the tenth.
        let slot = (usize::from(digit) + 9) % 10;
        bindings.extend(
            digit_keys(keyboard, digit)
                .into_iter()
                .map(|code| (code, InventoryCommand::Slot(slot))),
        );
    }
    bindings
}

/// Returns the command bound to `key` while the inventory is open on
/// `keyboard` (see [`inventory_bindings`]), or `None` when the key does
/// nothing there.
///
/// Ctrl+C quits. Releases and repeats are ignored, Shift is accepted (some
/// layouts need it for the digit row), and keys with Alt or Ctrl, Ctrl+C
/// aside, do nothing.
pub fn inventory_command_for(keyboard: Keyboard, key: KeyEvent) -> Option<InventoryCommand> {
    match plain_press(key)? {
        Press::Quit => Some(InventoryCommand::Quit),
        Press::Code(code) => bound(&inventory_bindings(keyboard), code),
    }
}

/// Returns the bindings of the end screen on `keyboard`: Enter goes back
/// to the title screen; the [`quit_key`] quits. Escape does nothing, like
/// every other key, so a press left over from the game never leaves the
/// summary.
pub fn end_bindings(keyboard: Keyboard) -> Vec<Binding<EndCommand>> {
    vec![
        (KeyCode::Enter, EndCommand::Title),
        (KeyCode::Char(quit_key(keyboard)), EndCommand::Quit),
    ]
}

/// Returns the command bound to `key` on the end screen of `keyboard` (see
/// [`end_bindings`]), or `None` when the key does nothing there.
///
/// Ctrl+C gives [`EndCommand::Interrupt`], told apart from the quit key
/// because only Ctrl+C works during the grace period of the end screen
/// (`app::END_SCREEN_GRACE`). Releases, repeats and keys with Alt or Ctrl,
/// Ctrl+C aside, give `None`.
pub fn end_command_for(keyboard: Keyboard, key: KeyEvent) -> Option<EndCommand> {
    match plain_press(key)? {
        Press::Quit => Some(EndCommand::Interrupt),
        Press::Code(code) => bound(&end_bindings(keyboard), code),
    }
}

/// Returns the bindings of the title menu on `keyboard`: the moves of
/// [`menu_bindings`] and the [`quit_key`]. Escape does nothing: the title is
/// the first screen, there is nothing to go back to.
pub fn title_bindings(keyboard: Keyboard) -> Vec<Binding<MenuCommand>> {
    let mut bindings = menu_bindings(keyboard);
    bindings.push((KeyCode::Char(quit_key(keyboard)), MenuCommand::Quit));
    bindings
}

/// Returns the command bound to `key` on the title menu of `keyboard` (see
/// [`title_bindings`]), or `None` when the key does nothing there; Ctrl+C
/// quits. Releases, repeats and keys with Alt or Ctrl, Ctrl+C aside, give
/// `None`.
pub fn title_command_for(keyboard: Keyboard, key: KeyEvent) -> Option<MenuCommand> {
    match plain_press(key)? {
        Press::Quit => Some(MenuCommand::Quit),
        Press::Code(code) => bound(&title_bindings(keyboard), code),
    }
}

/// Returns the bindings of the confirmation shown before a saved game is
/// abandoned, on `keyboard`: the moves of [`menu_bindings`]; Escape goes
/// back and keeps the saved game. The quit key does nothing, since a player
/// could press `q` meaning "no".
pub fn confirm_bindings(keyboard: Keyboard) -> Vec<Binding<MenuCommand>> {
    let mut bindings = menu_bindings(keyboard);
    bindings.push((KeyCode::Esc, MenuCommand::Back));
    bindings
}

/// Returns the command bound to `key` on the confirmation of `keyboard`
/// (see [`confirm_bindings`]), or `None` when the key does nothing there;
/// Ctrl+C quits.
pub fn confirm_command_for(keyboard: Keyboard, key: KeyEvent) -> Option<MenuCommand> {
    match plain_press(key)? {
        Press::Quit => Some(MenuCommand::Quit),
        Press::Code(code) => bound(&confirm_bindings(keyboard), code),
    }
}

/// Returns the command bound to `key` while a seed is typed, or `None`
/// when the key does nothing there.
///
/// Bindings: any character, Shift included, is typed (letters such as `q`
/// too, so nothing typed quits); Backspace erases the last one; Enter
/// starts the game; Escape goes back to the title menu; Ctrl+C quits.
/// Releases, repeats and keys with Alt or Ctrl, Ctrl+C aside, give `None`.
/// The same on every layout.
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

/// Returns the bindings of the options screen on `keyboard`: Up and Down,
/// `k` and `j`, or the north and south letters of [`movement_letters`],
/// Home and End choose an entry; Left and Right, `h` and `l`, or the west
/// and east letters (`a d` or `q d`) change the chosen option's value, as
/// do Enter and Space, which go back to the title on "Back"; Escape goes
/// back to the title. The quit key does nothing.
pub fn options_bindings(keyboard: Keyboard) -> Vec<Binding<OptionsCommand>> {
    let [north, west, south, east] = movement_letters(keyboard);
    vec![
        (KeyCode::Up, OptionsCommand::Previous),
        (KeyCode::Char('k'), OptionsCommand::Previous),
        (KeyCode::Char(north), OptionsCommand::Previous),
        (KeyCode::Down, OptionsCommand::Next),
        (KeyCode::Char('j'), OptionsCommand::Next),
        (KeyCode::Char(south), OptionsCommand::Next),
        (KeyCode::Home, OptionsCommand::First),
        (KeyCode::End, OptionsCommand::Last),
        (KeyCode::Left, OptionsCommand::PreviousValue),
        (KeyCode::Char('h'), OptionsCommand::PreviousValue),
        (KeyCode::Char(west), OptionsCommand::PreviousValue),
        (KeyCode::Right, OptionsCommand::NextValue),
        (KeyCode::Char('l'), OptionsCommand::NextValue),
        (KeyCode::Char(east), OptionsCommand::NextValue),
        (KeyCode::Enter, OptionsCommand::Confirm),
        (KeyCode::Char(' '), OptionsCommand::Confirm),
        (KeyCode::Esc, OptionsCommand::Back),
    ]
}

/// Returns the command bound to `key` on the options screen of `keyboard`
/// (see [`options_bindings`]), or `None` when the key does nothing there;
/// Ctrl+C quits. Releases, repeats and keys with Alt or Ctrl, Ctrl+C aside,
/// give `None`.
pub fn options_command_for(keyboard: Keyboard, key: KeyEvent) -> Option<OptionsCommand> {
    match plain_press(key)? {
        Press::Quit => Some(OptionsCommand::Quit),
        Press::Code(code) => bound(&options_bindings(keyboard), code),
    }
}

/// The moves and the confirmation shared by the menus on `keyboard`: Up and
/// Down, `k` and `j`, or the north and south letters of
/// [`movement_letters`]; Home, End and Enter.
fn menu_bindings(keyboard: Keyboard) -> Vec<Binding<MenuCommand>> {
    let [north, _, south, _] = movement_letters(keyboard);
    vec![
        (KeyCode::Up, MenuCommand::Previous),
        (KeyCode::Char('k'), MenuCommand::Previous),
        (KeyCode::Char(north), MenuCommand::Previous),
        (KeyCode::Down, MenuCommand::Next),
        (KeyCode::Char('j'), MenuCommand::Next),
        (KeyCode::Char(south), MenuCommand::Next),
        (KeyCode::Home, MenuCommand::First),
        (KeyCode::End, MenuCommand::Last),
        (KeyCode::Enter, MenuCommand::Confirm),
    ]
}

/// Returns the keys that point to a direction on `keyboard`, with that
/// direction:
/// - the arrows, and Home, Page Up, End and Page Down for the diagonals,
///   which is what the keypad sends with Num Lock off;
/// - the keys of the digits around 5 ([`digit_keys`]), laid out like the
///   keypad, so `8` is up and `3` is down-right;
/// - the vi keys `h j k l y u b n`;
/// - the four [`movement_letters`] of the layout, WASD or ZQSD.
pub fn direction_bindings(keyboard: Keyboard) -> Vec<Binding<Direction>> {
    let [north, west, south, east] = movement_letters(keyboard);
    let mut bindings = vec![
        (KeyCode::Up, Direction::North),
        (KeyCode::PageUp, Direction::NorthEast),
        (KeyCode::Right, Direction::East),
        (KeyCode::PageDown, Direction::SouthEast),
        (KeyCode::Down, Direction::South),
        (KeyCode::End, Direction::SouthWest),
        (KeyCode::Left, Direction::West),
        (KeyCode::Home, Direction::NorthWest),
        (KeyCode::Char('k'), Direction::North),
        (KeyCode::Char('u'), Direction::NorthEast),
        (KeyCode::Char('l'), Direction::East),
        (KeyCode::Char('n'), Direction::SouthEast),
        (KeyCode::Char('j'), Direction::South),
        (KeyCode::Char('b'), Direction::SouthWest),
        (KeyCode::Char('h'), Direction::West),
        (KeyCode::Char('y'), Direction::NorthWest),
        (KeyCode::Char(north), Direction::North),
        (KeyCode::Char(west), Direction::West),
        (KeyCode::Char(south), Direction::South),
        (KeyCode::Char(east), Direction::East),
    ];
    for (digit, direction) in KEYPAD_DIRECTIONS {
        bindings.extend(
            digit_keys(keyboard, digit)
                .into_iter()
                .map(|code| (code, direction)),
        );
    }
    bindings
}

/// The digits of the keypad around 5 and the direction each points to.
const KEYPAD_DIRECTIONS: [(u8, Direction); 8] = [
    (1, Direction::SouthWest),
    (2, Direction::South),
    (3, Direction::SouthEast),
    (4, Direction::West),
    (6, Direction::East),
    (7, Direction::NorthWest),
    (8, Direction::North),
    (9, Direction::NorthEast),
];

/// The keys the keypad sends with Num Lock off in place of the digits `1`
/// to `9`, in that order.
const NODE_NAVIGATION_KEYS: [KeyCode; 9] = [
    KeyCode::End,
    KeyCode::Down,
    KeyCode::PageDown,
    KeyCode::Left,
    KeyCode::KeypadBegin,
    KeyCode::Right,
    KeyCode::Home,
    KeyCode::Up,
    KeyCode::PageUp,
];

/// Returns the keys that type the digit `digit` (0 to 9) on `keyboard`:
/// the digit itself, from the keypad or from the top row (with Shift on
/// AZERTY, which crossterm reports as the digit), and on AZERTY the same
/// top-row key without Shift ([`AZERTY_DIGIT_ROW`]).
fn digit_keys(keyboard: Keyboard, digit: u8) -> Vec<KeyCode> {
    let mut keys = vec![KeyCode::Char(char::from(b'0' + digit))];
    if keyboard == Keyboard::Azerty {
        keys.push(KeyCode::Char(AZERTY_DIGIT_ROW[usize::from(digit)]));
    }
    keys
}

/// Returns what `code` does in `bindings`, or `None` when it is not bound.
fn bound<C: Copy>(bindings: &[Binding<C>], code: KeyCode) -> Option<C> {
    bindings
        .iter()
        .find(|(key, _)| *key == code)
        .map(|&(_, command)| command)
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

#[cfg(test)]
mod tests;
