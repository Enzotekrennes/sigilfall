//! The victory screen, shown once the player has broken the seal of the
//! bottom floor (decision D23): how the run ended, then the same summary as
//! the death screen.
//!
//! Exposes [`render`]. The screen answers the death summary of
//! `docs/design.md` line for line, so the two end screens read as a pair:
//! the title "T H E   S E A L   B R E A K S" in `text_title` where the death
//! screen has "T H E   D E S C E N T   E N D S", a line chosen from the seed
//! among `messages::VICTORY_LINES` in `text_muted` where it has an epitaph,
//! and "You broke the seal at depth 10, on turn 4120." in `text` where it
//! has the cause of death. Everything below (the counts, the seed in its
//! frame, the replay command, the line to share, which says "victory", and
//! the key help) is drawn by `ui::death::render_summary`, so a count or a
//! rule changed there changes on both screens.
//!
//! Like the death summary, the screen fits 80 × 24, is drawn with the
//! theme's tokens and glyphs only, and says everything in words: in
//! monochrome or in ASCII it reads the same.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use sigilfall_core::content::Content;
use sigilfall_core::game::Game;

use super::death;
use crate::messages;
use crate::theme::Theme;

/// Draws the victory screen of `game` over `area`: its title, the line of
/// its seed and where the seal was broken, then the summary shared with the
/// death screen, centred in the rows above the last one, and the key help
/// on the last row. Monster names come from `content`.
///
/// Meant for a game whose status is `Status::Won` (`ui::render` sorts the
/// end screens on it); the depth and turn written are those of `game`
/// whatever its status. The background is expected to be painted already,
/// and an area too small for the text is clipped without panicking.
pub fn render(frame: &mut Frame, area: Rect, game: &Game, content: &Content, theme: &Theme) {
    let head = vec![
        Line::styled(messages::VICTORY_TITLE, theme.text_title),
        Line::styled(messages::victory_line(game.seed()), theme.text_muted),
        Line::default(),
        Line::styled(
            messages::victory_outcome(game.depth(), game.player_turns()),
            theme.text,
        ),
        Line::default(),
    ];
    death::render_summary(frame, area, head, game, content, theme);
}
