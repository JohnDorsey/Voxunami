//! Walkers near a planet that goes round at 125 metres a second (found by reading the code, 2026-11-05; measured 2026-11-07): what a walker does at a ship near it must be what
//! they do at a ship near a planet that stands still.
//!
//! Inside the sphere of a planet on a rail the world carries what is there by the planet's displacement every tick (`ServerWorld::apply_rails`), so the speed of a walker's body
//! is *against the planet*; the speed of a ship, though, is the world's (`engine/grid_bodies`: ships are simulated in a frame that goes with the planet, and written with the
//! planet's speed added). A walker who boarded a ship, stepped off one, or was stepped against one (`speed_of_what_they_left`) took the ship's speed as theirs without the
//! planet's taken off, and had the planet's speed twice: once from the carry and once from the body. Every test runs in two worlds, with the planet going round and with it
//! on a rail so slow that it stands still (the same planet, sphere and gravity: the speed alone differs), and asks the same of both.

mod common;

use common::moving_planet::{WANDERER, a_barely_moving_scene, a_scene};
use common::{
    Player, PlayerExt, Scene, circular_speed, eyes_in_the_world, eyes_in_their_frame,
    greatest_kink, length, minus, player_pose, say, ship_id, ship_over, ship_voxels,
    tick_and_follow,
};
use game_shell::KeyCode;
use player_presence::player_kind;
use voxel_space::{EntityPose, Orientation, WorldPosition};
use voxunami_cave::aboard::in_voxels;

/// How far the middle of a deck is from the middle of the wanderer: past the reach of its ground (340 bricks, 10,880 voxels) and inside what it carries (16,320).
const DISTANCE: f64 = 12_000.0;

/// The same ask in two worlds: the wanderer on a rail so slow that it stands still where it starts, which the walkers have always been right near (a test that fails here is
/// the test's fault; it is asked first), and going round the home planet at 125.7 metres a second.
fn the_two_worlds() -> [(&'static str, Scene); 2] {
    [
        ("barely moving", a_barely_moving_scene()),
        ("going round", a_scene(None)),
    ]
}

/// Where the walker's eyes are from the middle of the wanderer, in the world's axes (it does not turn in these worlds).
fn eyes_from_the_wanderer(scene: &Scene) -> [f64; 3] {
    minus(eyes_in_the_world(scene, 1), ship_voxels(scene, WANDERER))
}

/// Where the middle of a ship is, in voxels.
fn ship_middle(scene: &Scene, number: u64) -> [f64; 3] {
    let ship = scene
        .rig
        .server
        .world()
        .grids()
        .get(ship_id(number))
        .expect("the ship is there");
    let origin = in_voxels(ship.pose.position);
    [0, 1, 2].map(|axis| origin[axis] + ship.centre_of_mass[axis])
}

/// How far a place is from another across the way up, which is +y over the pole of the wanderer where these tests are (the way the planet pulls is straight down).
fn level_distance(first: [f64; 3], second: [f64; 3]) -> f64 {
    let apart = minus(first, second);
    (apart[0] * apart[0] + apart[2] * apart[2]).sqrt()
}

/// A walker who joins over the pole of the wanderer, `height` from its middle, and has come into the world (a tick or more after they have joined; the planet has gone on
/// while they did, and they with it, once they are in its sphere).
fn a_walker_over_the_wanderer(scene: &mut Scene, height: f64) -> Player {
    let planet = ship_voxels(scene, WANDERER);
    let mut player = scene.rig.join_walking(
        [planet[0] + 0.5, planet[1] + height, planet[2] + 0.5],
        0.0,
        0.0,
    );
    for _ in 0..200 {
        if scene.rig.server.world().entities().iter().next().is_some() {
            break;
        }
        tick_and_follow(scene, &mut player, 1);
    }
    player
}

/// Puts the walker aboard a ship, at rest in it, with their eyes at this place in its coordinates.
fn put_aboard(scene: &mut Scene, ship: u64, eyes_in_the_ship: [f64; 3]) {
    scene.run(move |world| {
        let entity = world
            .entities()
            .iter()
            .find(|entity| entity.kind == player_kind())
            .expect("the player has an entity")
            .id;
        world
            .set_entity_pose(
                entity,
                EntityPose::new(
                    ship_id(ship),
                    WorldPosition::from_voxels(eyes_in_the_ship),
                    Orientation::IDENTITY,
                ),
            )
            .unwrap();
    });
}

/// A deck of 6 by 1 by 6 held over the pole of the wanderer, and a walker standing on it (aboard it, two seconds after they were put there). The walker and the deck's number.
fn a_walker_on_a_held_deck(scene: &mut Scene) -> (Player, u64) {
    scene.ticks(1);
    let mut player = a_walker_over_the_wanderer(scene, DISTANCE + 40.0);
    let deck = ship_over(
        scene,
        WANDERER,
        [-3.0, DISTANCE, -3.0],
        true,
        [0.0; 3],
        [6, 1, 6],
    );
    put_aboard(scene, deck, [3.0, 3.5, 3.0]);
    tick_and_follow(scene, &mut player, 40);
    assert_eq!(player_pose(scene, 1).grid, ship_id(deck), "aboard the deck");
    (player, deck)
}

/// Walks the walker toward -z, off the side of a deck they stand in the middle of, until they are in the level, and lets go of the keys. Returns the ticks it took.
fn walk_off(scene: &mut Scene, player: &mut Player) -> usize {
    player.hold(&[KeyCode::KeyW]);
    for tick in 1..=200 {
        tick_and_follow(scene, player, 1);
        if player_pose(scene, 1).grid.root_level().is_some() {
            player.hold(&[]);
            return tick;
        }
    }
    panic!("they did not walk off");
}

#[test]
fn a_walker_who_falls_onto_a_deck_near_a_planet_that_goes_round_stands_on_it_as_near_one_that_stands_still()
 {
    for (name, mut scene) in the_two_worlds() {
        scene.ticks(1);
        let mut player = a_walker_over_the_wanderer(&mut scene, DISTANCE + 40.0);
        // A deck 15 voxels under them, held (it goes with the planet), under the middle of the fall wherever the planet has gone.
        let over = eyes_from_the_wanderer(&scene);
        let deck = ship_over(
            &mut scene,
            WANDERER,
            [over[0] - 3.0, over[1] - 15.0, over[2] - 3.0],
            true,
            [0.0; 3],
            [6, 1, 6],
        );
        let mut landed = false;
        for _ in 0..150 {
            tick_and_follow(&mut scene, &mut player, 1);
            if player_pose(&scene, 1).grid == ship_id(deck) {
                landed = true;
                break;
            }
        }
        assert!(landed, "{name}: they fell onto the deck");
        // They stand on it: two seconds later they are aboard still, where they were on it (to a third of a voxel).
        let at_landing = eyes_in_their_frame(&scene, 1);
        tick_and_follow(&mut scene, &mut player, 40);
        assert_eq!(
            player_pose(&scene, 1).grid,
            ship_id(deck),
            "{name}: still aboard"
        );
        let later = eyes_in_their_frame(&scene, 1);
        assert!(
            level_distance(at_landing, later) < 0.34,
            "{name}: they were at {at_landing:?} on landing and at {later:?} two seconds later"
        );
    }
}

#[test]
fn a_walker_who_walks_off_a_deck_near_a_planet_that_goes_round_goes_on_beside_it_as_near_one_that_stands_still()
 {
    for (name, mut scene) in the_two_worlds() {
        let (mut player, deck) = a_walker_on_a_held_deck(&mut scene);
        walk_off(&mut scene, &mut player);
        // Three seconds on, with the keys let go, they have fallen toward the planet (the deck is held), and across the way down they are where a walk of a few voxels and the
        // air's braking put them: a few voxels from the deck, and not 6.3 a tick more with it, the planet's own speed (which is what a walker had that was thrown off the
        // deck in the tick they were put in the level).
        tick_and_follow(&mut scene, &mut player, 60);
        let apart = level_distance(eyes_in_the_world(&scene, 1), ship_middle(&scene, deck));
        assert!(
            apart < 15.0,
            "{name}: {apart} voxels from the deck across the way down, three seconds after they walked off it"
        );
    }
}

/// A deck of 6 by 1 by 6 in a circle round the wanderer, `DISTANCE` from its middle, going at the speed of the circle against it (and the planet's own), with a walker
/// aboard. It is a body of the solver's in a frame that goes with the planet: a ship does not go on a rail where a planet carries what is near it
/// (`ServerWorld::least_to_go_round`), however slowly the planet goes. The walker and the deck's number.
fn a_walker_on_a_deck_in_orbit(scene: &mut Scene) -> (Player, u64) {
    let (mut player, deck) = a_walker_on_a_held_deck(scene);
    let speed = circular_speed(scene, WANDERER, DISTANCE);
    scene.run(move |world| {
        let planet = world
            .data()
            .grids()
            .get(ship_id(WANDERER))
            .expect("the wanderer is there")
            .linear_velocity;
        world.set_grid_held(ship_id(deck), false).unwrap();
        world
            .set_grid_velocity(
                ship_id(deck),
                [planet[0] + speed, planet[1], planet[2]],
                [0.0; 3],
            )
            .unwrap();
    });
    // Six seconds: more than the second after which a ship is put on a rail where one may be.
    tick_and_follow(scene, &mut player, 120);
    assert_eq!(player_pose(scene, 1).grid, ship_id(deck), "still aboard");
    (player, deck)
}

/// The test of `leaving_a_ship.rs` for a walker on a deck in orbit, in a world where the planet goes round: they walk off the side, and stay beside the deck as they do near a
/// planet that stands still.
#[test]
fn a_walker_who_steps_off_a_ship_in_orbit_round_a_planet_that_goes_round_stays_beside_it_as_near_one_that_stands_still()
 {
    for (name, mut scene) in the_two_worlds() {
        let (mut player, deck) = a_walker_on_a_deck_in_orbit(&mut scene);
        walk_off(&mut scene, &mut player);
        // Twelve seconds on, with the keys let go.
        tick_and_follow(&mut scene, &mut player, 240);
        let eyes = eyes_in_the_world(&scene, 1);
        let apart = minus(eyes, ship_middle(&scene, deck));
        let distance = common::length(apart);
        assert!(
            distance < 40.0,
            "{name}: {distance} voxels ({apart:?}) from the deck, twelve seconds after they walked off it"
        );
        // And they do not drift off: a second later they are as far (to a few voxels).
        tick_and_follow(&mut scene, &mut player, 20);
        let later = common::length(minus(
            eyes_in_the_world(&scene, 1),
            ship_middle(&scene, deck),
        ));
        assert!(
            (later - distance).abs() < 6.0,
            "{name}: {distance} then {later}"
        );
    }
}

/// What falls with the planet falls as the planet does: a walker beside a deck in orbit round it is pulled by the planet and not by what the planet goes round, as the deck is
/// (`ServerWorld::gravity_in_the_free_fall_of`). Asked for the pull of every body the walker was pulled by the home planet as well, 0.127 metres a second a second at 400,000
/// voxels, and the deck, which goes round the wanderer as the wanderer goes round the home planet, was not: measured 2026-11-07, they were 6 voxels apart after twelve seconds and
/// 79 after thirty-six, in both worlds. Beside each other, with a walk of 4 voxels, they are 5 apart after thirty-six.
#[test]
fn a_walker_beside_a_deck_in_orbit_round_a_planet_that_goes_round_is_pulled_as_the_deck_is_for_half_a_minute()
 {
    for (name, mut scene) in the_two_worlds() {
        let (mut player, deck) = a_walker_on_a_deck_in_orbit(&mut scene);
        walk_off(&mut scene, &mut player);
        tick_and_follow(&mut scene, &mut player, 720);
        let apart = minus(eyes_in_the_world(&scene, 1), ship_middle(&scene, deck));
        let distance = common::length(apart);
        assert!(
            distance < 10.0,
            "{name}: {distance} voxels ({apart:?}) from the deck, thirty-six seconds after they walked off it"
        );
    }
}

/// How far from the middle of the wanderer a planet that goes round lets go of what it carries: a quarter farther than its sphere (16,320), which it takes things into.
const SPHERE: f64 = 16_320.0;
const SPHERE_KEPT: f64 = 20_400.0;

/// A walker aboard a raft that is thrown `against` the planet from `from` over its pole, who walks off it in the air and goes on for `ticks` ticks: where their eyes are in the
/// world at every tick after they are in the level, and how far each is from the middle of the wanderer.
fn fly_off_a_thrown_raft(
    scene: &mut Scene,
    from: f64,
    against: [f64; 3],
    ticks: usize,
) -> (Vec<[f64; 3]>, Vec<f64>) {
    scene.ticks(1);
    let mut player = a_walker_over_the_wanderer(scene, from + 40.0);
    let raft = ship_over(
        scene,
        WANDERER,
        [-3.0, from, -3.0],
        false,
        against,
        [6, 1, 6],
    );
    put_aboard(scene, raft, [3.0, 3.5, 3.0]);
    walk_off(scene, &mut player);
    let (mut path, mut distances) = (Vec::new(), Vec::new());
    for _ in 0..ticks {
        tick_and_follow(scene, &mut player, 1);
        path.push(eyes_in_the_world(scene, 1));
        distances.push(length(eyes_from_the_wanderer(scene)));
    }
    assert!(
        player_pose(scene, 1).grid.root_level().is_some(),
        "they are in the level still"
    );
    (path, distances)
}

/// A walker in the air goes on at the speed they have in the world when the planet that carried them lets go: the path of their eyes has no kink where they go out of the
/// sphere (measured 2026-11-07, before this was done: one of 6.26 voxels, the planet's own step, at 20,410 from its middle), as a ship's has none (`frames_that_follow.rs`).
#[test]
fn a_walker_who_flies_out_of_the_sphere_of_a_planet_that_goes_round_goes_on_at_the_speed_they_had_in_the_world()
 {
    for (name, mut scene) in the_two_worlds() {
        let (path, distances) = fly_off_a_thrown_raft(&mut scene, DISTANCE, [0.0, 400.0, 0.0], 700);
        assert!(
            distances[0] < SPHERE && distances[699] > SPHERE_KEPT + 1_000.0,
            "{name}: they start in the sphere and end well out of it: {} then {}",
            distances[0],
            distances[699]
        );
        let kink = greatest_kink(&path);
        assert!(
            kink < 0.5,
            "{name}: the path of the walker has a kink of {kink} voxels"
        );
    }
}

/// And as they go in: from outside the sphere, where nothing carries them, to the planet.
#[test]
fn a_walker_who_flies_into_the_sphere_of_a_planet_that_goes_round_is_taken_in_without_a_jump() {
    for (name, mut scene) in the_two_worlds() {
        let (path, distances) =
            fly_off_a_thrown_raft(&mut scene, 20_000.0, [0.0, -400.0, 0.0], 300);
        assert!(
            distances[0] > SPHERE_KEPT - 1_000.0 && distances[299] < SPHERE,
            "{name}: they start outside the sphere and end in it: {} then {}",
            distances[0],
            distances[299]
        );
        let kink = greatest_kink(&path);
        assert!(
            kink < 0.5,
            "{name}: the path of the walker has a kink of {kink} voxels"
        );
    }
}

/// A walker who is put somewhere (sent with `/tp`, or left by a ship that is taken away) is at rest against whatever carries the place: standing in the world where nothing
/// does, and going with the planet where one does. What the world said carried them was for where they were before, and is not a change of carrier that they keep their speed
/// across (that would give them the planet's speed, or take it from them, in the tick they are put).
#[test]
fn a_walker_who_is_sent_out_of_the_sphere_of_a_planet_that_goes_round_stands_in_the_world_and_one_sent_into_it_goes_with_the_planet()
 {
    for (name, mut scene) in the_two_worlds() {
        scene.ticks(1);
        let mut player = a_walker_over_the_wanderer(&mut scene, DISTANCE + 40.0);
        tick_and_follow(&mut scene, &mut player, 20);
        // Out of the sphere, far from every planet: at rest in the world, so two seconds later they have fallen a voxel or two to the home planet's pull and not gone with the
        // wanderer (250 voxels).
        say(&mut scene, &mut player, "/tp 200000 200000 0");
        let sent = eyes_in_the_world(&scene, 1);
        assert!(
            length(minus(sent, [200_000.0, 200_000.0, 0.0])) < 20.0,
            "{name}: sent to {sent:?}"
        );
        tick_and_follow(&mut scene, &mut player, 40);
        let after = length(minus(eyes_in_the_world(&scene, 1), sent));
        assert!(
            after < 5.0,
            "{name}: {after} voxels from where they were sent in two seconds"
        );
        // Into the sphere, 12,000 over the pole of the planet: at rest against it, so that two seconds later they are over the same place of it (they have fallen, and the planet
        // has gone 250 voxels along its orbit).
        let planet = ship_voxels(&scene, WANDERER);
        say(
            &mut scene,
            &mut player,
            &format!("/tp {} {} {}", planet[0], planet[1] + DISTANCE, planet[2]),
        );
        let sent = eyes_from_the_wanderer(&scene);
        tick_and_follow(&mut scene, &mut player, 40);
        let after = level_distance(eyes_from_the_wanderer(&scene), sent);
        assert!(
            after < 5.0,
            "{name}: {after} voxels along from the place of the planet they were sent to, two seconds later"
        );
    }
}

#[test]
fn a_walker_whose_ship_is_taken_away_near_a_planet_that_goes_round_is_at_rest_against_the_planet() {
    for (name, mut scene) in the_two_worlds() {
        let (mut player, deck) = a_walker_on_a_held_deck(&mut scene);
        let aboard = eyes_from_the_wanderer(&scene);
        scene.run(move |world| {
            world.remove_grid(ship_id(deck)).unwrap();
        });
        tick_and_follow(&mut scene, &mut player, 40);
        assert!(
            player_pose(&scene, 1).grid.root_level().is_some(),
            "{name}: they are in the level"
        );
        let after = level_distance(eyes_from_the_wanderer(&scene), aboard);
        assert!(
            after < 5.0,
            "{name}: {after} voxels along from where the ship was, two seconds after it was taken away"
        );
    }
}
