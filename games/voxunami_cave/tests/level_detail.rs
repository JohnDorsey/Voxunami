//! The terrain of a level drawn beyond its bricks, as the game says for each level (`GameDefinition::level_detail`): in Cave the level of space is drawn in coarse nodes out
//! past the size of the universe (so the great rivers are seen from anywhere), and the overworld out to the horizon of its hills (made from the hills without their caves, so
//! a node of it takes a third of a millisecond: `docs/specs/viewpoints-spec.md` 9p). A player who goes from one to the other has the view of each.

mod common;

use common::{Player, Scene, say, tick_and_follow, view_after};
use game_shell::GameDefinition;
use game_shell::view::{apply_updates, atmosphere, configure_renderer};
use renderer::{GraphicsContext, VoxelRenderer};
use voxunami_cave::Cave;
use world_view::MeshUpdate;

/// The coarse nodes the renderer draws, as the updates say (an empty mesh takes a node down, as a removal does, and a node that is held hidden is not drawn).
#[derive(Default)]
struct Nodes(std::collections::HashSet<region_octree::BrickCube>);

impl Nodes {
    fn apply(&mut self, updates: &[MeshUpdate]) {
        for update in updates {
            match update {
                MeshUpdate::UploadCoarse { cube, mesh } if !mesh.is_empty() => {
                    self.0.insert(*cube);
                }
                MeshUpdate::UploadCoarse { cube, .. }
                | MeshUpdate::RemoveCoarse(cube)
                | MeshUpdate::HideCoarse(cube) => {
                    self.0.remove(cube);
                }
                MeshUpdate::ShowCoarse(cube) => {
                    self.0.insert(*cube);
                }
                _ => {}
            }
        }
    }
}

/// The updates until the view has nothing more to say for a few rounds (the hills out to the horizon are some tens of thousands of nodes).
fn settled_view(scene: &mut Scene, player: &mut Player) -> Vec<MeshUpdate> {
    let mut updates = Vec::new();
    let mut quiet = 0;
    let began = std::time::Instant::now();
    while quiet < 5 {
        let more = view_after(scene, player);
        quiet = if more.is_empty() { quiet + 1 } else { 0 };
        updates.extend(more);
        assert!(
            began.elapsed() < std::time::Duration::from_secs(600),
            "the view does not settle"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    updates
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
fn the_level_of_space_and_the_overworld_are_each_drawn_in_nodes_beyond_their_bricks_and_the_nodes_of_one_are_not_left_up_in_the_other()
 {
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

    let river_nodes = renderer.0.clone();

    // The overworld is drawn in nodes as well, its own: the nodes of the river are taken down (the renderer is told), and the hills are drawn out past the bricks.
    // (A player keeps the place they had, which is over the pole of the planet, 63 kilometres from the hills: they are put on the hills.)
    say(&mut scene, &mut player, "/level go overworld");
    say(&mut scene, &mut player, "/tp 16 110 16");
    tick_and_follow(&mut scene, &mut player, 30);
    let updates = settled_view(&mut scene, &mut player);
    renderer.apply(&updates);
    assert!(
        player.draws_coarse_terrain(),
        "the overworld is drawn in nodes beyond its bricks"
    );
    assert!(
        coarse_uploads(&updates) > 0,
        "the hills of the overworld are drawn beyond the bricks"
    );
    assert!(
        river_nodes.is_disjoint(&renderer.0),
        "{} nodes of the river are left up in the overworld",
        river_nodes.intersection(&renderer.0).count()
    );
    // Nodes of more than one level: the near ones are fine and the far ones coarse.
    let levels: std::collections::BTreeSet<u8> =
        renderer.0.iter().map(|cube| cube.level()).collect();
    assert!(
        levels.len() >= 3,
        "the hills are drawn in nodes of levels {levels:?}"
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
fn cave_draws_the_level_of_space_in_nodes_out_past_the_universe_and_the_overworld_to_the_horizon_of_its_hills()
 {
    assert_eq!(Cave::coarse_detail(), None);
    let overworld: namespaced_id::NamespacedId = "voxunami_cave:overworld".parse().unwrap();
    let hills = Cave::level_detail(&overworld).expect("the overworld is drawn in nodes");
    assert!(
        (1.0e4..=1.0e6).contains(&hills.maximum_distance),
        "the hills are drawn out to {} voxels",
        hills.maximum_distance
    );
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
            player.package(),
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

#[test]
fn the_map_of_the_overworld_is_orthographic_and_is_nodes_of_one_level_over_the_box_where_the_map_of_space_is_perspective()
 {
    let (mut scene, mut player) = scene_with_a_player();
    let mut renderer = Nodes::default();
    // The map of the level of space, round the player's body: a perspective camera.
    player.show_map();
    assert!(!player.map_is_orthographic());
    assert!(!player.drawn_camera().is_orthographic());
    player.show_body();

    say(&mut scene, &mut player, "/level go overworld");
    say(&mut scene, &mut player, "/tp 16 110 16");
    tick_and_follow(&mut scene, &mut player, 30);
    renderer.apply(&settled_view(&mut scene, &mut player));
    let perspective_nodes = renderer.0.len();
    assert!(perspective_nodes > 5_000, "{perspective_nodes}");
    assert!(!player.drawn_camera().is_orthographic());

    // Out to a picture 11,000 voxels high: the map is orthographic, and the picture is made of nodes of one level, a few thousand.
    player.show_map();
    player.orbit_zoom(1_000.0);
    assert!(player.map_is_orthographic());
    let camera = player.drawn_camera();
    let renderer::Projection::Orthographic { height, depth } = camera.projection else {
        panic!(
            "the map of the overworld is orthographic: {:?}",
            camera.projection
        );
    };
    assert!((10_000.0..12_000.0).contains(&height), "{height}");
    assert!(
        depth > height,
        "the box reaches past the middle of the picture"
    );
    renderer.apply(&settled_view(&mut scene, &mut player));
    let mut by_level = std::collections::BTreeMap::<u8, usize>::new();
    for cube in &renderer.0 {
        *by_level.entry(cube.level()).or_default() += 1;
    }
    let total = renderer.0.len();
    // From so far a voxel is a tenth of a pixel, and there are no bricks: a node covers the brick the player is in.
    let focus_brick = voxel_space::BrickCoordinate::new(0, 3, 0);
    let covers_the_focus = |nodes: &Nodes| {
        nodes
            .0
            .iter()
            .any(|cube| region_octree::BrickCube::containing(focus_brick, cube.level()) == *cube)
    };
    assert!(covers_the_focus(&renderer), "no node covers the focus");
    let (level, count) = by_level
        .iter()
        .max_by_key(|(_, count)| **count)
        .map(|(level, count)| (*level, *count))
        .unwrap();
    // A pixel is 10 voxels, and the cells of a map are 3 to 6 pixels: 32 voxels, level 5.
    assert!((4..=5).contains(&level), "{by_level:?}");
    assert!(
        count * 10 >= total * 9,
        "nearly all of one level: {by_level:?}"
    );
    assert!(
        (500..6_000).contains(&total),
        "a map is a few thousand nodes: {by_level:?} against {perspective_nodes} for the view from the ground"
    );
    assert!(
        by_level.keys().all(|node_level| *node_level <= level),
        "no node is coarser than the level: {by_level:?}"
    );

    // Close in (a voxel five pixels or so) the ball of bricks is where the picture is looked at, the focus, and not where the eye is back from it: no node covers it.
    let height = player.orbit().unwrap().orthographic_height(&player.camera);
    player.orbit_zoom(200.0 / height);
    renderer.apply(&settled_view(&mut scene, &mut player));
    assert!(
        !covers_the_focus(&renderer),
        "a node covers the brick at the focus"
    );

    // Back in the player's eyes: perspective, and the view of the ground again.
    player.show_body();
    assert!(!player.drawn_camera().is_orthographic());
    renderer.apply(&settled_view(&mut scene, &mut player));
    assert!(renderer.0.len() > 5_000, "{}", renderer.0.len());
}

/// The map of the overworld is a picture of the hills seen from straight over (and aslant), on every adapter: the ground fills the picture from edge to edge (an orthographic camera
/// has no horizon), and it has the grass, the sun's light and the fog of none. `VOX_SAVE_IMAGES` names a directory to keep the pictures in.
#[test]
fn the_map_of_the_overworld_fills_the_picture_with_hills_on_every_graphics_adapter() {
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
    let mut updates = Vec::new();
    say(&mut scene, &mut player, "/level go overworld");
    say(&mut scene, &mut player, "/tp 16 110 16");
    tick_and_follow(&mut scene, &mut player, 30);
    updates.extend(settled_view(&mut scene, &mut player));
    player.show_map();
    player.orbit_zoom(300.0);
    // (Straight over, and a little aslant: the two ways a map is looked at.)
    for (name, elevation) in [("over", 1.5), ("aslant", 0.7)] {
        player.orbit_by(0.0, elevation - player.orbit().unwrap().elevation);
        updates.extend(settled_view(&mut scene, &mut player));
        let camera = player.drawn_camera();
        assert!(camera.is_orthographic());
        for context in &contexts {
            let adapter = renderer::describe(&context.adapter);
            let mut renderer = VoxelRenderer::new(context, wgpu::TextureFormat::Rgba8UnormSrgb);
            configure_renderer(
                &mut renderer,
                player.block_table(),
                player.package(),
                player.fog_radius_bricks(),
            );
            let (lit, occluders) = game_shell::view::lit_atmosphere(&player);
            renderer.set_sun_occluders(&occluders);
            renderer.set_atmosphere(lit);
            apply_updates(&mut renderer, updates.clone());
            let (rgba, statistics) = renderer.render_to_rgba(&camera, 320, 180);
            if let Some(directory) = std::env::var_os("VOX_SAVE_IMAGES") {
                let directory = std::path::PathBuf::from(directory);
                std::fs::create_dir_all(&directory).unwrap();
                let file = directory.join(format!(
                    "overworld_map_{name}_{}.png",
                    adapter
                        .chars()
                        .filter(|c| c.is_ascii_alphanumeric())
                        .collect::<String>()
                ));
                save_png(&file, 320, 180, &rgba);
            }
            // The ground is drawn all over the picture: grass green in nearly every pixel, and what is not is the shade of a slope or a block of stone.
            let green = rgba
                .chunks(4)
                .filter(|pixel| pixel[1] > pixel[0] && pixel[1] > pixel[2])
                .count();
            let pixels = 320 * 180;
            assert!(
                green * 100 >= pixels * 70,
                "{adapter} {name}: {green} of {pixels} pixels are grass ({} nodes drawn)",
                statistics.coarse_nodes_drawn
            );
            assert!(statistics.coarse_nodes_drawn > 100, "{adapter} {name}");
        }
    }
}

/// Keeps a picture in a file.
fn save_png(file: &std::path::Path, width: u32, height: u32, rgba: &[u8]) {
    let mut encoder = png::Encoder::new(
        std::io::BufWriter::new(std::fs::File::create(file).unwrap()),
        width,
        height,
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(rgba))
        .unwrap();
}
