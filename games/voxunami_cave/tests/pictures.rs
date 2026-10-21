//! What the player sees, drawn on every physical graphics adapter.
//!
//! These need a GPU. Without one they print a notice and pass, unless `VOX_REQUIRE_GPU` is set.

mod common;

use common::{PlayerExt, Rig, VIEW_RADIUS, above_ground};
use game_shell::Identity;
use game_shell::files::{open_recording, record_connection};
use game_shell::session::{Mode, Session};
use game_shell::view::{apply_updates, atmosphere, camera_at, configure_renderer};
use renderer::{Camera, GraphicsContext, VoxelRenderer};
use transport::Connection;
use voxunami_cave::game::{Cave, break_block, place_block_with_state};

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

/// A picture, once a frame after it repeats it exactly. The integrated AMD GPU of the development machine now and then leaves
/// a few pixels out of a picture in its first frames (seen: 15 pixels of the first frame of a fresh renderer, on DX12, in the
/// test that compares a live picture to a replayed one, which is the flake that was long unexplained), and never the NVIDIA card.
/// Pictures that are compared to the pixel are taken once two frames in a row agree.
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

/// These pictures are of terrain and blocks, which the game itself draws black where no light reaches
/// (there is no sky light yet), so they are drawn as a player who has set `.ambient` to a lit day would see them.
fn daylight(renderer: &mut VoxelRenderer) {
    renderer.set_atmosphere(atmosphere(VIEW_RADIUS, 160));
}

fn renderer_for(
    context: &GraphicsContext,
    session: &common::Player,
    updates: Vec<world_view::MeshUpdate>,
) -> VoxelRenderer {
    let mut renderer = VoxelRenderer::new(context, wgpu::TextureFormat::Rgba8UnormSrgb);
    configure_renderer(
        &mut renderer,
        session.block_table(),
        session.package().ambient_light,
        VIEW_RADIUS,
    );
    daylight(&mut renderer);
    apply_updates(&mut renderer, updates);
    renderer
}

#[test]
fn an_edit_changes_the_picture_and_a_replay_draws_exactly_what_the_player_saw() {
    let directory =
        std::env::temp_dir().join(format!("voxunami_cave_pictures_{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("session.voxreplay");

    // Play: look down at the ground, break a block, and remember the pictures before and after.
    let mut rig = Rig::new();
    let connection = rig.connector.connect().unwrap();
    let recorded = record_connection(connection, &path).unwrap();
    let mut player = rig.join_with(recorded, [16.5, 110.0, 16.5], 0.0, 0.0);
    let mut updates = {
        let server = &mut rig.server;
        player.settle(|| {
            server.step();
        })
    }
    .unwrap();
    player.camera = above_ground(&player, 16, 16, 3.0);
    player.teleport_to_camera();
    player.send_pose();
    {
        let server = &mut rig.server;
        updates.extend(
            player
                .settle(|| {
                    server.step();
                })
                .unwrap(),
        );
    }
    let before_updates = updates.clone();
    break_block(&mut player);
    let mut after_updates = Vec::new();
    {
        let server = &mut rig.server;
        after_updates.extend(
            player
                .settle(|| {
                    server.step();
                })
                .unwrap(),
        );
    }
    player.advance().unwrap();
    let final_camera = player.camera;
    let final_updates: Vec<_> = before_updates
        .iter()
        .chain(&after_updates)
        .cloned()
        .collect();
    drop(player);

    // Watch the recording to its end.
    let connection: Box<dyn Connection> = Box::new(open_recording(&path).unwrap());
    let mut replay = Session::<_, Cave>::connect(
        connection,
        &Identity::generate(),
        Mode::Replaying,
        VIEW_RADIUS,
        camera_at([0.0; 3], 0.0, 0.0, VIEW_RADIUS),
        || {},
    )
    .unwrap();
    while replay.advance_one_tick().is_ok() {}
    let replay_updates = replay.settle(|| {}).unwrap();
    assert_eq!(replay.camera.position, final_camera.position);

    for context in contexts() {
        let mut live_renderer = {
            // The live picture, from the meshes the live session produced.
            let mut renderer = VoxelRenderer::new(&context, wgpu::TextureFormat::Rgba8UnormSrgb);
            configure_renderer(
                &mut renderer,
                replay.block_table(),
                replay.package().ambient_light,
                VIEW_RADIUS,
            );
            daylight(&mut renderer);
            apply_updates(&mut renderer, final_updates.clone());
            renderer
        };
        let live_after = settled(&mut live_renderer, &final_camera);

        let mut replay_renderer = renderer_for(&context, &replay, replay_updates.clone());
        let replayed = settled(&mut replay_renderer, &replay.camera);
        assert!(
            replay_renderer
                .render_to_rgba(&replay.camera, WIDTH, HEIGHT)
                .1
                .quads_drawn
                > 0
        );
        let differing = replayed
            .chunks(4)
            .zip(live_after.chunks(4))
            .filter(|(replayed, live)| replayed != live)
            .count();
        assert!(
            differing == 0,
            "{}: {differing} pixels of the replayed picture differ from the live one",
            renderer::describe(&context.adapter)
        );

        // And breaking the block did change what is seen: the picture before it is different.
        let mut before_renderer = renderer_for(&context, &replay, before_updates.clone());
        let before = settled(&mut before_renderer, &final_camera);
        assert!(
            before != live_after,
            "breaking a block did not change the picture"
        );
    }
    std::fs::remove_dir_all(&directory).unwrap();
}

/// Looks down at the ground from three voxels up, places a stone block in `state` on the ground below
/// and returns what the picture was before and after, with the statistics of the second.
fn placing_a_block_in_a_state(
    context: &GraphicsContext,
    state: u32,
) -> (Vec<u8>, Vec<u8>, renderer::RenderStatistics) {
    let mut rig = Rig::new();
    let mut player = rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    let mut updates = {
        let server = &mut rig.server;
        player.settle(|| {
            server.step();
        })
    }
    .unwrap();
    player.camera = above_ground(&player, 16, 16, 3.0);
    player.teleport_to_camera();
    player.send_pose();
    {
        let server = &mut rig.server;
        updates.extend(
            player
                .settle(|| {
                    server.step();
                })
                .unwrap(),
        );
    }
    let mut renderer = renderer_for(context, &player, updates);
    let (before, _) = renderer.render_to_rgba(&player.camera, WIDTH, HEIGHT);

    place_block_with_state(&mut player, "base_blocks:stone", state);
    let updates = {
        let server = &mut rig.server;
        player.settle(|| {
            server.step();
        })
    }
    .unwrap();
    apply_updates(&mut renderer, updates);
    let (after, statistics) = renderer.render_to_rgba(&player.camera, WIDTH, HEIGHT);
    (before, after, statistics)
}

#[test]
fn a_slab_a_player_places_is_drawn_as_a_slab_and_a_whole_block_is_not() {
    for context in contexts() {
        let (before, whole, whole_statistics) = placing_a_block_in_a_state(&context, 0);
        let (_, slab, slab_statistics) = placing_a_block_in_a_state(&context, 1);
        let (_, stair, stair_statistics) = placing_a_block_in_a_state(&context, 3);
        assert_eq!(whole_statistics.detailed_quads_drawn, 0);
        assert!(slab_statistics.detailed_quads_drawn > 0);
        assert!(stair_statistics.detailed_quads_drawn > slab_statistics.detailed_quads_drawn);
        // Each placement changes the picture, and the three blocks look different from one another.
        assert!(before != whole && before != slab && before != stair);
        assert!(whole != slab && whole != stair && slab != stair);
    }
}

#[test]
fn a_replay_draws_exactly_what_a_walker_carried_by_a_ship_that_turns_saw() {
    let directory = std::env::temp_dir().join(format!(
        "voxunami_cave_aboard_pictures_{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("session.voxreplay");

    // A walker falls onto a deck that is held in the air, and rides it as it moves and turns, looking down at it a little.
    let mut scene = common::scene_with_a_deck();
    let connection = scene.rig.connector.connect().unwrap();
    let recorded = record_connection(connection, &path).unwrap();
    let mut player = scene.rig.join_walking_over(
        recorded,
        1,
        [20.0, common::FLOOR as f64 + 13.0, 24.0],
        0.0,
        -20.0,
    );
    let mut updates = Vec::new();
    let mut looking_in_the_ship: Vec<[f32; 3]> = Vec::new();
    // (Which way they look is theirs to say; they say it.)
    player.send_pose();
    let origin = common::ship_pose(&scene, 0).position;
    for tick in 0..140usize {
        scene.run(move |world| {
            let pose = voxel_space::GridPose::new(
                origin.translated([0.04 * tick as f32, 0.0, 0.0]),
                voxel_space::Orientation::from_yaw_pitch(0.01 * tick as f32, 0.0),
            );
            world.set_grid_pose(common::ship_id(0), pose).unwrap();
        });
        player.advance().unwrap();
        // (As the window does each frame.)
        updates.extend(player.update_view());
        // Once they are aboard (they land within 40 ticks), the view is carried with the ship: where they look in the ship's own axes
        // does not change as the ship turns.
        if tick >= 40 {
            let ship = common::ship_pose(&scene, 0);
            looking_in_the_ship.push(ship.direction_to_local(player.camera.forward().to_array()));
        }
    }
    {
        let server = &mut scene.rig.server;
        updates.extend(
            player
                .settle(|| {
                    server.step();
                })
                .unwrap(),
        );
    }
    player.advance().unwrap();
    assert_eq!(common::player_pose(&scene, 1).grid, common::ship_id(0));
    let final_camera = player.camera;
    // They look 20 degrees down, in the ship's axes, and the same way in them for the whole of the ride, though the ship turned a
    // radian and more in it.
    let pitch = -20.0f32.to_radians();
    for looking in &looking_in_the_ship {
        assert!((looking[1] - libm::sinf(pitch)).abs() < 1e-3, "{looking:?}");
        for axis in 0..3 {
            assert!(
                (looking[axis] - looking_in_the_ship[0][axis]).abs() < 1e-3,
                "{looking:?} against {:?}",
                looking_in_the_ship[0]
            );
        }
    }
    assert_ne!(
        final_camera.frame_rotation.to_array(),
        [0.0, 0.0, 0.0, 1.0],
        "the view is carried round with the ship"
    );
    drop(player);

    let connection: Box<dyn Connection> = Box::new(open_recording(&path).unwrap());
    let mut replay = Session::<_, Cave>::connect(
        connection,
        &Identity::generate(),
        Mode::Replaying,
        VIEW_RADIUS,
        camera_at([0.0; 3], 0.0, 0.0, VIEW_RADIUS),
        || {},
    )
    .unwrap();
    while replay.advance_one_tick().is_ok() {}
    let replay_updates = replay.settle(|| {}).unwrap();
    assert_eq!(replay.camera.position, final_camera.position);
    assert_eq!(replay.camera.frame_rotation, final_camera.frame_rotation);
    assert_eq!(
        (replay.camera.yaw, replay.camera.pitch),
        (final_camera.yaw, final_camera.pitch)
    );

    for context in contexts() {
        let mut live_renderer = {
            let mut renderer = VoxelRenderer::new(&context, wgpu::TextureFormat::Rgba8UnormSrgb);
            configure_renderer(
                &mut renderer,
                replay.block_table(),
                replay.package().ambient_light,
                VIEW_RADIUS,
            );
            daylight(&mut renderer);
            apply_updates(&mut renderer, updates.clone());
            renderer
        };
        let live = settled(&mut live_renderer, &final_camera);
        let mut replay_renderer = renderer_for(&context, &replay, replay_updates.clone());
        let replayed = settled(&mut replay_renderer, &replay.camera);
        // The picture is of something: the deck, in front of the camera, and the sky over it, so more than one colour is drawn.
        let (pixels, drawn) = replay_renderer.render_to_rgba(&replay.camera, WIDTH, HEIGHT);
        assert!(drawn.quads_drawn > 0);
        let colours: std::collections::BTreeSet<[u8; 4]> = pixels
            .chunks(4)
            .map(|pixel| [pixel[0], pixel[1], pixel[2], pixel[3]])
            .collect();
        assert!(colours.len() >= 2, "{} colours", colours.len());
        let differing = replayed
            .chunks(4)
            .zip(live.chunks(4))
            .filter(|(replayed, live)| replayed != live)
            .count();
        assert!(
            differing == 0,
            "{}: {differing} pixels of the replayed picture differ from the live one",
            renderer::describe(&context.adapter)
        );
    }
    std::fs::remove_dir_all(&directory).unwrap();
}

/// Lets the server tick `count` times with the player taking in what it says, as a window does each frame, and keeps the changes to what
/// is drawn.
fn step_drawing(
    scene: &mut common::Scene,
    player: &mut common::Player,
    updates: &mut Vec<world_view::MeshUpdate>,
    count: usize,
) {
    for _ in 0..count {
        scene.rig.step();
        player.advance().unwrap();
        updates.extend(player.update_view());
    }
}

/// Types a command and ticks twice, so that it has been done and its reply is in.
fn type_command(
    scene: &mut common::Scene,
    player: &mut common::Player,
    updates: &mut Vec<world_view::MeshUpdate>,
    command: &str,
) -> String {
    player.send_command(command);
    step_drawing(scene, player, updates, 2);
    player.take_command_replies().join(" | ")
}

#[test]
fn a_replay_draws_exactly_what_a_player_saw_who_made_a_ship_flew_it_turned_it_and_left_it() {
    let directory = std::env::temp_dir().join(format!(
        "voxunami_cave_flight_pictures_{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("session.voxreplay");

    // A raft built five above the floor, and a player looking up at it from the floor.
    let mut scene = common::Scene::new();
    common::flat_floor(&mut scene);
    common::build_raft(&mut scene, 20, common::FLOOR as i64 + 6, 22, 8);
    let connection = scene.rig.connector.connect().unwrap();
    let recorded = record_connection(connection, &path).unwrap();
    let mut player = scene.rig.join_walking_over(
        recorded,
        1,
        [24.0, common::FLOOR as f64 + 3.0, 34.0],
        0.0,
        30.0,
    );
    let mut updates = Vec::new();
    player.send_pose();
    step_drawing(&mut scene, &mut player, &mut updates, 20);

    // They make a ship of it and hold it, are put over it and fall aboard, and take the controls.
    let made = type_command(&mut scene, &mut player, &mut updates, "/ship create");
    assert!(made.starts_with("made ship 0 of 64 blocks"), "{made}");
    type_command(&mut scene, &mut player, &mut updates, "/ship hold on");
    player.send_command(&format!("/tp 24 {} 26", common::FLOOR as i64 + 12));
    step_drawing(&mut scene, &mut player, &mut updates, 60);
    assert_eq!(common::player_pose(&scene, 1).grid, common::ship_id(0));
    let piloting = type_command(&mut scene, &mut player, &mut updates, "/ship pilot");
    assert!(piloting.contains("you pilot ship 0"), "{piloting}");
    type_command(&mut scene, &mut player, &mut updates, "/ship hold off");

    // They fly it a little north, level, and let it come to rest.
    player.camera.look_by(0.0, -player.camera.pitch);
    player.hold(&[game_shell::KeyCode::KeyW]);
    step_drawing(&mut scene, &mut player, &mut updates, 8);
    player.hold(&[]);
    step_drawing(&mut scene, &mut player, &mut updates, 100);

    // It is turned: a push at the edge of it, off its middle, for half a second, and it goes on turning, with them carried.
    let before = common::ship_pose(&scene, 0).orientation;
    for _ in 0..10 {
        scene.run(|world| {
            world
                .apply_grid_force(
                    common::ship_id(0),
                    [0.0, 0.0, 400_000.0],
                    Some([7.5, 0.5, 4.0]),
                )
                .unwrap();
        });
        player.advance().unwrap();
        updates.extend(player.update_view());
    }
    step_drawing(&mut scene, &mut player, &mut updates, 40);
    assert_ne!(
        common::ship_pose(&scene, 0).orientation,
        before,
        "it turned"
    );
    assert_eq!(common::player_pose(&scene, 1).grid, common::ship_id(0));

    // They let go, stop the ship where it is (holding it stops its turning), and walk off the edge of it.
    type_command(&mut scene, &mut player, &mut updates, "/ship pilot");
    type_command(&mut scene, &mut player, &mut updates, "/ship hold on");
    player.hold(&[game_shell::KeyCode::KeyA]);
    step_drawing(&mut scene, &mut player, &mut updates, 100);
    player.hold(&[]);
    step_drawing(&mut scene, &mut player, &mut updates, 60);
    assert!(
        common::player_pose(&scene, 1).grid.root_level().is_some(),
        "they walked off the ship"
    );

    // They look back at it, and that is the picture.
    let centre = {
        let ship = scene
            .rig
            .server
            .world()
            .grids()
            .get(common::ship_id(0))
            .unwrap();
        voxunami_cave::aboard::in_voxels(
            ship.pose
                .local_to_world(voxel_space::WorldPosition::new(
                    voxel_space::BrickCoordinate::ORIGIN,
                    [4.0, 0.5, 4.0],
                ))
                .unwrap(),
        )
    };
    let eyes = common::voxels_of(&player.camera);
    let to_ship = [
        centre[0] - eyes[0],
        centre[1] - eyes[1],
        centre[2] - eyes[2],
    ];
    let across = libm::sqrt(to_ship[0] * to_ship[0] + to_ship[2] * to_ship[2]);
    // (Their view still has the turn of the room they left in it, so what they turn by is the difference of directions in the world.)
    let (wanted_yaw, wanted_pitch) = (
        libm::atan2(-to_ship[0], -to_ship[2]) as f32,
        libm::atan2(to_ship[1], across) as f32,
    );
    let forward = player.camera.forward().to_array();
    let (yaw, pitch) = (
        libm::atan2(-f64::from(forward[0]), -f64::from(forward[2])) as f32,
        libm::asin(f64::from(forward[1])) as f32,
    );
    player
        .camera
        .look_by(wanted_yaw - yaw, wanted_pitch - pitch);
    player.send_pose();
    step_drawing(&mut scene, &mut player, &mut updates, 5);
    {
        let server = &mut scene.rig.server;
        updates.extend(
            player
                .settle(|| {
                    server.step();
                })
                .unwrap(),
        );
    }
    player.advance().unwrap();
    let final_camera = player.camera;
    // The ship is in front of the camera: near the middle of the view.
    let forward = final_camera.forward().to_array();
    let length = libm::sqrt(to_ship.iter().map(|value| value * value).sum());
    let cosine = (0..3)
        .map(|axis| to_ship[axis] * f64::from(forward[axis]))
        .sum::<f64>()
        / length;
    assert!(cosine > 0.98, "the ship is not where they look: {cosine}");
    drop(player);

    let connection: Box<dyn Connection> = Box::new(open_recording(&path).unwrap());
    let mut replay = Session::<_, Cave>::connect(
        connection,
        &Identity::generate(),
        Mode::Replaying,
        VIEW_RADIUS,
        camera_at([0.0; 3], 0.0, 0.0, VIEW_RADIUS),
        || {},
    )
    .unwrap();
    while replay.advance_one_tick().is_ok() {}
    let replay_updates = replay.settle(|| {}).unwrap();
    assert_eq!(replay.camera.position, final_camera.position);
    assert_eq!(
        (replay.camera.yaw, replay.camera.pitch),
        (final_camera.yaw, final_camera.pitch)
    );

    for context in contexts() {
        let mut live_renderer = {
            let mut renderer = VoxelRenderer::new(&context, wgpu::TextureFormat::Rgba8UnormSrgb);
            configure_renderer(
                &mut renderer,
                replay.block_table(),
                replay.package().ambient_light,
                VIEW_RADIUS,
            );
            daylight(&mut renderer);
            apply_updates(&mut renderer, updates.clone());
            renderer
        };
        let live = settled(&mut live_renderer, &final_camera);
        let mut replay_renderer = renderer_for(&context, &replay, replay_updates.clone());
        let replayed = settled(&mut replay_renderer, &replay.camera);
        let (pixels, drawn) = replay_renderer.render_to_rgba(&replay.camera, WIDTH, HEIGHT);
        assert!(drawn.quads_drawn > 0);
        let colours: std::collections::BTreeSet<[u8; 4]> = pixels
            .chunks(4)
            .map(|pixel| [pixel[0], pixel[1], pixel[2], pixel[3]])
            .collect();
        assert!(colours.len() >= 2, "{} colours", colours.len());
        let differing = replayed
            .chunks(4)
            .zip(live.chunks(4))
            .filter(|(replayed, live)| replayed != live)
            .count();
        assert!(
            differing == 0,
            "{}: {differing} pixels of the replayed picture differ from the live one",
            renderer::describe(&context.adapter)
        );
    }
    std::fs::remove_dir_all(&directory).unwrap();
}
