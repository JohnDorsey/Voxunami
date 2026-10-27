//! Ships in play: what a player sees of one, aims at, breaks and builds on, wherever it is and however it is turned, and what the server
//! refuses. (The pictures of ships are drawn on every physical graphics adapter; those need a GPU, and without one they print a
//! notice and pass, unless `VOX_REQUIRE_GPU` is set.)

mod common;

use common::{Player, PlayerExt, Scene, VIEW_RADIUS, ground_height};
use game_shell::view::{apply_updates, atmosphere, camera_at, configure_renderer};
use protocol::Message;
use renderer::{Camera, GraphicsContext, VoxelRenderer};
use server::GridView;
use voxel_space::{
    GridId, GridPose, LatticeRotation, LevelIndex, Orientation, VoxelCoordinate, WorldPosition,
};
use voxunami_cave::content::CaveContent;
use voxunami_cave::game::{break_block, place_block, request_block_in};
use voxunami_cave::messages::SetBlock;

fn ship() -> GridId {
    GridId::new(GridId::FIRST_ALLOCATED)
}

/// The pose of a ship whose origin is this voxel of the world.
fn pose_at(voxel: [i128; 3], orientation: Orientation) -> GridPose {
    let (brick, local) = VoxelCoordinate::new(voxel[0], voxel[1], voxel[2]).split();
    GridPose::new(
        WorldPosition::new(brick, [local.x(), local.y(), local.z()].map(f32::from)),
        orientation,
    )
}

/// Makes a ship that is a cube of stone, three voxels along each edge, with its lowest corner at the ship's origin, held where it is.
fn make_a_cube(scene: &mut Scene, pose: GridPose) {
    let content = CaveContent::new();
    scene.run(move |world| {
        let grid = world.create_grid(LevelIndex::MAIN, pose).unwrap();
        // Held: these tests are of what is seen, aimed at and edited, and a ship hung in the air would fall.
        world.set_grid_held(grid, true).unwrap();
        let mut view = world.grid(grid).unwrap();
        for x in 0..3 {
            for y in 0..3 {
                for z in 0..3 {
                    view.set(
                        content.block_channel(),
                        VoxelCoordinate::new(x, y, z),
                        content.blocks.stone,
                    )
                    .unwrap();
                }
            }
        }
    });
}

fn looking(scene: &mut Scene, at: [f64; 3]) -> Player {
    let mut player = scene.rig.join(at, 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    player
}

fn block_in_the_ship(scene: &Scene, voxel: [i128; 3]) -> u32 {
    scene
        .rig
        .server
        .world()
        .grids()
        .get(ship())
        .expect("the ship exists")
        .voxels
        .get(
            scene.rig.content.block_channel(),
            VoxelCoordinate::new(voxel[0], voxel[1], voxel[2]),
        )
}

fn block_the_player_sees_in_the_ship(scene: &Scene, player: &Player, voxel: [i128; 3]) -> u32 {
    let _ = scene;
    player
        .client()
        .world()
        .grids()
        .get(ship())
        .expect("the player knows of the ship")
        .voxels
        .get(
            CaveContent::new().block_channel(),
            VoxelCoordinate::new(voxel[0], voxel[1], voxel[2]),
        )
}

#[test]
fn a_ship_is_aimed_at_in_its_own_coordinates_wherever_it_is_and_however_it_is_turned() {
    let half_turn = LatticeRotation::all()
        .into_iter()
        .find(|rotation| {
            rotation.apply([1, 0, 0]) == [-1, 0, 0]
                && rotation.apply([0, 1, 0]) == [0, 1, 0]
                && rotation.apply([0, 0, 1]) == [0, 0, -1]
        })
        .unwrap()
        .orientation();
    // (the ship's origin, its turn, where the player is, the voxel of the ship the crosshair is on and the one in front of it)
    let cases = [
        (
            "not turned",
            pose_at([20, 300, 16], Orientation::IDENTITY),
            [21.5, 301.5, 26.5],
            [1, 1, 2],
            [1, 1, 3],
        ),
        (
            // Turned half way round about the vertical axis, the ship's voxel (x, y, z) is the world's (23 - x, 300 + y, 19 - z).
            "turned half way round",
            pose_at([24, 300, 20], half_turn),
            [22.5, 301.5, 28.0],
            [1, 1, 0],
            [1, 1, -1],
        ),
        (
            "a long way from the origin",
            pose_at([-2_000_000, 300, 64_000], Orientation::IDENTITY),
            [-1_999_998.5, 301.5, 64_010.5],
            [1, 1, 2],
            [1, 1, 3],
        ),
    ];
    for (name, pose, camera, voxel, previous) in cases {
        let mut scene = Scene::new();
        let mut player = looking(&mut scene, camera);
        make_a_cube(&mut scene, pose);
        scene.rig.settle(&mut [&mut player]);
        assert_eq!(player.client().world().grids().len(), 1, "{name}");
        let hit = player
            .target()
            .unwrap_or_else(|| panic!("{name}: nothing aimed at"));
        assert_eq!(hit.grid, ship(), "{name}");
        assert_eq!(
            (
                [hit.voxel.x(), hit.voxel.y(), hit.voxel.z()],
                [hit.previous.x(), hit.previous.y(), hit.previous.z()]
            ),
            (voxel, previous),
            "{name}"
        );
        assert!(
            hit.distance > 5.0 && hit.distance < 9.0,
            "{name}: {}",
            hit.distance
        );

        // Breaking the block there changes the ship, and the player sees it change.
        let stone = CaveContent::new().blocks.stone;
        assert_eq!(block_in_the_ship(&scene, voxel), stone, "{name}");
        break_block(&mut player);
        scene.rig.settle(&mut [&mut player]);
        assert_eq!(
            block_in_the_ship(&scene, voxel),
            0,
            "{name}: broken on the server"
        );
        assert_eq!(
            block_the_player_sees_in_the_ship(&scene, &player, voxel),
            0,
            "{name}: broken for the player"
        );
        // The crosshair now reaches a deeper voxel, and building puts a block in front of it, in the same ship.
        let deeper = player
            .target()
            .unwrap_or_else(|| panic!("{name}: nothing aimed at after"));
        assert_eq!(deeper.grid, ship(), "{name}");
        place_block(&mut player, "base_blocks:stone");
        scene.rig.settle(&mut [&mut player]);
        assert_eq!(
            block_in_the_ship(
                &scene,
                [
                    deeper.previous.x(),
                    deeper.previous.y(),
                    deeper.previous.z()
                ]
            ),
            stone,
            "{name}: placed against the face that was aimed at"
        );
        // Nothing was written into the terrain meanwhile.
        assert_eq!(
            scene.rig.server.world().main().overrides.brick_count(),
            0,
            "{name}"
        );
    }
}

#[test]
fn the_server_refuses_edits_of_a_ship_out_of_reach_that_does_not_exist_or_is_of_another_level() {
    let mut scene = Scene::new();
    let mut player = looking(&mut scene, [21.5, 301.5, 26.5]);
    make_a_cube(&mut scene, pose_at([20, 300, 16], Orientation::IDENTITY));
    scene.rig.settle(&mut [&mut player]);
    let stone = CaveContent::new().blocks.stone;
    // In reach, it is accepted.
    request_block_in(
        &mut player,
        ship(),
        VoxelCoordinate::new(2, 2, 2),
        "base_blocks:air",
        0,
    );
    scene.rig.settle(&mut [&mut player]);
    assert_eq!(block_in_the_ship(&scene, [2, 2, 2]), 0);

    // Out of reach: the player flies off, and asks to break a block of the ship.
    player.camera = camera_at([21.5, 301.5, 60.5], 0.0, 0.0);
    player.teleport_to_camera();
    scene.rig.settle(&mut [&mut player]);
    request_block_in(
        &mut player,
        ship(),
        VoxelCoordinate::new(1, 1, 1),
        "base_blocks:air",
        0,
    );
    scene.rig.settle(&mut [&mut player]);
    assert_eq!(block_in_the_ship(&scene, [1, 1, 1]), stone, "out of reach");

    // Back in reach, a ship that does not exist, and the terrain of a level that is not the player's, are refused too, and
    // nothing is made of either.
    player.camera = camera_at([21.5, 301.5, 26.5], 0.0, 0.0);
    player.teleport_to_camera();
    scene.rig.settle(&mut [&mut player]);
    for grid in [
        GridId::new(GridId::FIRST_ALLOCATED + 40),
        GridId::root(LevelIndex::new(1)),
    ] {
        request_block_in(
            &mut player,
            grid,
            VoxelCoordinate::new(1, 1, 1),
            "base_blocks:stone",
            0,
        );
        scene.rig.settle(&mut [&mut player]);
    }
    assert_eq!(scene.rig.server.world().grids().len(), 1);
    assert_eq!(scene.rig.server.world().main().overrides.brick_count(), 0);
    // A request that names a voxel of a ship too far along it for the reach is refused (the check is on the turned, placed voxel).
    let far = VoxelCoordinate::new(1, 1, 40);
    let message = SetBlock {
        grid: ship(),
        at: far,
        block: "base_blocks:stone".parse().unwrap(),
        state: 0,
        against: None,
    };
    player.send_game_message(SetBlock::kind(), message.encode());
    scene.rig.settle(&mut [&mut player]);
    assert_eq!(block_in_the_ship(&scene, [1, 1, 40]), 0);
}

#[test]
fn a_lamp_put_on_a_ship_lights_the_ship_and_leaves_the_terrain_alone() {
    // The terrain lights a ship's neighbourhood by rules that see one grid: a lamp placed on a ship lights that ship.
    let mut scene = Scene::new();
    let mut player = looking(&mut scene, [21.5, 301.5, 26.5]);
    make_a_cube(&mut scene, pose_at([20, 300, 16], Orientation::IDENTITY));
    scene.rig.settle(&mut [&mut player]);
    let content = CaveContent::new();
    // A lamp on the face of the cube that the player looks at: in front of the voxel (1, 1, 2).
    request_block_in(
        &mut player,
        ship(),
        VoxelCoordinate::new(1, 1, 3),
        "base_blocks:lamp",
        0,
    );
    scene.ticks(30);
    scene.rig.settle(&mut [&mut player]);
    let lit = |at: [i128; 3]| {
        scene
            .rig
            .server
            .world()
            .grids()
            .get(ship())
            .unwrap()
            .voxels
            .get(
                content.light_channel(),
                VoxelCoordinate::new(at[0], at[1], at[2]),
            )
    };
    assert!(lit([1, 1, 4]) > 0, "air beside the lamp is lit");
    // The lamp's light is in the ship, and none of it is in the terrain at the same coordinates.
    assert!(
        scene.rig.server.world().main().overrides.brick_count() == 0,
        "no light was written into the terrain"
    );
}

// ---- pictures ----

const WIDTH: u32 = 160;
const HEIGHT: u32 = 90;

fn contexts() -> Vec<GraphicsContext> {
    let contexts = GraphicsContext::all_physical_adapters();
    if contexts.is_empty() {
        assert!(
            std::env::var_os("VOX_REQUIRE_GPU").is_none(),
            "VOX_REQUIRE_GPU is set but no graphics adapter is available"
        );
        eprintln!("no graphics adapter available; skipping GPU test");
    }
    contexts
}

/// A picture, once a frame after it repeats it exactly (see `pictures.rs` for why).
fn settled(renderer: &mut VoxelRenderer, camera: &Camera) -> Vec<u8> {
    let mut previous = renderer.render_to_rgba(camera, WIDTH, HEIGHT).0;
    for _ in 0..30 {
        let next = renderer.render_to_rgba(camera, WIDTH, HEIGHT).0;
        if next == previous {
            return next;
        }
        previous = next;
    }
    panic!("the picture never settled");
}

fn changed_pixels(first: &[u8], second: &[u8]) -> usize {
    first
        .chunks(4)
        .zip(second.chunks(4))
        .filter(|(a, b)| a != b)
        .count()
}

#[test]
fn a_ship_is_drawn_where_it_is_follows_it_when_it_moves_and_is_gone_when_it_is() {
    for context in contexts() {
        let name = renderer::describe(&context.adapter);
        let mut scene = Scene::new();
        let mut player = looking(&mut scene, [21.5, 301.5, 26.5]);
        let mut renderer = VoxelRenderer::new(&context, wgpu::TextureFormat::Rgba8UnormSrgb);
        configure_renderer(
            &mut renderer,
            player.block_table(),
            player.package().ambient_light,
            VIEW_RADIUS,
        );
        renderer.set_atmosphere(atmosphere(VIEW_RADIUS, 160));
        let camera = player.camera;
        // Nothing but sky.
        let mut updates = Vec::new();
        updates.extend(player.settle(|| scene.rig.step()).unwrap());
        apply_updates(&mut renderer, updates);
        let empty = settled(&mut renderer, &camera);

        // A ship appears in front of the player.
        make_a_cube(&mut scene, pose_at([20, 300, 16], Orientation::IDENTITY));
        let updates = player.settle(|| scene.rig.step()).unwrap();
        apply_updates(&mut renderer, updates);
        let with_the_ship = settled(&mut renderer, &camera);
        let drawn = changed_pixels(&empty, &with_the_ship);
        assert!(drawn > 200, "{name}: the ship covers {drawn} pixels");

        // It moves along: the picture changes, and the ship is still there.
        scene.run(|world| {
            world
                .set_grid_pose(ship(), pose_at([24, 300, 16], Orientation::IDENTITY))
                .unwrap();
        });
        let updates = player.settle(|| scene.rig.step()).unwrap();
        apply_updates(&mut renderer, updates);
        let moved = settled(&mut renderer, &camera);
        assert!(
            changed_pixels(&with_the_ship, &moved) > 100,
            "{name}: it did not move"
        );
        assert!(changed_pixels(&empty, &moved) > 200, "{name}: it went away");

        // It is edited: a block is taken out of it.
        scene.run(|world| {
            let content = CaveContent::new();
            let mut view = world.grid(ship()).unwrap();
            view.set(content.block_channel(), VoxelCoordinate::new(1, 1, 2), 0)
                .unwrap();
        });
        let updates = player.settle(|| scene.rig.step()).unwrap();
        apply_updates(&mut renderer, updates);
        let edited = settled(&mut renderer, &camera);
        assert!(
            changed_pixels(&moved, &edited) > 20,
            "{name}: the edit was not drawn"
        );

        // And it goes: the picture is the empty one again, to the pixel.
        scene.run(|world| {
            assert!(world.remove_grid(ship()).is_some());
        });
        let updates = player.settle(|| scene.rig.step()).unwrap();
        apply_updates(&mut renderer, updates);
        let gone = settled(&mut renderer, &camera);
        assert!(gone == empty, "{name}: the ship left something behind");
        assert_eq!(renderer.grid_brick_count(), 0);
    }
}

#[test]
fn a_ship_between_the_player_and_the_ground_is_aimed_at_first_and_the_ground_past_it_when_it_is_gone()
 {
    let mut scene = Scene::new();
    let mut player = scene.rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    // Looking straight down from 110 at ground whose top is at 103 (the ground is 102 high).
    let ground = ground_height(&player, 16, 16);
    let mut camera = camera_at([16.5, f64::from(ground as i32) + 7.0, 16.5], 0.0, 0.0);
    camera.look_by(0.0, -std::f32::consts::FRAC_PI_2);
    player.camera = camera;
    player.teleport_to_camera();
    scene.rig.settle(&mut [&mut player]);
    let ground_hit = player.target().expect("the ground is under the crosshair");
    assert_eq!(ground_hit.grid, GridId::MAIN);
    assert_eq!(ground_hit.voxel.y(), ground);

    // A ship hangs in the way, between the player and the ground: the crosshair is on it, whatever is behind.
    make_a_cube(
        &mut scene,
        pose_at([15, ground + 3, 15], Orientation::IDENTITY),
    );
    scene.rig.settle(&mut [&mut player]);
    let ship_hit = player.target().expect("the ship is under the crosshair");
    assert_eq!(ship_hit.grid, ship());
    assert!(ship_hit.distance < ground_hit.distance);
    // Moved out of the way, the ground is aimed at again; back, the ship is.
    scene.run(move |world| {
        world
            .set_grid_pose(ship(), pose_at([25, ground + 3, 15], Orientation::IDENTITY))
            .unwrap();
    });
    scene.rig.settle(&mut [&mut player]);
    assert_eq!(player.target().map(|hit| hit.grid), Some(GridId::MAIN));
    scene.run(move |world| {
        world
            .set_grid_pose(ship(), pose_at([15, ground + 3, 15], Orientation::IDENTITY))
            .unwrap();
    });
    scene.rig.settle(&mut [&mut player]);
    assert_eq!(player.target().map(|hit| hit.grid), Some(ship()));
    // Gone, it leaves the ground.
    scene.run(|world| {
        assert!(world.remove_grid(ship()).is_some());
    });
    scene.rig.settle(&mut [&mut player]);
    assert_eq!(player.target().map(|hit| hit.grid), Some(GridId::MAIN));
}
