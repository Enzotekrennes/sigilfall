//! The READMEs tell the truth: their keys, command-line options,
//! environment variables, files, data folders and pointers to other files
//! are checked against the program, from the player's side; the French
//! README follows the English one, section for section.

use super::player_view::{ctrl_c, files, launch_with_arguments, press};
use crate::cli::Cli;
use crate::input::{self, Command};
use crate::keyboard::Keyboard;
use crate::persist::{DATA_DIR_VAR, ScratchFolder};
use clap::{CommandFactory as _, Parser as _};
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use sigilfall_core::action::Action;
use sigilfall_core::geometry::Direction;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

// --- The README tells the truth ----------------------------------------------

/// The README, from the root of the repository.
pub(super) const README: &str = include_str!("../../../../../README.md");

/// The French README, beside it.
const README_FR: &str = include_str!("../../../../../README.fr.md");

/// Both READMEs, by name.
const READMES: [(&str, &str); 2] = [("README.md", README), ("README.fr.md", README_FR)];

/// A key press of `code`.
fn key(code: KeyCode) -> KeyEvent {
    press(code)
}

/// The command bound to `code` on the game screen of `keyboard`.
fn on_map(keyboard: Keyboard, code: KeyCode) -> Option<Command> {
    input::command_for(keyboard, key(code))
}

/// The keys the READMEs give for moving on both layouts, with their
/// direction.
const SHARED_MOVES: [(KeyCode, Direction); 8 + 4 + 8 + 4] = [
    (KeyCode::Up, Direction::North),
    (KeyCode::Down, Direction::South),
    (KeyCode::Left, Direction::West),
    (KeyCode::Right, Direction::East),
    (KeyCode::Char('h'), Direction::West),
    (KeyCode::Char('j'), Direction::South),
    (KeyCode::Char('k'), Direction::North),
    (KeyCode::Char('l'), Direction::East),
    (KeyCode::Char('y'), Direction::NorthWest),
    (KeyCode::Char('u'), Direction::NorthEast),
    (KeyCode::Char('b'), Direction::SouthWest),
    (KeyCode::Char('n'), Direction::SouthEast),
    (KeyCode::Char('1'), Direction::SouthWest),
    (KeyCode::Char('2'), Direction::South),
    (KeyCode::Char('3'), Direction::SouthEast),
    (KeyCode::Char('4'), Direction::West),
    (KeyCode::Char('6'), Direction::East),
    (KeyCode::Char('7'), Direction::NorthWest),
    (KeyCode::Char('8'), Direction::North),
    (KeyCode::Char('9'), Direction::NorthEast),
    (KeyCode::Home, Direction::NorthWest),
    (KeyCode::PageUp, Direction::NorthEast),
    (KeyCode::End, Direction::SouthWest),
    (KeyCode::PageDown, Direction::SouthEast),
];

#[test]
fn every_key_the_readme_gives_for_the_map_does_what_it_says_on_each_layout() {
    for keyboard in Keyboard::ALL {
        for (code, direction) in SHARED_MOVES {
            assert_eq!(
                on_map(keyboard, code),
                Some(Command::Act(Action::Move(direction))),
                "{keyboard:?} {code:?}"
            );
        }
        for code in [KeyCode::Char('.'), KeyCode::Char('5'), KeyCode::KeypadBegin] {
            assert_eq!(
                on_map(keyboard, code),
                Some(Command::Act(Action::Wait)),
                "{code:?}"
            );
        }
        assert_eq!(on_map(keyboard, KeyCode::Char('t')), Some(Command::Trace));
        for typed in ['g', ','] {
            assert_eq!(
                on_map(keyboard, KeyCode::Char(typed)),
                Some(Command::Act(Action::PickUp))
            );
        }
        assert_eq!(
            on_map(keyboard, KeyCode::Char('i')),
            Some(Command::Inventory)
        );
        assert_eq!(on_map(keyboard, KeyCode::Char('x')), Some(Command::Explore));
        assert_eq!(
            on_map(keyboard, KeyCode::Char('>')),
            Some(Command::Act(Action::Descend))
        );
        assert_eq!(input::command_for(keyboard, ctrl_c()), Some(Command::Quit));
        assert_eq!(
            on_map(keyboard, KeyCode::Esc),
            None,
            "Escape never quits and does nothing on the map"
        );
    }

    // QWERTY: WASD, `E` for the stairs, `q` to quit.
    let qwerty = Keyboard::Qwerty;
    for (typed, direction) in [
        ('w', Direction::North),
        ('a', Direction::West),
        ('s', Direction::South),
        ('d', Direction::East),
    ] {
        assert_eq!(
            on_map(qwerty, KeyCode::Char(typed)),
            Some(Command::Act(Action::Move(direction))),
            "{typed}"
        );
    }
    assert_eq!(
        on_map(qwerty, KeyCode::Char('E')),
        Some(Command::Act(Action::Descend))
    );
    assert_eq!(
        on_map(qwerty, KeyCode::Char('D')),
        None,
        "D is not the stairs"
    );
    assert_eq!(on_map(qwerty, KeyCode::Char('q')), Some(Command::Quit));
    assert_eq!(
        on_map(qwerty, KeyCode::Char('z')),
        None,
        "no key the README does not give"
    );

    // AZERTY: ZQSD, the top row without Shift, `<` and `E` for the stairs,
    // `p` to quit.
    let azerty = Keyboard::Azerty;
    for (typed, expected) in [
        ('z', Command::Act(Action::Move(Direction::North))),
        ('q', Command::Act(Action::Move(Direction::West))),
        ('s', Command::Act(Action::Move(Direction::South))),
        ('d', Command::Act(Action::Move(Direction::East))),
        ('&', Command::Act(Action::Move(Direction::SouthWest))),
        ('é', Command::Act(Action::Move(Direction::South))),
        ('"', Command::Act(Action::Move(Direction::SouthEast))),
        ('\'', Command::Act(Action::Move(Direction::West))),
        ('(', Command::Act(Action::Wait)),
        ('-', Command::Act(Action::Move(Direction::East))),
        ('è', Command::Act(Action::Move(Direction::NorthWest))),
        ('_', Command::Act(Action::Move(Direction::North))),
        ('ç', Command::Act(Action::Move(Direction::NorthEast))),
        ('<', Command::Act(Action::Descend)),
        ('E', Command::Act(Action::Descend)),
        ('p', Command::Quit),
    ] {
        assert_eq!(
            on_map(azerty, KeyCode::Char(typed)),
            Some(expected),
            "{typed}"
        );
    }
    assert_eq!(
        on_map(azerty, KeyCode::Char('D')),
        None,
        "D is not the stairs"
    );
    assert_eq!(
        on_map(azerty, KeyCode::Char('w')),
        None,
        "no key the README does not give"
    );

    assert!(README.contains("| Home, Page Up, End, Page Down | Home, Page Up, End, Page Down |"));
    // Both READMEs list these keys in their table.
    for (name, readme) in READMES {
        for fragment in [
            "`w a s d`",
            "`z q s d`",
            "`1`-`9`",
            "`& é \" ' - è _ ç`",
            "`h j k l y u b n`",
            "`.`, `5`, ",
            "`.`, `5` ",
            "`(`",
            "| `t` | `t` |",
            "| `g`, `,` | `g`, `,` |",
            "| `i` | `i` |",
            "| `>`, `E` | `>`, `<`, `E` |",
            "| `x` | `x` |",
            "| `q` | `p` |",
            "| Ctrl+C | Ctrl+C |",
        ] {
            assert!(readme.contains(fragment), "{name} names {fragment}");
        }
    }
}

#[test]
fn every_key_the_readme_gives_for_the_other_screens_does_what_it_says() {
    use input::{
        EndCommand, InventoryCommand, MenuCommand, OptionsCommand, SeedCommand, TraceCommand,
    };
    for keyboard in Keyboard::ALL {
        let [north, west, south, east] = input::movement_letters(keyboard);
        let quit = input::quit_key(keyboard);

        // The trace grid.
        for digit in '1'..='9' {
            assert!(
                matches!(
                    input::trace_command_for(keyboard, key(KeyCode::Char(digit))),
                    Some(TraceCommand::Link(_))
                ),
                "{digit}"
            );
        }
        for (code, command) in [
            (KeyCode::Backspace, TraceCommand::Undo),
            (KeyCode::Enter, TraceCommand::Cast),
            (KeyCode::Esc, TraceCommand::Cancel),
        ] {
            assert_eq!(input::trace_command_for(keyboard, key(code)), Some(command));
        }
        assert_eq!(
            input::trace_command_for(keyboard, key(KeyCode::Char(quit))),
            None,
            "the quit key does not quit the grid"
        );

        // The inventory: Up and Down, k and j or the layout's letters,
        // Home and End, digits, Enter or u, d, Escape.
        for (code, command) in [
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
        ] {
            assert_eq!(
                input::inventory_command_for(keyboard, key(code)),
                Some(command),
                "{keyboard:?} {code:?}"
            );
        }
        for (digit, slot) in ('1'..='9').zip(0..9).chain([('0', 9)]) {
            assert_eq!(
                input::inventory_command_for(keyboard, key(KeyCode::Char(digit))),
                Some(InventoryCommand::Slot(slot)),
                "{digit}"
            );
        }

        // The title and the confirmation.
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
        ] {
            assert_eq!(
                input::title_command_for(keyboard, key(code)),
                Some(command),
                "title {code:?}"
            );
            assert_eq!(
                input::confirm_command_for(keyboard, key(code)),
                Some(command),
                "confirm {code:?}"
            );
        }
        assert_eq!(
            input::title_command_for(keyboard, key(KeyCode::Char(quit))),
            Some(MenuCommand::Quit)
        );
        assert_eq!(input::title_command_for(keyboard, key(KeyCode::Esc)), None);
        assert_eq!(
            input::confirm_command_for(keyboard, key(KeyCode::Esc)),
            Some(MenuCommand::Back)
        );
        assert_eq!(
            input::confirm_command_for(keyboard, key(KeyCode::Char(quit))),
            None
        );

        // The options screen.
        for (code, command) in [
            (KeyCode::Up, OptionsCommand::Previous),
            (KeyCode::Char('k'), OptionsCommand::Previous),
            (KeyCode::Char(north), OptionsCommand::Previous),
            (KeyCode::Down, OptionsCommand::Next),
            (KeyCode::Char('j'), OptionsCommand::Next),
            (KeyCode::Char(south), OptionsCommand::Next),
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
                input::options_command_for(keyboard, key(code)),
                Some(command),
                "{keyboard:?} {code:?}"
            );
        }
        assert_eq!(
            input::options_command_for(keyboard, key(KeyCode::Char(quit))),
            None
        );

        // The end screen.
        assert_eq!(
            input::end_command_for(keyboard, key(KeyCode::Enter)),
            Some(EndCommand::Title)
        );
        assert_eq!(
            input::end_command_for(keyboard, key(KeyCode::Char(quit))),
            Some(EndCommand::Quit)
        );
        assert_eq!(input::end_command_for(keyboard, key(KeyCode::Esc)), None);
        assert_eq!(
            input::end_command_for(keyboard, ctrl_c()),
            Some(EndCommand::Interrupt)
        );
    }

    // On AZERTY the top row without Shift links the nodes and chooses the
    // slots, `&` to `ç` and `&` to `à`.
    for (digit, typed) in (1..=9).zip("&é\"'(-è_ç".chars()) {
        let node = sigilfall_core::sigil::Node::new(digit).unwrap();
        assert_eq!(
            input::trace_command_for(Keyboard::Azerty, key(KeyCode::Char(typed))),
            Some(TraceCommand::Link(node)),
            "{typed}"
        );
    }
    for (slot, typed) in "&é\"'(-è_çà".chars().enumerate() {
        assert_eq!(
            input::inventory_command_for(Keyboard::Azerty, key(KeyCode::Char(typed))),
            Some(InventoryCommand::Slot(slot)),
            "{typed}"
        );
    }

    // The seed entry, the same on both layouts.
    for (code, command) in [
        (KeyCode::Backspace, SeedCommand::Erase),
        (KeyCode::Enter, SeedCommand::Submit),
        (KeyCode::Esc, SeedCommand::Back),
    ] {
        assert_eq!(input::seed_command_for(key(code)), Some(command));
    }
}

#[test]
fn each_readme_links_the_other_at_its_top_and_both_follow_the_same_outline() {
    let top = |readme: &str| readme.lines().take(4).collect::<Vec<_>>().join("\n");
    assert!(
        top(README).contains("[Français](README.fr.md)"),
        "{}",
        top(README)
    );
    assert!(
        top(README_FR).contains("[English](README.md)"),
        "{}",
        top(README_FR)
    );

    // The same headings, one for one, at the same levels.
    let outline = |readme: &str| -> Vec<usize> {
        readme
            .lines()
            .filter(|line| line.starts_with('#'))
            .map(|line| line.chars().take_while(|c| *c == '#').count())
            .collect()
    };
    assert_eq!(outline(README), outline(README_FR));
    // The same tables, row for row.
    let rows = |readme: &str| readme.lines().filter(|line| line.starts_with('|')).count();
    assert_eq!(rows(README), rows(README_FR));
    // The same links, images and code, so a fix in one is made in the other.
    for readme in [README, README_FR] {
        for shared in [
            "assets/demo.gif",
            "(ARCHITECTURE.md)",
            "(LICENSE-APACHE)",
            "(LICENSE-MIT)",
            "(demo.tape)",
            "github.com/Enzotekrennes/sigilfall/releases/latest/download/sigilfall-installer.sh",
            "sigilfall-installer.ps1",
            "cargo install --locked --git https://github.com/Enzotekrennes/sigilfall sigilfall",
            "RUSTDOCFLAGS=\"-D warnings\" cargo doc --workspace --no-deps --locked",
            "`app::tests::demo_tape`",
            "Rust 1.88",
            "Sigilfall daily 2026-11-08 - depth 4, turn 812, 23 slain, 9 sigils",
        ] {
            assert!(readme.contains(shared), "{shared}");
        }
    }
    // The code is the same; only its comments are translated.
    let code_blocks = |readme: &str| -> Vec<String> {
        readme
            .split("```")
            .skip(1)
            .step_by(2)
            .flat_map(str::lines)
            .map(|line| line.split(" #").next().unwrap_or_default().trim_end())
            .filter(|line| !line.starts_with('#'))
            .map(str::to_owned)
            .collect()
    };
    assert_eq!(code_blocks(README), code_blocks(README_FR));
}

#[test]
fn the_command_line_options_of_the_readme_are_exactly_the_ones_of_the_program() {
    let real: BTreeSet<String> = Cli::command()
        .get_arguments()
        .filter_map(|argument| argument.get_long().map(|long| format!("--{long}")))
        .chain(["--help".to_owned(), "--version".to_owned()])
        .collect();
    for (name, readme, section, next) in [
        ("README.md", README, "### Command line", "### Keys"),
        (
            "README.fr.md",
            README_FR,
            "### Ligne de commande",
            "### Touches",
        ),
    ] {
        let table = readme
            .split(section)
            .nth(1)
            .and_then(|rest| rest.split(next).next())
            .expect("a command line section");
        assert_eq!(
            documented_options(table),
            real,
            "the table of {name} and the program agree"
        );
    }

    // Each of them is accepted, and `--help` and `--version` are real.
    assert!(Cli::try_parse_from(["sigilfall", "--seed", "42"]).is_ok());
    assert!(Cli::try_parse_from(["sigilfall", "--daily"]).is_ok());
    assert!(Cli::try_parse_from(["sigilfall", "--ascii"]).is_ok());
    assert!(Cli::try_parse_from(["sigilfall", "--lang", "fr"]).is_ok());
    assert!(Cli::try_parse_from(["sigilfall", "--keyboard", "azerty"]).is_ok());
    for flag in ["--help", "--version"] {
        let error = Cli::try_parse_from(["sigilfall", flag]).unwrap_err();
        assert_eq!(error.exit_code(), 0, "{flag} prints and exits 0");
    }
    assert!(Cli::try_parse_from(["sigilfall", "--nope"]).is_err());
}

/// The options named in the first column of the command-line `table` of a
/// README.
fn documented_options(table: &str) -> BTreeSet<String> {
    table
        .lines()
        .filter(|line| line.starts_with("| `--"))
        .flat_map(|line| {
            line.split('|')
                .nth(1)
                .unwrap_or("")
                .split(',')
                .filter_map(|cell| {
                    cell.trim()
                        .trim_matches('`')
                        .split_whitespace()
                        .next()
                        .map(str::to_owned)
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn the_environment_variables_and_files_of_the_readme_are_the_real_ones() {
    assert_eq!(DATA_DIR_VAR, "SIGILFALL_DATA_DIR");
    for variable in ["SIGILFALL_DATA_DIR", "NO_COLOR", "COLORTERM", "WT_SESSION"] {
        for (name, readme) in READMES {
            assert!(readme.contains(variable), "{name} names {variable}");
        }
    }

    // The program reads no environment variable the README does not name.
    let mut read = BTreeSet::new();
    let sources = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut pending = vec![sources];
    while let Some(folder) = pending.pop() {
        for entry in fs::read_dir(folder).unwrap().filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs")
                && !path.components().any(|part| part.as_os_str() == "tests")
            {
                let source = fs::read_to_string(&path).unwrap();
                for call in ["var(\"", "var_os(\""] {
                    for piece in source.split(call).skip(1) {
                        if let Some(name) = piece.split('"').next() {
                            read.insert(name.to_owned());
                        }
                    }
                }
            }
        }
    }
    read.retain(|name| name.chars().all(|c| c.is_ascii_uppercase() || c == '_'));
    for name in &read {
        assert!(
            README.contains(name.as_str()),
            "the program reads {name}, which the README does not name"
        );
    }
    for name in ["NO_COLOR", "COLORTERM", "WT_SESSION"] {
        assert!(
            read.contains(name),
            "{name} is really read, not only documented: {read:?}"
        );
    }

    // The files: save.ron and options.ron beside each other, save.ron.bad.
    let folder = ScratchFolder::new("readme-files");
    assert_eq!(folder.save_file().path().file_name().unwrap(), "save.ron");
    assert_eq!(
        folder.options_file().path().file_name().unwrap(),
        "options.ron"
    );
    assert_eq!(
        folder.save_file().path().parent(),
        folder.options_file().path().parent()
    );
    fs::write(folder.save_file().path(), "not a save").unwrap();
    let app = launch_with_arguments(&folder, &[]).unwrap();
    assert!(!app.has_saved_game());
    assert!(
        folder.path().join("save.ron.bad").is_file(),
        "an unreadable save is renamed save.ron.bad: {:?}",
        files(&folder)
    );
    for readme in [README, README_FR] {
        assert!(readme.contains("save.ron.bad") && readme.contains("`options.ron`"));
    }
}

#[test]
fn the_data_folder_the_readme_names_for_each_system_is_the_one_the_program_asks_the_system_for() {
    // The program asks the `directories` crate for the data folder of the
    // application "Sigilfall"; that crate's rules give the three paths of
    // the README.
    assert_eq!(crate::persist::APPLICATION, "Sigilfall");
    let asked = crate::persist::system_data_folder();
    assert_eq!(
        asked,
        directories::ProjectDirs::from("", "", "Sigilfall")
            .map(|dirs| dirs.data_dir().to_path_buf()),
        "the program asks the system for the folder of the application Sigilfall"
    );
    for row in [
        "`%APPDATA%\\Sigilfall\\data`",
        "`$XDG_DATA_HOME/sigilfall`",
        "`~/.local/share/sigilfall`",
        "`~/Library/Application Support/Sigilfall`",
    ] {
        for (name, readme) in READMES {
            assert!(readme.contains(row), "{name} gives {row}");
        }
    }
    // Nothing is created by asking: only the path is computed.
    if let Some(dirs) = directories::ProjectDirs::from("", "", "Sigilfall") {
        let path = dirs.data_dir().to_path_buf();
        if cfg!(windows) {
            let appdata = std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .expect("APPDATA on Windows");
            assert_eq!(path, appdata.join("Sigilfall").join("data"));
        } else if cfg!(target_os = "macos") {
            assert!(
                path.ends_with("Library/Application Support/Sigilfall"),
                "{path:?}"
            );
        } else {
            assert!(path.ends_with("sigilfall"), "{path:?}");
        }
    }
}

#[test]
fn the_files_the_readme_points_to_exist_and_the_placeholders_are_the_known_ones() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for file in [
        "ARCHITECTURE.md",
        "LICENSE-APACHE",
        "LICENSE-MIT",
        "demo.tape",
        "dist-workspace.toml",
        "rust-toolchain.toml",
        "Cargo.lock",
    ] {
        assert!(root.join(file).is_file(), "{file} exists");
    }
    for link in [
        "(ARCHITECTURE.md)",
        "(LICENSE-APACHE)",
        "(LICENSE-MIT)",
        "(demo.tape)",
    ] {
        assert!(README.contains(link), "the README links {link}");
    }
    // The repository the installers download from, and no placeholder left.
    assert!(README.contains("github.com/Enzotekrennes/sigilfall"));
    assert!(!README.contains("OWNER"), "no placeholder account is left");
    let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert!(
        manifest.contains("repository = \"https://github.com/Enzotekrennes/sigilfall\""),
        "Cargo.toml names the same repository"
    );
    assert!(!manifest.contains("OWNER"), "no placeholder in Cargo.toml");
    // The demo recorded from demo.tape is committed: the README shows it.
    assert!(README.contains("assets/demo.gif"));
    assert!(
        root.join("assets/demo.gif").is_file(),
        "assets/demo.gif is committed"
    );
    let toolchain = fs::read_to_string(root.join("rust-toolchain.toml")).unwrap();
    assert!(
        toolchain.contains("channel = \"stable\""),
        "README: the toolchain is pinned to stable"
    );
    assert!(
        manifest.contains("rust-version = \"1.88\""),
        "README: minimum supported version 1.88"
    );
    assert!(README.contains("Rust 1.88"));
    // The test the README names exists.
    assert!(README.contains("`app::tests::demo_tape`"));
    assert!(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/app/tests/demo_tape.rs")
            .is_file(),
        "the module the README names exists"
    );
}
