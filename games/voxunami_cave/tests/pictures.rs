//! What the player sees, drawn on every physical graphics adapter.
//!
//! These need a GPU. Without one they print a notice and pass, unless `VOX_REQUIRE_GPU` is set.

mod common;

use common::{Rig, VIEW_RADIUS, above_ground};
use game_shell::files::{open_recording, record_connection};
use game_shell::session::{Mode, Session};
use game_shell::view::{apply_updates, camera_at, configure_renderer};
use renderer::{GraphicsContext, VoxelRenderer};
use transport::Connection;
use voxunami_cave::game::{Cave, break_block};

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

fn renderer_for(
    context: &GraphicsContext,
    session: &common::Player,
    updates: Vec<world_view::MeshUpdate>,
) -> VoxelRenderer {
    let mut renderer = VoxelRenderer::new(context, wgpu::TextureFormat::Rgba8UnormSrgb);
    configure_renderer(&mut renderer, session.content(), VIEW_RADIUS);
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
            configure_renderer(&mut renderer, replay.content(), VIEW_RADIUS);
            apply_updates(&mut renderer, final_updates.clone());
            renderer
        };
        let (live_after, _) = live_renderer.render_to_rgba(&final_camera, WIDTH, HEIGHT);

        let mut replay_renderer = renderer_for(&context, &replay, replay_updates.clone());
        let (replayed, statistics) = replay_renderer.render_to_rgba(&replay.camera, WIDTH, HEIGHT);
        assert!(statistics.quads_drawn > 0);
        assert!(
            replayed == live_after,
            "the replayed picture differs from the live one"
        );

        // And breaking the block did change what is seen: the picture before it is different.
        let mut before_renderer = renderer_for(&context, &replay, before_updates.clone());
        let (before, _) = before_renderer.render_to_rgba(&final_camera, WIDTH, HEIGHT);
        assert!(
            before != live_after,
            "breaking a block did not change the picture"
        );
    }
    std::fs::remove_dir_all(&directory).unwrap();
}
