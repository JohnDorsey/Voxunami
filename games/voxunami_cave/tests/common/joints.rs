//! Ships with joints, for the tests of the dev ball joint (the ships and the way a block is made in them are those of `docking`), and the cars that the tests of its commands share.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use game_shell::GameContent;
use voxel_space::{
    BrickCoordinate, FaceDirection, GravitySettings, GridId, GridPose, Orientation, WorldPosition,
};
use voxunami_cave::content::{CaveContent, dev_ball_joint_id};
use voxunami_cave::placement::turn_carrying;

use super::docking::{Look, Placed, make, stones};
use server::ShipSettings;

use super::{FLOOR, Player, Scene, chains, circular_speed, flat_floor, tick_and_follow};

pub fn joint_block() -> u32 {
    CaveContent::new().registries()[0]
        .1
        .raw_id_of(&dev_ball_joint_id())
        .expect("Cave has the dev ball joint")
}

/// A dev ball joint turned so that its ball end (the face that is its joint face, which is up as it is made) looks this way.
pub fn ball_end_toward(face: FaceDirection) -> Look {
    Look {
        state: 0,
        orientation: u32::from(turn_carrying(FaceDirection::PositiveY, face).index()),
    }
}

/// A pose at a place, not turned.
pub fn pose(x: f64, y: f64, z: f64) -> GridPose {
    GridPose::new(
        WorldPosition::new(BrickCoordinate::ORIGIN, [x, y, z]),
        Orientation::IDENTITY,
    )
}

/// No gravity: cars hang in the air where they are put, and the only forces are the joint ends'.
pub fn weightless(scene: &mut Scene) {
    scene.run(|world| {
        world.set_gravity_settings(GravitySettings {
            base: [0.0; 3],
            ..GravitySettings::default()
        });
    });
}

/// The height of the cars above the floor.
pub const LEVEL: f64 = FLOOR + 10.0;

/// A car of `count` stones in a row along x with a joint end after them looking east (its pivot is at x = `count` + 1 in the car).
pub fn east_car(count: i128) -> Vec<Placed> {
    let mut blocks = stones(0, count);
    blocks.push((
        [count, 0, 0],
        joint_block(),
        ball_end_toward(FaceDirection::PositiveX),
    ));
    blocks
}

/// A car with a joint end looking west before `count` stones (its pivot is at x = 0 in the car).
pub fn west_car(count: i128) -> Vec<Placed> {
    let mut blocks = vec![(
        [0, 0, 0],
        joint_block(),
        ball_end_toward(FaceDirection::NegativeX),
    )];
    blocks.extend(stones(1, count + 1));
    blocks
}

/// A car with a joint end at each end, looking away from the middle (three stones between them).
pub fn middle_car() -> Vec<Placed> {
    let mut blocks = west_car(3);
    blocks.push((
        [4, 0, 0],
        joint_block(),
        ball_end_toward(FaceDirection::PositiveX),
    ));
    blocks
}

/// What the scene's bodies say, as a function.
pub type Report = Box<dyn Fn() -> grid_bodies::BodiesReport>;

/// A player on the floor and cars in the air above it, with the player's eyes at `eyes_x` along the cars.
pub fn a_player_and_cars(eyes_x: f64, cars: Vec<(f64, Vec<Placed>)>) -> (Scene, Report, Player) {
    a_player_at_and_cars([eyes_x, FLOOR + 5.0, 30.0], cars)
}

/// A player at a place, flying, and cars in the air over the floor.
pub fn a_player_at_and_cars(
    place: [f64; 3],
    cars: Vec<(f64, Vec<Placed>)>,
) -> (Scene, Report, Player) {
    a_player_at_and_cars_posed(
        place,
        cars.into_iter()
            .map(|(x, blocks)| (pose(x, LEVEL, 20.0), blocks))
            .collect(),
    )
}

/// A player at a place, flying, and cars in the air over the floor, each at a pose of its own.
pub fn a_player_at_and_cars_posed(
    place: [f64; 3],
    cars: Vec<(GridPose, Vec<Placed>)>,
) -> (Scene, Report, Player) {
    let (mut scene, report) = Scene::reporting();
    flat_floor(&mut scene);
    let mut player = scene.rig.join(place, 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    weightless(&mut scene);
    for (at, blocks) in cars {
        make(&mut scene, at, false, blocks);
    }
    (scene, Box::new(report), player)
}

/// Two cars with their joint ends three metres apart, and a player beside them: the east car is made first (ship 0).
pub fn two_cars() -> (Scene, Report, Player) {
    a_player_and_cars(22.0, vec![(20.0, east_car(3)), (27.0, west_car(3))])
}

/// Ticks (with the player following) until `count` couplings are made or `limit` ticks have gone.
pub fn until_coupled(
    scene: &mut Scene,
    player: &mut Player,
    report: &Report,
    count: u64,
    limit: usize,
) {
    let mut ticks = 0;
    while report().couplings < count && ticks < limit {
        tick_and_follow(scene, player, 1);
        ticks += 1;
    }
    assert_eq!(report().couplings, count, "after {ticks} ticks");
}

pub fn minus(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// Where the middle of a car's voxel is in the world, in voxels.
pub fn world_of(scene: &Scene, id: GridId, local: [f64; 3]) -> [f64; 3] {
    let ship = scene.rig.server.world().grids().get(id).unwrap();
    let rotation = ship.pose.rotation_matrix();
    let origin = ship
        .pose
        .position
        .relative_to(WorldPosition::from_voxels([0.0; 3]));
    [0, 1, 2].map(|axis| {
        origin[axis]
            + (0..3)
                .map(|column| rotation[axis][column] * local[column])
                .sum::<f64>()
    })
}

/// The pairs of ships that the world has as coupled.
pub fn pairs_the_world_has_as_coupled(scene: &mut Scene) -> BTreeSet<(GridId, GridId)> {
    let found = Arc::new(Mutex::new(BTreeSet::new()));
    let sink = Arc::clone(&found);
    scene.run(move |world| {
        sink.lock()
            .unwrap()
            .extend(world.coupled_ships().iter().copied())
    });
    let found = found.lock().unwrap();
    found.clone()
}

/// A player 10 voxels above a group that goes 38 voxels a tick, sent to where it will be a tick on (the command is heard a tick after, and the reach is 48 voxels).
pub fn keep_up_with(scene: &mut Scene, player: &mut Player, ships: &[GridId]) {
    let (place, going) = chains::centre_of(scene, ships);
    let ahead = 1.0 / f64::from(chains::TICKS_PER_SECOND);
    player.send_command(&format!(
        "/tp {} {} {}",
        place[0] + going[0] * ahead,
        place[1] + going[1] * ahead + 10.0,
        place[2] + going[2] * ahead
    ));
    tick_and_follow(scene, player, 1);
}

/// A chain of three cars on a rail round a planet, with a player who keeps up with it (`keep_up_with`).
pub struct ChainOnARail {
    pub scene: Scene,
    pub report: Report,
    pub player: Player,
    /// The three cars, in order.
    pub ships: Vec<GridId>,
}

/// A chain of three cars made in orbit round the planet, put on its rail, with a player keeping up with it for twenty ticks: the group rides the rail of its anchor, the first car.
pub fn a_chain_on_a_rail() -> ChainOnARail {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let distance = 70_016.0;
    let speed = circular_speed(&scene, 0, distance);
    // (The player is there, and has settled, before the chain is made: how many ticks a join takes to settle is not the same from one run to the next, and a chain that goes 38 voxels a
    // tick is frozen when the player is 110 behind, which made two runs of this differ by a whole number of ticks.)
    let mut player = scene.rig.join([0.0, distance + 10.0, 0.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    scene
        .rig
        .server
        .set_ship_settings(ShipSettings::new(100.0, 300.0).unwrap());
    let ships = chains::a_chain_in_orbit(&mut scene, chains::a_chain(), distance, speed);
    assert_eq!(report().couplings, 2);
    let rail = chains::put_on_its_rail(&mut scene, &ships, speed, chains::a_swing());
    for _ in 0..20 {
        keep_up_with(&mut scene, &mut player, &ships);
    }
    assert_eq!(
        scene.rig.server.world().grids().get(ships[0]).unwrap().rail,
        Some(rail)
    );
    ChainOnARail {
        scene,
        report: Box::new(report),
        player,
        ships,
    }
}
