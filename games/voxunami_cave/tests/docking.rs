//! Docking in Cave: ships with dev docking ports are drawn together, turned into line, snapped and made one ship; and a docked pair of ports can be
//! cut apart again unless a weld holds the two sides.

mod common;

use std::collections::BTreeMap;

use block_models::Shape;
use common::{Scene, ship_id};
use game_shell::GameContent;
use server::GridView;
use voxel_space::{
    BrickCoordinate, FaceDirection, GravitySettings, GridId, GridPose, LevelIndex, LocalVoxelIndex,
    Orientation, VoxelCoordinate, WorldPosition,
};
use voxunami_cave::content::{CaveContent, dev_docking_port_id};
use voxunami_cave::placement::turn_carrying;

/// How a block is made in a ship: its state, and which way it is turned (the number of a turn of the lattice).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Look {
    state: u32,
    orientation: u32,
}

/// A block as it is made, not turned.
const PLAIN: Look = Look {
    state: 0,
    orientation: 0,
};

/// A dev docking port turned so that its yellow face (the east, as it is made) is this one.
fn facing(face: FaceDirection) -> Look {
    Look {
        state: 0,
        orientation: u32::from(turn_carrying(FaceDirection::PositiveX, face).index()),
    }
}

fn port_block() -> u32 {
    CaveContent::new().registries()[0]
        .1
        .raw_id_of(&dev_docking_port_id())
        .expect("Cave has the dev docking port")
}

fn stone() -> u32 {
    CaveContent::new().blocks.stone
}

/// A block of a ship: where it is in the ship, which block, and how it is made.
type Placed = ([i64; 3], u32, Look);

fn stones(from: i64, to: i64) -> Vec<Placed> {
    (from..to).map(|x| ([x, 0, 0], stone(), PLAIN)).collect()
}

fn pose(x: f32, y: f32, z: f32, orientation: Orientation) -> GridPose {
    GridPose::new(
        WorldPosition::new(BrickCoordinate::ORIGIN, [x, y, z]),
        orientation,
    )
}

/// Makes a ship of these blocks at this pose, held if asked.
fn make(scene: &mut Scene, at: GridPose, held: bool, blocks: Vec<Placed>) {
    let content = CaveContent::new();
    scene.run(move |world| {
        let grid = world.create_grid(LevelIndex::MAIN, at).unwrap();
        world.set_grid_held(grid, held).unwrap();
        let mut view = world.grid(grid).unwrap();
        for ([x, y, z], block, look) in blocks {
            let voxel = VoxelCoordinate::new(x, y, z);
            view.set(content.state_channel(), voxel, look.state)
                .unwrap();
            view.set(content.orientation_channel(), voxel, look.orientation)
                .unwrap();
            view.set(content.block_channel(), voxel, block).unwrap();
        }
    });
}

/// No gravity: ships hang in the air where they are put.
fn weightless(scene: &mut Scene) {
    scene.run(|world| {
        world.set_gravity_settings(GravitySettings {
            base: [0.0; 3],
            ..GravitySettings::default()
        });
    });
}

fn ships(scene: &Scene) -> Vec<GridId> {
    scene
        .rig
        .server
        .world()
        .grids()
        .iter()
        .map(|(id, _)| id)
        .collect()
}

fn mass_of(scene: &Scene, id: GridId) -> f64 {
    f64::from(scene.rig.server.world().grids().get(id).unwrap().mass)
}

fn velocity_of(scene: &Scene, id: GridId) -> [f64; 3] {
    scene
        .rig
        .server
        .world()
        .grids()
        .get(id)
        .unwrap()
        .linear_velocity
        .map(f64::from)
}

/// A ship with a port on its east face at the end of a row of stone, and one with a port on its west face at the start: 4 and 3 blocks.
fn a_pair() -> (Vec<Placed>, Vec<Placed>) {
    let mut first = stones(0, 3);
    first.push(([3, 0, 0], port_block(), facing(FaceDirection::PositiveX)));
    let mut second = vec![([0, 0, 0], port_block(), facing(FaceDirection::NegativeX))];
    second.extend(stones(1, 3));
    (first, second)
}

#[test]
fn the_dev_docking_port_is_gunmetal_gray_with_one_yellow_face_that_docks_and_exists_only_in_creative()
 {
    use voxunami_cave::items::{creative_only_blocks, placeable_blocks};
    let content = CaveContent::new();
    let package = content.package_data();
    let id = dev_docking_port_id();
    assert!(placeable_blocks(&package).contains(&id));
    assert_eq!(creative_only_blocks(&package), std::slice::from_ref(&id));
    let table = content.block_table();
    let block = port_block();
    // One state, and the yellow face is the one that docks, wherever the port is turned to: all 24 turns.
    assert_eq!(table.state_count(block), 1);
    let palette = table.palette();
    for face in FaceDirection::ALL {
        let orientation = facing(face).orientation;
        assert_eq!(table.docking_face(block, 0, orientation), Some(face));
        let mark = table.face_mark(block, 0, orientation).unwrap();
        assert_eq!(mark.face, face);
        assert_eq!(palette[mark.material as usize], [1.0, 216.0 / 255.0, 0.0]);
        for other in FaceDirection::ALL
            .into_iter()
            .filter(|other| *other != face)
        {
            assert_eq!(table.material_of_face(block, 0, orientation, other), block);
        }
    }
    // Gunmetal gray: dark, a little blue, and not yellow.
    let [red, green, blue] = table.color(block);
    assert!(red < 0.3 && green < 0.3 && blue < 0.3 && blue >= red);
    assert!(table.shape(table.model_of(block, 0, 0)).is_full());
}

#[test]
fn ports_that_face_each_other_pull_the_ships_together_with_the_same_force_and_the_lighter_one_speeds_up_more()
 {
    let mut scene = Scene::new();
    weightless(&mut scene);
    let (first, second) = a_pair();
    make(
        &mut scene,
        pose(20.0, 230.0, 20.0, Orientation::IDENTITY),
        false,
        first,
    );
    // 3 metres of gap between the two port faces (the first's is at x = 24).
    make(
        &mut scene,
        pose(27.0, 230.0, 20.0, Orientation::IDENTITY),
        false,
        second,
    );
    // Let the bodies be made and weighed, and the pull begin.
    scene.ticks(2);
    let (a, b) = (ship_id(0), ship_id(1));
    let (mass_a, mass_b) = (mass_of(&scene, a), mass_of(&scene, b));
    assert_eq!((mass_a, mass_b), (4000.0, 3000.0));
    // In one tick, a twentieth of a second, each port is pulled by the same force (10,000 newtons) toward the other: the change in momentum is
    // the force times the tick (500 newton seconds), whatever the mass, and the changes of speed are that over the masses.
    let impulse = grid_bodies::DockingSettings::default().attraction_force / 20.0;
    assert_eq!(impulse, 500.0);
    let (before_a, before_b) = (velocity_of(&scene, a)[0], velocity_of(&scene, b)[0]);
    scene.ticks(1);
    let (speed_a, speed_b) = (
        velocity_of(&scene, a)[0] - before_a,
        velocity_of(&scene, b)[0] - before_b,
    );
    assert!(speed_a > 0.0 && speed_b < 0.0, "toward each other");
    assert!(
        (speed_a * mass_a - impulse).abs() < 10.0,
        "{}",
        speed_a * mass_a
    );
    assert!(
        (-speed_b * mass_b - impulse).abs() < 10.0,
        "{}",
        -speed_b * mass_b
    );
    assert!(
        -speed_b > speed_a,
        "the lighter ship is the one that is moved more"
    );
}

#[test]
fn ships_that_are_far_or_face_away_or_face_the_same_way_do_not_move_toward_each_other() {
    let mut scene = Scene::new();
    weightless(&mut scene);
    let (first, second) = a_pair();
    // Far: 9 metres of gap.
    make(
        &mut scene,
        pose(20.0, 230.0, 20.0, Orientation::IDENTITY),
        false,
        first.clone(),
    );
    make(
        &mut scene,
        pose(33.0, 230.0, 20.0, Orientation::IDENTITY),
        false,
        second.clone(),
    );
    // Behind: the second is on the side of the first's west end, its port facing away.
    make(
        &mut scene,
        pose(20.0, 250.0, 20.0, Orientation::IDENTITY),
        false,
        first.clone(),
    );
    make(
        &mut scene,
        pose(12.0, 250.0, 20.0, Orientation::IDENTITY),
        false,
        second.clone(),
    );
    // Both east: ports that look the same way.
    let mut both_east = second;
    both_east[0] = ([0, 0, 0], port_block(), facing(FaceDirection::PositiveX));
    make(
        &mut scene,
        pose(20.0, 270.0, 20.0, Orientation::IDENTITY),
        false,
        first,
    );
    make(
        &mut scene,
        pose(27.0, 270.0, 20.0, Orientation::IDENTITY),
        false,
        both_east,
    );
    scene.ticks(30);
    for id in (0..6).map(ship_id) {
        assert_eq!(velocity_of(&scene, id), [0.0; 3], "{id:?}");
    }
    assert_eq!(ships(&scene).len(), 6);
}

/// Every block of a ship: where, which block, its state and its orientation.
fn blocks_of(scene: &Scene, id: GridId) -> BTreeMap<VoxelCoordinate, (u32, u32, u32)> {
    let content = CaveContent::new();
    let ship = scene.rig.server.world().grids().get(id).unwrap();
    let mut blocks = BTreeMap::new();
    for brick in ship.voxels.sorted_brick_coordinates() {
        let contents = ship.voxels.brick(brick).unwrap();
        for local in LocalVoxelIndex::all() {
            let block = contents.get(content.block_channel(), local);
            if block != content.blocks.air {
                blocks.insert(
                    brick.voxel_at(local),
                    (
                        block,
                        contents.get(content.state_channel(), local),
                        contents.get(content.orientation_channel(), local),
                    ),
                );
            }
        }
    }
    blocks
}

/// Ticks until there is one ship, or `limit` ticks have gone.
fn until_one_ship(scene: &mut Scene, limit: usize) -> usize {
    let mut ticks = 0;
    while ships(scene).len() > 1 && ticks < limit {
        scene.ticks(1);
        ticks += 1;
    }
    ticks
}

#[test]
fn ships_that_meet_head_on_dock_and_become_one_ship_that_goes_as_their_momentum_says_and_does_not_turn()
 {
    let mut scene = Scene::new();
    weightless(&mut scene);
    let (first, second) = a_pair();
    make(
        &mut scene,
        pose(20.0, 230.0, 20.0, Orientation::IDENTITY),
        false,
        first,
    );
    // The first goes toward where the second will be at a metre a second (it is set going before the second is there to pull it, or what the
    // pull had given it would be lost).
    scene.run(|world| {
        world
            .set_grid_velocity(ship_id(0), [1.0, 0.0, 0.0], [0.0; 3])
            .unwrap();
    });
    make(
        &mut scene,
        pose(28.0, 230.0, 20.0, Orientation::IDENTITY),
        false,
        second,
    );
    let ticks = until_one_ship(&mut scene, 600);
    let merged = ships(&scene);
    assert_eq!(
        merged.len(),
        1,
        "after {ticks} ticks there are still two ships"
    );
    let id = merged[0];
    // All seven blocks, the lattice transform of the second's: it is one block from the first's last.
    let blocks = blocks_of(&scene, id);
    assert_eq!(blocks.len(), 7);
    // The momentum: 4,000 kilogram metres a second was all there was (the pull is equal and opposite), now carried by 7,000 kilograms.
    let speed = velocity_of(&scene, id);
    assert!((speed[0] - 4000.0 / 7000.0).abs() < 0.01, "{speed:?}");
    assert!(speed[1].abs() < 1e-3 && speed[2].abs() < 1e-3, "{speed:?}");
    let ship = scene.rig.server.world().grids().get(id).unwrap();
    assert!(
        ship.angular_velocity.iter().all(|turn| turn.abs() < 1e-3),
        "{:?} after {ticks}",
        ship.angular_velocity
    );
    // The docked ports are one pair.
    let docking = voxunami_cave::plugins::docking(&CaveContent::new());
    assert_eq!(docking.docked_pairs(ship).len(), 1);
}

#[test]
fn ships_that_are_off_to_the_side_and_turned_a_little_are_pulled_and_twisted_into_line_and_dock() {
    let mut scene = Scene::new();
    weightless(&mut scene);
    let (first, second) = a_pair();
    make(
        &mut scene,
        pose(20.0, 230.0, 20.0, Orientation::IDENTITY),
        false,
        first,
    );
    scene.run(|world| {
        world
            .set_grid_velocity(ship_id(0), [1.0, 0.0, 0.0], [0.0; 3])
            .unwrap();
    });
    // Off to the side by most of a metre, and turned 12 degrees about the vertical.
    make(
        &mut scene,
        pose(28.0, 230.6, 20.8, Orientation::from_yaw_pitch(0.2, 0.0)),
        false,
        second,
    );
    let ticks = until_one_ship(&mut scene, 900);
    let merged = ships(&scene);
    assert_eq!(
        merged.len(),
        1,
        "after {ticks} ticks there are still two ships"
    );
    assert_eq!(blocks_of(&scene, merged[0]).len(), 7);
    let speed = velocity_of(&scene, merged[0]);
    assert!((speed[0] - 4000.0 / 7000.0).abs() < 0.02, "{speed:?}");
}

/// The first ship with a port on its east face (as `a_pair`'s), and a second whose port faces west, rolled about the docking axis by `quarters`
/// quarter turns (an eighth is `0.5`), with a bottom slab of stone beside its port, at a gap of 2 metres. Weightless.
fn rolled_pair(quarters: f32) -> Scene {
    let mut scene = Scene::new();
    weightless(&mut scene);
    let (first, _) = a_pair();
    make(
        &mut scene,
        pose(20.0, 230.0, 20.0, Orientation::IDENTITY),
        false,
        first,
    );
    // The second ship: its port, then a bottom slab (state 1) and a stone.
    let second = vec![
        ([0, 0, 0], port_block(), facing(FaceDirection::NegativeX)),
        (
            [1, 0, 0],
            stone(),
            Look {
                state: 1,
                orientation: 0,
            },
        ),
        ([2, 0, 0], stone(), PLAIN),
    ];
    // Rolled about x by the angle: the middle of its port face, 0.5 along y and z from its origin, is then at the offset the roll gives.
    let angle = quarters * std::f32::consts::FRAC_PI_2;
    let (sine, cosine) = (libm::sinf(angle), libm::cosf(angle));
    let (y, z) = (0.5 * cosine - 0.5 * sine, 0.5 * sine + 0.5 * cosine);
    // The first's east face is at x = 24, 230.5, 20.5; the second's port face is to be there, 2 metres further along.
    let origin = (26.0, 230.5 - y, 20.5 - z);
    let half = f64::from(angle) / 2.0;
    let orientation =
        Orientation::from_components([libm::sin(half), 0.0, 0.0, libm::cos(half)]).unwrap();
    make(
        &mut scene,
        pose(origin.0, origin.1, origin.2, orientation),
        false,
        second,
    );
    scene
}

#[test]
fn a_ship_docks_whatever_its_roll_about_the_docking_axis_and_its_slab_is_the_same_slab_turned_with_it()
 {
    for quarters in [0.0, 1.0, 2.0, 3.0, 0.5, 1.5, -0.4] {
        let mut scene = rolled_pair(quarters);
        let ticks = until_one_ship(&mut scene, 900);
        let merged = ships(&scene);
        assert_eq!(
            merged.len(),
            1,
            "roll {quarters}: still two ships after {ticks} ticks"
        );
        let blocks = blocks_of(&scene, merged[0]);
        assert_eq!(blocks.len(), 4 + 3, "roll {quarters}");
        // The slab was a bottom slab (state 1) of the second ship: it is the same slab in the whole, turned as the ship was turned onto the lattice
        // of the first, whichever way that is (every block can stand every way).
        let (_, transform) = scene.rig.server.world().grids().resolve(ship_id(1))[0];
        let slab_at = transform.apply(VoxelCoordinate::new(1, 0, 0)).unwrap();
        let (block, state, orientation) = blocks[&slab_at];
        assert_eq!(block, stone(), "roll {quarters}");
        assert_eq!(state, 1, "roll {quarters}");
        // The block itself was not turned in its ship, so what it keeps is the number of the turn that carried the ship onto the lattice.
        assert_eq!(
            orientation,
            u32::from(transform.rotation.index()),
            "roll {quarters}"
        );
        let table = CaveContent::new().block_table();
        let slab = table.shape(table.model_of(block, state, orientation));
        assert!(
            slab.same_cells(&Shape::half(FaceDirection::NegativeY).turned(transform.rotation)),
            "roll {quarters}: orientation {orientation} is not the slab turned by {:?}",
            transform.rotation
        );
        // The ports are docked, and aligned to the lattice of the first.
        let docking = voxunami_cave::plugins::docking(&CaveContent::new());
        let ship = scene.rig.server.world().grids().get(merged[0]).unwrap();
        assert_eq!(docking.docked_pairs(ship).len(), 1, "roll {quarters}");
    }
}

#[test]
fn a_held_ship_stays_where_it_is_and_the_other_comes_to_it_and_the_whole_is_held_and_still() {
    let mut scene = Scene::new();
    weightless(&mut scene);
    let (first, second) = a_pair();
    make(
        &mut scene,
        pose(20.0, 230.0, 20.0, Orientation::IDENTITY),
        true,
        first,
    );
    make(
        &mut scene,
        pose(27.0, 230.4, 20.3, Orientation::IDENTITY),
        false,
        second,
    );
    let held_pose = scene
        .rig
        .server
        .world()
        .grids()
        .get(ship_id(0))
        .unwrap()
        .pose;
    let ticks = until_one_ship(&mut scene, 900);
    let merged = ships(&scene);
    assert_eq!(merged.len(), 1, "after {ticks} ticks");
    let ship = scene.rig.server.world().grids().get(merged[0]).unwrap();
    assert!(ship.held);
    assert_eq!(ship.pose, held_pose, "the held ship did not move");
    assert_eq!(ship.linear_velocity, [0.0; 3]);
    assert_eq!(ship.angular_velocity, [0.0; 3]);
    assert_eq!(blocks_of(&scene, merged[0]).len(), 7);
    // Nothing moves a held ship that has docked.
    scene.ticks(20);
    assert_eq!(
        scene
            .rig
            .server
            .world()
            .grids()
            .get(merged[0])
            .unwrap()
            .pose,
        held_pose
    );
}

#[test]
fn two_held_ships_do_not_dock_for_nothing_would_move_them() {
    let mut scene = Scene::new();
    weightless(&mut scene);
    let (first, second) = a_pair();
    make(
        &mut scene,
        pose(20.0, 230.0, 20.0, Orientation::IDENTITY),
        true,
        first,
    );
    make(
        &mut scene,
        pose(27.0, 230.0, 20.0, Orientation::IDENTITY),
        true,
        second,
    );
    scene.ticks(100);
    assert_eq!(ships(&scene).len(), 2);
}

#[test]
fn ships_near_enough_and_in_line_that_cannot_be_joined_are_left_two_and_tried_again_only_now_and_then()
 {
    // The second is a quarter of a metre from where it docks, and one of its blocks would be in the place of one of the first's, above the ports.
    // (That one of the first's is a flame, which bodies pass through, so that nothing pushes the ships apart and they stay as near as they were put.)
    // Nothing can be done about that, so nothing is: they stay two ships, and the attempt (which works out the whole of the smaller ship) is not
    // made every tick. (That the second ship is put back where it was after the attempt is not shown here: the bodies move it afterwards.)
    let (mut scene, report) = Scene::reporting();
    weightless(&mut scene);
    let content = CaveContent::new();
    let (mut first, mut second) = a_pair();
    first.push(([4, 1, 0], content.blocks.fire, PLAIN));
    second.push(([0, 1, 0], stone(), PLAIN));
    make(
        &mut scene,
        pose(20.0, 230.0, 20.0, Orientation::IDENTITY),
        true,
        first,
    );
    make(
        &mut scene,
        pose(24.25, 230.0, 20.0, Orientation::IDENTITY),
        false,
        second,
    );
    scene.ticks(100);
    assert_eq!(ships(&scene).len(), 2);
    let report = report();
    assert_eq!(report.dockings_made, 0);
    // Tried, refused, and left alone for a second (20 ticks) each time: five or six tries in 100 ticks, not a hundred.
    assert!(
        (2..=6).contains(&report.dockings_refused),
        "{} refusals",
        report.dockings_refused
    );
}

/// A deck, a player on it, and a small dynamic ship with a port that is to dock to the end of the deck. Weightless from the moment the player has
/// settled on the deck.
fn a_player_on_a_deck_with_a_ship_to_dock() -> (Scene, common::Player) {
    use common::{scene_with_a_deck, tick_and_follow, walker_on_the_deck};
    let mut scene = scene_with_a_deck();
    let mut player = walker_on_the_deck(&mut scene);
    // A port at the east end of the deck (the deck is ship 0: 30 along x, from x = 5, 2 thick above the floor + 3).
    scene.run(|world| {
        let content = CaveContent::new();
        let mut deck = world.grid(ship_id(0)).unwrap();
        let end = VoxelCoordinate::new(29, 1, 3);
        deck.set(
            content.orientation_channel(),
            end,
            facing(FaceDirection::PositiveX).orientation,
        )
        .unwrap();
        deck.set(content.block_channel(), end, port_block())
            .unwrap();
    });
    weightless(&mut scene);
    // The ship to dock: its port face is to be 2.5 metres beyond the end of the deck, on the level of the port.
    let level = common::FLOOR + 4.0;
    let (_, second) = a_pair();
    make(
        &mut scene,
        pose(37.5, level, 23.0, Orientation::IDENTITY),
        false,
        second,
    );
    tick_and_follow(&mut scene, &mut player, 2);
    (scene, player)
}

#[test]
fn a_player_aboard_a_ship_that_docks_is_aboard_the_whole_where_they_were_and_still_at_the_controls()
{
    use common::{eyes_in_the_world, player_pose, say, tick_and_follow};
    let (mut scene, mut player) = a_player_on_a_deck_with_a_ship_to_dock();
    assert!(say(&mut scene, &mut player, "/ship pilot").starts_with("you pilot ship 0"));
    let before = eyes_in_the_world(&scene, 1);
    let mut ticks = 0;
    while ships(&scene).len() > 1 && ticks < 900 {
        tick_and_follow(&mut scene, &mut player, 1);
        ticks += 1;
    }
    let merged = ships(&scene);
    assert_eq!(merged.len(), 1, "after {ticks} ticks");
    let frame = player_pose(&scene, 1).grid;
    assert_eq!(frame, merged[0], "aboard the whole");
    assert_ne!(frame, ship_id(0));
    // Where they were in the world, to within the little the other ship was moved (the deck is held: not at all).
    let after = eyes_in_the_world(&scene, 1);
    assert!(
        (0..3).all(|axis| (after[axis] - before[axis]).abs() < 0.05),
        "{before:?} to {after:?}"
    );
    // At the controls still: letting go says which ship it is now.
    let reply = say(&mut scene, &mut player, "/ship pilot");
    assert_eq!(
        reply,
        format!(
            "you let go of the controls of ship {}",
            merged[0].get() - GridId::FIRST_ALLOCATED
        )
    );
}

/// A flat floor, a player hovering near it, and two ships 10 above the floor, weightless, the second on its way to dock with the first.
fn a_player_and_two_ships(first: Vec<Placed>, second: Vec<Placed>) -> (Scene, common::Player) {
    use common::{FLOOR, flat_floor};
    let mut scene = Scene::new();
    flat_floor(&mut scene);
    let mut player = scene
        .rig
        .join([22.0, f64::from(FLOOR) + 5.0, 30.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    weightless(&mut scene);
    let level = FLOOR + 10.0;
    make(
        &mut scene,
        pose(20.0, level, 20.0, Orientation::IDENTITY),
        false,
        first,
    );
    make(
        &mut scene,
        pose(27.0, level, 20.0, Orientation::IDENTITY),
        false,
        second,
    );
    (scene, player)
}

/// The mass, and the speed of the middle of the mass, of every ship, added: the momentum.
fn momentum(scene: &Scene) -> [f64; 3] {
    let mut total = [0.0; 3];
    for id in ships(scene) {
        let (mass, speed) = (mass_of(scene, id), velocity_of(scene, id));
        for axis in 0..3 {
            total[axis] += mass * speed[axis];
        }
    }
    total
}

#[test]
fn undocking_cuts_the_ship_where_the_ports_meet_into_two_that_part_slowly_keep_the_momentum_and_do_not_dock_again_until_apart()
 {
    use common::{say, tick_and_follow};
    let (first, second) = a_pair();
    let (mut scene, mut player) = a_player_and_two_ships(first, second);
    let mut ticks = 0;
    while ships(&scene).len() > 1 && ticks < 900 {
        tick_and_follow(&mut scene, &mut player, 1);
        ticks += 1;
    }
    assert_eq!(ships(&scene).len(), 1, "docked after {ticks} ticks");
    let whole = ships(&scene)[0];
    let reply = say(&mut scene, &mut player, "/ship undock");
    assert!(
        reply.contains(&format!(
            "ship {} came apart into ships",
            whole.get() - GridId::FIRST_ALLOCATED
        )),
        "{reply}"
    );
    let parts = ships(&scene);
    assert_eq!(parts.len(), 2);
    // Four blocks and three, as they were.
    let mut sizes: Vec<usize> = parts
        .iter()
        .map(|id| blocks_of(&scene, *id).len())
        .collect();
    sizes.sort();
    assert_eq!(sizes, [3, 4]);
    // They move apart at the speed the settings say, and the momentum is what it was (none, here: the ships began at rest and are pulled
    // by equal and opposite forces).
    scene.ticks(2);
    let speed_apart = {
        let (low, high) = if parts[0] < parts[1] {
            (parts[0], parts[1])
        } else {
            (parts[1], parts[0])
        };
        // The side of the first port (the first of the two ships made, the lower number) goes back and the other forward: the second is east
        // of the first, so it goes faster in x, by the whole of the speed.
        velocity_of(&scene, high)[0] - velocity_of(&scene, low)[0]
    };
    let wanted = grid_bodies::DockingSettings::default().separation_speed;
    assert!(
        (speed_apart - wanted).abs() < 0.01,
        "{speed_apart} against {wanted}"
    );
    let total = momentum(&scene);
    assert!(
        total.iter().all(|component| component.abs() < 5.0),
        "{total:?}"
    );
    // In range of each other and facing, but not pulled, for they were just undocked: still two ships a good while later.
    scene.ticks(60);
    assert_eq!(ships(&scene).len(), 2);
    // Once they are farther apart than the range, they are let go; brought back within it they dock again.
    scene.ticks(300);
    let parts = ships(&scene);
    assert_eq!(parts.len(), 2);
    scene.run(move |world| {
        for id in &parts {
            world.set_grid_velocity(*id, [0.0; 3], [0.0; 3]).unwrap();
        }
    });
    let (low, high) = {
        let parts = ships(&scene);
        (parts[0].min(parts[1]), parts[0].max(parts[1]))
    };
    let gap = |scene: &Scene| {
        let (a, b) = (
            scene
                .rig
                .server
                .world()
                .grids()
                .get(low)
                .unwrap()
                .pose
                .position,
            scene
                .rig
                .server
                .world()
                .grids()
                .get(high)
                .unwrap()
                .pose
                .position,
        );
        b.relative_to(a)[0].abs()
    };
    assert!(gap(&scene) > 7.0, "{}", gap(&scene));
    // Bring the second side back to 3 metres from the first's: they were one ship, so the origins of the two sides were together when they
    // were docked, and ports are docked when the origins are.
    scene.run(move |world| {
        let near = world.data().grids().get(low).unwrap().pose;
        let offset = near.position.offset();
        let brought = GridPose::new(
            WorldPosition::new(
                near.position.brick(),
                [offset[0] + 3.0, offset[1], offset[2]],
            ),
            near.orientation,
        );
        world.set_grid_pose(high, brought).unwrap();
    });
    let ticks = until_one_ship(&mut scene, 900);
    assert_eq!(ships(&scene).len(), 1, "docked again after {ticks} ticks");
    let _ = player;
}

#[test]
fn ships_welded_by_blocks_beside_the_ports_cannot_be_undocked_until_the_weld_is_broken() {
    use common::{say, tick_and_follow};
    let (mut first, mut second) = a_pair();
    // A stone above each port: once docked, the two are beside each other, and join the ships whatever the ports do.
    first.push(([3, 1, 0], stone(), PLAIN));
    second.push(([0, 1, 0], stone(), PLAIN));
    let (mut scene, mut player) = a_player_and_two_ships(first, second);
    let mut ticks = 0;
    while ships(&scene).len() > 1 && ticks < 900 {
        tick_and_follow(&mut scene, &mut player, 1);
        ticks += 1;
    }
    let whole = ships(&scene);
    assert_eq!(whole.len(), 1, "docked after {ticks} ticks");
    let number = whole[0].get() - GridId::FIRST_ALLOCATED;
    let reply = say(&mut scene, &mut player, "/ship undock");
    assert!(
        reply.contains(&format!("ship {number} is welded there")),
        "{reply}"
    );
    assert_eq!(ships(&scene), whole, "nothing changed");
    // Break one of the two stones that join them (the second's, which is at (4, 1, 0) of the whole): they can be parted.
    let content = CaveContent::new();
    let id = whole[0];
    scene.run(move |world| {
        world
            .grid(id)
            .unwrap()
            .set(
                content.block_channel(),
                VoxelCoordinate::new(4, 1, 0),
                content.blocks.air,
            )
            .unwrap();
    });
    let reply = say(&mut scene, &mut player, "/ship undock");
    assert!(reply.contains("came apart into ships"), "{reply}");
    assert_eq!(ships(&scene).len(), 2);
}

#[test]
fn a_command_to_undock_on_a_ship_with_nothing_docked_says_so() {
    use common::say;
    let (first, second) = a_pair();
    let (mut scene, mut player) = a_player_and_two_ships(first, second);
    // The ships are two, 3 metres apart, not docked yet (a tick or two is all that has gone).
    let reply = say(&mut scene, &mut player, "/ship undock");
    assert!(reply.contains("has no ships docked to it"), "{reply}");
    assert_eq!(ships(&scene).len(), 2);
}

/// Stone blocks along x from `from` to `to` (not including it), at this height and depth.
fn stones_at_height(from: i64, to: i64, y: i64, z: i64) -> Vec<Placed> {
    (from..to).map(|x| ([x, y, z], stone(), PLAIN)).collect()
}

/// The first ship (held) has a wall of wood beside its port, 6 wide and 5 high, and nine fires beside it; the second has a wall of wood just like it
/// beside its own port, which touches the first's wall if they are docked. How much of the second's wood is left after a good while, and whether
/// the ships docked. The ports face each other if `docking`, and look the same way (so the ships touch and do not dock) if not.
///
/// (The fire is chance, and what it is depends on the tick a player's first message reaches the server, which a loaded machine moves, and on the
/// numbers of the ships, so a test about whether it spreads must not hang on the luck of one fire or of a few blocks: a wall of 29 and nine fires.)
fn wood_of_the_second_ship_left_after_a_while(docking: bool) -> (usize, u64) {
    use common::tick_and_follow;
    let content = CaveContent::new();
    // Each wall is a plane of wood with the port in the middle of it and a row of stone behind the port (a port at the edge of a heavy ship would
    // swing it, as ports do, and the test is about fire).
    let mut first = stones_at_height(0, 3, 2, 3);
    let mut second = stones_at_height(1, 3, 2, 3);
    for (y, z) in (0..5).flat_map(|y| (0..6).map(move |z| (y, z))) {
        if (y, z) != (2, 3) {
            first.push(([3, y, z], content.blocks.wood, PLAIN));
            second.push(([0, y, z], content.blocks.wood, PLAIN));
        }
    }
    first.push(([3, 2, 3], port_block(), facing(FaceDirection::PositiveX)));
    second.insert(
        0,
        ([0, 2, 3], port_block(), facing(FaceDirection::NegativeX)),
    );
    if !docking {
        second[0] = ([0, 2, 3], port_block(), facing(FaceDirection::PositiveX));
    }
    let (mut scene, report) = Scene::reporting();
    common::flat_floor(&mut scene);
    let mut player = scene
        .rig
        .join([22.0, f64::from(common::FLOOR) + 5.0, 30.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    weightless(&mut scene);
    let level = common::FLOOR + 10.0;
    make(
        &mut scene,
        pose(20.0, level, 20.0, Orientation::IDENTITY),
        true,
        first,
    );
    // Where it docks, 4 along from the first, or 2 metres further if it is to be pulled there.
    let apart = if docking { 6.5 } else { 4.0 };
    make(
        &mut scene,
        pose(20.0 + apart, level, 20.0, Orientation::IDENTITY),
        false,
        second,
    );
    // Time to dock, if they dock, and then the fires are lit (in the ship that has the first wall, whichever ship that is now): a fire lit while
    // the second ship was on its way would have burned the first wall down before it came.
    tick_and_follow(&mut scene, &mut player, 120);
    assert_eq!(report().dockings_made, u64::from(docking));
    let wall = VoxelCoordinate::new(3, 2, 2);
    let ship_with_the_wall = ships(&scene)
        .into_iter()
        .find(|id| blocks_of(&scene, *id).contains_key(&wall))
        .expect("the first wall is there");
    let fire = content.blocks.fire;
    let block_channel = content.block_channel();
    scene.run(move |world| {
        let mut view = world.grid(ship_with_the_wall).unwrap();
        for (y, z) in [
            (1, 0),
            (1, 3),
            (1, 5),
            (2, 1),
            (2, 5),
            (3, 2),
            (3, 0),
            (4, 4),
            (4, 1),
        ] {
            view.set(block_channel, VoxelCoordinate::new(2, y, z), fire)
                .unwrap();
        }
    });
    tick_and_follow(&mut scene, &mut player, 200);
    // The wood of the second ship's side, wherever it is now: wood at or beyond the plane where the two meet (the first ship's wall is at 23 to
    // 24 metres, the second's from 24).
    let wood = content.blocks.wood;
    let mut left = 0;
    for id in ships(&scene) {
        let origin = scene
            .rig
            .server
            .world()
            .grids()
            .get(id)
            .unwrap()
            .pose
            .position
            .offset()[0];
        left += blocks_of(&scene, id)
            .iter()
            .filter(|(voxel, (block, _, _))| {
                *block == wood && origin + voxel.x() as f32 + 0.5 > 24.0
            })
            .count();
    }
    (left, report().dockings_made)
}

#[test]
fn fire_crosses_what_was_the_boundary_between_two_docked_ships_and_does_not_between_two_that_only_touch()
 {
    let (docked_left, made) = wood_of_the_second_ship_left_after_a_while(true);
    assert_eq!(made, 1, "the ships docked");
    assert!(
        docked_left <= 5,
        "docked: {docked_left} of the second ship's 29 blocks of wood are left, the fire having spread to it"
    );
    let (touching_left, made) = wood_of_the_second_ship_left_after_a_while(false);
    assert_eq!(made, 0, "the ships did not dock");
    assert_eq!(
        touching_left, 29,
        "touching but not docked: nothing crosses between ships"
    );
}
