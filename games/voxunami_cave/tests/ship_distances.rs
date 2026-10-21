//! Ships by how near the players are, in Cave: a ship far from every player is where it was and waits, farther still it is in a file, and a player who
//! comes back finds it and sets it going again. (The tests of what ships do otherwise keep them going wherever the players are.)

mod common;

use common::{FLOOR, Scene, flat_floor, make_ship, pose_at, ship_id, tick_and_follow};
use server::ShipSettings;

fn height_of_the_ship(scene: &Scene) -> Option<f32> {
    let ship = scene.rig.server.world().grids().get(ship_id(0))?;
    Some(ship.pose.position.brick().y() as f32 * 32.0 + ship.pose.position.offset()[1])
}

#[test]
fn a_ship_that_falls_stops_where_it_is_when_the_player_goes_away_is_put_in_a_file_and_falls_on_when_they_come_back()
 {
    let mut scene = Scene::new();
    flat_floor(&mut scene);
    scene
        .rig
        .server
        .set_ship_settings(ShipSettings::new(40.0, 80.0).unwrap());
    // A flying player, near where the ship is made.
    let mut player = scene
        .rig
        .join([20.0, f64::from(FLOOR) + 12.0, 20.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    make_ship(
        &mut scene,
        pose_at(14.0, FLOOR + 30.0, 14.0),
        false,
        [4, 2, 4],
        |_| true,
    );
    tick_and_follow(&mut scene, &mut player, 6);
    let falling = height_of_the_ship(&scene).unwrap();
    assert!(
        falling < FLOOR + 30.0 - 0.5 && falling > FLOOR + 5.0,
        "{falling}"
    );

    // The player goes a long way off. The ship is where it was when they went, and stays there, and then is in a file.
    player.send_command("/tp 2000 280 20");
    tick_and_follow(&mut scene, &mut player, 2);
    let stopped = height_of_the_ship(&scene);
    assert!(
        scene.rig.server.world().grids().get(ship_id(0)).is_none()
            || scene
                .rig
                .server
                .world()
                .grids()
                .get(ship_id(0))
                .unwrap()
                .frozen
    );
    tick_and_follow(&mut scene, &mut player, 40);
    assert!(
        scene.rig.server.world().grids().get(ship_id(0)).is_none(),
        "in a file"
    );
    assert!(
        scene
            .rig
            .server
            .world()
            .grids()
            .get_parked(ship_id(0))
            .is_some()
    );
    assert_eq!(scene.rig.files.len(), 1);
    // (It did not fall on in the meantime: the height it was parked at is the height it was frozen at.)
    let parked = scene
        .rig
        .server
        .world()
        .grids()
        .get_parked(ship_id(0))
        .unwrap()
        .pose;
    let parked_height = parked.position.brick().y() as f32 * 32.0 + parked.position.offset()[1];
    if let Some(stopped) = stopped {
        assert_eq!(parked_height, stopped);
    }
    assert!(parked_height > FLOOR + 5.0, "{parked_height}");

    // The player comes back: the ship is brought in at rest where it was, and falls on to the floor.
    player.send_command("/tp 20 280 20");
    tick_and_follow(&mut scene, &mut player, 3);
    assert!(scene.rig.server.world().grids().get(ship_id(0)).is_some());
    tick_and_follow(&mut scene, &mut player, 120);
    let rested = height_of_the_ship(&scene).unwrap();
    assert!(
        (rested - FLOOR).abs() < 0.1,
        "it rests on the floor: {rested}"
    );
}

#[test]
fn a_ship_nobody_is_near_does_not_fall_and_one_a_player_comes_near_does() {
    let mut scene = Scene::new();
    flat_floor(&mut scene);
    // Ships are frozen when nobody is within 40 and never put in files (so that this is only about waiting).
    scene
        .rig
        .server
        .set_ship_settings(ShipSettings::new(40.0, f64::INFINITY).unwrap());
    make_ship(
        &mut scene,
        pose_at(14.0, FLOOR + 30.0, 14.0),
        false,
        [4, 2, 4],
        |_| true,
    );
    scene.ticks(60);
    // Two seconds, and a ship that was let go would have fallen to the floor in one and a half: it hangs where it was made.
    assert_eq!(height_of_the_ship(&scene), Some(FLOOR + 30.0));
    assert!(
        scene
            .rig
            .server
            .world()
            .grids()
            .get(ship_id(0))
            .unwrap()
            .frozen
    );

    let mut player = scene
        .rig
        .join([20.0, f64::from(FLOOR) + 12.0, 20.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 120);
    let rested = height_of_the_ship(&scene).unwrap();
    assert!(
        (rested - FLOOR).abs() < 0.1,
        "it fell, and rests on the floor: {rested}"
    );
    assert!(
        !scene
            .rig
            .server
            .world()
            .grids()
            .get(ship_id(0))
            .unwrap()
            .frozen
    );
}

#[test]
fn what_a_ship_holds_is_the_same_after_it_has_been_in_a_file() {
    let mut scene = Scene::new();
    flat_floor(&mut scene);
    scene
        .rig
        .server
        .set_ship_settings(ShipSettings::new(40.0, 80.0).unwrap());
    let mut player = scene
        .rig
        .join([20.0, f64::from(FLOOR) + 12.0, 20.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    make_ship(
        &mut scene,
        pose_at(14.0, FLOOR + 3.0, 14.0),
        true,
        [4, 2, 4],
        |[x, y, z]| (x + y + z) % 3 != 0,
    );
    tick_and_follow(&mut scene, &mut player, 3);
    let digest = scene
        .rig
        .server
        .world()
        .grids()
        .get(ship_id(0))
        .unwrap()
        .voxels
        .content_digest();
    player.send_command("/tp 3000 280 20");
    tick_and_follow(&mut scene, &mut player, 40);
    assert!(
        scene
            .rig
            .server
            .world()
            .grids()
            .get_parked(ship_id(0))
            .is_some()
    );
    player.send_command("/tp 20 280 20");
    tick_and_follow(&mut scene, &mut player, 5);
    let ship = scene.rig.server.world().grids().get(ship_id(0)).unwrap();
    assert_eq!(ship.voxels.content_digest(), digest);
    assert!(ship.held, "a held ship is held still");
}

#[test]
fn the_light_of_a_ship_waits_while_it_is_frozen_and_is_made_when_a_player_comes_near() {
    use server::GridView;
    use voxel_space::{LevelIndex, VoxelCoordinate};
    use voxunami_cave::content::CaveContent;

    let content = CaveContent::new();
    let (block, light, lamp) = (
        content.block_channel(),
        content.light_channel(),
        content.blocks.lamp,
    );
    let mut scene = Scene::new();
    flat_floor(&mut scene);
    scene
        .rig
        .server
        .set_ship_settings(ShipSettings::new(40.0, f64::INFINITY).unwrap());
    // A ship that is a lamp, held where it is, with nobody near it.
    scene.run(move |world| {
        let ship = world
            .create_grid(LevelIndex::MAIN, pose_at(14.0, FLOOR + 30.0, 14.0))
            .unwrap();
        world.set_grid_held(ship, true).unwrap();
        world
            .grid(ship)
            .unwrap()
            .set(block, VoxelCoordinate::new(2, 2, 2), lamp)
            .unwrap();
    });
    scene.ticks(20);
    let light_beside_the_lamp = |scene: &Scene| {
        scene
            .rig
            .server
            .world()
            .grids()
            .get(ship_id(0))
            .unwrap()
            .voxels
            .get(light, VoxelCoordinate::new(3, 2, 2))
    };
    // It was frozen in the tick it was made, and its light was not looked at.
    assert!(
        scene
            .rig
            .server
            .world()
            .grids()
            .get(ship_id(0))
            .unwrap()
            .frozen
    );
    assert_eq!(light_beside_the_lamp(&scene), 0);

    let mut player = scene
        .rig
        .join([20.0, f64::from(FLOOR) + 12.0, 20.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 10);
    assert!(
        !scene
            .rig
            .server
            .world()
            .grids()
            .get(ship_id(0))
            .unwrap()
            .frozen
    );
    assert!(
        light_beside_the_lamp(&scene) > 0,
        "the light was made when the ship was let go"
    );
}
