//! What the renderer is to be told of the occupancy of bricks (which voxels block the sun: `voxel_meshing::BrickOccupancy`), kept apart from the meshes.
//!
//! A mesher makes it in the job that makes a brick's mesh, for every brick it meshes, the buried ones and the ones of sky too (those have an empty mesh and so no
//! [`crate::MeshUpdate`]). It is a stream of its own, drained with `take_occupancy_updates`, for two reasons: a brick with nothing to draw still has something to say, and
//! every program that matches on `MeshUpdate` would have to know a variant it has nothing to do with. It is off until a program asks for it (`enable_occupancy`), and costs
//! nothing while it is off.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use voxel_meshing::BrickOccupancy;

/// What a brick's occupancy is now, for the renderer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OccupancyChange {
    /// The brick's occupancy is this, replacing what there was.
    Set(BrickOccupancy),
    /// The brick is no longer known (it is out of range, or the mesher was reset).
    Clear,
}

/// How many of the bricks the renderer has been told of are of each kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OccupancyStatistics {
    pub empty: usize,
    pub full: usize,
    pub mixed: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    Empty,
    Full,
    Mixed,
}

/// The changes to the occupancy of the bricks of a mesher (keyed by brick, or by ship and brick), and what the renderer has been told, so that only a change is sent: a brick
/// is meshed again whenever a neighbour changes, and nearly always to the same occupancy.
///
/// What was sent is kept as a 64-bit digest and not as the occupancy (4 KB a brick): two occupancies that differ and have the same digest would be taken for the same, and
/// the second not sent, which is a chance of about 2^-64 for each pair.
#[derive(Debug)]
pub(crate) struct OccupancyLog<K> {
    enabled: bool,
    sent: HashMap<K, (u64, Class)>,
    pending: Vec<(K, OccupancyChange)>,
}

impl<K: Copy + Eq + Hash + Ord> OccupancyLog<K> {
    pub(crate) fn new() -> Self {
        Self {
            enabled: false,
            sent: HashMap::new(),
            pending: Vec::new(),
        }
    }

    pub(crate) fn enable(&mut self) {
        self.enabled = true;
    }

    pub(crate) fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Records that the brick's occupancy is this, if that is a change.
    pub(crate) fn set(&mut self, key: K, occupancy: BrickOccupancy) {
        if !self.enabled {
            return;
        }
        let mut hasher = DefaultHasher::new();
        occupancy.hash(&mut hasher);
        let digest = hasher.finish();
        let class = match &occupancy {
            BrickOccupancy::Empty => Class::Empty,
            BrickOccupancy::Full => Class::Full,
            BrickOccupancy::Mixed(_) => Class::Mixed,
        };
        if self.sent.get(&key) == Some(&(digest, class)) {
            return;
        }
        self.sent.insert(key, (digest, class));
        self.pending.push((key, OccupancyChange::Set(occupancy)));
    }

    /// Records that the brick is no longer known, if the renderer was told of it.
    pub(crate) fn clear(&mut self, key: K) {
        if self.sent.remove(&key).is_some() {
            self.pending.push((key, OccupancyChange::Clear));
        }
    }

    /// Records that no brick is known any more, in the order of the keys.
    pub(crate) fn clear_all(&mut self) {
        let mut keys: Vec<K> = self.sent.drain().map(|(key, _)| key).collect();
        keys.sort();
        self.pending
            .extend(keys.into_iter().map(|key| (key, OccupancyChange::Clear)));
    }

    /// Forgets what was sent for the keys that satisfy `forgotten`, without a change to send: whoever holds them drops them whole (a ship that is gone).
    pub(crate) fn forget_where(&mut self, forgotten: impl Fn(&K) -> bool) {
        self.sent.retain(|key, _| !forgotten(key));
        self.pending.retain(|(key, _)| !forgotten(key));
    }

    /// The changes since the last call, in the order they were made.
    pub(crate) fn take(&mut self) -> Vec<(K, OccupancyChange)> {
        std::mem::take(&mut self.pending)
    }

    pub(crate) fn statistics(&self) -> OccupancyStatistics {
        let mut statistics = OccupancyStatistics::default();
        for (_, class) in self.sent.values() {
            match class {
                Class::Empty => statistics.empty += 1,
                Class::Full => statistics.full += 1,
                Class::Mixed => statistics.mixed += 1,
            }
        }
        statistics
    }
}

#[cfg(test)]
mod tests {
    use voxel_meshing::OCCUPANCY_WORDS;

    use super::*;

    fn mixed(bit: usize) -> BrickOccupancy {
        let mut words = Box::new([0u32; OCCUPANCY_WORDS]);
        words[bit / 32] |= 1 << (bit % 32);
        BrickOccupancy::from_words(words)
    }

    fn log() -> OccupancyLog<u32> {
        let mut log = OccupancyLog::new();
        log.enable();
        log
    }

    #[test]
    fn a_log_that_was_not_enabled_records_and_counts_nothing() {
        let mut log: OccupancyLog<u32> = OccupancyLog::new();
        // (What a mesher asks before it makes the occupancy of a brick at all: a log that is off costs nothing.)
        assert!(!log.is_enabled());
        log.set(1, BrickOccupancy::Full);
        log.clear(1);
        assert!(log.take().is_empty());
        assert_eq!(log.statistics(), OccupancyStatistics::default());
    }

    #[test]
    fn only_a_change_is_sent_and_a_clear_only_for_what_was_sent() {
        let mut log = log();
        log.set(1, BrickOccupancy::Full);
        log.set(1, BrickOccupancy::Full);
        log.set(2, mixed(5));
        log.set(2, mixed(5));
        assert_eq!(
            log.take(),
            [
                (1, OccupancyChange::Set(BrickOccupancy::Full)),
                (2, OccupancyChange::Set(mixed(5)))
            ]
        );
        // The same again is nothing; a different one replaces; a change of class is a change.
        log.set(2, mixed(5));
        assert!(log.take().is_empty());
        log.set(2, mixed(6));
        log.set(1, BrickOccupancy::Empty);
        assert_eq!(
            log.take(),
            [
                (2, OccupancyChange::Set(mixed(6))),
                (1, OccupancyChange::Set(BrickOccupancy::Empty))
            ]
        );
        log.clear(3);
        log.clear(1);
        log.clear(1);
        assert_eq!(log.take(), [(1, OccupancyChange::Clear)]);
        // A cleared brick that comes back is sent again, though it is what it was.
        log.set(1, BrickOccupancy::Empty);
        assert_eq!(
            log.take(),
            [(1, OccupancyChange::Set(BrickOccupancy::Empty))]
        );
    }

    #[test]
    fn clearing_all_clears_each_brick_that_was_sent_in_order_and_forgetting_sends_nothing() {
        let mut log = log();
        for key in [5, 3, 9, 1] {
            log.set(key, BrickOccupancy::Full);
        }
        log.take();
        log.clear_all();
        assert_eq!(
            log.take(),
            [
                (1, OccupancyChange::Clear),
                (3, OccupancyChange::Clear),
                (5, OccupancyChange::Clear),
                (9, OccupancyChange::Clear)
            ]
        );
        assert_eq!(log.statistics(), OccupancyStatistics::default());
        log.set(4, BrickOccupancy::Full);
        log.set(6, mixed(1));
        log.forget_where(|key| *key == 4);
        // Nothing is sent for the forgotten one, neither its set (not yet taken) nor a clear.
        assert_eq!(log.take(), [(6, OccupancyChange::Set(mixed(1)))]);
        log.clear(4);
        assert!(log.take().is_empty());
    }

    #[test]
    fn the_statistics_count_what_the_renderer_holds_by_kind() {
        let mut log = log();
        log.set(1, BrickOccupancy::Empty);
        log.set(2, BrickOccupancy::Empty);
        log.set(3, BrickOccupancy::Full);
        log.set(4, mixed(0));
        log.set(5, mixed(1));
        log.set(5, BrickOccupancy::Full);
        log.clear(2);
        assert_eq!(
            log.statistics(),
            OccupancyStatistics {
                empty: 1,
                full: 2,
                mixed: 1
            }
        );
    }
}
