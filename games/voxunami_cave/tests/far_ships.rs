//! Ships that are far away in Cave: told of with no brick and drawn from the nodes the server makes of them (`docs/specs/far-nodes-spec.md`, F5), in the ground of Cave's own
//! blocks and plugins; and a ship that has gone into its file is drawn as it was.

mod common;

use std::collections::HashSet;

use common::{Player, PlayerExt, Scene, make_ship, pose_at, ship_id, view_after};
use game_shell::GameDefinition;
use game_shell::files::{open_world, save_world};
use region_octree::BrickCube;
use server::ShipSettings;
use voxel_space::{GridId, WorldPosition};
use voxunami_cave::game::Cave;
use world_view::MeshUpdate;

/// A stone ship, 40 long, 8 high and 24 deep, held in the air 1,900 voxels along x from where the player is.
fn scene_with_a_ship_far_away() -> Scene {
    let mut scene = Scene::new();
    make_ship(
        &mut scene,
        pose_at(1936.0, 150.0, 16.0),
        true,
        [40, 8, 24],
        |_| true,
    );
    scene
}

fn join_looking_at_far_ships(scene: &mut Scene) -> Player {
    let mut player = scene.rig.join([16.0, 140.0, 16.0], 0.0, 0.0);
    player.enable_grid_detail(Cave::grid_detail().expect("Cave draws planets in detail"));
    player
}

/// What the renderer ends up holding of a ship: its bricks, and its nodes.
#[derive(Debug, Default)]
struct Holding {
    bricks: HashSet<voxel_space::BrickCoordinate>,
    nodes: HashSet<BrickCube>,
}

impl Holding {
    fn apply(&mut self, ship: GridId, updates: Vec<MeshUpdate>) {
        for update in updates {
            match update {
                MeshUpdate::UploadGridBrick {
                    grid,
                    coordinate,
                    mesh,
                } if grid == ship => {
                    if mesh.is_empty() {
                        self.bricks.remove(&coordinate);
                    } else {
                        self.bricks.insert(coordinate);
                    }
                }
                MeshUpdate::RemoveGridBrick { grid, coordinate } if grid == ship => {
                    self.bricks.remove(&coordinate);
                }
                MeshUpdate::UploadGridCoarse { grid, cube, mesh } if grid == ship => {
                    if mesh.is_empty() {
                        self.nodes.remove(&cube);
                    } else {
                        self.nodes.insert(cube);
                    }
                }
                MeshUpdate::RemoveGridCoarse { grid, cube } if grid == ship => {
                    self.nodes.remove(&cube);
                }
                MeshUpdate::RemoveGrid(grid) if grid == ship => {
                    self.bricks.clear();
                    self.nodes.clear();
                }
                _ => {}
            }
        }
    }
}

fn look(scene: &mut Scene, player: &mut Player, holding: &mut Holding, ship: GridId) {
    holding.apply(ship, view_after(scene, player));
}

#[test]
fn cave_tells_of_ships_that_are_far_away() {
    assert!(Cave::far_ships().is_some());
}

#[test]
fn a_ship_far_away_is_drawn_from_its_nodes_in_cave_and_in_no_brick() {
    let mut scene = scene_with_a_ship_far_away();
    let ship = ship_id(0);
    let mut player = join_looking_at_far_ships(&mut scene);
    let mut holding = Holding::default();
    look(&mut scene, &mut player, &mut holding, ship);
    let held = player
        .client()
        .world()
        .grids()
        .get(ship)
        .expect("it is told of");
    assert!(held.far.is_some(), "told of as far");
    assert_eq!(held.voxels.brick_count(), 0);
    assert!(holding.bricks.is_empty());
    assert!(!holding.nodes.is_empty());
}

#[test]
fn a_ship_the_player_flies_to_is_drawn_in_bricks_and_in_nodes_again_when_the_player_goes_away() {
    let mut scene = scene_with_a_ship_far_away();
    let ship = ship_id(0);
    let mut player = join_looking_at_far_ships(&mut scene);
    let mut holding = Holding::default();
    look(&mut scene, &mut player, &mut holding, ship);
    let far_nodes = holding.nodes.clone();
    assert!(!far_nodes.is_empty());

    // Beside the ship.
    player.camera.position = WorldPosition::from_voxels([1936.0 + 20.0, 140.0, 16.0]);
    player.teleport_to_camera();
    look(&mut scene, &mut player, &mut holding, ship);
    assert!(!holding.bricks.is_empty(), "its bricks");
    assert!(holding.nodes.is_empty(), "and none of its nodes");
    assert!(
        player
            .client()
            .world()
            .grids()
            .get(ship)
            .unwrap()
            .far
            .is_none()
    );

    // Away again: the nodes as before, and no brick.
    player.camera.position = WorldPosition::from_voxels([16.0, 140.0, 16.0]);
    player.teleport_to_camera();
    look(&mut scene, &mut player, &mut holding, ship);
    assert!(holding.bricks.is_empty());
    assert_eq!(holding.nodes, far_nodes);
}

#[test]
fn a_ship_that_has_gone_into_its_file_is_drawn_to_a_player_who_comes_after_as_it_was() {
    let mut scene = scene_with_a_ship_far_away();
    let ship = ship_id(0);
    let mut first = join_looking_at_far_ships(&mut scene);
    let mut seen = Holding::default();
    look(&mut scene, &mut first, &mut seen, ship);
    let before = seen.nodes.clone();
    assert!(!before.is_empty());
    // The ship has had its nodes made, and is put in its file when no one is near it (the other tests of the game keep ships where they are). A ship that is put away before
    // its nodes are made has no far view: they are made from its bricks, which are out of memory.
    scene.rig.server.set_ship_settings(ShipSettings::default());
    for _ in 0..40 {
        scene.rig.step();
    }
    assert!(
        scene.rig.server.world().grids().get(ship).is_none()
            && scene.rig.server.world().grids().get_parked(ship).is_some(),
        "the ship is in its file"
    );
    look(&mut scene, &mut first, &mut seen, ship);
    assert_eq!(seen.nodes, before, "the picture did not change");

    let mut second = join_looking_at_far_ships(&mut scene);
    let mut holding = Holding::default();
    look(&mut scene, &mut second, &mut holding, ship);
    assert_eq!(holding.nodes, before);
    assert!(holding.bricks.is_empty());
}

#[test]
fn a_ship_in_a_world_that_was_saved_and_loaded_is_drawn_from_afar_before_anyone_goes_near_it() {
    let directory =
        std::env::temp_dir().join(format!("voxunami_cave_far_ships_{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("world.voxunami");
    let ship = ship_id(0);

    // The game is played with the ship in memory, and closed.
    let mut scene = scene_with_a_ship_far_away();
    let mut player = join_looking_at_far_ships(&mut scene);
    let mut seen = Holding::default();
    look(&mut scene, &mut player, &mut seen, ship);
    let before = seen.nodes.clone();
    assert!(!before.is_empty());
    drop(player);
    scene.rig.step();
    let common::Rig {
        content, server, ..
    } = scene.rig;
    let mut world = server.into_world();
    save_world(&content, &mut world, &path).unwrap();

    // A new run: every ship is in its file, the player is far from it, and it is drawn as it was.
    let mut content = voxunami_cave::content::CaveContent::new();
    let loaded = open_world(&mut content, Some(&path), 1).unwrap();
    assert!(loaded.grids().get(ship).is_none() && loaded.grids().get_parked(ship).is_some());
    let plugins = Cave::plugins(&content);
    let mut rig = common::Rig::with_plugins_on_disk(
        content,
        loaded,
        plugins,
        world_storage::MemoryShipFiles::new(),
        Box::new(world_storage::DirectoryShipFiles::beside(&path)),
    );
    // (The rig brings ships in wherever they are; a ship is in its file when no one is near it.)
    rig.server.set_ship_settings(ShipSettings::default());
    let mut later = rig.join([16.0, 140.0, 16.0], 0.0, 0.0);
    later.enable_grid_detail(Cave::grid_detail().unwrap());
    let mut holding = Holding::default();
    for _ in 0..6 {
        let updates = {
            let server = &mut rig.server;
            later
                .settle(|| {
                    server.step();
                })
                .unwrap()
        };
        holding.apply(ship, updates);
    }
    assert!(rig.server.world().grids().get(ship).is_none());
    assert_eq!(holding.nodes, before);
    assert!(holding.bricks.is_empty());
    std::fs::remove_dir_all(&directory).unwrap();
}
