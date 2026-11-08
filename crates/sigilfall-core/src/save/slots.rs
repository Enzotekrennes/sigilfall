//! Slot maps as a save records them: every key in use, and the order in
//! which the freed slots will be reused.
//!
//! Exposes to the crate [`SlotKey`], [`SlotEntry`], [`SlotsRecord`],
//! [`capture`], which records a slot map, and [`rebuild`], which turns a
//! record back into a slot map with the very same keys, and [`Rebuilt`],
//! the result.
//!
//! Events and actions name actors and items by their keys, and a slot map
//! hands out new keys from its freed slots, the most recently freed first.
//! For a loaded game to play out as the saved one, the keys in use are not
//! enough: the next keys handed out must match too. So the record keeps,
//! besides the key and value of each occupied slot, the vacant slots with
//! their versions, in the order they will be reused.
//!
//! A slot map does not show its free slots. [`capture`] finds them by
//! inserting stand-ins into a copy until a brand-new slot comes out;
//! [`rebuild`] replays insertions and removals on an empty map until each
//! slot has its recorded version and the vacant ones are chained in the
//! recorded order. Both rely on two behaviours of `slotmap::SlotMap`: an
//! insertion reuses the most recently freed slot, or else appends a slot at
//! version 1, and each insertion or removal raises the version of its slot by
//! one (odd while occupied, even while vacant). Every rebuilt key is checked
//! against the record, so a change of that behaviour fails the load instead
//! of mixing keys up. Keys are read as numbers through their serde form, the
//! one `slotmap` documents for saving them.

use serde::{Deserialize, Serialize};
use slotmap::{Key, SlotMap};

use super::SaveError;

/// Version of a slot the slot map has just appended: a new slot is never
/// vacant before, so no vacant slot comes back at this version.
const FRESH_VERSION: u32 = 1;

/// Most slots a saved slot map may have.
///
/// A slot map has as many slots as values held at once at its peak, and a
/// floor holds a dozen monsters and a few dozen items at most: this leaves
/// room to spare while keeping a faulty save from making the rebuild work
/// for long.
const MAX_SLOTS: usize = 1024;

/// Highest version a saved slot may have.
///
/// A slot gains 2 each time it is freed then reused, which happens about
/// once per floor and slot (monsters and items are cleared on each
/// descent), so a real save stays far below this; the bound keeps the replay
/// of a faulty save short.
const MAX_SLOT_VERSION: u32 = 4096;

/// A key of a slot map as a save writes it: the pair `(index, version)`,
/// such as `(3, 1)`.
///
/// The index is that of the slot, counted from 1; the version is odd while
/// the slot is occupied and even while it is vacant. A pair rather than a
/// struct keeps each key on the line of what it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "(u32, u32)", into = "(u32, u32)")]
pub(crate) struct SlotKey {
    /// Index of the slot, from 1.
    pub(crate) index: u32,
    /// Version of the slot.
    pub(crate) version: u32,
}

impl From<(u32, u32)> for SlotKey {
    fn from((index, version): (u32, u32)) -> Self {
        Self { index, version }
    }
}

impl From<SlotKey> for (u32, u32) {
    fn from(key: SlotKey) -> Self {
        (key.index, key.version)
    }
}

impl SlotKey {
    /// Returns the index and version of `key`.
    ///
    /// They are read through the serde form of the key, `slotmap`'s
    /// documented way of saving keys; its other accessors make no promise
    /// about their values.
    pub(crate) fn of<K: Key>(key: K) -> Self {
        /// The serde form of a `slotmap::KeyData`.
        #[derive(Deserialize)]
        struct KeyFields {
            /// Index of the slot.
            idx: u32,
            /// Version of the slot.
            version: u32,
        }
        // Writing two integers and reading them back cannot fail: a failure
        // would be a bug of the RON crate, not something a save can cause.
        let text = ron::to_string(&key.data()).expect("a slot map key is written as RON");
        let fields: KeyFields = ron::from_str(&text).expect("a written slot map key reads back");
        Self {
            index: fields.idx,
            version: fields.version,
        }
    }
}

/// An occupied slot as a save writes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SlotEntry<T> {
    /// Key of the slot.
    pub(crate) key: SlotKey,
    /// What the slot holds.
    pub(crate) value: T,
}

/// A slot map as a save writes it.
///
/// Together, `entries` and `vacant` name every slot from index 1 to the
/// last exactly once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SlotsRecord<T> {
    /// The occupied slots, by increasing index.
    pub(crate) entries: Vec<SlotEntry<T>>,
    /// The vacant slots with their (even) versions, in the order the next
    /// insertions will reuse them.
    pub(crate) vacant: Vec<SlotKey>,
}

impl<T> SlotsRecord<T> {
    /// Turns each value with `convert`, given the value's key, keeping the
    /// layout of the slots.
    ///
    /// # Errors
    ///
    /// Returns the first error of `convert`, which then sees no further
    /// value.
    pub(crate) fn try_map<U, E>(
        self,
        mut convert: impl FnMut(SlotKey, T) -> Result<U, E>,
    ) -> Result<SlotsRecord<U>, E> {
        let entries = self
            .entries
            .into_iter()
            .map(|entry| {
                Ok(SlotEntry {
                    key: entry.key,
                    value: convert(entry.key, entry.value)?,
                })
            })
            .collect::<Result<_, E>>()?;
        Ok(SlotsRecord {
            entries,
            vacant: self.vacant,
        })
    }
}

/// Records `slots`, its values written by `record`.
///
/// `stand_in` makes values to fill a copy of `slots` while its vacant slots
/// are found; they never reach `slots` nor the record.
pub(crate) fn capture<K: Key, V: Clone, R>(
    slots: &SlotMap<K, V>,
    stand_in: impl Fn() -> V,
    record: impl Fn(&V) -> R,
) -> SlotsRecord<R> {
    let entries = slots
        .iter()
        .map(|(key, value)| SlotEntry {
            key: SlotKey::of(key),
            value: record(value),
        })
        .collect();
    let mut probe = slots.clone();
    let mut vacant = Vec::new();
    loop {
        let reused = SlotKey::of(probe.insert(stand_in()));
        if reused.version == FRESH_VERSION {
            break;
        }
        vacant.push(SlotKey {
            index: reused.index,
            version: reused.version - 1,
        });
    }
    SlotsRecord { entries, vacant }
}

/// A slot map rebuilt from its record, with the keys of its occupied slots.
#[derive(Debug)]
pub(crate) struct Rebuilt<K: Key, V> {
    /// The slot map, with the recorded keys, values and vacant slots.
    pub(crate) slots: SlotMap<K, V>,
    /// Key of each occupied slot, by index (index 0 unused); `None` for a
    /// vacant slot.
    keys: Vec<Option<K>>,
}

impl<K: Key, V> Rebuilt<K, V> {
    /// Returns the key of the rebuilt map that `saved` names, or `None` if
    /// `saved` names no occupied slot at that version.
    pub(crate) fn key(&self, saved: SlotKey) -> Option<K> {
        let index = usize::try_from(saved.index).ok()?;
        let key = (*self.keys.get(index)?)?;
        (SlotKey::of(key) == saved).then_some(key)
    }
}

/// Rebuilds the slot map recorded in `record`, whose slots are named
/// `what` in error messages ("actor", "item").
///
/// `stand_in` makes values that fill the slots while they are replayed; none
/// is left in the result. The rebuilt map has the recorded keys and values,
/// and hands out the same next keys as the recorded one.
///
/// # Errors
///
/// Returns [`SaveError::Inconsistent`] if the record has more than
/// [`MAX_SLOTS`] slots, a version above [`MAX_SLOT_VERSION`], an occupied
/// slot at an even version or a vacant one at an odd version, or does not
/// name every slot from 1 to the last exactly once; or if the slot map does
/// not rebuild to the recorded keys.
pub(crate) fn rebuild<K: Key, V>(
    record: SlotsRecord<V>,
    what: &str,
    stand_in: impl Fn() -> V,
) -> Result<Rebuilt<K, V>, SaveError> {
    let targets = slot_targets(&record, what)?;

    let mut slots = SlotMap::with_key();
    let mut keys: Vec<K> = Vec::with_capacity(targets.len() + 1);
    keys.push(K::null());
    for _ in 0..targets.len() {
        keys.push(slots.insert(stand_in()));
    }
    // Each slot starts occupied at version 1; one removal and one insertion
    // raise it by 2, and a removal right before an insertion makes it reuse
    // the same slot, as no other slot is free yet. A vacant slot stops one
    // version short, the one its final removal adds.
    for (index, &target) in targets.iter().enumerate() {
        let occupied_version = if target % 2 == 1 { target } else { target - 1 };
        for _ in 0..occupied_version / 2 {
            slots.remove(keys[index + 1]);
            keys[index + 1] = slots.insert(stand_in());
        }
    }
    // The slot freed last is reused first: freeing in reverse order chains
    // the vacant slots in the recorded order.
    for vacant in record.vacant.iter().rev() {
        slots.remove(keys[slot_index(*vacant)]);
    }

    let mut occupied = vec![None; keys.len()];
    for entry in record.entries {
        let index = slot_index(entry.key);
        let key = keys[index];
        if SlotKey::of(key) != entry.key {
            return Err(SaveError::Inconsistent(format!(
                "the {what} slot {} could not be rebuilt at version {}",
                entry.key.index, entry.key.version
            )));
        }
        slots[key] = entry.value;
        occupied[index] = Some(key);
    }
    Ok(Rebuilt {
        slots,
        keys: occupied,
    })
}

/// Returns the version each slot of `record` must end at, by index from 1
/// (the first element is slot 1).
///
/// # Errors
///
/// Returns [`SaveError::Inconsistent`] for each fault [`rebuild`] lists
/// about the record itself.
fn slot_targets<V>(record: &SlotsRecord<V>, what: &str) -> Result<Vec<u32>, SaveError> {
    let count = record.entries.len() + record.vacant.len();
    if count > MAX_SLOTS {
        return Err(SaveError::Inconsistent(format!(
            "the {what} slots number {count}, at most {MAX_SLOTS} are allowed"
        )));
    }
    let mut targets: Vec<Option<u32>> = vec![None; count];
    let occupied = record.entries.iter().map(|entry| (entry.key, true));
    let vacant = record.vacant.iter().map(|&key| (key, false));
    for (key, is_occupied) in occupied.chain(vacant) {
        let state = if is_occupied { "occupied" } else { "vacant" };
        let in_bounds = (1..=MAX_SLOT_VERSION).contains(&key.version);
        if !in_bounds || (key.version % 2 == 1) != is_occupied {
            return Err(SaveError::Inconsistent(format!(
                "the {what} slot {} is {state} at version {}: versions go from 1 to \
                 {MAX_SLOT_VERSION}, odd while occupied and even while vacant",
                key.index, key.version
            )));
        }
        let target = usize::try_from(key.index)
            .ok()
            .and_then(|index| targets.get_mut(index.checked_sub(1)?));
        match target {
            Some(target @ None) => *target = Some(key.version),
            _ => {
                return Err(SaveError::Inconsistent(format!(
                    "the {what} slot {} is out of the slots 1 to {count} or listed twice",
                    key.index
                )));
            }
        }
    }
    // Every slot was named once: `count` keys landed in `count` places.
    Ok(targets.into_iter().flatten().collect())
}

/// Returns the position of `key`'s slot in the list of rebuilt keys, which
/// [`slot_targets`] has checked to be within it.
fn slot_index(key: SlotKey) -> usize {
    usize::try_from(key.index).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use rand::RngExt as _;

    use super::*;
    use crate::rng::{Purpose, Seed};

    slotmap::new_key_type! {
        /// Key of the slot maps of these tests.
        struct TestKey;
    }

    /// Value of the stand-ins: never inserted by the tests themselves.
    const STAND_IN: u32 = u32::MAX;

    /// Records `slots`, its values copied as they are.
    fn record(slots: &SlotMap<TestKey, u32>) -> SlotsRecord<u32> {
        capture(slots, || STAND_IN, |&value| value)
    }

    /// Returns the next `count` keys `slots` would hand out.
    fn next_keys(slots: &SlotMap<TestKey, u32>, count: usize) -> Vec<TestKey> {
        let mut copy = slots.clone();
        (0..count).map(|_| copy.insert(STAND_IN)).collect()
    }

    /// A slot map after a random run of insertions and removals drawn from
    /// `seed`, with values counting up from 0.
    fn churned(seed: u64) -> SlotMap<TestKey, u32> {
        let mut rng = Seed::new(seed).stream(0, Purpose::Gameplay);
        let mut slots = SlotMap::with_key();
        let mut live: Vec<TestKey> = Vec::new();
        for value in 0..rng.random_range(0..200) {
            if !live.is_empty() && rng.random_range(0..3) == 0 {
                let gone = live.swap_remove(rng.random_range(0..live.len()));
                slots.remove(gone);
            } else {
                live.push(slots.insert(value));
            }
        }
        slots
    }

    #[test]
    fn a_rebuilt_slot_map_has_the_same_keys_and_hands_out_the_same_next_ones() {
        for seed in 0..200 {
            let original = churned(seed);
            let saved = record(&original);

            let rebuilt = rebuild::<TestKey, u32>(saved.clone(), "test", || STAND_IN)
                .expect("a recorded map rebuilds");

            let pairs = |slots: &SlotMap<TestKey, u32>| -> Vec<(TestKey, u32)> {
                slots.iter().map(|(key, &value)| (key, value)).collect()
            };
            assert_eq!(pairs(&rebuilt.slots), pairs(&original), "seed {seed}");
            assert_eq!(
                next_keys(&rebuilt.slots, 20),
                next_keys(&original, 20),
                "seed {seed}"
            );
            assert_eq!(record(&rebuilt.slots), saved, "seed {seed}");
            for (key, _) in &original {
                assert_eq!(rebuilt.key(SlotKey::of(key)), Some(key), "seed {seed}");
            }
        }
    }

    #[test]
    fn the_free_slots_are_reused_most_recently_freed_first() {
        let mut slots: SlotMap<TestKey, u32> = SlotMap::with_key();
        let keys: Vec<TestKey> = (0..4).map(|value| slots.insert(value)).collect();
        slots.remove(keys[1]);
        slots.remove(keys[3]);
        slots.remove(keys[0]);

        let saved = record(&slots);

        let vacant: Vec<(u32, u32)> = saved
            .vacant
            .iter()
            .map(|key| (key.index, key.version))
            .collect();
        assert_eq!(vacant, [(1, 2), (4, 2), (2, 2)]);
        assert_eq!(saved.entries.len(), 1);
        assert_eq!(
            saved.entries[0].key,
            SlotKey {
                index: 3,
                version: 1
            }
        );
    }

    #[test]
    fn a_key_names_only_its_occupied_slot_at_its_version() {
        let mut slots: SlotMap<TestKey, u32> = SlotMap::with_key();
        let first = slots.insert(1);
        slots.remove(first);
        let second = slots.insert(2);
        let rebuilt =
            rebuild::<TestKey, u32>(record(&slots), "test", || STAND_IN).expect("rebuilds");

        assert_eq!(rebuilt.key(SlotKey::of(second)), Some(second));
        assert_eq!(rebuilt.key(SlotKey::of(first)), None, "an older version");
        assert_eq!(
            rebuilt.key(SlotKey {
                index: 9,
                version: 1
            }),
            None
        );
        assert_eq!(
            rebuilt.key(SlotKey {
                index: 0,
                version: 1
            }),
            None
        );
    }

    #[test]
    fn faulty_records_are_refused() {
        let key = |index, version| SlotKey { index, version };
        let entry = |index, version| SlotEntry {
            key: key(index, version),
            value: 7,
        };
        let cases = [
            (vec![entry(1, 2)], vec![], "slot 1 is occupied at version 2"),
            (
                vec![entry(1, 1)],
                vec![key(2, 3)],
                "slot 2 is vacant at version 3",
            ),
            (vec![entry(1, 1), entry(1, 3)], vec![], "slot 1 is out of"),
            (
                vec![entry(2, 1)],
                vec![],
                "slot 2 is out of the slots 1 to 1",
            ),
            (vec![entry(0, 1)], vec![], "slot 0 is out of"),
            (
                vec![entry(1, MAX_SLOT_VERSION + 1)],
                vec![],
                "versions go from 1 to",
            ),
            (
                vec![entry(1, 1)],
                vec![key(2, 0)],
                "slot 2 is vacant at version 0",
            ),
            (
                vec![],
                (1..=1025).map(|index| key(index, 2)).collect(),
                "at most 1024",
            ),
        ];
        for (entries, vacant, expected) in cases {
            let saved = SlotsRecord { entries, vacant };

            let error = rebuild::<TestKey, u32>(saved, "test", || STAND_IN).expect_err(expected);

            assert!(
                matches!(&error, SaveError::Inconsistent(problem) if problem.contains(expected)),
                "expected `{expected}`, got {error}"
            );
        }
    }
}
