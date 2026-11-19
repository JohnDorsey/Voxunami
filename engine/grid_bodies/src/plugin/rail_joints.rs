//! Joint ends that pull on each other when a ship that has one **rides a rail, alone or in a group** (step 4c-5-4b of `docs/decisions/joints-and-cars.md`): the pull is a force on the rail
//! and a torque on the bodies, so it never takes the ship, or its group, off its rail; **only the coupling, which is a touch** (the owner, 2026-11-19: "a coupling should be a touch, as a
//! dock is"), does. At the moment two ends are at one place, in line and slow enough to be coupled (`JointSettings::can_couple`), both ships (or groups) are taken off their rails where
//! they are, and the bodies couple them in the tick after, between ships of one frame, as they always did (`couplings.rs`).
//!
//! The pull is `rail_ports.rs`'s for joint ends, and differs from it in two ways:
//!
//! - **It is asked after the step**, where the world has every ship at the place of the end of the tick, and not before it. A pull of a joint depends on how fast the two ends go against
//!   each other (it is asked to arrive slowly: `JointSettings::pull_on_second`), and a ship that the rails place is a tick ahead of one that is free while the bodies are about to step
//!   (at orbital speed a tick of the orbit's turn is 0.4 metres a second, sideways to the ends, and the pull would work that off as if it were real: a drift of the ends that never
//!   arrive). After the step the places and the speeds are of one instant, and what is asked now is done in the next tick by the bodies (a free ship's force) and at the end of this one by
//!   the world (a rail's, `ServerWorld::update_coasting`, from this instant on): the same instant for both.
//! - **The pull is of groups.** A group is pulled as one (the mass it is pulled as is the mass of all its ships, as in `couplings.rs`), by the nearest group that has ends that pull on its
//!   own, one at a time.
//!
//! **Two ships of one group are not a pair here** (they are coupled already), and a pair of which neither rides a rail is the bodies': `couple_and_pull_in`, in one frame, as it always
//! was. A pair of which one does is left out of it (`is_a_rail_pair`): the ship on a rail is a fixed body or has a frame of its own, and the pull is asked once.

use std::collections::BTreeSet;

use physics::{BodyKind, MassMoments};
use server::ServerWorld;
use voxel_space::GridId;
use world_storage::MovingGrid;

use super::rail_ports::{are_of_one_group, rides_a_rail};
use super::{Bodies, Ship};
use crate::docking::{Approach, BlockFace, BlockFaceView, approach, cross, dot, length, turn};
use crate::joints::{JointSettings, effective_mass};

/// Two ships, one of which at least rides a rail, with ends that pull on each other: the closest such pair of ends, and what the rules need to know of how the ships go.
struct RailJointPair {
    first: GridId,
    second: GridId,
    first_view: BlockFaceView,
    second_view: BlockFaceView,
    how: Approach,
    /// Where each end is from the ship's centre of mass, in the world's axes.
    first_arm: [f64; 3],
    second_arm: [f64; 3],
    /// How fast the second end goes against the first, and how fast the second ship turns against the first.
    relative_velocity: [f64; 3],
    relative_spin: [f64; 3],
}

/// What a ship that has joint ends is, to the pairs.
struct Bearer<'a> {
    id: GridId,
    ship: &'a MovingGrid,
    record: &'a Ship,
    rides_a_rail: bool,
}

impl Bodies {
    /// For every two ships that have joint ends that pull on each other (the closest pair of ends of the two), of which one at least rides a rail, alone or in a group, and which are not
    /// of one group, in order of ship (the lower identifier first), as the world has them: asked after the step. Ships that are frozen and ships in another level have none.
    fn railed_joint_pairs(
        &self,
        world: &ServerWorld,
        settings: &JointSettings,
    ) -> Vec<RailJointPair> {
        let grids = world.data().grids();
        let mut bearers: Vec<Bearer<'_>> = Vec::new();
        for frame in self.frames.values() {
            for (id, record) in &frame.ships {
                // (Most ships have no joint ends: this is asked of every ship in every tick.)
                if record.joint_ends.is_empty() {
                    continue;
                }
                let Some(ship) = grids.get(*id) else {
                    continue;
                };
                if ship.frozen {
                    continue;
                }
                bearers.push(Bearer {
                    id: *id,
                    ship,
                    record,
                    rides_a_rail: rides_a_rail(world, *id),
                });
            }
        }
        if !bearers.iter().any(|bearer| bearer.rides_a_rail) {
            return Vec::new();
        }
        bearers.sort_by_key(|bearer| bearer.id);
        let mut pairs = Vec::new();
        for (index, first) in bearers.iter().enumerate() {
            for second in &bearers[index + 1..] {
                if !(first.rides_a_rail || second.rides_a_rail)
                    || first.ship.level != second.ship.level
                    || are_of_one_group(world, first.id, second.id)
                {
                    continue;
                }
                if let Some(pair) = closest_pulling_ends(settings, first, second) {
                    pairs.push(pair);
                }
            }
        }
        pairs
    }

    /// What a pull on this ship moves: the mass of its group, if the group can move (a group that rides a rail does; one that is free does if none of its ships is held).
    fn mass_a_pull_moves(&self, world: &ServerWorld, ship: GridId) -> Option<f64> {
        let members = world.data().grids().group_of(ship);
        let records: Vec<&Ship> = members
            .iter()
            .map(|member| self.record_of(*member))
            .collect::<Option<_>>()?;
        if !rides_a_rail(world, ship) && records.iter().any(|record| record.kind == BodyKind::Fixed)
        {
            return None;
        }
        Some(records.iter().map(|record| record.mass).sum())
    }

    /// What the end of a ship answers a force along `direction` with, as a mass: the ship's own mass and what its lever adds (a force at the end of a ship that is away from its centre
    /// of mass turns it, and the end goes faster than the mass alone says: `1/m + (arm x direction)^2 / I`, the inertia as a number as `inertia_a_pull_turns` has it). `None` for a
    /// ship that a pull cannot move (`mass_a_pull_moves`).
    ///
    /// **This is not the mass the free pull is sized by** (the mass of the group, `couplings.rs`): the pull asks for a force of that mass times the error in the speed over a tenth of a second,
    /// cut to 10 kilonewtons, and a car whose end is a metre or two from its centre answers it several times harder than its mass says, so the speed of the ends overshoots every tick
    /// and settles to no less than a metre a second or so (measured 2026-11-19 on a chain's end car: 1.0 to 1.5 against the 1 the coupling asks), and a pair never couples if that is
    /// all there is. The end's own mass is the one that makes the pull settle.
    fn mass_at_the_end(
        &self,
        world: &ServerWorld,
        ship: GridId,
        arm: [f64; 3],
        direction: [f64; 3],
    ) -> Option<f64> {
        self.mass_a_pull_moves(world, ship)?;
        let record = self.record_of(ship)?;
        let inertia = self.inertia_a_pull_turns(world, ship)?;
        let lever = cross(arm, direction);
        let inverse = 1.0 / record.mass + dot(lever, lever) / inertia;
        (inverse.is_finite() && inverse > 0.0).then(|| 1.0 / inverse)
    }

    /// The inertia that the spin of two ships against each other is slowed against: each one's own, about its centre of mass, as a number (a third of the trace of the tensor).
    fn inertia_a_pull_turns(&self, world: &ServerWorld, ship: GridId) -> Option<f64> {
        self.mass_a_pull_moves(world, ship)?;
        let record = self.record_of(ship)?;
        let tensor = MassMoments::sum(record.moments.values().copied())
            .properties()?
            .inertia;
        Some((tensor[0][0] + tensor[1][1] + tensor[2][2]) / 3.0)
    }

    /// Pulls the joint ends of ships of which one rides a rail, or couples them: the pairs that are in line and slow enough to be coupled (`JointSettings::can_couple`) take their
    /// ships, and the groups of them, **off their rails** where they are (the coupling is a touch: the bodies make it in the tick after, between ships of one frame), and each of the
    /// others is pulled, nearest first, each group by one other at most (as `couple_and_pull_in` does it): the force at the centre of mass of each ship, which the world gives the rail
    /// or the body, and the torque (the lever of the end, and the spin of the ships against each other slowed), which is the bodies'. It is asked **after the step** (see the module).
    pub(super) fn couple_or_pull_joints_of_ships_on_rails(&mut self, world: &mut ServerWorld) {
        let Some(settings) = self.config.joints else {
            return;
        };
        let mut pairs = self.railed_joint_pairs(world, &settings);
        if pairs.is_empty() {
            return;
        }
        pairs.sort_by(|a, b| {
            a.how
                .distance
                .total_cmp(&b.how.distance)
                .then(a.first.cmp(&b.first))
                .then(a.second.cmp(&b.second))
        });
        let name_of = |ship: GridId| {
            world
                .data()
                .grids()
                .group_of(ship)
                .first()
                .copied()
                .unwrap_or(ship)
        };
        let mut busy: BTreeSet<GridId> = BTreeSet::new();
        let mut touching: BTreeSet<GridId> = BTreeSet::new();
        let mut pulls: Vec<(GridId, [f64; 3], [f64; 3])> = Vec::new();
        for pair in &pairs {
            let groups = [name_of(pair.first), name_of(pair.second)];
            if groups.iter().any(|group| busy.contains(group)) {
                continue;
            }
            // (The ends touch, and the bodies will couple them in the tick after: near enough, in line and slow enough, as a coupling asks. A hand-over any earlier would leave the pull to
            // the bodies', which size it by the mass of the group and not of the end, and do not settle a chain's end car: see `mass_at_the_end`.)
            if settings.can_couple(
                &pair.first_view,
                &pair.second_view,
                &pair.how,
                pair.relative_velocity,
                pair.relative_spin,
            ) {
                busy.extend(groups);
                for ship in [pair.first, pair.second] {
                    if rides_a_rail(world, ship) {
                        touching.insert(name_of(ship));
                    }
                }
                continue;
            }
            // The way the pull will point: the way of the error in the speed asked of the ends, whatever the mass (it only scales it).
            let Some(direction) =
                unit(settings.pull_on_second(pair.how.gap, pair.relative_velocity, 1.0))
            else {
                continue;
            };
            let Some(mass) = effective_mass(
                self.mass_at_the_end(world, pair.first, pair.first_arm, direction),
                self.mass_at_the_end(world, pair.second, pair.second_arm, direction),
            ) else {
                continue;
            };
            busy.extend(groups);
            let on_second = settings.pull_on_second(pair.how.gap, pair.relative_velocity, mass);
            let damping = effective_mass(
                self.inertia_a_pull_turns(world, pair.first),
                self.inertia_a_pull_turns(world, pair.second),
            )
            .map_or([0.0; 3], |inertia| {
                settings.spin_damping_on_second(pair.relative_spin, inertia)
            });
            for (ship, arm, sign) in [
                (pair.first, pair.first_arm, -1.0),
                (pair.second, pair.second_arm, 1.0),
            ] {
                if self.mass_a_pull_moves(world, ship).is_none() {
                    continue;
                }
                let force = on_second.map(|component| component * sign);
                let lever = cross(arm, force);
                let torque = [0, 1, 2].map(|axis| lever[axis] + damping[axis] * sign);
                pulls.push((ship, force, torque));
            }
        }
        for (ship, force, torque) in pulls {
            let _ = world.apply_grid_force(ship, force, None);
            // (No torque is no torque waiting: a ship alone on a rail is not given a body to turn for nothing.)
            if torque != [0.0; 3] {
                let _ = world.apply_grid_torque(ship, torque);
            }
        }
        for anchor in &touching {
            world.take_group_off_its_rail(*anchor);
        }
        if !touching.is_empty() {
            let left = touching.len() as u64;
            self.count(|report| report.rails_left_for_ends += left);
        }
    }
}

/// The closest pair of ends, an end of each of two ships, that pull on each other, if there is one: the ends and how the ships are placed and go. The first of those at the same
/// distance in order of brick and voxel is the one.
fn closest_pulling_ends(
    settings: &JointSettings,
    first: &Bearer<'_>,
    second: &Bearer<'_>,
) -> Option<RailJointPair> {
    let from = first.ship.pose.position;
    let mut best: Option<(BlockFace, BlockFace, BlockFaceView, BlockFaceView, Approach)> = None;
    for first_end in first.record.joint_ends.values().flatten() {
        let first_view = BlockFaceView::of(*first_end, &first.ship.pose, from);
        for second_end in second.record.joint_ends.values().flatten() {
            let second_view = BlockFaceView::of(*second_end, &second.ship.pose, from);
            let how = approach(&first_view, &second_view);
            if settings.pulls(&first_view, &second_view, &how)
                && best
                    .as_ref()
                    .is_none_or(|top| how.distance < top.4.distance)
            {
                best = Some((*first_end, *second_end, first_view, second_view, how));
            }
        }
    }
    let (first_end, second_end, first_view, second_view, how) = best?;
    // (The speed of an end is the centre of mass' and what the ship's turning adds at the end's distance from it: the speeds are the world's, which the bodies have written, and
    // the rails' for a ship that rides one.)
    let arm = |bearer: &Bearer<'_>, end: BlockFace| {
        let at = end.local_centre();
        turn(
            bearer.ship.pose.rotation_matrix(),
            [0, 1, 2].map(|axis| at[axis] - bearer.record.centre_of_mass[axis]),
        )
    };
    let (first_arm, second_arm) = (arm(first, first_end), arm(second, second_end));
    let velocity_at = |bearer: &Bearer<'_>, arm: [f64; 3]| {
        let spun = cross(bearer.ship.angular_velocity, arm);
        [0, 1, 2].map(|axis| bearer.ship.linear_velocity[axis] + spun[axis])
    };
    let (first_velocity, second_velocity) = (
        velocity_at(first, first_arm),
        velocity_at(second, second_arm),
    );
    Some(RailJointPair {
        first: first.id,
        second: second.id,
        first_view,
        second_view,
        how,
        first_arm,
        second_arm,
        relative_velocity: [0, 1, 2].map(|axis| second_velocity[axis] - first_velocity[axis]),
        relative_spin: [0, 1, 2]
            .map(|axis| second.ship.angular_velocity[axis] - first.ship.angular_velocity[axis]),
    })
}

/// A vector of length 1 in the direction of this one, if it has a direction.
fn unit(vector: [f64; 3]) -> Option<[f64; 3]> {
    let size = length(vector);
    (size.is_finite() && size > 0.0).then(|| vector.map(|part| part / size))
}
