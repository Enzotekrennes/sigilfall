//! The overlays leave no sliver of the map against their frame: on 80 × 24,
//! the map column between the 57-column frame and the side panel, and the map
//! row under it, are blank.

use super::*;

/// The map cells of an 80 × 24 screen around the overlays' frame:
/// column 57 of the map rows, then row 16 left of the side panel.
fn gutter() -> Vec<(u16, u16)> {
    let column = (1..=16).map(|y| (57, y));
    let row = (0..57).map(|x| (x, 16));
    column.chain(row).collect()
}

/// The gutter cells of `buffer` that show something.
fn drawn(buffer: &Buffer) -> Vec<(u16, u16)> {
    gutter()
        .into_iter()
        .filter(|&cell| buffer[cell].symbol() != " ")
        .collect()
}

#[test]
fn the_inventory_and_the_trace_blank_the_map_around_their_frame() {
    let theme = Theme::new(ColorMode::TrueColorDark, GlyphSet::Unicode);
    let mut app = generated_floor_app();
    // Walk towards the stairs until map glyphs fill the gutter, so
    // the test shows they are hidden rather than never drawn.
    let floor = generation::generate(SEED, FIRST_DEPTH);
    let to_stairs = DijkstraMap::new(floor.map(), [floor.stairs()]);
    while drawn(render(&app, 80, 24).backend().buffer()).is_empty() {
        let direction = to_stairs
            .downhill(player_position(&app))
            .expect("map glyphs reach the gutter before the stairs");
        app.handle_key(press(vi_key(direction)));
    }

    for (open, close) in [
        (KeyCode::Char('i'), KeyCode::Esc),
        (KeyCode::Char('t'), KeyCode::Esc),
    ] {
        app.handle_key(press(open));
        let buffer = render(&app, 80, 24).backend().buffer().clone();

        assert_eq!(drawn(&buffer), [], "{open:?}: the gutter is blank");
        for cell in gutter() {
            assert_eq!(Some(buffer[cell].bg), theme.bg.bg, "{open:?} {cell:?}");
        }
        assert_eq!(buffer[(56, 1)].symbol(), "╮", "{open:?}: frame corner");
        assert_eq!(buffer[(58, 1)].symbol(), "│", "{open:?}: panel border");
        app.handle_key(press(close));
    }
    assert!(!drawn(render(&app, 80, 24).backend().buffer()).is_empty());
}
