//! The README tells the truth: its keys, command-line options, environment
//! variables, files, data folders and pointers to other files are checked
//! against the program, from the player's side.

use super::player_view::{ctrl_c, files, launch_with_arguments, press};
use crate::cli::Cli;
use crate::input::{self, Command};
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

/// A key press of `code`.
fn key(code: KeyCode) -> KeyEvent {
    press(code)
}

/// The command bound to `code` on the game screen.
fn on_map(code: KeyCode) -> Option<Command> {
    input::command_for(key(code))
}

#[test]
fn every_key_the_readme_gives_for_the_map_does_what_it_says() {
    let moves: [(KeyCode, Direction); 8 + 4 + 8 + 4] = [
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
    for (code, direction) in moves {
        assert_eq!(
            on_map(code),
            Some(Command::Act(Action::Move(direction))),
            "{code:?}"
        );
    }
    for code in [KeyCode::Char('.'), KeyCode::Char('5'), KeyCode::KeypadBegin] {
        assert_eq!(on_map(code), Some(Command::Act(Action::Wait)), "{code:?}");
    }
    assert_eq!(on_map(KeyCode::Char('t')), Some(Command::Trace));
    assert_eq!(
        on_map(KeyCode::Char('g')),
        Some(Command::Act(Action::PickUp))
    );
    assert_eq!(
        on_map(KeyCode::Char(',')),
        Some(Command::Act(Action::PickUp))
    );
    assert_eq!(on_map(KeyCode::Char('i')), Some(Command::Inventory));
    assert_eq!(
        on_map(KeyCode::Char('>')),
        Some(Command::Act(Action::Descend))
    );
    assert_eq!(
        on_map(KeyCode::Char('D')),
        Some(Command::Act(Action::Descend))
    );
    assert_eq!(on_map(KeyCode::Char('q')), Some(Command::Quit));
    assert_eq!(input::command_for(ctrl_c()), Some(Command::Quit));
    assert_eq!(
        on_map(KeyCode::Esc),
        None,
        "Escape never quits and does nothing on the map"
    );
    assert_eq!(
        on_map(KeyCode::Char('d')),
        None,
        "lower-case d is not the stairs"
    );
    assert_eq!(
        on_map(KeyCode::Char('z')),
        None,
        "no key the README does not give"
    );

    // The README lists these keys in its table.
    for fragment in [
        "Arrows",
        "`1`-`9`",
        "`h j k l y u b n`",
        "Home, Page Up, End, Page Down",
        "`.`, `5`",
        "`t`",
        "`g`, `,`",
        "`i`",
        "`>`, `D`",
        "`q`",
        "Ctrl+C",
    ] {
        assert!(README.contains(fragment), "the README names {fragment}");
    }
}

#[test]
fn every_key_the_readme_gives_for_the_other_screens_does_what_it_says() {
    use input::{
        EndCommand, InventoryCommand, MenuCommand, OptionsCommand, SeedCommand, TraceCommand,
    };
    // The trace grid.
    for digit in '1'..='9' {
        assert!(
            matches!(
                input::trace_command_for(key(KeyCode::Char(digit))),
                Some(TraceCommand::Link(_))
            ),
            "{digit}"
        );
    }
    assert_eq!(
        input::trace_command_for(key(KeyCode::Backspace)),
        Some(TraceCommand::Undo)
    );
    assert_eq!(
        input::trace_command_for(key(KeyCode::Enter)),
        Some(TraceCommand::Cast)
    );
    assert_eq!(
        input::trace_command_for(key(KeyCode::Esc)),
        Some(TraceCommand::Cancel)
    );
    assert_eq!(
        input::trace_command_for(key(KeyCode::Char('q'))),
        None,
        "q does not quit the grid"
    );

    // The inventory: Up and Down or k and j, Home and End, digits, Enter or u, d, Escape.
    assert_eq!(
        input::inventory_command_for(key(KeyCode::Up)),
        Some(InventoryCommand::Previous)
    );
    assert_eq!(
        input::inventory_command_for(key(KeyCode::Char('k'))),
        Some(InventoryCommand::Previous)
    );
    assert_eq!(
        input::inventory_command_for(key(KeyCode::Down)),
        Some(InventoryCommand::Next)
    );
    assert_eq!(
        input::inventory_command_for(key(KeyCode::Char('j'))),
        Some(InventoryCommand::Next)
    );
    assert_eq!(
        input::inventory_command_for(key(KeyCode::Home)),
        Some(InventoryCommand::First)
    );
    assert_eq!(
        input::inventory_command_for(key(KeyCode::End)),
        Some(InventoryCommand::Last)
    );
    for (digit, slot) in ('1'..='9').zip(0..9).chain([('0', 9)]) {
        assert_eq!(
            input::inventory_command_for(key(KeyCode::Char(digit))),
            Some(InventoryCommand::Slot(slot)),
            "{digit}"
        );
    }
    assert_eq!(
        input::inventory_command_for(key(KeyCode::Enter)),
        Some(InventoryCommand::Use)
    );
    assert_eq!(
        input::inventory_command_for(key(KeyCode::Char('u'))),
        Some(InventoryCommand::Use)
    );
    assert_eq!(
        input::inventory_command_for(key(KeyCode::Char('d'))),
        Some(InventoryCommand::Drop)
    );
    assert_eq!(
        input::inventory_command_for(key(KeyCode::Esc)),
        Some(InventoryCommand::Close)
    );

    // The title and the confirmation.
    for (code, command) in [
        (KeyCode::Up, MenuCommand::Previous),
        (KeyCode::Char('k'), MenuCommand::Previous),
        (KeyCode::Down, MenuCommand::Next),
        (KeyCode::Char('j'), MenuCommand::Next),
        (KeyCode::Home, MenuCommand::First),
        (KeyCode::End, MenuCommand::Last),
        (KeyCode::Enter, MenuCommand::Confirm),
    ] {
        assert_eq!(
            input::title_command_for(key(code)),
            Some(command),
            "title {code:?}"
        );
        assert_eq!(
            input::confirm_command_for(key(code)),
            Some(command),
            "confirm {code:?}"
        );
    }
    assert_eq!(
        input::title_command_for(key(KeyCode::Char('q'))),
        Some(MenuCommand::Quit)
    );
    assert_eq!(input::title_command_for(key(KeyCode::Esc)), None);
    assert_eq!(
        input::confirm_command_for(key(KeyCode::Esc)),
        Some(MenuCommand::Back)
    );
    assert_eq!(input::confirm_command_for(key(KeyCode::Char('q'))), None);

    // The seed entry.
    assert_eq!(
        input::seed_command_for(key(KeyCode::Backspace)),
        Some(SeedCommand::Erase)
    );
    assert_eq!(
        input::seed_command_for(key(KeyCode::Enter)),
        Some(SeedCommand::Submit)
    );
    assert_eq!(
        input::seed_command_for(key(KeyCode::Esc)),
        Some(SeedCommand::Back)
    );

    // The options screen.
    for (code, command) in [
        (KeyCode::Up, OptionsCommand::Previous),
        (KeyCode::Char('k'), OptionsCommand::Previous),
        (KeyCode::Down, OptionsCommand::Next),
        (KeyCode::Char('j'), OptionsCommand::Next),
        (KeyCode::Left, OptionsCommand::PreviousValue),
        (KeyCode::Char('h'), OptionsCommand::PreviousValue),
        (KeyCode::Right, OptionsCommand::NextValue),
        (KeyCode::Char('l'), OptionsCommand::NextValue),
        (KeyCode::Enter, OptionsCommand::Confirm),
        (KeyCode::Char(' '), OptionsCommand::Confirm),
        (KeyCode::Esc, OptionsCommand::Back),
    ] {
        assert_eq!(
            input::options_command_for(key(code)),
            Some(command),
            "{code:?}"
        );
    }
    assert_eq!(input::options_command_for(key(KeyCode::Char('q'))), None);

    // The end screen.
    assert_eq!(
        input::end_command_for(key(KeyCode::Enter)),
        Some(EndCommand::Title)
    );
    assert_eq!(
        input::end_command_for(key(KeyCode::Char('q'))),
        Some(EndCommand::Quit)
    );
    assert_eq!(input::end_command_for(key(KeyCode::Esc)), None);
    assert_eq!(
        input::end_command_for(ctrl_c()),
        Some(EndCommand::Interrupt)
    );
}

#[test]
fn the_command_line_options_of_the_readme_are_exactly_the_ones_of_the_program() {
    let table = README
        .split("### Command line")
        .nth(1)
        .and_then(|rest| rest.split("### Keys").next())
        .expect("a command line section");
    let documented: BTreeSet<String> = table
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
        .collect();
    let real: BTreeSet<String> = Cli::command()
        .get_arguments()
        .filter_map(|argument| argument.get_long().map(|long| format!("--{long}")))
        .chain(["--help".to_owned(), "--version".to_owned()])
        .collect();
    assert_eq!(documented, real, "the README table and the program agree");

    // Each of them is accepted, and `--help` and `--version` are real.
    assert!(Cli::try_parse_from(["sigilfall", "--seed", "42"]).is_ok());
    assert!(Cli::try_parse_from(["sigilfall", "--daily"]).is_ok());
    assert!(Cli::try_parse_from(["sigilfall", "--ascii"]).is_ok());
    for flag in ["--help", "--version"] {
        let error = Cli::try_parse_from(["sigilfall", flag]).unwrap_err();
        assert_eq!(error.exit_code(), 0, "{flag} prints and exits 0");
    }
    assert!(Cli::try_parse_from(["sigilfall", "--nope"]).is_err());
}

#[test]
fn the_environment_variables_and_files_of_the_readme_are_the_real_ones() {
    assert_eq!(DATA_DIR_VAR, "SIGILFALL_DATA_DIR");
    for variable in ["SIGILFALL_DATA_DIR", "NO_COLOR", "COLORTERM", "WT_SESSION"] {
        assert!(README.contains(variable), "the README names {variable}");
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
                && !path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().contains("acceptance"))
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
    let folder = ScratchFolder::new("f10-readme-files");
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
    assert!(README.contains("save.ron.bad") && README.contains("`options.ron`"));
}

#[test]
fn the_data_folder_the_readme_names_for_each_system_is_the_one_the_program_asks_the_system_for() {
    // The program asks the `directories` crate for the data folder of the
    // application "Sigilfall" (read from the source, because the constant is
    // private); that crate's rules give the three paths of the README.
    let persist = include_str!("../../persist.rs");
    assert!(persist.contains("const APPLICATION: &str = \"Sigilfall\";"));
    assert!(persist.contains("ProjectDirs::from(\"\", \"\", APPLICATION)"));
    for row in [
        "`%APPDATA%\\Sigilfall\\data`",
        "`$XDG_DATA_HOME/sigilfall`",
        "`~/.local/share/sigilfall`",
        "`~/Library/Application Support/Sigilfall`",
    ] {
        assert!(README.contains(row), "the README gives {row}");
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
    // Placeholders to fill in phase 6, and nothing else of that kind.
    assert!(README.contains("github.com/OWNER/sigilfall"));
    assert!(README.contains("assets/demo.gif"));
    assert!(
        !root.join("assets/demo.gif").exists(),
        "assets/demo.gif is still to be recorded by CI (phase 6): when it exists, delete this check"
    );
    let toolchain = fs::read_to_string(root.join("rust-toolchain.toml")).unwrap();
    assert!(
        toolchain.contains("channel = \"stable\""),
        "README: the toolchain is pinned to stable"
    );
    let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
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
