//! Ships that coast: a free ship far from everything it could touch goes on a rail, and comes off it when anything happens to it (`docs/orbits-and-thrust-plan.md`, step B).
//!
//! A ship that nothing pushes and nothing is near is not simulated: it goes round the planet that pulls it most on the orbit its place and its speed are on, and its place
//! is a function of the time of the world (the rail of `world_storage::rail`, which holds exactly however long it goes on). Observed or not, a ship on a rail is where the clock
//! says, and a ship that is saved on one and brought back is where the clock says too. The bodies hold a ship on a rail where it is put.
//!
//! **What is ruled out, and why.** A rail is a conic round one planet (patched conics: the planet's pull and no other, as the owner chose, 2026-11-01), so a ship is put on one only
//! where that is the motion it would have. It is not put on one, and comes off one it is on, when:
//! - it is held, or its level has a pull of its own (a level with a ground), or it turns (a rail holds a ship's place and not its turn: a ship that spins is the bodies');
//! - something pushes it: a force or a torque, a docking, a program that puts it somewhere, anything that is not the bodies moving it;
//! - it is near what it could touch (another ship, or a planet that is not the one it goes round), nearer than a margin and the way it would go in a second at the speed it
//!   closes at, so that nothing meets it while it is on a rail; or it is in the sphere that a planet on a rail carries what is near it with;
//! - the orbit does not lie round the planet in the way the rail needs: it comes lower than the planet's ground, or goes out past the planet's sphere of influence (where
//!   what the planet goes round pulls more).
//!
//! A ship has to be undisturbed for [`QUIET_SECONDS`] before it goes on a rail, so one that is pushed now and then does not go on and off. Its orbit is made from its place and its
//! speed at the time (`Orbit::from_place`), the orbit the numbers of that state are on (to 128 bits, with nothing snapped to a circle), and is made again only when the ship was
//! pushed, never for a ship that was left alone, which keeps the same elements however many times it is saved and brought back.

use voxel_space::{BrickBox, GridId, WorldPosition};
use world_storage::MovingGrid;
use world_storage::rail::{Centre, Orbit, Rail, RailError, RailTime};

use super::*;

/// Seconds a ship has to be undisturbed before it goes on a rail.
pub const QUIET_SECONDS: u64 = 1;

/// How near, in voxels, something may be, over the way the ship would go in [`NEAR_SECONDS`] at the speed it closes at, before a ship is not on a rail (a ship that is on one
/// comes off it at half as near, so that it does not go on and off).
const NEAR_MARGIN: f64 = 64.0;

/// The seconds a ship would go on at the speed it closes at, for how near it may come.
const NEAR_SECONDS: f64 = 1.0;

/// How much a mass parameter has to differ from the one an orbit was made with, as a part of it, to be a change (one in a billion: the period and the axis of an orbit say the
/// mass they were made with to a part in 10^15, and the mass of a planet is not worked out to better than that).
const MASS_CHANGE_THAT_COUNTS: f64 = 1.0e-9;

/// The eccentricity under which a ship that is on a rail answers a change of the mass of its planet at once and does not wait for an end of its ellipse: the two ends of such
/// an ellipse are within that many parts of its size of each other, so there is nothing to choose between them. (The owner's figure, 2026-11-03, for orbits close enough to
/// circular to be shown as circles in the map. It decides **when** the ship is put on the orbit its state is on; it does not change any orbit: a rail and a file keep the
/// ellipse they have, and an orbit that is exactly a circle has no ends at all.)
const NEARLY_A_CIRCLE: f64 = 1.0e-14;

/// Why a ship is not on a rail that goes round a planet.
#[derive(Clone, Debug, PartialEq)]
pub enum NotCoasting {
    /// There is no such ship, or it is a planet (a ship with terrain is held where its own rail says and goes round nothing here).
    NotAShip,
    /// The ship is held.
    Held,
    /// The ship turns: a rail holds a ship's place and not its turn.
    Turning,
    /// The level the ship is in has a pull of its own (a ground to fall to), which is no pull toward a planet.
    LevelPulls,
    /// No planet that pulls (a planet with a number lower than the ship's, held or on a rail, weighing at least what pulls) is there.
    NoPlanetPulls,
    /// The place and the speed are on no orbit round the planet (see [`world_storage::rail::Orbit::from_place`]).
    NoOrbit(RailError),
    /// The orbit comes nearer the planet's middle than its ground and what it carries.
    TooLow { lowest: f64, least: f64 },
    /// The orbit goes out past the planet's sphere of influence, where what it goes round pulls more.
    PastTheSphere { farthest: f64, sphere: f64 },
    /// Another ship or planet is near, or the ship is in what a planet on a rail carries.
    SomethingNear(GridId),
}

/// Where the middle of a grid is (the origin of a planet, whose centre of mass is there) and how far from it its bricks reach, in voxels.
struct Reach {
    centre: WorldPosition,
    radius: f64,
}

/// How far a box of bricks reaches from the origin of its coordinates, in voxels: the farthest corner of the bricks it holds, a whole brick on the far side of each.
fn reach_of_bricks(low: [i128; 3], high: [i128; 3]) -> f64 {
    let edge = voxel_space::BRICK_EDGE_LENGTH as f64;
    (0..3)
        .map(|axis| {
            let farthest = (low[axis].abs() as f64).max(high[axis].abs() as f64 + 1.0) * edge;
            farthest * farthest
        })
        .sum::<f64>()
        .sqrt()
}

/// How far from the middle of a planet its ground reaches, in voxels: the farthest brick its terrain makes, from the middle.
pub(super) fn terrain_reach(extent: BrickBox) -> f64 {
    let reach_in_bricks = [extent.min(), extent.max()]
        .into_iter()
        .flat_map(|corner| [corner.x(), corner.y(), corner.z()])
        .map(i128::abs)
        .max()
        .unwrap_or(0);
    reach_in_bricks as f64 * voxel_space::BRICK_EDGE_LENGTH as f64
}

fn reach(ship: &MovingGrid) -> Reach {
    let radius = match (ship.terrain, ship.brick_extent()) {
        (Some(terrain), _) => terrain_reach(terrain.extent),
        (None, Some((low, high))) => {
            reach_of_bricks([low.x(), low.y(), low.z()], [high.x(), high.y(), high.z()])
        }
        (None, None) => 0.0,
    };
    Reach {
        centre: ship.pose.position,
        radius,
    }
}

fn size(vector: [f64; 3]) -> f64 {
    libm::sqrt(vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2])
}

/// Whether a rail is of the kind this module makes: an orbit round a planet that turns nothing. (Any other rail of a ship is someone else's, and left to them.)
pub(super) fn is_a_coasting_rail(rail: &Rail) -> bool {
    rail.spin.is_none()
        && rail
            .orbit
            .as_ref()
            .is_some_and(|orbit| matches!(orbit.around, Centre::Grid(_)))
}

impl ServerWorld {
    /// The rail a ship is on if it coasts round the planet that pulls it most, at `time`, or why it cannot be on one. Nothing changes. It is the orbit that the ship's place (its
    /// centre of mass) and its speed are on, with the pull of that planet alone.
    pub fn coasting_rail(&self, grid: GridId, time: RailTime) -> Result<Rail, NotCoasting> {
        let ship = self.data.grids().get(grid).ok_or(NotCoasting::NotAShip)?;
        if ship.terrain.is_some() {
            return Err(NotCoasting::NotAShip);
        }
        if ship.held {
            return Err(NotCoasting::Held);
        }
        if ship.angular_velocity != [0.0; 3] {
            return Err(NotCoasting::Turning);
        }
        if self
            .level_gravity
            .get(&ship.level)
            .unwrap_or(&self.gravity.base)
            != &[0.0; 3]
        {
            return Err(NotCoasting::LevelPulls);
        }
        let middle = self.middle_of(ship).ok_or(NotCoasting::NoPlanetPulls)?;
        let (parent, planet, place) = self
            .planet_that_pulls_most(grid, ship, middle)
            .ok_or(NotCoasting::NoPlanetPulls)?;
        let orbit = self.orbit_from_the_state(ship, parent, planet, place, time)?;
        if let Some(near) = self.near(grid, ship, Some(parent), 2.0) {
            return Err(NotCoasting::SomethingNear(near));
        }
        Ok(Rail {
            spin: None,
            orbit: Some(orbit),
        })
    }

    /// The orbit a ship's speed and `place` (its centre of mass from the middle of the planet) are on round `planet`, with the mass the planet has now, if it is one the ship may go on a
    /// rail in: it does not come lower than the ground and what the planet carries, and does not go out of the planet's sphere of influence.
    fn orbit_from_the_state(
        &self,
        ship: &MovingGrid,
        parent: GridId,
        planet: &MovingGrid,
        place: [f64; 3],
        time: RailTime,
    ) -> Result<Orbit, NotCoasting> {
        let relative_velocity: [f64; 3] =
            [0, 1, 2].map(|axis| ship.linear_velocity[axis] - planet.linear_velocity[axis]);
        let orbit = Orbit::from_place(
            Centre::Grid(parent),
            place,
            relative_velocity,
            self.mass_parameter_of(planet),
            time,
        )
        .map_err(NotCoasting::NoOrbit)?;
        // How low it comes, and how far out it goes.
        let lowest = orbit.nearest_distance();
        let least = self.least_to_go_round(parent, planet);
        if lowest < least {
            return Err(NotCoasting::TooLow { lowest, least });
        }
        if let Some(sphere) = self.sphere_of_influence(parent) {
            let farthest = orbit.farthest_distance();
            if farthest > sphere {
                return Err(NotCoasting::PastTheSphere { farthest, sphere });
            }
        }
        Ok(orbit)
    }

    /// `G M` of a planet, in voxels cubed a second squared: what it pulls with, which is nothing if it weighs less than what pulls.
    fn mass_parameter_of(&self, planet: &MovingGrid) -> f64 {
        match self.gravity.pull_mass_threshold {
            Some(threshold) if planet.mass >= threshold && planet.mass > 0.0 => {
                self.gravity.pull_strength * planet.mass
            }
            _ => 0.0,
        }
    }

    /// Where the centre of mass of a ship is in the world.
    fn middle_of(&self, ship: &MovingGrid) -> Option<WorldPosition> {
        ship.pose.local_to_world(WorldPosition::new(
            BrickCoordinate::ORIGIN,
            ship.centre_of_mass,
        ))
    }

    /// The planet that pulls a place most, with the vector from its middle to the place: one with a number lower than the ship's (so that its place is worked out first), in the
    /// ship's level, weighing at least what pulls, and held or on a rail (so that its motion is the rail's and does not wait on a push). The pull is the mass over the square of the
    /// distance; of two that pull alike the one with the lower number.
    fn planet_that_pulls_most(
        &self,
        grid: GridId,
        ship: &MovingGrid,
        middle: WorldPosition,
    ) -> Option<(GridId, &MovingGrid, [f64; 3])> {
        let threshold = self.gravity.pull_mass_threshold?;
        let mut best: Option<(f64, GridId, &MovingGrid, [f64; 3])> = None;
        for (id, planet) in self.data.grids().iter() {
            if id >= grid
                || planet.level != ship.level
                || planet.terrain.is_none()
                || !(planet.held || planet.rail.is_some())
                || !(planet.mass >= threshold && planet.mass > 0.0)
            {
                continue;
            }
            let place = planet.pose.position.displacement_to(middle).in_voxels();
            let distance = size(place);
            if distance == 0.0 {
                continue;
            }
            let pull = planet.mass / (distance * distance);
            if best.is_none_or(|(strongest, ..)| pull > strongest) {
                best = Some((pull, id, planet, place));
            }
        }
        best.map(|(_, id, planet, place)| (id, planet, place))
    }

    /// How near to the middle of a planet an orbit round it may come, in voxels: past the reach of its ground by the margin, and past what it carries if it is on a rail.
    fn least_to_go_round(&self, planet: GridId, ground: &MovingGrid) -> f64 {
        let ground_reach = ground
            .terrain
            .map_or(0.0, |terrain| terrain_reach(terrain.extent));
        let carried = self
            .influence_radius(planet)
            .map_or(0.0, |radius| radius * INFLUENCE_LEAVING);
        (ground_reach + NEAR_MARGIN).max(carried)
    }

    /// How far from the middle of a planet what it pulls is pulled by it more than by what it goes round, in voxels (the sphere of influence, `a (m / M)^(2/5)` of its orbit
    /// round the planet it goes round); `None` for a planet that goes round nothing that is a planet here, which pulls everywhere.
    fn sphere_of_influence(&self, planet: GridId) -> Option<f64> {
        let ship = self.data.grids().get(planet)?;
        let orbit = ship.rail.as_ref()?.orbit.as_ref()?;
        let Centre::Grid(parent) = orbit.around else {
            return None;
        };
        let heavier = self.data.grids().get(parent)?;
        if !(heavier.mass > 0.0 && ship.mass > 0.0) {
            return None;
        }
        Some(orbit.radius() * libm::pow(ship.mass / heavier.mass, 0.4))
    }

    /// Some other grid whose gap from the ship is less than `factor` times the margin and how far the ship would go in a second at the speed it closes at, a planet whose sphere of
    /// influence the ship is in, or a planet on a rail that carries the ship (what is within its sphere; a quarter farther out when `factor` is above 1). A ship that goes on a
    /// rail needs twice the room (`factor` 2) that one that is on a rail keeps (`factor` 1), so that it does not go on and off. What the ship goes round and what that goes round
    /// are not counted: the orbit has its own rule for them.
    fn near(
        &self,
        grid: GridId,
        ship: &MovingGrid,
        goes_round: Option<GridId>,
        factor: f64,
    ) -> Option<GridId> {
        let mut chain = goes_round.map_or_else(Vec::new, |parent| {
            let mut chain = self.what_it_goes_round(parent);
            chain.push(parent);
            chain
        });
        chain.push(grid);
        let mine = reach(ship);
        for (id, other) in self.data.grids().iter() {
            if chain.contains(&id) || other.level != ship.level {
                continue;
            }
            let theirs = reach(other);
            let apart = size(mine.centre.displacement_to(theirs.centre).in_voxels());
            let velocity: [f64; 3] =
                [0, 1, 2].map(|axis| other.linear_velocity[axis] - ship.linear_velocity[axis]);
            let gap = apart - mine.radius - theirs.radius;
            if gap < factor * (NEAR_MARGIN + size(velocity) * NEAR_SECONDS) {
                return Some(id);
            }
        }
        // (A planet that is not what the ship goes round, and that the ship is within the sphere of influence of: there it pulls more than what the ship goes round.)
        let place = self.middle_of(ship)?;
        for (id, other) in self.data.grids().iter() {
            if chain.contains(&id) || other.level != ship.level || other.terrain.is_none() {
                continue;
            }
            if let Some(sphere) = self.sphere_of_influence(id)
                && size(other.pose.position.displacement_to(place).in_voxels()) < sphere
            {
                return Some(id);
            }
        }
        let carriers = self.carriers();
        let limit = if factor > 1.0 { INFLUENCE_LEAVING } else { 1.0 };
        carriers
            .iter()
            .filter(|carrier| carrier.level == ship.level)
            .find(|carrier| {
                size(carrier.position.displacement_to(place).in_voxels()) <= carrier.radius * limit
            })
            .map(|carrier| carrier.id)
    }

    /// Whether a ship that is on a rail of this module has to come off it: it is held, turns, is in a level with a pull of its own, goes round nothing that is there, or
    /// something is near.
    fn has_to_leave_its_rail(&self, grid: GridId) -> bool {
        let Some(ship) = self.data.grids().get(grid) else {
            return false;
        };
        let Some(Centre::Grid(parent)) = ship
            .rail
            .as_ref()
            .and_then(|rail| rail.orbit.as_ref())
            .map(|orbit| orbit.around)
        else {
            return false;
        };
        ship.held
            || ship.angular_velocity != [0.0; 3]
            || self
                .level_gravity
                .get(&ship.level)
                .unwrap_or(&self.gravity.base)
                != &[0.0; 3]
            || self.data.grids().get(parent).is_none()
            || self.near(grid, ship, Some(parent), 1.0).is_some()
    }

    /// Answers a change of the mass of the planet a ship goes round, as the owner chose (2026-11-02): a ship that is on a rail made for another mass parameter than the planet has now
    /// (the elements say the one they were made with: [`Orbit::mass_parameter`]) is made an orbit again from its place and its speed, which a change of mass does not touch, **at the
    /// end of the ellipse where that is safe**. A change keeps the distance at the end it is made at, since the speed there is across the radius, and moves the other end: an
    /// **increase** is answered at the periapsis (the far end comes in: it cannot fall into the planet) and a **decrease** at the apoapsis (the near end goes out: it cannot be
    /// thrown out), and at the other end the ship waits. A circle has no ends and answers at once. An end is passed in the tick whose half-tick either side it is in, so the ship is the
    /// nearest tick to it. A ship that is on no ellipse with the new mass (it would escape, or the planet pulls nothing, or the orbit would be in the ground) is taken off its rail,
    /// as it is, to go on as the bodies have it.
    fn answer_a_change_of_mass(&mut self, grid: GridId, ticks_per_second: u32) {
        let now = self.data.clock();
        let Some(ship) = self.data.grids().get(grid) else {
            return;
        };
        let Some(orbit) = ship.rail.as_ref().and_then(|rail| rail.orbit.as_ref()) else {
            return;
        };
        let Centre::Grid(parent) = orbit.around else {
            return;
        };
        let Some(planet) = self.data.grids().get(parent) else {
            return;
        };
        let (known, now_mu) = (orbit.mass_parameter(), self.mass_parameter_of(planet));
        let change = now_mu / known - 1.0;
        if change.abs() <= MASS_CHANGE_THAT_COUNTS {
            return;
        }
        let answers_now = orbit.eccentricity() < NEARLY_A_CIRCLE || {
            let passed = orbit.apsides_passed(
                RailTime::of_half_ticks((2 * now).saturating_sub(1), ticks_per_second),
                RailTime::of_half_ticks(2 * now + 1, ticks_per_second),
            );
            if change > 0.0 {
                passed.periapsis
            } else {
                passed.apoapsis
            }
        };
        if !answers_now {
            return;
        }
        let fitted = self.middle_of(ship).and_then(|middle| {
            let place = planet.pose.position.displacement_to(middle).in_voxels();
            self.orbit_from_the_state(
                ship,
                parent,
                planet,
                place,
                RailTime::of_tick(now, ticks_per_second),
            )
            .ok()
        });
        let rail = fitted.map(|orbit| Rail {
            spin: None,
            orbit: Some(orbit),
        });
        let _ = self.set_grid_rail(grid, rail);
    }

    /// Puts quiet ships on rails and takes off those that have to come off, as the last of what is done in a tick before the rails are applied: a ship that has been undisturbed
    /// for [`QUIET_SECONDS`] and can coast is put on the orbit its place and speed are on, and one on a rail of this module that is held, turns, has something near it, or goes
    /// round nothing is taken off, as it is, at the place and the speed the rail gave it.
    pub(crate) fn update_coasting(&mut self, ticks_per_second: u32) {
        let now = self.data.clock();
        let time = RailTime::of_tick(now, ticks_per_second);
        let wait = QUIET_SECONDS * u64::from(ticks_per_second);
        let ships: Vec<GridId> = self
            .data
            .grids()
            .iter()
            .filter(|(_, ship)| ship.terrain.is_none())
            .map(|(id, _)| id)
            .collect();
        for id in ships {
            let coasting = self
                .data
                .grids()
                .get(id)
                .and_then(|ship| ship.rail.as_ref())
                .map(is_a_coasting_rail);
            match coasting {
                Some(true) => {
                    if self.has_to_leave_its_rail(id) {
                        self.quiet_since.remove(&id);
                        let _ = self.set_grid_rail(id, None);
                    } else {
                        self.answer_a_change_of_mass(id, ticks_per_second);
                    }
                }
                Some(false) => {}
                None => match self.coasting_rail(id, time) {
                    Ok(rail) => {
                        let since = *self.quiet_since.entry(id).or_insert(now);
                        if now.saturating_sub(since) >= wait {
                            self.quiet_since.remove(&id);
                            let _ = self.set_grid_rail(id, Some(rail));
                        }
                    }
                    Err(_) => {
                        self.quiet_since.remove(&id);
                    }
                },
            }
        }
        let grids = self.data.grids();
        self.quiet_since.retain(|id, _| grids.get(*id).is_some());
    }

    /// Notes that something other than the bodies has done something to a ship: it is not quiet, and if it is on a rail of this module it comes off it, where it is (a rail
    /// is the place the ship is put at, so whatever is done to the ship is done from there).
    pub(super) fn disturb(&mut self, grid: GridId) {
        self.quiet_since.remove(&grid);
        let Some(ship) = self.data.grids_mut().get_mut(grid) else {
            return;
        };
        if ship.terrain.is_none() && ship.rail.as_ref().is_some_and(is_a_coasting_rail) {
            ship.rail = None;
        }
    }
}
