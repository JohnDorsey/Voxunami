//! A planet that goes round something at a hundred metres a second or more (Phase 40e): ships near it are simulated in a frame that goes with it, so that a ship falls
//! onto it, rests on it, is carried round by its spin and goes with it, and a ship that leaves its sphere goes on in the world at the speed it had.

mod common;

use common::docking::{a_pair, make};
use common::{SEED, Scene, length, minus, player_pose, ship_id, ship_pose, tick_and_follow};
use game_shell::{GameContent, RawFile, RawPack};
use server::GridView;
use voxel_space::{GridId, GridPose, LevelIndex, Orientation, VoxelCoordinate, WorldPosition};
use voxunami_cave::aboard::in_voxels;
use voxunami_cave::content::CaveContent;

/// What the pack of a test says of the planet it adds: where it is and how it moves (lines of the body file), how far its ground reaches in bricks and how hard it
/// pulls at the surface. It is a planet of 10,000 voxels.
fn wanderer_pack(motion: &str, extent_bricks: u32, surface_gravity: f64) -> Vec<RawPack> {
    vec![RawPack {
        folder: "sky".to_owned(),
        manifest: r#"{ "id": "sky" }"#.to_owned(),
        blocks: Vec::new(),
        patches: Vec::new(),
        bodies: vec![RawFile {
            name: "wanderer".to_owned(),
            text: format!(
                r#"{{
                "level": "voxunami_cave:mini_cosmos",
                "terrain": {{
                    "family": "base_blocks:planet_v1",
                    "parameters": {{ "seed": 99, "radius": 10000, "hill_height": 20,
                        "largest_hill_wavelength_log2": 7, "hill_octave_count": 4 }}
                }},
                "extent_bricks": {extent_bricks},
                "surface_gravity": {surface_gravity},
                {motion}
            }}"#
            ),
        }],
    }]
}

/// The motion of the usual wanderer: it goes round the home planet at 400,000 voxels once in 20,000 seconds (125.7 metres a second, 6.3 voxels a tick), and turns
/// once in `spin` seconds if it is given one (about its own y axis).
fn orbit_of_the_home_planet(spin: Option<f64>) -> String {
    let spin = spin.map_or(String::new(), |period| {
        format!(r#""spin": {{ "axis": [0, 1, 0], "period": {period} }},"#)
    });
    format!(
        r#""rail": {{ {spin} "orbit": {{ "around": "voxunami_cave:planet", "radius": 400000, "period": 20000 }} }}"#
    )
}

fn scene_with(pack: &[RawPack]) -> Scene {
    let mut content = CaveContent::new();
    content.apply_packs(pack).unwrap();
    let world = content.new_world(SEED);
    Scene::in_this_world(content, world)
}

fn a_scene(spin: Option<f64>) -> Scene {
    scene_with(&wanderer_pack(&orbit_of_the_home_planet(spin), 340, 8.0))
}

/// The wanderer is the second ship, and what a test makes after it the third.
const WANDERER: u64 = 1;
const THE_RAFT: u64 = 2;

/// How far the sphere of the wanderer reaches: one and a half times the reach of its ground, which is 340 bricks (a brick is 32 voxels).
const SPHERE: f64 = 16_320.0;
/// And how far it keeps what it carries: a quarter more.
const SPHERE_KEPT: f64 = 20_400.0;

fn voxels(scene: &Scene, number: u64) -> [f64; 3] {
    ship_pose(scene, number).position.in_voxels()
}

fn velocity(scene: &Scene, number: u64) -> [f64; 3] {
    scene
        .rig
        .server
        .world()
        .grids()
        .get(ship_id(number))
        .expect("the ship is there")
        .linear_velocity
        .map(f64::from)
}

/// Where a ship is from the middle of the wanderer, in the world's axes.
fn from_the_middle(scene: &Scene, number: u64) -> [f64; 3] {
    minus(voxels(scene, number), voxels(scene, WANDERER))
}

/// A raft of stone over the wanderer (`common::raft_over`).
fn raft_over(scene: &mut Scene, offset: [f64; 3], held: bool, against: [f32; 3]) -> u64 {
    common::raft_over(scene, WANDERER, offset, held, against)
}

/// The raft over the wanderer, followed for `ticks`: where it is each tick in the world, and from the middle of the planet.
fn follow_a_raft(
    scene: &mut Scene,
    offset: [f64; 3],
    against: [f32; 3],
    ticks: usize,
) -> (Vec<[f64; 3]>, Vec<[f64; 3]>) {
    let raft = raft_over(scene, offset, false, against);
    let (mut path, mut offsets) = (Vec::new(), Vec::new());
    for _ in 0..ticks {
        scene.ticks(1);
        path.push(voxels(scene, raft));
        offsets.push(from_the_middle(scene, raft));
    }
    (path, offsets)
}

#[test]
fn a_raft_dropped_over_a_planet_on_a_fast_orbit_falls_to_it_rests_on_it_and_goes_with_it() {
    let mut scene = a_scene(None);
    scene.ticks(1);
    let planet = voxels(&scene, WANDERER);
    // Sixty voxels over its pole: the planet is at 400,000 along x and 10,000 voxels (and some hills) high.
    let raft = raft_over(&mut scene, [0.0, 10_060.0, 0.0], false, [0.0; 3]);
    scene.ticks(320);
    // Sixteen seconds on, the planet has gone two kilometres. The raft fell to it, in about four seconds, and lies on its ground, with it: over the pole it was dropped
    // over (not two kilometres behind, which is as far from the middle of the planet but nowhere near the pole), on the ground there, and going at the planet's speed.
    let travelled = length(minus(voxels(&scene, WANDERER), planet));
    assert!(travelled > 1_900.0, "the planet went {travelled} voxels");
    let offset = from_the_middle(&scene, raft);
    assert!(
        offset[0].abs() < 15.0 && offset[2].abs() < 15.0,
        "the raft is {offset:?} from the middle of the planet"
    );
    assert!(
        (9_950.0..10_045.0).contains(&offset[1]),
        "the raft is {offset:?} from the middle of the planet"
    );
    let relative = length(minus(velocity(&scene, raft), velocity(&scene, WANDERER)));
    assert!(
        relative < 0.5,
        "the raft moves {relative} metres a second against the planet"
    );
}

/// `vector` turned by the unit quaternion `[x, y, z, w]`.
fn rotated(quaternion: [f64; 4], vector: [f64; 3]) -> [f64; 3] {
    let [x, y, z, w] = quaternion;
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let t = cross([x, y, z], vector).map(|component| 2.0 * component);
    let u = cross([x, y, z], t);
    [
        vector[0] + w * t[0] + u[0],
        vector[1] + w * t[1] + u[1],
        vector[2] + w * t[2] + u[2],
    ]
}

#[test]
fn a_raft_resting_on_the_ground_of_a_planet_that_turns_is_carried_round_with_it() {
    // A turn in 12,566 seconds is five metres a second at the equator, 10,000 voxels out.
    let mut scene = a_scene(Some(12_566.0));
    scene.ticks(1);
    // On the +x side, over the equator, a little above the ground (the hills are 20 voxels or so).
    let raft = raft_over(&mut scene, [10_070.0, 0.0, 0.0], false, [0.0; 3]);
    scene.ticks(400);
    // Where the raft is in the planet's own coordinates: the turn of the planet undone.
    let place_on_the_planet = |scene: &Scene| {
        let turn = ship_pose(scene, WANDERER)
            .orientation
            .inverse()
            .components();
        rotated(turn, from_the_middle(scene, raft))
    };
    let early = place_on_the_planet(&scene);
    // On the ground where it was put, on the planet's own +x side (not left behind: that is as far from the middle but two kilometres away along the orbit).
    assert!(
        early[0] > 9_950.0 && early[0] < 10_060.0 && early[1].abs() < 40.0 && early[2].abs() < 40.0,
        "on the ground at the equator: {early:?}"
    );
    scene.ticks(100);
    let late = place_on_the_planet(&scene);
    // Five seconds later, in the planet's own coordinates, it has not moved: it is carried round, not left behind (left behind it would have gone 25 metres).
    assert!(length(minus(late, early)) < 0.6, "{early:?} then {late:?}");
    // And it is going round with the ground: some five metres a second against the planet's centre, the planet's own speed apart.
    let relative = length(minus(velocity(&scene, raft), velocity(&scene, WANDERER)));
    assert!(
        (relative - 5.0).abs() < 0.6,
        "{relative} metres a second round the middle"
    );
}

#[test]
fn a_walker_dropped_over_a_planet_on_a_fast_orbit_lands_on_it_and_goes_with_it() {
    let mut scene = a_scene(None);
    let planet = voxels(&scene, WANDERER);
    let mut player = scene.rig.join_walking(
        [planet[0] + 0.5, planet[1] + 10_100.0, planet[2] + 0.5],
        0.0,
        0.0,
    );
    // (It comes into the world a tick or more after it has joined.)
    for _ in 0..200 {
        if scene.rig.server.world().entities().iter().next().is_some() {
            break;
        }
        tick_and_follow(&mut scene, &mut player, 1);
    }
    let spawned_over = minus(
        in_voxels(player_pose(&scene, 1).position),
        voxels(&scene, WANDERER),
    );
    // (No `settle`: it waits for the view of the level around the camera to go quiet for five ticks, and a camera that goes with a planet at 125 metres a second is in a
    // new brick every five ticks. Whether it did was a race between the camera and the machine: a fast one won it after thousands of ticks, a 4-core Linux lost it
    // in the two minutes it is given. The server says where the walker lands, whatever the client has loaded.)
    tick_and_follow(&mut scene, &mut player, 300);
    let pose = player_pose(&scene, 1);
    assert_eq!(pose.grid, ship_id(WANDERER), "aboard the planet");
    let [x, y, z] = in_voxels(pose.position);
    let distance = (x * x + y * y + z * z).sqrt();
    assert!((9_950.0..10_080.0).contains(&distance), "{distance}");
    // It landed where it was over the planet when it came in (the planet goes six voxels a tick and the walker falls for a hundred ticks: left behind it would be
    // six hundred voxels along the orbit from there).
    assert!(
        (x - spawned_over[0]).abs() < 20.0 && (z - spawned_over[2]).abs() < 20.0,
        "landed at {x} {z}, spawned over {spawned_over:?}"
    );
    let standing = pose;
    let before = voxels(&scene, WANDERER);
    tick_and_follow(&mut scene, &mut player, 100);
    let after = voxels(&scene, WANDERER);
    assert!(length(minus(after, before)) > 500.0, "the planet goes on");
    assert_eq!(player_pose(&scene, 1).position, standing.position);
}

/// The greatest change from one tick to the next of the step a ship takes between them, in voxels. A ship that goes on straight has none; the pull of a planet bends
/// its path by a few hundredths of a voxel a tick; a ship whose speed was taken wrong when it came into or went out of a frame has the planet's own, 6.3.
fn greatest_kink(path: &[[f64; 3]]) -> f64 {
    path.windows(3)
        .map(|three| length(minus(minus(three[2], three[1]), minus(three[1], three[0]))))
        .fold(0.0, f64::max)
}

#[test]
fn a_ship_that_flies_away_from_a_planet_on_a_fast_orbit_goes_on_in_the_world_at_the_speed_it_had() {
    let mut scene = a_scene(None);
    scene.ticks(1);
    // Thrown up from 200 voxels over the pole at 400 metres a second against the planet: it is out
    // of the sphere (16,320) in some 12 seconds and out of the distance at which it is let go (20,400) in 25.
    let (path, offsets) = follow_a_raft(&mut scene, [0.0, 10_200.0, 0.0], [0.0, 400.0, 0.0], 800);
    let distances: Vec<f64> = offsets.iter().map(|offset| length(*offset)).collect();
    assert!(distances[0] < SPHERE, "it starts in the sphere");
    assert!(
        distances
            .iter()
            .any(|distance| *distance > SPHERE && *distance < SPHERE_KEPT),
        "it goes through the margin where it is kept"
    );
    assert!(
        distances[799] > SPHERE_KEPT + 1_000.0,
        "it ends well out of it: {}",
        distances[799]
    );
    // No tick of its way has a kink: where it was carried with the planet (the first ticks), where it was let go, and after.
    let kink = greatest_kink(&path);
    assert!(
        kink < 0.5,
        "the path of the raft has a kink of {kink} voxels"
    );
}

#[test]
fn a_ship_that_flies_in_to_a_planet_on_a_fast_orbit_is_taken_in_without_a_jump() {
    let mut scene = a_scene(None);
    scene.ticks(1);
    // Twenty thousand voxels over the pole and going toward the planet at 400 metres a second against it: in the sphere in some 10 seconds, where the planet
    // carries it, and on toward the ground (about 20 voxels a tick, and the planet's own 6.3 along).
    let (path, offsets) = follow_a_raft(&mut scene, [0.0, 20_000.0, 0.0], [0.0, -400.0, 0.0], 300);
    let distances: Vec<f64> = offsets.iter().map(|offset| length(*offset)).collect();
    assert!(distances[0] > SPHERE_KEPT - 500.0, "it starts outside");
    assert!(
        distances[299] < SPHERE,
        "it ends inside: {}",
        distances[299]
    );
    let kink = greatest_kink(&path);
    assert!(
        kink < 0.5,
        "the path of the raft has a kink of {kink} voxels"
    );
}

#[test]
fn a_ship_moves_against_a_planet_on_a_fast_orbit_as_it_does_against_one_that_stands_still() {
    // The same planet, once going round the point where the home planet is, at 400,000 voxels (the home planet pulls in both, and is not what it goes round), and
    // once standing where that starts it. A raft thrown up from it in the same way goes the same way against it, but for the pull of the home planet, which changes a little as the planet goes (a tenth of a voxel in twenty seconds, measured).
    let going =
        r#""rail": { "orbit": { "around": [0, 0, 0], "radius": 400000, "period": 20000 } }"#;
    let standing = r#""position": [400000, 0, 0]"#;
    let mut flights = Vec::new();
    for motion in [going, standing] {
        let mut scene = scene_with(&wanderer_pack(motion, 340, 8.0));
        scene.ticks(1);
        flights.push(follow_a_raft(&mut scene, [30.0, 10_100.0, -20.0], [3.0, 150.0, 2.0], 400).1);
    }
    let (rail, still) = (&flights[0], &flights[1]);
    let worst = rail
        .iter()
        .zip(still)
        .map(|(on_the_rail, standing)| length(minus(*on_the_rail, *standing)))
        .fold(0.0, f64::max);
    assert!(
        worst < 0.3,
        "the two flights are as far as {worst} voxels apart"
    );
    // (It went somewhere: 150 metres a second up for twenty seconds and back down by a pull of 8, over the ground.)
    assert!(length(minus(rail[200], rail[0])) > 500.0);
}

#[test]
fn a_held_ship_near_a_planet_on_a_fast_orbit_goes_with_it_and_one_beyond_its_sphere_stays_where_it_was_put()
 {
    let mut scene = a_scene(None);
    scene.ticks(1);
    let planet = voxels(&scene, WANDERER);
    // Two hundred voxels over the pole is in the sphere (16,320); thirty thousand is not.
    // (A ship that is held has no speed in the world: that is what holding it does, and it is made so here, against the ground. Near the planet it goes with it all
    // the same.)
    let still_in_the_world = velocity(&scene, WANDERER).map(|part| -part as f32);
    let near = raft_over(&mut scene, [0.0, 10_200.0, 0.0], true, still_in_the_world);
    let far = raft_over(&mut scene, [0.0, 30_000.0, 0.0], true, [0.0; 3]);
    let far_at_first = (ship_pose(&scene, far), velocity(&scene, far));
    scene.ticks(200);
    assert!(
        length(minus(voxels(&scene, WANDERER), planet)) > 1_000.0,
        "the planet went by"
    );
    // The held ship near is where it was from the planet (to a hundredth of a voxel), and goes at the planet's speed, so that it goes on with it when it is let go.
    let offset = from_the_middle(&scene, near);
    assert!(
        length(minus(offset, [0.0, 10_200.0, 0.0])) < 0.01,
        "{offset:?} from the middle of the planet"
    );
    let against = length(minus(velocity(&scene, near), velocity(&scene, WANDERER)));
    assert!(
        against < 1e-3,
        "{against} metres a second against the planet"
    );
    // The one beyond the sphere was not touched.
    assert_eq!(
        (ship_pose(&scene, far), velocity(&scene, far)),
        far_at_first
    );
}

/// The angle in radians between two turns.
fn angle_between(first: Orientation, second: Orientation) -> f64 {
    let (a, b) = (first.components(), second.components());
    let dot: f64 = (0..4).map(|index| a[index] * b[index]).sum();
    2.0 * libm::acos(dot.abs().min(1.0))
}

#[test]
fn a_ship_moved_or_turned_or_sped_by_something_else_while_a_planet_carries_it_goes_on_from_there() {
    let mut scene = a_scene(None);
    scene.ticks(1);
    // A raft a little over the ground of the pole, let fall and rest, going with the planet.
    let raft = raft_over(&mut scene, [0.0, 10_040.0, 0.0], false, [0.0; 3]);
    scene.ticks(120);
    let resting = from_the_middle(&scene, raft);
    assert!(
        resting[0].abs() < 15.0
            && resting[2].abs() < 15.0
            && (9_950.0..10_045.0).contains(&resting[1]),
        "resting on the pole: {resting:?}"
    );
    let turn_at_rest = ship_pose(&scene, raft).orientation;

    // Moved fifty voxels aside and thirty up (as a command might, from the pose the ship has in the tick), and nothing else: in two ticks it is there, and has not
    // gone back to where the frame had it, and has not turned.
    scene.run(move |world| {
        let pose = world.data().grids().get(ship_id(raft)).unwrap().pose;
        let aside = GridPose::new(
            pose.position.translated([50.0, 30.0, 0.0]),
            pose.orientation,
        );
        world.set_grid_pose(ship_id(raft), aside).unwrap();
    });
    scene.ticks(2);
    let moved = from_the_middle(&scene, raft);
    assert!(
        length(minus(minus(moved, resting), [50.0, 30.0, 0.0])) < 1.0,
        "{resting:?} then {moved:?}"
    );
    assert!(angle_between(ship_pose(&scene, raft).orientation, turn_at_rest) < 1e-3);

    // Turned a quarter about the vertical, and nothing else: it keeps the turn, where it is.
    let turn = Orientation::from_yaw_pitch(1.5, 0.0);
    scene.run(move |world| {
        let pose = world.data().grids().get(ship_id(raft)).unwrap().pose;
        world
            .set_grid_pose(ship_id(raft), GridPose::new(pose.position, turn))
            .unwrap();
    });
    scene.ticks(2);
    assert!(angle_between(ship_pose(&scene, raft).orientation, turn) < 1e-3);
    let turned = from_the_middle(&scene, raft);
    assert!(
        length(minus(turned, moved)) < 1.5,
        "{moved:?} then {turned:?}"
    );

    // Given a spin of two radians a second about the vertical, and nothing else (it is in the air still, thirty voxels up): half a second later it has turned a radian.
    let before_spinning = ship_pose(&scene, raft).orientation;
    let ground = velocity(&scene, WANDERER);
    let kept = velocity(&scene, raft);
    scene.run(move |world| {
        world
            .set_grid_velocity(ship_id(raft), kept.map(|part| part as f32), [0.0, 2.0, 0.0])
            .unwrap();
    });
    scene.ticks(9);
    let turned_by_spin = angle_between(ship_pose(&scene, raft).orientation, before_spinning);
    assert!(
        (turned_by_spin - 1.0).abs() < 0.15,
        "{turned_by_spin} radians in half a second"
    );
    // It falls to the ground there, going with the planet.
    scene.ticks(120);
    let landed = from_the_middle(&scene, raft);
    assert!(
        (9_950.0..10_045.0).contains(&landed[1]),
        "on the ground: {landed:?}"
    );
    let against = length(minus(velocity(&scene, raft), ground));
    assert!(
        against < 1.0,
        "{against} metres a second against the planet"
    );

    // A new velocity set from outside is the velocity it has: up at ten metres a second against the planet, it is, a second later, a pull of eight less than ten
    // metres higher a second after.
    let height = from_the_middle(&scene, raft)[1];
    let ground = velocity(&scene, WANDERER);
    scene.run(move |world| {
        world
            .set_grid_velocity(
                ship_id(raft),
                [ground[0] as f32, ground[1] as f32 + 10.0, ground[2] as f32],
                [0.0; 3],
            )
            .unwrap();
    });
    scene.ticks(19);
    let risen = from_the_middle(&scene, raft)[1] - height;
    assert!((risen - 6.0).abs() < 1.5, "{risen} voxels up in a second");
}

#[test]
fn two_ships_that_dock_over_a_planet_on_a_fast_orbit_become_one_that_goes_with_the_planet() {
    let mut scene = a_scene(None);
    scene.ticks(1);
    // (Each over the planet where it is when the ship is made: it goes six voxels along its orbit in a tick.)
    let over_the_pole = |scene: &Scene, x: f64| {
        let planet = voxels(scene, WANDERER);
        let place = [planet[0] + x, planet[1] + 10_200.0, planet[2]];
        GridPose::new(WorldPosition::from_voxels(place), Orientation::IDENTITY)
    };
    let (first, second) = a_pair();
    let at = over_the_pole(&scene, 0.0);
    make(&mut scene, at, false, first);
    // Three voxels between the faces of their ports.
    let at = over_the_pole(&scene, 7.0);
    make(&mut scene, at, false, second);
    // At rest against the ground, as ships made by someone standing on it would be.
    for number in [THE_RAFT, THE_RAFT + 1] {
        let speed = velocity(&scene, WANDERER).map(|part| part as f32);
        scene.run(move |world| {
            world
                .set_grid_velocity(ship_id(number), speed, [0.0; 3])
                .unwrap();
        });
    }
    scene.ticks(100);
    let ships: Vec<_> = scene
        .rig
        .server
        .world()
        .grids()
        .iter()
        .map(|(id, _)| id)
        .filter(|id| *id != ship_id(0) && *id != ship_id(WANDERER))
        .collect();
    assert_eq!(ships.len(), 1, "the two docked: {ships:?}");
    let whole = ships[0];
    let ship = scene.rig.server.world().grids().get(whole).unwrap();
    let against = minus(
        ship.linear_velocity.map(f64::from),
        velocity(&scene, WANDERER),
    );
    // They have fallen for some four seconds (a pull of 8): and no more has happened to the speed they share than that, sideways or along the orbit.
    assert!(
        against[0].abs() < 1.0
            && against[2].abs() < 1.0
            && against[1] < -20.0
            && against[1] > -40.0,
        "{against:?} against the planet"
    );
    let offset = minus(ship.pose.position.in_voxels(), voxels(&scene, WANDERER));
    assert!(
        offset[0].abs() < 20.0 && offset[2].abs() < 20.0 && offset[1] < 10_190.0,
        "{offset:?} from the middle of the planet"
    );
}

#[test]
fn blocks_built_in_space_near_a_planet_on_a_fast_orbit_are_not_solid_to_what_goes_with_the_planet()
{
    let mut scene = a_scene(None);
    scene.ticks(1);
    // A slab of stone, 40 by 40, in the level (not on the planet) eighty voxels over the pole, where the world is: it stands still in the world.
    let planet = voxels(&scene, WANDERER);
    let corner = [
        planet[0] as i64 - 20,
        planet[1] as i64 + 10_100,
        planet[2] as i64 - 20,
    ];
    let content = CaveContent::new();
    scene.run(move |world| {
        for x in 0..40 {
            for z in 0..40 {
                world
                    .grid(GridId::root(LevelIndex::MAIN))
                    .unwrap()
                    .set(
                        content.block_channel(),
                        VoxelCoordinate::new(corner[0] + x, corner[1], corner[2] + z),
                        content.blocks.stone,
                    )
                    .unwrap();
            }
        }
    });
    // A raft dropped over it, sixty voxels higher, goes with the planet and falls past the slab, which the planet leaves behind, to the ground.
    let raft = raft_over(&mut scene, [0.0, 10_160.0, 0.0], false, [0.0; 3]);
    scene.ticks(250);
    let offset = from_the_middle(&scene, raft);
    assert!(
        (9_950.0..10_045.0).contains(&offset[1]),
        "on the ground of the planet and not on the slab: {offset:?}"
    );
}

#[test]
fn a_ship_that_goes_far_from_the_ships_it_was_with_in_the_sphere_of_a_planet_is_simulated_in_a_frame_of_its_own()
 {
    // A planet whose ground reaches 1,500 bricks, so that its sphere is 72,000 voxels, and that pulls at a half (an escape at 100 metres a second): a ship thrown along
    // the ground at 400 is 33 kilometres from the one it was with in 83 seconds, farther than a frame reaches (twice 500 bricks, 32,000 voxels).
    let mut content = CaveContent::new();
    content
        .apply_packs(&wanderer_pack(&orbit_of_the_home_planet(None), 1_500, 0.5))
        .unwrap();
    let world = content.new_world(SEED);
    let (mut scene, report) = Scene::reporting_in(content, world);
    scene.ticks(1);
    let stays = raft_over(&mut scene, [0.0, 10_100.0, 0.0], true, [0.0; 3]);
    let goes = raft_over(&mut scene, [20.0, 10_100.0, 0.0], false, [400.0, 0.0, 0.0]);
    scene.ticks(100);
    assert_eq!(report().frames, 1, "together at first");
    scene.ticks(1_800);
    let apart = length(minus(voxels(&scene, goes), voxels(&scene, stays)));
    assert!(
        apart > 33_000.0 && apart < 72_000.0,
        "{apart} voxels apart, in the sphere"
    );
    assert_eq!(report().frames, 2, "a frame for each");
}
