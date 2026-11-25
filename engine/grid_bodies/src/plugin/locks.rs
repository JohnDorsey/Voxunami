//! Locking: two cars that are coupled by their joint ends are turned into line and made one ship, when a player asks for it (`/joint lock`, `docs/decisions/joints-and-cars.md`: *dock and
//! lock merge grids, and nothing else does*). The ends of a coupling are together to within the coupling distance, and a ball joint turns however it is, so what a lock does is the part
//! that docking does: **turn the cars** toward the line-up (the turn of the lattice that is nearest, the quarter turns about the axis of the ends being the same line-up to a car with
//! the blocks of one cube for cube), and **snap and merge** when the ends are within the docking snap (0.3 metres, 10 degrees), the ship that stays and the ship that moves, and the motion of
//! the whole, being those of a dock over the rails (`who_stays_when_joined`).
//!
//! The ask is the joint desk's (`JointDesk::lock`) and lasts until it succeeds, the pair is parted (a command, or a coupling that is gone), or a player takes it back. **The turning is
//! before the step** (a torque on each car, equal and opposite, as docking's twist is, so the momentum is what it was: the cars turn about the ball as they like); **the merge is after
//! it**, when the world has every car at the place of the end of the tick, as a parting is (`couplings.rs`): the cars of a group on a rail are taken off it where they are (they are fixed
//! bodies on it, and are not turned), and the ship that is made goes on rails again by the quiet rule. A merge that is refused (a block of one car would be in the place of a block of
//! the other) is tried again only now and then, as docking's is (`REFUSED_RETRY_TICKS`).
//!
//! The ship that is made is a new one, and the cars that were coupled to either of the two are coupled to it by the rule that couples ends that are together, in a tick or two.

use physics::{BodyId, BodyKind};
use server::ServerWorld;
use voxel_space::GridId;

use super::couplings::Coupled;
use super::{Bodies, Joining, REFUSED_RETRY_TICKS};
use crate::docking::{Approach, BlockFaceView, approach};

/// How the ends of a coupling are placed to each other, as the world has the two cars.
fn how_the_ends_are_placed(
    world: &ServerWorld,
    pair: (GridId, GridId),
    coupled: &Coupled,
) -> Option<Approach> {
    let grids = world.data().grids();
    let (first, second) = (grids.get(pair.0)?, grids.get(pair.1)?);
    let from = first.pose.position;
    let first_view = BlockFaceView::of(coupled.first_end, &first.pose, from);
    let second_view = BlockFaceView::of(coupled.second_end, &second.pose, from);
    Some(approach(&first_view, &second_view))
}

impl Bodies {
    /// Before the step: each pair of coupled cars that a player asked to have locked, and that are not in line yet, is given the torque that turns them toward it (docking's twist: the
    /// second car about the axis that brings it into line and the first about the opposite, whatever each weighs). A car that cannot move is not turned.
    pub(super) fn turn_the_cars_wanted_locked(&mut self, world: &ServerWorld) {
        let Some(docking) = self.config.docking.clone() else {
            return;
        };
        for pair in self.config.joint_desk.locks_wanted() {
            let Some(coupled) = self.couplings.get(&pair).copied() else {
                continue;
            };
            let Some(how) = how_the_ends_are_placed(world, pair, &coupled) else {
                continue;
            };
            // (In line already: the merge, after the step, is what is left.)
            if docking.settings.can_snap(&how) {
                continue;
            }
            let twist = docking.settings.twist(&how);
            let Some(frame) = self.frames.get_mut(&coupled.frame) else {
                continue;
            };
            for (id, sign) in [(pair.0, -1.0), (pair.1, 1.0)] {
                if frame
                    .ships
                    .get(&id)
                    .is_none_or(|record| record.kind == BodyKind::Fixed)
                {
                    continue;
                }
                let _ = frame
                    .bodies
                    .apply_torque(BodyId(id.get()), twist.map(|component| component * sign));
            }
        }
    }

    /// After the step: the pairs that a player asked to have locked and that are in line (within the docking snap) are made one ship, the car that is held or rides a rail or is heavier
    /// staying and the other put exactly where they join. A lock is wanted only of a pair that is coupled: one that is not any more is forgotten. A group that rides a rail is taken
    /// off it, where it is, to be turned (its cars are fixed bodies on the rail).
    pub(super) fn lock_the_cars_that_are_in_line(&mut self, world: &mut ServerWorld) {
        let Some(docking) = self.config.docking.clone() else {
            return;
        };
        let desk = self.config.joint_desk.clone();
        desk.keep_locks_only(|pair| self.couplings.contains_key(&pair));
        for pair in desk.locks_wanted() {
            let Some(coupled) = self.couplings.get(&pair).copied() else {
                continue;
            };
            if let Some(anchor) = world
                .rides_a_rail_in_a_group(pair.0)
                .or_else(|| world.rides_a_rail_in_a_group(pair.1))
            {
                world.take_group_off_its_rail(anchor);
            }
            let Some(how) = how_the_ends_are_placed(world, pair, &coupled) else {
                continue;
            };
            if !docking.settings.can_snap(&how) || self.refused.contains_key(&pair) {
                continue;
            }
            // (A planet stays whatever it weighs, and the car is written into it: the planet keeps its identifier, `write_ship_into_planet`.)
            let planet_side = self.frames.get(&coupled.frame).and_then(|frame| {
                [
                    (pair.0, coupled.first_end, pair.1, coupled.second_end),
                    (pair.1, coupled.second_end, pair.0, coupled.first_end),
                ]
                .into_iter()
                .find(|(id, ..)| frame.planets.contains_key(id))
            });
            if let Some((planet, planet_end, car, car_end)) = planet_side {
                if self.write_ship_into_planet(world, (planet, planet_end), (car, car_end)) {
                    self.count(|report| report.locks_made += 1);
                } else {
                    self.refused.insert(pair, self.tick + REFUSED_RETRY_TICKS);
                    self.count(|report| report.locks_refused += 1);
                }
                continue;
            }
            let Some(join) = self.who_stays_when_joined(
                world,
                (pair.0, coupled.first_end),
                (pair.1, coupled.second_end),
            ) else {
                continue;
            };
            match self.make_one_ship_of_two(world, join.still, join.moving, pair, join.motion) {
                // (The lock is forgotten with the coupling, which the ships that are gone let go of in the next tick: `keep_locks_only`.)
                Joining::Made => self.count(|report| report.locks_made += 1),
                Joining::Refused => self.count(|report| report.locks_refused += 1),
                Joining::NotTried => {}
            }
        }
    }
}
