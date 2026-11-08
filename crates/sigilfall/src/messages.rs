//! Every English sentence and label of the message log, the side panel and
//! the end screen, and the translation of game events into log messages.
//!
//! Exposes [`describe`], which turns the events of one `Game::submit` into
//! [`Message`]s, the opening message ([`welcome`]), the cause of death of
//! the end screen ([`cause_of_death`]), monster names ([`monster_name`]) and
//! the fixed labels of those screens. Keeping the words here, away from the
//! drawing code, is what decision D10 asks: a translation would only touch
//! this file. The status line and key help of the game screen predate this
//! module and keep their words in `ui::game`.
//!
//! The game itself never writes text (decision D4): it reports events, and
//! this module chooses what the player reads about them.

use std::collections::HashSet;

use sigilfall_core::actor::ActorId;
use sigilfall_core::combat::Combatant;
use sigilfall_core::content::Content;
use sigilfall_core::event::Event;
use sigilfall_core::game::Game;
use sigilfall_core::rng::Seed;

use crate::message_log::{Message, Tone};

/// Title of the side panel's health block.
pub const HP_LABEL: &str = "HP";

/// Mark written before the health value below a quarter of the maximum, so
/// the danger reads without colour (`docs/design.md`, "PV").
pub const LOW_HP_MARK: &str = "!";

/// Title of the side panel's list of what the player sees.
pub const IN_VIEW_TITLE: &str = "IN VIEW";

/// Name of the player in the "in view" list, after the `@`.
pub const PLAYER_IN_VIEW: &str = "you";

/// Title of the separator above the message log.
pub const LOG_TITLE: &str = "Log";

/// Title of the end screen, letter-spaced as in the death summary of
/// `docs/design.md`.
pub const DEATH_TITLE: &str = "T H E   D E S C E N T   E N D S";

/// Line under the end screen's title.
pub const DEATH_SUBTITLE: &str = "The walls will keep your trace.";

/// Label before the seed on the end screen.
pub const SEED_LABEL: &str = "Seed";

/// Key that leaves the game from the end screen, as its key help shows it.
pub const QUIT_KEY: &str = "q";

/// What [`QUIT_KEY`] does, as the end screen's key help shows it.
pub const QUIT_LABEL: &str = "quit";

/// The first message of every run, so the log is never empty and tells
/// where the player stands.
pub fn welcome() -> Message {
    Message::new(Tone::Info, "You enter the dungeon.")
}

/// Turns `events`, the result of one `Game::submit` on `game`, into the
/// messages the player reads, in the same order.
///
/// `game` is the game after the call, so it still knows every actor that
/// survived; one that died during the call is named from its
/// [`Event::Died`], which the events always carry. Monster names come from
/// `content`.
///
/// What is written:
/// - a blow: who struck whom and the damage, "You hit the shade (3)."
///   ([`Tone::Info`]) or "The shade hits you (2)." ([`Tone::Danger`]);
/// - a killing blow: the same sentence with "kill", which names the victim,
///   "You kill the shade (4)." ([`Tone::Success`]) or "The shade kills you
///   (5)." ([`Tone::Danger`]); its [`Event::Died`] adds nothing more;
/// - a death without a killing blow in the same events: "The shade dies."
///   or "You die.";
/// - nothing for moves and waits, which the map already shows, nor for a
///   move refused by a wall: no time passes and the player sees they did not
///   move, while a line per bump would push the blows out of the five
///   visible lines;
/// - nothing yet for sigils (casts, their effects and backlashes, glyphs
///   learnt, lack of essence), whose lines come with the tracing screen.
pub fn describe(events: &[Event], game: &Game, content: &Content) -> Vec<Message> {
    let mut killed = HashSet::new();
    let mut messages = Vec::new();
    for event in events {
        let message = match event {
            Event::Attacked {
                attacker,
                target,
                damage,
                hp_left,
            } => {
                let lethal = *hp_left <= 0;
                if lethal {
                    killed.insert(*target);
                }
                let attacker = combatant(*attacker, events, game);
                let target = combatant(*target, events, game);
                attacker
                    .zip(target)
                    .map(|(attacker, target)| blow(&attacker, &target, *damage, lethal, content))
            }
            Event::Died { actor, who, .. } if !killed.contains(actor) => Some(death(who, content)),
            Event::Died { .. }
            | Event::Moved { .. }
            | Event::Waited { .. }
            | Event::MoveBlocked { .. }
            | Event::SigilCast { .. }
            | Event::NotEnoughEssence { .. }
            | Event::DirectionMissing { .. }
            | Event::SpellMissed { .. }
            | Event::Wounded { .. }
            | Event::Healed { .. }
            | Event::Held { .. }
            | Event::TurnLost { .. }
            | Event::Blinked { .. }
            | Event::Backlash { .. }
            | Event::EssenceDrained { .. }
            | Event::GlyphLearned { .. } => None,
        };
        messages.extend(message);
    }
    messages
}

/// Returns the sentence of the end screen that says who killed the player
/// and when: "Killed by an acolyte on turn 1283."
///
/// `turn` is the player's turn the death happened on, counted from 1 as on
/// the status line. The player cannot kill itself today; should it happen,
/// the sentence says so instead of naming a monster.
pub fn cause_of_death(killed_by: &Combatant, turn: u64, content: &Content) -> String {
    match killed_by {
        Combatant::Player => format!("You fell by your own hand on turn {turn}."),
        Combatant::Monster { definition } => {
            let name = monster_name(content, definition);
            format!(
                "Killed by {} {name} on turn {turn}.",
                indefinite_article(name)
            )
        }
    }
}

/// Returns the command that replays `seed`, as the end screen suggests it:
/// "Replay this seed: sigilfall --seed 7F3A-91C2-0B5E-D4A8".
pub fn replay_hint(seed: Seed) -> String {
    format!("Replay this seed: {} --seed {seed}", env!("CARGO_BIN_NAME"))
}

/// Returns the last line of the "in view" list when `hidden` monsters do
/// not fit in the side panel: "and 3 more".
pub fn more_in_view(hidden: usize) -> String {
    format!("and {hidden} more")
}

/// Returns the name shown to the player for the monster definition
/// `definition` of `content`, such as "ossuary rat".
///
/// A definition missing from `content` (only possible when a game was built
/// from other content than the one given here) is shown by its identifier,
/// so the screen still says something.
pub fn monster_name<'name>(content: &'name Content, definition: &'name str) -> &'name str {
    content
        .monster(definition)
        .map_or(definition, |monster| monster.name())
}

/// Returns who the actor `id` is: the player, a monster still in `game`, or
/// the one an [`Event::Died`] of `events` describes. `None` only if none of
/// these knows it, which the guarantees of `Game::submit` rule out.
fn combatant(id: ActorId, events: &[Event], game: &Game) -> Option<Combatant> {
    if id == game.player() {
        return Some(Combatant::Player);
    }
    if let Some(monster) = game.actor(id).and_then(|actor| actor.as_monster()) {
        return Some(Combatant::Monster {
            definition: monster.definition().to_owned(),
        });
    }
    events.iter().find_map(|event| match event {
        Event::Died { actor, who, .. } if *actor == id => Some(who.clone()),
        _ => None,
    })
}

/// The message of a blow of `damage` hit points struck by `attacker` on
/// `target`, `lethal` when it killed. See [`describe`] for the wording and
/// the tones.
fn blow(
    attacker: &Combatant,
    target: &Combatant,
    damage: i32,
    lethal: bool,
    content: &Content,
) -> Message {
    let verb = match (lethal, attacker) {
        (false, Combatant::Player) => "hit",
        (false, Combatant::Monster { .. }) => "hits",
        (true, Combatant::Player) => "kill",
        (true, Combatant::Monster { .. }) => "kills",
    };
    let tone = match (target, lethal) {
        (Combatant::Player, _) => Tone::Danger,
        (Combatant::Monster { .. }, true) => Tone::Success,
        (Combatant::Monster { .. }, false) => Tone::Info,
    };
    let text = format!(
        "{} {verb} {} ({damage}).",
        subject(attacker, content),
        object(target, content)
    );
    Message::new(tone, text)
}

/// The message of the death of `who` when no killing blow announced it.
fn death(who: &Combatant, content: &Content) -> Message {
    match who {
        Combatant::Player => Message::new(Tone::Danger, "You die."),
        Combatant::Monster { .. } => {
            Message::new(Tone::Success, format!("{} dies.", subject(who, content)))
        }
    }
}

/// `who` at the start of a sentence: "You" or "The shade".
fn subject(who: &Combatant, content: &Content) -> String {
    match who {
        Combatant::Player => "You".to_owned(),
        Combatant::Monster { definition } => format!("The {}", monster_name(content, definition)),
    }
}

/// `who` after a verb: "you" or "the shade".
fn object(who: &Combatant, content: &Content) -> String {
    match who {
        Combatant::Player => "you".to_owned(),
        Combatant::Monster { definition } => format!("the {}", monster_name(content, definition)),
    }
}

/// "an" before a `name` that starts with a vowel letter, "a" otherwise.
///
/// Spelling decides, not sound: right for every monster of the game
/// ("an acolyte", "a hierophant"), wrong for a name such as "unicorn" or
/// "hour", which the content does not have.
fn indefinite_article(name: &str) -> &'static str {
    let starts_with_vowel = name
        .chars()
        .next()
        .is_some_and(|first| "aeiouAEIOU".contains(first));
    if starts_with_vowel { "an" } else { "a" }
}

#[cfg(test)]
mod tests {
    use sigilfall_core::action::Action;
    use sigilfall_core::geometry::Direction;

    use super::*;

    /// Seed of the test games; the fixed room does not depend on it.
    const SEED: Seed = Seed::new(42);

    /// The game's own monsters.
    fn content() -> Content {
        Content::load().expect("the shipped content is valid")
    }

    /// A fixed-room game with a monster of `definition` east of the player.
    fn duel(definition: &str) -> (Game, Content) {
        let content = content();
        let mut game = Game::in_fixed_room(SEED);
        let east = game.player_actor().position().step(Direction::East);
        game.spawn_monster(east, content.monster(definition).unwrap());
        (game, content)
    }

    /// The texts of `messages`, for comparisons.
    fn texts(messages: &[Message]) -> Vec<&str> {
        messages.iter().map(|m| m.text.as_str()).collect()
    }

    #[test]
    fn a_blow_on_a_monster_gives_the_damage_and_a_killing_blow_names_it() {
        // The hierophant (20 hit points) survives a first blow; striking it
        // again and again ends with a kill, bounded by its hit points.
        let (mut game, content) = duel("hierophant");
        let first = game.submit(Action::Move(Direction::East));
        let Some(Event::Attacked { damage, .. }) = first.first() else {
            panic!("the move struck: {first:?}");
        };

        let messages = describe(&first, &game, &content);

        assert_eq!(messages[0].tone, Tone::Info);
        assert_eq!(
            messages[0].text,
            format!("You hit the hierophant ({damage}).")
        );

        for _ in 0..20 {
            let events = game.submit(Action::Move(Direction::East));
            let messages = describe(&events, &game, &content);
            if let Some(Event::Attacked {
                hp_left: 0, damage, ..
            }) = events.first()
            {
                assert_eq!(messages[0].tone, Tone::Success);
                assert_eq!(
                    messages[0].text,
                    format!("You kill the hierophant ({damage}).")
                );
                assert!(
                    texts(&messages).iter().all(|t| !t.contains("dies")),
                    "the death is not told twice: {messages:?}"
                );
                return;
            }
        }
        panic!("twenty blows of at least one point kill 20 hit points");
    }

    #[test]
    fn a_monster_blow_on_the_player_is_a_danger_with_its_damage() {
        let (mut game, content) = duel("ossuary_rat");

        // The rat starts with no energy: waiting lets it strike.
        let events: Vec<_> = (0..3).flat_map(|_| game.submit(Action::Wait)).collect();
        let messages = describe(&events, &game, &content);

        let blows: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                Event::Attacked { damage, .. } => Some(*damage),
                _ => None,
            })
            .collect();
        assert!(!blows.is_empty(), "the rat struck: {events:?}");
        let expected: Vec<_> = blows
            .iter()
            .map(|damage| format!("The ossuary rat hits you ({damage})."))
            .collect();
        assert_eq!(texts(&messages), expected);
        assert!(messages.iter().all(|m| m.tone == Tone::Danger));
    }

    /// A monster that kills the player in one blow: its attack of 99
    /// leaves at least 97 damage against the player's 30 hit points.
    const EXECUTIONER: &str = r#"[
        Monster(id: "executioner", name: "executioner", family: Acolyte, elite: true,
            speed: 100, hp: 999, attack: 99, defense: 0, min_depth: 1),
    ]"#;

    #[test]
    fn the_killing_blow_on_the_player_names_the_killer_once() {
        let content = Content::parse(EXECUTIONER).unwrap();
        let mut game = Game::in_fixed_room(SEED);
        let east = game.player_actor().position().step(Direction::East);
        game.spawn_monster(east, content.monster("executioner").unwrap());

        // The monster starts with no energy; a few waits are enough.
        let mut events = Vec::new();
        for _ in 0..3 {
            events.extend(game.submit(Action::Wait));
        }
        assert!(game.is_over(), "{events:?}");
        let Some(Event::Attacked { damage, .. }) = events
            .iter()
            .find(|event| matches!(event, Event::Attacked { .. }))
        else {
            panic!("the executioner struck: {events:?}");
        };

        let messages = describe(&events, &game, &content);

        assert_eq!(
            texts(&messages),
            [format!("The executioner kills you ({damage}).")]
        );
        assert_eq!(messages[0].tone, Tone::Danger);
    }

    #[test]
    fn moves_waits_and_bumps_into_walls_write_nothing() {
        let mut game = Game::in_fixed_room(SEED);
        let content = content();
        let mut events = game.submit(Action::Move(Direction::East));
        events.extend(game.submit(Action::Wait));
        for _ in 0..10 {
            events.extend(game.submit(Action::Move(Direction::North)));
        }
        assert!(
            events
                .iter()
                .any(|event| matches!(event, Event::MoveBlocked { .. })),
            "the walk ended against the wall: {events:?}"
        );

        assert_eq!(describe(&events, &game, &content), []);
    }

    #[test]
    fn the_cause_of_death_names_the_killer_with_its_article() {
        let content = content();
        let by = |definition: &str| Combatant::Monster {
            definition: definition.to_owned(),
        };

        assert_eq!(
            cause_of_death(&by("acolyte"), 1283, &content),
            "Killed by an acolyte on turn 1283."
        );
        assert_eq!(
            cause_of_death(&by("ossuary_rat"), 7, &content),
            "Killed by an ossuary rat on turn 7."
        );
        assert_eq!(
            cause_of_death(&by("hierophant"), 12, &content),
            "Killed by a hierophant on turn 12."
        );
    }

    #[test]
    fn an_unknown_definition_is_shown_by_its_identifier() {
        assert_eq!(monster_name(&content(), "mystery_beast"), "mystery_beast");
        assert_eq!(monster_name(&content(), "wax_beetle"), "wax beetle");
    }

    #[test]
    fn the_replay_hint_gives_the_command_with_the_shareable_seed() {
        assert_eq!(
            replay_hint(Seed::new(0x7F3A_91C2_0B5E_D4A8)),
            "Replay this seed: sigilfall --seed 7F3A-91C2-0B5E-D4A8"
        );
    }

    #[test]
    fn a_monster_dying_without_a_blow_is_still_told() {
        let events = [Event::Died {
            actor: ActorId::default(),
            who: Combatant::Monster {
                definition: "shade".to_owned(),
            },
            killed_by: Combatant::Player,
        }];
        let game = Game::in_fixed_room(SEED);

        let messages = describe(&events, &game, &content());

        assert_eq!(texts(&messages), ["The shade dies."]);
        assert_eq!(messages[0].tone, Tone::Success);
    }
}
