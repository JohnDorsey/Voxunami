//! `ServerWorld::near` (`server_world/coasting.rs`): which grid, if any, is in the way of a ship. It is asked of every ship that is not pushed, every tick, so how it finds the
//! answer matters as much as what the answer is. These pin the answer on a crowded world, to the bit of the decision, from the program as it was when `near` looked at every
//! grid for every ship (2026-11-03); they must pass unchanged for any way of finding it that is meant to give the same answers.

use voxel_space::{BRICK_COORDINATE_MAXIMUM, BrickCoordinate};
use world_storage::rail::Centre;

use super::coasting::Neighbours;
use super::coasting::neighbours::{Body, ShipsInTheWay, is_in_the_way};
use super::coasting_ships::{TICKS, ship_at, space};
use super::*;

/// SplitMix64: the same numbers on every machine, and nothing to depend on.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// From -1 up to 1.
    fn signed(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 52) as f64 - 1.0
    }

    fn below(&mut self, count: u64) -> u64 {
        self.next() % count
    }

    /// From 0 up to 1.
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// A planet, a moon on a rail round it (which carries what is near it and has a sphere of influence), and `count` ships of four kinds: spread over a cube 200 km across; in
/// clusters (within 150 voxels of the one before); round the moon; and very far out. Their speeds are none, slow, fast or orbital. Returns the world, the planet and the ships
/// in order of their numbers.
pub(super) fn crowded_world(seed: u64, count: usize) -> (ServerWorld, GridId, Vec<GridId>) {
    let (mut world, planet) = space();
    let moon = planet_on(
        &mut world,
        20,
        1.0e19,
        Some(orbit_about(Centre::Grid(planet), 60_000.0, 5_000.0)),
    );
    world.data.set_clock(0);
    world.apply_rails(TICKS);
    let moon_at = world
        .data()
        .grids()
        .get(moon)
        .unwrap()
        .pose
        .position
        .in_voxels();
    let mut random = Random(seed);
    let mut ships: Vec<GridId> = Vec::new();
    let mut last = [0.0; 3];
    for index in 0..count {
        let place = match index % 4 {
            0 => [0, 1, 2].map(|_| 100_000.0 * random.signed()),
            1 => [0, 1, 2].map(|axis| last[axis] + 150.0 * random.signed()),
            2 => [0, 1, 2].map(|axis| moon_at[axis] + 3_000.0 * random.signed()),
            _ => [0, 1, 2].map(|_| 5.0e6 * random.signed()),
        };
        let scale = [0.0, 30.0, 800.0, 9_000.0][random.below(4) as usize];
        let velocity = [0, 1, 2].map(|_| scale * random.signed());
        ships.push(ship_at(&mut world, place, velocity));
        last = place;
    }
    (world, planet, ships)
}

/// FNV-1a, 64 bits.
fn hash_of(text: &str, mut hash: u64) -> u64 {
    for byte in text.bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// What `near` says of every ship of a crowded world, at room 1 and room 2, for a ship that goes round the planet and for one that goes round nothing, as a line of text each.
fn answers(
    near: impl Fn(&ServerWorld, GridId, &MovingGrid, Option<GridId>, f64) -> Option<GridId>,
    world: &ServerWorld,
    planet: GridId,
    ships: &[GridId],
) -> Vec<(String, Option<GridId>)> {
    let mut answers = Vec::new();
    for &ship in ships {
        let grid = world.data().grids().get(ship).unwrap();
        for goes_round in [Some(planet), None] {
            for factor in [1.0, 2.0] {
                let answer = near(world, ship, grid, goes_round, factor);
                answers.push((
                    format!("{ship:?}|{goes_round:?}|{factor}|{answer:?};"),
                    answer,
                ));
            }
        }
    }
    answers
}

fn digest(answers: &[(String, Option<GridId>)]) -> u64 {
    answers
        .iter()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, (line, _)| {
            hash_of(line, hash)
        })
}

/// The hash of the answers of `near` on `crowded_world(1, 80)`, from the program that looked at every grid for every ship.
const ANSWERS_PIN: u64 = 8097754105232184481;

fn by_looking_at_every_grid(
    world: &ServerWorld,
    ship: GridId,
    grid: &MovingGrid,
    goes_round: Option<GridId>,
    factor: f64,
) -> Option<GridId> {
    world.near_with(None, ship, grid, goes_round, factor)
}

#[test]
fn near_gives_the_answers_it_gave_when_it_looked_at_every_grid() {
    let (world, planet, ships) = crowded_world(1, 80);
    let answers = answers(by_looking_at_every_grid, &world, planet, &ships);
    // A world in which nothing was near, or everything was, would pin nothing.
    let found = answers
        .iter()
        .filter(|(_, answer)| answer.is_some())
        .count();
    let free = answers.len() - found;
    assert!(found >= 40 && free >= 40, "{found} found, {free} free");
    assert_eq!(
        digest(&answers),
        ANSWERS_PIN,
        "the answers changed: {}",
        digest(&answers)
    );
}

#[test]
fn near_with_the_ships_found_once_gives_the_answers_looking_at_every_grid_gives() {
    // Six crowded worlds of 80 to 130 ships: every answer of every ship, at both rooms, going round the planet and round nothing, is the same, and the first of them is the pinned.
    for seed in 1..=6 {
        let (world, planet, ships) = crowded_world(seed, 70 + 10 * seed as usize);
        let known = Neighbours::of(&world);
        let looking = answers(by_looking_at_every_grid, &world, planet, &ships);
        let with_known = answers(
            |world, ship, grid, goes_round, factor| {
                world.near_with(Some(&known), ship, grid, goes_round, factor)
            },
            &world,
            planet,
            &ships,
        );
        assert_eq!(with_known, looking, "seed {seed}");
        if seed == 1 {
            assert_eq!(digest(&with_known), ANSWERS_PIN);
        }
    }
}

#[test]
fn a_ship_in_the_way_that_is_what_the_ship_goes_round_is_not_counted_and_the_next_is() {
    let (mut world, _) = space();
    // Three ships within a few voxels of one another, far from the planet; the lowest-numbered is what a moon goes round, and the moon is what the third goes round, so the
    // lowest is in the chain of the third and is not counted: the second is.
    let lowest = ship_at(&mut world, [50_000.0, 0.0, 0.0], [0.0; 3]);
    let second = ship_at(&mut world, [50_030.0, 0.0, 0.0], [0.0; 3]);
    let third = ship_at(&mut world, [50_015.0, 0.0, 0.0], [0.0; 3]);
    let moon = planet_on(
        &mut world,
        5,
        1.0e19,
        Some(orbit_about(Centre::Grid(lowest), 1_000.0, 100.0)),
    );
    let known = Neighbours::of(&world);
    let grid = world.data().grids().get(third).unwrap();
    for factor in [1.0, 2.0] {
        let looking = world.near_with(None, third, grid, Some(moon), factor);
        let with_known = world.near_with(Some(&known), third, grid, Some(moon), factor);
        assert_eq!(looking, Some(second), "room {factor}");
        assert_eq!(with_known, looking, "room {factor}");
        // Going round nothing, the lowest is in the way.
        assert_eq!(
            world.near_with(Some(&known), third, grid, None, factor),
            Some(lowest)
        );
    }
}

#[test]
fn a_tick_asks_for_the_pairs_that_can_be_near_and_not_for_every_pair() {
    // 400 ships round a planet, a thousand voxels apart along a line, each in the circle it is on (a hundred of them left alone, so on rails, and a hundred pushed): the ships
    // in the way of a ship are asked of a few of its neighbours and not of the other 399. The world says how many pairs it asked.
    let (mut world, _) = space();
    let mut ships = Vec::new();
    for index in 0..400u64 {
        let distance = 10_000.0 + 1_000.0 * index as f64;
        let speed = libm::sqrt(6.674e-11 * 1.0e22 / distance);
        ships.push(ship_at(&mut world, [distance, 0.0, 0.0], [0.0, 0.0, speed]));
    }
    world.data.set_clock(0);
    world.update_coasting(TICKS);
    let asked = world.rail_work().ship_pairs_asked;
    // (The count is the structure's own: nothing asked means the tick did not use it.)
    assert!(asked > 0, "the tick asked nothing of what it found");
    assert!(
        asked < 30 * 400,
        "{asked} pairs asked of the {} there are",
        400 * 399 / 2
    );
}

// What the structure that finds the ships in the way (`ShipsInTheWay`) has to be: the same answers as looking at every ship, with the same arithmetic.

/// The rooms asked about: the two a ship asks for (to stay on a rail, to go on one) and others, which the search is as good for.
const ROOMS: [f64; 5] = [0.5, 1.0, 1.5, 2.0, 3.0];

fn ids(count: usize, random: &mut Random) -> Vec<GridId> {
    let mut numbers: Vec<u64> = (0..count as u64).collect();
    for index in (1..numbers.len()).rev() {
        numbers.swap(index, random.below(index as u64 + 1) as usize);
    }
    numbers
        .into_iter()
        .map(|number| GridId::new(GridId::FIRST_ALLOCATED + number))
        .collect()
}

fn body(level: u32, brick: [i128; 3], offset: [f64; 3], radius: f64, velocity: [f64; 3]) -> Body {
    Body::new(
        LevelIndex::new(level),
        WorldPosition::new(BrickCoordinate::new(brick[0], brick[1], brick[2]), offset),
        radius,
        velocity,
    )
}

/// What looking at every ship says: the lowest-numbered ship of the level of `asking`, not counting `excluded`, that is in its way with this room.
fn by_looking_at_every_ship(
    ships: &[(GridId, Body)],
    asking: &(GridId, Body),
    factor: f64,
    excluded: &[GridId],
) -> Option<GridId> {
    ships
        .iter()
        .filter(|(id, other)| {
            *id != asking.0
                && !excluded.contains(id)
                && other.level() == asking.1.level()
                && is_in_the_way(&asking.1, other, factor)
        })
        .map(|(id, _)| *id)
        .min()
}

/// Asks every ship of `ships` what is in its way, at each of `rooms`, and says what is not what looking at every ship says; and asks again without the one that was found
/// (and without it and the next), which is what a ship that goes round the one in its way is asked.
fn assert_finds_what_looking_at_every_ship_finds(
    ships: &[(GridId, Body)],
    rooms: &[f64],
    what: &str,
) -> ShipsInTheWay {
    let found = ShipsInTheWay::find(ships);
    for asking in ships {
        for &factor in rooms {
            let mut excluded = Vec::new();
            for _ in 0..3 {
                let expected = by_looking_at_every_ship(ships, asking, factor, &excluded);
                assert_eq!(
                    found.lowest(asking.0, factor, &excluded),
                    Some(expected),
                    "{what}: {:?} at room {factor} without {excluded:?}",
                    asking.0
                );
                match expected {
                    Some(lowest) => excluded.push(lowest),
                    None => break,
                }
            }
        }
    }
    found
}

/// How many pairs are asked the exact question when each ship asks what is in its way at the two rooms of a tick (a fresh structure: no other question has been asked).
fn pairs_asked_in_a_tick(ships: &[(GridId, Body)]) -> u64 {
    let found = ShipsInTheWay::find(ships);
    for (id, _) in ships {
        for factor in [1.0, 2.0] {
            found.lowest(*id, factor, &[]).unwrap();
        }
    }
    found.pairs_tested()
}

/// `count` ships with centres in a cube of `half` bricks round `centre`, radii up to `radius` (a third of them none), speeds up to `speed` (a quarter of them none), in `levels`
/// levels.
fn scatter(
    random: &mut Random,
    count: usize,
    centre: [i128; 3],
    half: i128,
    radius: f64,
    speed: f64,
    levels: u32,
) -> Vec<(GridId, Body)> {
    ids(count, random)
        .into_iter()
        .map(|id| {
            let brick = [0, 1, 2]
                .map(|axis| centre[axis] + random.below(2 * half as u64 + 1) as i128 - half);
            let offset = [0, 1, 2].map(|_| 32.0 * random.unit());
            let radius = if random.below(3) == 0 {
                0.0
            } else {
                radius * random.unit()
            };
            let scale = if random.below(4) == 0 {
                0.0
            } else {
                speed * random.unit()
            };
            let velocity = [0, 1, 2].map(|_| scale * random.signed());
            let level = random.below(u64::from(levels)) as u32;
            (id, body(level, brick, offset, radius, velocity))
        })
        .collect()
}

#[test]
fn the_ships_in_the_way_are_those_that_looking_at_every_ship_finds() {
    let far = BRICK_COORDINATE_MAXIMUM / 2;
    for seed in 0..8 {
        let mut random = Random(seed);
        // Dense (most are near), middling, sparse (few are), very far from the origin, in three levels, and with fast ships among slow.
        let scenarios: [(&str, Vec<(GridId, Body)>); 6] = [
            (
                "dense",
                scatter(&mut random, 100, [0; 3], 6, 40.0, 300.0, 1),
            ),
            (
                "middling",
                scatter(&mut random, 150, [0; 3], 200, 40.0, 3_000.0, 1),
            ),
            (
                "sparse",
                scatter(&mut random, 150, [0; 3], 100_000, 40.0, 9_000.0, 1),
            ),
            (
                "far out",
                scatter(&mut random, 100, [far, -far, far / 3], 50, 40.0, 500.0, 1),
            ),
            (
                "levels",
                scatter(&mut random, 120, [0; 3], 30, 40.0, 500.0, 3),
            ),
            (
                "fast",
                scatter(&mut random, 120, [0; 3], 100, 40.0, 1.0e7, 1),
            ),
        ];
        for (what, ships) in &scenarios {
            assert_finds_what_looking_at_every_ship_finds(
                ships,
                &ROOMS,
                &format!("{what}, seed {seed}"),
            );
        }
    }
}

#[test]
fn a_pair_at_the_edge_of_the_room_is_found_exactly_as_looking_at_it_finds_it() {
    // Two ships that close head on at 123.456 and 78.9 voxels a second, with radii 37.3 and 11.1, the second set at the distance where the room (the margin and the way they
    // go in a second at the speed they close at, once or twice) just ends, to within 40 steps of the last bit either way, along an axis and slantwise.
    let (first_radius, second_radius, first_speed, second_speed) = (37.3, 11.1, 123.456, 78.9);
    let closing = first_speed + second_speed;
    let from = [3.0 * 32.0 + 1.0, -7.0 * 32.0 + 2.0, 11.0 * 32.0 + 3.0];
    for factor in [1.0, 2.0] {
        let edge: f64 = first_radius + second_radius + factor * (64.0 + closing);
        for direction in [[1.0, 0.0, 0.0], [0.6, 0.0, 0.8], [0.0, -1.0, 0.0]] {
            for step in -40i64..=40 {
                let apart = f64::from_bits((edge.to_bits() as i64 + step) as u64);
                let first = body(
                    0,
                    [3, -7, 11],
                    [1.0, 2.0, 3.0],
                    first_radius,
                    direction.map(|c| c * first_speed),
                );
                let at = [0, 1, 2].map(|axis| from[axis] + direction[axis] * apart);
                let second = Body::new(
                    LevelIndex::new(0),
                    WorldPosition::from_voxels(at),
                    second_radius,
                    direction.map(|c| -c * second_speed),
                );
                let ships = vec![
                    (GridId::new(GridId::FIRST_ALLOCATED + 1), first),
                    (GridId::new(GridId::FIRST_ALLOCATED), second),
                ];
                assert_finds_what_looking_at_every_ship_finds(
                    &ships,
                    &[1.0, 2.0],
                    &format!("room {factor}, direction {direction:?}, {step} steps from the edge"),
                );
            }
        }
    }
}

#[test]
fn ships_just_inside_the_room_three_quadrillion_voxels_from_the_first_ship_are_not_dropped_with_their_node()
 {
    // The places of a tree are from its first ship, in `f64`: out at 3.2e15 voxels a place is only to half a voxel, and the room of a ship that goes nowhere is 128 voxels
    // (room 2). A ship asks about nine that are between a tenth and a hundredth of a voxel inside the edge of it, in a node of their own (the tree is the first ship at the
    // origin, eight ships round the asking one, and these nine), and what a node drops is dropped with the rounding to spare, so all nine are found, whichever way the
    // places round.
    let brick = 100_000_000_000_000i128;
    let id = |number: u64| GridId::new(GridId::FIRST_ALLOCATED + number);
    for shift in 0..24 {
        let at = 1.0 + 1.3 * f64::from(shift);
        let mut ships = vec![(id(0), body(0, [0, 0, 0], [0.0; 3], 0.0, [0.0; 3]))];
        let round_the_asking_one: Vec<GridId> = (1..=8).map(id).collect();
        for number in 1..=8 {
            let offset = at + 0.03 * number as f64;
            ships.push((
                id(number),
                body(0, [brick, 3, 5], [offset, 2.0, 3.0], 0.0, [0.0; 3]),
            ));
        }
        for number in 0..9 {
            let apart = 128.0 - 0.01 * (number + 1) as f64;
            let along = at + 0.03 + apart;
            let whole = (along / 32.0).floor();
            let offset = along - 32.0 * whole;
            ships.push((
                id(20 + number),
                body(
                    0,
                    [brick + whole as i128, 3, 5],
                    [offset, 2.0, 3.0],
                    0.0,
                    [0.0; 3],
                ),
            ));
        }
        let found = ShipsInTheWay::find(&ships);
        let asking = &ships[1];
        let expected = by_looking_at_every_ship(&ships, asking, 2.0, &round_the_asking_one);
        assert_eq!(
            expected,
            Some(id(20)),
            "the nine are in the way of the asking one"
        );
        assert_eq!(
            found.lowest(asking.0, 2.0, &round_the_asking_one),
            Some(expected),
            "shift {shift}"
        );
    }
}

#[test]
fn a_ship_whose_speed_is_not_a_number_or_is_endless_is_where_looking_at_every_ship_puts_it() {
    let mut random = Random(99);
    let mut ships = scatter(&mut random, 60, [0; 3], 10, 20.0, 200.0, 1);
    // An endless speed makes a ship in the way of everything in its level (the room is endless); a speed that is not a number makes it in the way of nothing.
    let endless = body(0, [1, 1, 1], [5.0; 3], 3.0, [f64::INFINITY, 0.0, 0.0]);
    let not_a_number = body(0, [-2, 0, 4], [5.0; 3], 3.0, [f64::NAN, 1.0, 2.0]);
    ships.push((GridId::new(GridId::FIRST_ALLOCATED + 1000), endless));
    ships.push((GridId::new(GridId::FIRST_ALLOCATED + 1001), not_a_number));
    assert_finds_what_looking_at_every_ship_finds(&ships, &ROOMS, "odd speeds");
}

#[test]
fn nothing_is_in_the_way_of_one_ship_or_of_none_and_a_ship_unknown_is_not_answered_for() {
    let none = ShipsInTheWay::find(&[]);
    assert_eq!(
        none.lowest(GridId::new(GridId::FIRST_ALLOCATED), 2.0, &[]),
        None
    );
    let only = GridId::new(GridId::FIRST_ALLOCATED + 5);
    let one = ShipsInTheWay::find(&[(only, body(0, [0; 3], [1.0; 3], 5.0, [1.0; 3]))]);
    assert_eq!(one.lowest(only, 1.0, &[]), Some(None));
    assert_eq!(one.lowest(only, 2.0, &[]), Some(None));
    assert_eq!(
        one.lowest(GridId::new(GridId::FIRST_ALLOCATED), 2.0, &[]),
        None
    );
}

#[test]
fn a_spread_out_crowd_costs_what_it_has_near_it_and_not_what_it_has_in_all() {
    // 2,000 ships spread over a cube 200 km across move slowly: each has a few others within reach, not two thousand.
    let mut random = Random(7);
    let spread = scatter(&mut random, 2_000, [0; 3], 3_125, 20.0, 30.0, 1);
    let asked = pairs_asked_in_a_tick(&spread);
    assert!(
        asked < 20 * 2_000,
        "{asked} pairs asked of the {} there are",
        2_000 * 1_999 / 2
    );
    assert_finds_what_looking_at_every_ship_finds(&spread[..300], &[1.0, 2.0], "spread");
}

#[test]
fn a_convoy_that_goes_the_same_way_costs_what_it_has_near_it_though_it_goes_at_8_km_a_second() {
    // 1,500 ships in a line, two hundred voxels apart (a gap of 180 voxels, over the room of 128 at most), all going at 8,170 voxels a second the same way: the room is the way
    // two go in a second at the speed they CLOSE at, which is none, and not at the speed they go. Not one is in the way of another.
    let ships: Vec<(GridId, Body)> = (0..1_500u64)
        .map(|index| {
            (
                GridId::new(GridId::FIRST_ALLOCATED + index),
                Body::new(
                    LevelIndex::MAIN,
                    WorldPosition::from_voxels([200.0 * index as f64, 0.0, 0.0]),
                    10.0,
                    [0.0, 0.0, 8_170.0],
                ),
            )
        })
        .collect();
    let asked = pairs_asked_in_a_tick(&ships);
    assert!(
        asked < 40 * 1_500,
        "{asked} pairs asked of the {} there are",
        1_500 * 1_499 / 2
    );
    let found = assert_finds_what_looking_at_every_ship_finds(&ships[..200], &[1.0, 2.0], "convoy");
    assert_eq!(found.lowest(ships[10].0, 2.0, &[]), Some(None));
    // Closed up to forty voxels apart, each is in the way of its neighbours up to three along either way, and the lowest number among them is the third before it.
    let closed_up: Vec<(GridId, Body)> = (0..1_500u64)
        .map(|index| {
            (
                GridId::new(GridId::FIRST_ALLOCATED + index),
                Body::new(
                    LevelIndex::MAIN,
                    WorldPosition::from_voxels([40.0 * index as f64, 0.0, 0.0]),
                    10.0,
                    [0.0, 0.0, 8_170.0],
                ),
            )
        })
        .collect();
    let found = ShipsInTheWay::find(&closed_up);
    assert_eq!(
        found.lowest(closed_up[700].0, 2.0, &[]),
        Some(Some(closed_up[697].0)),
        "a gap of 40 - 20 = 20 voxels; room 2 x 64 = 128 voxels: three ships back is 3 x 40 - 20 = 100 away, in the way, and the fourth 140, not"
    );
    assert!(found.pairs_tested() < 100, "{}", found.pairs_tested());
}

#[test]
fn a_crowd_that_is_all_in_the_way_of_each_other_costs_one_leaf_a_ship() {
    // A ring of 1,500 ships 20 km across, each going round it at 8 km/s: two on opposite sides close at 16 km/s, so by the rule every one is in the way of every other, and the
    // answer for each is the lowest number but its own. Looking at every ship stops at the first; so does this.
    let mut random = Random(5);
    let mut ring: Vec<(GridId, Body)> = Vec::new();
    for (index, id) in ids(1_500, &mut random).into_iter().enumerate() {
        let angle = index as f64 * 0.004_189;
        let (sine, cosine) = (libm::sin(angle), libm::cos(angle));
        ring.push((
            id,
            Body::new(
                LevelIndex::MAIN,
                WorldPosition::from_voxels([10_000.0 * cosine, 10_000.0 * sine, 0.0]),
                10.0,
                [-8_170.0 * sine, 8_170.0 * cosine, 0.0],
            ),
        ));
    }
    let asked = pairs_asked_in_a_tick(&ring);
    assert!(
        asked < 40 * 1_500,
        "{asked} pairs asked of the {} there are",
        1_500 * 1_499 / 2
    );
    assert_finds_what_looking_at_every_ship_finds(&ring[..150], &[1.0, 2.0], "ring");
}

/// How many pairs a tick asks for each of `count` slow ships spread over a cube that grows with them (the same density), and one that goes at 4,000 km a second, which has
/// everything in its way.
fn pairs_for_each_ship_with_a_fast_one(count: usize) -> f64 {
    let mut random = Random(11);
    let half = (3_125.0 * libm::cbrt(count as f64 / 1_000.0)) as i128;
    let mut ships = scatter(&mut random, count, [0; 3], half, 20.0, 30.0, 1);
    ships.push((
        GridId::new(GridId::FIRST_ALLOCATED + 100_000),
        body(0, [0; 3], [1.0; 3], 5.0, [4.0e6, 0.0, 0.0]),
    ));
    pairs_asked_in_a_tick(&ships) as f64 / ships.len() as f64
}

#[test]
fn a_fast_ship_among_slow_ones_costs_what_it_is_near_and_a_ship_more_costs_no_more_than_the_ships_before()
 {
    // Looking at every pair, eight times the ships cost sixty-four times as much, and each ship eight times as much. Here a ship costs about what it did.
    let (few, many) = (
        pairs_for_each_ship_with_a_fast_one(500),
        pairs_for_each_ship_with_a_fast_one(4_000),
    );
    assert!(
        many < 2.5 * few,
        "{few} pairs a ship for 500 ships, {many} for 4,000"
    );
    // And it is right (everything is in the way of the fast one, which has the highest number, so it is the answer for a ship only if nothing else is).
    let mut random = Random(11);
    let mut ships = scatter(&mut random, 1_000, [0; 3], 3_125, 20.0, 30.0, 1);
    ships.push((
        GridId::new(GridId::FIRST_ALLOCATED + 100_000),
        body(0, [0; 3], [1.0; 3], 5.0, [4.0e6, 0.0, 0.0]),
    ));
    assert_finds_what_looking_at_every_ship_finds(&ships[..150], &[1.0, 2.0], "one fast ship");
    let found = ShipsInTheWay::find(&ships);
    for slow in &ships[150..170] {
        assert_eq!(
            found.lowest(slow.0, 2.0, &[]),
            Some(by_looking_at_every_ship(&ships, slow, 2.0, &[]))
        );
    }
}
