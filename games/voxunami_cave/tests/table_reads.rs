//! What the server's rules read of the block table, pinned from outside, through a server and a player, so that they stay as they are when the table
//! becomes one shared, replaceable thing (`hot-reload-plan.md`, step 5). Each test is of a read that a survey found no test depended on: the table of one
//! rule was given a changed property and the whole suite still passed.

mod common;

use common::{
    FLOOR, PlayerExt, Rig, Scene, eyes_in_the_world, flat_floor, ground_height, tick_and_follow,
};
use game_shell::GameContent;
use game_shell::view::camera_at;
use protocol::Message;
use server::GridView;
use voxel_space::{FaceDirection, GridId, LevelIndex, VoxelCoordinate};
use voxunami_cave::content::{CaveContent, dev_docking_port_id};
use voxunami_cave::messages::SetBlock;

/// Which face of a docking port is its front (the yellow face that docks) after a player who looks `yaw_degrees` round from north puts one down on the top of
/// the ground, and the horizontal way they were looking.
fn front_of_a_port_put_down_looking(yaw_degrees: f64) -> (FaceDirection, [f64; 2]) {
    let mut rig = Rig::new();
    let mut player = rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    let ground = ground_height(&player, 16, 16);
    let voxel = VoxelCoordinate::new(16, ground + 1, 16);
    player.camera = camera_at([19.5, ground as f64 + 4.0, 16.5], yaw_degrees, 0.0);
    player.teleport_to_camera();
    rig.settle(&mut [&mut player]);
    let forward = player.camera.forward();
    let request = SetBlock {
        grid: GridId::root(LevelIndex::MAIN),
        at: voxel,
        block: dev_docking_port_id(),
        state: 0,
        // The top of the ground is what was clicked.
        against: Some(FaceDirection::PositiveY),
    };
    player.send_game_message(SetBlock::kind(), request.encode());
    rig.settle(&mut [&mut player]);
    let content = &rig.content;
    let port = content.registries()[0]
        .1
        .raw_id_of(&dev_docking_port_id())
        .unwrap();
    let overrides = &rig.server.world().main().overrides;
    assert_eq!(
        overrides.get(content.block_channel(), voxel),
        port,
        "the port is there"
    );
    let orientation = overrides.get(content.orientation_channel(), voxel);
    let table = content.block_table();
    assert_eq!(
        table.axis_face(port, 0, orientation),
        Some(FaceDirection::PositiveY),
        "its top is up, away from the ground"
    );
    let front = table
        .front_face(port, 0, orientation)
        .expect("a docking port has a front");
    (front, [f64::from(forward.x), f64::from(forward.z)])
}

#[test]
fn a_docking_port_is_put_down_with_its_front_to_the_player_whichever_way_they_look() {
    // The front is the one of the four sides of the port's top that is the opposite of the way the player looks (it is on their side). A rule that left the port as
    // it is made, with its front in one place, gives three of the four looks the wrong side.
    let mut fronts = Vec::new();
    for yaw in [0.0, 90.0, 180.0, 270.0] {
        let (front, [look_x, look_z]) = front_of_a_port_put_down_looking(yaw);
        let wanted = if look_x.abs() > look_z.abs() {
            if look_x > 0.0 {
                FaceDirection::NegativeX
            } else {
                FaceDirection::PositiveX
            }
        } else if look_z > 0.0 {
            FaceDirection::NegativeZ
        } else {
            FaceDirection::PositiveZ
        };
        assert_eq!(
            front, wanted,
            "looking {yaw} degrees round, along ({look_x:.2}, {look_z:.2})"
        );
        fronts.push(front);
    }
    fronts.sort_by_key(|face| face.index());
    fronts.dedup();
    assert_eq!(fronts.len(), 4, "four looks, four different fronts");
}

/// How high a walker's eyes are in the world after they have fallen onto the block at the top of a floor, in this shape (state), and stood there.
fn eyes_over_a_block_of_state(state: u32) -> f64 {
    let mut scene = Scene::new();
    flat_floor(&mut scene);
    let content = CaveContent::new();
    scene.run(move |world| {
        let mut grid = world.grid(GridId::root(LevelIndex::MAIN)).unwrap();
        let at = VoxelCoordinate::new(30, 201, 30);
        grid.set(content.state_channel(), at, state).unwrap();
        grid.set(content.block_channel(), at, content.blocks.stone)
            .unwrap();
    });
    let mut player = scene.rig.join_walking([30.5, FLOOR + 6.0, 30.5], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 80);
    eyes_in_the_world(&scene, 1)[1]
}

#[test]
fn a_walker_stands_on_a_slab_half_a_voxel_lower_than_on_a_whole_block() {
    let on_whole = eyes_over_a_block_of_state(0);
    let on_bottom_slab = eyes_over_a_block_of_state(1);
    assert!(
        (on_whole - on_bottom_slab - 0.5).abs() < 0.01,
        "{on_whole} over a whole block and {on_bottom_slab} over a slab"
    );
}

#[test]
fn every_rule_that_reads_the_block_table_reads_the_one_table_the_server_shares_and_makes_none_of_its_own()
 {
    use game_shell::SharedPackage;
    use voxunami_cave::plugins::cave_plugins_sharing;

    let content = CaveContent::new();
    let package = SharedPackage::new(content.package_data()).unwrap();
    let table = package.table();
    assert_eq!((table.handle_count(), table.table_holders()), (1, 1));
    let (plugins, report) = cave_plugins_sharing(&content, &package);
    // Handles to the cell, which the parts that read the table as it is when they are asked keep: block editing, items (which hold the package), fire, light and the ship
    // commands (5); the two ways of docking, the one in the ship commands and the one in the plugin that moves ships (2); the walking bodies, which hold one to see that
    // it was replaced and one in their collisions (2); the ground that a ship lands on when its pilot comes back (1); and the plugin that moves ships (1). A part that made
    // a table of its own holds none of these, and the count is short of it.
    assert_eq!(table.handle_count(), 1 + 11, "handles");
    // Holders of the table itself, which the parts that work out from it keep as it was when they last looked: the collisions of the walking bodies and the ground that a
    // returning pilot's ship lands on (2). (The collisions of the plugin that moves ships are made when it is registered.)
    assert_eq!(table.table_holders(), 1 + 2, "holders of the table");
    drop((plugins, report));
    assert_eq!(
        (table.handle_count(), table.table_holders()),
        (1, 1),
        "nothing else keeps it"
    );
}
