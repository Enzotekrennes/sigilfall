//! Tests of the keymaps: what each key does on each screen of each layout,
//! that no key does two things on one screen of one layout, and that the
//! keys the key help names are the ones bound.

use std::collections::HashMap;
use std::fmt::Debug;

use ratatui::crossterm::event::KeyEventState;

use super::*;

const QWERTY: Keyboard = Keyboard::Qwerty;
const AZERTY: Keyboard = Keyboard::Azerty;

/// A key press of `code` without modifiers.
fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// A key press of the character `typed` without modifiers.
fn char_press(typed: char) -> KeyEvent {
    press(KeyCode::Char(typed))
}

/// A key press of `code` with Shift, as layouts that need it for the digit
/// row or `.` report it.
fn shifted(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::SHIFT)
}

/// Ctrl+C, which quits from every screen.
fn ctrl_c() -> KeyEvent {
    KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
}

/// The command expected for a move in `direction`.
const fn step(direction: Direction) -> Option<Command> {
    Some(Command::Act(Action::Move(direction)))
}

/// The node numbered `digit`.
fn node(digit: u8) -> Node {
    Node::new(digit).expect("a digit from 1 to 9")
}

/// The keys of `bindings` bound more than once, with what each does.
fn repeated<C: Debug>(bindings: &[Binding<C>]) -> Vec<String> {
    let mut seen: HashMap<KeyCode, &C> = HashMap::new();
    let mut repeated = Vec::new();
    for (code, command) in bindings {
        if let Some(first) = seen.insert(*code, command) {
            repeated.push(format!("{code:?}: {first:?} and {command:?}"));
        }
    }
    repeated
}

// --- No key does two things -----------------------------------------------

#[test]
fn no_key_is_bound_twice_on_any_screen_of_any_layout() {
    for keyboard in Keyboard::ALL {
        let screens = [
            ("game", repeated(&game_bindings(keyboard))),
            ("trace", repeated(&trace_bindings(keyboard))),
            ("aim", repeated(&aim_bindings(keyboard))),
            ("inventory", repeated(&inventory_bindings(keyboard))),
            ("end", repeated(&end_bindings(keyboard))),
            ("title", repeated(&title_bindings(keyboard))),
            ("confirmation", repeated(&confirm_bindings(keyboard))),
            ("options", repeated(&options_bindings(keyboard))),
        ];
        for (screen, repeated) in screens {
            assert!(repeated.is_empty(), "{keyboard:?}, {screen}: {repeated:#?}");
        }
    }
}

#[test]
fn the_lookup_gives_what_the_table_says_for_every_key_of_every_table() {
    // The functions the application calls read the tables the test above
    // checks: every bound key gives its binding, alone or with Shift.
    for keyboard in Keyboard::ALL {
        for (code, command) in game_bindings(keyboard) {
            assert_eq!(command_for(keyboard, press(code)), Some(command));
            assert_eq!(command_for(keyboard, shifted(code)), Some(command));
        }
        for (code, command) in trace_bindings(keyboard) {
            assert_eq!(trace_command_for(keyboard, press(code)), Some(command));
        }
        for (code, command) in aim_bindings(keyboard) {
            assert_eq!(aim_command_for(keyboard, press(code)), Some(command));
        }
        for (code, command) in inventory_bindings(keyboard) {
            assert_eq!(inventory_command_for(keyboard, press(code)), Some(command));
        }
        for (code, command) in end_bindings(keyboard) {
            assert_eq!(end_command_for(keyboard, press(code)), Some(command));
        }
        for (code, command) in title_bindings(keyboard) {
            assert_eq!(title_command_for(keyboard, press(code)), Some(command));
        }
        for (code, command) in confirm_bindings(keyboard) {
            assert_eq!(confirm_command_for(keyboard, press(code)), Some(command));
        }
        for (code, command) in options_bindings(keyboard) {
            assert_eq!(options_command_for(keyboard, press(code)), Some(command));
        }
    }
}

// --- The game screen ------------------------------------------------------

#[test]
fn on_both_layouts_every_direction_has_an_arrow_or_navigation_key_a_digit_and_a_vi_key() {
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
    for keyboard in Keyboard::ALL {
        for (direction, navigation, digit, vi) in bindings {
            for key in [
                press(navigation),
                char_press(digit),
                shifted(KeyCode::Char(digit)),
                char_press(vi),
            ] {
                assert_eq!(
                    command_for(keyboard, key),
                    step(direction),
                    "{keyboard:?} {key:?}"
                );
            }
        }
    }
}

#[test]
fn wasd_moves_on_qwerty_and_zqsd_on_azerty() {
    for (keyboard, letters) in [(QWERTY, "wasd"), (AZERTY, "zqsd")] {
        let directions = [
            Direction::North,
            Direction::West,
            Direction::South,
            Direction::East,
        ];
        for (letter, direction) in letters.chars().zip(directions) {
            assert_eq!(
                command_for(keyboard, char_press(letter)),
                step(direction),
                "{keyboard:?} {letter}"
            );
        }
    }
    // Each layout keeps the other's letters free.
    assert_eq!(command_for(QWERTY, char_press('z')), None);
    assert_eq!(command_for(AZERTY, char_press('w')), None);
    assert_eq!(command_for(AZERTY, char_press('a')), None);
}

#[test]
fn on_azerty_the_top_row_without_shift_moves_and_waits_like_the_digits() {
    for (typed, expected) in [
        ('&', step(Direction::SouthWest)),
        ('é', step(Direction::South)),
        ('"', step(Direction::SouthEast)),
        ('\'', step(Direction::West)),
        ('(', Some(Command::Act(Action::Wait))),
        ('-', step(Direction::East)),
        ('è', step(Direction::NorthWest)),
        ('_', step(Direction::North)),
        ('ç', step(Direction::NorthEast)),
    ] {
        assert_eq!(command_for(AZERTY, char_press(typed)), expected, "{typed}");
        assert_eq!(command_for(QWERTY, char_press(typed)), None, "{typed}");
    }
    assert_eq!(command_for(AZERTY, char_press('à')), None);
}

#[test]
fn keypad_digits_move_like_the_digit_row_on_both_layouts() {
    let keypad_eight = KeyEvent::new_with_kind_and_state(
        KeyCode::Char('8'),
        KeyModifiers::NONE,
        KeyEventKind::Press,
        KeyEventState::KEYPAD,
    );
    for keyboard in Keyboard::ALL {
        assert_eq!(command_for(keyboard, keypad_eight), step(Direction::North));
    }
}

#[test]
fn dot_five_and_keypad_centre_wait_on_both_layouts() {
    for keyboard in Keyboard::ALL {
        for code in [KeyCode::Char('.'), KeyCode::Char('5'), KeyCode::KeypadBegin] {
            assert_eq!(
                command_for(keyboard, press(code)),
                Some(Command::Act(Action::Wait)),
                "{keyboard:?} {code:?}"
            );
        }
        assert_eq!(
            command_for(keyboard, shifted(KeyCode::Char('.'))),
            Some(Command::Act(Action::Wait))
        );
    }
}

#[test]
fn q_quits_on_qwerty_and_p_on_azerty_where_q_moves_west() {
    assert_eq!(command_for(QWERTY, char_press('q')), Some(Command::Quit));
    assert_eq!(command_for(QWERTY, char_press('p')), None);
    assert_eq!(command_for(AZERTY, char_press('p')), Some(Command::Quit));
    assert_eq!(command_for(AZERTY, char_press('q')), step(Direction::West));
    for keyboard in Keyboard::ALL {
        assert_eq!(command_for(keyboard, ctrl_c()), Some(Command::Quit));
        assert_eq!(command_for(keyboard, press(KeyCode::Esc)), None);
    }
}

#[test]
fn greater_than_and_capital_e_descend_on_both_layouts_and_less_than_on_azerty() {
    let descend = Some(Command::Act(Action::Descend));
    for keyboard in Keyboard::ALL {
        // `>` arrives with Shift on most layouts; `E` with Shift or Caps Lock.
        for key in [
            char_press('>'),
            shifted(KeyCode::Char('>')),
            shifted(KeyCode::Char('E')),
            char_press('E'),
        ] {
            assert_eq!(command_for(keyboard, key), descend, "{keyboard:?} {key:?}");
        }
    }
    assert_eq!(command_for(AZERTY, char_press('<')), descend);
    for (keyboard, key) in [
        (QWERTY, char_press('<')),
        (QWERTY, char_press('e')),
        (AZERTY, char_press('e')),
        (QWERTY, KeyEvent::new(KeyCode::Char('>'), KeyModifiers::ALT)),
        (
            AZERTY,
            KeyEvent::new_with_kind(
                KeyCode::Char('>'),
                KeyModifiers::NONE,
                KeyEventKind::Release,
            ),
        ),
    ] {
        assert_eq!(command_for(keyboard, key), None, "{keyboard:?} {key:?}");
    }
    // `d` moves east on both layouts, and its capital, as Shift or Caps Lock
    // types it, does nothing rather than go down the stairs.
    for keyboard in Keyboard::ALL {
        assert_eq!(
            command_for(keyboard, char_press('d')),
            step(Direction::East)
        );
        for key in [char_press('D'), shifted(KeyCode::Char('D'))] {
            assert_eq!(command_for(keyboard, key), None, "{keyboard:?} {key:?}");
        }
    }
}

#[test]
fn g_and_comma_pick_up_i_opens_the_inventory_t_traces_and_x_explores() {
    for keyboard in Keyboard::ALL {
        for typed in ['g', ','] {
            assert_eq!(
                command_for(keyboard, char_press(typed)),
                Some(Command::Act(Action::PickUp))
            );
        }
        assert_eq!(
            command_for(keyboard, char_press('i')),
            Some(Command::Inventory)
        );
        assert_eq!(command_for(keyboard, char_press('t')), Some(Command::Trace));
        assert_eq!(
            command_for(keyboard, char_press(EXPLORE_KEY)),
            Some(Command::Explore)
        );
        assert_eq!(command_for(keyboard, char_press('X')), None);
        for code in [KeyCode::Char('g'), KeyCode::Char('t')] {
            let alt = KeyEvent::new(code, KeyModifiers::ALT);
            assert_eq!(command_for(keyboard, alt), None);
        }
    }
}

#[test]
fn releases_repeats_and_keys_with_ctrl_or_alt_do_nothing() {
    for keyboard in Keyboard::ALL {
        for kind in [KeyEventKind::Release, KeyEventKind::Repeat] {
            for code in [KeyCode::Right, KeyCode::Char('.'), KeyCode::Char('q')] {
                let key = KeyEvent::new_with_kind(code, KeyModifiers::NONE, kind);
                assert_eq!(command_for(keyboard, key), None, "{kind:?} {code:?}");
            }
        }
        for key in [
            KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL),
            KeyEvent::new(KeyCode::Left, KeyModifiers::ALT),
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::ALT),
            char_press('K'),
            press(KeyCode::Enter),
            press(KeyCode::F(1)),
        ] {
            assert_eq!(command_for(keyboard, key), None, "{keyboard:?} {key:?}");
        }
    }
}

// --- Automatic exploration and the verdict --------------------------------

#[test]
fn while_exploring_any_press_stops_and_only_ctrl_c_quits() {
    for code in [
        KeyCode::Char('q'),
        KeyCode::Char('p'),
        KeyCode::Char(EXPLORE_KEY),
        KeyCode::Right,
        KeyCode::Char('.'),
        KeyCode::Esc,
        KeyCode::Enter,
    ] {
        assert_eq!(
            exploring_command_for(press(code)),
            Some(ExploringCommand::Stop),
            "{code:?}"
        );
    }
    assert_eq!(
        exploring_command_for(shifted(KeyCode::Char('A'))),
        Some(ExploringCommand::Stop)
    );
    assert_eq!(
        exploring_command_for(ctrl_c()),
        Some(ExploringCommand::Quit)
    );
}

#[test]
fn while_exploring_releases_repeats_and_alt_keys_do_not_stop() {
    for kind in [KeyEventKind::Release, KeyEventKind::Repeat] {
        let key = KeyEvent::new_with_kind(KeyCode::Char(EXPLORE_KEY), KeyModifiers::NONE, kind);
        assert_eq!(exploring_command_for(key), None, "{kind:?}");
    }
    let alt = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT);
    assert_eq!(exploring_command_for(alt), None);
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
    assert_eq!(verdict_command_for(ctrl_c()), Some(VerdictCommand::Quit));
    let release =
        KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::NONE, KeyEventKind::Release);
    assert_eq!(verdict_command_for(release), None);
}

// --- The sigil overlay ----------------------------------------------------

#[test]
fn while_tracing_digits_and_the_keypad_without_num_lock_link_their_node_on_both_layouts() {
    for keyboard in Keyboard::ALL {
        for (digit, navigation) in (1..=9).zip(NODE_NAVIGATION_KEYS) {
            let expected = Some(TraceCommand::Link(node(digit)));
            let digit_key = KeyCode::Char(char::from(b'0' + digit));
            for key in [press(digit_key), shifted(digit_key), press(navigation)] {
                assert_eq!(
                    trace_command_for(keyboard, key),
                    expected,
                    "{keyboard:?} {key:?}"
                );
            }
        }
        assert_eq!(trace_command_for(keyboard, char_press('0')), None);
    }
}

#[test]
fn on_azerty_the_top_row_without_shift_links_the_nodes_one_to_nine() {
    for (digit, typed) in (1..=9).zip("&é\"'(-è_ç".chars()) {
        assert_eq!(
            trace_command_for(AZERTY, char_press(typed)),
            Some(TraceCommand::Link(node(digit))),
            "{typed}"
        );
        assert_eq!(
            trace_command_for(QWERTY, char_press(typed)),
            None,
            "{typed}"
        );
    }
    assert_eq!(trace_command_for(AZERTY, char_press('à')), None);
}

#[test]
fn while_tracing_enter_casts_backspace_undoes_escape_cancels_and_only_ctrl_c_quits() {
    for keyboard in Keyboard::ALL {
        for (code, expected) in [
            (KeyCode::Enter, TraceCommand::Cast),
            (KeyCode::Backspace, TraceCommand::Undo),
            (KeyCode::Esc, TraceCommand::Cancel),
        ] {
            assert_eq!(trace_command_for(keyboard, press(code)), Some(expected));
        }
        assert_eq!(
            trace_command_for(keyboard, ctrl_c()),
            Some(TraceCommand::Quit)
        );
        for key in [
            char_press(quit_key(keyboard)),
            char_press('t'),
            char_press('.'),
            KeyEvent::new(KeyCode::Char('5'), KeyModifiers::ALT),
            KeyEvent::new_with_kind(
                KeyCode::Char('5'),
                KeyModifiers::NONE,
                KeyEventKind::Release,
            ),
            KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::NONE, KeyEventKind::Repeat),
        ] {
            assert_eq!(
                trace_command_for(keyboard, key),
                None,
                "{keyboard:?} {key:?}"
            );
        }
    }
}

#[test]
fn while_aiming_movement_keys_aim_and_escape_goes_back() {
    for (keyboard, code, direction) in [
        (QWERTY, KeyCode::Right, Direction::East),
        (QWERTY, KeyCode::Char('7'), Direction::NorthWest),
        (QWERTY, KeyCode::Char('j'), Direction::South),
        (QWERTY, KeyCode::Char('w'), Direction::North),
        (AZERTY, KeyCode::PageDown, Direction::SouthEast),
        (AZERTY, KeyCode::Char('q'), Direction::West),
        (AZERTY, KeyCode::Char('è'), Direction::NorthWest),
    ] {
        assert_eq!(
            aim_command_for(keyboard, press(code)),
            Some(AimCommand::Aim(direction)),
            "{keyboard:?} {code:?}"
        );
    }
    for keyboard in Keyboard::ALL {
        assert_eq!(
            aim_command_for(keyboard, press(KeyCode::Esc)),
            Some(AimCommand::Back)
        );
        for code in [KeyCode::Char('5'), KeyCode::Char('.'), KeyCode::Enter] {
            assert_eq!(aim_command_for(keyboard, press(code)), None, "{code:?}");
        }
    }
    assert_eq!(aim_command_for(AZERTY, char_press('(')), None);
    assert_eq!(aim_command_for(QWERTY, char_press('q')), None);
}

// --- The inventory --------------------------------------------------------

#[test]
fn in_the_inventory_arrows_vi_keys_layout_letters_and_digits_choose_an_item() {
    for keyboard in Keyboard::ALL {
        let [north, _, south, _] = movement_letters(keyboard);
        for (code, expected) in [
            (KeyCode::Up, InventoryCommand::Previous),
            (KeyCode::Char('k'), InventoryCommand::Previous),
            (KeyCode::Char(north), InventoryCommand::Previous),
            (KeyCode::Down, InventoryCommand::Next),
            (KeyCode::Char('j'), InventoryCommand::Next),
            (KeyCode::Char(south), InventoryCommand::Next),
            (KeyCode::Home, InventoryCommand::First),
            (KeyCode::End, InventoryCommand::Last),
            (KeyCode::Char('1'), InventoryCommand::Slot(0)),
            (KeyCode::Char('9'), InventoryCommand::Slot(8)),
            (KeyCode::Char('0'), InventoryCommand::Slot(9)),
        ] {
            assert_eq!(
                inventory_command_for(keyboard, press(code)),
                Some(expected),
                "{keyboard:?} {code:?}"
            );
        }
        assert_eq!(
            inventory_command_for(keyboard, shifted(KeyCode::Char('3'))),
            Some(InventoryCommand::Slot(2))
        );
    }
    for (slot, typed) in "&é\"'(-è_çà".chars().enumerate() {
        assert_eq!(
            inventory_command_for(AZERTY, char_press(typed)),
            Some(InventoryCommand::Slot(slot)),
            "{typed}"
        );
    }
}

#[test]
fn in_the_inventory_enter_and_u_use_d_drops_escape_closes_and_only_ctrl_c_quits() {
    for keyboard in Keyboard::ALL {
        for (code, expected) in [
            (KeyCode::Enter, InventoryCommand::Use),
            (KeyCode::Char('u'), InventoryCommand::Use),
            (KeyCode::Char('d'), InventoryCommand::Drop),
            (KeyCode::Esc, InventoryCommand::Close),
        ] {
            assert_eq!(
                inventory_command_for(keyboard, press(code)),
                Some(expected),
                "{code:?}"
            );
        }
        assert_eq!(
            inventory_command_for(keyboard, ctrl_c()),
            Some(InventoryCommand::Quit)
        );
        for key in [
            char_press(quit_key(keyboard)),
            char_press('i'),
            char_press('g'),
            press(KeyCode::Left),
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::ALT),
            KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::NONE, KeyEventKind::Release),
            KeyEvent::new_with_kind(KeyCode::Down, KeyModifiers::NONE, KeyEventKind::Repeat),
        ] {
            assert_eq!(
                inventory_command_for(keyboard, key),
                None,
                "{keyboard:?} {key:?}"
            );
        }
    }
}

// --- The end, title, confirmation, seed and options screens -----------------

#[test]
fn the_end_screen_goes_back_to_the_title_on_enter_and_quits_on_the_quit_key() {
    for keyboard in Keyboard::ALL {
        assert_eq!(
            end_command_for(keyboard, press(KeyCode::Enter)),
            Some(EndCommand::Title)
        );
        assert_eq!(
            end_command_for(keyboard, char_press(quit_key(keyboard))),
            Some(EndCommand::Quit)
        );
        assert_eq!(
            end_command_for(keyboard, ctrl_c()),
            Some(EndCommand::Interrupt)
        );
        for code in [KeyCode::Esc, KeyCode::Char('.'), KeyCode::Right] {
            assert_eq!(end_command_for(keyboard, press(code)), None, "{code:?}");
        }
    }
    assert_eq!(end_command_for(AZERTY, char_press('q')), None);
}

#[test]
fn the_title_menu_moves_confirms_and_quits_on_the_quit_key_but_not_on_escape() {
    for keyboard in Keyboard::ALL {
        let [north, _, south, _] = movement_letters(keyboard);
        for (code, command) in [
            (KeyCode::Up, MenuCommand::Previous),
            (KeyCode::Char('k'), MenuCommand::Previous),
            (KeyCode::Char(north), MenuCommand::Previous),
            (KeyCode::Down, MenuCommand::Next),
            (KeyCode::Char('j'), MenuCommand::Next),
            (KeyCode::Char(south), MenuCommand::Next),
            (KeyCode::Home, MenuCommand::First),
            (KeyCode::End, MenuCommand::Last),
            (KeyCode::Enter, MenuCommand::Confirm),
            (KeyCode::Char(quit_key(keyboard)), MenuCommand::Quit),
        ] {
            assert_eq!(
                title_command_for(keyboard, press(code)),
                Some(command),
                "{keyboard:?} {code:?}"
            );
        }
        assert_eq!(
            title_command_for(keyboard, ctrl_c()),
            Some(MenuCommand::Quit)
        );
        assert_eq!(title_command_for(keyboard, press(KeyCode::Esc)), None);
    }
    assert_eq!(title_command_for(AZERTY, char_press('q')), None);
}

#[test]
fn the_confirmation_goes_back_on_escape_and_ignores_the_quit_key() {
    for keyboard in Keyboard::ALL {
        assert_eq!(
            confirm_command_for(keyboard, press(KeyCode::Esc)),
            Some(MenuCommand::Back)
        );
        assert_eq!(
            confirm_command_for(keyboard, press(KeyCode::Enter)),
            Some(MenuCommand::Confirm)
        );
        assert_eq!(
            confirm_command_for(keyboard, char_press(quit_key(keyboard))),
            None
        );
        assert_eq!(
            confirm_command_for(keyboard, char_press('q')),
            None,
            "q may mean no"
        );
        assert_eq!(
            confirm_command_for(keyboard, ctrl_c()),
            Some(MenuCommand::Quit)
        );
    }
}

#[test]
fn every_character_is_typed_into_the_seed_even_q() {
    for typed in ['7', 'F', 'a', '-', 'q', ' ', 'é'] {
        assert_eq!(
            seed_command_for(char_press(typed)),
            Some(SeedCommand::Type(typed))
        );
    }
    assert_eq!(
        seed_command_for(shifted(KeyCode::Char('F'))),
        Some(SeedCommand::Type('F'))
    );
    for (code, command) in [
        (KeyCode::Backspace, SeedCommand::Erase),
        (KeyCode::Enter, SeedCommand::Submit),
        (KeyCode::Esc, SeedCommand::Back),
    ] {
        assert_eq!(seed_command_for(press(code)), Some(command));
    }
    assert_eq!(seed_command_for(ctrl_c()), Some(SeedCommand::Quit));
    assert_eq!(seed_command_for(press(KeyCode::Left)), None);
}

#[test]
fn the_options_screen_moves_changes_values_and_goes_back() {
    for keyboard in Keyboard::ALL {
        let [north, west, south, east] = movement_letters(keyboard);
        for (code, command) in [
            (KeyCode::Up, OptionsCommand::Previous),
            (KeyCode::Char(north), OptionsCommand::Previous),
            (KeyCode::Down, OptionsCommand::Next),
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
        ] {
            assert_eq!(
                options_command_for(keyboard, press(code)),
                Some(command),
                "{keyboard:?} {code:?}"
            );
        }
        assert_eq!(
            options_command_for(keyboard, char_press(quit_key(keyboard))),
            None
        );
        assert_eq!(
            options_command_for(keyboard, ctrl_c()),
            Some(OptionsCommand::Quit)
        );
    }
    assert_eq!(options_command_for(QWERTY, char_press('q')), None);
}

#[test]
fn the_menu_keymaps_ignore_releases() {
    let release =
        KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::NONE, KeyEventKind::Release);
    for keyboard in Keyboard::ALL {
        assert_eq!(title_command_for(keyboard, release), None);
        assert_eq!(confirm_command_for(keyboard, release), None);
        assert_eq!(options_command_for(keyboard, release), None);
        assert_eq!(end_command_for(keyboard, release), None);
    }
    assert_eq!(seed_command_for(release), None);
}

// --- The keys the key help names --------------------------------------------

#[test]
fn the_key_names_of_each_layout_are_the_keys_it_binds() {
    let qwerty = key_names(QWERTY);
    assert_eq!(
        (
            qwerty.movement.as_str(),
            qwerty.nodes.as_str(),
            qwerty.slots.as_str(),
            qwerty.quit.as_str()
        ),
        ("wasd", "1-9", "1-0", "q")
    );
    let azerty = key_names(AZERTY);
    assert_eq!(
        (
            azerty.movement.as_str(),
            azerty.nodes.as_str(),
            azerty.slots.as_str(),
            azerty.quit.as_str()
        ),
        ("zqsd", "&…ç", "&…à", "p")
    );

    for keyboard in Keyboard::ALL {
        let names = key_names(keyboard);
        for letter in names.movement.chars() {
            assert!(
                matches!(
                    command_for(keyboard, char_press(letter)),
                    Some(Command::Act(Action::Move(_)))
                ),
                "{keyboard:?} {letter}"
            );
        }
        let quit = names.quit.chars().next().unwrap();
        assert_eq!(command_for(keyboard, char_press(quit)), Some(Command::Quit));
        assert_eq!(
            title_command_for(keyboard, char_press(quit)),
            Some(MenuCommand::Quit)
        );
        assert_eq!(
            end_command_for(keyboard, char_press(quit)),
            Some(EndCommand::Quit)
        );
        // The first and last keys of the ranges are bound as named.
        let ends = |name: &str| {
            let keys: Vec<char> = name.chars().collect();
            (keys[0], keys[keys.len() - 1])
        };
        let (first, last) = ends(&names.nodes);
        assert_eq!(
            trace_command_for(keyboard, char_press(first)),
            Some(TraceCommand::Link(node(1)))
        );
        assert_eq!(
            trace_command_for(keyboard, char_press(last)),
            Some(TraceCommand::Link(node(9)))
        );
        let (first, last) = ends(&names.slots);
        assert_eq!(
            inventory_command_for(keyboard, char_press(first)),
            Some(InventoryCommand::Slot(0))
        );
        assert_eq!(
            inventory_command_for(keyboard, char_press(last)),
            Some(InventoryCommand::Slot(9))
        );
        assert_eq!(
            command_for(keyboard, char_press(descend_keys(keyboard)[0])),
            Some(Command::Act(Action::Descend))
        );
    }
}
