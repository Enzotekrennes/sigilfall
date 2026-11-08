//! Spawns and difficulty, on the core crate.
//!
//! Written against the public API only. They check that a floor is populated
//! from the spawn table of its depth, that the draw follows the weights, that
//! difficulty can be measured and rises with depth, and that
//! `content/spawns.ron` is validated with errors that name the file, the rule
//! and the field.
//!
//! The expected table is read from the text of the shipped `spawns.ron` by
//! a small reader of its own (`table`), not from `Content`, so a bug in the
//! production reader or in the weight lookup is noticed. The numbers of the
//! difficulty (6 monsters on the first floor, 12 at most) are written out
//! here. Every loop is bounded.

use std::collections::BTreeMap;

use sigilfall_core::content::{Content, ContentError, MAX_DEPTH, SPAWNS_FILE};
use sigilfall_core::generation::{self, FIRST_DEPTH};
use sigilfall_core::rng::Seed;

/// The files shipped with the game.
const MONSTERS: &str = include_str!("../content/monsters.ron");
const SIGILS: &str = include_str!("../content/sigils.ron");
const ITEMS: &str = include_str!("../content/items.ron");
const SPAWNS: &str = include_str!("../content/spawns.ron");

/// Floors checked: the content's, and two beyond (they use the last table).
const DEEPEST_CHECKED: u32 = MAX_DEPTH + 2;

/// Seeds drawn for each floor of the statistics.
const SAMPLE_SEEDS: u64 = 200;

/// The content shipped with the game.
fn shipped() -> Content {
    Content::load().expect("shipped content is valid")
}

/// Whether a rule of the table places a monster or an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Monster,
    Item,
}

/// One rule read from the spawn file.
#[derive(Debug, Clone)]
struct Rule {
    kind: Kind,
    id: String,
    from: u32,
    to: u32,
    weight: u32,
}

/// The text between `after` and the next `stop` in `line`.
fn between<'a>(line: &'a str, after: &str, stop: char) -> &'a str {
    let rest = &line[line
        .find(after)
        .unwrap_or_else(|| panic!("{after} in {line}"))
        + after.len()..];
    rest[..rest.find(stop).unwrap_or(rest.len())].trim()
}

/// The rules written in `text`, one `Spawn(...)` per line.
fn table(text: &str) -> Vec<Rule> {
    text.lines()
        .map(str::trim)
        .filter(|line| line.starts_with("Spawn("))
        .map(|line| {
            let (kind, after) = if line.contains("Monster(\"") {
                (Kind::Monster, "Monster(\"")
            } else {
                (Kind::Item, "Item(\"")
            };
            let number = |name: &str| -> u32 {
                between(line, &format!("{name}:"), ',')
                    .trim_end_matches(')')
                    .parse()
                    .unwrap_or_else(|_| panic!("a number for {name} in {line}"))
            };
            Rule {
                kind,
                id: between(line, after, '"').to_owned(),
                from: number("from_depth"),
                to: number("to_depth"),
                weight: number("weight"),
            }
        })
        .collect()
}

/// The weights the table of `text` gives to the entries of `kind` on floor
/// `depth` (floors deeper than [`MAX_DEPTH`] count as [`MAX_DEPTH`]).
fn weights(rules: &[Rule], kind: Kind, depth: u32) -> BTreeMap<String, u32> {
    let depth = depth.min(MAX_DEPTH);
    rules
        .iter()
        .filter(|rule| rule.kind == kind && rule.from <= depth && depth <= rule.to)
        .map(|rule| (rule.id.clone(), rule.weight))
        .collect()
}

/// Seeds `0..count`.
fn seeds(count: u64) -> impl Iterator<Item = Seed> {
    (0..count).map(Seed::from)
}

// --- Monsters come from the table of the depth --------------------

#[test]
fn the_reader_of_this_file_found_every_shipped_rule() {
    let rules = table(SPAWNS);
    let content = shipped();
    assert_eq!(
        rules.iter().filter(|r| r.kind == Kind::Monster).count(),
        20,
        "20 monster rules in the shipped table"
    );
    for monster in content.monsters() {
        assert!(
            rules.iter().any(|r| r.id == monster.id()),
            "{}",
            monster.id()
        );
    }
    for item in content.items() {
        assert!(rules.iter().any(|r| r.id == item.id()), "{}", item.id());
    }
}

#[test]
fn the_content_gives_every_floor_the_weights_written_in_the_file() {
    let rules = table(SPAWNS);
    let content = shipped();
    for depth in FIRST_DEPTH..=DEEPEST_CHECKED {
        let monsters: BTreeMap<String, u32> = content
            .monster_weights(depth)
            .map(|(monster, weight)| (monster.id().to_owned(), weight))
            .collect();
        let items: BTreeMap<String, u32> = content
            .item_weights(depth)
            .map(|(item, weight)| (item.id().to_owned(), weight))
            .collect();
        assert_eq!(
            monsters,
            weights(&rules, Kind::Monster, depth),
            "depth {depth}"
        );
        assert_eq!(items, weights(&rules, Kind::Item, depth), "depth {depth}");
    }
}

#[test]
fn every_monster_and_item_of_a_floor_is_in_the_table_of_its_depth() {
    let rules = table(SPAWNS);
    let content = shipped();
    for depth in FIRST_DEPTH..=DEEPEST_CHECKED {
        let allowed_monsters = weights(&rules, Kind::Monster, depth);
        let allowed_items = weights(&rules, Kind::Item, depth);
        for seed in seeds(60) {
            let floor = generation::generate_with_monsters(seed, depth, &content);
            for spawn in floor.monsters() {
                assert!(
                    allowed_monsters.contains_key(spawn.definition().id()),
                    "seed {seed}, depth {depth}: {} is not in the table of this depth",
                    spawn.definition().id()
                );
            }
            for spawn in floor.items() {
                assert!(
                    allowed_items.contains_key(spawn.definition().id()),
                    "seed {seed}, depth {depth}: {} is not in the table of this depth",
                    spawn.definition().id()
                );
            }
        }
    }
}

#[test]
fn deeper_floors_bring_in_their_new_creatures_and_drop_none_without_a_rule() {
    let content = shipped();
    let kinds_at = |depth: u32| -> Vec<String> {
        let mut kinds: Vec<String> = seeds(SAMPLE_SEEDS)
            .flat_map(|seed| {
                generation::generate_with_monsters(seed, depth, &content)
                    .monsters()
                    .to_vec()
            })
            .map(|spawn| spawn.definition().id().to_owned())
            .collect();
        kinds.sort();
        kinds.dedup();
        kinds
    };

    assert_eq!(kinds_at(1), ["ossuary_rat", "wax_beetle"]);
    let two = kinds_at(2);
    assert!(two.contains(&"acolyte".to_owned()) && two.contains(&"homunculus".to_owned()));
    assert!(
        !two.contains(&"vigil_eye".to_owned()) && !two.contains(&"shade".to_owned()),
        "no creature from a deeper band: {two:?}"
    );
    let ten = kinds_at(10);
    assert!(ten.contains(&"lead_homunculus".to_owned()) && ten.contains(&"hierophant".to_owned()));
    assert_eq!(kinds_at(11), ten, "floor 11 draws like floor 10");
    assert_eq!(kinds_at(12), ten, "floor 12 draws like floor 10");
}

#[test]
fn the_draw_follows_the_weights_of_the_floor() {
    let rules = table(SPAWNS);
    let content = shipped();
    // First floor (equal weights), a middle one and the deepest.
    for depth in [1, 4, 10] {
        let expected = weights(&rules, Kind::Monster, depth);
        let total: u32 = expected.values().sum();
        let mut seen: BTreeMap<String, u32> = BTreeMap::new();
        let mut draws = 0_u32;
        for seed in seeds(2 * SAMPLE_SEEDS) {
            for spawn in generation::generate_with_monsters(seed, depth, &content).monsters() {
                *seen.entry(spawn.definition().id().to_owned()).or_default() += 1;
                draws += 1;
            }
        }
        for (id, weight) in &expected {
            let share = f64::from(*seen.get(id).unwrap_or(&0)) / f64::from(draws);
            let wanted = f64::from(*weight) / f64::from(total);
            assert!(
                (share - wanted).abs() < 0.04,
                "depth {depth}: {id} drawn {share:.3} of the time, the table says {wanted:.3}"
            );
        }
    }
}

#[test]
fn the_items_of_a_floor_also_follow_the_table() {
    let rules = table(SPAWNS);
    let content = shipped();
    for depth in [1, 2, 7] {
        let expected = weights(&rules, Kind::Item, depth);
        let total: u32 = expected.values().sum();
        let mut seen: BTreeMap<String, u32> = BTreeMap::new();
        let mut draws = 0_u32;
        for seed in seeds(2 * SAMPLE_SEEDS) {
            for spawn in generation::generate_with_monsters(seed, depth, &content).items() {
                *seen.entry(spawn.definition().id().to_owned()).or_default() += 1;
                draws += 1;
            }
        }
        for (id, weight) in &expected {
            let share = f64::from(*seen.get(id).unwrap_or(&0)) / f64::from(draws);
            let wanted = f64::from(*weight) / f64::from(total);
            assert!(
                (share - wanted).abs() < 0.06,
                "depth {depth}: {id} drawn {share:.3} of the time, the table says {wanted:.3}"
            );
        }
    }
}

#[test]
fn a_spawn_file_that_changes_the_table_changes_the_floor_without_recompiling() {
    // The rats of floors 2 and 3 go from weight 4 to the highest weight, 99.
    let rule = r#"Spawn(of: Monster("ossuary_rat"), from_depth: 2, to_depth: 3, weight: 4)"#;
    assert!(
        SPAWNS.contains(rule),
        "the shipped rule this test edits moved"
    );
    let edited = SPAWNS.replace(rule, &rule.replace("weight: 4", "weight: 99"));
    let heavier = parse(&edited).expect("the edited table is valid");
    let rats = |content: &Content| -> usize {
        seeds(SAMPLE_SEEDS)
            .flat_map(|seed| {
                generation::generate_with_monsters(seed, 2, content)
                    .monsters()
                    .to_vec()
            })
            .filter(|spawn| spawn.definition().id() == "ossuary_rat")
            .count()
    };

    let (before, after) = (rats(&shipped()), rats(&heavier));

    // 4 of 10 monsters of floor 2 before, 99 of 105 after.
    assert!(
        before * 2 < 7 * 200 && after * 10 > 7 * 200 * 8,
        "{before} then {after} rats"
    );
}

// --- Difficulty is measurable and rises with depth --------------------------------

/// What the monsters of floor `depth` add up to, averaged over
/// [`SAMPLE_SEEDS`] seeds: how many they are, their hit points, and the
/// damage they could deal in 100 ticks (attack times speed over 100).
fn floor_load(content: &Content, depth: u32) -> (f64, f64, f64) {
    let (mut count, mut hp, mut threat) = (0.0, 0.0, 0.0);
    for seed in seeds(SAMPLE_SEEDS) {
        for spawn in generation::generate_with_monsters(seed, depth, content).monsters() {
            let monster = spawn.definition();
            count += 1.0;
            hp += f64::from(monster.hp());
            threat += f64::from(monster.attack()) * f64::from(monster.speed()) / 100.0;
        }
    }
    let n = SAMPLE_SEEDS as f64;
    (count / n, hp / n, threat / n)
}

#[test]
fn the_number_of_monsters_per_floor_is_six_then_one_more_per_floor_up_to_twelve() {
    let content = shipped();
    for depth in FIRST_DEPTH..=DEEPEST_CHECKED {
        let expected = usize::try_from((5 + depth).min(12)).unwrap();
        assert_eq!(
            generation::monsters_per_floor(depth),
            expected,
            "depth {depth}"
        );
        for seed in seeds(10) {
            let floor = generation::generate_with_monsters(seed, depth, &content);
            assert_eq!(
                floor.monsters().len(),
                expected,
                "seed {seed}, depth {depth}"
            );
        }
    }
}

#[test]
fn difficulty_rises_with_depth_by_band_and_stops_at_the_last_floor() {
    let content = shipped();
    let loads: Vec<(f64, f64, f64)> = (FIRST_DEPTH..=DEEPEST_CHECKED)
        .map(|depth| floor_load(&content, depth))
        .collect();
    let band = |from: usize, to: usize, pick: fn(&(f64, f64, f64)) -> f64| -> f64 {
        loads[from - 1..to].iter().map(pick).sum::<f64>() / (to - from + 1) as f64
    };
    for (name, pick) in [
        ("hit points", (|l: &(f64, f64, f64)| l.1) as fn(&_) -> f64),
        ("threat", |l: &(f64, f64, f64)| l.2),
    ] {
        let bands = [
            band(1, 2, pick),
            band(3, 4, pick),
            band(5, 6, pick),
            band(7, 8, pick),
            band(9, 10, pick),
        ];
        for pair in bands.windows(2) {
            assert!(
                pair[0] < pair[1],
                "{name} does not rise from band to band: {bands:?}"
            );
        }
        assert!(
            pick(&loads[9]) > 3.0 * pick(&loads[0]),
            "{name} at depth 10 is not more than 3 times depth 1: {} against {}",
            pick(&loads[9]),
            pick(&loads[0])
        );
    }
    // Past the last floor nothing grows any more.
    for pick in [
        |l: &(f64, f64, f64)| l.0,
        |l: &(f64, f64, f64)| l.1,
        |l: &(f64, f64, f64)| l.2,
    ] {
        let (ten, eleven, twelve) = (pick(&loads[9]), pick(&loads[10]), pick(&loads[11]));
        assert!((eleven - ten).abs() / ten < 0.05 && (twelve - ten).abs() / ten < 0.05);
    }
}

#[test]
fn the_toughest_creature_of_the_last_floors_is_stronger_than_the_first_floor_ones() {
    let content = shipped();
    let strongest = |depth: u32| -> i32 {
        content
            .monster_weights(depth)
            .map(|(monster, _)| monster.hp() + 3 * monster.attack())
            .max()
            .unwrap_or(0)
    };
    assert!(strongest(10) > 2 * strongest(1));
    assert!(strongest(5) > strongest(1));
}

// --- Validation of spawns.ron ----------------------------------------------------

/// The shipped content with `spawns` as the spawn file.
fn parse(spawns: &str) -> Result<Content, ContentError> {
    Content::parse_with_spawns(MONSTERS, SIGILS, ITEMS, spawns)
}

/// The shipped spawn file without the lines that contain `text`.
fn without(text: &str) -> String {
    SPAWNS
        .lines()
        .filter(|line| !line.contains(text))
        .map(|line| format!("{line}\n"))
        .collect()
}

/// The shipped spawn file with `rules` added before its closing bracket.
fn with(rules: &[&str]) -> String {
    let end = SPAWNS.rfind(']').expect("a list");
    let added: String = rules.iter().map(|rule| format!("    {rule},\n")).collect();
    format!("{}{added}]\n", &SPAWNS[..end])
}

/// Asserts that `spawns` is refused with a message that names the spawn
/// file, the rule (by the identifier it places) and the field, and holds
/// `problem` as a hint of what is wrong.
fn assert_refused(spawns: &str, id: &str, field: &str, problem: &str) {
    let error = parse(spawns).expect_err("the spawn file should be refused");
    let message = error.to_string();
    assert!(message.contains(SPAWNS_FILE), "no file in: {message}");
    assert!(
        message.contains(&format!("`{id}`")),
        "no rule `{id}` in: {message}"
    );
    assert!(
        message.contains(&format!("`{field}`")),
        "no field `{field}` in: {message}"
    );
    assert!(message.contains(problem), "no `{problem}` in: {message}");
}

#[test]
fn the_shipped_spawn_file_is_valid_and_covers_every_floor() {
    let content = parse(SPAWNS).expect("the shipped table is valid");
    for depth in FIRST_DEPTH..=MAX_DEPTH {
        assert!(
            content.monster_weights(depth).next().is_some(),
            "floor {depth}"
        );
    }
    assert_eq!(content, shipped());
}

#[test]
fn validation_names_the_rule_and_the_field_for_an_unknown_creature() {
    assert_refused(
        &with(&[r#"Spawn(of: Monster("ghoul"), from_depth: 1, to_depth: 2, weight: 1)"#]),
        "ghoul",
        "of",
        "not the identifier of any monster",
    );
    assert_refused(
        &with(&[r#"Spawn(of: Item("sword"), from_depth: 1, to_depth: 2, weight: 1)"#]),
        "sword",
        "of",
        "not the identifier of any item",
    );
}

#[test]
fn validation_refuses_floors_out_of_bounds_and_names_the_field() {
    let base = without("Monster(\"vigil_eye\")");
    let rule = |from: i64, to: i64| {
        format!(r#"Spawn(of: Monster("vigil_eye"), from_depth: {from}, to_depth: {to}, weight: 1)"#)
    };
    assert_refused(&with_on(&base, &rule(0, 5)), "vigil_eye", "from_depth", "0");
    assert_refused(
        &with_on(&base, &rule(11, 11)),
        "vigil_eye",
        "from_depth",
        "11",
    );
    assert_refused(&with_on(&base, &rule(4, 11)), "vigil_eye", "to_depth", "11");
    assert_refused(&with_on(&base, &rule(6, 5)), "vigil_eye", "to_depth", "5");
    assert!(
        parse(&with_on(&base, &rule(10, 10))).is_ok(),
        "floor 10 is accepted"
    );
}

/// `base` with `rule` added before its closing bracket.
fn with_on(base: &str, rule: &str) -> String {
    let end = base.rfind(']').expect("a list");
    format!("{}    {rule},\n]\n", &base[..end])
}

#[test]
fn validation_refuses_weights_out_of_bounds_and_names_the_field() {
    let base = without("steadying_ink");
    for weight in [0, -3, 100, 1000] {
        let rule = format!(
            r#"Spawn(of: Item("steadying_ink"), from_depth: 1, to_depth: 10, weight: {weight})"#
        );
        assert_refused(
            &with_on(&base, &rule),
            "steadying_ink",
            "weight",
            &weight.to_string(),
        );
    }
    let rule = r#"Spawn(of: Item("steadying_ink"), from_depth: 1, to_depth: 10, weight: 99)"#;
    assert!(
        parse(&with_on(&base, rule)).is_ok(),
        "99 is the highest weight accepted"
    );
}

#[test]
fn validation_refuses_a_band_shallower_than_what_it_places() {
    let base = without("hierophant");
    let rule = r#"Spawn(of: Monster("hierophant"), from_depth: 1, to_depth: 3, weight: 1)"#;
    assert_refused(
        &with_on(&base, rule),
        "hierophant",
        "from_depth",
        "min_depth",
    );
}

#[test]
fn validation_refuses_two_rules_of_one_creature_on_a_common_floor() {
    assert_refused(
        &with(&[r#"Spawn(of: Monster("ossuary_rat"), from_depth: 3, to_depth: 5, weight: 2)"#]),
        "ossuary_rat",
        "from_depth",
        "overlap",
    );
}

#[test]
fn validation_refuses_a_rule_with_a_missing_field_or_a_misspelt_one() {
    assert_refused(
        &with(&[r#"Spawn(of: Monster("shade"), from_depth: 3, weight: 1)"#]),
        "shade",
        "to_depth",
        "missing",
    );
    let error = parse(&with(&[
        r#"Spawn(of: Monster("shade"), from_depth: 3, to_depth: 4, wieght: 1)"#,
    ]))
    .expect_err("a misspelt field");
    let message = error.to_string();
    assert!(
        message.contains(SPAWNS_FILE) && message.contains("shade") && message.contains("wieght"),
        "{message}"
    );
}

#[test]
fn validation_refuses_a_file_that_is_not_a_list_of_rules() {
    for broken in [
        "",
        "[",
        "Spawn(",
        "[ Spawn(of: Monster(\"ossuary_rat\") ]",
        "{}",
        "42",
    ] {
        let error = parse(broken).expect_err(broken);
        assert!(
            error.to_string().contains(SPAWNS_FILE),
            "{broken:?}: {error}"
        );
    }
}

#[test]
fn validation_refuses_a_creature_or_an_item_that_no_rule_places() {
    let error = parse(&without("lead_homunculus")).expect_err("a creature without rule");
    assert!(
        matches!(error, ContentError::NoSpawnRule { .. }),
        "{error:?}"
    );
    let message = error.to_string();
    assert!(
        message.contains(SPAWNS_FILE) && message.contains("lead_homunculus"),
        "{message}"
    );

    let error = parse(&without("veil_page")).expect_err("an item without rule");
    assert!(error.to_string().contains("veil_page"), "{error}");
}

#[test]
fn validation_refuses_a_table_that_leaves_a_floor_without_monster() {
    let monsters = r#"[
        Monster(id: "mote", name: "mote", family: Vermin, elite: false,
            speed: 100, hp: 1, attack: 1, defense: 0, min_depth: 1),
    ]"#;
    let rules = |to: u32| {
        format!(
            r#"[
        Spawn(of: Monster("mote"), from_depth: 1, to_depth: {to}, weight: 1),
        Spawn(of: Item("healing_draught"), from_depth: 1, to_depth: 10, weight: 1),
        Spawn(of: Item("essence_phial"), from_depth: 1, to_depth: 10, weight: 1),
        Spawn(of: Item("steadying_ink"), from_depth: 1, to_depth: 10, weight: 1),
        Spawn(of: Item("veil_page"), from_depth: 2, to_depth: 10, weight: 1),
    ]"#
        )
    };

    let error = Content::parse_with_spawns(monsters, SIGILS, ITEMS, &rules(9))
        .expect_err("floor 10 has no monster");

    assert!(
        matches!(error, ContentError::NoMonsterAtDepth { depth: 10, .. }),
        "{error:?}"
    );
    let message = error.to_string();
    assert!(
        message.contains(SPAWNS_FILE) && message.contains("floor 10"),
        "{message}"
    );
    assert!(
        Content::parse_with_spawns(monsters, SIGILS, ITEMS, &rules(10)).is_ok(),
        "the same table reaching floor 10 is valid"
    );
}
