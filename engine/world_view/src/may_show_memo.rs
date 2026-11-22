//! What the terrain said of whether a cube may show anything, kept from one working out of the cubes to the next, and for as many of them as there is memory for.
//!
//! The cubes of a view are worked out from these answers (a cube that is all air or buried is left out), and each answer for a big cube is a walk through the cubes in it. A view that
//! moves a brick asks about nearly the same cubes as the one before, and a view that goes away and comes back (from the ground to a map, and back) asks about the cubes it asked about
//! when it left. The answers are kept until the memory is full and the ones that were used longest ago go first, counting rounds (a round is one working out of the cubes) and never a
//! clock, so what is kept does not depend on how fast the machine is.
//!
//! Measured 2026-11-22 (`docs/specs/viewpoints-spec.md` 9g): a view from 264 km up back to the ground asked the terrain about 32,338 of the 33,030 cubes it worked out, because the
//! answers of the ground were forgotten by the third working out on the way up, and that was 25 of the 38 ms it took to choose them.

use std::collections::HashMap;

use region_octree::BrickCube;

/// How many answers are kept: two views' worth of a view that reaches far (about 33,000 are asked about from 264 km up, and a hundred thousand in one to 100 million voxels), about 64
/// bytes each.
pub(crate) const MAY_SHOW_MEMORY: usize = 1 << 18;

#[derive(Clone, Copy)]
struct Answer {
    may_show: bool,
    /// The round (see [`MayShowMemo::finish_round`]) the answer was last asked for in.
    last_used: u64,
}

pub(crate) struct MayShowMemo {
    /// The version of the terrain ([`crate::lod::CoarseTerrainSource::version`]) the answers are for.
    version: u64,
    /// The round that is going on: how many rounds have ended.
    round: u64,
    answers: HashMap<BrickCube, Answer>,
    capacity: usize,
}

impl Default for MayShowMemo {
    fn default() -> Self {
        Self::with_capacity(MAY_SHOW_MEMORY)
    }
}

impl MayShowMemo {
    /// A memo that keeps about `capacity` answers (more by at most one round, which is never forgotten in part).
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            version: 0,
            round: 0,
            answers: HashMap::new(),
            capacity,
        }
    }

    /// The kept answer for `cube`, or the one `ask` gives (which is then kept). Either way the answer is as new as this round.
    pub(crate) fn get_or_ask(&mut self, cube: BrickCube, ask: impl FnOnce() -> bool) -> bool {
        if let Some(answer) = self.answers.get_mut(&cube) {
            answer.last_used = self.round;
            return answer.may_show;
        }
        let may_show = ask();
        self.answers.insert(
            cube,
            Answer {
                may_show,
                last_used: self.round,
            },
        );
        may_show
    }

    /// Ends a round, as the cubes of a working out are done. If more answers are kept than there is memory for, those used longest ago are forgotten, down to three quarters of the
    /// memory so that it is not done at every round, and always whole rounds: what was used in the round that has just ended is kept.
    pub(crate) fn finish_round(&mut self) {
        self.round += 1;
        if self.answers.len() > self.capacity {
            self.forget_the_oldest();
        }
    }

    fn forget_the_oldest(&mut self) {
        let keep = (self.capacity * 3 / 4).max(1);
        let mut rounds: Vec<u64> = self
            .answers
            .values()
            .map(|answer| answer.last_used)
            .collect();
        // The round of the `keep`-th newest answer: every answer of it or of a newer round stays.
        let (_, &mut oldest_kept, _) = rounds.select_nth_unstable_by(keep - 1, |a, b| b.cmp(a));
        self.answers
            .retain(|_, answer| answer.last_used >= oldest_kept);
    }

    /// Forgets everything if the terrain is not the version the answers are for.
    pub(crate) fn keep_for(&mut self, version: u64) {
        if self.version != version {
            self.clear();
            self.version = version;
        }
    }

    pub(crate) fn clear(&mut self) {
        self.answers.clear();
    }

    /// How many answers are kept.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.answers.len()
    }
}

#[cfg(test)]
mod tests;
