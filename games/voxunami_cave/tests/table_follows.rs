//! A server whose package is replaced while it runs (`hot-reload-plan.md`, step 5): every rule that read the table of blocks follows it. Each test makes a change to the
//! package of Cave that one rule can show, replaces the package of a running server with it, and looks at what the rule does next; and, where it can, puts the
//! package back and looks again.

mod common;

use common::docking::{a_pair, make, make_going, stones};
use common::{
    FLOOR, PlayerExt, Scene, build_raft, eyes_in_the_world, flat_floor, pose_at, put_stone, say,
    ship_id, ship_voxels, tick_and_follow,
};
use game_shell::{GameContent, PackageData};
use namespaced_id::NamespacedId;
use protocol::Message;
use server::GridView;
use voxel_space::{FaceDirection, GravitySettings, GridId, LevelIndex, VoxelCoordinate};
use voxunami_cave::content::{CaveContent, dev_docking_port_id};
use voxunami_cave::game::request_block;
use voxunami_cave::messages::SetBlock;

fn id(text: &str) -> NamespacedId {
    text.parse().unwrap()
}

use common::{changed_package as changed, original_package as original};

/// Bodies move through stone as if it were not there.
fn stone_is_passable() -> PackageData {
    changed(|name, definition| {
        if name == "base_blocks:stone" {
            definition.set_property_value(id("base_blocks:passable"), None, 1);
        }
    })
}

#[test]
fn the_package_lists_the_blocks_in_the_order_of_their_numbers() {
    // (What block editing finds a block's number by: its place in the list. The registry of the world is the other way to know.)
    let content = CaveContent::new();
    let package = content.package_data();
    let registry = content.registries()[0].1;
    assert_eq!(package.blocks.len(), registry.entry_count());
    for (number, (identifier, _)) in package.blocks.iter().enumerate() {
        assert_eq!(registry.identifier_of_raw(number as u32), Some(identifier));
        assert_eq!(package.raw_of(identifier), Some(number as u32));
    }
}

// ---- bodies: players ----

#[test]
fn a_walker_falls_through_what_has_become_passable() {
    let mut scene = Scene::new();
    flat_floor(&mut scene);
    let mut player = scene.rig.join_walking([30.5, FLOOR + 3.0, 30.5], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 40);
    let standing = eyes_in_the_world(&scene, 1)[1];
    assert!(
        (FLOOR + 1.0..FLOOR + 3.0).contains(&standing),
        "stands on the floor: {standing}"
    );

    scene.package.replace(stone_is_passable()).unwrap();
    tick_and_follow(&mut scene, &mut player, 80);
    let fallen = eyes_in_the_world(&scene, 1)[1];
    assert!(
        fallen < standing - 3.0,
        "fell through the floor: {standing} then {fallen}"
    );
}

// ---- bodies: ships ----

#[test]
fn a_ship_at_rest_on_the_floor_falls_through_it_when_the_floor_becomes_passable() {
    let mut scene = Scene::new();
    flat_floor(&mut scene);
    common::make_ship(
        &mut scene,
        pose_at(30.0, FLOOR, 30.0),
        false,
        [3, 3, 3],
        |_| true,
    );
    scene.ticks(60);
    let resting = ship_voxels(&scene, 0)[1];
    assert!(
        (FLOOR - 0.5..FLOOR + 0.5).contains(&resting),
        "rests on the floor: {resting}"
    );

    // (Its own blocks are stone too, and are passable then: nothing of it is in the way of what falls, so the test is of the colliders being made again.)
    scene.package.replace(stone_is_passable()).unwrap();
    scene.ticks(120);
    let fallen = ship_voxels(&scene, 0)[1];
    assert!(
        fallen < resting - 5.0,
        "fell through the floor: {resting} then {fallen}"
    );
}

#[test]
fn a_ship_weighs_what_the_blocks_weigh_now() {
    let mut scene = Scene::new();
    common::make_ship(
        &mut scene,
        pose_at(30.0, FLOOR + 10.0, 30.0),
        true,
        [3, 3, 3],
        |_| true,
    );
    scene.ticks(5);
    let mass = |scene: &Scene| {
        scene
            .rig
            .server
            .world()
            .grids()
            .get(ship_id(0))
            .unwrap()
            .mass
    };
    let before = mass(&scene);
    assert!(before > 0.0);

    // Stone twice as dense as it was (the density is in kilograms a voxel, 1000 by default).
    scene
        .package
        .replace(changed(|name, definition| {
            if name == "base_blocks:stone" {
                definition.set_property_value(id("engine:density"), None, 2000);
            }
        }))
        .unwrap();
    scene.ticks(5);
    let heavier = mass(&scene);
    assert!(
        (heavier - 2.0 * before).abs() < 1e-6 * before,
        "{before} then {heavier}"
    );

    scene.package.replace(original()).unwrap();
    scene.ticks(5);
    assert!((mass(&scene) - before).abs() < 1e-6 * before);
}

/// Where a ship that was made going at 3 voxels a second in weightless space is `ticks` ticks after, with the package replaced by the same one at tick `replaced_at` (or
/// not replaced at all).
fn where_a_going_ship_is_after(ticks: usize, replaced_at: Option<usize>) -> [f64; 3] {
    let mut scene = Scene::new();
    scene.run(|world| {
        world.set_gravity_settings(GravitySettings {
            base: [0.0; 3],
            ..GravitySettings::default()
        });
    });
    make_going(
        &mut scene,
        pose_at(30.0, FLOOR + 30.0, 30.0),
        stones(0, 4),
        [3.0, 0.0, 1.0],
    );
    for tick in 0..ticks {
        if replaced_at == Some(tick) {
            scene.package.replace(original()).unwrap();
        }
        scene.ticks(1);
    }
    ship_voxels(&scene, 0)
}

#[test]
fn a_replacement_that_changes_nothing_does_not_move_a_ship_that_is_going() {
    // (The bodies of every ship are made again from the world's place and speed for it after a replacement, which has nothing to say of what a ship does: the ship is
    // where it would have been, to what a tick of the bodies is accurate to.)
    let undisturbed = where_a_going_ship_is_after(80, None);
    let replaced = where_a_going_ship_is_after(80, Some(20));
    assert!(undisturbed[0] > 30.0 + 3.0, "it went: {undisturbed:?}");
    for axis in 0..3 {
        assert!(
            (undisturbed[axis] - replaced[axis]).abs() < 1e-6,
            "{undisturbed:?} against {replaced:?}"
        );
    }
}

// ---- docking ----

#[test]
fn the_docking_ports_of_a_ship_are_the_blocks_the_package_says_are_ports_now() {
    let mut scene = Scene::new();
    let (first, _) = a_pair();
    make(&mut scene, pose_at(20.0, FLOOR + 10.0, 20.0), true, first);
    let docking =
        voxunami_cave::plugins::docking_sharing(&CaveContent::new(), scene.package.table());
    let ports = |scene: &Scene| {
        let world = scene.rig.server.world();
        docking
            .ports_of(world.grids().get(ship_id(0)).unwrap())
            .len()
    };
    assert_eq!(ports(&scene), 1);

    scene
        .package
        .replace(changed(|_, definition| {
            definition.remove_property_value(&id("engine:docking_face"), None);
        }))
        .unwrap();
    assert_eq!(ports(&scene), 0, "no block is a port now");

    scene.package.replace(original()).unwrap();
    assert_eq!(ports(&scene), 1);
}

// ---- ship commands ----

#[test]
fn the_ship_commands_look_at_what_the_package_says_is_there_now() {
    let mut scene = Scene::new();
    flat_floor(&mut scene);
    build_raft(&mut scene, 20, FLOOR as i128 + 6, 22, 8);
    let mut player = scene.rig.join_walking([24.0, FLOOR + 3.0, 34.0], 0.0, 30.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 20);

    // Where the raft is stone that bodies pass through, there is nothing to look at.
    scene.package.replace(stone_is_passable()).unwrap();
    assert!(say(&mut scene, &mut player, "/ship create").contains("not looking at a block"));

    scene.package.replace(original()).unwrap();
    assert_eq!(
        say(&mut scene, &mut player, "/ship create"),
        "made ship 0 of 64 blocks (it falls unless you /ship hold on)"
    );
}

// ---- block editing ----

/// A player standing east of a place on the ground looks `yaw` degrees round and puts a docking port in it, the top of the ground clicked; the orientation it was given.
fn orientation_of_a_port_put_down(scene: &mut Scene, voxel: VoxelCoordinate, yaw: f64) -> u32 {
    let mut player = scene.rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    player.camera = game_shell::view::camera_at(
        [
            voxel.x() as f64 + 3.5,
            voxel.y() as f64 + 3.0,
            voxel.z() as f64 + 0.5,
        ],
        yaw,
        0.0,
    );
    player.teleport_to_camera();
    scene.rig.settle(&mut [&mut player]);
    let request = SetBlock {
        grid: GridId::root(LevelIndex::MAIN),
        at: voxel,
        block: dev_docking_port_id(),
        state: 0,
        against: Some(FaceDirection::PositiveY),
    };
    player.send_game_message(SetBlock::kind(), request.encode());
    scene.rig.settle(&mut [&mut player]);
    let content = &scene.rig.content;
    let overrides = &scene.rig.server.world().main().overrides;
    overrides.get(content.orientation_channel(), voxel)
}

#[test]
fn a_block_is_turned_by_what_the_package_says_of_it_now_and_a_block_it_gained_can_be_placed() {
    let mut scene = Scene::new();
    let ground = {
        let mut probe = scene.rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
        scene.rig.settle(&mut [&mut probe]);
        common::ground_height(&probe, 16, 16)
    };
    let spot = |along: i128| VoxelCoordinate::new(16 + along, ground + 1, 16);
    let looks = [0.0, 90.0, 180.0, 270.0];
    let orientations = |scene: &mut Scene, first: i128| -> Vec<u32> {
        looks
            .iter()
            .enumerate()
            .map(|(number, yaw)| {
                orientation_of_a_port_put_down(scene, spot(first + number as i128), *yaw)
            })
            .collect()
    };
    let distinct = |mut orientations: Vec<u32>| {
        orientations.sort_unstable();
        orientations.dedup();
        orientations.len()
    };
    assert_eq!(
        distinct(orientations(&mut scene, 0)),
        4,
        "four looks, four turns"
    );

    // A port with no front is turned by what it is put against alone, whichever way the player looks.
    scene
        .package
        .replace(changed(|_, definition| {
            definition.remove_property_value(&id("engine:front_face"), None);
        }))
        .unwrap();
    assert_eq!(
        distinct(orientations(&mut scene, 4)),
        1,
        "no front, no turn to the player"
    );

    // A block that the package gained is a block a player can put down; before, the server did not know the name.
    let marble = id("test:marble");
    let spot_of_marble = spot(9);
    let place_marble = |scene: &mut Scene| {
        let mut player = scene.rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
        scene.rig.settle(&mut [&mut player]);
        player.camera = game_shell::view::camera_at(
            [
                spot_of_marble.x() as f64 + 0.5,
                spot_of_marble.y() as f64 + 3.0,
                spot_of_marble.z() as f64 + 0.5,
            ],
            0.0,
            0.0,
        );
        player.teleport_to_camera();
        scene.rig.settle(&mut [&mut player]);
        request_block(&mut player, spot_of_marble, marble.as_str());
        // (The server alone ticks: no player is told of the block, which no client knows yet, until the message of a replaced package exists, and a player who was told
        // would stop at a block outside their registry.)
        scene.ticks(2);
        scene
            .rig
            .server
            .world()
            .main()
            .overrides
            .get(scene.rig.content.block_channel(), spot_of_marble)
    };
    assert_eq!(
        place_marble(&mut scene),
        0,
        "an unknown block is not placed"
    );
    let mut with_marble = original();
    let stone = with_marble.blocks[scene.rig.content.blocks.stone as usize]
        .1
        .clone();
    with_marble.blocks.push((marble.clone(), stone));
    let number = with_marble.raw_of(&marble).unwrap();
    scene.package.replace(with_marble).unwrap();
    assert_eq!(
        place_marble(&mut scene),
        number,
        "the block the package gained is placed"
    );
}

// ---- items ----

fn items_on_the_ground(scene: &Scene) -> Vec<VoxelCoordinate> {
    scene
        .rig
        .server
        .world()
        .entities()
        .iter()
        .filter(|entity| entity.kind == voxunami_cave::items::dropped_item_kind())
        .map(|entity| voxunami_cave::items::voxel_of(entity.pose.position))
        .collect()
}

#[test]
fn an_item_falls_through_a_block_that_has_become_passable() {
    let mut scene = Scene::new();
    let mut player = scene.rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    player.camera = common::above_ground(&player, 16, 16, 8.0);
    player.teleport_to_camera();
    scene.rig.settle(&mut [&mut player]);
    player.take_command_replies();
    let ground = VoxelCoordinate::new(16, common::ground_height(&player, 16, 16), 16);
    let at = |height: i128| VoxelCoordinate::new(ground.x(), ground.y() + height, ground.z());
    let tick = |scene: &mut Scene, player: &mut common::Player, count: usize| {
        for _ in 0..count {
            scene.rig.step();
            player.advance().unwrap();
            let _ = player.update_view();
        }
    };
    request_block(&mut player, at(1), "base_blocks:stone");
    request_block(&mut player, at(2), "base_blocks:stone");
    tick(&mut scene, &mut player, 3);
    player.send_command("/gamemode survival");
    tick(&mut scene, &mut player, 2);
    request_block(&mut player, at(2), "base_blocks:air");
    tick(&mut scene, &mut player, 2);
    let settle = voxunami_cave::items::FALL_INTERVAL_TICKS as usize * 3;
    tick(&mut scene, &mut player, settle);
    assert_eq!(items_on_the_ground(&scene), [at(2)], "held up by the stone");

    // The stone is something bodies pass through now: the item falls into it, and rests on the ground.
    scene.package.replace(stone_is_passable()).unwrap();
    tick(&mut scene, &mut player, settle);
    assert_eq!(
        items_on_the_ground(&scene),
        [at(1)],
        "falls through the stone"
    );
}

// ---- fire ----

#[test]
fn what_burns_is_what_the_package_says_burns_now() {
    let mut scene = Scene::new();
    flat_floor(&mut scene);
    // Stone, with a fire on it, and a player near enough for the fire to burn.
    let mut player = scene.rig.join([30.5, FLOOR + 6.0, 34.5], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    let content = CaveContent::new();
    let fire_voxel = VoxelCoordinate::new(30, FLOOR as i128, 30);
    let burn = |scene: &mut Scene| {
        let content = CaveContent::new();
        scene.run(move |world| {
            let fire = content.blocks.fire;
            let mut grid = world.grid(GridId::root(LevelIndex::MAIN)).unwrap();
            grid.set(content.block_channel(), fire_voxel, fire).unwrap();
        });
        scene.ticks(400);
    };
    let floor = |scene: &mut Scene| common::block_at(scene, [30, FLOOR as i128 - 1, 30]);
    burn(&mut scene);
    assert_eq!(
        floor(&mut scene),
        content.blocks.stone,
        "stone does not burn"
    );

    scene
        .package
        .replace(changed(|name, definition| {
            if name == "base_blocks:stone" {
                definition.set_property_value(id("base_blocks:flammability"), None, 255);
            }
        }))
        .unwrap();
    burn(&mut scene);
    assert_ne!(
        floor(&mut scene),
        content.blocks.stone,
        "stone that burns, burnt"
    );
    let _ = &mut player;
}

// ---- light ----

#[test]
fn what_gives_light_is_what_the_package_says_gives_light_now() {
    let mut scene = Scene::new();
    let mut player = scene.rig.join([30.5, 140.0, 30.5], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    let content = CaveContent::new();
    // A stone put in the open, built by someone, so that its brick is stored (light is kept only in stored bricks).
    let (stone_at, beside) = ([30, 130, 30], VoxelCoordinate::new(30, 131, 30));
    put_stone(&mut scene, stone_at);
    scene.ticks(30);
    let light_at = |scene: &Scene| {
        scene
            .rig
            .server
            .world()
            .main()
            .overrides
            .get(content.light_channel(), beside)
    };
    assert_eq!(light_at(&scene), 0, "stone is dark");

    scene
        .package
        .replace(changed(|name, definition| {
            if name == "base_blocks:stone" {
                definition.set_property_value(id("base_blocks:emission"), None, 14);
            }
        }))
        .unwrap();
    scene.ticks(60);
    assert!(light_at(&scene) > 0, "stone gives light now");

    scene.package.replace(original()).unwrap();
    scene.ticks(60);
    assert_eq!(light_at(&scene), 0, "and does not when it is put back");
    let _ = &mut player;
}

/// A flat floor, a player near it, and two ships weightless above it, the second on its way to dock with the first.
fn a_pair_of_ships_in_space() -> (Scene, common::Player) {
    let mut scene = Scene::new();
    flat_floor(&mut scene);
    let mut player = scene.rig.join([22.0, FLOOR + 5.0, 30.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    scene.run(|world| {
        world.set_gravity_settings(GravitySettings {
            base: [0.0; 3],
            ..GravitySettings::default()
        });
    });
    let (first, second) = a_pair();
    make(&mut scene, pose_at(20.0, FLOOR + 10.0, 20.0), false, first);
    make(&mut scene, pose_at(27.0, FLOOR + 10.0, 20.0), false, second);
    (scene, player)
}

fn ships_in(scene: &Scene) -> Vec<GridId> {
    scene
        .rig
        .server
        .world()
        .grids()
        .iter()
        .map(|(id, _)| id)
        .collect()
}

/// The mass of every ship, each times the speed of its middle, added: the momentum.
fn momentum_of(scene: &Scene) -> [f64; 3] {
    let mut total = [0.0; 3];
    for id in ships_in(scene) {
        let ship = scene.rig.server.world().grids().get(id).unwrap();
        for (sum, speed) in total.iter_mut().zip(ship.linear_velocity) {
            *sum += ship.mass * speed;
        }
    }
    total
}

#[test]
fn the_two_sides_of_an_undocked_ship_part_by_what_their_blocks_weigh_now() {
    // The sides of an undocked ship are sent apart at a speed that is shared by their masses, so that the momentum is what it was: the masses are those of the table
    // now. A port that has become five times as heavy, and the masses of the old table, would leave the pair with a momentum.
    let (mut scene, mut player) = a_pair_of_ships_in_space();
    let mut ticks = 0;
    while ships_in(&scene).len() > 1 && ticks < 900 {
        tick_and_follow(&mut scene, &mut player, 1);
        ticks += 1;
    }
    assert_eq!(ships_in(&scene).len(), 1, "docked after {ticks} ticks");
    scene
        .package
        .replace(changed(|name, definition| {
            if name == "voxunami_cave:dev_docking_port" {
                definition.set_property_value(id("engine:density"), None, 5000);
            }
        }))
        .unwrap();
    tick_and_follow(&mut scene, &mut player, 3);
    let reply = say(&mut scene, &mut player, "/ship undock");
    assert!(reply.contains("came apart into ships"), "{reply}");
    scene.ticks(2);
    let momentum = momentum_of(&scene);
    assert!(
        momentum.iter().all(|component| component.abs() < 5.0),
        "{momentum:?}"
    );
}
