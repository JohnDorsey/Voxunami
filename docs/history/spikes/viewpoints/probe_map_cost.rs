//! A probe of 2026-11-22 (`docs/specs/viewpoints-spec.md` section 9i), kept as a record and not compiled with the workspace: what a map that is kept ready and never used costs the
//! player, and what is left when it is taken away (what a server that disables the map would do). The scaffolding is that of `probe_warm.rs`. Copy it to
//! `games/voxunami_cave/tests/probe_map_cost.rs`, then `PROBE_MAP=none|warm|warm_then_off PROBE_FRAME_MS=16.7 cargo test --release -p voxunami_cave --test probe_map_cost
//! the_map_that_is_not_used -- --nocapture --test-threads=1` (one mode for each run: the memory of a process is not that of another), and remove it again. `PROBE_MAP_HEIGHT` (voxels over the player, 264,000) and `PROBE_MAP_ALONG` (voxels along the ground, 0) say where the map is (`0` and `5000` is a place at the ground that nothing has looked at), and `PROBE_FOLLOW` how many frames apart the map is moved over the player in flight (15; 0 never).

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
    /// What the renderer holds and does not draw.
    hidden: HashMap<Key, (usize, usize)>,
    shows: usize,
    hides: usize,
}

impl Screen {
    fn put(&mut self, key: Key, quads: usize, detailed: usize) {
        if quads + detailed == 0 {
            self.held.remove(&key);
            self.hidden.remove(&key);
        } else if let Some(hidden) = self.hidden.get_mut(&key) {
            *hidden = (quads, detailed);
        } else {
            self.held.insert(key, (quads, detailed));
        }
    }

    /// What the renderer does with a mesh it is given hidden: replaces it where it is, drawn or hidden, or holds it hidden if it is new; nothing to see removes it.
    fn put_hidden(&mut self, key: Key, quads: usize, detailed: usize) {
        if quads + detailed == 0 {
            self.held.remove(&key);
            self.hidden.remove(&key);
        } else if let Some(drawn) = self.held.get_mut(&key) {
            *drawn = (quads, detailed);
        } else {
            self.hidden.insert(key, (quads, detailed));
        }
    }

    fn hide(&mut self, key: Key) {
        self.hides += 1;
        let value = self.held.remove(&key).expect("hidden what was drawn");
        self.hidden.insert(key, value);
    }

    fn show(&mut self, key: Key) {
        self.shows += 1;
        let value = self.hidden.remove(&key).expect("shown what was hidden");
        self.held.insert(key, value);
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
                    self.hidden.remove(&(None, Self::brick(coordinate)));
                }
                MeshUpdate::HideBrick(coordinate) => self.hide((None, Self::brick(coordinate))),
                MeshUpdate::ShowBrick(coordinate) => self.show((None, Self::brick(coordinate))),
                MeshUpdate::HideGridBrick { grid, coordinate } => {
                    self.hide((Some(grid), Self::brick(coordinate)));
                }
                MeshUpdate::ShowGridBrick { grid, coordinate } => {
                    self.show((Some(grid), Self::brick(coordinate)));
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
                    self.hidden.remove(&(Some(grid), Self::brick(coordinate)));
                }
                MeshUpdate::UploadCoarse { cube, mesh } => {
                    self.put((None, cube), mesh.quads.len(), mesh.detailed.len())
                }
                MeshUpdate::RemoveCoarse(cube) => {
                    self.held.remove(&(None, cube));
                    self.hidden.remove(&(None, cube));
                }
                MeshUpdate::HideCoarse(cube) => self.hide((None, cube)),
                MeshUpdate::ShowCoarse(cube) => self.show((None, cube)),
                MeshUpdate::HideGridCoarse { grid, cube } => self.hide((Some(grid), cube)),
                MeshUpdate::ShowGridCoarse { grid, cube } => self.show((Some(grid), cube)),
                MeshUpdate::UploadGridCoarse { grid, cube, mesh } => {
                    self.put((Some(grid), cube), mesh.quads.len(), mesh.detailed.len())
                }
                MeshUpdate::RemoveGridCoarse { grid, cube } => {
                    self.held.remove(&(Some(grid), cube));
                    self.hidden.remove(&(Some(grid), cube));
                }
                MeshUpdate::UploadHiddenBrick { coordinate, mesh } => self.put_hidden(
                    (None, Self::brick(coordinate)),
                    mesh.quads.len(),
                    mesh.detailed.len(),
                ),
                MeshUpdate::UploadHiddenGridBrick {
                    grid,
                    coordinate,
                    mesh,
                } => self.put_hidden(
                    (Some(grid), Self::brick(coordinate)),
                    mesh.quads.len(),
                    mesh.detailed.len(),
                ),
                MeshUpdate::UploadHiddenCoarse { cube, mesh } => {
                    self.put_hidden((None, cube), mesh.quads.len(), mesh.detailed.len());
                }
                MeshUpdate::UploadHiddenGridCoarse { grid, cube, mesh } => {
                    self.put_hidden((Some(grid), cube), mesh.quads.len(), mesh.detailed.len());
                }
                MeshUpdate::RemoveGrid(grid) => {
                    self.held.retain(|(held, _), _| *held != Some(grid))
                }
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
        self.held
            .values()
            .map(|(quads, detailed)| quads + detailed)
            .sum()
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
        println!(
            "   cubes drawn inside another drawn cube: {}",
            self.overlaps()
        );
        println!("   extents (bricks, origins):{}", self.extents());
    }
}

thread_local! {
    /// Seconds spent in `update_view` in the calls that changed nothing, and how many.
    static IDLE: std::cell::RefCell<(f64, usize)> = const { std::cell::RefCell::new((0.0, 0)) };
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
    let mut last_trace = Instant::now();
    let mut arrived_at: Option<Instant> = None;
    let frame_ms: Option<f64> = std::env::var("PROBE_FRAME_MS")
        .ok()
        .and_then(|t| t.parse().ok());
    loop {
        let frame_began = Instant::now();
        assert!(Instant::now() < deadline, "gave up waiting for the view");
        tick_and_follow(scene, player, 1);
        let timed = Instant::now();
        let updates = player.update_view();
        let spent = timed.elapsed();
        if updates.is_empty() {
            IDLE.with(|idle| {
                let mut idle = idle.borrow_mut();
                idle.0 += spent.as_secs_f64();
                idle.1 += 1;
            });
        }
        result.calls += 1;
        let arrived = (player.camera.position.brick().y() - target_brick_y).abs() <= 2;
        if arrived && arrived_at.is_none() {
            arrived_at = Some(Instant::now());
            last_change = Instant::now();
        }
        if updates.is_empty() {
            if frame_ms.is_none() {
                std::thread::sleep(Duration::from_millis(2));
            }
        } else {
            result.changing_calls += 1;
            result.most_in_a_call = result.most_in_a_call.max(updates.len());
            if std::env::var_os("PROBE_TIMELINE").is_some() {
                let count =
                    |test: &dyn Fn(&MeshUpdate) -> bool| updates.iter().filter(|u| test(u)).count();
                println!(
                    "   t={:6.3} s call {:4}: {:5} updates: {:5} show, {:5} hide, {:4} upload brick, {:4} upload node, {:4} remove",
                    began.elapsed().as_secs_f64(),
                    result.calls,
                    updates.len(),
                    count(&|u| matches!(
                        u,
                        MeshUpdate::ShowCoarse(_) | MeshUpdate::ShowGridCoarse { .. }
                    )),
                    count(&|u| matches!(
                        u,
                        MeshUpdate::HideCoarse(_) | MeshUpdate::HideGridCoarse { .. }
                    )),
                    count(&|u| matches!(
                        u,
                        MeshUpdate::Upload { .. } | MeshUpdate::UploadGridBrick { .. }
                    )),
                    count(&|u| matches!(
                        u,
                        MeshUpdate::UploadCoarse { .. } | MeshUpdate::UploadGridCoarse { .. }
                    )),
                    count(&|u| matches!(
                        u,
                        MeshUpdate::Remove(_)
                            | MeshUpdate::RemoveGridBrick { .. }
                            | MeshUpdate::RemoveCoarse(_)
                            | MeshUpdate::RemoveGridCoarse { .. }
                    )),
                );
            }
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
        if let Some(frame_ms) = frame_ms {
            let frame = Duration::from_secs_f64(frame_ms / 1000.0);
            if let Some(rest) = frame.checked_sub(frame_began.elapsed()) {
                std::thread::sleep(rest);
            }
        }
        if arrived_at.is_some() && last_change.elapsed() >= quiet {
            break;
        }
        if std::env::var_os("PROBE_TRACE").is_some()
            && last_trace.elapsed() >= Duration::from_millis(250)
        {
            last_trace = Instant::now();
            let levels = screen.per_level();
            println!(
                "   trace {:5.2} s: {} meshes, coarsest level held {:?}, finest {:?}",
                began.elapsed().as_secs_f64(),
                screen.held.len(),
                levels.keys().rev().find(|level| **level < 70),
                levels.keys().next()
            );
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
    println!(
        "   camera at the end: {:?}, vertical field of view {}",
        player.camera.position, player.camera.vertical_field_of_view
    );
    println!(
        "   screen so far: {} drawn, {} hidden ({:.1} MB), {} shows and {} hides in all",
        screen.held.len(),
        screen.hidden.len(),
        screen
            .hidden
            .values()
            .map(|(q, d)| q * 8 + d * 16)
            .sum::<usize>() as f64
            / 1e6,
        screen.shows,
        screen.hides
    );
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

/// What `Rig::settle` does, keeping the updates it throws away (the first picture of the world is made in them: a screen that does not take them in holds only what changed since).
fn join_settle(scene: &mut Scene, player: &mut common::Player) -> Vec<MeshUpdate> {
    let mut updates = Vec::new();
    player.send_pose();
    for _ in 0..2 {
        let server = &mut scene.rig.server;
        updates.extend(
            player
                .settle(|| {
                    server.step();
                })
                .unwrap(),
        );
    }
    updates
}

fn report(name: &str, settled: &Settled) {
    println!("   (camera y at the end: see next line)");
    println!(
        "{name}: settled after {:.2} s ({} calls, {} that changed something, at most {} updates in one call, {} uploads, {} removals)",
        settled.seconds,
        settled.calls,
        settled.changing_calls,
        settled.most_in_a_call,
        settled.uploads,
        settled.removals
    );
    IDLE.with(|idle| {
        let mut idle = idle.borrow_mut();
        if idle.1 > 0 {
            println!(
                "   idle update_view calls: {} of them, {:.3} ms each",
                idle.1,
                idle.0 / idle.1 as f64 * 1e3
            );
        }
        *idle = (0.0, 0);
    });
}


/// The CPU time this process has used, in seconds, and its memory (working set and private bytes) in megabytes, as Windows says (a call to PowerShell: about a third of a second).
fn process_use() -> (f64, f64, f64) {
    let pid = std::process::id();
    let script = format!(
        "$p=Get-Process -Id {pid}; Write-Output \"$($p.TotalProcessorTime.TotalSeconds) $($p.WorkingSet64) $($p.PrivateMemorySize64)\""
    );
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .expect("powershell runs");
    let text = String::from_utf8_lossy(&output.stdout).replace(',', ".");
    let numbers: Vec<f64> = text
        .split_whitespace()
        .filter_map(|word| word.parse().ok())
        .collect();
    (numbers[0], numbers[1] / 1.0e6, numbers[2] / 1.0e6)
}

/// How long `update_view` took in each call of a stretch, in milliseconds, and what it was told to do.
#[derive(Default)]
struct Stretch {
    milliseconds: Vec<f64>,
    hidden_uploads: usize,
    most_hidden_uploads_in_a_call: usize,
    uploads: usize,
    shows: usize,
    hides: usize,
    removals: usize,
    updates: usize,
}

impl Stretch {
    fn note(&mut self, spent: Duration, updates: &[MeshUpdate]) {
        self.milliseconds.push(spent.as_secs_f64() * 1e3);
        let mut hidden = 0;
        for update in updates {
            match update {
                MeshUpdate::UploadHiddenCoarse { .. }
                | MeshUpdate::UploadHiddenGridCoarse { .. }
                | MeshUpdate::UploadHiddenBrick { .. }
                | MeshUpdate::UploadHiddenGridBrick { .. } => hidden += 1,
                MeshUpdate::Upload { .. }
                | MeshUpdate::UploadGridBrick { .. }
                | MeshUpdate::UploadCoarse { .. }
                | MeshUpdate::UploadGridCoarse { .. } => self.uploads += 1,
                MeshUpdate::ShowCoarse(_)
                | MeshUpdate::ShowGridCoarse { .. }
                | MeshUpdate::ShowBrick(_)
                | MeshUpdate::ShowGridBrick { .. } => self.shows += 1,
                MeshUpdate::HideCoarse(_)
                | MeshUpdate::HideGridCoarse { .. }
                | MeshUpdate::HideBrick(_)
                | MeshUpdate::HideGridBrick { .. } => self.hides += 1,
                MeshUpdate::Remove(_)
                | MeshUpdate::RemoveGridBrick { .. }
                | MeshUpdate::RemoveCoarse(_)
                | MeshUpdate::RemoveGridCoarse { .. } => self.removals += 1,
                _ => {}
            }
        }
        self.hidden_uploads += hidden;
        self.most_hidden_uploads_in_a_call = self.most_hidden_uploads_in_a_call.max(hidden);
        self.updates += updates.len();
    }

    fn percentile(&self, fraction: f64) -> f64 {
        let mut sorted = self.milliseconds.clone();
        sorted.sort_by(f64::total_cmp);
        sorted[((sorted.len() - 1) as f64 * fraction) as usize]
    }

    fn print(&self, name: &str, cpu_seconds: f64, wall_seconds: f64) {
        let calls = self.milliseconds.len();
        let mean = self.milliseconds.iter().sum::<f64>() / calls as f64;
        println!(
            "   {name}: {calls} calls of update_view, mean {mean:.3} ms, median {:.3}, p99 {:.3}, worst {:.3} ms; {} mesh updates ({} hidden uploads, at most {} in a call, {} uploads, {} shows, {} hides, {} removals); the process used {cpu_seconds:.2} s of CPU in {wall_seconds:.1} s (an average of {:.2} cores)",
            self.percentile(0.5),
            self.percentile(0.99),
            self.percentile(1.0),
            self.updates,
            self.hidden_uploads,
            self.most_hidden_uploads_in_a_call,
            self.uploads,
            self.shows,
            self.hides,
            self.removals,
            cpu_seconds / wall_seconds,
        );
    }
}

/// Runs the view for `seconds` of wall-clock time at the frame rate asked for, moving the camera with `step` (given how many calls have been made, and what to do with the
/// player), applying the updates to `screen`.
fn run_for(
    scene: &mut Scene,
    player: &mut common::Player,
    screen: &mut Screen,
    seconds: f64,
    mut step: impl FnMut(usize, &mut Scene, &mut common::Player),
) -> (Stretch, f64, f64) {
    let frame = Duration::from_secs_f64(
        std::env::var("PROBE_FRAME_MS")
            .ok()
            .and_then(|text| text.parse::<f64>().ok())
            .unwrap_or(16.7)
            / 1000.0,
    );
    let (cpu_before, _, _) = process_use();
    let began = Instant::now();
    let mut stretch = Stretch::default();
    let mut call = 0;
    while began.elapsed().as_secs_f64() < seconds {
        let frame_began = Instant::now();
        step(call, scene, player);
        tick_and_follow(scene, player, 1);
        let timed = Instant::now();
        let updates = player.update_view();
        stretch.note(timed.elapsed(), &updates);
        screen.apply(updates);
        call += 1;
        if let Some(rest) = frame.checked_sub(frame_began.elapsed()) {
            std::thread::sleep(rest);
        }
    }
    let wall = began.elapsed().as_secs_f64();
    let (cpu_after, _, _) = process_use();
    (stretch, cpu_after - cpu_before, wall)
}

/// Where the map would be for a camera at `place` on a planet whose centre is the origin of its own coordinates: straight up from it, 264 km further out.
fn map_place_over(place: voxel_space::WorldPosition) -> voxel_space::WorldPosition {
    let height: f64 = std::env::var("PROBE_MAP_HEIGHT")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(264_000.0);
    let along: f64 = std::env::var("PROBE_MAP_ALONG")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(0.0);
    let brick = place.brick();
    let at = |axis: usize| {
        let (coordinate, offset) = match axis {
            0 => (brick.x(), place.offset()[0]),
            1 => (brick.y(), place.offset()[1]),
            _ => (brick.z(), place.offset()[2]),
        };
        coordinate as f64 * 32.0 + offset
    };
    let (x, y, z) = (at(0) + along, at(1), at(2));
    let radius = (x * x + y * y + z * z).sqrt();
    let scale = (radius + height) / radius;
    let (x, y, z) = (x * scale, y * scale, z * scale);
    let split = |value: f64| (value.div_euclid(32.0) as i128, value.rem_euclid(32.0));
    let ((bx, ox), (by, oy), (bz, oz)) = (split(x), split(y), split(z));
    voxel_space::WorldPosition::new(BrickCoordinate::new(bx, by, bz), [ox, oy, oz])
}

/// A player standing on a planet's surface, who is given a map kept ready 264 km over his head (`PROBE_MAP=warm`), or one that is kept ready and then taken away
/// (`warm_then_off`), or none (`none`): what it costs while it is not used.
#[test]
fn the_map_that_is_not_used() {
    let mode = std::env::var("PROBE_MAP").unwrap_or_else(|_| "none".to_string());
    let mut scene = Scene::in_a_new_world();
    let mut player = scene
        .rig
        .join_walking(Cave::default_camera().position, 0.0, 0.0);
    player.enable_level_detail();
    player.hold_hidden_pictures(game_shell::session::HELD_PICTURE_BUDGET);
    player.enable_grid_detail(
        <Cave as GameDefinition>::grid_detail().expect("Cave draws planets in detail"),
    );
    let mut screen = Screen::default();
    screen.apply(join_settle(&mut scene, &mut player));
    tick_and_follow(&mut scene, &mut player, 5);
    player.take_command_replies();
    say(&mut scene, &mut player, "/fly");
    player.send_command(&format!("/tp 16 {SURFACE_Y} 16"));
    tick_and_follow(&mut scene, &mut player, 2);
    let first = settle_view(
        &mut scene,
        &mut player,
        &mut screen,
        BRICK_Y_OF_SURFACE,
        QUIET,
    );
    report("the surface", &first);
    let planet = screen
        .held
        .keys()
        .find_map(|(grid, _)| *grid)
        .expect("a planet is drawn");
    let (cpu, working_set, private) = process_use();
    println!(
        "=== mode {mode}: the surface view settled ({} meshes drawn, {} held hidden): CPU so far {cpu:.1} s, working set {working_set:.0} MB, private {private:.0} MB",
        screen.held.len(),
        screen.hidden.len()
    );

    {
        // What handing the level's answers to a thread costs the thread of the update: a copy of the terrain (the generators and the tree of which generator makes which region).
        let world = player.client().world();
        let terrain = &world.world().main().terrain;
        let began = Instant::now();
        let copy = terrain.clone();
        let took = began.elapsed();
        println!(
            "the level's assignment tree: {} nodes; a copy of the terrain takes {:.3} ms",
            copy.assignments().node_count(),
            took.as_secs_f64() * 1e3
        );
    }

    // Where the player is, and where the map would be.
    let place = player.camera_place_in(planet).expect("a place on the planet");
    let map = map_place_over(place);
    println!("the player is at {place:?} and the map at {map:?}");

    let (stretch, cpu, wall) = run_for(&mut scene, &mut player, &mut screen, 10.0, |_, _, _| {});
    stretch.print("10 s standing, no map yet", cpu, wall);

    if mode != "none" {
        let (cpu_before, _, _) = process_use();
        let began = Instant::now();
        player.keep_ready(planet, Some(map));
        let mut warming = Stretch::default();
        let mut last_change = Instant::now();
        let frame = Duration::from_secs_f64(16.7 / 1000.0);
        loop {
            let frame_began = Instant::now();
            tick_and_follow(&mut scene, &mut player, 1);
            let timed = Instant::now();
            let updates = player.update_view();
            warming.note(timed.elapsed(), &updates);
            if !updates.is_empty() {
                last_change = Instant::now();
            }
            screen.apply(updates);
            if let Some(rest) = frame.checked_sub(frame_began.elapsed()) {
                std::thread::sleep(rest);
            }
            if last_change.elapsed() >= Duration::from_millis(2500) || began.elapsed().as_secs() > 300 {
                break;
            }
        }
        let wall = began.elapsed().as_secs_f64() - 2.5;
        let (cpu_after, working_set, private) = process_use();
        warming.print("keeping the map ready (until nothing changed for 2.5 s)", cpu_after - cpu_before, wall);
        println!(
            "   the map is ready after {:.1} s: {} meshes drawn, {} held hidden ({:.1} MB of quads); working set {working_set:.0} MB, private {private:.0} MB",
            wall,
            screen.held.len(),
            screen.hidden.len(),
            screen.hidden.values().map(|(q, d)| q * 8 + d * 16).sum::<usize>() as f64 / 1e6
        );
    }

    let (stretch, cpu, wall) = run_for(&mut scene, &mut player, &mut screen, 15.0, |_, _, _| {});
    stretch.print("15 s standing, the map ready and not used", cpu, wall);
    let (_, working_set, private) = process_use();
    println!("   working set {working_set:.0} MB, private {private:.0} MB, {} held hidden", screen.hidden.len());

    // The player flies along the ground, smoothly (a pose a frame), and the map goes with him (kept ready at the place over his head `PROBE_FOLLOW` frames apart).
    let speed: f64 = std::env::var("PROBE_SPEED")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(100.0);
    let follow: usize = std::env::var("PROBE_FOLLOW")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(15);
    use common::PlayerExt;
    let start = player.camera.position;
    let start_voxels = common::voxels_of(&player.camera);
    player.hold(&[game_shell::KeyCode::KeyW, game_shell::KeyCode::ControlLeft]);
    let (stretch, cpu, wall) = run_for(&mut scene, &mut player, &mut screen, 20.0, |call, _scene, player| {
        if mode != "none" && mode != "warm_then_off" && follow > 0 && call % follow == 0 {
            if let Some(here) = player.camera_place_in(planet) {
                player.keep_ready(planet, Some(map_place_over(here)));
            }
        }
    });
    player.hold(&[]);
    let end_voxels = common::voxels_of(&player.camera);
    let moved = ((end_voxels[0] - start_voxels[0]).powi(2) + (end_voxels[1] - start_voxels[1]).powi(2) + (end_voxels[2] - start_voxels[2]).powi(2)).sqrt();
    println!("   flew {moved:.0} voxels in {wall:.1} s: {:.1} voxels a second (PROBE_SPEED is not used: the player is held walking forward, and sprinting)", moved / wall);
    let _ = (speed, start);
    stretch.print(&format!("20 s flying forward, sprinting (the map follows every {follow} frames)"), cpu, wall);
    let (_, working_set, private) = process_use();
    println!("   working set {working_set:.0} MB, private {private:.0} MB, {} held hidden, {} drawn; the camera is at {:?}", screen.hidden.len(), screen.held.len(), player.camera.position);

    if mode == "warm_then_off" {
        // The server says no map: nothing is kept ready any more.
        let before = (screen.hidden.len(), screen.hidden.values().map(|(q, d)| q * 8 + d * 16).sum::<usize>());
        player.keep_ready(planet, None);
        let (stretch, cpu, wall) = run_for(&mut scene, &mut player, &mut screen, 15.0, |_, _, _| {});
        stretch.print("15 s after the map was taken away", cpu, wall);
        let (_, working_set, private) = process_use();
        println!(
            "   held hidden: {} meshes ({:.1} MB of quads) before, {} ({:.1} MB) after; working set {working_set:.0} MB, private {private:.0} MB",
            before.0,
            before.1 as f64 / 1e6,
            screen.hidden.len(),
            screen.hidden.values().map(|(q, d)| q * 8 + d * 16).sum::<usize>() as f64 / 1e6
        );
    }
}
