//! Ships that coast: a free ship far from everything it could touch goes on a rail, and comes off it when anything happens to it (`docs/orbits-and-thrust-plan.md`, step B).
//!
//! A ship that nothing pushes and nothing is near is not simulated: it goes round the planet that pulls it most on the orbit its place and its speed are on, and its place
//! is a function of the time of the world (the rail of `world_storage::rail`, which holds exactly however long it goes on). Observed or not, a ship on a rail is where the clock
//! says, and a ship that is saved on one and brought back is where the clock says too. The bodies hold a ship on a rail where it is put.
//!
//! **What is ruled out, and why.** A rail is a conic round one planet (patched conics: the planet's pull and no other, as the owner chose, 2026-11-01), so a ship is put on one only
//! where that is the motion it would have. It is not put on one, and comes off one it is on, when:
//! - it is held, or its level has a pull of its own (a level with a ground); (it may turn: a rail holds a ship's place and not its turn, and the turn is the bodies');
//! - something pushes it: a docking, a program that puts it somewhere, anything that is not the bodies moving it, and a force or a torque on a ship that is in a group or on no rail
//!   (**a force or a torque on a ship that rides a rail alone does not take it off it**, step 4c-5-4b: the force is the rail's, which is made again with the speed it gave, and the
//!   torque is the bodies');
//! - it is near what it could touch (another ship, or a planet that is not the one it goes round), nearer than a margin and the way it would go in a second at the speed it
//!   closes at, so that nothing meets it while it is on a rail; or it is in the sphere that a planet on a rail carries what is near it with. **A group of ships coupled by their joint
//!   ends is the exception for other ships** (the rail's kick, 2026-11-17, `docs/decisions/joints-and-cars.md`, 4c-5-4): what is near it is the contact ghost's, which the bodies make, and
//!   it comes off its rail at a touch (the bodies tell the world: [`ServerWorld::take_group_off_its_rail`], with the kick of the hit in the speed of its ships) or for the ends of a
//!   ship that pull on its own; planets, spheres and carriers are asked of it as of any ship;
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
use neighbours::{Body, ShipsInTheWay, ground_in_the_way, is_in_the_way};
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
    /// The ship is coupled to others by joints, and the rail of a group is on its anchor, the ship of the group with the lowest number (the one given).
    InAGroup(GridId),
    /// A ship of the group is not in memory (it is in a file), so there is no centre of mass of the group to put on a rail.
    GroupNotWhole,
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

/// A ship, or a group of ships, that passes the rules that look at one ship or group, with the planet that pulls it and where its centre of mass is from the planet's middle.
struct Candidate<'a> {
    /// The ships: just the one, or the group, lowest number first.
    members: Vec<GridId>,
    parent: GridId,
    planet: &'a MovingGrid,
    place: [f64; 3],
    /// How fast the centre of mass goes in the world.
    velocity: [f64; 3],
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
    rail.is_coasting()
}

/// A group of ships coupled by their joint ends that rides a rail: the ship with the lowest number holds the rail, and the rail is the orbit of the centre of mass of the group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RailGroup {
    pub anchor: GridId,
    /// The ships, lowest number first (the anchor is the first).
    pub members: Vec<GridId>,
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
            members,
            parent,
            planet,
            place,
            velocity,
        } = self.coasting_candidate(known, grid)?;
        let orbit = self.orbit_from_the_state(parent, planet, place, velocity, time)?;
        if let Some(near) = self.near_group(known, &members, Some(parent), 2.0) {
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
        match self.near_group(known, &candidate.members, Some(candidate.parent), 2.0) {
            Some(near) => Err(NotCoasting::SomethingNear(near)),
            None => Ok(()),
        }
    }

    /// The ship, or the group of ships, and the planet that pulls it, and where its centre of mass is from the planet, if the rules of [`ServerWorld::may_be_put_on_a_rail`] that look
    /// at one ship or group are met. **A group of ships** (ships coupled by their joint ends) has one rail, on the ship of it with the lowest number, and the rail is the orbit of its
    /// centre of mass: the centres of mass of the ships, and their speeds, weighed by their masses (a ship that turns is no reason for anything: a rail holds the centre of mass and
    /// nothing else, and the turn is the bodies'). A ship of a group that is not its anchor is refused, and so is a group of which a ship is in a file.
    fn coasting_candidate(
        &self,
        known: Option<&Neighbours>,
        grid: GridId,
    ) -> Result<Candidate<'_>, NotCoasting> {
        let ship = self.data.grids().get(grid).ok_or(NotCoasting::NotAShip)?;
        if ship.terrain.is_some() {
            return Err(NotCoasting::NotAShip);
        }
        let members = self.data.grids().group_of(grid);
        let grouped = members.len() > 1;
        if grouped && members[0] != grid {
            return Err(NotCoasting::InAGroup(members[0]));
        }
        for member in &members {
            let ship = self
                .data
                .grids()
                .get(*member)
                .ok_or(NotCoasting::GroupNotWhole)?;
            if ship.terrain.is_some() {
                return Err(NotCoasting::NotAShip);
            }
            if ship.held {
                return Err(NotCoasting::Held);
            }
        }
        if self.level_pulls(ship.level) {
            return Err(NotCoasting::LevelPulls);
        }
        let (middle, velocity) = if grouped {
            let centre = self
                .data
                .grids()
                .group_centre(&members)
                .ok_or(NotCoasting::GroupNotWhole)?;
            (centre.position, centre.velocity)
        } else {
            (
                self.middle_of(ship).ok_or(NotCoasting::NoPlanetPulls)?,
                ship.linear_velocity,
            )
        };
        let (parent, planet, place) = self
            .planet_that_pulls_most(known, grid, ship, middle)
            .ok_or(NotCoasting::NoPlanetPulls)?;
        Ok(Candidate {
            members,
            parent,
            planet,
            place,
            velocity,
        })
    }

    /// Whether a level has a pull of its own, which a level in space has none of: what is in it falls to what weighs enough, and that is all.
    fn level_pulls(&self, level: LevelIndex) -> bool {
        self.level_gravity.get(&level).unwrap_or(&self.gravity.base) != &[0.0; 3]
    }

    /// Whether the group of ships coupled by their joint ends that `anchor` (the ship of it with the lowest number) speaks for could be **alone**, so that the bodies may give it a
    /// frame of its own, centred on its centre of mass and in free fall (step 4c-4 of `docs/decisions/joints-and-cars.md`), **whatever the ground of a planet is near it**: it is two
    /// ships or more, in a level with no pull of its own, free (not held, not frozen, no rail of its own) and weighed. **Other ships are no
    /// part of the rule** (step 4c-5-2: what is near a group is the contact ghost's, and the group keeps its frame whatever is near; the bodies add that a ship with joint ends or
    /// docking ports that pull on its own takes it out of its frame, which the world does not know).
    ///
    /// **Whether a planet carries the group is no part of it** (2026-11-16, the owner: continue carrying everything with the planet, and leave frames centred on groups that do not
    /// know they are moving through space). A planet carries a group as it carries any ship, whole and by its anchor (`apply_rails`, through `pick_carrier`: what is within its reach
    /// of what it carries, and what it carried a quarter farther out; see [`ServerWorld::carrier_of`]), and the group's frame goes with the planet's displacement as well as with
    /// its own motion while the world carries it (`grid_bodies`: the follow of a frame of its own has a carrier), so that the planet's speed is in none of the numbers of what the
    /// group does. **The sphere of influence of a planet is not part of the rule either** (which planet a rail goes round is nothing to a group off a rail, which is pulled by every
    /// planet's pull at its point: `gravity_at`), **and there is no room**. Blocks built into the level itself are not looked at, as for a frame that goes with a planet. **The ground
    /// of a planet is not asked about here** (step 4c-5-3: [`ServerWorld::ground_is_in_the_way_of_group`], and the bodies decide what it means: a group that comes down to the
    /// ground keeps its frame, with the ground in it, until it touches it).
    pub fn group_is_alone_but_for_the_ground(&self, anchor: GridId) -> bool {
        let members = self.data.grids().group_of(anchor);
        if members.len() < 2 || members[0] != anchor {
            return false;
        }
        let mut level = None;
        for member in &members {
            let Some(ship) = self.data.grids().get(*member) else {
                return false;
            };
            if ship.terrain.is_some()
                || ship.held
                || ship.frozen
                || ship.rail.is_some()
                || ship.mass.is_nan()
                || ship.mass <= 0.0
                || level.is_some_and(|level| level != ship.level)
            {
                return false;
            }
            level = Some(ship.level);
        }
        let Some(level) = level else {
            return false;
        };
        if self.level_pulls(level) {
            return false;
        }
        // (**Other ships are not asked about**: what is near a group is the ghost's. **Nor is the carrying**: see above.)
        true
    }

    /// Whether the ground of a planet is within the lookahead of the terrain of a ship of the group (`ground_in_the_way`: a margin and a quarter of a second of the speed they close at,
    /// `factor` times over; a planet is no ship to the speed rule, which counts a second of the speed two things close at, and the group of a ship that orbits low would be near the
    /// planet it orbits all the time). It is the room in which the bodies have the ground of the planet in the group's frame, as bodies that come at it.
    pub fn ground_is_in_the_way_of_group(&self, anchor: GridId, factor: f64) -> bool {
        let members = self.data.grids().group_of(anchor);
        let Some(level) = members
            .first()
            .and_then(|member| self.data.grids().get(*member))
            .map(|ship| ship.level)
        else {
            return false;
        };
        let planets = self.planets_of(level);
        members.iter().any(|member| {
            let Some(ship) = self.data.grids().get(*member) else {
                return false;
            };
            let mine = Body::of(ship);
            planets.iter().any(|planet| {
                self.data
                    .grids()
                    .get(*planet)
                    .is_some_and(|planet| ground_in_the_way(&mine, &Body::of(planet), factor))
            })
        })
    }

    /// The planets of a level: the grids that have terrain.
    fn planets_of(&self, level: LevelIndex) -> Vec<GridId> {
        self.data
            .grids()
            .iter()
            .filter(|(_, other)| other.terrain.is_some() && other.level == level)
            .map(|(id, _)| id)
            .collect()
    }

    /// The orbit a ship's speed and `place` (its centre of mass from the middle of the planet) are on round `planet`, with the mass the planet has now, if it is one the ship may go on a
    /// rail in: it does not come lower than the ground and what the planet carries, and does not go out of the planet's sphere of influence.
    fn orbit_from_the_state(
        &self,
        parent: GridId,
        planet: &MovingGrid,
        place: [f64; 3],
        velocity: [f64; 3],
        time: RailTime,
    ) -> Result<Orbit, NotCoasting> {
        let relative_velocity: [f64; 3] =
            [0, 1, 2].map(|axis| velocity[axis] - planet.linear_velocity[axis]);
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
        self.near_with_group(known, grid, ship, goes_round, factor, &[])
    }

    /// As [`ServerWorld::near_with`], not counting the ships of `group` either: the ships coupled to this one are with it, and not in its way.
    fn near_with_group(
        &self,
        known: Option<&Neighbours>,
        grid: GridId,
        ship: &MovingGrid,
        goes_round: Option<GridId>,
        factor: f64,
        group: &[GridId],
    ) -> Option<GridId> {
        let mut chain = goes_round.map_or_else(Vec::new, |parent| {
            let mut chain = self.what_it_goes_round(parent);
            chain.push(parent);
            chain
        });
        chain.push(grid);
        chain.extend_from_slice(group);
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
        self.sphere_or_carrier_in_the_way(known, ship, &chain, factor)
    }

    /// The part of [`ServerWorld::near_with_group`] that is not about other ships: a planet whose sphere of influence the ship is in (not counting `chain`), or a planet on a rail
    /// that carries it.
    fn sphere_or_carrier_in_the_way(
        &self,
        known: Option<&Neighbours>,
        ship: &MovingGrid,
        chain: &[GridId],
        factor: f64,
    ) -> Option<GridId> {
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

    /// As [`ServerWorld::near_with`] for the ships of a group together: some grid is in the way of the group if it is in the way of a ship of it, and the ships of the group are not
    /// in each other's way. The first found, a ship of the group at a time.
    fn near_group(
        &self,
        known: Option<&Neighbours>,
        members: &[GridId],
        goes_round: Option<GridId>,
        factor: f64,
    ) -> Option<GridId> {
        members.iter().find_map(|member| {
            let ship = self.data.grids().get(*member)?;
            // (A ship that is alone is asked as it always was.)
            match members {
                [_] => self.near_with(known, *member, ship, goes_round, factor),
                _ => self.near_with_group(known, *member, ship, goes_round, factor, members),
            }
        })
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
    /// something is near. (Turning is no reason: the rail holds the centre of mass and the bodies turn the ship.) For a group, a ship of it that is held or is in a file, or
    /// something near any ship of it. **A ship is no grid that is in the way of another** (step 4c-5-4c, 2026-11-18): a ship on a rail, alone or in a group, keeps it while another
    /// ship is near, and a touch takes it (the contact ghost's, and the rail's kick); what is in the way is a planet, a sphere of influence or a carrier.
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
        let members = self.data.grids().group_of(grid);
        // (No ship is in the way of a ship or a group that rides a rail: the contact ghost has what is near it, and a touch takes it off its rail. **Except for a ship that is alone
        // and frozen**: it has no body, so no frame of its own to be hit in, and a stranger would meet it as a fixed wall that goes round at the speed of its rail; it comes off its
        // rail for a ship that is near, as every ship did.)
        let in_the_way = if members.len() == 1 && ship.frozen {
            self.near_group(known, &members, Some(parent), 1.0)
        } else {
            self.ground_near_group(known, &members, Some(parent), 1.0)
        };
        members
            .iter()
            .any(|member| self.data.grids().get(*member).is_none_or(|ship| ship.held))
            || self
                .level_gravity
                .get(&ship.level)
                .unwrap_or(&self.gravity.base)
                != &[0.0; 3]
            || self.data.grids().get(parent).is_none()
            || in_the_way.is_some()
    }

    /// As [`ServerWorld::near_group`], **but for the grids that are not ships**: a planet that is in the way of a ship of the group, a planet whose sphere of influence it is in, or a
    /// planet on a rail that carries it. Other ships are not asked about (a group on a rail keeps it while a ship is near, and the bodies' contact ghost has the ship: see the head of
    /// this module). Everything else is as for [`ServerWorld::near_with_group`].
    fn ground_near_group(
        &self,
        known: Option<&Neighbours>,
        members: &[GridId],
        goes_round: Option<GridId>,
        factor: f64,
    ) -> Option<GridId> {
        members.iter().find_map(|member| {
            let ship = self.data.grids().get(*member)?;
            let mut chain = goes_round.map_or_else(Vec::new, |parent| {
                let mut chain = self.what_it_goes_round(parent);
                chain.push(parent);
                chain
            });
            chain.extend_from_slice(members);
            let mine = known
                .and_then(|known| known.bodies.get(member))
                .copied()
                .unwrap_or_else(|| Body::of(ship));
            let among_planets = match known {
                Some(known) => known
                    .planets
                    .iter()
                    .find(|(id, other)| {
                        !chain.contains(id)
                            && other.level() == mine.level()
                            && is_in_the_way(&mine, other, factor)
                    })
                    .map(|(id, _)| *id),
                None => self
                    .data
                    .grids()
                    .iter()
                    .find(|(id, other)| {
                        other.terrain.is_some()
                            && !chain.contains(id)
                            && other.level == mine.level()
                            && is_in_the_way(&mine, &Body::of(other), factor)
                    })
                    .map(|(id, _)| id),
            };
            among_planets.or_else(|| self.sphere_or_carrier_in_the_way(known, ship, &chain, factor))
        })
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
        let members = self.data.grids().group_of(grid);
        let state = if members.len() > 1 {
            self.data
                .grids()
                .group_centre(&members)
                .map(|centre| (centre.position, centre.velocity))
        } else {
            self.middle_of(ship)
                .map(|middle| (middle, ship.linear_velocity))
        };
        let fitted = state.and_then(|(middle, velocity)| {
            let place = planet.pose.position.displacement_to(middle).in_voxels();
            self.orbit_from_the_state(
                parent,
                planet,
                place,
                velocity,
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
        self.kick_rails_by_forces(time, ticks_per_second);
        let wait = QUIET_SECONDS * u64::from(ticks_per_second);
        // What is in the way of each ship is found once: nothing it is found from changes while the ships are gone through (a rail is all that does).
        let known = Neighbours::of(self);
        self.railed_with_a_stranger_near.clear();
        let ships: Vec<GridId> = self
            .data
            .grids()
            .iter()
            .filter(|(_, ship)| ship.terrain.is_none())
            .map(|(id, _)| id)
            .collect();
        for id in ships {
            // A group of ships has one rail, on its anchor: a ship of it that is not the anchor is not asked (the group is, in its name), and a rail it has was made for
            // something else.
            if self.data.grids().has_partners(id) && self.data.grids().group_of(id)[0] != id {
                self.quiet_since.remove(&id);
                self.take_a_coasting_rail_from(id);
                continue;
            }
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
                        // (A ship that is alone on its rail and has another ship in its way, by the room that would have taken it off the rail before, is told to the bodies, which give
                        // it a frame of its own for as long as it is so.)
                        if !self.data.grids().has_partners(id)
                            && known
                                .ships
                                .lowest(id, 1.0, &[id])
                                .is_some_and(|found| found.is_some())
                        {
                            self.railed_with_a_stranger_near.insert(id);
                        }
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

    /// Whether a ship rides a rail of this module **alone**, so that a push does not take it off: it has a coasting rail (an orbit round a planet that is still there, no spin of its
    /// own), no partners (a group's rail is its anchor's and holds the centre of mass of all of them: a push on a ship of a group is [`ServerWorld::rides_a_rail_in_a_group`]'s), is no terrain, and weighs something. A force or a torque
    /// asked of such a ship leaves it on its rail ([`ServerWorld::apply_grid_force`]), and so does the pull of a docking port (the bodies ask it in the same way).
    pub fn rides_a_rail_alone(&self, grid: GridId) -> bool {
        let grids = self.data.grids();
        grids.get(grid).is_some_and(|ship| {
            ship.terrain.is_none()
                && !grids.has_partners(grid)
                && ship.mass.is_finite()
                && ship.mass > 0.0
                && ship.rail.as_ref().is_some_and(|rail| {
                    is_a_coasting_rail(rail)
                        && rail.orbit.as_ref().is_some_and(|orbit| {
                            matches!(orbit.around, Centre::Grid(parent) if grids.get(parent).is_some())
                        })
                })
        })
    }

    /// The anchor of the group that this ship is of, if the group **rides a rail that a push leaves it on** (step 4c-5-4b of `docs/decisions/joints-and-cars.md`, for groups): the group
    /// rides a coasting rail (`rail_group_of`: held by its anchor, whole, in memory), the orbit goes round a planet that is there, and every ship of it weighs something (the centre of
    /// mass the rail holds is weighed by their masses, and what a force does to it is the force over the mass of all of them). `None` for a ship that is alone (that is
    /// [`ServerWorld::rides_a_rail_alone`]), for a group that is on no rail, and for a group that has a ship that is not weighed.
    pub fn rides_a_rail_in_a_group(&self, grid: GridId) -> Option<GridId> {
        let group = self.rail_group_of(grid)?;
        let grids = self.data.grids();
        let goes_round_a_planet = grids
            .get(group.anchor)?
            .rail
            .as_ref()?
            .orbit
            .as_ref()
            .is_some_and(
                |orbit| matches!(orbit.around, Centre::Grid(parent) if grids.get(parent).is_some()),
            );
        let all_weigh = group.members.iter().all(|member| {
            grids.get(*member).is_some_and(|ship| {
                ship.terrain.is_none() && ship.mass.is_finite() && ship.mass > 0.0
            })
        });
        (goes_round_a_planet && all_weigh).then_some(group.anchor)
    }

    /// What a push on the rail of this ship moves: the ship's mass if it rides a rail alone, and the mass of all the ships of its group if it is the anchor of one that rides a rail
    /// (see [`ServerWorld::rides_a_rail_in_a_group`]); `None` for anything else.
    fn mass_that_the_rail_carries(&self, grid: GridId) -> Option<f64> {
        let grids = self.data.grids();
        if self.rides_a_rail_alone(grid) {
            return grids.get(grid).map(|ship| ship.mass);
        }
        let anchor = self.rides_a_rail_in_a_group(grid)?;
        // (A ship that is not the anchor has no rail of its own to push: `kicked_rail` finds none.)
        Some(
            grids
                .group_of(anchor)
                .iter()
                .filter_map(|member| grids.get(*member))
                .map(|ship| ship.mass)
                .sum(),
        )
    }

    /// The torque, in the world's directions, that a force makes about the centre of mass of a ship if it is applied at `at` (a place on the ship in its own coordinates): the lever from the
    /// centre of mass to the place, crossed with the force. `None` if the force is at the centre of mass, or the world cannot say where the place is.
    pub(super) fn torque_of_a_force_at(
        &self,
        grid: GridId,
        force: [f64; 3],
        at: [f64; 3],
    ) -> Option<[f64; 3]> {
        let ship = self.data.grids().get(grid)?;
        let point = ship
            .pose
            .local_to_world(WorldPosition::new(BrickCoordinate::ORIGIN, at))?;
        let lever = self.middle_of(ship)?.displacement_to(point).in_voxels();
        let torque = [
            lever[1] * force[2] - lever[2] * force[1],
            lever[2] * force[0] - lever[0] * force[2],
            lever[0] * force[1] - lever[1] * force[0],
        ];
        (torque != [0.0; 3]).then_some(torque)
    }

    /// Makes the rail of each ship that was pushed at its centre of mass while it rode a rail alone (see [`ServerWorld::apply_grid_force`]) again, **through the place it has at `time` and the
    /// speed it has there, exact, with the change of speed of the force added** (the force over the mass, for the tick): `Orbit::with_speed_changed_at`. The same for the rail of a group, which
    /// is its anchor's and is of the centre of mass of all its ships: the mass is that of all of them. The forces asked of a ship or a group in the tick are one push, their sum. A rail
    /// that is gone by now (something else took it off in the tick), or whose new orbit is no orbit, is disturbed as a ship that is on no rail is, and **the force that the bodies have not
    /// been given is theirs in the next step** (a ship alone is given none, and a group's ships were given theirs when it was asked).
    fn kick_rails_by_forces(&mut self, time: RailTime, ticks_per_second: u32) {
        if self.rail_forces.is_empty() {
            return;
        }
        // (For each rail: the whole of the force, and the part of it that the bodies were not given.)
        let mut sums: BTreeMap<GridId, ([f64; 3], [f64; 3])> = BTreeMap::new();
        for (grid, force, bodies_have_it) in std::mem::take(&mut self.rail_forces) {
            let (sum, owed) = sums.entry(grid).or_insert(([0.0; 3], [0.0; 3]));
            for axis in 0..3 {
                sum[axis] += force[axis];
                if !bodies_have_it {
                    owed[axis] += force[axis];
                }
            }
        }
        let seconds = 1.0 / f64::from(ticks_per_second);
        for (grid, (force, owed)) in sums {
            let kicked = self
                .mass_that_the_rail_carries(grid)
                .and_then(|mass| self.kicked_rail(grid, mass, force, seconds, time));
            match kicked {
                Some(rail) => {
                    let _ = self.set_grid_rail(grid, Some(rail));
                }
                None => {
                    self.disturb(grid);
                    if owed != [0.0; 3] {
                        self.grid_forces.push(GridForce {
                            grid,
                            force: owed,
                            at: None,
                        });
                    }
                }
            }
        }
    }

    /// The rail of a ship that rides one alone, or of a group (its anchor), once `force` has pushed what weighs `mass` for `seconds`, made at `time`; `None` if no orbit is made.
    fn kicked_rail(
        &self,
        grid: GridId,
        mass: f64,
        force: [f64; 3],
        seconds: f64,
        time: RailTime,
    ) -> Option<Rail> {
        let ship = self.data.grids().get(grid)?;
        let orbit = ship.rail.as_ref()?.orbit.as_ref()?;
        let Centre::Grid(parent) = orbit.around else {
            return None;
        };
        let planet = self.data.grids().get(parent)?;
        let change = force.map(|part| part / mass * seconds);
        let orbit = orbit
            .with_speed_changed_at(time, change, self.mass_parameter_of(planet))
            .ok()?;
        Some(Rail {
            spin: None,
            orbit: Some(orbit),
        })
    }

    /// Notes that something other than the bodies has done something to a ship: it is not quiet, and if it is on a rail of this module it comes off it, where it is (a rail
    /// is the place the ship is put at, so whatever is done to the ship is done from there). **A ship of a group is the group's to disturb**: none of the ships coupled to it is
    /// quiet, and the group's rail, on its anchor, comes off.
    pub(super) fn disturb(&mut self, grid: GridId) {
        for member in self.data.grids().group_of(grid) {
            self.quiet_since.remove(&member);
            self.take_a_coasting_rail_from(member);
        }
    }

    /// Takes a group of ships off the rail it is on, where it is, as the bodies have it: for **a hit** (the rail's kick: the bodies have written the group's new speed to its ships, the
    /// exact change that the contact gave it, and from here the group goes on from where it is with that), and for **ends that pull** on each other (a coupling or a docking to be made
    /// needs the one frame). It is not quiet from this tick, so it is put on a rail again after `QUIET_SECONDS` with nothing near it (`anchor` is any ship of the group).
    pub fn take_group_off_its_rail(&mut self, anchor: GridId) {
        self.disturb(anchor);
    }

    /// Takes the rail of this module from a ship, if it has one.
    fn take_a_coasting_rail_from(&mut self, grid: GridId) {
        if let Some(ship) = self.data.grids_mut().get_mut(grid)
            && ship.terrain.is_none()
            && ship.rail.as_ref().is_some_and(is_a_coasting_rail)
        {
            ship.rail = None;
        }
    }

    /// Whether this ship is alone, rides a rail of this module, and had another ship in its way (by the room that took a ship off its rail before the rail's kick: a margin of 64
    /// voxels and a second of the speed they close at) when the ships were last looked at, in `update_coasting`. **The bodies give such a ship a frame of its own, a free body at the
    /// rail's point that is a side of the contact ghost, for as long as it is so** (step 4c-5-4c of `docs/decisions/joints-and-cars.md`): a ship alone on a rail has no body of its
    /// own otherwise (a stranger meets it as a wall that cannot move), and nothing could be hit that does not have one. It is the answer of the last tick: `update_coasting` is
    /// the last of what a tick does to the ships, and the room is a second wide, so the frame is there some ticks before anything can touch.
    pub fn has_a_stranger_near_on_its_rail(&self, ship: GridId) -> bool {
        self.railed_with_a_stranger_near.contains(&ship)
    }

    /// The group of ships coupled by joints that this ship is in, if the group is on a rail: its rail is on its anchor, and holds the centre of mass of the ships. `None` for a ship
    /// that is alone, whatever it is doing, and for a group of which a ship is in a file or whose anchor has no rail of this module. The bodies hold such a group in a frame that goes
    /// with the rail's point.
    pub fn rail_group_of(&self, ship: GridId) -> Option<RailGroup> {
        let grids = self.data.grids();
        if !grids.has_partners(ship) {
            return None;
        }
        let members = grids.group_of(ship);
        let anchor = *members.first()?;
        let on_a_rail = grids.get(anchor).is_some_and(|anchor| {
            anchor.terrain.is_none() && anchor.rail.as_ref().is_some_and(is_a_coasting_rail)
        });
        let whole = members.iter().all(|member| grids.get(*member).is_some());
        (on_a_rail && whole).then_some(RailGroup { anchor, members })
    }

    /// Where the centre of mass of the group on a rail whose anchor this is, is in the world, and how fast it goes: the rail's point, as the ships are put at it. `None` if the
    /// ship is not the anchor of a group on a rail.
    pub fn rail_group_centre(&self, anchor: GridId) -> Option<(WorldPosition, [f64; 3])> {
        let group = self.rail_group_of(anchor)?;
        if group.anchor != anchor {
            return None;
        }
        let centre = self.data.grids().group_centre(&group.members)?;
        Some((centre.position, centre.velocity))
    }
}
