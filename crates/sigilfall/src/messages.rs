//! Every English sentence and label of the message log, the side panel, the
//! sigil overlay and the end screen, and the translation of game events into
//! log messages.
//!
//! Exposes [`describe`], which turns the events of one `Game::submit` into
//! [`Message`]s, the opening message ([`welcome`]), the cause of death of
//! the end screen ([`cause_of_death`]), monster and sigil names
//! ([`monster_name`], [`sigil_name`]), what a sigil does in a few words
//! ([`effect_summary`]), the time a cast takes ([`cast_time`]), the notices
//! of the sigil overlay and the fixed labels of those screens and of the key
//! help. Keeping the words here, away from the drawing code, is what
//! decision D10 asks: a translation would only touch this file. The turn and
//! seed labels of the status line predate this module and keep their words
//! in `ui::game`.
//!
//! The game itself never writes text (decision D4): it reports events, and
//! this module chooses what the player reads about them.

use std::collections::HashSet;

use sigilfall_core::actor::ActorId;
use sigilfall_core::combat::Combatant;
use sigilfall_core::content::{Content, Effect, SigilDef, Targeting};
use sigilfall_core::event::Event;
use sigilfall_core::game::Game;
use sigilfall_core::rng::Seed;
use sigilfall_core::schedule::{MOVE_COST, STROKE_COST, cast_cost};
use sigilfall_core::sigil::{CastQuality, Node};

use crate::message_log::{Message, Tone};

/// Title of the side panel's health block.
pub const HP_LABEL: &str = "HP";

/// Mark written before a value that warns of a danger, so the warning reads
/// without colour: health below a quarter of the maximum (`docs/design.md`,
/// "PV"), essence too low to cast, a node the trace refused.
pub const WARNING_MARK: &str = "!";

/// Label of the player's essence, in the side panel and the sigil overlay.
pub const ESSENCE_LABEL: &str = "Essence";

/// Title of the side panel's list of known sigils.
pub const SIGILS_TITLE: &str = "SIGILS";

/// What the side panel's list of known sigils says while the grimoire is
/// empty.
pub const NO_SIGIL_YET: &str = "none yet";

/// Title of the side panel's list of what the player sees.
pub const IN_VIEW_TITLE: &str = "IN VIEW";

/// Name of the player in the "in view" list, after the `@`.
pub const PLAYER_IN_VIEW: &str = "you";

/// Name of an engraved glyph not read yet, in the "in view" list.
pub const UNREAD_GLYPH_IN_VIEW: &str = "carved glyph";

/// Name of an engraved glyph already read, in the "in view" list.
pub const READ_GLYPH_IN_VIEW: &str = "known glyph";

/// Title of the separator above the message log.
pub const LOG_TITLE: &str = "Log";

/// Title of the sigil overlay while the player traces.
pub const TRACE_TITLE: &str = "Trace a sigil";

/// Title of the sigil overlay once an unstable cast has been made.
pub const UNSTABLE_TITLE: &str = "Unstable trace";

/// Title of the sigil overlay once a trace has fizzled.
pub const FAILED_TITLE: &str = "Failed trace";

/// Title of the grimoire, in the sigil overlay.
pub const GRIMOIRE_TITLE: &str = "GRIMOIRE";

/// The lines the grimoire shows while it is empty: how sigils are learnt.
/// `{glyph}` stands for the map glyph of an unread engraving.
pub const EMPTY_GRIMOIRE: [&str; 4] = [
    "No sigil known yet.",
    "Step on a glyph carved",
    "in the floor, {glyph} on the",
    "map, to learn its sigil.",
];

/// Label before the nodes linked so far, in the sigil overlay.
pub const TRACE_LABEL: &str = "Trace";

/// Label before the time the trace takes, in the sigil overlay.
pub const TIME_LABEL: &str = "Time";

/// Label before the pattern a near miss was meant to be, on the verdict of
/// an unstable cast.
pub const EXPECTED_LABEL: &str = "expected";

/// Label before the nodes the player linked, on a verdict.
pub const TRACED_LABEL: &str = "traced";

/// Verdict word of an unstable cast, as the mock-up writes it.
pub const UNSTABLE_WORD: &str = "UNSTABLE";

/// Verdict word of a fizzled cast, as the mock-up writes it.
pub const FAILED_WORD: &str = "FAILED";

/// How far an unstable trace was from its sigil: decision D7 makes it
/// always one edit.
pub const ONE_NODE_OFF: &str = "one node off";

/// Why a fizzled trace cast nothing.
pub const NO_KNOWN_PATTERN: &str = "no known pattern";

/// Last line of the verdict of an unstable cast.
pub const UNSTABLE_OUTCOME: &str = "The magic lashes back.";

/// Last line of the verdict of a fizzled cast.
pub const FAILED_OUTCOME: &str = "The spell fizzles.";

/// Notice of the sigil overlay when the player validates an empty trace,
/// which is never cast.
pub const EMPTY_TRACE: &str = "Link at least one node first.";

/// Title of the end screen, letter-spaced as in the death summary of
/// `docs/design.md`.
pub const DEATH_TITLE: &str = "T H E   D E S C E N T   E N D S";

/// Line under the end screen's title.
pub const DEATH_SUBTITLE: &str = "The walls will keep your trace.";

/// Label before the seed on the end screen.
pub const SEED_LABEL: &str = "Seed";

/// Key that leaves the game, as the key help shows it.
pub const QUIT_KEY: &str = "q";

/// What [`QUIT_KEY`] does, as the key help shows it.
pub const QUIT_LABEL: &str = "quit";

/// What the movement keys do, in the key help of the game screen.
pub const MOVE_LABEL: &str = "move";

/// Key that waits, in the key help of the game screen.
pub const WAIT_KEY: &str = ".";

/// What [`WAIT_KEY`] does.
pub const WAIT_LABEL: &str = "wait";

/// Key that opens the sigil overlay, in the key help of the game screen.
pub const TRACE_KEY: &str = "t";

/// What [`TRACE_KEY`] does.
pub const TRACE_KEY_LABEL: &str = "trace";

/// Keys that link a node, in the key help of the sigil overlay.
pub const LINK_KEYS: &str = "1-9";

/// What [`LINK_KEYS`] do.
pub const LINK_LABEL: &str = "link a node";

/// Key that removes the last node, in the key help of the sigil overlay.
pub const UNDO_KEY: &str = "Backspace";

/// What [`UNDO_KEY`] does.
pub const UNDO_LABEL: &str = "undo";

/// Key that validates, in the key help of the sigil overlay.
pub const CAST_KEY: &str = "Enter";

/// What [`CAST_KEY`] does while tracing.
pub const CAST_LABEL: &str = "cast";

/// What [`CAST_KEY`] does on a verdict.
pub const CONTINUE_LABEL: &str = "continue";

/// Key that cancels the trace, or goes back to it from the direction
/// prompt, in the key help of the sigil overlay.
pub const CANCEL_KEY: &str = "Esc";

/// What [`CANCEL_KEY`] does while tracing.
pub const CANCEL_LABEL: &str = "cancel";

/// What [`CANCEL_KEY`] does while a direction is asked.
pub const BACK_LABEL: &str = "back to the trace";

/// What the direction keys do while a direction is asked.
pub const AIM_LABEL: &str = "aim";

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
/// [`Event::Died`], which the events always carry. Monster and sigil names
/// come from `content`.
///
/// What is written:
/// - a blow: who struck whom and the damage, "You hit the shade (3)."
///   ([`Tone::Info`]) or "The shade hits you (2)." ([`Tone::Danger`]);
/// - a killing blow: the same sentence with "kill", which names the victim,
///   "You kill the shade (4)." ([`Tone::Success`]) or "The shade kills you
///   (5)." ([`Tone::Danger`]); its [`Event::Died`] adds nothing more;
/// - a death without a killing blow in the same events: "The shade dies."
///   or "You die.";
/// - a cast: one [`Tone::Sigil`] line naming the sigil, the quality of the
///   trace and what the effect did, "Ember, pure trace: the shade is seared
///   (6)."; a spell that kills says "destroyed" and its [`Event::Died`] adds
///   nothing more;
/// - a fizzle: "The trace matches no sigil you know and fizzles."
///   ([`Tone::Sigil`]);
/// - a backlash, on its own [`Tone::Danger`] line after the cast, with what
///   it cost: "The backlash scorches you (2).", "The backlash drains your
///   essence (4).", "The backlash dazes you: you lose 1 turn.";
/// - a glyph learnt: "You decipher a carved glyph: Recall joins your
///   grimoire." ([`Tone::Success`]);
/// - a refused cast, which took no time: "Not enough essence to cast: 1 of
///   2." or "Passage needs a direction." ([`Tone::Info`]);
/// - nothing for moves and waits, which the map already shows, nor for a
///   move refused by a wall: no time passes and the player sees they did not
///   move, while a line per bump would push the blows out of the five
///   visible lines; nothing either for a turn lost while held, which the
///   daze line has already announced.
pub fn describe(events: &[Event], game: &Game, content: &Content) -> Vec<Message> {
    let mut killed = HashSet::new();
    let mut messages = Vec::new();
    // Opening of the line of the cast in progress, "Ember, pure trace",
    // written once the event of its effect follows.
    let mut cast_lead: Option<String> = None;
    for event in events {
        if !is_spell_effect(event, game)
            && let Some(lead) = cast_lead.take()
        {
            messages.push(Message::new(Tone::Sigil, format!("{lead}.")));
        }
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
            Event::SigilCast {
                sigil: Some(id),
                quality,
            } => {
                cast_lead = Some(format!(
                    "{}, {}",
                    sigil_name(content, id),
                    quality_words(*quality)
                ));
                None
            }
            Event::SigilCast { sigil: None, .. } => Some(Message::new(
                Tone::Sigil,
                "The trace matches no sigil you know and fizzles.",
            )),
            Event::Wounded {
                actor,
                damage,
                hp_left,
            } => {
                if *actor == game.player() {
                    Some(Message::new(
                        Tone::Danger,
                        format!("The backlash scorches you ({damage})."),
                    ))
                } else {
                    let lethal = *hp_left <= 0;
                    if lethal {
                        killed.insert(*actor);
                    }
                    let verb = if lethal { "destroyed" } else { "seared" };
                    combatant(*actor, events, game).map(|target| {
                        let effect = format!("{} is {verb} ({damage})", object(&target, content));
                        spell_line(cast_lead.take(), &effect)
                    })
                }
            }
            Event::Healed { amount, .. } => {
                let effect = if *amount > 0 {
                    format!("you mend ({amount})")
                } else {
                    "you were not hurt".to_owned()
                };
                Some(spell_line(cast_lead.take(), &effect))
            }
            Event::Held { actor, turns } if *actor == game.player() => Some(Message::new(
                Tone::Danger,
                format!("The backlash dazes you: you lose {}.", turn_count(*turns)),
            )),
            Event::Held { actor, turns } => combatant(*actor, events, game).map(|target| {
                let effect = format!(
                    "{} is held ({})",
                    object(&target, content),
                    turn_count(*turns)
                );
                spell_line(cast_lead.take(), &effect)
            }),
            Event::Blinked { from, to, .. } => {
                let cells = from.x.abs_diff(to.x).max(from.y.abs_diff(to.y));
                let effect = match cells {
                    0 => "the way is blocked".to_owned(),
                    1 => "you blink 1 cell".to_owned(),
                    cells => format!("you blink {cells} cells"),
                };
                Some(spell_line(cast_lead.take(), &effect))
            }
            Event::SpellMissed { .. } => Some(spell_line(cast_lead.take(), "no foe in reach")),
            Event::EssenceDrained { lost, .. } => Some(Message::new(
                Tone::Danger,
                format!("The backlash drains your essence ({lost})."),
            )),
            Event::NotEnoughEssence { essence, cost } => Some(Message::new(
                Tone::Info,
                format!("Not enough essence to cast: {essence} of {cost}."),
            )),
            Event::DirectionMissing { sigil } => Some(Message::new(
                Tone::Info,
                format!("{} needs a direction.", sigil_name(content, sigil)),
            )),
            Event::GlyphLearned { sigil, .. } => Some(Message::new(
                Tone::Success,
                format!(
                    "You decipher a carved glyph: {} joins your grimoire.",
                    sigil_name(content, sigil)
                ),
            )),
            Event::Died { .. }
            | Event::Moved { .. }
            | Event::Waited { .. }
            | Event::MoveBlocked { .. }
            | Event::TurnLost { .. }
            | Event::Backlash { .. }
            | Event::PickedUp { .. }
            | Event::InventoryFull { .. }
            | Event::NothingToPickUp { .. }
            | Event::Used { .. }
            | Event::Dropped { .. }
            | Event::ItemRefused { .. }
            | Event::EssenceRestored { .. }
            | Event::SigilLearned { .. }
            | Event::Steadied { .. }
            | Event::CastSteadied { .. } => None,
        };
        messages.extend(message);
    }
    if let Some(lead) = cast_lead {
        messages.push(Message::new(Tone::Sigil, format!("{lead}.")));
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

/// Returns the last line of a list of the side panel or the grimoire when
/// `hidden` entries do not fit: "and 3 more".
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

/// Returns the name shown to the player for the sigil identified by `id`
/// in `content`, such as "Ember"; an identifier missing from `content` is
/// shown as is, as [`monster_name`] does.
pub fn sigil_name<'name>(content: &'name Content, id: &'name str) -> &'name str {
    content.sigil(id).map_or(id, SigilDef::name)
}

/// Returns what `sigil` does in a few words, for the grimoire: its effect
/// and power, then whom it reaches, joined by `separator`, such as
/// "harm 6 · nearest" or "blink 4 · aimed".
///
/// The power's unit follows the effect: hit points to harm or heal, turns
/// to hold, cells to blink. "nearest" is the nearest foe in sight,
/// "aimed" a direction asked after the trace, "self" the player.
pub fn effect_summary(sigil: &SigilDef, separator: char) -> String {
    let verb = match sigil.effect() {
        Effect::Damage => "harm",
        Effect::Heal => "heal",
        Effect::Blink => "blink",
        Effect::Hold => "hold",
    };
    let target = match sigil.targeting() {
        Targeting::Caster => "self",
        Targeting::Direction => "aimed",
        Targeting::NearestEnemy => "nearest",
    };
    format!("{verb} {} {separator} {target}", sigil.power())
}

/// Returns the nodes `nodes` as the player presses them, joined by
/// `separator`: "7·5·3". An empty list gives an empty string.
pub fn pattern_text(nodes: &[Node], separator: char) -> String {
    nodes
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(&separator.to_string())
}

/// Returns the time a cast of a trace of `nodes` nodes takes, in turns of
/// the player (one turn being the time of a step), then what each further
/// node adds, joined by `separator`: "1.25 turns · +0.25 a node".
///
/// The cost is `schedule::cast_cost`, the energy `Game::submit` charges for
/// the cast; at the player's speed, a step's worth of energy is one turn.
pub fn cast_time(nodes: usize, separator: char) -> String {
    format!(
        "{} turns {separator} +{} a node",
        turns(cast_cost(nodes)),
        turns(STROKE_COST)
    )
}

/// Returns the notice of the sigil overlay when the player presses the key
/// of `node` while the trace already links it: "Node 5 is already linked."
pub fn node_refused(node: Node) -> String {
    format!("Node {node} is already linked.")
}

/// Returns the notice of the sigil overlay while the player has less
/// essence than a cast costs: "Not enough essence: 1 of 2."
pub fn essence_too_low(essence: i32, cost: i32) -> String {
    format!("Not enough essence: {essence} of {cost}.")
}

/// Returns the title of the sigil overlay while a direction is asked for
/// the sigil named `name`: "Aim Passage".
pub fn aim_title(name: &str) -> String {
    format!("Aim {name}")
}

/// Returns the notice of the sigil overlay while a direction is asked for
/// the sigil named `name`: "Passage is aimed: choose a direction."
pub fn aim_prompt(name: &str) -> String {
    format!("{name} is aimed: choose a direction.")
}

/// `energy` written in turns of the player, with two decimals: 125 is
/// "1.25", one step being [`MOVE_COST`].
fn turns(energy: i32) -> String {
    let hundredths = energy * 100 / MOVE_COST;
    format!("{}.{:02}", hundredths / 100, hundredths % 100)
}

/// How a trace of `quality` is named in the line of its cast.
const fn quality_words(quality: CastQuality) -> &'static str {
    match quality {
        CastQuality::Pure => "pure trace",
        CastQuality::Unstable => "unstable trace",
        CastQuality::Fizzled => "fizzled trace",
    }
}

/// "1 turn" or "3 turns".
fn turn_count(turns: u32) -> String {
    if turns == 1 {
        "1 turn".to_owned()
    } else {
        format!("{turns} turns")
    }
}

/// Whether `event` is the effect of a spell that ends the line of its cast
/// (see [`describe`]): a backlash on the player is not.
fn is_spell_effect(event: &Event, game: &Game) -> bool {
    match event {
        Event::Wounded { actor, .. } | Event::Held { actor, .. } => *actor != game.player(),
        Event::Healed { .. } | Event::Blinked { .. } | Event::SpellMissed { .. } => true,
        _ => false,
    }
}

/// The [`Tone::Sigil`] line of a cast: `lead` ("Ember, pure trace"), then
/// `effect` ("the shade is seared (6)"). Without a lead, which the order of
/// the events rules out, the effect alone makes the sentence.
fn spell_line(lead: Option<String>, effect: &str) -> Message {
    let text = match lead {
        Some(lead) => format!("{lead}: {effect}."),
        None => {
            let mut chars = effect.chars();
            let first = chars.next().map(|c| c.to_uppercase().to_string());
            format!("{}{}.", first.unwrap_or_default(), chars.as_str())
        }
    };
    Message::new(Tone::Sigil, text)
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
    use sigilfall_core::geometry::{Direction, Position};
    use sigilfall_core::sigil::Backlash;

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

    /// A fixed-room game with an ossuary rat three cells east of the
    /// player, its key, and the shipped content.
    fn spell_target() -> (Game, ActorId, Content) {
        let content = content();
        let mut game = Game::in_fixed_room(SEED);
        let cell = Position::new(8, 5);
        let rat = game.spawn_monster(cell, content.monster("ossuary_rat").unwrap());
        (game, rat, content)
    }

    /// The event of a cast of the sigil `id` with `quality`.
    fn cast(id: &str, quality: CastQuality) -> Event {
        Event::SigilCast {
            sigil: Some(id.to_owned()),
            quality,
        }
    }

    /// The tone and text of each of `messages`.
    fn lines(messages: &[Message]) -> Vec<(Tone, &str)> {
        messages.iter().map(|m| (m.tone, m.text.as_str())).collect()
    }

    #[test]
    fn a_cast_and_its_effect_make_one_sigil_line() {
        let (game, rat, content) = spell_target();
        let player = game.player();
        let cases = [
            (
                vec![
                    cast("ember", CastQuality::Pure),
                    Event::Wounded {
                        actor: rat,
                        damage: 6,
                        hp_left: 2,
                    },
                ],
                "Ember, pure trace: the ossuary rat is seared (6).",
            ),
            (
                vec![
                    cast("recall", CastQuality::Pure),
                    Event::Healed {
                        actor: player,
                        amount: 8,
                        hp: 30,
                    },
                ],
                "Recall, pure trace: you mend (8).",
            ),
            (
                vec![
                    cast("recall", CastQuality::Pure),
                    Event::Healed {
                        actor: player,
                        amount: 0,
                        hp: 30,
                    },
                ],
                "Recall, pure trace: you were not hurt.",
            ),
            (
                vec![
                    cast("veil", CastQuality::Pure),
                    Event::Held {
                        actor: rat,
                        turns: 2,
                    },
                ],
                "Veil, pure trace: the ossuary rat is held (2 turns).",
            ),
            (
                vec![
                    cast("passage", CastQuality::Pure),
                    Event::Blinked {
                        actor: player,
                        from: Position::new(5, 5),
                        to: Position::new(9, 5),
                    },
                ],
                "Passage, pure trace: you blink 4 cells.",
            ),
            (
                vec![
                    cast("passage", CastQuality::Pure),
                    Event::Blinked {
                        actor: player,
                        from: Position::new(1, 5),
                        to: Position::new(1, 5),
                    },
                ],
                "Passage, pure trace: the way is blocked.",
            ),
            (
                vec![
                    cast("lance", CastQuality::Pure),
                    Event::SpellMissed {
                        sigil: "lance".to_owned(),
                    },
                ],
                "Lance, pure trace: no foe in reach.",
            ),
        ];

        for (events, expected) in cases {
            let messages = describe(&events, &game, &content);
            assert_eq!(lines(&messages), [(Tone::Sigil, expected)], "{events:?}");
        }
    }

    #[test]
    fn a_spell_that_kills_says_so_once() {
        let (game, rat, content) = spell_target();
        let events = [
            cast("ember", CastQuality::Pure),
            Event::Wounded {
                actor: rat,
                damage: 6,
                hp_left: 0,
            },
            Event::Died {
                actor: rat,
                who: Combatant::Monster {
                    definition: "ossuary_rat".to_owned(),
                },
                killed_by: Combatant::Player,
            },
        ];

        let messages = describe(&events, &game, &content);

        assert_eq!(
            texts(&messages),
            ["Ember, pure trace: the ossuary rat is destroyed (6)."]
        );
    }

    #[test]
    fn an_unstable_cast_is_followed_by_its_backlash_as_a_danger() {
        let (game, rat, content) = spell_target();
        let player = game.player();
        let backlashes = [
            (
                Backlash::Scorch,
                Event::Wounded {
                    actor: player,
                    damage: 2,
                    hp_left: 28,
                },
                "The backlash scorches you (2).",
            ),
            (
                Backlash::Drain,
                Event::EssenceDrained {
                    actor: player,
                    lost: 4,
                },
                "The backlash drains your essence (4).",
            ),
            (
                Backlash::Daze,
                Event::Held {
                    actor: player,
                    turns: 1,
                },
                "The backlash dazes you: you lose 1 turn.",
            ),
        ];

        for (backlash, consequence, expected) in backlashes {
            let events = [
                cast("ember", CastQuality::Unstable),
                Event::Wounded {
                    actor: rat,
                    damage: 6,
                    hp_left: 2,
                },
                Event::Backlash {
                    caster: player,
                    backlash,
                },
                consequence,
            ];
            let messages = describe(&events, &game, &content);
            assert_eq!(
                lines(&messages),
                [
                    (
                        Tone::Sigil,
                        "Ember, unstable trace: the ossuary rat is seared (6)."
                    ),
                    (Tone::Danger, expected),
                ],
                "{backlash:?}"
            );
        }
    }

    #[test]
    fn fizzles_refusals_and_glyphs_have_their_own_lines() {
        let (game, _, content) = spell_target();
        let events = [
            Event::SigilCast {
                sigil: None,
                quality: CastQuality::Fizzled,
            },
            Event::NotEnoughEssence {
                essence: 1,
                cost: 2,
            },
            Event::DirectionMissing {
                sigil: "passage".to_owned(),
            },
            Event::GlyphLearned {
                sigil: "recall".to_owned(),
                position: Position::new(6, 5),
            },
        ];

        let messages = describe(&events, &game, &content);

        assert_eq!(
            lines(&messages),
            [
                (
                    Tone::Sigil,
                    "The trace matches no sigil you know and fizzles."
                ),
                (Tone::Info, "Not enough essence to cast: 1 of 2."),
                (Tone::Info, "Passage needs a direction."),
                (
                    Tone::Success,
                    "You decipher a carved glyph: Recall joins your grimoire."
                ),
            ]
        );
    }

    #[test]
    fn a_cast_without_an_effect_event_is_still_told() {
        let (game, _, content) = spell_target();

        let messages = describe(&[cast("ember", CastQuality::Pure)], &game, &content);

        assert_eq!(lines(&messages), [(Tone::Sigil, "Ember, pure trace.")]);
    }

    #[test]
    fn the_overlay_texts_follow_the_glyph_set() {
        let ember = content().sigil("ember").unwrap().clone();

        assert_eq!(
            pattern_text(ember.pattern().nodes(), '\u{b7}'),
            "7\u{b7}5\u{b7}3"
        );
        assert_eq!(pattern_text(ember.pattern().nodes(), '-'), "7-5-3");
        assert_eq!(pattern_text(&[], '-'), "");
        assert_eq!(effect_summary(&ember, '-'), "harm 6 - nearest");
        assert_eq!(cast_time(3, '-'), "1.25 turns - +0.25 a node");
        assert_eq!(cast_time(0, '-'), "0.50 turns - +0.25 a node");
        assert_eq!(cast_time(9, '-'), "2.75 turns - +0.25 a node");
    }

    #[test]
    fn every_shipped_sigil_has_a_short_effect_summary() {
        let summaries: Vec<String> = content()
            .sigils()
            .iter()
            .map(|sigil| effect_summary(sigil, '-'))
            .collect();

        assert_eq!(
            summaries,
            [
                "harm 6 - nearest",
                "heal 8 - self",
                "hold 2 - nearest",
                "blink 4 - aimed",
                "harm 9 - aimed",
            ]
        );
    }
}
