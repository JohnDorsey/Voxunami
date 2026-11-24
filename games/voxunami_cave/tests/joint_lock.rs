//! The commands `/joint lock` and `/joint unlock` (`docs/decisions/joints-and-cars.md`, "The command"): two cars that are coupled by their joint ends are turned into line and made
//! one ship when a player says so, and a ship that was made of cars so is cut into them again, the cars coupled as they were. (Dock and lock merge grids, and nothing else does.)

mod common;

use std::collections::BTreeMap;

use common::docking::{PLAIN, Placed, blocks_of, ships, stone};
use common::joints::{
    LEVEL, a_chain_on_a_rail, a_player_and_cars, a_player_at_and_cars_posed, ball_end_toward,
    east_car, joint_block, keep_up_with, middle_car, minus, pairs_the_world_has_as_coupled, pose,
    two_cars, until_coupled, west_car, world_of,
};
use common::{Player, Scene, length, say, ship_id, tick_and_follow};
use server::GridView;
use voxel_space::{
    BrickCoordinate, FaceDirection, GridId, GridPose, Orientation, VoxelCoordinate, WorldPosition,
};
use voxunami_cave::content::CaveContent;

/// What blocks of cars make, as a ship has them: where, which block, its state and its orientation.
fn layout_of(blocks: &[Placed]) -> BTreeMap<VoxelCoordinate, (u32, u32, u32)> {
    blocks
        .iter()
        .map(|([x, y, z], block, look)| {
            (
                VoxelCoordinate::new(*x, *y, *z),
                (*block, look.state, look.orientation),
            )
        })
        .collect()
}

/// The blocks of cars moved this many voxels along x.
fn moved_along_x(blocks: Vec<Placed>, by: i128) -> Vec<Placed> {
    blocks
        .into_iter()
        .map(|([x, y, z], block, look)| ([x + by, y, z], block, look))
        .collect()
}

/// The blocks of a ship that is the east car and the west car locked: the ends of their joints are in cells that touch, at 3 and 4.
fn the_two_cars_locked() -> Vec<Placed> {
    let mut blocks = east_car(3);
    blocks.extend(moved_along_x(west_car(3), 4));
    blocks
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

/// The momentum of every ship together.
fn momentum(scene: &Scene) -> [f64; 3] {
    let mut total = [0.0; 3];
    for id in ships(scene) {
        let mass = scene.rig.server.world().grids().get(id).unwrap().mass;
        let speed = velocity_of(scene, id);
        for axis in 0..3 {
            total[axis] += mass * speed[axis];
        }
    }
    total
}

/// A player beside a ship that is made of cars that are locked.
fn a_player_and_a_locked_ship(blocks: Vec<Placed>) -> (Scene, common::joints::Report, Player) {
    a_player_and_cars(24.0, vec![(20.0, blocks)])
}

/// Ticks (with the player following) until there are this many ships, or `limit` ticks have gone.
fn until_ships(scene: &mut Scene, player: &mut Player, count: usize, limit: usize) -> usize {
    let mut ticks = 0;
    while ships(scene).len() != count && ticks < limit {
        tick_and_follow(scene, player, 1);
        ticks += 1;
    }
    ticks
}

#[test]
fn a_locked_ship_is_unlocked_into_two_cars_that_go_as_it_went_and_are_coupled_by_their_ends_again()
{
    let (mut scene, report, mut player) = a_player_and_a_locked_ship(the_two_cars_locked());
    // The ship goes 3 m/s along z and turns a little (0.1 rad/s about y, so the cars at its ends go 0.4 m/s one way and the other: the ends' speeds are the same, the cars' are not).
    scene.run(|world| {
        world
            .set_grid_velocity(ship_id(0), [0.0, 0.0, 3.0], [0.0, 0.1, 0.0])
            .unwrap();
    });
    tick_and_follow(&mut scene, &mut player, 3);
    let before = momentum(&scene);
    let reply = say(&mut scene, &mut player, "/joint unlock");
    assert!(
        reply.contains("unlocked ship 0 into ships 1 and 2"),
        "{reply}"
    );
    assert_eq!(ships(&scene), vec![ship_id(1), ship_id(2)]);
    // The east car is the first (its end is the first of the pair), the west car the second, each as it was made.
    assert_eq!(
        blocks_of(&scene, ship_id(1)),
        layout_of(&east_car(3)),
        "the east car"
    );
    assert_eq!(
        blocks_of(&scene, ship_id(2)),
        layout_of(&moved_along_x(west_car(3), 4)),
        "the west car"
    );
    // They are not set apart (a joint is not a docking port): their ends are together, and in line, and they are coupled in a few ticks.
    until_coupled(&mut scene, &mut player, &report, 1, 60);
    assert_eq!(
        pairs_the_world_has_as_coupled(&mut scene)
            .into_iter()
            .collect::<Vec<_>>(),
        vec![(ship_id(1), ship_id(2))]
    );
    // The speed is what it was, and so is the momentum (nothing was pushed): the ends go together at 3 m/s along z.
    let after = momentum(&scene);
    for axis in 0..3 {
        assert!(
            (after[axis] - before[axis]).abs() < 0.01 * before[2].abs(),
            "axis {axis}: {before:?} became {after:?}"
        );
    }
    tick_and_follow(&mut scene, &mut player, 40);
    assert_eq!(report().couplings, 1, "still coupled 40 ticks on");
}

#[test]
fn a_ship_with_two_pairs_of_joint_ends_between_two_cars_is_cut_at_both_at_once() {
    // Two cars of five blocks, a column of stone at each side and a pair of joint ends at the bottom and one at the top, with nothing between the two cars in the middle.
    let mut blocks = Vec::new();
    for y in 0..3 {
        blocks.push(([0, y, 0], stone(), PLAIN));
        blocks.push(([3, y, 0], stone(), PLAIN));
    }
    for y in [0, 2] {
        blocks.push((
            [1, y, 0],
            joint_block(),
            ball_end_toward(FaceDirection::PositiveX),
        ));
        blocks.push((
            [2, y, 0],
            joint_block(),
            ball_end_toward(FaceDirection::NegativeX),
        ));
    }
    let (mut scene, _report, mut player) = a_player_and_a_locked_ship(blocks);
    tick_and_follow(&mut scene, &mut player, 3);
    let reply = say(&mut scene, &mut player, "/joint unlock");
    assert!(reply.contains("unlocked ship 0 into ships"), "{reply}");
    let parts: Vec<usize> = ships(&scene)
        .into_iter()
        .map(|id| blocks_of(&scene, id).len())
        .collect();
    assert_eq!(parts, vec![5, 5], "{reply}");
}

#[test]
fn cars_joined_by_blocks_besides_their_joint_ends_are_welded_and_unlock_says_so_until_the_weld_is_broken()
 {
    let mut blocks = the_two_cars_locked();
    // A stone above each joint end: beside each other, they join the two cars whatever the ends do.
    blocks.push(([3, 1, 0], stone(), PLAIN));
    blocks.push(([4, 1, 0], stone(), PLAIN));
    let (mut scene, _report, mut player) = a_player_and_a_locked_ship(blocks);
    tick_and_follow(&mut scene, &mut player, 3);
    let reply = say(&mut scene, &mut player, "/joint unlock");
    assert!(reply.contains("ship 0 is welded there"), "{reply}");
    assert_eq!(ships(&scene), vec![ship_id(0)], "nothing changed");
    let content = CaveContent::new();
    scene.run(move |world| {
        world
            .grid(ship_id(0))
            .unwrap()
            .set(
                content.block_channel(),
                VoxelCoordinate::new(4, 1, 0),
                content.blocks.air,
            )
            .unwrap();
    });
    tick_and_follow(&mut scene, &mut player, 3);
    let reply = say(&mut scene, &mut player, "/joint unlock");
    assert!(reply.contains("unlocked ship 0 into ships"), "{reply}");
    assert_eq!(ships(&scene).len(), 2);
}

#[test]
fn unlock_with_nothing_locked_near_says_so_and_changes_nothing() {
    let (mut scene, report, mut player) = two_cars();
    until_coupled(&mut scene, &mut player, &report, 1, 600);
    let reply = say(&mut scene, &mut player, "/joint unlock");
    assert!(reply.contains("nothing is locked within"), "{reply}");
    tick_and_follow(&mut scene, &mut player, 5);
    assert_eq!(ships(&scene).len(), 2);
    assert_eq!(report().couplings, 1);
}

#[test]
fn of_two_locked_pairs_the_one_nearest_the_player_is_unlocked() {
    // Three cars locked in a row, of 4, 5 and 3 blocks: the ends of the first lock are at 3 and 4 along the ship and the second's at 8 and 9.
    let mut blocks = east_car(3);
    blocks.extend(moved_along_x(middle_car(), 4));
    blocks.extend(moved_along_x(west_car(2), 9));
    for (eyes_x, sizes) in [(24.0, [4, 8]), (29.0, [9, 3])] {
        let (mut scene, _report, mut player) = a_player_at_and_cars_posed(
            [eyes_x, common::FLOOR + 5.0, 30.0],
            vec![(pose(20.0, LEVEL, 20.0), blocks.clone())],
        );
        tick_and_follow(&mut scene, &mut player, 3);
        let reply = say(&mut scene, &mut player, "/joint unlock");
        assert!(
            reply.contains("unlocked ship 0 into ships"),
            "{eyes_x}: {reply}"
        );
        let mut parts: Vec<usize> = ships(&scene)
            .into_iter()
            .map(|id| blocks_of(&scene, id).len())
            .collect();
        parts.sort_unstable();
        let mut wanted = sizes;
        wanted.sort_unstable();
        assert_eq!(parts, wanted, "{eyes_x}: {reply}");
    }
}

#[test]
fn two_cars_in_line_that_are_coupled_are_made_one_ship_by_lock_with_the_blocks_of_both_and_the_speed_they_had()
 {
    let (mut scene, report, mut player) = two_cars();
    scene.run(|world| {
        for id in [ship_id(0), ship_id(1)] {
            world
                .set_grid_velocity(id, [0.0, 0.0, 2.0], [0.0; 3])
                .unwrap();
        }
    });
    until_coupled(&mut scene, &mut player, &report, 1, 600);
    let reply = say(&mut scene, &mut player, "/joint lock");
    assert!(reply.contains("locking ships 0 and 1"), "{reply}");
    let ticks = until_ships(&mut scene, &mut player, 1, 60);
    assert_eq!(ships(&scene).len(), 1, "locked after {ticks} ticks");
    assert_eq!(report().locks_made, 1);
    let whole = ships(&scene)[0];
    assert_eq!(
        blocks_of(&scene, whole),
        layout_of(&the_two_cars_locked()),
        "the cars are one ship, the ends of the joints in cells that touch"
    );
    let speed = velocity_of(&scene, whole);
    assert!(
        length([speed[0], speed[1], speed[2] - 2.0]) < 0.05,
        "{speed:?}"
    );
    // The coupling went with the cars it was of: nothing is coupled, in the report or in the world.
    tick_and_follow(&mut scene, &mut player, 3);
    assert_eq!(report().couplings, 0);
    assert!(pairs_the_world_has_as_coupled(&mut scene).is_empty());
    // And a second lock is nothing to do.
    let reply = say(&mut scene, &mut player, "/joint lock");
    assert!(reply.contains("nothing is coupled within"), "{reply}");
}

/// A pose of the west car, which is rolled this much about the axis of its joint end (the ends are where they would be if it was not): `x` is where the end is along x.
fn rolled_about_the_ends_axis(x: f64, roll: f64) -> GridPose {
    let (sine, cosine) = (libm::sin(roll), libm::cos(roll));
    // The end is at (0, 0.5, 0.5) in the car; the car is turned about its origin, and put where the end is where it would have been.
    let origin = [
        x,
        LEVEL + 0.5 - 0.5 * (cosine - sine),
        20.5 - 0.5 * (sine + cosine),
    ];
    GridPose::new(
        WorldPosition::new(BrickCoordinate::ORIGIN, origin),
        Orientation::from_components([libm::sin(roll / 2.0), 0.0, 0.0, libm::cos(roll / 2.0)])
            .unwrap(),
    )
}

/// The east car and a west car that is rolled, with a block off the axis (a stone above its second block), and a player beside them.
fn a_rolled_pair(roll: f64) -> (Scene, common::joints::Report, Player) {
    let mut west = west_car(3);
    west.push(([2, 1, 0], stone(), PLAIN));
    a_player_at_and_cars_posed(
        [22.0, common::FLOOR + 5.0, 30.0],
        vec![
            (pose(20.0, LEVEL, 20.0), east_car(3)),
            (rolled_about_the_ends_axis(27.0, roll), west),
        ],
    )
}

/// Where the stone off the axis of the west car is in the ship that locking makes, when it is the west car that is carried onto the east one (the voxels it can be at are a
/// quarter turn about the axis from where it was made).
fn the_stone_off_the_axis(scene: &Scene, whole: GridId) -> Vec<VoxelCoordinate> {
    let content = CaveContent::new();
    blocks_of(scene, whole)
        .into_iter()
        .filter(|(voxel, (block, ..))| {
            *block == content.blocks.stone && voxel.x() == 6 && (voxel.y(), voxel.z()) != (0, 0)
        })
        .map(|(voxel, _)| voxel)
        .collect()
}

#[test]
fn a_car_turned_a_quarter_about_the_axis_of_the_ends_is_locked_with_its_blocks_turned_onto_the_lattice_of_the_other()
 {
    let (mut scene, report, mut player) = a_rolled_pair(std::f64::consts::FRAC_PI_2);
    until_coupled(&mut scene, &mut player, &report, 1, 600);
    say(&mut scene, &mut player, "/joint lock");
    until_ships(&mut scene, &mut player, 1, 60);
    assert_eq!(ships(&scene).len(), 1);
    let whole = ships(&scene)[0];
    // Along the axis nothing moved; the stone that was above the second block is a voxel to one side, a quarter turn from where it was made.
    let found = the_stone_off_the_axis(&scene, whole);
    assert_eq!(found.len(), 1, "{found:?}");
    let at = found[0];
    assert!(
        (at.y(), at.z()) == (0, 1) || (at.y(), at.z()) == (0, -1),
        "{at:?}"
    );
}

#[test]
fn a_car_that_is_out_of_line_is_turned_into_line_and_then_locked_and_the_pair_keeps_its_momentum() {
    // Rolled 40 degrees: the nearest turn that lines the cars up is none, and the ends snap (within 10 degrees) only when they have been turned most of the way.
    let (mut scene, report, mut player) = a_rolled_pair(40.0_f64.to_radians());
    scene.run(|world| {
        for id in [ship_id(0), ship_id(1)] {
            world
                .set_grid_velocity(id, [0.0, 0.0, 2.0], [0.0; 3])
                .unwrap();
        }
    });
    until_coupled(&mut scene, &mut player, &report, 1, 600);
    let before = momentum(&scene);
    say(&mut scene, &mut player, "/joint lock");
    // (Not at once: it has to be turned.)
    tick_and_follow(&mut scene, &mut player, 2);
    assert_eq!(ships(&scene).len(), 2, "turned into line first");
    let ticks = until_ships(&mut scene, &mut player, 1, 600);
    assert_eq!(ships(&scene).len(), 1, "locked after {ticks} ticks");
    let whole = ships(&scene)[0];
    // The stone off the axis is straight above the second block: the car was turned back, not on.
    let found = the_stone_off_the_axis(&scene, whole);
    assert_eq!(found, vec![VoxelCoordinate::new(6, 1, 0)]);
    // (A ship that is made weighs nothing until it is weighed, a tick or two on.)
    tick_and_follow(&mut scene, &mut player, 5);
    let speed = velocity_of(&scene, whole);
    assert!(
        length([speed[0], speed[1], speed[2] - 2.0]) < 0.05,
        "{speed:?}"
    );
    // The two cars were turned by torques that are equal and opposite (the ship they make turns as the cars did together, which is not at all: it was at rest about its centre of mass).
    let spin = scene
        .rig
        .server
        .world()
        .grids()
        .get(whole)
        .unwrap()
        .angular_velocity;
    // (Measured: 0.0086 rad/s, the joint's rounding after a second of swinging; with both cars turned the same way, which the mutant did, it was 1.8.)
    assert!(length(spin) < 0.05, "{spin:?} rad/s");
    let after = momentum(&scene);
    for axis in 0..3 {
        assert!(
            (after[axis] - before[axis]).abs() < 0.02 * before[2].abs(),
            "axis {axis}: {before:?} became {after:?}"
        );
    }
}

#[test]
fn a_lock_that_cannot_be_made_is_tried_again_only_now_and_then_and_unlock_takes_it_back() {
    // The west car has a stone above its end, and the east car water in the cell that the stone would be in when they are one ship (water is passed through by bodies, so
    // nothing pushes the cars apart): they are coupled, in line, and cannot be made one.
    let content = CaveContent::new();
    let mut east = east_car(3);
    east.push(([4, 1, 0], content.blocks.great_river_water, PLAIN));
    let mut west = west_car(3);
    west.push(([0, 1, 0], stone(), PLAIN));
    let (mut scene, report, mut player) = a_player_and_cars(22.0, vec![(20.0, east), (27.0, west)]);
    until_coupled(&mut scene, &mut player, &report, 1, 600);
    say(&mut scene, &mut player, "/joint lock");
    tick_and_follow(&mut scene, &mut player, 100);
    assert_eq!(ships(&scene).len(), 2);
    assert_eq!(report().locks_made, 0);
    assert_eq!(report().couplings, 1, "the cars are coupled all the same");
    // Tried, refused, and left alone for a second (20 ticks) each time: five or six tries in 100 ticks, not a hundred.
    let refused = report().locks_refused;
    assert!((2..=6).contains(&refused), "{refused} refusals");
    let reply = say(&mut scene, &mut player, "/joint unlock");
    assert!(reply.contains("not to be locked any more"), "{reply}");
    tick_and_follow(&mut scene, &mut player, 100);
    assert_eq!(report().locks_refused, refused, "not tried again");
    assert_eq!(ships(&scene).len(), 2);
}

#[test]
fn parting_a_pair_that_is_to_be_locked_takes_the_lock_back_with_it() {
    let (mut scene, report, mut player) = a_rolled_pair(40.0_f64.to_radians());
    until_coupled(&mut scene, &mut player, &report, 1, 600);
    say(&mut scene, &mut player, "/joint lock");
    let reply = say(&mut scene, &mut player, "/joint decouple");
    assert!(reply.contains("decoupled"), "{reply}");
    tick_and_follow(&mut scene, &mut player, 200);
    assert_eq!(ships(&scene).len(), 2);
    assert_eq!(report().locks_made, 0);
    assert_eq!(report().couplings, 0);
}

#[test]
fn a_lock_is_asked_only_of_a_coupling_and_a_second_ask_is_told_so() {
    // (The cars are three metres apart, not coupled yet, in the first tick.)
    let (mut scene, _report, mut player) = two_cars();
    let reply = say(&mut scene, &mut player, "/joint lock");
    assert!(reply.contains("nothing is coupled within"), "{reply}");
    // Rolled out of line, so that the lock is not made in the tick it is asked for.
    let (mut scene, report, mut player) = a_rolled_pair(40.0_f64.to_radians());
    until_coupled(&mut scene, &mut player, &report, 1, 600);
    assert!(say(&mut scene, &mut player, "/joint lock").contains("locking ships 0 and 1"));
    let reply = say(&mut scene, &mut player, "/joint lock");
    assert!(reply.contains("already being locked"), "{reply}");
}

#[test]
fn locking_two_cars_of_a_chain_leaves_the_rest_of_the_chain_coupled_to_the_ship_they_make() {
    let (mut scene, report, mut player) = a_player_and_cars(
        25.0,
        vec![
            (20.0, east_car(3)),
            (27.0, middle_car()),
            (36.0, west_car(3)),
        ],
    );
    until_coupled(&mut scene, &mut player, &report, 2, 900);
    let reply = say(&mut scene, &mut player, "/joint lock");
    assert!(reply.contains("locking ships 0 and 1"), "{reply}");
    until_ships(&mut scene, &mut player, 2, 60);
    assert_eq!(ships(&scene).len(), 2);
    // The ship they make is a new one, and the third car is coupled to it in a tick or two (its end was at the other car's end, and is where it was).
    let mut ticks = 0;
    while report().couplings != 1 && ticks < 60 {
        tick_and_follow(&mut scene, &mut player, 1);
        ticks += 1;
    }
    assert_eq!(report().couplings, 1, "after {ticks} ticks");
    let coupled = pairs_the_world_has_as_coupled(&mut scene);
    assert_eq!(coupled.len(), 1);
    let (first, second) = coupled.into_iter().next().unwrap();
    assert!(
        [first, second].contains(&ship_id(2)),
        "the third car is coupled: {first:?} {second:?}"
    );
}

/// The ships of a world that has a planet (which is a grid too): the grids that have no terrain.
fn cars_in_a_world_with_a_planet(scene: &Scene) -> Vec<GridId> {
    scene
        .rig
        .server
        .world()
        .grids()
        .iter()
        .filter(|(_, ship)| ship.terrain.is_none())
        .map(|(id, _)| id)
        .collect()
}

/// What a chain of three cars on a rail is some ticks after a player at its middle says `/joint lock` (or says nothing): what the command said, how many locks were made and couplings
/// there are, where each ship is and how it goes, by identifier, and which have a rail.
type ChainAfterTheLock = (
    String,
    u64,
    u64,
    BTreeMap<GridId, ([f64; 3], [f64; 3], bool)>,
);

fn a_chain_on_a_rail_after(lock: bool, ticks: usize) -> ChainAfterTheLock {
    let mut chain = a_chain_on_a_rail();
    if lock {
        chain.player.send_command("/joint lock");
    }
    let mut now = chain.ships.clone();
    for _ in 0..ticks {
        keep_up_with(&mut chain.scene, &mut chain.player, &now);
        now = cars_in_a_world_with_a_planet(&chain.scene);
    }
    let reply = chain.player.take_command_replies().join(" | ");
    let cars = now
        .iter()
        .map(|id| {
            let ship = chain.scene.rig.server.world().grids().get(*id).unwrap();
            (
                *id,
                (
                    ship.pose.position.in_voxels(),
                    ship.linear_velocity,
                    ship.rail.is_some(),
                ),
            )
        })
        .collect();
    let report = (chain.report)();
    (reply, report.locks_made, report.couplings, cars)
}

#[test]
fn a_chain_on_a_rail_that_is_locked_comes_off_it_where_it_is_and_the_cars_that_are_not_locked_lose_no_tick()
 {
    let (_, _, couplings_before, kept) = a_chain_on_a_rail_after(false, 4);
    assert_eq!((couplings_before, kept.len()), (2, 3));
    let (reply, _, _, locked) = a_chain_on_a_rail_after(true, 4);
    assert!(reply.contains("locking ships"), "{reply}");
    assert_eq!(
        locked.len(),
        3,
        "not made one yet: the chain swings, and the cars are turned into line first"
    );
    assert!(
        locked.values().all(|(_, _, railed)| !railed),
        "the group is off the rail: its cars are turned, which a rail does not let them"
    );
    // A car that went a tick short, or was put at the rail's point for a car alone, is 38 voxels or metres off. The cars that are turned are given a torque and the third is held by the ball it
    // is coupled with, so the difference is not nothing: measured 0.013 to 0.023 voxels and 0.05 to 0.09 metres a second after four ticks (the tolerances are 20 and 6 times that, and 76 times less than a tick).
    for (id, (kept_place, kept_speed, _)) in &kept {
        let (place, speed, _) = locked[id];
        let (apart, slower) = (
            length(minus(place, *kept_place)),
            length(minus(speed, *kept_speed)),
        );
        assert!(
            apart < 0.5,
            "{id:?} is {apart} voxels from where it was to be"
        );
        assert!(
            slower < 0.5,
            "{id:?} goes {slower} m/s from how it was to go"
        );
    }
}

#[test]
fn a_chain_on_a_rail_that_is_locked_is_made_one_ship_that_goes_as_the_cars_did_and_the_third_car_is_coupled_to_it_again()
 {
    let (_, _, _, kept) = a_chain_on_a_rail_after(false, 60);
    let speed_of_the_chain = length(kept[&ship_id(1)].1);
    let (reply, locks, couplings, locked) = a_chain_on_a_rail_after(true, 60);
    assert_eq!(locks, 1, "{reply}");
    assert_eq!(locked.len(), 2, "{reply}");
    assert_eq!(
        couplings, 1,
        "the third car is coupled to what the others made: {reply}"
    );
    for (id, (_, speed, _)) in &locked {
        assert!(
            (length(*speed) - speed_of_the_chain).abs() < 0.005 * speed_of_the_chain,
            "{id:?} goes {speed:?}, and the chain went {speed_of_the_chain}"
        );
    }
}

#[test]
fn a_lock_wanted_of_a_coupling_that_is_gone_is_not_wanted_when_the_cars_are_coupled_again() {
    let (mut scene, report, mut player) = a_rolled_pair(40.0_f64.to_radians());
    until_coupled(&mut scene, &mut player, &report, 1, 600);
    assert!(say(&mut scene, &mut player, "/joint lock").contains("locking ships 0 and 1"));
    // The end of the east car is taken away (a block edit), and the coupling goes with it; put back, the cars are coupled again.
    let content = CaveContent::new();
    let joint = joint_block();
    let look = ball_end_toward(FaceDirection::PositiveX);
    let (air, block_channel) = (content.blocks.air, content.block_channel());
    scene.run(move |world| {
        world
            .grid(ship_id(0))
            .unwrap()
            .set(block_channel, VoxelCoordinate::new(3, 0, 0), air)
            .unwrap();
    });
    tick_and_follow(&mut scene, &mut player, 3);
    assert_eq!(report().couplings, 0, "the coupling went with the block");
    assert_eq!(report().locks_made, 0);
    let (state_channel, orientation_channel) =
        (content.state_channel(), content.orientation_channel());
    scene.run(move |world| {
        let mut view = world.grid(ship_id(0)).unwrap();
        let at = VoxelCoordinate::new(3, 0, 0);
        view.set(state_channel, at, look.state).unwrap();
        view.set(orientation_channel, at, look.orientation).unwrap();
        view.set(block_channel, at, joint).unwrap();
    });
    until_coupled(&mut scene, &mut player, &report, 1, 600);
    // Nobody asked for this pair to be locked since: asking says so, and nothing was made one in the meantime.
    assert_eq!(report().locks_made, 0);
    assert_eq!(ships(&scene).len(), 2);
    let reply = say(&mut scene, &mut player, "/joint lock");
    assert!(reply.contains("locking ships 0 and 1"), "{reply}");
}

#[test]
fn unlock_takes_back_a_lock_that_is_wanted_or_cuts_a_lock_that_is_made_whichever_is_nearer_the_player()
 {
    // A ship of two cars locked (the ends at 3 and 4 along it) with an end at its far side (8), which is coupled to a third car that is turned out of line, so that a lock asked for it is
    // not made at once: the pivots are at 24 and at 29, and a player is beside one or the other.
    let mut locked = east_car(3);
    locked.extend(moved_along_x(middle_car(), 4));
    for (unlock_from, cancels) in [(29.0, true), (24.0, false)] {
        let (mut scene, report, mut player) = a_player_at_and_cars_posed(
            [29.0, common::FLOOR + 5.0, 30.0],
            vec![
                (pose(20.0, LEVEL, 20.0), locked.clone()),
                (
                    rolled_about_the_ends_axis(32.0, 40.0_f64.to_radians()),
                    west_car(3),
                ),
            ],
        );
        until_coupled(&mut scene, &mut player, &report, 1, 600);
        let reply = say(&mut scene, &mut player, "/joint lock");
        assert!(reply.contains("locking ships 0 and 1"), "{reply}");
        // (The player is at 29, beside the coupling, when the lock is asked, and goes to where it unlocks from.)
        player.send_command(&format!("/tp {unlock_from} {} 30", common::FLOOR + 5.0));
        tick_and_follow(&mut scene, &mut player, 1);
        let reply = say(&mut scene, &mut player, "/joint unlock");
        if cancels {
            assert!(
                reply.contains("not to be locked any more"),
                "{unlock_from}: {reply}"
            );
            assert_eq!(ships(&scene).len(), 2, "{unlock_from}: nothing was cut");
        } else {
            assert!(
                reply.contains("unlocked ship 0 into ships"),
                "{unlock_from}: {reply}"
            );
        }
    }
}

#[test]
fn a_lock_farther_from_the_player_than_the_reach_is_not_unlocked() {
    let (mut scene, _report, mut player) = a_player_at_and_cars_posed(
        [24.0, LEVEL + 60.0, 22.0],
        vec![(pose(20.0, LEVEL, 20.0), the_two_cars_locked())],
    );
    tick_and_follow(&mut scene, &mut player, 3);
    let reply = say(&mut scene, &mut player, "/joint unlock");
    assert!(reply.contains("nothing is locked within"), "{reply}");
    assert_eq!(ships(&scene).len(), 1);
}

#[test]
fn of_two_cars_locked_the_heavier_stays_where_it_is_and_the_lighter_is_put_to_it() {
    // The east car is heavy (six stones and its end: ship 0, which is the base of the ship the merge makes), the west car light (a stone and its end), and the light one is turned 8 degrees
    // about the axis of the ends, which is within the snap: it is made one ship in the tick it is asked for, and the car that is turned a little to make it is the light one.
    let (mut scene, report, mut player) = a_player_at_and_cars_posed(
        [27.0, common::FLOOR + 5.0, 30.0],
        vec![
            (pose(20.0, LEVEL, 20.0), east_car(6)),
            (
                rolled_about_the_ends_axis(30.0, 8.0_f64.to_radians()),
                west_car(1),
            ),
        ],
    );
    until_coupled(&mut scene, &mut player, &report, 1, 900);
    // (A corner of the far stone, off the axis of the ends that the light car is turned about: a point on it would not move if the heavy car was turned instead.)
    let far_stone = [0.0, 0.0, 0.0];
    let before = world_of(&scene, ship_id(0), far_stone);
    assert!(say(&mut scene, &mut player, "/joint lock").contains("locking ships 0 and 1"));
    until_ships(&mut scene, &mut player, 1, 60);
    assert_eq!(ships(&scene).len(), 1);
    // The ship is made on the heavy car's lattice: its stone is where it was in it.
    let after = world_of(&scene, ships(&scene)[0], far_stone);
    let moved = length(minus(after, before));
    assert!(
        moved < 0.01,
        "the heavy car is {moved} voxels from where it was: the light one was to be put to it"
    );
}
