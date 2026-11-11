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
//! - the orbit does not lie round the planet in the way the rail needs: it comes lower than the planet's ground (a ship that is outside the planet's sphere of influence already,
//!   where what the planet goes round pulls more, goes round that and not this).
//!
//! **A ship leaves the sphere of influence of its planet on its rail** (E16 phase B): an orbit that goes out of it is a rail. At the instant it crosses, found in closed form and rounded up to
//! a grid of 2^-31 of a second (`Orbit::leaves_at`), the ship goes on the orbit round what the planet goes round that its state is on there (`Orbit::handed_over_to`), however many ticks
//! that is behind and however many spheres it has crossed since; so a path is the same at every tick rate, and a ship that was parked is where it would have been. See
//! [`ServerWorld::hand_over_what_has_left_its_sphere`].
//!
//! A ship has to be undisturbed for [`QUIET_SECONDS`] before it goes on a rail, so one that is pushed now and then does not go on and off. Its orbit is made from its place and its
//! speed at the time (`Orbit::from_place`), the orbit the numbers of that state are on (to 128 bits, with nothing snapped to a circle), and is made again only when the ship was
//! pushed, never for a ship that was left alone, which keeps the same elements however many times it is saved and brought back.
//!
//! **What is asked of a ship every tick, and what once.** Every tick, for every ship that is not on a rail, only the rules that need no orbit are asked (a free ship that turns nothing, in
//! a level with no pull of its own, with a planet that pulls it and nothing near it: a few comparisons, the planets, and a search of the ships that are near: see [`neighbours`], which finds them without looking at every ship). Only when a ship has passed them for the whole
//! [`QUIET_SECONDS`], undisturbed, is the orbit made from its state (114 to 145 microseconds) to see whether it is one the ship may have (it exists, does not come too low, does not leave
//! the sphere of influence); if it is not, the second begins again. So a ship that is pushed every tick is never fitted, one that is left alone is fitted once, and one whose orbit is refused is
//! fitted once a second and a tick, where every one was fitted every tick before. The cost is that the orbit-dependent rules are looked at at the end of the second and not at each tick of it:
//! a ship that was quiet but whose orbit was refused for a while and is allowed at the end goes on a rail at the end, though it was allowed for less than a second.

use std::cmp::Ordering;
use std::collections::HashMap;

use voxel_space::{BrickBox, GridId, WorldPosition};
use world_storage::MovingGrid;
use world_storage::rail::{Centre, Orbit, Rail, RailError, RailTime};

use super::*;

pub(super) mod neighbours;
mod prediction;
use neighbours::{Body, ShipsInTheWay, is_in_the_way};
pub use prediction::{PredictedRail, Prediction, PredictionEnd};

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

/// How far inside a sphere of influence, as a part of its radius, a ship that is on a rail has to be to be taken for one that went into it: the ship that has just come out of the
/// sphere, at the instant of the crossing, is at its edge to the rounding of its place (2^-32 voxel), and must not be taken for one that is in it.
const SPHERE_SLACK: f64 = 1.0e-9;

/// The most hand-overs one ship has in one asking: a ship goes up one planet at a time, so the depth of the planets of a world is all it can need. (The bound is for a world whose planets go
/// round each other in a loop, which a rail cannot make: a planet goes round one with a lower number.)
const MOST_HAND_OVERS_AT_ONCE: usize = 64;

/// When a ship on a rail leaves the sphere of influence of the planet it goes round: worked out once for an orbit and a sphere, and kept until either changes.
#[derive(Debug)]
pub(super) struct ExitPlan {
    orbit: Orbit,
    sphere: f64,
    at: Option<RailTime>,
}

/// The instant an orbit leaves a sphere of this radius, if it does, and whether the closed form had to be worked out for it (an orbit that stays well inside the sphere needs none: a comparison
/// of doubles says so). The orbit is on from `orbit.since`, or from `time` if it has none.
pub(super) fn search_for_exit(
    orbit: &Orbit,
    sphere: f64,
    time: RailTime,
) -> (Option<RailTime>, bool) {
    let can_leave = orbit.farthest_distance() > sphere * (1.0 - SPHERE_SLACK);
    let at = if can_leave {
        orbit
            .leaves_at(sphere, orbit.since.unwrap_or(time))
            .ok()
            .flatten()
    } else {
        None
    };
    (at, can_leave)
}

/// How a ship is flying against the planet that pulls it most: where it is and how it goes against the planet, and the orbit that state is on with the planet's pull now (the one a rail
/// would hold if the ship were on one), or why there is none.
#[derive(Clone, Debug)]
pub struct FlightState {
    /// The planet that pulls the ship most.
    pub planet: GridId,
    /// Where the centre of mass of the ship is from the middle of the planet, in voxels.
    pub place: [f64; 3],
    /// How fast the ship goes against the planet, in voxels a second.
    pub velocity: [f64; 3],
    /// The orbit that state is on, round the planet.
    pub orbit: Result<Orbit, RailError>,
}

/// Why a ship is not on a rail that goes round a planet.
#[derive(Clone, Debug, PartialEq)]
pub enum NotCoasting {
    /// There is no such ship, or it is a planet (a ship with terrain is held where its own rail says and goes round nothing here).
    NotAShip,
    /// The ship is held.
    Held,
    /// The ship turns and is coupled to another by a joint: a rail holds a ship's centre of mass and not its turn (the bodies turn it), but it does not yet hold a group.
    Turning,
    /// The level the ship is in has a pull of its own (a ground to fall to), which is no pull toward a planet.
    LevelPulls,
    /// No planet that pulls (a planet with a number lower than the ship's, held or on a rail, weighing at least what pulls) is there.
    NoPlanetPulls,
    /// The place and the speed are on no orbit round the planet (see [`world_storage::rail::Orbit::from_place`]).
    NoOrbit(RailError),
    /// The orbit comes nearer the planet's middle than its ground and what it carries.
    TooLow { lowest: f64, least: f64 },
    /// The ship is outside the planet's sphere of influence already (`farthest` is how far out it is), where what the planet goes round pulls more.
    PastTheSphere { farthest: f64, sphere: f64 },
    /// Another ship or planet is near, or the ship is in what a planet on a rail carries.
    SomethingNear(GridId),
}

/// A ship that passes the rules that look at one ship, with the planet that pulls it and where it is from the planet's middle.
struct Candidate<'a> {
    ship: &'a MovingGrid,
    parent: GridId,
    planet: &'a MovingGrid,
    place: [f64; 3],
}

/// Where the middle of a grid is (the origin of a planet, whose centre of mass is there) and how far from it its bricks reach, in voxels.
#[derive(Clone, Copy, Debug, PartialEq)]
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

/// A planet that has a sphere of influence: a ship inside it, that does not go round it, is not put on a rail.
struct Sphere {
    id: GridId,
    level: LevelIndex,
    position: WorldPosition,
    radius: f64,
}

/// What [`ServerWorld::near`] asks of the world, found once for the ships of a tick. Nothing it is found from changes while [`ServerWorld::update_coasting`] goes through the
/// ships (it puts ships on rails and takes them off, and a rail is not part of it: the places, speeds and reach of the grids, their levels, the planets' spheres and what they
/// carry stay as they are), and a ship that is asked about is checked against what it was when this was made.
pub(super) struct Neighbours {
    ships: ShipsInTheWay,
    /// What each ship was.
    bodies: HashMap<GridId, Body>,
    /// Every grid with terrain, in the order of their numbers.
    planets: Vec<(GridId, Body)>,
    spheres: Vec<Sphere>,
    carriers: Vec<Carrier>,
}

impl Neighbours {
    pub(super) fn of(world: &ServerWorld) -> Neighbours {
        let (mut ships, mut planets) = (Vec::new(), Vec::new());
        for (id, grid) in world.data.grids().iter() {
            let body = Body::of(grid);
            if grid.terrain.is_some() {
                planets.push((id, body));
            } else {
                ships.push((id, body));
            }
        }
        Neighbours {
            ships: ShipsInTheWay::find(&ships),
            bodies: ships.into_iter().collect(),
            planets,
            spheres: world.spheres(),
            carriers: world.carriers(),
        }
    }

    /// The first grid, in the order of their numbers, that is in the way of the ship `grid` (`mine`) with this room, not counting `chain`: the lowest of the ships in its way
    /// and of the planets. A ship that is not one of those this was made of (one that has terrain) is for looking at every grid.
    fn in_the_way(
        &self,
        world: &ServerWorld,
        grid: GridId,
        mine: &Body,
        chain: &[GridId],
        factor: f64,
    ) -> Option<GridId> {
        let among_ships = match self.ships.lowest(grid, factor, chain) {
            Some(found) => found,
            None => return world.in_the_way_looking_at_every_grid(mine, chain, factor),
        };
        let among_planets = self
            .planets
            .iter()
            .find(|(id, other)| {
                !chain.contains(id)
                    && other.level() == mine.level()
                    && is_in_the_way(mine, other, factor)
            })
            .map(|(id, _)| *id);
        match (among_ships, among_planets) {
            (Some(ship), Some(planet)) => Some(ship.min(planet)),
            (ship, planet) => ship.or(planet),
        }
    }
}

impl ServerWorld {
    /// The rail a ship is on if it coasts round the planet that pulls it most, at `time`, or why it cannot be on one. Nothing changes. It is the orbit that the ship's place (its
    /// centre of mass) and its speed are on, with the pull of that planet alone.
    pub fn coasting_rail(&self, grid: GridId, time: RailTime) -> Result<Rail, NotCoasting> {
        self.coasting_rail_with(None, grid, time)
    }

    /// How a ship flies against the planet that pulls it most at `time`, whatever it is doing (pushed, held or turning: it need not be a ship that can go on a rail), or `None` for a grid that is
    /// a planet or that no planet pulls. The orbit is made as a rail's is, from the place and the speed, with the pull of the planet now. Nothing changes.
    pub fn flight_state(&self, grid: GridId, time: RailTime) -> Option<FlightState> {
        let ship = self.data.grids().get(grid)?;
        if ship.terrain.is_some() {
            return None;
        }
        let middle = self.middle_of(ship)?;
        let (parent, planet, place) = self.planet_that_pulls_most(None, grid, ship, middle)?;
        let velocity: [f64; 3] =
            [0, 1, 2].map(|axis| ship.linear_velocity[axis] - planet.linear_velocity[axis]);
        let orbit = Orbit::from_place(
            Centre::Grid(parent),
            place,
            velocity,
            self.mass_parameter_of(planet),
            time,
        );
        Some(FlightState {
            planet: parent,
            place,
            velocity,
            orbit,
        })
    }

    /// As [`ServerWorld::coasting_rail`], with what is in the way of each ship already found for this tick (`known`), or looking at every grid.
    fn coasting_rail_with(
        &self,
        known: Option<&Neighbours>,
        grid: GridId,
        time: RailTime,
    ) -> Result<Rail, NotCoasting> {
        let Candidate {
            ship,
            parent,
            planet,
            place,
        } = self.coasting_candidate(known, grid)?;
        let orbit = self.orbit_from_the_state(ship, parent, planet, place, time)?;
        if let Some(near) = self.near_with(known, grid, ship, Some(parent), 2.0) {
            return Err(NotCoasting::SomethingNear(near));
        }
        Ok(Rail {
            spin: None,
            orbit: Some(orbit),
        })
    }

    /// Whether a ship passes the rules of [`ServerWorld::coasting_rail`] that do not need its orbit: it is a free ship that turns nothing, in a level with no pull of its own, with a
    /// planet that pulls it, and nothing near it. These are a few comparisons, the planets, and a search of the tree of the ships ([`neighbours`]). The rules that do need the orbit (it does not come too low, it does not leave the
    /// sphere of influence, it is one at all) cost an orbit made from a state, 114 to 145 microseconds, and are asked only when the second of quiet is up.
    fn may_be_put_on_a_rail(
        &self,
        known: Option<&Neighbours>,
        grid: GridId,
    ) -> Result<(), NotCoasting> {
        let candidate = self.coasting_candidate(known, grid)?;
        match self.near_with(known, grid, candidate.ship, Some(candidate.parent), 2.0) {
            Some(near) => Err(NotCoasting::SomethingNear(near)),
            None => Ok(()),
        }
    }

    /// The ship and the planet that pulls it, and where it is from the planet, if the rules of [`ServerWorld::may_be_put_on_a_rail`] that look at one ship are met.
    fn coasting_candidate(
        &self,
        known: Option<&Neighbours>,
        grid: GridId,
    ) -> Result<Candidate<'_>, NotCoasting> {
        let ship = self.data.grids().get(grid).ok_or(NotCoasting::NotAShip)?;
        if ship.terrain.is_some() {
            return Err(NotCoasting::NotAShip);
        }
        if ship.held {
            return Err(NotCoasting::Held);
        }
        // (A ship that turns may ride a rail, which holds its centre of mass and nothing else: the bodies do the turn. Not one that is coupled to another by a joint, until a rail
        // carries a group: its car would be on a rail of its own, in a frame of its own, and the joint would die with the body that has it.)
        if ship.angular_velocity != [0.0; 3]
            && self
                .coupled_ships
                .iter()
                .any(|&(first, second)| first == grid || second == grid)
        {
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
            .planet_that_pulls_most(known, grid, ship, middle)
            .ok_or(NotCoasting::NoPlanetPulls)?;
        Ok(Candidate {
            ship,
            parent,
            planet,
            place,
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
        self.orbit_fits.set(self.orbit_fits.get() + 1);
        let orbit = Orbit::from_place(
            Centre::Grid(parent),
            place,
            relative_velocity,
            self.mass_parameter_of(planet),
            time,
        )
        .map_err(NotCoasting::NoOrbit)?;
        self.is_not_too_low(&orbit, parent, planet, time)?;
        // A ship that is out of the planet's sphere already goes round what the planet goes round; one that is in it and goes out of it later is handed over when it does.
        if let Some(sphere) = self.sphere_of_influence(parent) {
            let distance = size(place);
            if distance >= sphere {
                return Err(NotCoasting::PastTheSphere {
                    farthest: orbit.farthest_distance().max(distance),
                    sphere,
                });
            }
        }
        Ok(orbit)
    }

    /// Whether an orbit round `planet` stays above its ground and what it carries, from `time` on.
    fn is_not_too_low(
        &self,
        orbit: &Orbit,
        parent: GridId,
        planet: &MovingGrid,
        time: RailTime,
    ) -> Result<(), NotCoasting> {
        let lowest = orbit.nearest_distance();
        let least = self.least_to_go_round(parent, planet);
        // (A hyperbola that is going away has passed its nearest point: it is behind the ship, and a rail is where the ship goes.)
        if lowest < least && orbit.periapsis_ahead(time) {
            return Err(NotCoasting::TooLow { lowest, least });
        }
        Ok(())
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
        known: Option<&Neighbours>,
        grid: GridId,
        ship: &MovingGrid,
        middle: WorldPosition,
    ) -> Option<(GridId, &MovingGrid, [f64; 3])> {
        let threshold = self.gravity.pull_mass_threshold?;
        let mut best: Option<(f64, GridId, &MovingGrid, [f64; 3])> = None;
        // The grids with terrain are all there is to look at: the snapshot of a tick has them, and a ship does not look at every ship for them.
        let grids: Box<dyn Iterator<Item = (GridId, &MovingGrid)> + '_> = match known {
            Some(known) => Box::new(
                known
                    .planets
                    .iter()
                    .filter_map(|(id, _)| Some((*id, self.data.grids().get(*id)?))),
            ),
            None => Box::new(self.data.grids().iter()),
        };
        for (id, planet) in grids {
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
    pub(super) fn sphere_of_influence(&self, planet: GridId) -> Option<f64> {
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
    /// are not counted: the orbit has its own rule for them. The first of the grids in the order of their numbers is the one that is given.
    ///
    /// What is in the way of each ship is either found once for the tick (`known`, which [`ServerWorld::update_coasting`] makes: a tick costs what the ships have near them) or
    /// looked for among every grid for this one (`None`). **The two give the same answers**, which the tests of `near_ships.rs` pin against the program that always looked at
    /// every grid.
    pub(super) fn near_with(
        &self,
        known: Option<&Neighbours>,
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
        let mine = match known.and_then(|known| known.bodies.get(&grid)) {
            Some(body) => {
                debug_assert_eq!(
                    *body,
                    Body::of(ship),
                    "a ship moved while its neighbours were known"
                );
                *body
            }
            None => Body::of(ship),
        };
        let in_the_way = match known {
            Some(known) => known.in_the_way(self, grid, &mine, &chain, factor),
            None => self.in_the_way_looking_at_every_grid(&mine, &chain, factor),
        };
        if in_the_way.is_some() {
            return in_the_way;
        }
        // (A planet that is not what the ship goes round, and that the ship is within the sphere of influence of: there it pulls more than what the ship goes round.)
        let place = self.middle_of(ship)?;
        let (spheres_here, carriers_here);
        let (spheres, carriers): (&[Sphere], &[Carrier]) = match known {
            Some(known) => (&known.spheres, &known.carriers),
            None => {
                spheres_here = self.spheres();
                carriers_here = self.carriers();
                (&spheres_here, &carriers_here)
            }
        };
        for sphere in spheres {
            if chain.contains(&sphere.id) || sphere.level != ship.level {
                continue;
            }
            // (A ship that is on a rail may be at the edge of a sphere it has just come out of.)
            let radius = if factor > 1.0 {
                sphere.radius
            } else {
                sphere.radius * (1.0 - SPHERE_SLACK)
            };
            if size(sphere.position.displacement_to(place).in_voxels()) < radius {
                return Some(sphere.id);
            }
        }
        let limit = if factor > 1.0 { INFLUENCE_LEAVING } else { 1.0 };
        carriers
            .iter()
            .filter(|carrier| carrier.level == ship.level)
            .find(|carrier| {
                size(carrier.position.displacement_to(place).in_voxels()) <= carrier.radius * limit
            })
            .map(|carrier| carrier.id)
    }

    /// The first grid, in the order of their numbers, that is in the way of a ship in the sense of [`is_in_the_way`], not counting `chain`: every grid is looked at.
    fn in_the_way_looking_at_every_grid(
        &self,
        mine: &Body,
        chain: &[GridId],
        factor: f64,
    ) -> Option<GridId> {
        self.data
            .grids()
            .iter()
            .find(|(id, other)| {
                !chain.contains(id)
                    && other.level == mine.level()
                    && is_in_the_way(mine, &Body::of(other), factor)
            })
            .map(|(id, _)| id)
    }

    /// Every planet that has a sphere of influence, in the order of their numbers.
    fn spheres(&self) -> Vec<Sphere> {
        self.data
            .grids()
            .iter()
            .filter(|(_, other)| other.terrain.is_some())
            .filter_map(|(id, other)| {
                Some(Sphere {
                    id,
                    level: other.level,
                    position: other.pose.position,
                    radius: self.sphere_of_influence(id)?,
                })
            })
            .collect()
    }

    /// Hands over every ship on a rail of this module that has left the sphere of influence of the planet it goes round, at an instant at or before `time`: the ship goes on the orbit
    /// round what the planet goes round that its state is on **at the instant of the crossing**, and again for each sphere it has crossed since, so that it is where its path puts it at
    /// `time` whatever the rate of the ticks is and however long it was not looked at. A ship whose new orbit is no orbit a rail may have (it comes lower than the ground it now goes round,
    /// or what it now goes round pulls nothing) comes off its rail where it is.
    ///
    /// It is called by [`ServerWorld::apply_rails`], before the ships are put where their rails say, so that nothing looks at a ship on a rail it has left.
    pub(super) fn hand_over_what_has_left_its_sphere(&mut self, time: RailTime) {
        let ships: Vec<GridId> = self
            .data
            .grids()
            .iter()
            .filter(|(_, ship)| {
                ship.terrain.is_none() && ship.rail.as_ref().is_some_and(is_a_coasting_rail)
            })
            .map(|(id, _)| id)
            .collect();
        for ship in ships {
            self.hand_over(ship, time);
        }
        let grids = self.data.grids();
        self.exits.retain(|id, _| {
            grids
                .get(*id)
                .is_some_and(|ship| ship.rail.as_ref().is_some_and(is_a_coasting_rail))
        });
    }

    /// The hand-overs of one ship (see [`ServerWorld::hand_over_what_has_left_its_sphere`]).
    fn hand_over(&mut self, grid: GridId, time: RailTime) {
        for _ in 0..MOST_HAND_OVERS_AT_ONCE {
            let Some((orbit, instant)) = self.due_exit(grid, time) else {
                return;
            };
            match self.handed_over(grid, &orbit, instant) {
                Ok(()) => self.hand_overs += 1,
                Err(_) => {
                    self.quiet_since.remove(&grid);
                    let _ = self.set_grid_rail(grid, None);
                    return;
                }
            }
        }
    }

    /// The orbit of a ship and the instant it left the sphere of the planet it goes round, if that is at or before `time`. The instant is worked out when the orbit or the sphere is not
    /// the one it was worked out for, and not otherwise (see [`ExitPlan`]).
    fn due_exit(&mut self, grid: GridId, time: RailTime) -> Option<(Orbit, RailTime)> {
        let orbit = self.data.grids().get(grid)?.rail.as_ref()?.orbit.as_ref()?;
        let Centre::Grid(parent) = orbit.around else {
            return None;
        };
        let Some(sphere) = self.sphere_of_influence(parent) else {
            self.exits.remove(&grid);
            return None;
        };
        let cached = self
            .exits
            .get(&grid)
            .filter(|plan| plan.sphere == sphere && plan.orbit == *orbit)
            .map(|plan| plan.at);
        let (at, plan) = match cached {
            Some(at) => (at, None),
            None => {
                let (at, can_leave) = search_for_exit(orbit, sphere, time);
                let plan = ExitPlan {
                    orbit: orbit.clone(),
                    sphere,
                    at,
                };
                (at, Some((plan, can_leave)))
            }
        };
        let found = at
            .filter(|instant| instant.compare(time) != Ordering::Greater)
            .map(|instant| (orbit.clone(), instant));
        if let Some((plan, searched)) = plan {
            self.exits.insert(grid, plan);
            self.exit_searches += u64::from(searched);
        }
        found
    }

    /// Puts a ship on the orbit round what its planet goes round that its state is on at `instant`, if that orbit is one a rail may have.
    fn handed_over(
        &mut self,
        grid: GridId,
        orbit: &Orbit,
        instant: RailTime,
    ) -> Result<(), NotCoasting> {
        let handed_over = self.orbit_after(orbit, instant)?;
        let _ = self.set_grid_rail(
            grid,
            Some(Rail {
                spin: None,
                orbit: Some(handed_over),
            }),
        );
        Ok(())
    }

    /// The orbit round what the planet an orbit goes round goes round that a body on it is on at `instant` (when it leaves the planet's sphere), with the mass the planets have now and
    /// the widest rail the host allows, if that is an orbit a rail may have. Nothing changes: this is what the hand-over of a ship and a prediction of its path both ask.
    pub(super) fn orbit_after(
        &self,
        orbit: &Orbit,
        instant: RailTime,
    ) -> Result<Orbit, NotCoasting> {
        let Centre::Grid(parent) = orbit.around else {
            return Err(NotCoasting::NotAShip);
        };
        let centre = self
            .data
            .grids()
            .get(parent)
            .and_then(|planet| planet.rail.as_ref())
            .and_then(|rail| rail.orbit.as_ref())
            .ok_or(NotCoasting::NoPlanetPulls)?;
        let Centre::Grid(grand_parent) = centre.around else {
            return Err(NotCoasting::NoPlanetPulls);
        };
        let grand = self
            .data
            .grids()
            .get(grand_parent)
            .ok_or(NotCoasting::NoPlanetPulls)?;
        let handed_over = orbit
            .handed_over_to(
                centre,
                self.mass_parameter_of(grand),
                instant,
                self.rail_settings.widest_rail_bits,
            )
            .map_err(NotCoasting::NoOrbit)?;
        self.is_not_too_low(&handed_over, grand_parent, grand, instant)?;
        Ok(handed_over)
    }

    /// Whether a ship that is on a rail of this module has to come off it: it is held, is in a level with a pull of its own, goes round nothing that is there, or
    /// something is near. (Turning is no reason: the rail holds the centre of mass and the bodies turn the ship.)
    fn has_to_leave_its_rail(&self, known: Option<&Neighbours>, grid: GridId) -> bool {
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
            || self
                .level_gravity
                .get(&ship.level)
                .unwrap_or(&self.gravity.base)
                != &[0.0; 3]
            || self.data.grids().get(parent).is_none()
            || self
                .near_with(known, grid, ship, Some(parent), 1.0)
                .is_some()
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
            if orbit.is_a_hyperbola() {
                // A hyperbola has no apoapsis. A decrease cannot throw it out of anything, so it is answered at once; an increase is answered at the periapsis if that is
                // still ahead (the nearest point keeps its distance), and at once if it is behind (there is nothing to fall into that it has not passed).
                change < 0.0
                    || passed.periapsis
                    || !orbit.periapsis_ahead(RailTime::of_tick(now, ticks_per_second))
            } else if change > 0.0 {
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
    /// for [`QUIET_SECONDS`] and can coast is put on the orbit its place and speed are on, and one on a rail of this module that is held, has something near it, or goes
    /// round nothing is taken off, as it is, at the place and the speed the rail gave it.
    /// The orbit is made from the state of a ship (the costly part) only when its second is up: see the notes at the top of this module.
    pub(crate) fn update_coasting(&mut self, ticks_per_second: u32) {
        let now = self.data.clock();
        let time = RailTime::of_tick(now, ticks_per_second);
        let wait = QUIET_SECONDS * u64::from(ticks_per_second);
        // What is in the way of each ship is found once: nothing it is found from changes while the ships are gone through (a rail is all that does).
        let known = Neighbours::of(self);
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
                    if self.has_to_leave_its_rail(Some(&known), id) {
                        self.quiet_since.remove(&id);
                        let _ = self.set_grid_rail(id, None);
                    } else {
                        self.answer_a_change_of_mass(id, ticks_per_second);
                    }
                }
                Some(false) => {}
                None => match self.may_be_put_on_a_rail(Some(&known), id) {
                    Ok(()) => {
                        let since = *self.quiet_since.entry(id).or_insert(now);
                        if now.saturating_sub(since) >= wait {
                            // The second is up: only now is the orbit made from the state, to see whether it is one the ship may have. If it is not, the second begins again.
                            self.quiet_since.remove(&id);
                            if let Ok(rail) = self.coasting_rail_with(Some(&known), id, time) {
                                let _ = self.set_grid_rail(id, Some(rail));
                            }
                        }
                    }
                    Err(_) => {
                        self.quiet_since.remove(&id);
                    }
                },
            }
        }
        self.ship_pairs_asked
            .set(self.ship_pairs_asked.get() + known.ships.pairs_tested());
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
