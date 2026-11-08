//! The provisional end screen, shown once the player has died: who killed
//! the player and when, the run's seed, the last messages of the log, and
//! how to leave.
//!
//! Exposes [`render`]. The full death summary of `docs/design.md` (floor,
//! creatures killed, sigils cast, shareable line) replaces this screen in
//! T25; until then it keeps the summary's title and seed so a run can be
//! replayed, and the log stays where it was during the game, so the blows
//! that led to the death can still be read.

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use sigilfall_core::content::Content;
use sigilfall_core::game::{Game, Status};

use super::game::log_area;
use super::log;
use crate::message_log::MessageLog;
use crate::messages;
use crate::theme::Theme;

/// Draws the end screen of `game` in `area`: the summary centred in the
/// rows above the log, the messages of `log` in the same rows as on the
/// game screen, and the key help on the last row. Monster names come from
/// `content`.
///
/// The cause line is only written when the game is over; called on a game
/// still in progress (a caller's mistake), the screen still shows its title
/// and seed. The background is expected to be painted already, and an area
/// too small for the text is clipped without panicking.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    game: &Game,
    log: &MessageLog,
    content: &Content,
    theme: &Theme,
) {
    let log_area = log_area(area);
    let summary_area = Rect {
        height: log_area.y.saturating_sub(area.y),
        ..area
    };
    let lines = summary(game, content, theme);
    let text_height = u16::try_from(lines.len()).unwrap_or(u16::MAX);
    let top = summary_area.height.saturating_sub(text_height) / 2;
    let text_area = Rect {
        y: summary_area.y + top,
        height: summary_area.height - top,
        ..summary_area
    };
    frame.render_widget(
        Paragraph::new(lines).alignment(Alignment::Center),
        text_area,
    );

    log::render(frame.buffer_mut(), log_area, None, log, theme);

    let help = Line::from(vec![
        Span::raw(" "),
        Span::styled(messages::QUIT_KEY, theme.text),
        Span::styled(format!(" {}", messages::QUIT_LABEL), theme.text_muted),
    ]);
    let help_area = Rect {
        y: area.bottom().saturating_sub(1),
        height: area.height.min(1),
        ..area
    };
    frame.render_widget(help, help_area);
}

/// The centred lines of the end screen: title, subtitle, cause of death
/// (when the game is over), seed and the command that replays it.
fn summary(game: &Game, content: &Content, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::styled(messages::DEATH_TITLE, theme.text_title),
        Line::styled(messages::DEATH_SUBTITLE, theme.text_muted),
        Line::default(),
    ];
    if let Status::GameOver { killed_by } = game.status() {
        lines.push(Line::styled(
            messages::cause_of_death(killed_by, game.player_turns(), content),
            theme.text,
        ));
        lines.push(Line::default());
    }
    lines.push(Line::from(vec![
        Span::styled(format!("{}  ", messages::SEED_LABEL), theme.text_muted),
        Span::styled(game.seed().to_string(), theme.text_title),
    ]));
    lines.push(Line::styled(
        messages::replay_hint(game.seed()),
        theme.text_muted,
    ));
    lines
}
