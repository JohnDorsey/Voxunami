//! Docking ports that pull on each other when a ship that has one **rides a rail, alone or in a group** (step 4c-5-4b of `docs/decisions/joints-and-cars.md`): the pull is a force on the
//! rail and a torque on the bodies, so it never takes the ship, or its group, off its rail; only the dock, which is a touch, does (the owner's promise of 2026-11-09).
//!
//! Ships are pulled together, and docked, between ships of one frame (`port_pairs`, `attract`, `dock`), and a ship on a rail is in a frame of its own while another ship is near it
//! (or in none), so it was taken off its rail for any port that pulled on its own. These pairs are found over all the ships instead, wherever their bodies are. What differs from a pair
//! of ships in one frame:
//!
//! - **One instant.** The rails put a ship at its place of the end of the tick before anything else is done in it, and a ship that is free is at its place of the start until the bodies
//!   step it, so the places are not of one time while the bodies are about to step (at orbital speed a tick is 38 voxels, six times the range of a pull). The pull is worked out with a ship on
//!   a rail put back a tick of its speed (`lag`, as the rule for the ends of a group does it), and **the dock is made after the step**, when every ship is at the place of the end of the
//!   tick and no ship has to be put back.
//! - **The force goes where the ship is.** A ship that is free has the force in its body, as a ship in one frame has; a ship that rides a rail, alone or in a group, is asked of the world as
//!   an engine asks: `ServerWorld::apply_grid_force` at its centre of mass is a change of its rail (made at the end of the tick, with the speed it has then and the change exact; a group's
//!   rail is of the centre of mass of all its ships, and the bodies are given the force as well, for the turn and the swing of the joints), and the torque (the lever of the port and the
//!   twist that turns it into line) is the bodies', which give a ship alone a frame to turn in while the torque waits (`ServerWorld::has_a_torque_waiting`). So the two ships get the same
//!   force and the opposite one, each its own change, whatever each weighs.
//! - **A ship that rides a rail stays where it is** in the dock: it is the one that does not move (a ship that is held, which does not move either, comes before it), and the other is put
//!   where it docks and made one ship with it that goes as the two did together.
//!
//! **Two ships of one group are not a pair here** (a group is in one frame, and its ports pull as they always did, in `port_pairs`); **nor are the planets' ports** (a planet is held, and
//! a ship on a rail docks with one as before). **Joint ends are not here either**: a coupling is made between ships of one body world, so a ship, or a group, on a rail is still taken off it
//! for a joint end that pulls on its own (`ends_pull_on_the_group`).

use std::collections::BTreeSet;

use physics::BodyKind;
use server::ServerWorld;
use voxel_space::{GridId, GridPose, WorldPosition};

use super::alone::pose_at_the_start;
use super::{
    Bodies, Ports, Ship, closest_pulling_ports, joined_motion, motion_of_a_ship, pull_and_turn,
};
use crate::docking::{Approach, BlockFace, Docking};

/// A pair of ports of two ships, one of which at least rides a rail (alone or in a group), that pull on each other: the closest such pair of ports of the two.
pub(super) struct RailPortPair {
    /// The ship with the lower identifier, and the other.
    first: GridId,
    second: GridId,
    first_port: BlockFace,
    second_port: BlockFace,
    approach: Approach,
}

/// What a ship that has ports is, to the pairs: where it is at the instant the pairs are asked about, its ports, and whether it rides a rail, alone or in a group.
struct Bearer<'a> {
    id: GridId,
    pose: GridPose,
    ports: &'a Ports,
    rides_a_rail: bool,
}

/// Whether this ship rides a rail that a push leaves it on: alone (`ServerWorld::rides_a_rail_alone`) or in a group (`ServerWorld::rides_a_rail_in_a_group`).
fn rides_a_rail(world: &ServerWorld, ship: GridId) -> bool {
    world.rides_a_rail_alone(ship) || world.rides_a_rail_in_a_group(ship).is_some()
}

/// Whether two ships are of one group (coupled by joint ends, directly or through others).
fn are_of_one_group(world: &ServerWorld, first: GridId, second: GridId) -> bool {
    let grids = world.data().grids();
    grids.has_partners(first) && grids.group_of(first).contains(&second)
}

/// Whether the pair of ports of these two ships is a pair of [`Bodies::railed_port_pairs`] and not of `port_pairs`: a ship of the two rides a rail, alone or in a group, and they are
/// not of one group.
pub(super) fn is_a_rail_pair(world: &ServerWorld, first: GridId, second: GridId) -> bool {
    (rides_a_rail(world, first) || rides_a_rail(world, second))
        && !are_of_one_group(world, first, second)
}

impl Bodies {
    /// For every two ships that have ports that pull on each other (the closest pair of ports of the two), of which one at least rides a rail, alone or in a group
    /// (`ServerWorld::rides_a_rail_alone`, `ServerWorld::rides_a_rail_in_a_group`), in order of ship (the lower identifier first). **`lag`** is the seconds that a ship that the rails place
    /// is ahead of the others, as for `ends_pull_on_the_group`: a tick before the bodies step, none after. Ships that are frozen, two ships of one group, ships in another level, pairs that
    /// were undocked and have not yet come apart, and pairs that could not be docked where they are, have none.
    pub(super) fn railed_port_pairs(
        &self,
        world: &ServerWorld,
        docking: &Docking,
        lag: f64,
    ) -> Vec<RailPortPair> {
        let grids = world.data().grids();
        let mut bearers: Vec<Bearer<'_>> = Vec::new();
        for frame in self.frames.values() {
            for (id, record) in &frame.ships {
                // (Most ships have no ports: this is asked of every ship in every tick.)
                if record.ports.is_empty() {
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
                    pose: pose_at_the_start(world, *id, ship, lag),
                    ports: &record.ports,
                    rides_a_rail: rides_a_rail(world, *id),
                });
            }
        }
        if !bearers.iter().any(|bearer| bearer.rides_a_rail) {
            return Vec::new();
        }
        bearers.sort_by_key(|bearer| bearer.id);
        let released = docking.released();
        let mut pairs = Vec::new();
        for (index, first) in bearers.iter().enumerate() {
            for second in &bearers[index + 1..] {
                let pair = (first.id, second.id);
                if !(first.rides_a_rail || second.rides_a_rail)
                    || are_of_one_group(world, first.id, second.id)
                    || released.contains(&pair)
                    || self
                        .refused
                        .get(&pair)
                        .is_some_and(|until| self.tick < *until)
                    || grids.get(first.id).map(|ship| ship.level)
                        != grids.get(second.id).map(|ship| ship.level)
                {
                    continue;
                }
                if let Some((first_port, second_port, approach)) = closest_pulling_ports(
                    &docking.settings,
                    (&first.pose, first.ports),
                    (&second.pose, second.ports),
                ) {
                    pairs.push(RailPortPair {
                        first: first.id,
                        second: second.id,
                        first_port,
                        second_port,
                        approach,
                    });
                }
            }
        }
        pairs
    }

    /// Asks the world for the pull of every pair of ports of which a ship rides a rail, on each ship of it: the force at the centre of mass and the torque (the lever of the port, less
    /// the twist that turns the ship into line), the same on both ends and the opposite. A ship that is free has the force in its body, one that rides a rail in its rail (and a ship of a
    /// group in its body too). It is asked before the ships are given their bodies for the tick: a torque that waits gives a ship alone on a rail a body to turn.
    pub(super) fn pull_ports_of_ships_on_rails(&mut self, world: &mut ServerWorld) {
        let Some(docking) = self.config.docking.clone() else {
            return;
        };
        let pairs = self.railed_port_pairs(world, &docking, self.config.settings.time_step);
        if pairs.is_empty() {
            return;
        }
        // (Both ends are worked out before the world is asked of any: the places the ships are at are of this tick's start.)
        let mut pulls: Vec<(GridId, [f64; 3], [f64; 3])> = Vec::new();
        {
            let grids = world.data().grids();
            for pair in &pairs {
                let (Some(first), Some(second)) = (grids.get(pair.first), grids.get(pair.second))
                else {
                    continue;
                };
                // The ports must still be there (an edit this tick may have taken one away).
                if !docking.has_port(first, pair.first_port)
                    || !docking.has_port(second, pair.second_port)
                {
                    continue;
                }
                let force = docking.settings.attraction(&pair.approach);
                let twist = docking.settings.twist(&pair.approach);
                for (id, ship, port, sign) in [
                    (pair.first, first, pair.first_port, 1.0),
                    (pair.second, second, pair.second_port, -1.0),
                ] {
                    let Some(record) = self.record_of(id) else {
                        continue;
                    };
                    // (A ship that is held is not pulled.)
                    if record.kind == BodyKind::Fixed && !rides_a_rail(world, id) {
                        continue;
                    }
                    let (pull, turn) = pull_and_turn(record, &ship.pose, port, force, twist, sign);
                    pulls.push((id, pull, turn));
                }
            }
        }
        for (id, pull, turn) in pulls {
            let _ = world.apply_grid_force(id, pull, None);
            // (No torque is no torque waiting: a ship alone on a rail is not given a body to turn for nothing.)
            if turn != [0.0; 3] {
                let _ = world.apply_grid_torque(id, turn);
            }
        }
    }

    /// Ships whose ports have come together and are in line, of which one at least rides a rail, are made one ship. It is asked **after the step** (see the module): every ship
    /// is at its place of the end of the tick, the world's numbers for how it goes are the ones the ship has, and the ship that rides a rail does not move in the dock.
    pub(super) fn dock_ships_on_rails(&mut self, world: &mut ServerWorld) {
        let Some(docking) = self.config.docking.clone() else {
            return;
        };
        let mut joined: BTreeSet<GridId> = BTreeSet::new();
        for pair in self.railed_port_pairs(world, &docking, 0.0) {
            if !docking.settings.can_snap(&pair.approach)
                || joined.contains(&pair.first)
                || joined.contains(&pair.second)
            {
                continue;
            }
            if self.dock_ships_on_rails_pair(world, &docking, &pair) {
                joined.insert(pair.first);
                joined.insert(pair.second);
            }
        }
    }

    /// Docks two ships that are close enough and in line, of which one at least rides a rail: the one that stays is the one that is held, else the one that rides a rail, else the
    /// heavier, and the other is put exactly where it docks; the two are made one ship that goes as the two did together (the momentum of both) unless one is held. (A ship of a group that
    /// docks is no longer coupled to it: a merge forgets the couplings of the ships it makes one, and the group's rail goes with them.) Whether it was done.
    fn dock_ships_on_rails_pair(
        &mut self,
        world: &mut ServerWorld,
        docking: &Docking,
        pair: &RailPortPair,
    ) -> bool {
        let grids = world.data().grids();
        let (Some(first_ship), Some(second_ship)) = (grids.get(pair.first), grids.get(pair.second))
        else {
            return false;
        };
        // The ports must still be there (an edit this tick may have taken one away).
        if !docking.has_port(first_ship, pair.first_port)
            || !docking.has_port(second_ship, pair.second_port)
        {
            return false;
        }
        let (Some(first), Some(second)) = (self.record_of(pair.first), self.record_of(pair.second))
        else {
            return false;
        };
        let rank = |id: GridId, record: &Ship| {
            if rides_a_rail(world, id) {
                1
            } else if record.kind == BodyKind::Fixed {
                2
            } else {
                0
            }
        };
        let (first_rank, second_rank) = (rank(pair.first, first), rank(pair.second, second));
        let held = first_rank == 2 || second_rank == 2;
        let first_stays = (first_rank, first.mass) >= (second_rank, second.mass);
        let (still, still_port, moving, moving_port) = if first_stays {
            (pair.first, pair.first_port, pair.second, pair.second_port)
        } else {
            (pair.second, pair.second_port, pair.first, pair.first_port)
        };
        let from: WorldPosition = grids.get(still).expect("checked above").pose.position;
        // The motion of both as the world has it (the bodies have written it: a ship on a rail is going as its rail goes), before one is moved.
        let motions = [still, moving].map(|id| {
            let record = self.record_of(id)?;
            let ship = grids.get(id)?;
            motion_of_a_ship(
                record,
                &ship.pose,
                from,
                ship.linear_velocity,
                ship.angular_velocity,
            )
        });
        let motion = joined_motion(held, motions);
        self.make_one_ship_of_two(
            world,
            (still, still_port),
            (moving, moving_port),
            (pair.first, pair.second),
            motion,
        )
    }
}
