//! The ball of a joint (`docs/decisions/joints-and-cars.md`, "The block": *a ball of 0.5 m diameter is drawn in the gap, and only then*): where two joint ends are coupled or locked there
//! is an entity of the kind `engine:joint_ball`, aboard the car that has the first of the two ends, at the middle of the face where the two meet. It is the server's, so every client
//! (and every recording) is told of it as of any entity; what a client draws for it is in the client's tests. Nothing is saved of it that a load does not put right.

mod common;

use common::docking::{PLAIN, ships, stone};
use common::joints::{
    a_player_and_cars, east_car, middle_car, minus, two_cars, until_coupled, west_car, world_of,
};
use common::{Player, Scene, length, say, ship_id, tick_and_follow};
use entities::ComponentBag;
use grid_bodies::joint_ball_kind;
use protocol::EntityId;
use voxel_space::{BrickCoordinate, EntityPose, GridId, Orientation, WorldPosition};

/// Every ball there is: its identifier, the ship it is aboard and where in the ship (in voxels), in order of identifier.
fn balls(scene: &Scene) -> Vec<(EntityId, GridId, [f64; 3])> {
    scene
        .rig
        .server
        .world()
        .entities()
        .iter()
        .filter(|entity| entity.kind == joint_ball_kind())
        .map(|entity| {
            (
                entity.id,
                entity.pose.grid,
                entity.pose.position.in_voxels(),
            )
        })
        .collect()
}

fn places_of(balls: &[(EntityId, GridId, [f64; 3])]) -> Vec<(GridId, [f64; 3])> {
    balls
        .iter()
        .map(|(_, ship, place)| (*ship, *place))
        .collect()
}

/// The two cars of a ship that was made by a lock: the ends of their joints are in cells that touch, at 3 and 4.
fn the_two_cars_locked() -> Vec<common::docking::Placed> {
    let mut blocks = east_car(3);
    blocks.extend(
        west_car(3)
            .into_iter()
            .map(|([x, y, z], block, look)| ([x + 4, y, z], block, look)),
    );
    blocks
}

fn until_a_ball(scene: &mut Scene, player: &mut Player, count: usize, limit: usize) {
    let mut ticks = 0;
    while balls(scene).len() != count && ticks < limit {
        tick_and_follow(scene, player, 1);
        ticks += 1;
    }
    assert_eq!(balls(scene).len(), count, "after {ticks} ticks");
}

#[test]
fn a_coupling_has_a_ball_where_its_ends_meet_aboard_the_first_car_and_has_it_only_while_it_is_coupled()
 {
    let (mut scene, report, mut player) = two_cars();
    assert!(balls(&scene).is_empty(), "the cars are three metres apart");
    until_coupled(&mut scene, &mut player, &report, 1, 600);
    until_a_ball(&mut scene, &mut player, 1, 5);
    // The middle of the face of the east car's end: the end is the joint block at 3, its face the east side of the cell.
    assert_eq!(
        places_of(&balls(&scene)),
        vec![(ship_id(0), [4.0, 0.5, 0.5])]
    );
    assert!(say(&mut scene, &mut player, "/joint decouple").contains("decoupled"));
    tick_and_follow(&mut scene, &mut player, 2);
    assert!(balls(&scene).is_empty(), "parted, there is no ball");
}

#[test]
fn a_ball_that_is_there_is_the_same_entity_tick_after_tick_and_is_not_made_again() {
    let (mut scene, report, mut player) = two_cars();
    until_coupled(&mut scene, &mut player, &report, 1, 600);
    until_a_ball(&mut scene, &mut player, 1, 5);
    let first = balls(&scene);
    tick_and_follow(&mut scene, &mut player, 60);
    assert_eq!(balls(&scene), first, "one ball, as it was");
}

#[test]
fn a_chain_has_a_ball_at_each_of_its_couplings() {
    let (mut scene, report, mut player) = a_player_and_cars(
        25.0,
        vec![
            (20.0, east_car(3)),
            (27.0, middle_car()),
            (36.0, west_car(3)),
        ],
    );
    until_coupled(&mut scene, &mut player, &report, 2, 900);
    until_a_ball(&mut scene, &mut player, 2, 5);
    // The first coupling at the east end of the first car, the second at the east end of the middle car (a joint at its west end, three stones, a joint at 4: the face at 5).
    assert_eq!(
        places_of(&balls(&scene)),
        vec![(ship_id(0), [4.0, 0.5, 0.5]), (ship_id(1), [5.0, 0.5, 0.5])]
    );
}

#[test]
fn a_ship_that_was_made_of_cars_by_a_lock_has_a_ball_at_the_ends_that_are_locked() {
    let (mut scene, mut player) = {
        let (scene, _report, player) = a_player_and_cars(24.0, vec![(20.0, the_two_cars_locked())]);
        (scene, player)
    };
    until_a_ball(&mut scene, &mut player, 1, 10);
    assert_eq!(
        places_of(&balls(&scene)),
        vec![(ship_id(0), [4.0, 0.5, 0.5])]
    );
}

#[test]
fn a_lock_keeps_the_ball_it_is_the_same_entity_on_the_ship_that_is_made_and_there_is_one() {
    let (mut scene, report, mut player) = two_cars();
    until_coupled(&mut scene, &mut player, &report, 1, 600);
    until_a_ball(&mut scene, &mut player, 1, 5);
    let before = balls(&scene);
    assert!(say(&mut scene, &mut player, "/joint lock").contains("locking"));
    let mut ticks = 0;
    while ships(&scene).len() > 1 && ticks < 60 {
        tick_and_follow(&mut scene, &mut player, 1);
        ticks += 1;
    }
    assert_eq!(ships(&scene).len(), 1, "locked after {ticks} ticks");
    tick_and_follow(&mut scene, &mut player, 5);
    let after = balls(&scene);
    assert_eq!(after.len(), 1, "{after:?}");
    assert_eq!(
        after[0].0, before[0].0,
        "the same ball: a merge carries what is aboard"
    );
    assert_eq!(after[0].1, ships(&scene)[0], "aboard the ship that is made");
    assert_eq!(after[0].2, [4.0, 0.5, 0.5]);
}

#[test]
fn unlocking_leaves_a_ball_for_the_cars_that_are_coupled_again_and_never_two() {
    let (mut scene, mut player) = {
        let (scene, _report, player) = a_player_and_cars(24.0, vec![(20.0, the_two_cars_locked())]);
        (scene, player)
    };
    until_a_ball(&mut scene, &mut player, 1, 10);
    assert!(say(&mut scene, &mut player, "/joint unlock").contains("unlocked"));
    // (The cars are coupled again in a tick or two: until then the ball is on whichever car it went with or none.)
    tick_and_follow(&mut scene, &mut player, 30);
    let found = balls(&scene);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        ships(&scene).contains(&found[0].1),
        "aboard a car that is there"
    );
    assert_eq!(found[0].2, [4.0, 0.5, 0.5]);
}

#[test]
fn a_ball_that_belongs_to_no_coupling_or_lock_is_taken_away_whether_a_world_kept_it_or_a_command_made_it()
 {
    let (mut scene, report, mut player) = two_cars();
    until_coupled(&mut scene, &mut player, &report, 1, 600);
    until_a_ball(&mut scene, &mut player, 1, 5);
    let real = balls(&scene)[0].0;
    // Two more: one aboard a car, where no ends meet, and one in the level. (A tick follows what is asked of the world, and takes them away in it.)
    let made = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = std::sync::Arc::clone(&made);
    scene.run(move |world| {
        let aboard = EntityPose::new(
            ship_id(1),
            WorldPosition::new(BrickCoordinate::ORIGIN, [2.0, 0.5, 0.5]),
            Orientation::IDENTITY,
        );
        let in_the_level = EntityPose::new(
            GridId::root(voxel_space::LevelIndex::MAIN),
            WorldPosition::new(BrickCoordinate::ORIGIN, [30.0, 205.0, 30.0]),
            Orientation::IDENTITY,
        );
        for pose in [aboard, in_the_level] {
            let id = world
                .spawn_entity(&joint_ball_kind(), pose, ComponentBag::new())
                .unwrap();
            sink.lock().unwrap().push(id);
        }
    });
    tick_and_follow(&mut scene, &mut player, 2);
    for id in made.lock().unwrap().iter() {
        assert!(
            scene.rig.server.world().entities().get(*id).is_none(),
            "{id:?} is still there"
        );
    }
    let left = balls(&scene);
    assert_eq!(left.len(), 1, "{left:?}");
    assert_eq!(left[0].0, real, "the one that is a coupling's stays");
}

#[test]
fn a_second_ball_at_the_same_place_is_taken_away_and_the_first_is_kept() {
    let (mut scene, report, mut player) = two_cars();
    until_coupled(&mut scene, &mut player, &report, 1, 600);
    until_a_ball(&mut scene, &mut player, 1, 5);
    let real = balls(&scene)[0].0;
    scene.run(|world| {
        let at = EntityPose::new(
            ship_id(0),
            WorldPosition::new(BrickCoordinate::ORIGIN, [4.0, 0.5, 0.5]),
            Orientation::IDENTITY,
        );
        world
            .spawn_entity(&joint_ball_kind(), at, ComponentBag::new())
            .unwrap();
    });
    tick_and_follow(&mut scene, &mut player, 2);
    let left = balls(&scene);
    assert_eq!(left.len(), 1, "{left:?}");
    assert_eq!(left[0].0, real);
}

#[test]
fn cars_that_are_only_near_each_other_have_no_ball() {
    // Two stones facing nothing: no joint ends, no ball.
    let (mut scene, _report, mut player) =
        a_player_and_cars(22.0, vec![(20.0, vec![([0, 0, 0], stone(), PLAIN)])]);
    tick_and_follow(&mut scene, &mut player, 5);
    assert!(balls(&scene).is_empty());
}

#[test]
fn a_player_is_shown_the_ball_as_a_ball_of_the_joints_colour_where_the_ends_meet_and_it_goes_with_the_ship()
 {
    let (mut scene, report, mut player) = two_cars();
    until_coupled(&mut scene, &mut player, &report, 1, 600);
    until_a_ball(&mut scene, &mut player, 1, 5);
    // The client is told of the entity in the tick it is made, and the ship it is aboard in the tick before: a few ticks on, it draws it.
    tick_and_follow(&mut scene, &mut player, 5);
    let boxes = player.entity_boxes();
    assert_eq!(boxes.len(), 1, "{boxes:?}");
    let drawn = boxes[0];
    assert_eq!(drawn.shape, renderer::EntityShape::Ball);
    assert_eq!(drawn.material, common::joints::joint_block());
    // Where the ends meet, in the world: the east car's end at (4, 0.5, 0.5) in the car, which is where the world has it.
    let wanted = world_of(&scene, ship_id(0), [4.0, 0.5, 0.5]);
    let at = drawn.position.in_voxels();
    let apart = length(minus(at, wanted));
    assert!(
        apart < 0.01,
        "the ball is drawn {apart} voxels from the pivot"
    );
}
