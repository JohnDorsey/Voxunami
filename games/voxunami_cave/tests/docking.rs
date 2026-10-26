//! Docking in Cave: ships with dev docking ports are drawn together, turned into line, snapped and made one ship; and a docked pair of ports can be
//! cut apart again unless a weld holds the two sides.

mod common;

use std::collections::BTreeMap;

use block_models::Shape;
use common::docking::{Look, PLAIN, Placed, a_pair, facing, make, port_block, stone};
use common::{Scene, ship_id};
use game_shell::GameContent;
use server::GridView;
use voxel_space::{
    BrickCoordinate, FaceDirection, GravitySettings, GridId, GridPose, LevelIndex, LocalVoxelIndex,
    Orientation, VoxelCoordinate, WorldPosition,
};
use voxunami_cave::content::{CaveContent, dev_docking_port_id};

fn pose(x: f32, y: f32, z: f32, orientation: Orientation) -> GridPose {
    GridPose::new(
        WorldPosition::new(BrickCoordinate::ORIGIN, [x, y, z]),
        orientation,
    )
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

/// The planet of Cave's small-planet generator in the overworld, with the overworld's pull taken away so that its own is all there is, put so that the
/// top of the ground of the patch at x 0 to 3 and z 5 to 8 (flat there, top voxel 63,679) is at height 399. Returns the height of the planet's centre.
fn a_planet_under_the_patch(scene: &mut Scene) -> f64 {
    let centre = 399.0 - 63_680.0;
    scene.run(move |world| {
        world.set_level_gravity(LevelIndex::MAIN, [0.0; 3]);
        let generator = world
            .data()
            .main()
            .terrain
            .generators()
            .runtime_id(&base_blocks::small_planet_generator_identifier())
            .expect("the world has the planet's generator");
        let planet = world
            .create_grid(
                LevelIndex::MAIN,
                pose(0.0, centre as f32, 0.0, Orientation::IDENTITY),
            )
            .unwrap();
        world
            .set_grid_terrain(
                planet,
                world_storage::GridTerrain {
                    generator,
                    extent: voxel_space::BrickBox::around(BrickCoordinate::ORIGIN, 2000),
                    mass: (10.0 * 63_710.0f64 * 63_710.0 / 6.674e-11) as f32,
                },
            )
            .unwrap();
    });
    centre
}

/// A docking port on the ground of the planet at x 1, z 6, facing up.
fn build_a_port_on_the_planet(scene: &mut Scene) {
    let content = CaveContent::new();
    let look = facing(FaceDirection::PositiveY);
    scene.run(move |world| {
        let mut view = world.grid(ship_id(0)).unwrap();
        let at = VoxelCoordinate::new(1, 63_680, 6);
        view.set(content.state_channel(), at, look.state).unwrap();
        view.set(content.orientation_channel(), at, look.orientation)
            .unwrap();
        view.set(content.block_channel(), at, port_block()).unwrap();
    });
}

fn block_of_the_planet(scene: &mut Scene, at: [i64; 3]) -> u32 {
    let found = std::sync::Arc::new(std::sync::Mutex::new(u32::MAX));
    let slot = std::sync::Arc::clone(&found);
    let channel = CaveContent::new().block_channel();
    scene.run(move |world| {
        *slot.lock().unwrap() = world
            .grid(ship_id(0))
            .unwrap()
            .get(channel, VoxelCoordinate::new(at[0], at[1], at[2]))
            .unwrap();
    });
    std::mem::take(&mut *found.lock().unwrap())
}

#[test]
fn a_ship_with_a_port_that_comes_to_a_port_built_on_a_planet_is_written_into_the_planet_which_keeps_its_identifier()
 {
    let (mut scene, report) = Scene::reporting();
    let centre = a_planet_under_the_patch(&mut scene);
    let planet = ship_id(0);
    // A ship of a port facing down and three stones above it, a metre and a half over where the port will be on the ground, in the pull of the planet.
    let blocks = vec![
        ([0, 0, 0], port_block(), facing(FaceDirection::NegativeY)),
        ([0, 1, 0], stone(), PLAIN),
        ([1, 1, 0], stone(), PLAIN),
        ([0, 1, 1], stone(), PLAIN),
    ];
    make(
        &mut scene,
        pose(
            1.0,
            (centre + 63_681.0 + 1.5) as f32,
            6.0,
            Orientation::IDENTITY,
        ),
        false,
        blocks,
    );
    // The port is built on the ground of the planet after the planet has a body (the ship made it): facing up, a stored block of the planet over its air.
    build_a_port_on_the_planet(&mut scene);
    let ship = ship_id(1);
    for _ in 0..200 {
        scene.ticks(1);
        if scene.rig.server.world().grids().get(ship).is_none() {
            break;
        }
    }
    // The ship is gone, its id leads to the planet, and the planet has its blocks: the port beside the one built on the ground, and the stones above.
    assert!(scene.rig.server.world().grids().get(ship).is_none());
    assert_eq!(scene.rig.server.world().grids().resolve(ship)[0].0, planet);
    assert!(
        scene
            .rig
            .server
            .world()
            .grids()
            .get(planet)
            .unwrap()
            .terrain
            .is_some()
    );
    assert_eq!(
        block_of_the_planet(&mut scene, [1, 63_681, 6]),
        port_block()
    );
    for at in [[1, 63_682, 6], [2, 63_682, 6], [1, 63_682, 7]] {
        assert_eq!(block_of_the_planet(&mut scene, at), stone(), "{at:?}");
    }
    // The ground is as it was.
    assert_ne!(block_of_the_planet(&mut scene, [1, 63_679, 6]), 0);
    assert_eq!(report().dockings_made, 1);
    // A world that is saved and loaded has the planet with its new blocks, and the old id of the ship still leads to the planet.
    let mut scene = scene.restarted();
    // (A planet comes out of its file in the tick after the world is loaded.)
    scene.ticks(2);
    assert_eq!(scene.rig.server.world().grids().resolve(ship)[0].0, planet);
    assert_eq!(block_of_the_planet(&mut scene, [1, 63_682, 6]), stone());
}

#[test]
fn a_ship_whose_blocks_would_be_in_blocks_of_the_planet_is_not_written_into_it_and_is_tried_again_only_now_and_then()
 {
    let (mut scene, report) = Scene::reporting();
    let centre = a_planet_under_the_patch(&mut scene);
    let planet = ship_id(0);
    build_a_port_on_the_planet(&mut scene);
    let content = CaveContent::new();
    scene.run(move |world| {
        let mut view = world.grid(planet).unwrap();
        // A flame where a stone of the ship would be (bodies pass through flames, so nothing pushes the ship away from it).
        view.set(
            content.block_channel(),
            VoxelCoordinate::new(2, 63_682, 6),
            content.blocks.fire,
        )
        .unwrap();
    });
    let blocks = vec![
        ([0, 0, 0], port_block(), facing(FaceDirection::NegativeY)),
        ([0, 1, 0], stone(), PLAIN),
        ([1, 1, 0], stone(), PLAIN),
    ];
    make(
        &mut scene,
        pose(
            1.0,
            (centre + 63_681.0 + 1.5) as f32,
            6.0,
            Orientation::IDENTITY,
        ),
        false,
        blocks,
    );
    scene.ticks(100);
    // Neither ship nor planet changed, and the dockings that were refused are counted.
    assert!(scene.rig.server.world().grids().get(ship_id(1)).is_some());
    assert_eq!(block_of_the_planet(&mut scene, [1, 63_682, 6]), 0);
    let report = report();
    assert_eq!(report.dockings_made, 0);
    assert!(report.dockings_refused >= 1, "{report:?}");
    // Tried again after a second (20 ticks), not every tick: a few in five seconds.
    assert!(report.dockings_refused <= 6, "{report:?}");
}

#[test]
fn a_held_ship_a_tenth_of_a_metre_from_a_port_of_a_planet_is_not_docked_for_nothing_would_move_them()
 {
    let (mut scene, report) = Scene::reporting();
    let centre = a_planet_under_the_patch(&mut scene);
    let blocks = vec![
        ([0, 0, 0], port_block(), facing(FaceDirection::NegativeY)),
        ([0, 1, 0], stone(), PLAIN),
    ];
    let at = pose(
        1.0,
        (centre + 63_681.0 + 0.1) as f32,
        6.0,
        Orientation::IDENTITY,
    );
    make(&mut scene, at, true, blocks);
    build_a_port_on_the_planet(&mut scene);
    scene.ticks(100);
    assert_eq!(
        scene
            .rig
            .server
            .world()
            .grids()
            .get(ship_id(1))
            .unwrap()
            .pose,
        at
    );
    assert_eq!(report().dockings_made + report().dockings_refused, 0);
}

/// A planet with a ship docked to a port built on its ground (see the first test of a planet above), and a player standing on the ground beside them.
fn a_planet_with_a_ship_docked_and_a_player() -> (Scene, common::Player) {
    use common::tick_and_follow;
    let mut scene = Scene::new();
    let centre = a_planet_under_the_patch(&mut scene);
    let blocks = vec![
        ([0, 0, 0], port_block(), facing(FaceDirection::NegativeY)),
        ([0, 1, 0], stone(), PLAIN),
        ([1, 1, 0], stone(), PLAIN),
        ([0, 1, 1], stone(), PLAIN),
    ];
    make(
        &mut scene,
        pose(
            1.0,
            (centre + 63_681.0 + 1.5) as f32,
            6.0,
            Orientation::IDENTITY,
        ),
        false,
        blocks,
    );
    build_a_port_on_the_planet(&mut scene);
    for _ in 0..200 {
        scene.ticks(1);
        if scene.rig.server.world().grids().get(ship_id(1)).is_none() {
            break;
        }
    }
    assert!(scene.rig.server.world().grids().get(ship_id(1)).is_none());
    let mut player = scene.rig.join_walking([0.5, 399.0 + 6.0, 3.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 60);
    (scene, player)
}

#[test]
fn a_ship_docked_to_a_planet_can_be_undocked_and_leaves_with_its_blocks_and_does_not_dock_again() {
    use common::{say, tick_and_follow};
    let (mut scene, mut player) = a_planet_with_a_ship_docked_and_a_player();
    let planet = ship_id(0);
    let reply = say(&mut scene, &mut player, "/ship undock");
    assert!(
        reply.contains("ship 0 came apart into ships 0 and 2"),
        "{reply}"
    );
    // The planet keeps its identifier, its ground and the port built on it; the ship that left has the port that was its own and its three stones.
    assert!(
        scene
            .rig
            .server
            .world()
            .grids()
            .get(planet)
            .unwrap()
            .terrain
            .is_some()
    );
    assert_eq!(
        block_of_the_planet(&mut scene, [1, 63_680, 6]),
        port_block()
    );
    for at in [
        [1, 63_681, 6],
        [1, 63_682, 6],
        [2, 63_682, 6],
        [1, 63_682, 7],
    ] {
        assert_eq!(block_of_the_planet(&mut scene, at), 0, "{at:?}");
    }
    assert_ne!(block_of_the_planet(&mut scene, [1, 63_679, 6]), 0);
    let ship = ship_id(2);
    let left = scene.rig.server.world().grids().get(ship).unwrap();
    assert!(!left.held && left.terrain.is_none());
    assert_eq!(left.voxels.brick_count(), 1);
    // It moves away from the port, slowly, and the planet and it are not drawn together again for as long as they are near.
    tick_and_follow(&mut scene, &mut player, 3);
    assert!(velocity_of(&scene, ship)[1] > 0.0);
    tick_and_follow(&mut scene, &mut player, 200);
    assert!(scene.rig.server.world().grids().get(ship).is_some());
    assert_eq!(block_of_the_planet(&mut scene, [1, 63_682, 6]), 0);
    // Once it has been taken out of range of the port it may dock again: put back a metre and a half over the port at rest, it is written into the planet.
    let here = scene.rig.server.world().grids().get(ship).unwrap().pose;
    let far = GridPose::new(here.position.translated([0.0, 0.0, 30.0]), here.orientation);
    scene.run(move |world| {
        world.set_grid_pose(ship, far).unwrap();
        world.set_grid_velocity(ship, [0.0; 3], [0.0; 3]).unwrap();
    });
    tick_and_follow(&mut scene, &mut player, 5);
    let again = pose(
        1.0,
        (centre_of_the_patch() + 63_681.0 + 1.5) as f32,
        6.0,
        Orientation::IDENTITY,
    );
    scene.run(move |world| {
        world.set_grid_pose(ship, again).unwrap();
        world.set_grid_velocity(ship, [0.0; 3], [0.0; 3]).unwrap();
    });
    for _ in 0..200 {
        tick_and_follow(&mut scene, &mut player, 1);
        if scene.rig.server.world().grids().get(ship).is_none() {
            break;
        }
    }
    assert!(scene.rig.server.world().grids().get(ship).is_none());
    assert_eq!(block_of_the_planet(&mut scene, [1, 63_682, 6]), stone());
}

/// The height of the centre of the planet that `a_planet_under_the_patch` makes.
fn centre_of_the_patch() -> f64 {
    399.0 - 63_680.0
}

#[test]
fn two_ports_built_on_the_ground_of_a_planet_cannot_be_undocked_for_neither_side_would_leave() {
    use common::{say, tick_and_follow};
    let mut scene = Scene::new();
    a_planet_under_the_patch(&mut scene);
    let content = CaveContent::new();
    scene.run(move |world| {
        let mut view = world.grid(ship_id(0)).unwrap();
        for (x, face) in [(1, FaceDirection::PositiveX), (2, FaceDirection::NegativeX)] {
            let at = VoxelCoordinate::new(x, 63_680, 6);
            let look = facing(face);
            view.set(content.state_channel(), at, look.state).unwrap();
            view.set(content.orientation_channel(), at, look.orientation)
                .unwrap();
            view.set(content.block_channel(), at, port_block()).unwrap();
        }
    });
    let mut player = scene.rig.join_walking([0.5, 399.0 + 6.0, 3.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 60);
    let reply = say(&mut scene, &mut player, "/ship undock");
    assert!(reply.contains("stand on the ground"), "{reply}");
    assert_eq!(
        block_of_the_planet(&mut scene, [2, 63_680, 6]),
        port_block()
    );
    assert_eq!(scene.rig.server.world().grids().len(), 1);
}

#[test]
fn a_pilot_aboard_a_ship_that_docks_with_a_planet_stands_on_the_planet_and_does_not_pilot_it() {
    use common::{PlayerExt, player_pose, say, tick_and_follow};
    let mut scene = Scene::new();
    let centre = a_planet_under_the_patch(&mut scene);
    build_a_port_on_the_planet(&mut scene);
    // A ship with a port facing down and a deck of 3 by 3 stones over it, held a metre and a half over the port while the player lands on it and takes the
    // controls; then let go, it falls and docks.
    let mut blocks = vec![([1, 0, 1], port_block(), facing(FaceDirection::NegativeY))];
    for x in 0..3 {
        for z in 0..3 {
            blocks.push(([x, 1, z], stone(), PLAIN));
        }
    }
    make(
        &mut scene,
        pose(
            0.0,
            (centre + 63_681.0 + 1.5) as f32,
            5.0,
            Orientation::IDENTITY,
        ),
        true,
        blocks,
    );
    // (The port of the ship is at x 1, z 6 like the one on the ground: the deck is the 3 by 3 round it.)
    let mut player = scene.rig.join_walking([1.5, 399.0 + 6.0, 6.5], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 60);
    assert_eq!(player_pose(&scene, 1).grid, ship_id(1), "aboard the ship");
    assert!(say(&mut scene, &mut player, "/ship pilot").starts_with("you pilot ship 1"));
    scene.run(|world| world.set_grid_held(ship_id(1), false).unwrap());
    for _ in 0..200 {
        tick_and_follow(&mut scene, &mut player, 1);
        if scene.rig.server.world().grids().get(ship_id(1)).is_none() {
            break;
        }
    }
    assert!(scene.rig.server.world().grids().get(ship_id(1)).is_none());
    tick_and_follow(&mut scene, &mut player, 10);
    // They are on the planet, and walk: the keys are theirs again.
    let before = in_voxels_of(&scene);
    player.hold(&[game_shell::KeyCode::KeyW]);
    tick_and_follow(&mut scene, &mut player, 40);
    let after = in_voxels_of(&scene);
    assert_eq!(player_pose(&scene, 1).grid, ship_id(0));
    let (across, along) = (after[0] - before[0], after[2] - before[2]);
    let moved = libm::sqrt(across * across + along * along);
    assert!(moved > 2.0, "walked {moved}");
}

fn in_voxels_of(scene: &Scene) -> [f64; 3] {
    voxunami_cave::aboard::in_voxels(common::player_pose(scene, 1).position)
}

#[test]
fn the_player_beside_a_planet_sees_a_ship_dock_with_it_and_leave_it() {
    use common::{say, tick_and_follow};
    let (mut scene, mut player) = a_planet_with_a_ship_docked_and_a_player();
    let channel = CaveContent::new().block_channel();
    let seen = |player: &common::Player, at: [i64; 3]| {
        player
            .client()
            .world()
            .grids()
            .get(ship_id(0))
            .map(|planet| {
                planet
                    .voxels
                    .get(channel, VoxelCoordinate::new(at[0], at[1], at[2]))
            })
    };
    // The ship that docked is not in the world of the client, and the planet has its blocks.
    tick_and_follow(&mut scene, &mut player, 5);
    assert!(player.client().world().grids().get(ship_id(1)).is_none());
    assert_eq!(seen(&player, [1, 63_682, 6]), Some(stone()));
    // It leaves: the stones are gone from the planet and the ship is there.
    assert!(say(&mut scene, &mut player, "/ship undock").contains("came apart"));
    tick_and_follow(&mut scene, &mut player, 10);
    assert_eq!(seen(&player, [1, 63_682, 6]), Some(0));
    assert!(player.client().world().grids().get(ship_id(2)).is_some());
}

#[test]
fn of_two_docked_sides_that_float_over_the_ground_of_a_planet_the_smaller_leaves() {
    use common::{say, tick_and_follow};
    let mut scene = Scene::new();
    a_planet_under_the_patch(&mut scene);
    let content = CaveContent::new();
    // Thirty voxels over the ground: a port facing east with two stones over it (three blocks), and the port facing west that it is docked to with one
    // under it (two blocks), all stored in the planet and joined to nothing else.
    scene.run(move |world| {
        let mut view = world.grid(ship_id(0)).unwrap();
        let mut put = |at: [i64; 3], block: u32, face: Option<FaceDirection>| {
            let voxel = VoxelCoordinate::new(at[0], at[1], at[2]);
            if let Some(face) = face {
                let look = facing(face);
                view.set(content.state_channel(), voxel, look.state)
                    .unwrap();
                view.set(content.orientation_channel(), voxel, look.orientation)
                    .unwrap();
            }
            view.set(content.block_channel(), voxel, block).unwrap();
        };
        put([1, 63_710, 6], port_block(), Some(FaceDirection::PositiveX));
        put([1, 63_711, 6], stone(), None);
        put([1, 63_712, 6], stone(), None);
        put([2, 63_710, 6], port_block(), Some(FaceDirection::NegativeX));
        put([2, 63_709, 6], stone(), None);
    });
    let mut player = scene.rig.join_walking([0.5, 399.0 + 6.0, 3.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 60);
    let reply = say(&mut scene, &mut player, "/ship undock");
    assert!(reply.contains("came apart"), "{reply}");
    // The side with the fewer blocks is the ship that left; the other is still in the planet.
    assert_eq!(block_of_the_planet(&mut scene, [2, 63_710, 6]), 0);
    assert_eq!(block_of_the_planet(&mut scene, [2, 63_709, 6]), 0);
    assert_eq!(
        block_of_the_planet(&mut scene, [1, 63_710, 6]),
        port_block()
    );
    assert_eq!(block_of_the_planet(&mut scene, [1, 63_712, 6]), stone());
}
