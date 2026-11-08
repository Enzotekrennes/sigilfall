//! The key help at the bottom of every screen fits in 80 columns, whole.

use super::player_view::{enter_options, go_to, launch_with_arguments, press, rows, save_game};
use crate::app::App;
use crate::persist::ScratchFolder;
use crate::ui::title::TitleItem;
use ratatui::crossterm::event::KeyCode;

// --- The key help fits in 80 columns on every screen --------------------------

#[test]
fn the_key_help_of_every_screen_fits_in_80_columns_whole() {
    let folder = ScratchFolder::new("help");
    save_game(&folder, 7, 1);

    let mut screens: Vec<(&str, App, Vec<&str>)> = Vec::new();
    let title = launch_with_arguments(&folder, &[]).unwrap();
    screens.push((
        "title",
        title,
        vec!["↑↓ choose", "Enter confirm", "q quit", "v0."],
    ));
    let mut seed = launch_with_arguments(&folder, &[]).unwrap();
    go_to(&mut seed, TitleItem::Seed);
    seed.handle_key(press(KeyCode::Enter));
    screens.push((
        "seed entry",
        seed,
        vec!["Enter start", "Backspace erase", "Esc back"],
    ));
    let mut confirm = launch_with_arguments(&folder, &[]).unwrap();
    go_to(&mut confirm, TitleItem::NewGame);
    confirm.handle_key(press(KeyCode::Enter));
    screens.push(("confirmation", confirm, vec!["Enter", "Esc"]));
    let mut options = launch_with_arguments(&folder, &[]).unwrap();
    enter_options(&mut options);
    screens.push((
        "options",
        options,
        vec!["↑↓ choose", "←→ change", "Esc back"],
    ));

    let mut game = launch_with_arguments(&folder, &[]).unwrap();
    game.handle_key(press(KeyCode::Enter));
    let mut trace = launch_with_arguments(&folder, &[]).unwrap();
    trace.handle_key(press(KeyCode::Enter));
    trace.handle_key(press(KeyCode::Char('t')));
    let mut pack = launch_with_arguments(&folder, &[]).unwrap();
    pack.handle_key(press(KeyCode::Enter));
    pack.handle_key(press(KeyCode::Char('i')));
    screens.push((
        "game",
        game,
        vec![
            "←↑→↓/1-9 move",
            ". wait",
            "t trace",
            "g take",
            "i pack",
            "> descend",
            "q save & quit",
        ],
    ));
    screens.push((
        "trace",
        trace,
        vec![
            "1-9 link a node",
            "Backspace undo",
            "Enter cast",
            "Esc cancel",
        ],
    ));
    screens.push(("inventory", pack, vec!["Esc close"]));

    for (name, app, parts) in &screens {
        let screen = rows(app, 80, 24);
        let help = screen.last().unwrap();
        assert!(help.chars().count() <= 80, "{name}: {help:?}");
        for part in parts {
            assert!(
                help.contains(part),
                "{name}: `{part}` is missing from {help:?}"
            );
        }
    }
    // The ASCII game help is the same words with plain arrows.
    let ascii = ScratchFolder::new("help-ascii");
    let mut app = launch_with_arguments(&ascii, &["--ascii", "--seed", "42"]).unwrap();
    app.handle_key(press(KeyCode::Char('.')));
    let help = rows(&app, 80, 24).pop().unwrap();
    assert!(help.is_ascii() && help.chars().count() <= 80, "{help:?}");
    assert!(
        help.contains("move") && help.contains("save & quit"),
        "{help:?}"
    );

    // Below 80 x 24 the game says so instead of drawing a cut screen.
    for (width, height) in [(79, 24), (80, 23), (60, 15), (1, 1)] {
        let small = rows(&screens[5].1, width, height);
        let joined = small.join(" ");
        assert!(
            joined.contains("Enlarge the terminal to 80") || width < 20,
            "{width}x{height}: {joined:?}"
        );
    }
}
