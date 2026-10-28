//! The terrain of a level drawn beyond its bricks, as the game says for each level (`GameDefinition::level_detail`): in Cave the level of space is drawn in coarse nodes out
//! past the size of the universe (so the great rivers are seen from anywhere), and the overworld, which the game does not draw so, is bricks only. A player who goes
//! from one to the other has the view of each.

mod common;

use common::{Player, Scene, say, tick_and_follow, view_after};
use game_shell::GameDefinition;
use game_shell::view::{apply_updates, atmosphere, configure_renderer};
use renderer::{GraphicsContext, VoxelRenderer};
use voxunami_cave::Cave;
use world_view::MeshUpdate;

/// The coarse nodes the renderer holds, as the updates say (an empty mesh takes a node down, as a removal does).
#[derive(Default)]
struct Nodes(std::collections::HashSet<region_octree::BrickCube>);

impl Nodes {
    fn apply(&mut self, updates: &[MeshUpdate]) {
        for update in updates {
            match update {
                MeshUpdate::UploadCoarse { cube, mesh } if !mesh.is_empty() => {
                    self.0.insert(*cube);
                }
                MeshUpdate::UploadCoarse { cube, .. } | MeshUpdate::RemoveCoarse(cube) => {
                    self.0.remove(cube);
                }
                _ => {}
            }
        }
    }
}

fn coarse_uploads(updates: &[MeshUpdate]) -> usize {
    updates
        .iter()
        .filter(
            |update| matches!(update, MeshUpdate::UploadCoarse { mesh, .. } if !mesh.is_empty()),
        )
        .count()
}

fn scene_with_a_player() -> (Scene, Player) {
    let mut scene = Scene::in_a_new_world();
    let mut player = scene
        .rig
        .join_walking(Cave::default_camera().position, 0.0, 0.0);
    player.enable_level_detail();
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 5);
    player.take_command_replies();
    (scene, player)
}

#[test]
fn the_level_of_space_is_drawn_in_nodes_and_the_overworld_in_bricks_only() {
    let (mut scene, mut player) = scene_with_a_player();
    let mut renderer = Nodes::default();
    // At the narrowest part of the river, 320 metres from it: the thread is in the nodes of the finest levels.
    say(
        &mut scene,
        &mut player,
        "/tp base_blocks:great_river_narrowest",
    );
    let updates = view_after(&mut scene, &mut player);
    renderer.apply(&updates);
    assert!(
        player.draws_coarse_terrain(),
        "the level of space is drawn in nodes"
    );
    assert!(coarse_uploads(&updates) > 0, "nodes of the river are drawn");
    assert!(!renderer.0.is_empty());

    // The overworld is not: bricks, no node is drawn there, and the nodes of the river are taken down (the renderer is told: it holds none).
    say(&mut scene, &mut player, "/level go overworld");
    let updates = view_after(&mut scene, &mut player);
    renderer.apply(&updates);
    assert!(
        !player.draws_coarse_terrain(),
        "the overworld is drawn in bricks"
    );
    assert_eq!(
        coarse_uploads(&updates),
        0,
        "no node of the river is drawn in the overworld"
    );
    assert!(
        renderer.0.is_empty(),
        "{} nodes of the river are left up in the overworld",
        renderer.0.len()
    );

    // And back: the nodes again.
    say(
        &mut scene,
        &mut player,
        "/tp base_blocks:great_river_narrowest",
    );
    let updates = view_after(&mut scene, &mut player);
    renderer.apply(&updates);
    assert!(player.draws_coarse_terrain());
    assert!(coarse_uploads(&updates) > 0);
    assert!(!renderer.0.is_empty());
}

#[test]
fn from_the_widest_part_the_river_is_drawn_too() {
    let (mut scene, mut player) = scene_with_a_player();
    // 1.4 x 10^25 voxels from the middle of the widest tube, which is a billion light years across: nodes of 2^70 voxels and more.
    say(
        &mut scene,
        &mut player,
        "/tp base_blocks:great_river_widest",
    );
    let updates = view_after(&mut scene, &mut player);
    assert!(player.draws_coarse_terrain());
    assert!(
        coarse_uploads(&updates) > 0,
        "the widest part is drawn from outside"
    );
}

#[test]
fn cave_draws_the_level_of_space_in_nodes_out_past_the_universe_and_no_other_level() {
    assert_eq!(Cave::coarse_detail(), None);
    let overworld: namespaced_id::NamespacedId = "voxunami_cave:overworld".parse().unwrap();
    assert_eq!(Cave::level_detail(&overworld), None);
    let space: namespaced_id::NamespacedId = "voxunami_cave:mini_cosmos".parse().unwrap();
    let detail = Cave::level_detail(&space).expect("the level of space is drawn in nodes");
    // Out past the diameter of the universe (93 billion light years is 8.8 x 10^26 metres), from anywhere in it.
    assert!(
        detail.maximum_distance > 1.8e27,
        "{}",
        detail.maximum_distance
    );
}

/// The pixel at the middle of a picture, as red, green and blue.
fn middle_and_corner(rgba: &[u8], width: u32, height: u32) -> ([u8; 3], [u8; 3]) {
    let at = |x: u32, y: u32| {
        let start = ((y * width + x) * 4) as usize;
        [rgba[start], rgba[start + 1], rgba[start + 2]]
    };
    (at(width / 2, height / 2), at(2, 2))
}

/// From 1.4 x 10^25 voxels out, at the widest part of the river (a billion light years across), the whole of it is drawn on every adapter: the nodes are 2^70 voxels and more
/// across, and the camera is a hundred orders of magnitude out of the range of an `f32` squared. The middle of the picture is the water, and the corner is the sky.
#[test]
fn the_widest_part_of_the_river_is_drawn_from_outside_on_every_graphics_adapter() {
    let contexts = GraphicsContext::all_physical_adapters();
    if contexts.is_empty() {
        assert!(
            std::env::var_os("VOX_REQUIRE_GPU").is_none(),
            "VOX_REQUIRE_GPU is set but no graphics adapter is available"
        );
        eprintln!("no graphics adapter available; skipping GPU test");
        return;
    }
    let (mut scene, mut player) = scene_with_a_player();
    say(
        &mut scene,
        &mut player,
        "/tp base_blocks:great_river_widest",
    );
    let updates = view_after(&mut scene, &mut player);
    assert!(coarse_uploads(&updates) > 0);
    let eyes = common::player_pose(&scene, 1).position;
    player.camera.position = eyes;
    // (Looking along -z, as the player was put to look: the knot is straight ahead unless the place back along the view was in the water.)
    player.camera.yaw = 0.0;
    player.camera.pitch = 0.0;
    for context in contexts {
        let name = renderer::describe(&context.adapter);
        let mut renderer = VoxelRenderer::new(&context, wgpu::TextureFormat::Rgba8UnormSrgb);
        configure_renderer(
            &mut renderer,
            player.block_table(),
            player.package().ambient_light,
            player.fog_radius_bricks(),
        );
        // A dark sky, so that the water is told from it, and a lit day, as the pictures of terrain have (the game's own ambient light is none).
        renderer.set_atmosphere({
            let mut atmosphere = atmosphere(player.fog_radius_bricks(), 160);
            atmosphere.sky_color = [0.02, 0.02, 0.05];
            atmosphere
        });
        apply_updates(&mut renderer, updates.clone());
        let (rgba, _) = renderer.render_to_rgba(&player.camera, 160, 90);
        let (middle, corner) = middle_and_corner(&rgba, 160, 90);
        assert!(
            middle[2] > 100 && middle[2] > middle[0] && middle[2] > middle[1],
            "{name}: the middle is {middle:?}, which is not the blue of the water"
        );
        assert!(
            corner[2] < 60,
            "{name}: the corner is {corner:?}, which is not the dark sky"
        );
    }
}
