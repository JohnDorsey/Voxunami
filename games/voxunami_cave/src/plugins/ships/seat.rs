//! Sitting in the dev command seat: `/ship sit` puts a player who looks at a seat of the ship they are aboard in it, and until they get up their keys fly the ship by the rules of
//! [`crate::flight`]: the seat turns it, the throttle is set by shift and control, and every dev rocket engine of the ship pushes it with the throttle. The pilot is also in the record of
//! pilots (which is how a ship is found to be piloted, how it follows a pilot who leaves, and how the hover controls and the seat keep out of each other's way).

use server::{Audience, FlightState};

use super::*;
use crate::flight::{
    ENGINE_THRUST, Engine, FlightKeys, OrbitSummary, Readout, SeatFrame,
    distance_speed_and_speed_away, engine_at, throttle_after, turning,
};
use crate::messages::FlightReadoutIs;
use protocol::Message;
use voxel_space::{LatticeTransform, LocalVoxelIndex};
use world_storage::MovingGrid;
use world_storage::rail::RailTime;

/// How near, in voxels, a player must be to a seat to sit in it.
const SIT_REACH: f64 = 6.0;

/// The ticks a second of the clock that rails are timed by (the length of a tick is [`TICK_SECONDS`]).
const TICKS_PER_SECOND: u32 = 20;

/// One pilot sitting in a seat.
#[derive(Clone, Copy, Debug)]
pub(super) struct Seated {
    /// The ship the seat is in: a ship that was cut apart, made one with another or given a new id is followed (`Ships::follow_ships_that_were_replaced`).
    ship: GridId,
    /// The voxel of the seat, in the ship's own coordinates.
    seat: VoxelCoordinate,
    /// The throttle, 0 to 1.
    throttle: f64,
}

impl Seated {
    /// The ship the seat is in.
    pub(super) fn ship(&self) -> GridId {
        self.ship
    }

    /// The seat after the ship it was in became `new` (see [`crate::renaming`]): in that ship, at the voxel the transform carries the seat's to. If the seat is not there (a ship cut
    /// apart put it in another part), `fly_seated` finds no seat at the voxel and stands the pilot up.
    pub(super) fn follow(&mut self, new: GridId, transform: LatticeTransform) {
        self.ship = new;
        if let Some(seat) = transform.apply(self.seat) {
            self.seat = seat;
        }
    }
}

/// What a ship is made of, as far as flying it goes.
struct Parts {
    engines: Vec<Engine>,
    /// The inertia of the ship about the right hand, up and forward of the seat through its centre of mass, in kilograms times voxels squared: an estimate from where its blocks are (the
    /// mass of the ship spread evenly over its blocks, and a cube's own inertia added; the products of inertia are left out).
    inertia: [f64; 3],
}

impl Ships {
    pub(super) fn seats(&self) -> MutexGuard<'_, BTreeMap<ClientId, Seated>> {
        self.seats
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Tells a player the lines to show, or (with none) that there are none now.
    fn send_readout(world: &mut ServerWorld, client: ClientId, lines: Vec<String>) {
        world.send(
            Audience::Client(client),
            FlightReadoutIs::kind(),
            FlightReadoutIs(lines).encode(),
        );
    }

    /// `/ship sit`: gets up, if they sit; otherwise sits in the seat they look at, in the ship they are aboard.
    pub(super) fn sit(&self, world: &mut ServerWorld, client: ClientId) -> String {
        let mut pilots = self.pilots();
        let mut seats = self.seats();
        if let Some(seated) = seats.remove(&client) {
            pilots.remove(&client);
            {
                let mut movement = locked(&self.movement);
                movement.set_piloting(client, false);
                // (Let go of the seat: they stand in the ship again.)
                movement.unbuckle(client);
            }
            Self::send_readout(world, client, Vec::new());
            return format!(
                "you get up from the command seat of ship {}; the engines stop",
                number(seated.ship)
            );
        }
        if pilots.contains_key(&client) {
            return "you pilot with the hover controls: let go of them first (/ship pilot)"
                .to_owned();
        }
        let presence = lock(&self.presence);
        let hit = block_looked_at(
            world,
            &presence,
            client,
            &self.table,
            [
                self.block_channel,
                self.state_channel,
                self.orientation_channel,
            ],
            SIT_REACH,
        );
        let seat = hit.ok().filter(|hit| {
            hit.grid.root_level().is_none()
                && world.data().grids().get(hit.grid).is_some_and(|ship| {
                    ship.voxels.get(self.block_channel, hit.voxel) == self.seat_block
                })
        });
        let Some(hit) = seat else {
            return "you are not looking at a command seat (stand beside one, within reach)"
                .to_owned();
        };
        let ship = hit.grid;
        if presence.frame_of(world.data(), client) != Some(ship) {
            return "stand on the ship to sit in its seat".to_owned();
        }
        if pilots.values().any(|piloted| *piloted == ship) {
            return format!("someone else is flying ship {}", number(ship));
        }
        pilots.insert(client, ship);
        seats.insert(
            client,
            Seated {
                ship,
                seat: hit.voxel,
                throttle: 0.0,
            },
        );
        {
            let mut movement = locked(&self.movement);
            movement.set_piloting(client, true);
            // Strapped in: the ship carries a seated pilot exactly however its seat turns it and its engines push it (a walker is pulled by the acceleration of the ship, and
            // would be left behind by it).
            movement.buckle(client, ship);
        }
        format!(
            "you sit in the command seat of ship {}: W S pitch, A D yaw, Q E roll, shift raises the throttle and control lowers it; /ship sit gets you up",
            number(ship)
        )
    }

    /// What the ship is made of, from its voxels.
    fn parts_of(&self, ship: &MovingGrid, frame: &SeatFrame) -> Parts {
        let centre = ship.centre_of_mass;
        let mut engines = Vec::new();
        let mut blocks = 0_u64;
        let axes = [frame.right, frame.up, frame.forward];
        let mut spread = [0.0; 3];
        for (brick, contents) in ship.voxels.bricks() {
            for local in LocalVoxelIndex::all() {
                let block = contents.get(self.block_channel, local);
                if block == self.air {
                    continue;
                }
                let voxel = brick.voxel_at(local);
                blocks += 1;
                let middle =
                    [voxel.x(), voxel.y(), voxel.z()].map(|coordinate| coordinate as f64 + 0.5);
                let apart: [f64; 3] = [0, 1, 2].map(|axis| middle[axis] - centre[axis]);
                let square: f64 = apart.iter().map(|part| part * part).sum();
                for (about, axis) in spread.iter_mut().zip(axes) {
                    // (The distance from the axis: all of it but the part along the axis.)
                    let along: f64 = (0..3).map(|index| apart[index] * axis[index]).sum();
                    *about += square - along * along;
                }
                if block == self.engine_block {
                    engines.extend(engine_at(
                        voxel,
                        contents.get(self.orientation_channel, local),
                    ));
                }
            }
        }
        // (The mean square of the distance from the axis, and a cube's own: a sixth of its mass.)
        let about = |sum: f64| {
            let mean_square = if blocks == 0 {
                0.0
            } else {
                sum / blocks as f64
            };
            ship.mass * (mean_square + 1.0 / 6.0)
        };
        Parts {
            engines,
            inertia: spread.map(about),
        }
    }

    /// Each tick: every seated pilot's keys turn their ship and set the throttle, and the engines push it. A pilot whose seat is gone, who is no longer aboard, or whose ship is gone,
    /// is up.
    pub(super) fn fly_seated(&self, world: &mut ServerWorld) {
        let presence = lock(&self.presence);
        let mut pilots = self.pilots();
        let mut seats = self.seats();
        if seats.is_empty() {
            return;
        }
        Self::follow_ships_that_were_replaced(world, &presence, &mut pilots, &mut seats);
        let mut movement = locked(&self.movement);
        let mut up = Vec::new();
        for (&client, seated) in seats.iter_mut() {
            let Some(&ship) = pilots.get(&client) else {
                up.push(client);
                continue;
            };
            let aboard = presence.frame_of(world.data(), client) == Some(ship);
            let (Some(moving), true) = (world.data().grids().get(ship), aboard) else {
                up.push(client);
                continue;
            };
            let seat_is_there = ship == seated.ship
                && moving.voxels.get(self.block_channel, seated.seat) == self.seat_block;
            let (Some(request), true, Some(frame)) = (
                movement.request_of(client),
                seat_is_there,
                SeatFrame::of_orientation(moving.voxels.get(self.orientation_channel, seated.seat)),
            ) else {
                up.push(client);
                continue;
            };
            let keys = FlightKeys::of_movement(
                request.input.forward,
                request.input.strafe,
                request.input.vertical,
                request.input.sprint,
                request.roll,
            );
            seated.throttle = throttle_after(seated.throttle, keys.throttle, TICK_SECONDS);
            let parts = self.parts_of(moving, &frame);
            let (pose, mass, linear, angular) = (
                moving.pose,
                moving.mass,
                moving.linear_velocity,
                moving.angular_velocity,
            );
            let turn = turning(
                &frame,
                &keys,
                pose.direction_to_local(angular),
                parts.inertia,
                TICK_SECONDS,
            );
            let readout = self.readout(world, ship, seated.throttle, &parts, mass, angular);
            if seated.throttle > 0.0 {
                for engine in &parts.engines {
                    let push = engine
                        .push
                        .map(|part| part * ENGINE_THRUST * seated.throttle);
                    let _ = world.apply_grid_force(
                        ship,
                        pose.direction_to_world(push),
                        Some(engine.at),
                    );
                }
            }
            if turn.still {
                if angular != [0.0; 3] {
                    let _ = world.set_grid_velocity(ship, linear, [0.0; 3]);
                }
            } else if turn.torque != [0.0; 3] {
                let _ = world.apply_grid_torque(ship, pose.direction_to_world(turn.torque));
            }
            Self::send_readout(world, client, readout.lines());
        }
        for client in up {
            seats.remove(&client);
            pilots.remove(&client);
            movement.set_piloting(client, false);
            movement.unbuckle(client);
            Self::send_readout(world, client, Vec::new());
        }
    }

    /// What the pilot is told of the ship's flight now.
    fn readout(
        &self,
        world: &ServerWorld,
        ship: GridId,
        throttle: f64,
        parts: &Parts,
        mass: f64,
        angular_velocity: [f64; 3],
    ) -> Readout {
        let clock = world.data().clock();
        let flight = world.flight_state(ship, RailTime::of_tick(clock, TICKS_PER_SECOND));
        let length = |vector: [f64; 3]| vector.iter().map(|part| part * part).sum::<f64>().sqrt();
        let (body, distance, speed, speed_away, orbit) = match flight {
            None => (None, 0.0, 0.0, 0.0, OrbitSummary::None),
            Some(FlightState {
                planet,
                place,
                velocity,
                orbit,
            }) => {
                let (distance, speed, away) = distance_speed_and_speed_away(place, velocity);
                (
                    Some(number(planet)),
                    distance,
                    speed,
                    away,
                    OrbitSummary::of(&orbit),
                )
            }
        };
        Readout {
            throttle,
            engines: parts.engines.len(),
            acceleration: if mass > 0.0 {
                parts.engines.len() as f64 * ENGINE_THRUST * throttle / mass
            } else {
                0.0
            },
            turn_rate: length(angular_velocity),
            body,
            distance,
            speed,
            speed_away,
            orbit,
        }
    }
}
