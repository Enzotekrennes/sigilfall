//! Tests of the catalogue: the sentences each event gives, in each
//! language, the bounds of what a message quotes from a file, and the
//! rules of French typography.

use sigilfall_core::action::Action;
use sigilfall_core::game::PLAYER_START;
use sigilfall_core::geometry::{Direction, Position};
use sigilfall_core::sigil::{Backlash, Trace};

use super::*;
use crate::language::Language;

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
    // The hierophant (14 hit points) survives a first blow; striking it
    // again and again ends with a kill, bounded by its hit points.
    let (mut game, content) = duel("hierophant");
    let first = game.submit(Action::Move(Direction::East));
    let Some(Event::Attacked { damage, .. }) = first.first() else {
        panic!("the move struck: {first:?}");
    };

    let messages = describe(Language::English, &first, &game, &content);

    assert_eq!(messages[0].tone, Tone::Info);
    assert_eq!(
        messages[0].text,
        format!("You hit the hierophant ({damage}).")
    );

    for _ in 0..20 {
        let events = game.submit(Action::Move(Direction::East));
        let messages = describe(Language::English, &events, &game, &content);
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
    panic!("twenty blows of at least one point kill 14 hit points");
}

#[test]
fn a_monster_blow_on_the_player_is_a_danger_with_its_damage() {
    let (mut game, content) = duel("ossuary_rat");

    // The rat starts with no energy: waiting lets it strike.
    let events: Vec<_> = (0..3).flat_map(|_| game.submit(Action::Wait)).collect();
    let messages = describe(Language::English, &events, &game, &content);

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
    Monster(id: "executioner", name: "executioner", name_fr: Masculine("le monstre"), plural: "executioners", plural_fr: "monstres", family: Acolyte, elite: true,
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

    let messages = describe(Language::English, &events, &game, &content);

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

    assert_eq!(describe(Language::English, &events, &game, &content), []);
}

#[test]
fn the_cause_of_death_names_the_killer_with_its_article() {
    let content = content();
    let by = |definition: &str| Combatant::Monster {
        definition: definition.to_owned(),
    };

    assert_eq!(
        cause_of_death(Language::English, &by("acolyte"), 4, 1283, &content),
        "Killed by an acolyte at depth 4, on turn 1283."
    );
    assert_eq!(
        cause_of_death(Language::English, &by("ossuary_rat"), 1, 7, &content),
        "Killed by an ossuary rat at depth 1, on turn 7."
    );
    assert_eq!(
        cause_of_death(Language::English, &by("hierophant"), 10, 12, &content),
        "Killed by a hierophant at depth 10, on turn 12."
    );
}

#[test]
fn a_death_by_the_player_own_hand_names_no_monster() {
    assert_eq!(
        cause_of_death(Language::English, &Combatant::Player, 3, 40, &content()),
        "Undone by your own magic at depth 3, on turn 40."
    );
}

#[test]
fn the_epitaph_follows_the_seed_and_every_one_can_be_drawn() {
    let seed = Seed::new(0x7F3A_91C2_0B5E_D4A8);
    assert_eq!(
        epitaph(Language::English, seed),
        epitaph(Language::English, seed),
        "deterministic"
    );

    let drawn: HashSet<&str> = (0..64)
        .map(|value| epitaph(Language::English, Seed::new(value)))
        .collect();
    assert_eq!(
        drawn.len(),
        ENGLISH.epitaphs.len(),
        "small seeds reach them all"
    );
    for line in ENGLISH.epitaphs {
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
        cast_breakdown(Language::English, casts, '·'),
        "pure 22 · unstable 6 · fizzled 3"
    );
    assert_eq!(
        cast_breakdown(Language::English, casts, '-'),
        "pure 22 - unstable 6 - fizzled 3"
    );
}

#[test]
fn an_unknown_definition_is_shown_by_its_identifier() {
    assert_eq!(
        monster_name(Language::English, &content(), "mystery_beast"),
        "mystery_beast"
    );
    assert_eq!(
        monster_name(Language::English, &content(), "wax_beetle"),
        "wax beetle"
    );
}

#[test]
fn the_replay_hint_gives_the_command_with_the_shareable_seed() {
    assert_eq!(
        replay_hint(Language::English, Seed::new(0x7F3A_91C2_0B5E_D4A8)),
        "Replay this seed: sigilfall --seed 7F3A-91C2-0B5E-D4A8"
    );
}

#[test]
fn the_share_line_names_the_daily_date_or_the_seed_then_the_counts() {
    let date = DailyDate::new(2026, 11, 8).expect("a real date");
    let fresh = Stats::default();

    assert_eq!(
        share_line(Some(date), Seed::daily(date), &fresh, &Status::Playing),
        "Sigilfall daily 2026-11-08 - depth 1, turn 0, 0 slain, 0 sigils"
    );
    assert_eq!(
        share_line(
            None,
            Seed::new(0x7F3A_91C2_0B5E_D4A8),
            &fresh,
            &Status::Playing
        ),
        "Sigilfall seed 7F3A-91C2-0B5E-D4A8 - depth 1, turn 0, 0 slain, 0 sigils"
    );
}

#[test]
fn the_share_line_reads_every_count_from_the_stats() {
    let content = content();
    let mut game = Game::in_fixed_room(SEED);
    let rat = content.monster("ossuary_rat").expect("a shipped monster");
    let east = PLAYER_START.step(Direction::East);
    game.spawn_monster(east, rat);
    while game.stats().total_kills() == 0 {
        game.submit(Action::Move(Direction::East));
    }
    game.submit(Action::Cast {
        trace: Trace::from_pattern(&[1, 2, 3, 6, 9, 8, 7, 4]).expect("a trace"),
        direction: None,
    });
    let stats = game.stats();
    assert_eq!(stats.casts().total(), 1, "one cast, fizzled");

    assert_eq!(
        share_line(None, SEED, stats, game.status()),
        format!(
            "Sigilfall seed {SEED} - depth 1, turn {}, 1 slain, 1 sigil",
            stats.turns()
        )
    );
    assert!(stats.turns() > 1);
}

#[test]
fn the_longest_share_line_fits_80_columns_in_plain_ascii() {
    // Depth 10, the deepest floor, a seed (longer than a date), and
    // counts far beyond the runs of the balance tests (deaths at depth
    // 2 to 7, a few hundred turns).
    // A fresh run writes one digit for each count: the longest line
    // has 1 + 4 + 2 + 1 more.
    let fresh = share_line(
        None,
        Seed::new(u64::MAX),
        &Stats::default(),
        &Status::Playing,
    );
    let longest = fresh.len() + 1 + 4 + 2 + 1;

    assert!(fresh.is_ascii(), "{fresh}");
    assert!(longest <= 80, "{longest} columns: {fresh}");
    // A victory writes "victory" in the place of "depth 1", which
    // leaves room for "depth 10": its longest line is shorter still.
    let won = share_line(None, Seed::new(u64::MAX), &Stats::default(), &Status::Won);
    assert!(won.is_ascii(), "{won}");
    assert!(won.len() + 4 + 2 + 1 < longest, "{won}");
}

#[test]
fn the_share_line_of_a_victory_says_so_in_place_of_the_depth() {
    let date = DailyDate::new(2026, 11, 8).expect("a real date");
    let fresh = Stats::default();

    assert_eq!(
        share_line(Some(date), Seed::daily(date), &fresh, &Status::Won),
        "Sigilfall daily 2026-11-08 - victory, turn 0, 0 slain, 0 sigils"
    );
    assert_eq!(
        share_line(None, Seed::new(0x7F3A_91C2_0B5E_D4A8), &fresh, &Status::Won),
        "Sigilfall seed 7F3A-91C2-0B5E-D4A8 - victory, turn 0, 0 slain, 0 sigils"
    );
}

#[test]
fn the_victory_line_follows_the_seed_and_every_one_can_be_drawn() {
    let seed = Seed::new(0x7F3A_91C2_0B5E_D4A8);
    assert_eq!(
        victory_line(Language::English, seed),
        victory_line(Language::English, seed),
        "deterministic"
    );

    let drawn: HashSet<&str> = (0..64)
        .map(|value| victory_line(Language::English, Seed::new(value)))
        .collect();
    assert_eq!(
        drawn.len(),
        ENGLISH.victory_lines.len(),
        "small seeds reach them all"
    );
    for line in ENGLISH.victory_lines {
        assert!(line.is_ascii() && line.len() <= 60, "{line}");
        assert!(!ENGLISH.epitaphs.contains(&line), "{line}");
    }
    assert_eq!(
        victory_outcome(Language::English, 10, 1283),
        "You broke the seal at depth 10, on turn 1283."
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

    let messages = describe(Language::English, &events, &game, &content());

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
        let messages = describe(Language::English, &events, &game, &content);
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

    let messages = describe(Language::English, &events, &game, &content);

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
        let messages = describe(Language::English, &events, &game, &content);
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
        let messages = describe(Language::English, &events, &game, &content);
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

    let messages = describe(Language::English, &events, &game, &content);

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
fn an_unknown_pattern_has_one_line_that_points_to_the_glyphs() {
    let (game, _, content) = spell_target();
    let events = [
        Event::SigilCast {
            sigil: None,
            quality: CastQuality::Fizzled,
        },
        Event::UnknownPattern,
    ];

    let messages = describe(Language::English, &events, &game, &content);

    assert_eq!(
        lines(&messages),
        [(
            Tone::Sigil,
            "That pattern is not in your grimoire, and the trace fizzles. \
             Sigils are learnt by reading the glyphs carved in the floor."
        )]
    );
}

#[test]
fn a_cast_without_an_effect_event_is_still_told() {
    let (game, _, content) = spell_target();

    let messages = describe(
        Language::English,
        &[cast("ember", CastQuality::Pure)],
        &game,
        &content,
    );

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
    assert_eq!(
        effect_summary(Language::English, &ember, '-'),
        "harm 6 - nearest"
    );
    assert_eq!(
        cast_time(Language::English, 3, '-'),
        "1.25 turns - +0.25 a node"
    );
    assert_eq!(
        cast_time(Language::English, 0, '-'),
        "0.50 turns - +0.25 a node"
    );
    assert_eq!(
        cast_time(Language::English, 9, '-'),
        "2.75 turns - +0.25 a node"
    );
}

#[test]
fn every_shipped_sigil_has_a_short_effect_summary() {
    let summaries: Vec<String> = content()
        .sigils()
        .iter()
        .map(|sigil| effect_summary(Language::English, sigil, '-'))
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
        let messages = describe(Language::English, &events, &game, &content);
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
        Language::English,
        &[cast("recall", CastQuality::Pure), healed.clone()],
        &game,
        &content,
    );
    let draught = describe(
        Language::English,
        &[used("healing_draught"), healed],
        &game,
        &content,
    );

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

    let messages = describe(Language::English, &events, &game, &content);

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
        Event::Descended { depth: 9 },
    ];

    let messages = describe(Language::English, &events, &game, &content);

    assert_eq!(
        lines(&messages),
        [
            (Tone::Info, "There are no stairs here."),
            (Tone::Info, "You descend to depth 2."),
            (Tone::Info, "You descend to depth 9."),
        ]
    );
}

#[test]
fn the_descent_to_the_bottom_floor_says_it_is_the_last_and_holds_the_seal() {
    let (game, _, content) = spell_target();
    let line = "You descend to depth 10, the last. The seal lies here.";
    assert!(generation::is_bottom_floor(10), "10 is the bottom floor");
    let events = [Event::Descended { depth: 10 }];

    let messages = describe(Language::English, &events, &game, &content);

    assert_eq!(lines(&messages), [(Tone::Info, line)]);
    assert!(
        line.chars().count() <= TEXT_WIDTH,
        "the line fits the log without wrapping"
    );
}

#[test]
fn nothing_left_to_explore_is_said_once() {
    let (game, _, content) = spell_target();
    let events = [Event::NothingToExplore {
        position: Position::new(5, 5),
    }];

    let messages = describe(Language::English, &events, &game, &content);

    assert_eq!(
        lines(&messages),
        [(Tone::Info, "There is nothing left to explore within reach.")]
    );
    assert_eq!(
        exploration_halted(Language::English, &Halt::Explored, &content),
        None
    );
}

#[test]
fn every_other_halt_of_exploration_says_why() {
    let content = content();
    let halts = [
        (
            Halt::FoeInView("ossuary_rat".to_owned()),
            Tone::Info,
            "You cannot explore with the ossuary rat in view.",
        ),
        (
            Halt::FoeAppeared("ossuary_rat".to_owned()),
            Tone::Danger,
            "An ossuary rat comes into view.",
        ),
        (
            Halt::FoeAppeared("shade".to_owned()),
            Tone::Danger,
            "A shade comes into view.",
        ),
        (
            Halt::ItemAppeared("essence_phial".to_owned()),
            Tone::Info,
            "You spot an Essence Phial.",
        ),
        (
            Halt::ItemAppeared("healing_draught".to_owned()),
            Tone::Info,
            "You spot a Healing Draught.",
        ),
        (
            Halt::GlyphAppeared,
            Tone::Sigil,
            "You spot a carved glyph you have not read.",
        ),
        (Halt::Hurt, Tone::Danger, "You are hurt and stop exploring."),
        (
            Halt::Blocked,
            Tone::Info,
            "Something stands in your way; you stop exploring.",
        ),
        (
            Halt::StepLimit,
            Tone::Info,
            "You stop exploring after 200 steps.",
        ),
        (Halt::Interrupted, Tone::Info, "You stop exploring."),
    ];

    for (halt, tone, text) in halts {
        let message = exploration_halted(Language::English, &halt, &content);

        assert_eq!(message, Some(Message::new(tone, text)), "{halt:?}");
        assert!(text.chars().count() <= TEXT_WIDTH, "{text}");
    }
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
        item_refusal(Language::English, &game, page, ItemRefusal::NothingToLearn),
        "You already know Veil: the Page of the Veil has nothing to teach."
    );
    assert_eq!(
        item_refusal(Language::English, &game, draught, ItemRefusal::CellTaken),
        "Something already lies here: drop the Healing Draught elsewhere."
    );
    assert_eq!(
        item_refusal(
            Language::English,
            &game,
            ItemId::default(),
            ItemRefusal::CellTaken
        ),
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

    let messages = describe(Language::English, &events, &game, &content);

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
        .map(|item| item_summary(Language::English, item, false, '-'))
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
    assert_eq!(
        item_summary(Language::English, &page, true, '-'),
        "teach Veil - known"
    );
    assert_eq!(capacity(3), "3/10");
}

#[test]
fn a_refused_save_gives_its_cause_whole_and_cuts_only_what_it_quotes() {
    use sigilfall_core::save::Reference;

    let refusal = |error: SaveError| save_refusal(Language::English, &SaveRefusal::Refused(error));
    // The sentences of the game are never cut, however long.
    let cause = "the hit points of the actor in slot 4294967295 is -2147483648, expected 0 to 999";
    assert_eq!(
        refusal(SaveError::Inconsistent(cause.to_owned())),
        format!("{cause} (inconsistent save)")
    );
    assert_eq!(
        refusal(SaveError::Inconsistent(
            "floor 10, the bottom floor, has 2 seals".to_owned()
        )),
        "floor 10, the bottom floor, has 2 seals (inconsistent save)"
    );
    // What the reader quotes from the file is cut, the sentence kept.
    let found = format!("found `{}`", "x".repeat(20_000));
    let malformed = refusal(SaveError::Malformed {
        line: 1,
        column: 20_002,
        message: found,
    });
    assert!(
        malformed.starts_with("the save is unreadable at line 1, column 20002: found `x"),
        "{malformed}"
    );
    assert!(malformed.ends_with(ELLIPSIS), "{malformed}");
    let quote = malformed.split(": ").nth(1).unwrap();
    assert_eq!(quote.chars().count(), MAX_QUOTED_CHARS, "{malformed}");
    let unknown = refusal(SaveError::UnknownReference {
        kind: Reference::Monster,
        id: "d".repeat(500),
    });
    assert!(
        unknown.starts_with("the save references monster `d"),
        "{unknown}"
    );
    assert!(
        unknown.ends_with(&format!("{ELLIPSIS}`, absent from the content")),
        "{unknown}"
    );
    // A short quote is kept whole.
    assert_eq!(
        refusal(SaveError::Malformed {
            line: 3,
            column: 7,
            message: "Expected colon".to_owned()
        }),
        "the save is unreadable at line 3, column 7: Expected colon"
    );
    let too_large = SaveRefusal::TooLarge {
        limit: crate::persist::MAX_FILE_BYTES,
    };
    assert_eq!(
        save_refusal(Language::English, &too_large),
        "the save is larger than 4 MiB"
    );
    assert_eq!(
        save_refusal(Language::French, &too_large),
        "la sauvegarde dépasse 4 Mio"
    );
}

#[test]
fn a_refused_save_is_told_in_french_with_the_same_quotes() {
    use sigilfall_core::save::Reference;

    let refusal = |error: SaveError| save_refusal(Language::French, &SaveRefusal::Refused(error));
    assert_eq!(
        refusal(SaveError::Malformed {
            line: 3,
            column: 7,
            message: "Expected colon".to_owned()
        }),
        "la sauvegarde est illisible ligne 3, colonne 7\u{a0}: Expected colon"
    );
    assert_eq!(
        refusal(SaveError::UnknownReference {
            kind: Reference::Sigil,
            id: "flame".to_owned(),
        }),
        "la sauvegarde mentionne le sigille `flame`, absent du contenu"
    );
    assert_eq!(
        refusal(SaveError::UnsupportedVersion {
            found: 9,
            expected: 4
        }),
        "la sauvegarde est au format 9, mais ce jeu ne lit que le format 4"
    );
    assert_eq!(
        save_refusal(Language::French, &SaveRefusal::NotText),
        "la sauvegarde n'est pas un texte"
    );
}

/// Every notice about a file, with `reason` as the system's cause and the
/// save's refusal, and `path` as the file.
fn every_file_notice(refusal: &SaveRefusal, reason: &str, path: &str) -> Vec<FileNotice> {
    vec![
        FileNotice::SaveSetAside {
            reason: refusal.clone(),
            moved_to: path.to_owned(),
        },
        FileNotice::SaveNotDeleted {
            path: path.to_owned(),
            cause: reason.to_owned(),
        },
        FileNotice::FinishedSaveNotDeleted {
            path: path.to_owned(),
            cause: reason.to_owned(),
        },
        FileNotice::OptionsNotSaved {
            path: path.to_owned(),
            cause: reason.to_owned(),
        },
    ]
}

#[test]
fn no_control_character_of_a_file_reaches_the_screen_or_the_error_output() {
    use sigilfall_core::save::Reference;

    let hostile = "dra\u{1b}[2Jgon\u{7}\r\n\u{9b}31m";
    let unknown = SaveRefusal::Refused(SaveError::UnknownReference {
        kind: Reference::Monster,
        id: hostile.to_owned(),
    });
    assert_eq!(
        save_refusal(Language::English, &unknown),
        "the save references monster `dra?[2Jgon????31m`, absent from the content"
    );
    let malformed = SaveRefusal::Refused(SaveError::Malformed {
        line: 1,
        column: 2,
        message: hostile.to_owned(),
    });
    assert_eq!(
        save_refusal(Language::English, &malformed),
        "the save is unreadable at line 1, column 2: dra?[2Jgon????31m"
    );
    for language in Language::ALL {
        let mut texts: Vec<String> = every_file_notice(&malformed, hostile, hostile)
            .iter()
            .map(|notice| file_notice(language, notice))
            .collect();
        texts.push(save_set_aside(language, &malformed, hostile).text);
        texts.push(file_notice(
            language,
            &FileNotice::OptionsUnreadable(OptionsProblem::System(hostile.to_owned())),
        ));
        texts.push(launch_refused(
            language,
            hostile,
            "--seed",
            &LaunchWanted::Seed(SEED),
        ));
        for text in texts {
            assert!(!text.chars().any(char::is_control), "{text:?}");
        }
    }
    assert_eq!(printable("plain text"), "plain text");
}

#[test]
fn a_long_reason_is_cut_with_an_ellipsis_and_a_short_one_kept_whole() {
    let short = "the save is unreadable at line 1, column 20002: Expected colon";
    assert_eq!(bounded_reason(short), short);
    let limit = "r".repeat(MAX_REASON_CHARS);
    assert_eq!(bounded_reason(&limit), limit);

    let long = format!("the save is unreadable: found `{}`", "x".repeat(20_000));
    let cut = bounded_reason(&long);
    assert_eq!(cut.chars().count(), MAX_REASON_CHARS);
    assert!(cut.ends_with(ELLIPSIS), "{cut}");
    assert!(cut.starts_with("the save is unreadable: found `x"), "{cut}");
    // No space is left before the ellipsis.
    let spaced = format!("{} {}", "w".repeat(MAX_REASON_CHARS - 2), "tail");
    assert_eq!(
        bounded_reason(&spaced),
        format!("{}{ELLIPSIS}", "w".repeat(MAX_REASON_CHARS - 2))
    );
}

#[test]
fn a_path_longer_than_a_line_loses_its_start_and_keeps_the_file_name() {
    let short = r"C:\Users\player\AppData\Roaming\sigilfall\data\save.ron.bad";
    assert_eq!(bounded_path(short), short);

    let folder = "d".repeat(120);
    let long = format!(r"C:\{folder}\save.ron.bad");
    let cut = bounded_path(&long);
    assert_eq!(cut.chars().count(), MAX_PATH_CHARS);
    assert!(cut.starts_with(ELLIPSIS), "{cut}");
    assert!(long.ends_with(cut.trim_start_matches(ELLIPSIS)), "{cut}");
    assert!(cut.ends_with(r"\save.ron.bad"), "{cut}");
    // Characters beyond ASCII count as one each, and are not split.
    let accented = format!("{}\\é.bad", "é".repeat(100));
    assert_eq!(bounded_path(&accented).chars().count(), MAX_PATH_CHARS);
}

#[test]
fn every_message_about_a_file_keeps_its_path_whole_or_its_end() {
    let reason = "x".repeat(20_000);
    let refusal = SaveRefusal::Refused(SaveError::Malformed {
        line: 1,
        column: 1,
        message: reason.clone(),
    });
    let path = format!(r"C:\{}\save.ron.bad", "p".repeat(104));
    assert_eq!(path.chars().count(), 120);
    let shown = bounded_path(&path);
    for language in Language::ALL {
        let mut texts: Vec<String> = every_file_notice(&refusal, &reason, &path)
            .iter()
            .map(|notice| file_notice(language, notice))
            .collect();
        texts.push(save_set_aside(language, &refusal, &path).text);
        for text in texts {
            assert!(text.contains(&shown), "{text}");
            assert!(text.contains(r"\save.ron.bad"), "{text}");
            assert!(text.chars().count() < 420, "{text}");
        }
        let unreadable = FileNotice::OptionsUnreadable(OptionsProblem::System(reason.clone()));
        assert!(file_notice(language, &unreadable).chars().count() < 220);
    }
}

#[test]
fn the_readme_names_the_colour_values_as_the_options_screen_shows_them() {
    /// The README, from the root of the repository.
    const README: &str = include_str!("../../../../README.md");
    let options = README
        .split("### Options")
        .nth(1)
        .and_then(|section| {
            section
                .split(
                    "
## ",
                )
                .next()
        })
        .expect("the README has an Options section");
    let options = options.split_whitespace().collect::<Vec<_>>().join(" ");

    let listed = format!(
        "**Colours** ({}, {} or {})",
        auto_colors(Language::English, ColorSupport::None, '=')
            .split_whitespace()
            .next()
            .unwrap(),
        ENGLISH.ansi16_value,
        ENGLISH.monochrome_value
    );
    assert!(options.contains(&listed), "{listed} in {options}");
}

#[test]
fn the_colour_help_names_the_values_as_the_options_screen_shows_them() {
    let help = ENGLISH.option_help[1];
    assert!(help.contains(ENGLISH.ansi16_value), "{help}");
    assert!(help.contains(ENGLISH.monochrome_value), "{help}");
    assert!(!help.contains("none"), "{help}");
}

/// Whether `text` follows French typography: every `:`, `;`, `!` and `?`
/// comes after a no-break space, and none after an ordinary one.
fn has_french_typography(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    chars
        .iter()
        .enumerate()
        .all(|(index, &mark)| !":;!?".contains(mark) || index > 0 && chars[index - 1] == '\u{a0}')
}

#[test]
fn every_french_label_follows_french_typography() {
    for text in FRENCH.all() {
        assert!(has_french_typography(text), "{text:?}");
    }
    // The rule itself: an ordinary space or none fails.
    assert!(!has_french_typography("Tracé : raté"));
    assert!(!has_french_typography("Tracé: raté"));
    assert!(has_french_typography("Tracé\u{a0}: raté"));
}

#[test]
fn the_french_sentences_follow_french_typography() {
    let content = content();
    let (game, _) = duel("shade");
    let date = DailyDate::new(2026, 11, 9).expect("a real date");
    let mut texts = vec![
        enlarge_terminal(Language::French, (80, 24), (60, 20), '×'),
        invalid_seed(Language::French, "abc"),
        auto_colors(Language::French, ColorSupport::None, '·'),
        language_forced(Language::French, Language::French),
        replay_hint(Language::French, SEED),
        cast_time(Language::French, 3, '·'),
        node_refused(Language::French, Node::new(5).unwrap()),
        essence_too_low(Language::French, 1, 2),
        aim_prompt(Language::French, "Passage"),
        welcome(Language::French).text,
        welcome_back(Language::French).text,
        save_not_deleted(Language::French).text,
        launch_refused(
            Language::French,
            "save.ron",
            "--daily",
            &LaunchWanted::Daily(date),
        ),
    ];
    for halt in [
        Halt::FoeInView("shade".to_owned()),
        Halt::FoeAppeared("acolyte".to_owned()),
        Halt::ItemAppeared("essence_phial".to_owned()),
        Halt::GlyphAppeared,
        Halt::Hurt,
        Halt::Blocked,
        Halt::StepLimit,
        Halt::Interrupted,
    ] {
        texts.extend(exploration_halted(Language::French, &halt, &content).map(|m| m.text));
    }
    for reason in [ItemRefusal::NotCarried, ItemRefusal::CellTaken] {
        texts.push(item_refusal(
            Language::French,
            &game,
            ItemId::default(),
            reason,
        ));
    }
    for text in texts {
        assert!(has_french_typography(&text), "{text:?}");
    }
}

#[test]
fn the_french_sentences_agree_with_the_creature_and_elide_its_article() {
    let content = content();
    let halted = |id: &str| {
        exploration_halted(
            Language::French,
            &Halt::FoeAppeared(id.to_owned()),
            &content,
        )
        .unwrap()
        .text
    };
    assert_eq!(halted("shade"), "Une ombre apparaît.");
    assert_eq!(halted("hierophant"), "Un hiérophante apparaît.");
    let in_view = exploration_halted(
        Language::French,
        &Halt::FoeInView("acolyte".to_owned()),
        &content,
    )
    .unwrap();
    assert_eq!(in_view.text, "Impossible d'explorer avec l'acolyte en vue.");
    assert_eq!(
        cause_of_death(
            Language::French,
            &Combatant::Monster {
                definition: "shade".to_owned()
            },
            4,
            1283,
            &content
        ),
        "Victime d'une ombre à la profondeur 4, au tour 1283."
    );
    assert_eq!(
        cause_of_death(
            Language::French,
            &Combatant::Monster {
                definition: "hierophant".to_owned()
            },
            4,
            12,
            &content
        ),
        "Victime d'un hiérophante à la profondeur 4, au tour 12."
    );
}

#[test]
fn a_french_blow_puts_the_player_before_the_verb() {
    let (mut game, content) = duel("ossuary_rat");
    let events: Vec<_> = (0..3).flat_map(|_| game.submit(Action::Wait)).collect();
    let messages = describe(Language::French, &events, &game, &content);
    assert!(!messages.is_empty(), "the rat struck: {events:?}");
    for message in &messages {
        assert!(
            message.text.starts_with("Le rat d'ossuaire vous frappe ("),
            "{message:?}"
        );
    }
    let (mut game, content) = duel("shade");
    let events = game.submit(Action::Move(Direction::East));
    let messages = describe(Language::French, &events, &game, &content);
    assert!(
        messages[0].text.starts_with("Vous frappez l'ombre ("),
        "{messages:?}"
    );
}

#[test]
fn the_fixed_french_texts_fit_their_places() {
    for line in FRENCH.epitaphs.iter().chain(&FRENCH.victory_lines) {
        assert!(line.chars().count() <= 60, "{line}");
    }
    for help in FRENCH.option_help {
        assert!(help.chars().count() <= 80, "{help}");
    }
    // The grimoire and the verdict share the 36 columns right of the grid;
    // `{glyph}` is one column once drawn.
    for line in FRENCH
        .empty_grimoire
        .iter()
        .chain(&FRENCH.learn_from_glyphs)
    {
        let width = line.replace("{glyph}", "§").chars().count();
        assert!(width <= 36, "{line}");
    }
    let content = content();
    for sigil in content.sigils() {
        let summary = effect_summary(Language::French, sigil, '·');
        assert!(summary.chars().count() <= 16, "{summary}");
    }
    for item in content.items() {
        let summary = item_summary(Language::French, item, true, '·');
        assert!(summary.chars().count() <= 24, "{summary}");
    }
}

#[test]
fn every_french_name_of_the_shipped_content_fits_the_side_panel() {
    // The side panel keeps 17 columns for a name; a longer one is cut.
    let content = content();
    for monster in content.monsters() {
        assert!(
            monster.french_name().noun().chars().count() <= 17,
            "{}",
            monster.id()
        );
    }
    for item in content.items() {
        assert!(
            item.french_name().noun().chars().count() <= 17,
            "{}",
            item.id()
        );
    }
}
