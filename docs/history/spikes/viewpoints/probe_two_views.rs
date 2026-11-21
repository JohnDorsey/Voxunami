//! A probe of 2026-11-21 (`docs/specs/viewpoints-spec.md` section 3), kept as a record and not compiled with the workspace: what the view of a planet holds from the surface and from far away, how much the two share, what each takes in memory,
//! To run it, copy it to `games/voxunami_cave/tests/`, then `cargo test --release -p voxunami_cave --test probe_two_views -- --nocapture --test-threads=1`, and remove the copy (the code is of commit `aa419ee`).
//! and what going from one to the other and back costs today (nodes that leave the view are thrown away).

mod common;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant};

use common::{Scene, say, tick_and_follow};
use game_shell::GameDefinition;
use region_octree::BrickCube;
use voxel_space::{BrickCoordinate, GridId};
use voxunami_cave::Cave;
use world_view::MeshUpdate;

/// `None` is the terrain of the level, `Some` a planet or ship.
type Key = (Option<GridId>, BrickCube);

/// The planet's radius is 63,710 voxels and its hills reach about 320 above that: this is about 490 voxels over the middle of the hills.
const SURFACE_Y: f64 = 64_200.0;
const BRICK_Y_OF_SURFACE: i128 = 2006;
const QUIET: Duration = Duration::from_millis(2500);

#[derive(Default, Clone)]
struct Screen {
    /// Quads and detailed quads of every node and brick that has something to see.
    held: HashMap<Key, (usize, usize)>,
}

impl Screen {
    fn put(&mut self, key: Key, quads: usize, detailed: usize) {
        if quads + detailed == 0 {
            self.held.remove(&key);
        } else {
            self.held.insert(key, (quads, detailed));
        }
    }

    fn brick(coordinate: BrickCoordinate) -> BrickCube {
        BrickCube::new(coordinate, 0).expect("a brick is a cube")
    }

    fn apply(&mut self, updates: Vec<MeshUpdate>) {
        for update in updates {
            match update {
                MeshUpdate::Upload { coordinate, mesh } => self.put(
                    (None, Self::brick(coordinate)),
                    mesh.quads.len(),
                    mesh.detailed.len(),
                ),
                MeshUpdate::Remove(coordinate) => {
                    self.held.remove(&(None, Self::brick(coordinate)));
                }
                MeshUpdate::UploadGridBrick {
                    grid,
                    coordinate,
                    mesh,
                } => self.put(
                    (Some(grid), Self::brick(coordinate)),
                    mesh.quads.len(),
                    mesh.detailed.len(),
                ),
                MeshUpdate::RemoveGridBrick { grid, coordinate } => {
                    self.held.remove(&(Some(grid), Self::brick(coordinate)));
                }
                MeshUpdate::UploadCoarse { cube, mesh } => {
                    self.put((None, cube), mesh.quads.len(), mesh.detailed.len())
                }
                MeshUpdate::RemoveCoarse(cube) => {
                    self.held.remove(&(None, cube));
                }
                MeshUpdate::UploadGridCoarse { grid, cube, mesh } => {
                    self.put((Some(grid), cube), mesh.quads.len(), mesh.detailed.len())
                }
                MeshUpdate::RemoveGridCoarse { grid, cube } => {
                    self.held.remove(&(Some(grid), cube));
                }
                MeshUpdate::RemoveGrid(grid) => self.held.retain(|(held, _), _| *held != Some(grid)),
                MeshUpdate::GridPose { .. } | MeshUpdate::GridIsGround(_) => {}
            }
        }
    }

    fn bytes(&self) -> usize {
        self.held
            .values()
            .map(|(quads, detailed)| quads * 8 + detailed * 16)
            .sum()
    }

    fn quads(&self) -> usize {
        self.held.values().map(|(quads, detailed)| quads + detailed).sum()
    }

    fn per_level(&self) -> BTreeMap<u8, (usize, usize)> {
        let mut levels: BTreeMap<u8, (usize, usize)> = BTreeMap::new();
        for ((_, cube), (quads, detailed)) in &self.held {
            let entry = levels.entry(cube.level()).or_default();
            entry.0 += 1;
            entry.1 += (quads * 8 + detailed * 16) as usize;
        }
        levels
    }

    /// How many held cubes have a cube that holds them held too (the view's own rule says none).
    fn overlaps(&self) -> usize {
        self.held
            .keys()
            .filter(|(grid, cube)| {
                (cube.level() + 1..=region_octree::ROOT_LEVEL).any(|level| {
                    self.held
                        .contains_key(&(*grid, BrickCube::containing(cube.origin(), level)))
                })
            })
            .count()
    }

    fn extents(&self) -> String {
        let mut text = String::new();
        let mut by_level: BTreeMap<u8, ([i128; 3], [i128; 3])> = BTreeMap::new();
        for (_, cube) in self.held.keys() {
            let origin = cube.origin();
            let entry = by_level
                .entry(cube.level())
                .or_insert(([i128::MAX; 3], [i128::MIN; 3]));
            for (axis, value) in [origin.x(), origin.y(), origin.z()].into_iter().enumerate() {
                entry.0[axis] = entry.0[axis].min(value);
                entry.1[axis] = entry.1[axis].max(value);
            }
        }
        for (level, (low, high)) in by_level {
            text += &format!(" [level {level}: {low:?} .. {high:?}]");
        }
        text
    }

    fn summary(&self, name: &str) {
        let levels: Vec<String> = self
            .per_level()
            .iter()
            .map(|(level, (count, bytes))| format!("{level}:{count}/{}KB", bytes / 1024))
            .collect();
        let mut by_grid: BTreeMap<String, usize> = BTreeMap::new();
        for (grid, _) in self.held.keys() {
            *by_grid.entry(format!("{grid:?}")).or_default() += 1;
        }
        println!(
            "{name}: by grid {by_grid:?}; {} meshes (non-empty), {} quads, {:.2} MB; level:count/bytes {}",
            self.held.len(),
            self.quads(),
            self.bytes() as f64 / 1.0e6,
            levels.join(" ")
        );
        println!("   cubes drawn inside another drawn cube: {}", self.overlaps());
        println!("   extents (bricks, origins):{}", self.extents());
    }
}

struct Settled {
    seconds: f64,
    calls: usize,
    changing_calls: usize,
    most_in_a_call: usize,
    uploads: usize,
    removals: usize,
}

/// Runs the player's view until the camera has arrived at brick height `target_brick_y` and nothing has changed for `quiet` of wall-clock time, applying every update to `screen`.
fn settle_view(
    scene: &mut Scene,
    player: &mut common::Player,
    screen: &mut Screen,
    target_brick_y: i128,
    quiet: Duration,
) -> Settled {
    let began = Instant::now();
    let deadline = began + Duration::from_secs(900);
    let mut result = Settled {
        seconds: 0.0,
        calls: 0,
        changing_calls: 0,
        most_in_a_call: 0,
        uploads: 0,
        removals: 0,
    };
    let mut last_change = Instant::now();
    let mut last_report = Instant::now();
    let mut arrived_at: Option<Instant> = None;
    loop {
        assert!(Instant::now() < deadline, "gave up waiting for the view");
        tick_and_follow(scene, player, 1);
        let updates = player.update_view();
        result.calls += 1;
        let arrived = (player.camera.position.brick().y() - target_brick_y).abs() <= 2;
        if arrived && arrived_at.is_none() {
            arrived_at = Some(Instant::now());
            last_change = Instant::now();
        }
        if updates.is_empty() {
            std::thread::sleep(Duration::from_millis(2));
        } else {
            result.changing_calls += 1;
            result.most_in_a_call = result.most_in_a_call.max(updates.len());
            for update in &updates {
                match update {
                    MeshUpdate::Upload { .. }
                    | MeshUpdate::UploadGridBrick { .. }
                    | MeshUpdate::UploadCoarse { .. }
                    | MeshUpdate::UploadGridCoarse { .. } => result.uploads += 1,
                    MeshUpdate::Remove(_)
                    | MeshUpdate::RemoveGridBrick { .. }
                    | MeshUpdate::RemoveCoarse(_)
                    | MeshUpdate::RemoveGridCoarse { .. } => result.removals += 1,
                    _ => {}
                }
            }
            screen.apply(updates);
            last_change = Instant::now();
        }
        if arrived_at.is_some() && last_change.elapsed() >= quiet {
            break;
        }
        if last_report.elapsed() > Duration::from_secs(10) {
            last_report = Instant::now();
            println!(
                "   ... {:.0} s, {} calls, {} meshes held, camera brick y {}",
                began.elapsed().as_secs_f64(),
                result.calls,
                screen.held.len(),
                player.camera.position.brick().y()
            );
        }
    }
    println!("   camera at the end: {:?}, vertical field of view {}", player.camera.position, player.camera.vertical_field_of_view);
    // Seconds from the camera's arrival to the last change.
    result.seconds = arrived_at.map_or(0.0, |arrived| {
        last_change.saturating_duration_since(arrived).as_secs_f64()
    });
    result
}

fn relation(name_a: &str, a: &Screen, name_b: &str, b: &Screen) {
    let a_cubes: HashSet<Key> = a.held.keys().copied().collect();
    let b_cubes: HashSet<Key> = b.held.keys().copied().collect();
    // Every cube that holds one of A's, with the grid it is in.
    let mut a_holders: HashSet<Key> = HashSet::new();
    for &(grid, cube) in &a_cubes {
        for level in cube.level() + 1..=region_octree::ROOT_LEVEL {
            a_holders.insert((grid, BrickCube::containing(cube.origin(), level)));
        }
    }
    let mut b_holders: HashSet<Key> = HashSet::new();
    for &(grid, cube) in &b_cubes {
        for level in cube.level() + 1..=region_octree::ROOT_LEVEL {
            b_holders.insert((grid, BrickCube::containing(cube.origin(), level)));
        }
    }
    let mut same = 0usize;
    let mut same_bytes = 0usize;
    let mut b_over_a = 0usize; // B's cube holds some of A's (B is coarser there)
    let mut b_over_a_bytes = 0usize;
    let mut b_under_a = 0usize; // B's cube is inside one of A's (B is finer there)
    let mut b_under_a_bytes = 0usize;
    let mut b_apart = 0usize; // no relation: a place A does not draw (A pruned it or never reached it)
    let mut b_apart_bytes = 0usize;
    for &key in &b_cubes {
        let (quads, detailed) = b.held[&key];
        let bytes = quads * 8 + detailed * 16;
        if a_cubes.contains(&key) {
            same += 1;
            same_bytes += bytes;
        } else if a_holders.contains(&key) {
            b_over_a += 1;
            b_over_a_bytes += bytes;
        } else if (key.1.level() + 1..=region_octree::ROOT_LEVEL)
            .any(|level| a_cubes.contains(&(key.0, BrickCube::containing(key.1.origin(), level))))
        {
            b_under_a += 1;
            b_under_a_bytes += bytes;
        } else {
            b_apart += 1;
            b_apart_bytes += bytes;
        }
    }
    let _ = &b_holders;
    println!(
        "  {name_b} ({} meshes, {:.2} MB) against {name_a} ({} meshes, {:.2} MB): same cube {} ({:.2} MB); {name_b} coarser over {name_a}'s {} ({:.2} MB); {name_b} finer inside {name_a}'s {} ({:.2} MB); unrelated {} ({:.2} MB)",
        b.held.len(),
        b.bytes() as f64 / 1.0e6,
        a.held.len(),
        a.bytes() as f64 / 1.0e6,
        same,
        same_bytes as f64 / 1.0e6,
        b_over_a,
        b_over_a_bytes as f64 / 1.0e6,
        b_under_a,
        b_under_a_bytes as f64 / 1.0e6,
        b_apart,
        b_apart_bytes as f64 / 1.0e6,
    );
}

fn report(name: &str, settled: &Settled) {
    println!("   (camera y at the end: see next line)");
    println!(
        "{name}: settled after {:.2} s ({} calls, {} that changed something, at most {} updates in one call, {} uploads, {} removals)",
        settled.seconds, settled.calls, settled.changing_calls, settled.most_in_a_call, settled.uploads, settled.removals
    );
}

/// Stand where the server puts the player, and look at what is drawn; then walk a few bricks along x and look again.
#[test]
fn the_view_at_the_spawn_and_after_a_walk() {
    let mut scene = Scene::in_a_new_world();
    let mut player = scene
        .rig
        .join_walking(Cave::default_camera().position, 0.0, 0.0);
    player.enable_level_detail();
    player.enable_grid_detail(<Cave as GameDefinition>::grid_detail().expect("Cave draws planets in detail"));
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 5);
    player.take_command_replies();
    say(&mut scene, &mut player, "/fly");
    let mut screen = Screen::default();
    let brick_y = player.camera.position.brick().y();
    {
        let world = player.client().world();
        println!("at the spawn: grids known {}, watched level {:?}", world.grids().len(), world.watched_level().map(|(level, _)| level));
        for (id, ship) in world.grids().iter() {
            println!("   grid {id:?}: terrain {:?} far {:?}", ship.terrain.is_some(), ship.far.is_some());
        }
    }
    let at_spawn = settle_view(&mut scene, &mut player, &mut screen, brick_y, Duration::from_millis(3000));
    {
        let world = player.client().world();
        println!("after 3 s at the spawn: grids known {}", world.grids().len());
    }
    report("at the spawn", &at_spawn);
    screen.summary("spawn");
    for step in 1..=4 {
        player.send_command(&format!("/tp {} 63690 16", 16 + 64 * step));
        tick_and_follow(&mut scene, &mut player, 2);
        let moved = settle_view(&mut scene, &mut player, &mut screen, 1990, Duration::from_millis(3000));
        report(&format!("after walking {} bricks", 2 * step), &moved);
        screen.summary("walked");
    }
}

#[test]
fn does_the_surface_view_keep_growing() {
    let mut scene = Scene::in_a_new_world();
    let mut player = scene
        .rig
        .join_walking(Cave::default_camera().position, 0.0, 0.0);
    player.enable_level_detail();
    player.enable_grid_detail(<Cave as GameDefinition>::grid_detail().expect("Cave draws planets in detail"));
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 5);
    player.take_command_replies();
    say(&mut scene, &mut player, "/fly");
    player.send_command(&format!("/tp 16 {SURFACE_Y} 16"));
    tick_and_follow(&mut scene, &mut player, 2);
    let mut screen = Screen::default();
    let began = Instant::now();
    let mut next_report = 0.5;
    let mut nudged = false;
    while began.elapsed().as_secs_f64() < 14.0 {
        if !nudged && began.elapsed().as_secs_f64() > 6.0 {
            nudged = true;
            println!("--- nudging the camera one brick up");
            player.send_command(&format!("/tp 16 {} 16", SURFACE_Y + 40.0));
            tick_and_follow(&mut scene, &mut player, 2);
        }
        tick_and_follow(&mut scene, &mut player, 1);
        let updates = player.update_view();
        screen.apply(updates);
        std::thread::sleep(Duration::from_millis(2));
        if began.elapsed().as_secs_f64() >= next_report {
            println!(
                "{:5.1} s: {} meshes, {:.1} MB, camera {:?}",
                began.elapsed().as_secs_f64(),
                screen.held.len(),
                screen.bytes() as f64 / 1.0e6,
                player.camera.position.brick()
            );
            next_report += 1.0;
        }
    }
    screen.summary("after 25 s");
}

#[test]
fn two_views_of_a_planet() {
    let heights: Vec<f64> = std::env::var("PROBE_HEIGHTS")
        .unwrap_or_else(|_| "66200,84200,264200,2064200".to_string())
        .split(',')
        .map(|text| text.trim().parse().expect("a number"))
        .collect();
    let mut scene = Scene::in_a_new_world();
    let mut player = scene
        .rig
        .join_walking(Cave::default_camera().position, 0.0, 0.0);
    player.enable_level_detail();
    player.enable_grid_detail(<Cave as GameDefinition>::grid_detail().expect("Cave draws planets in detail"));
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 5);
    player.take_command_replies();
    let reply = say(&mut scene, &mut player, "/fly");
    println!("/fly replied: {reply}");

    let mut screen = Screen::default();
    println!("camera at start: {:?}", player.camera.position);
    player.send_command(&format!("/tp 16 {SURFACE_Y} 16"));
    tick_and_follow(&mut scene, &mut player, 2);
    let first = settle_view(&mut scene, &mut player, &mut screen, BRICK_Y_OF_SURFACE, QUIET);
    report("surface (first time)", &first);
    screen.summary("surface");
    let surface = screen.clone();

    for &height in &heights {
        player.send_command(&format!("/tp 16 {height} 16"));
        tick_and_follow(&mut scene, &mut player, 2);
        let out = settle_view(&mut scene, &mut player, &mut screen, (height / 32.0).floor() as i128, QUIET);
        report(&format!("to {height} m up (first time)"), &out);
        screen.summary(&format!("view at {height} m"));
        relation("surface", &surface, &format!("view at {height} m"), &screen);
        relation(&format!("view at {height} m"), &screen, "surface", &surface);
        let far = screen.clone();

        // And back: today every node that is not in the new view was thrown away on the way out.
        player.send_command(&format!("/tp 16 {SURFACE_Y} 16"));
        tick_and_follow(&mut scene, &mut player, 2);
        let back = settle_view(&mut scene, &mut player, &mut screen, BRICK_Y_OF_SURFACE, QUIET);
        report(&format!("back from {height} m to the surface"), &back);
        screen.summary("surface again");
        let _ = far;
    }
}
