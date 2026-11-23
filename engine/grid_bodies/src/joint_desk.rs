//! What a command that parts two coupled cars and the plugin that couples them tell each other (`docs/decisions/joints-and-cars.md`, "The command").
//!
//! The plugin that moves the ships lives behind a lock of its own and a command cannot ask it anything, as the command for docking asks [`crate::Docking`], which both share. The
//! joint desk is what the two share for joints: **the pivots of the couplings** (the plugin tells them in each tick, so that a command can say which coupling is the nearest the
//! player and answer at once), **the requests** to part a pair (the plugin takes them at the start of its next tick), and **the pairs held apart**, which are not drawn together
//! or coupled again while their ends are where they were, as the ships a docking command cut apart are not docked again (`Docking::released`). And **the locks that are wanted**: a
//! player asked for two coupled cars to be turned into line and made one ship, which the plugin does when it can, and goes on trying until it has, the pair is parted or the lock is
//! taken back; a lock that is wanted is told to the plugin by the desk and not taken by it, as a request is. A clone is the same desk.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};

use voxel_space::{GridId, LevelIndex, WorldPosition};

/// A pair of coupled cars, the lower identifier first.
pub type CoupledPair = (GridId, GridId);

#[derive(Debug, Default)]
struct Desk {
    pivots: BTreeMap<CoupledPair, (LevelIndex, WorldPosition)>,
    requests: BTreeSet<CoupledPair>,
    held_apart: BTreeSet<CoupledPair>,
    locks_wanted: BTreeSet<CoupledPair>,
}

/// The record that a command for joints and the plugin that couples cars share (see the module).
#[derive(Clone, Debug, Default)]
pub struct JointDesk {
    desk: Arc<Mutex<Desk>>,
}

impl JointDesk {
    fn desk(&self) -> MutexGuard<'_, Desk> {
        self.desk
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Tells where the pivot of each coupling there is now is, the places of the last tick replaced (the plugin does this in every tick).
    pub fn set_pivots(&self, pivots: BTreeMap<CoupledPair, (LevelIndex, WorldPosition)>) {
        self.desk().pivots = pivots;
    }

    /// The couplings in `level` whose pivots are within `reach` voxels of `place`, nearest first (a tie is the pair that is first in order), each with how far its pivot is.
    pub fn couplings_within(
        &self,
        level: LevelIndex,
        place: WorldPosition,
        reach: f64,
    ) -> Vec<(f64, CoupledPair)> {
        let desk = self.desk();
        let mut near: Vec<(f64, CoupledPair)> = desk
            .pivots
            .iter()
            .filter(|(_, (pivot_level, _))| *pivot_level == level)
            .map(|(pair, (_, pivot))| {
                let arm = pivot.relative_to(place);
                (
                    arm.iter().map(|part| part * part).sum::<f64>().sqrt(),
                    *pair,
                )
            })
            .filter(|(distance, _)| *distance <= reach)
            .collect();
        // (`pivots` is in order of pair, and the sort is stable.)
        near.sort_by(|(near, _), (far, _)| near.total_cmp(far));
        near
    }

    /// Asks for a pair to be parted: the plugin lets go of their coupling in its next tick and holds them apart (see [`JointDesk::is_held_apart`]). A lock that was wanted of the pair is
    /// not wanted any more.
    pub fn decouple(&self, pair: CoupledPair) {
        let mut desk = self.desk();
        desk.locks_wanted.remove(&pair);
        desk.requests.insert(pair);
    }

    /// Asks for a pair to be locked: the plugin turns the two cars into line and makes them one ship, and goes on trying until it is done, the pair is parted or the lock is
    /// [cancelled](JointDesk::cancel_lock).
    pub fn lock(&self, pair: CoupledPair) {
        self.desk().locks_wanted.insert(pair);
    }

    /// Whether a lock is wanted of a pair.
    pub fn is_lock_wanted(&self, pair: CoupledPair) -> bool {
        self.desk().locks_wanted.contains(&pair)
    }

    /// The pairs that are wanted locked, in order of pair (asking does not take them).
    pub fn locks_wanted(&self) -> BTreeSet<CoupledPair> {
        self.desk().locks_wanted.clone()
    }

    /// Takes back the lock that is wanted of a pair, if one is.
    pub fn cancel_lock(&self, pair: CoupledPair) {
        self.desk().locks_wanted.remove(&pair);
    }

    /// Keeps the locks wanted of the pairs that `still_there` says are, and takes back the rest.
    pub fn keep_locks_only(&self, still_there: impl Fn(CoupledPair) -> bool) {
        self.desk().locks_wanted.retain(|pair| still_there(*pair));
    }

    /// The requests made since they were last taken, in order of pair; they are not asked for again.
    pub fn take_requests(&self) -> BTreeSet<CoupledPair> {
        std::mem::take(&mut self.desk().requests)
    }

    /// A pair that is to be neither drawn together nor coupled until [`JointDesk::let_go_of`] it.
    pub fn hold_apart(&self, pair: CoupledPair) {
        self.desk().held_apart.insert(pair);
    }

    /// Whether a pair is held apart.
    pub fn is_held_apart(&self, pair: CoupledPair) -> bool {
        self.desk().held_apart.contains(&pair)
    }

    /// Lets a pair be drawn together and coupled again.
    pub fn let_go_of(&self, pair: CoupledPair) {
        self.desk().held_apart.remove(&pair);
    }

    /// Whether any pair is held apart (asked of every ship pair in every tick, so cheap).
    pub(crate) fn holds_any_apart(&self) -> bool {
        !self.desk().held_apart.is_empty()
    }

    /// Keeps the pairs held apart that `still_there` says are, and lets go of the rest.
    pub(crate) fn keep_held_apart_only(&self, still_there: impl Fn(CoupledPair) -> bool) {
        self.desk().held_apart.retain(|pair| still_there(*pair));
    }
}
