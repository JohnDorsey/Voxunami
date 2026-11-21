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
    scene_with_a_ship_far_away_held(true)
}

/// The same, held still or let go (a ship that is let go falls, and is saved going down).
fn scene_with_a_ship_far_away_held(held: bool) -> Scene {
    let mut scene = Scene::new();
    make_ship(
        &mut scene,
        pose_at(1936.0, 150.0, 16.0),
        held,
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

/// A world saved with a ship far from the start in memory, at `path`; and what a player sees of the ship from the start.
fn save_a_world_with_a_ship_far_away(path: &std::path::Path, held: bool) -> HashSet<BrickCube> {
    let mut scene = scene_with_a_ship_far_away_held(held);
    let mut player = join_looking_at_far_ships(&mut scene);
    let mut seen = Holding::default();
    look(&mut scene, &mut player, &mut seen, ship_id(0));
    assert!(!seen.nodes.is_empty());
    drop(player);
    scene.rig.step();
    let common::Rig {
        content, server, ..
    } = scene.rig;
    let mut world = server.into_world();
    save_world(&content, &mut world, path).unwrap();
    seen.nodes
}

/// What a player who comes to the world at `path` with a fresh server sees of the ship, from the start, where the ship is in its file and stays there.
fn what_a_new_player_sees_of_the_ship_in_the_world_at(path: &std::path::Path) -> Holding {
    let ship = ship_id(0);
    let mut content = voxunami_cave::content::CaveContent::new();
    let loaded = open_world(&mut content, Some(path), 1).unwrap();
    assert!(loaded.grids().get(ship).is_none() && loaded.grids().get_parked(ship).is_some());
    let plugins = Cave::plugins(&content);
    let mut rig = common::Rig::with_plugins_on_disk(
        content,
        loaded,
        plugins,
        world_storage::MemoryShipFiles::new(),
        Box::new(world_storage::DirectoryShipFiles::beside(path)),
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
    holding
}

fn a_folder_for(name: &str) -> std::path::PathBuf {
    let directory =
        std::env::temp_dir().join(format!("voxunami_cave_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

#[test]
fn a_ship_in_a_world_that_was_saved_and_loaded_is_drawn_from_afar_before_anyone_goes_near_it() {
    let directory = a_folder_for("far_ships");
    let path = directory.join("world.voxunami");
    let before = save_a_world_with_a_ship_far_away(&path, true);
    let holding = what_a_new_player_sees_of_the_ship_in_the_world_at(&path);
    assert_eq!(holding.nodes, before);
    assert!(holding.bricks.is_empty());
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn a_world_saved_before_ships_were_kept_ready_is_made_ready_by_preparing_it_and_nothing_in_it_moves()
 {
    let directory = a_folder_for("prepare");
    let path = directory.join("world.voxunami");
    // (A ship that is let go: it fell while the world was played and was saved going down, and it must be where it was saved when the world is prepared.)
    let _ = save_a_world_with_a_ship_far_away(&path, false);
    let ship = ship_id(0);

    // The world as it was saved by a program from before: no far file, and no box of the ship's bricks in the world file.
    let ships_folder = world_storage::DirectoryShipFiles::beside(&path);
    for name in {
        let mut files = ships_folder.clone();
        world_storage::ShipFiles::far_files_mut(&mut files)
            .names()
            .unwrap()
    } {
        let mut files = ships_folder.clone();
        world_storage::ShipFiles::far_files_mut(&mut files)
            .remove(&name)
            .unwrap();
    }
    let mut content = voxunami_cave::content::CaveContent::new();
    let mut world = open_world(&mut content, Some(&path), 1).unwrap();
    world.grids_mut().get_parked_mut(ship).unwrap().extent = None;
    save_world(&content, &mut world, &path).unwrap();
    let (clock, pose) = (world.clock(), world.grids().get_parked(ship).unwrap().pose);
    {
        let mut content = voxunami_cave::content::CaveContent::new();
        let old = open_world(&mut content, Some(&path), 1).unwrap();
        assert_eq!(old.grids().get_parked(ship).unwrap().extent, None);
    }
    assert!(
        what_a_new_player_sees_of_the_ship_in_the_world_at(&path)
            .nodes
            .is_empty(),
        "nothing of the ship is seen of an old world"
    );

    // Prepared, it is ready.
    let report = game_shell::prepare::prepare_ships::<Cave>(&game_shell::prepare::PrepareOptions {
        world_file: path.clone(),
        packs: None,
    })
    .unwrap();
    assert_eq!(report.brought_in, 1);
    assert!(
        report.left_in_files.is_empty(),
        "{:?}",
        report.left_in_files
    );
    assert_eq!(report.far_files, 1);
    let holding = what_a_new_player_sees_of_the_ship_in_the_world_at(&path);
    assert!(!holding.nodes.is_empty(), "the ship is seen from afar");
    assert!(holding.bricks.is_empty());

    // And nothing in the world moved: the clock is as it was, and the ship is where it was.
    let mut content = voxunami_cave::content::CaveContent::new();
    let prepared = open_world(&mut content, Some(&path), 1).unwrap();
    assert_eq!(prepared.clock(), clock);
    let record = prepared.grids().get_parked(ship).unwrap();
    assert_eq!(record.pose, pose);
    assert!(record.extent.is_some());
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn preparing_a_world_that_is_not_there_is_an_error_and_makes_none() {
    let directory = a_folder_for("prepare_nothing");
    let path = directory.join("no_such_world.voxunami");
    let error = game_shell::prepare::prepare_ships::<Cave>(&game_shell::prepare::PrepareOptions {
        world_file: path.clone(),
        packs: None,
    })
    .unwrap_err();
    assert!(error.contains("no such world"), "{error}");
    assert!(!path.exists());
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn preparing_a_world_leaves_a_ship_that_waits_for_its_pilot_in_its_file_as_it_was() {
    let directory = a_folder_for("prepare_waiting");
    let path = directory.join("world.voxunami");
    let _ = save_a_world_with_a_ship_far_away(&path, true);
    let ship = ship_id(0);
    // The ship went away with its pilot: it is in its file, waiting for those aboard.
    let mut content = voxunami_cave::content::CaveContent::new();
    let mut world = open_world(&mut content, Some(&path), 1).unwrap();
    let mut files = world_storage::DirectoryShipFiles::beside(&path);
    let format = world
        .ship_format(&game_shell::GameContent::registries(&content))
        .unwrap();
    world.load_ship(ship, &files, &format).unwrap();
    world.park_ship(ship, true, &mut files, &format).unwrap();
    save_world(&content, &mut world, &path).unwrap();
    let record = world.grids().get_parked(ship).unwrap().clone();
    assert!(record.waits_for_those_aboard);

    let report = game_shell::prepare::prepare_ships::<Cave>(&game_shell::prepare::PrepareOptions {
        world_file: path.clone(),
        packs: None,
    })
    .unwrap();
    assert_eq!(report.brought_in, 0);
    assert_eq!(report.left_in_files.len(), 1);
    assert_eq!(report.left_in_files[0].0, ship);
    let mut content = voxunami_cave::content::CaveContent::new();
    let prepared = open_world(&mut content, Some(&path), 1).unwrap();
    let after = prepared.grids().get_parked(ship).unwrap();
    assert!(after.waits_for_those_aboard);
    assert_eq!(after.file, record.file, "its file was not written again");
    std::fs::remove_dir_all(&directory).unwrap();
}
