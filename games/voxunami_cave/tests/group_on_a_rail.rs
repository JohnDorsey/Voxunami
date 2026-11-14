//! A group of ships coupled by their joint ends rides a rail (step 4c-3 of `docs/decisions/joints-and-cars.md`).
//!
//! The rail is on the ship of the group with the lowest number and holds the centre of mass of the ships, weighed by their masses; the world puts the group where the rail says
//! (all the ships by the same, so that they stand against each other as the bodies left them), and the bodies hold the ships in **one frame that goes with the rail's point**:
//! free bodies with their joints, nothing pulling on them, nothing else in the frame, the centre of mass of the group held at the point. What the joints do is the bodies' (promise
//! (a) of the joint plan: a mechanism does not change because its group is on a rail), and the path of the group is the rail's (promise (b): nothing the bodies do moves it).
//!
//! The rails here are made by hand, from the centre of mass and the speed of the group, in the tick that sets the ships going (`put_on_its_rail`): that a group is put on one by
//! itself is a matter of the rules of `coasting_rail`, and has its own tests (`group_goes_on_a_rail.rs`).

mod common;

use common::chains::{
    Setting, TICKS_PER_SECOND, Vec3, a_chain, a_chain_in_orbit, a_swing, alike_in_turn, centre_of,
    close, has_swung, how_the_chain_turns, mass_of, minus, most_apart, most_apart_up_to_the_mirror,
    orientation_of, origins_of, pivot_gap, put_on_its_rail, rail_point, rail_point_in, spin_of,
};
use common::docking::stone;
use common::{Player, Scene, circular_speed, length, ship_id, tick_and_follow};
use server::{GridView, ShipSettings};
use voxel_space::VoxelCoordinate;
use voxunami_cave::content::CaveContent;
use world_storage::rail::RailTime;

#[test]
fn a_chain_on_a_rail_has_its_centre_of_mass_on_the_rail_while_its_joints_swing() {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let distance = 70_016.0;
    let speed = circular_speed(&scene, 0, distance);
    let cars = a_chain();
    let lengths: Vec<f64> = cars.iter().map(|(_, length)| *length).collect();
    let ships = a_chain_in_orbit(&mut scene, cars, distance, speed);
    assert_eq!(report().couplings, 2, "the bodies coupled the chain");
    let rail = put_on_its_rail(&mut scene, &ships, speed, a_swing());
    let mut widest: f64 = 1.0;
    let mut worst_gap: f64 = 0.0;
    for tick in 1..=400 {
        scene.ticks(1);
        let (place, going) = centre_of(&scene, &ships);
        let (on_the_rail, rail_speed) = rail_point(&scene, &rail);
        close(
            place,
            on_the_rail,
            1e-6,
            &format!("tick {tick}: the centre of mass is the rail's"),
        );
        close(
            going,
            rail_speed,
            1e-6,
            &format!("tick {tick}: and goes as the rail does"),
        );
        worst_gap = worst_gap
            .max(pivot_gap(&scene, (ships[0], lengths[0]), ships[1]))
            .max(pivot_gap(&scene, (ships[1], lengths[1]), ships[2]));
        widest = widest.min(alike_in_turn(&scene, ships[0], ships[2]));
    }
    assert!(worst_gap < 0.05, "the joints hold: {worst_gap} m");
    assert!(
        widest < 0.999,
        "and the chain swung: the ends were alike to {widest}"
    );
    assert_eq!(report().couplings, 2, "and it is coupled still");
    assert_eq!(
        scene.rig.server.world().grids().get(ships[0]).unwrap().rail,
        Some(rail),
        "the rail is what it was"
    );
}

#[test]
fn what_a_mechanism_does_does_not_depend_on_where_the_group_is_or_how_fast_it_goes() {
    // The same chain, swung the same, at 70 and at 140 kilometres (distances that are a whole number of bricks, so that nothing differs in the rounding of a place): on a rail
    // the facing and the turn of each car are those of bodies in a frame that has nothing but them in it. The two differ by a few parts in 10^9, which is the ticks before the
    // rail in which the ships were free (the pull of the planet on them differs from one distance to the other: measured, 1.8e-9 after one such tick, 3.7e-9 after two and
    // 9.8e-9 after four) and not the rail; **in the frame everything is in, with a stranger near, they differ by millions of times as much** (the pull, the number of steps a
    // tick is cut into by the speed), and the control of this test is that they do. (Before step 4c-4 the control was the chain free and alone; now that has a frame of its own too:
    // `group_off_a_rail.rs`.)
    // (Both up to the mirror image in the plane of the swing: see `most_apart_up_to_the_mirror`.)
    let on_a_rail = most_apart_up_to_the_mirror(
        &how_the_chain_turns(70_016.0, Setting::OnARail),
        &how_the_chain_turns(140_000.0, Setting::OnARail),
    );
    let with_a_stranger = most_apart_up_to_the_mirror(
        &how_the_chain_turns(70_016.0, Setting::WithAStranger),
        &how_the_chain_turns(140_000.0, Setting::WithAStranger),
    );
    assert!(on_a_rail < 1e-7, "on a rail the two differ by {on_a_rail}");
    assert!(
        with_a_stranger > 1e-4,
        "with a stranger near they differ by {with_a_stranger}"
    );
    // (They did swing: the chain has turned.)
    assert!(has_swung(&how_the_chain_turns(70_016.0, Setting::OnARail)));
}

#[test]
fn a_push_on_a_car_takes_the_group_off_its_rail_and_it_goes_on_from_where_it_was_with_the_push() {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let distance = 70_016.0;
    let speed = circular_speed(&scene, 0, distance);
    let ships = a_chain_in_orbit(&mut scene, a_chain(), distance, speed);
    let rail = put_on_its_rail(&mut scene, &ships, speed, a_swing());
    scene.ticks(60);
    assert_eq!(
        scene.rig.server.world().grids().get(ships[0]).unwrap().rail,
        Some(rail.clone())
    );
    // A force on the middle car for a tick: 60,000 kilogram voxels a second a second on a chain of about 16,000 kilograms is 0.19 metres a second more, in a tick, and the
    // rail comes off the anchor in the tick it is asked.
    let force = [0.0, 60_000.0, 0.0];
    let mass = mass_of(&scene, &ships);
    let middle = ships[1];
    scene.run(move |world| world.apply_grid_force(middle, force, None).unwrap());
    assert_eq!(
        scene.rig.server.world().grids().get(ships[0]).unwrap().rail,
        None
    );
    let (_, going) = centre_of(&scene, &ships);
    // (A ship that comes off its rail in a tick has been put where the rail says at the end of it, as every ship on a rail is at the beginning of a tick, and is moved by the
    // bodies for the tick: it is a tick ahead of the rail, as a single ship that is pushed off its rail is. So it is compared with the rail a tick on.)
    let (_, on_the_rail) = rail_point_in(&scene, &rail, 1);
    let pushed = minus(going, on_the_rail);
    close(
        pushed,
        [0.0, force[1] / f64::from(TICKS_PER_SECOND) / mass, 0.0],
        5e-3,
        "the group goes as the rail does, and a tick of the push faster",
    );
    // It goes on as a chain: coupled still, its joints holding.
    let mut worst_gap: f64 = 0.0;
    for _ in 0..40 {
        scene.ticks(1);
        worst_gap = worst_gap
            .max(pivot_gap(&scene, (ships[0], 7.0), ships[1]))
            .max(pivot_gap(&scene, (ships[1], 6.0), ships[2]));
    }
    assert!(worst_gap < 0.05, "the joints hold: {worst_gap} m");
    assert_eq!(report().couplings, 2);
}

#[test]
fn a_block_placed_on_a_car_of_a_group_on_a_rail_steps_the_whole_group_by_the_shift_of_its_centre_of_mass()
 {
    let (mut scene, _report) = Scene::reporting_in_a_new_world();
    let distance = 70_016.0;
    let speed = circular_speed(&scene, 0, distance);
    let ships = a_chain_in_orbit(&mut scene, a_chain(), distance, speed);
    let rail = put_on_its_rail(&mut scene, &ships, speed, vec![([0.0; 3], [0.0; 3]); 3]);
    scene.ticks(5);
    let (centre_before, _) = centre_of(&scene, &ships);
    let (point_before, _) = rail_point(&scene, &rail);
    let origins_before = origins_of(&scene, &ships);
    let (mass_before, last_mass_before) = (mass_of(&scene, &ships), mass_of(&scene, &ships[2..]));
    // A stone on the end of the last car: its middle is 6.5 along the car, 0.5 up and across.
    let content = CaveContent::new();
    let last = ships[2];
    scene.run(move |world| {
        world
            .grid(last)
            .unwrap()
            .set(
                content.block_channel(),
                VoxelCoordinate::new(6, 0, 0),
                stone(),
            )
            .unwrap();
    });
    let (centre_after, _) = centre_of(&scene, &ships);
    let (point_after, _) = rail_point(&scene, &rail);
    close(
        centre_after,
        point_after,
        1e-6,
        "the centre of mass of the group, with the stone, is on the rail",
    );
    let block = mass_of(&scene, &ships[2..]) - last_mass_before;
    assert!(block > 0.0, "the stone weighs {block}");
    // Where the stone was, before the group moved (the cars are not turned): the group is stepped by the weight of the stone over the weight of the group, away from it.
    let stone_at = [0, 1, 2].map(|axis| origins_before[2][axis] + [6.5, 0.5, 0.5][axis]);
    let towards = minus(stone_at, centre_before);
    let step = towards.map(|part| -part * block / (mass_before + block));
    let origins_after = origins_of(&scene, &ships);
    for index in 0..3 {
        // Each origin from the rail's point, before and after: it changed by the step.
        let before = minus(origins_before[index], point_before);
        let after = minus(origins_after[index], point_after);
        close(
            minus(after, before),
            step,
            1e-6,
            &format!("car {index} was stepped by the shift"),
        );
    }
    assert!(length(step) > 0.05, "and the step is not nothing: {step:?}");
}

#[test]
fn a_block_placed_on_a_car_that_swings_does_not_give_the_group_a_speed_of_its_own() {
    // A stone appears on a car that is moving against the others: the group weighs more, and with the speed of that car in it would go off (the stone comes from nowhere, and
    // is given the car's speed). The rail is the authority for how the group goes, and the bodies take that out: the centre of mass of the group goes at the speed of the rail.
    let (mut scene, _report) = Scene::reporting_in_a_new_world();
    let distance = 70_016.0;
    let speed = circular_speed(&scene, 0, distance);
    let ships = a_chain_in_orbit(&mut scene, a_chain(), distance, speed);
    // The last car swings fast about its pivot, the others little.
    let motion = vec![
        ([0.0; 3], [0.0, 0.0, 0.0]),
        ([0.0; 3], [0.0, 0.0, 0.0]),
        ([0.0; 3], [0.0, 0.0, 1.0]),
    ];
    let rail = put_on_its_rail(&mut scene, &ships, speed, motion);
    scene.ticks(5);
    let content = CaveContent::new();
    let last = ships[2];
    scene.run(move |world| {
        world
            .grid(last)
            .unwrap()
            .set(
                content.block_channel(),
                VoxelCoordinate::new(6, 0, 0),
                stone(),
            )
            .unwrap();
    });
    let (place, going) = centre_of(&scene, &ships);
    let (on_the_rail, rail_speed) = rail_point(&scene, &rail);
    close(place, on_the_rail, 1e-6, "the group is on the rail");
    close(going, rail_speed, 1e-6, "and goes as the rail does");
    // (The control of this test is that the cars move against the group: without that, no stone could give the group a speed.)
    let against_the_group = ships
        .iter()
        .map(|ship| {
            let own = scene
                .rig
                .server
                .world()
                .grids()
                .get(*ship)
                .unwrap()
                .linear_velocity;
            length(minus(own, going))
        })
        .fold(0.0, f64::max);
    assert!(
        against_the_group > 0.05,
        "the cars move against the group: {against_the_group}"
    );
}

#[test]
fn the_group_is_on_its_rail_as_soon_as_the_bodies_have_moved_it_and_not_only_when_the_world_puts_it_there()
 {
    // The world puts a group on its rail at the beginning of a tick and at the end (the centre of mass by one displacement, the speed by one change), so a group that the bodies
    // left off it would be on it at the end of every tick all the same. What is between, and is seen by whatever runs after the bodies in the tick (the lifecycle, the rules), is
    // the bodies' to get right: here a watcher looks at the world after them, in every tick of a chain that swings and gets a stone in the middle of it. The centre of mass is the
    // rail's place and goes at the rail's speed, to the rounding of the sums, in all of them.
    let (mut scene, _report) = Scene::reporting_in_a_new_world();
    let distance = 70_016.0;
    let speed = circular_speed(&scene, 0, distance);
    let ships = a_chain_in_orbit(&mut scene, a_chain(), distance, speed);
    let rail = put_on_its_rail(&mut scene, &ships, speed, a_swing());
    let planet = scene
        .rig
        .server
        .world()
        .grids()
        .get(ship_id(0))
        .unwrap()
        .pose
        .position
        .in_voxels();
    let seen: std::sync::Arc<std::sync::Mutex<Vec<(u64, Vec3, Vec3)>>> = Default::default();
    let (record, watched) = (std::sync::Arc::clone(&seen), ships.clone());
    scene.watch(move |world| {
        let centre = world.data().grids().group_centre(&watched).unwrap();
        record.lock().unwrap().push((
            world.data().clock(),
            centre.position.in_voxels(),
            centre.velocity,
        ));
    });
    let content = CaveContent::new();
    let last = ships[2];
    for tick in 0..100 {
        if tick == 50 {
            let content = CaveContent::new();
            scene.run(move |world| {
                world
                    .grid(last)
                    .unwrap()
                    .set(
                        content.block_channel(),
                        VoxelCoordinate::new(6, 0, 0),
                        stone(),
                    )
                    .unwrap();
            });
        } else {
            scene.ticks(1);
        }
    }
    let _ = content;
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 100);
    for (clock, place, going) in seen.iter() {
        let state = rail.state(
            RailTime::of_tick(*clock, TICKS_PER_SECOND),
            planet,
            [0.0; 3],
        );
        close(
            *place,
            state.position.unwrap(),
            1e-8,
            &format!("clock {clock}: after the bodies, the centre of mass is the rail's"),
        );
        close(
            *going,
            state.velocity,
            1e-8,
            &format!("clock {clock}: and goes as the rail does"),
        );
    }
}

#[test]
fn what_the_world_is_made_to_say_of_a_ship_of_a_group_on_a_rail_is_not_taken_back_by_the_bodies() {
    // The ships of a group on a rail are the bodies': where they stand against each other, how they move against each other, how they turn. Whatever else moves one takes the
    // group off the rail first (a force, a move, a speed), so a number in the world's record of a ship that nothing of that kind wrote is no word of anyone's, and is not taken
    // for one. Here the record of the middle car is made to say it moves and turns fast, as the bodies themselves write it (which takes no rail off), in one of two chains that
    // were the same: they stay the same.
    let chain = |tamper: bool| {
        let mut scene = Scene::in_a_new_world();
        let distance = 70_016.0;
        let speed = circular_speed(&scene, 0, distance);
        let ships = a_chain_in_orbit(&mut scene, a_chain(), distance, speed);
        put_on_its_rail(&mut scene, &ships, speed, a_swing());
        scene.ticks(10);
        // (The same tick in both, which does the tampering in one.)
        let middle = ships[1];
        scene.run(move |world| {
            if tamper {
                let ship = world.data().grids().get(middle).unwrap();
                let (pose, speed) = (ship.pose, ship.linear_velocity);
                world
                    .move_grid_by_bodies(
                        middle,
                        pose,
                        [speed[0] + 3.0, speed[1], speed[2]],
                        [0.0, 0.0, 2.0],
                    )
                    .unwrap();
            }
        });
        scene.ticks(10);
        ships
            .iter()
            .map(|ship| (orientation_of(&scene, *ship), spin_of(&scene, *ship)))
            .collect::<Vec<_>>()
    };
    let untouched = chain(false);
    let tampered = chain(true);
    assert!(
        most_apart(&untouched, &tampered) < 1e-9,
        "the two chains are {} apart",
        most_apart(&untouched, &tampered)
    );
}

/// What a lifecycle asks of ships in a world with players: they stop moving 110 voxels from the nearest player and are put in files beyond 330.
fn kept_by_distance() -> ShipSettings {
    ShipSettings::new(100.0, 300.0).unwrap()
}

fn player_near(scene: &mut Scene, place: Vec3) -> Player {
    let mut player = scene
        .rig
        .join([place[0], place[1] + 40.0, place[2]], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    player
}

/// Sends the player to a place a little ahead of the group (40 voxels above it, and where it will be in two ticks, since the group goes 38 voxels a tick and the command is
/// heard a tick or two later), so that a player who is kept sent there is always near it.
fn send_the_player_to(player: &mut Player, group: (Vec3, Vec3)) {
    let (place, going) = group;
    let ahead = 2.0 / f64::from(TICKS_PER_SECOND);
    player.send_command(&format!(
        "/tp {} {} {}",
        place[0] + going[0] * ahead,
        place[1] + going[1] * ahead + 40.0,
        place[2] + going[2] * ahead
    ));
}

#[test]
fn a_group_on_a_rail_that_no_player_is_near_is_frozen_in_memory_and_goes_on_round_and_wakes_with_its_joints()
 {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let distance = 70_016.0;
    let speed = circular_speed(&scene, 0, distance);
    let ships = a_chain_in_orbit(&mut scene, a_chain(), distance, speed);
    let rail = put_on_its_rail(&mut scene, &ships, speed, a_swing());
    let here = centre_of(&scene, &ships).0;
    let mut player = player_near(&mut scene, here);
    scene.rig.server.set_ship_settings(kept_by_distance());
    // The group goes 38 voxels a tick and the player stays: in a few ticks it is beyond the 110 a ship stops at, and in a few more beyond the 330 a ship is put in a file at,
    // which a ship on a rail is not (its place is the clock's).
    tick_and_follow(&mut scene, &mut player, 30);
    for ship in ships.iter().copied() {
        let grid = scene
            .rig
            .server
            .world()
            .grids()
            .get(ship)
            .expect("in memory");
        assert!(grid.frozen, "{ship}");
        assert!(scene.rig.server.world().grids().get_parked(ship).is_none());
    }
    assert_eq!(
        scene.rig.server.world().grids().get(ships[0]).unwrap().rail,
        Some(rail.clone())
    );
    // It goes on round: on the rail, its ships where they stand against each other, all at the speed of the rail, none turning.
    let stood = origins_of(&scene, &ships);
    tick_and_follow(&mut scene, &mut player, 40);
    let (place, going) = centre_of(&scene, &ships);
    let (on_the_rail, rail_speed) = rail_point(&scene, &rail);
    close(place, on_the_rail, 1e-6, "the frozen group is on the rail");
    close(going, rail_speed, 1e-6, "and goes as the rail does");
    let now = origins_of(&scene, &ships);
    for index in 1..3 {
        close(
            minus(now[index], now[0]),
            minus(stood[index], stood[0]),
            1e-6,
            "its ships stand as they stood",
        );
    }
    for ship in ships.iter().copied() {
        assert_eq!(spin_of(&scene, ship), [0.0; 3], "{ship} does not turn");
    }
    assert_eq!(report().couplings, 2, "and it is coupled still");
    // The player comes to it: it wakes, and its joints are the joints it had.
    send_the_player_to(&mut player, centre_of(&scene, &ships));
    tick_and_follow(&mut scene, &mut player, 3);
    for ship in ships.iter().copied() {
        assert!(
            !scene.rig.server.world().grids().get(ship).unwrap().frozen,
            "{ship} is awake"
        );
    }
    let mut worst_gap: f64 = 0.0;
    for _ in 0..40 {
        // (The player is kept with the group, or the group, which goes 38 voxels a tick, would be frozen again as soon as the player was 110 behind.)
        send_the_player_to(&mut player, centre_of(&scene, &ships));
        tick_and_follow(&mut scene, &mut player, 1);
        worst_gap = worst_gap
            .max(pivot_gap(&scene, (ships[0], 7.0), ships[1]))
            .max(pivot_gap(&scene, (ships[1], 6.0), ships[2]));
        let (place, _) = centre_of(&scene, &ships);
        close(
            place,
            rail_point(&scene, &rail).0,
            1e-6,
            "and rides the rail again",
        );
    }
    assert!(worst_gap < 0.05, "the joints hold: {worst_gap} m");
    assert_eq!(report().couplings, 2);
}
