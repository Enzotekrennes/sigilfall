//! Every English sentence and label of the message log, the side panel, the
//! sigil overlay, the inventory and the end screen, and the translation of
//! game events into log messages.
//!
//! Exposes [`describe`], which turns the events of one `Game::submit` into
//! [`Message`]s, the opening messages ([`welcome`], [`welcome_back`],
//! [`save_set_aside`]), the save notice [`save_not_deleted`], the words of
//! the death summary (its [`epitaph`], [`cause_of_death`], the labels of its
//! counts and [`cast_breakdown`]), monster, sigil and item names
//! ([`monster_name`], [`sigil_name`], [`item_name`]), what a sigil or an
//! item does in a few words ([`effect_summary`], [`item_summary`]), the time
//! a cast takes ([`cast_time`]), why the game refused an item
//! ([`item_refusal`]), the notices of the sigil overlay and the inventory,
//! and the fixed labels of those screens and of the key help. Keeping the words here, away from the drawing code, is what
//! decision D10 asks: a translation would only touch this file. The depth,
//! turn and seed labels of the status line keep their words in `ui::game`,
//! beside the turn and seed labels that predate this module.
//!
//! The game itself never writes text (decision D4): it reports events, and
//! this module chooses what the player reads about them.

use std::collections::HashSet;

use sigilfall_core::actor::ActorId;
use sigilfall_core::combat::Combatant;
use sigilfall_core::content::{
    Content, Effect, ItemDef, ItemEffect, ItemFamily, SigilDef, Targeting,
};
use sigilfall_core::event::Event;
use sigilfall_core::game::Game;
use sigilfall_core::item::{INVENTORY_CAPACITY, ItemId, ItemRefusal};
use sigilfall_core::rng::Seed;
use sigilfall_core::schedule::{MOVE_COST, STROKE_COST, cast_cost};
use sigilfall_core::sigil::{CastQuality, Node};
use sigilfall_core::stats::CastCounts;

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

/// Name of the stairs down in the "in view" list, after their glyph.
pub const STAIRS_IN_VIEW: &str = "stairs down";

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

/// Lines under the death summary's title, one per run, chosen from the
/// seed by [`epitaph`]: each speaks of the trace the player leaves in the
/// dungeon, the theme of the game. At most 60 characters, so a line fits an
/// 80-column screen with room to spare.
pub const EPITAPHS: [&str; 8] = [
    "The walls will keep your trace.",
    "Your last stroke is still drying on the stone.",
    "The ink outlived the hand that traced it.",
    "Somewhere below, a glyph waits for another reader.",
    "The dark learnt your sigils by heart.",
    "Your essence seeps into the old carvings.",
    "The grimoire closes; its margins remember you.",
    "Dust settles over an unfinished pattern.",
];

/// Label inside the frame around the seed on the death summary.
pub const SEED_LABEL: &str = "SEED";

/// Label of the deepest floor reached, in the death summary's counts.
pub const DEPTH_REACHED_LABEL: &str = "Depth reached";

/// Label of the number of turns played, in the death summary's counts.
pub const TURNS_LABEL: &str = "Turns";

/// Label of the number of creatures killed, in the death summary's counts.
pub const KILLS_LABEL: &str = "Creatures slain";

/// Label of the number of glyphs read that taught a sigil, in the death
/// summary's counts.
pub const GLYPHS_LABEL: &str = "Glyphs learnt";

/// Label of the number of sigils cast, in the death summary's counts.
pub const CASTS_LABEL: &str = "Sigils cast";

/// Label of the number of items used, in the death summary's counts.
pub const ITEMS_USED_LABEL: &str = "Items used";

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

/// Key that picks up the item underfoot, in the key help of the game
/// screen; `,` does the same, as in other roguelikes.
pub const PICK_UP_KEY: &str = "g";

/// What [`PICK_UP_KEY`] does.
pub const PICK_UP_LABEL: &str = "pick up";

/// Key that opens the inventory, in the key help of the game screen.
pub const INVENTORY_KEY: &str = "i";

/// What [`INVENTORY_KEY`] does.
pub const INVENTORY_LABEL: &str = "inventory";

/// Key that goes down the stairs, in the key help of the game screen; the
/// alternative `input::DESCEND_ALTERNATIVE_KEY` does the same, but the
/// 80-column help line has no room left to name it.
pub const DESCEND_KEY: &str = ">";

/// What [`DESCEND_KEY`] does.
pub const DESCEND_LABEL: &str = "descend";

/// Title of the inventory's frame.
pub const INVENTORY_TITLE: &str = "Inventory";

/// Keys that choose an item after the arrows, in the key help of the
/// inventory: the digits of the slots, `0` being the tenth.
pub const SLOT_KEYS: &str = "1-0";

/// What the arrows and [`SLOT_KEYS`] do in the inventory.
pub const CHOOSE_LABEL: &str = "choose";

/// Keys that use the chosen item, in the key help of the inventory.
pub const USE_KEYS: &str = "Enter/u";

/// What [`USE_KEYS`] do.
pub const USE_LABEL: &str = "use";

/// Key that drops the chosen item, in the key help of the inventory.
pub const DROP_KEY: &str = "d";

/// What [`DROP_KEY`] does.
pub const DROP_LABEL: &str = "drop";

/// What [`CANCEL_KEY`] does in the inventory.
pub const CLOSE_LABEL: &str = "close";

/// The lines the inventory shows while the player carries nothing: that
/// the pack is empty and how to pick an item up. `{glyph}` stands for the
/// map glyph of a vial, the most common item.
pub fn empty_inventory() -> [String; 3] {
    [
        "You carry nothing yet.".to_owned(),
        "Stand on an item, {glyph} on the map, and press".to_owned(),
        format!("{PICK_UP_KEY} to pick it up."),
    ]
}

/// The first message of every run, so the log is never empty and tells
/// where the player stands.
pub fn welcome() -> Message {
    Message::new(Tone::Info, "You enter the dungeon.")
}

/// The first message of a run resumed from its save, in place of
/// [`welcome`], so the player knows the game went on where it stopped.
pub fn welcome_back() -> Message {
    Message::new(Tone::Info, "Welcome back. The dungeon is as you left it.")
}

/// The message that opens a new run when the save could not be read back:
/// `reason` is why (a sentence of `persist::Loaded::SetAside`), and
/// `moved_to` the path the save was renamed to, where it can still be
/// looked at.
pub fn save_set_aside(reason: &str, moved_to: &str) -> Message {
    Message::new(
        Tone::Danger,
        format!(
            "Your saved game could not be read: {reason}. It was kept as \
             {moved_to}, and a new game begins."
        ),
    )
}

/// The message logged when the save of the dead character could not be
/// deleted: deleting it is tried again when the player quits, and a failure
/// then is reported on leaving.
pub fn save_not_deleted() -> Message {
    Message::new(
        Tone::Danger,
        "The save of this character could not be deleted; it will be tried again on quitting.",
    )
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
///   essence (4).", "The backlash dazes you: you lose 1 turn."; a backlash
///   that took nothing is told as such, never as a loss of 0: a scorch
///   spared at the last hit point, "The backlash scorches you, but you hold
///   on." (still [`Tone::Danger`]: one hit point is left), and a drain with
///   no essence left, "The backlash finds no essence to drain."
///   ([`Tone::Info`]: nothing was at stake). The drain is still announced,
///   so every unstable cast logs which backlash it drew;
/// - a glyph learnt: "You decipher a carved glyph: Recall joins your
///   grimoire." ([`Tone::Success`]);
/// - a refused cast, which took no time: "Not enough essence to cast: 1 of
///   2." or "Passage needs a direction." ([`Tone::Info`]);
/// - a cast an item steadied: the cast's line says so, with the steadied
///   casts left, "Ember, steadied trace (1 left): the shade is seared (6).";
/// - an item picked up: "You pick up the Healing Draught." ([`Tone::Success`]);
///   one dropped: "You drop the Healing Draught." ([`Tone::Info`]);
/// - an item used and its effect, on one [`Tone::Success`] line with the
///   verb of its family (drink a vial, read a page, use a catalyst) and,
///   for a restoration, the value reached out of its maximum, so a capped
///   heal reads as such: "You drink the Healing Draught and recover 8 HP
///   (30/30).", "You drink the Healing Draught, but you were not hurt
///   (30/30).", "You drink the Essence Phial and recover 4 essence (6/6).",
///   "You read the Page of the Veil: Veil joins your grimoire.", "You use
///   the Steadying Ink: your next 2 unstable casts will be cast pure.";
/// - an action on an item that the game refused, which took no time
///   ([`Tone::Info`]): "There is nothing here to pick up.", "Your pack is
///   full (10/10): the Healing Draught stays on the floor.", and the
///   sentences of [`item_refusal`];
///   a descent: "You descend to depth 2." ([`Tone::Info`]: the status line
///   and the new floor already show the change, and the line marks in the
///   history where the floor above ends); a descent off the stairs, which
///   took no time: "There are no stairs here." ([`Tone::Info`]);
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
    // Opening of the line of the item being used, "You drink the Healing
    // Draught", likewise completed by the event of its effect.
    let mut use_lead: Option<String> = None;
    for event in events {
        if !is_spell_effect(event, game)
            && !matches!(event, Event::CastSteadied { .. })
            && let Some(lead) = cast_lead.take()
        {
            messages.push(Message::new(Tone::Sigil, format!("{lead}.")));
        }
        if !is_item_effect(event)
            && let Some(lead) = use_lead.take()
        {
            messages.push(Message::new(Tone::Success, format!("{lead}.")));
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
                    // The game spares the last hit point: a scorch at 1 HP
                    // takes nothing, which "(0)" would misreport.
                    let text = if *damage > 0 {
                        format!("The backlash scorches you ({damage}).")
                    } else {
                        "The backlash scorches you, but you hold on.".to_owned()
                    };
                    Some(Message::new(Tone::Danger, text))
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
            Event::Healed { actor, amount, hp } if use_lead.is_some() => {
                let max = game.actor(*actor).map_or(*hp, |actor| actor.max_hp());
                let lead = use_lead.take().unwrap_or_default();
                Some(Message::new(
                    Tone::Success,
                    restored_line(&lead, *amount, Restored::Health, *hp, max),
                ))
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
            Event::EssenceDrained { lost, .. } if *lost > 0 => Some(Message::new(
                Tone::Danger,
                format!("The backlash drains your essence ({lost})."),
            )),
            Event::EssenceDrained { .. } => Some(Message::new(
                Tone::Info,
                "The backlash finds no essence to drain.",
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
            Event::CastSteadied { casts_left, .. } => {
                if let Some(lead) = cast_lead.as_mut() {
                    // The game reports a steadied cast as pure: the line
                    // says what made it so instead.
                    let pure = quality_words(CastQuality::Pure);
                    *lead = lead.replace(pure, &format!("steadied trace ({casts_left} left)"));
                }
                None
            }
            Event::PickedUp { definition, .. } => Some(Message::new(
                Tone::Success,
                format!("You pick up the {}.", item_name(content, definition)),
            )),
            Event::InventoryFull { definition, .. } => Some(Message::new(
                Tone::Info,
                format!(
                    "Your pack is full ({}): the {} stays on the floor.",
                    capacity(INVENTORY_CAPACITY),
                    item_name(content, definition)
                ),
            )),
            Event::NothingToPickUp { .. } => Some(Message::new(
                Tone::Info,
                "There is nothing here to pick up.",
            )),
            Event::Used { definition, .. } => {
                let verb = content
                    .item(definition)
                    .map_or("use", |item| use_verb(item.family()));
                use_lead = Some(format!("You {verb} the {}", item_name(content, definition)));
                None
            }
            Event::EssenceRestored {
                actor,
                amount,
                essence,
            } => {
                let max = game
                    .actor(*actor)
                    .map_or(*essence, |actor| actor.max_essence());
                let lead = use_lead.take().unwrap_or_else(|| "You rest".to_owned());
                Some(Message::new(
                    Tone::Success,
                    restored_line(&lead, *amount, Restored::Essence, *essence, max),
                ))
            }
            Event::SigilLearned { sigil } => {
                let lead = use_lead.take().unwrap_or_else(|| "You learn".to_owned());
                Some(Message::new(
                    Tone::Success,
                    format!(
                        "{lead}: {} joins your grimoire.",
                        sigil_name(content, sigil)
                    ),
                ))
            }
            Event::Steadied { casts } => {
                let lead = use_lead.take().unwrap_or_else(|| "You steady".to_owned());
                let next = match casts {
                    1 => "your next unstable cast".to_owned(),
                    casts => format!("your next {casts} unstable casts"),
                };
                Some(Message::new(
                    Tone::Success,
                    format!("{lead}: {next} will be cast pure."),
                ))
            }
            Event::Dropped { definition, .. } => Some(Message::new(
                Tone::Info,
                format!("You drop the {}.", item_name(content, definition)),
            )),
            Event::ItemRefused { item, reason } => {
                Some(Message::new(Tone::Info, item_refusal(game, *item, *reason)))
            }
            Event::Descended { depth } => Some(Message::new(
                Tone::Info,
                format!("You descend to depth {depth}."),
            )),
            Event::NoStairsHere { .. } => {
                Some(Message::new(Tone::Info, "There are no stairs here."))
            }
            Event::Died { .. }
            | Event::Moved { .. }
            | Event::Waited { .. }
            | Event::MoveBlocked { .. }
            | Event::TurnLost { .. }
            | Event::Backlash { .. } => None,
        };
        messages.extend(message);
    }
    if let Some(lead) = cast_lead {
        messages.push(Message::new(Tone::Sigil, format!("{lead}.")));
    }
    if let Some(lead) = use_lead {
        messages.push(Message::new(Tone::Success, format!("{lead}.")));
    }
    messages
}

/// Returns why the game refused to use or drop the item `item` of `game`
/// for `reason`, as the log and the inventory say it: "You do not carry
/// that item.", "You already know Veil: the Page of the Veil has nothing
/// to teach.", "Something already lies here: drop the Healing Draught
/// elsewhere."
///
/// The item is named from `game`, which keeps every item not used up; one
/// it no longer knows is called "that item".
pub fn item_refusal(game: &Game, item: ItemId, reason: ItemRefusal) -> String {
    let definition = game.item(item).map(|item| item.definition());
    let name = definition.map_or_else(
        || "that item".to_owned(),
        |definition| format!("the {}", definition.name()),
    );
    match reason {
        ItemRefusal::NotCarried => "You do not carry that item.".to_owned(),
        ItemRefusal::NothingToLearn => {
            let sigil = definition.and_then(|definition| match definition.effect() {
                ItemEffect::Teach(sigil) => Some(sigil.name()),
                _ => None,
            });
            format!(
                "You already know {}: {name} has nothing to teach.",
                sigil.unwrap_or("its sigil")
            )
        }
        ItemRefusal::CellTaken => format!("Something already lies here: drop {name} elsewhere."),
    }
}

/// Returns the name shown to the player for the item definition `id` of
/// `content`, such as "Healing Draught"; an identifier missing from
/// `content` is shown as is, as [`monster_name`] does.
pub fn item_name<'name>(content: &'name Content, id: &'name str) -> &'name str {
    content.item(id).map_or(id, ItemDef::name)
}

/// Returns what `item` does in a few words, for the inventory: "heal 10
/// HP", "restore 4 essence", "teach Veil", "steady 2 casts".
///
/// A page whose sigil the player already knows (`known`) says so after
/// `separator`, "teach Veil · known", since the game would refuse to use
/// it.
pub fn item_summary(item: &ItemDef, known: bool, separator: char) -> String {
    match item.effect() {
        ItemEffect::Heal(amount) => format!("heal {amount} HP"),
        ItemEffect::RestoreEssence(amount) => format!("restore {amount} essence"),
        ItemEffect::Teach(sigil) if known => format!("teach {} {separator} known", sigil.name()),
        ItemEffect::Teach(sigil) => format!("teach {}", sigil.name()),
        ItemEffect::Steady(1) => "steady 1 cast".to_owned(),
        ItemEffect::Steady(casts) => format!("steady {casts} casts"),
    }
}

/// Returns how full the inventory is with `carried` items, out of
/// `item::INVENTORY_CAPACITY`: "3/10".
pub fn capacity(carried: usize) -> String {
    format!("{carried}/{INVENTORY_CAPACITY}")
}

/// Returns the sentence of the death summary that says who killed the
/// player, where and when: "Killed by an acolyte at depth 4, on turn 1283."
///
/// `depth` is the floor of the death and `turn` the player's turn it
/// happened on, counted from 1 as on the status line. A death by the
/// player's own hand (`Combatant::Player`, such as its own spell, which no
/// rule causes today) says so instead of naming a monster.
pub fn cause_of_death(killed_by: &Combatant, depth: u32, turn: u64, content: &Content) -> String {
    match killed_by {
        Combatant::Player => {
            format!("Undone by your own magic at depth {depth}, on turn {turn}.")
        }
        Combatant::Monster { definition } => {
            let name = monster_name(content, definition);
            format!(
                "Killed by {} {name} at depth {depth}, on turn {turn}.",
                indefinite_article(name)
            )
        }
    }
}

/// Returns the line under the death summary's title for the run of `seed`,
/// one of [`EPITAPHS`]: the same seed always gets the same line, so a
/// replayed run ends the same way.
pub fn epitaph(seed: Seed) -> &'static str {
    let value = seed.value();
    // Folding the high half in spreads the seeds typed by hand (small
    // numbers) and the random ones alike over the whole list.
    let mixed = value ^ (value >> 32);
    let count = EPITAPHS.len() as u64;
    let index = usize::try_from(mixed % count).unwrap_or_default();
    EPITAPHS[index]
}

/// Returns the sigils cast by quality, as the death summary details them:
/// "pure 22 · unstable 6 · fizzled 3", joined by `separator`.
pub fn cast_breakdown(casts: CastCounts, separator: char) -> String {
    format!(
        "pure {} {separator} unstable {} {separator} fizzled {}",
        casts.pure, casts.unstable, casts.fizzled
    )
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

/// The verb of using an item of `family`: a vial is drunk, a page read, a
/// catalyst used.
const fn use_verb(family: ItemFamily) -> &'static str {
    match family {
        ItemFamily::Vial => "drink",
        ItemFamily::Page => "read",
        ItemFamily::Catalyst => "use",
    }
}

/// What an item restored, for [`restored_line`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Restored {
    /// Hit points.
    Health,
    /// Essence.
    Essence,
}

/// The line of an item that restored `amount` of `what`, opened by `lead`
/// ("You drink the Healing Draught") and closed by the `value` reached out
/// of `max`, so a capped restoration reads as such: "… and recover 8 HP
/// (30/30)." or, when nothing was missing, "…, but you were not hurt
/// (30/30)."
fn restored_line(lead: &str, amount: i32, what: Restored, value: i32, max: i32) -> String {
    let (unit, already) = match what {
        Restored::Health => ("HP", "you were not hurt"),
        Restored::Essence => ("essence", "your essence was full"),
    };
    if amount > 0 {
        format!("{lead} and recover {amount} {unit} ({value}/{max}).")
    } else {
        format!("{lead}, but {already} ({value}/{max}).")
    }
}

/// Whether `event` is the effect of an item that ends the line of its use
/// (see [`describe`]).
const fn is_item_effect(event: &Event) -> bool {
    matches!(
        event,
        Event::Healed { .. }
            | Event::EssenceRestored { .. }
            | Event::SigilLearned { .. }
            | Event::Steadied { .. }
    )
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
            cause_of_death(&by("acolyte"), 4, 1283, &content),
            "Killed by an acolyte at depth 4, on turn 1283."
        );
        assert_eq!(
            cause_of_death(&by("ossuary_rat"), 1, 7, &content),
            "Killed by an ossuary rat at depth 1, on turn 7."
        );
        assert_eq!(
            cause_of_death(&by("hierophant"), 10, 12, &content),
            "Killed by a hierophant at depth 10, on turn 12."
        );
    }

    #[test]
    fn a_death_by_the_player_own_hand_names_no_monster() {
        assert_eq!(
            cause_of_death(&Combatant::Player, 3, 40, &content()),
            "Undone by your own magic at depth 3, on turn 40."
        );
    }

    #[test]
    fn the_epitaph_follows_the_seed_and_every_one_can_be_drawn() {
        let seed = Seed::new(0x7F3A_91C2_0B5E_D4A8);
        assert_eq!(epitaph(seed), epitaph(seed), "deterministic");

        let drawn: HashSet<&str> = (0..64).map(|value| epitaph(Seed::new(value))).collect();
        assert_eq!(drawn.len(), EPITAPHS.len(), "small seeds reach them all");
        for line in EPITAPHS {
            assert!(line.is_ascii() && line.len() <= 60, "{line}");
        }
    }

    #[test]
    fn the_cast_breakdown_names_each_quality_in_order() {
        let casts = CastCounts {
            pure: 22,
            unstable: 6,
            fizzled: 3,
        };

        assert_eq!(
            cast_breakdown(casts, '·'),
            "pure 22 · unstable 6 · fizzled 3"
        );
        assert_eq!(
            cast_breakdown(casts, '-'),
            "pure 22 - unstable 6 - fizzled 3"
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
    fn a_backlash_that_takes_nothing_is_never_told_as_a_loss_of_zero() {
        let (game, _, content) = spell_target();
        let player = game.player();
        let cases = [
            (
                Backlash::Scorch,
                Event::Wounded {
                    actor: player,
                    damage: 0,
                    hp_left: 1,
                },
                (Tone::Danger, "The backlash scorches you, but you hold on."),
            ),
            (
                Backlash::Scorch,
                Event::Wounded {
                    actor: player,
                    damage: 1,
                    hp_left: 1,
                },
                (Tone::Danger, "The backlash scorches you (1)."),
            ),
            (
                Backlash::Drain,
                Event::EssenceDrained {
                    actor: player,
                    lost: 0,
                },
                (Tone::Info, "The backlash finds no essence to drain."),
            ),
        ];

        for (backlash, consequence, expected) in cases {
            let events = [
                Event::Backlash {
                    caster: player,
                    backlash,
                },
                consequence,
            ];
            let messages = describe(&events, &game, &content);
            assert_eq!(lines(&messages), [expected], "{backlash:?}");
            assert!(!messages[0].text.contains("(0)"));
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

    /// The event of the item of definition `id` used up.
    fn used(id: &str) -> Event {
        Event::Used {
            item: ItemId::default(),
            definition: id.to_owned(),
        }
    }

    #[test]
    fn an_item_used_and_its_effect_make_one_line_with_the_capped_value() {
        let (game, _, content) = spell_target();
        let player = game.player();
        let cases = [
            (
                vec![
                    used("healing_draught"),
                    Event::Healed {
                        actor: player,
                        amount: 8,
                        hp: 30,
                    },
                ],
                "You drink the Healing Draught and recover 8 HP (30/30).",
            ),
            (
                vec![
                    used("healing_draught"),
                    Event::Healed {
                        actor: player,
                        amount: 0,
                        hp: 30,
                    },
                ],
                "You drink the Healing Draught, but you were not hurt (30/30).",
            ),
            (
                vec![
                    used("essence_phial"),
                    Event::EssenceRestored {
                        actor: player,
                        amount: 4,
                        essence: 5,
                    },
                ],
                "You drink the Essence Phial and recover 4 essence (5/6).",
            ),
            (
                vec![
                    used("essence_phial"),
                    Event::EssenceRestored {
                        actor: player,
                        amount: 0,
                        essence: 6,
                    },
                ],
                "You drink the Essence Phial, but your essence was full (6/6).",
            ),
            (
                vec![
                    used("veil_page"),
                    Event::SigilLearned {
                        sigil: "veil".to_owned(),
                    },
                ],
                "You read the Page of the Veil: Veil joins your grimoire.",
            ),
            (
                vec![used("steadying_ink"), Event::Steadied { casts: 2 }],
                "You use the Steadying Ink: your next 2 unstable casts will be cast pure.",
            ),
            (
                vec![used("steadying_ink"), Event::Steadied { casts: 1 }],
                "You use the Steadying Ink: your next unstable cast will be cast pure.",
            ),
            (
                vec![used("healing_draught")],
                "You drink the Healing Draught.",
            ),
        ];

        for (events, expected) in cases {
            let messages = describe(&events, &game, &content);
            assert_eq!(lines(&messages), [(Tone::Success, expected)], "{events:?}");
        }
    }

    #[test]
    fn a_healing_spell_and_a_healing_draught_read_apart() {
        let (game, _, content) = spell_target();
        let healed = Event::Healed {
            actor: game.player(),
            amount: 8,
            hp: 30,
        };

        let spell = describe(
            &[cast("recall", CastQuality::Pure), healed.clone()],
            &game,
            &content,
        );
        let draught = describe(&[used("healing_draught"), healed], &game, &content);

        assert_eq!(texts(&spell), ["Recall, pure trace: you mend (8)."]);
        assert_eq!(
            texts(&draught),
            ["You drink the Healing Draught and recover 8 HP (30/30)."]
        );
    }

    #[test]
    fn picking_up_dropping_and_refusals_have_their_own_lines() {
        let (game, _, content) = spell_target();
        let item = ItemId::default();
        let events = [
            Event::PickedUp {
                item,
                definition: "healing_draught".to_owned(),
            },
            Event::NothingToPickUp {
                position: Position::new(5, 5),
            },
            Event::InventoryFull {
                item,
                definition: "steadying_ink".to_owned(),
            },
            Event::Dropped {
                item,
                definition: "essence_phial".to_owned(),
                position: Position::new(5, 5),
            },
            Event::ItemRefused {
                item,
                reason: ItemRefusal::NotCarried,
            },
        ];

        let messages = describe(&events, &game, &content);

        assert_eq!(
            lines(&messages),
            [
                (Tone::Success, "You pick up the Healing Draught."),
                (Tone::Info, "There is nothing here to pick up."),
                (
                    Tone::Info,
                    "Your pack is full (10/10): the Steadying Ink stays on the floor."
                ),
                (Tone::Info, "You drop the Essence Phial."),
                (Tone::Info, "You do not carry that item."),
            ]
        );
    }

    #[test]
    fn a_descent_names_the_new_depth_and_a_refused_one_says_why() {
        let (game, _, content) = spell_target();
        let events = [
            Event::NoStairsHere {
                position: Position::new(5, 5),
            },
            Event::Descended { depth: 2 },
            Event::Descended { depth: 10 },
        ];

        let messages = describe(&events, &game, &content);

        assert_eq!(
            lines(&messages),
            [
                (Tone::Info, "There are no stairs here."),
                (Tone::Info, "You descend to depth 2."),
                (Tone::Info, "You descend to depth 10."),
            ]
        );
    }

    #[test]
    fn a_refusal_names_the_item_and_why() {
        let content = content();
        let mut game = Game::in_fixed_room(SEED);
        let page = game.place_item(Position::new(6, 5), content.item("veil_page").unwrap());
        let draught = game.place_item(
            Position::new(7, 5),
            content.item("healing_draught").unwrap(),
        );

        assert_eq!(
            item_refusal(&game, page, ItemRefusal::NothingToLearn),
            "You already know Veil: the Page of the Veil has nothing to teach."
        );
        assert_eq!(
            item_refusal(&game, draught, ItemRefusal::CellTaken),
            "Something already lies here: drop the Healing Draught elsewhere."
        );
        assert_eq!(
            item_refusal(&game, ItemId::default(), ItemRefusal::CellTaken),
            "Something already lies here: drop that item elsewhere."
        );
    }

    #[test]
    fn a_steadied_cast_says_so_in_its_line() {
        let (game, rat, content) = spell_target();
        let events = [
            cast("ember", CastQuality::Pure),
            Event::CastSteadied {
                sigil: "ember".to_owned(),
                casts_left: 1,
            },
            Event::Wounded {
                actor: rat,
                damage: 6,
                hp_left: 2,
            },
        ];

        let messages = describe(&events, &game, &content);

        assert_eq!(
            lines(&messages),
            [(
                Tone::Sigil,
                "Ember, steadied trace (1 left): the ossuary rat is seared (6)."
            )]
        );
    }

    #[test]
    fn every_shipped_item_has_a_short_summary() {
        let summaries: Vec<String> = content()
            .items()
            .iter()
            .map(|item| item_summary(item, false, '-'))
            .collect();

        assert_eq!(
            summaries,
            [
                "heal 10 HP",
                "restore 4 essence",
                "steady 2 casts",
                "teach Veil"
            ]
        );
        let page = content().item("veil_page").unwrap().clone();
        assert_eq!(item_summary(&page, true, '-'), "teach Veil - known");
        assert_eq!(capacity(3), "3/10");
    }
}
