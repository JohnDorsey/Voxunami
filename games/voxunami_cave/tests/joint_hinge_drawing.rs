//! The cylinder of a hinge (`docs/decisions/joints-and-cars.md`, "The dev hinge joint"): where two hinge ends whose lines are one line are coupled, or locked, there is an entity of the kind
//! `engine:joint_hinge` where there would be a ball, aboard the car that has the first of the two ends, at the middle of the face where they meet, turned so that its length runs along the line
//! of the hinge. Any other pair of ends has the ball. A client draws it as a cylinder, turned as the car is.

mod common;

use common::joints::{
    a_player_and_cars, a_player_at_and_cars_posed, east_hinge_car, hinge_block, minus, two_cars,
    two_hinge_cars, until_coupled, west_car, west_hinge_car, world_of,
};
use common::{FLOOR, Player, Scene, length, say, ship_id, tick_and_follow};
use entities::ComponentBag;
use grid_bodies::{JointRigid, joint_ball_kind, joint_hinge_kind};
use renderer::EntityShape;
use voxel_space::{
    BrickCoordinate, EntityPose, FaceDirection, GridId, GridPose, Orientation, WorldPosition,
};

/// What the cylinders there are are: the ship each is aboard, where in the ship (in voxels), which way its length runs in the ship's own axes, and whether it is marked as the cylinder of a
/// rigid joint, in order of identifier.
fn hinges(scene: &Scene) -> Vec<(GridId, [f64; 3], [f64; 3], bool)> {
    scene
        .rig
        .server
        .world()
        .entities()
        .iter()
        .filter(|entity| entity.kind == joint_hinge_kind())
        .map(|entity| {
            (
                entity.pose.grid,
                entity.pose.position.in_voxels(),
                the_length_of_a_cylinder_turned(entity.pose.orientation),
                entity
                    .components
                    .get::<JointRigid>()
                    .expect("the component can be read")
                    .expect("a cylinder has it")
                    .0,
            )
        })
        .collect()
}

/// The identifier of the one cylinder there is.
fn the_cylinder(scene: &Scene) -> protocol::EntityId {
    let found: Vec<_> = scene
        .rig
        .server
        .world()
        .entities()
        .iter()
        .filter(|entity| entity.kind == joint_hinge_kind())
        .map(|entity| entity.id)
        .collect();
    assert_eq!(found.len(), 1, "{found:?}");
    found[0]
}

fn balls(scene: &Scene) -> Vec<(GridId, [f64; 3])> {
    scene
        .rig
        .server
        .world()
        .entities()
        .iter()
        .filter(|entity| entity.kind == joint_ball_kind())
        .map(|entity| (entity.pose.grid, entity.pose.position.in_voxels()))
        .collect()
}

/// Which way the length of a cylinder (along x as it is made) runs when it is turned: the image of x, rounded (the turns are of the lattice, so each part is 0, 1 or -1 to rounding).
fn the_length_of_a_cylinder_turned(orientation: Orientation) -> [f64; 3] {
    let turn = GridPose::new(
        WorldPosition::new(BrickCoordinate::ORIGIN, [0.0; 3]),
        orientation,
    )
    .rotation_matrix();
    [0, 1, 2].map(|row| (turn[row][0] * 1e6).round() / 1e6)
}

/// The same, with the sign dropped: a cylinder is the same one turned half way round.
fn along(axis: usize) -> [f64; 3] {
    let mut line = [0.0; 3];
    line[axis] = 1.0;
    line
}

fn the_same_line(found: [f64; 3], wanted: [f64; 3]) -> bool {
    found == wanted || found == wanted.map(|part| -part)
}

fn until_hinges(scene: &mut Scene, player: &mut Player, count: usize, limit: usize) {
    let mut ticks = 0;
    while hinges(scene).len() != count && ticks < limit {
        tick_and_follow(scene, player, 1);
        ticks += 1;
    }
    assert_eq!(hinges(scene).len(), count, "after {ticks} ticks");
}

#[test]
fn a_hinge_coupling_has_a_cylinder_along_its_line_where_its_ends_meet_and_no_ball_and_has_it_only_while_it_is_coupled()
 {
    let (mut scene, report, mut player) =
        two_hinge_cars(FaceDirection::PositiveZ, FaceDirection::PositiveZ);
    assert!(hinges(&scene).is_empty(), "the cars are three metres apart");
    until_coupled(&mut scene, &mut player, &report, 1, 600);
    until_hinges(&mut scene, &mut player, 1, 5);
    let found = hinges(&scene);
    assert_eq!((found[0].0, found[0].1), (ship_id(0), [4.0, 0.5, 0.5]));
    assert!(
        the_same_line(found[0].2, along(2)),
        "its length runs along z, the line of the hinge: {:?}",
        found[0].2
    );
    assert!(!found[0].3, "a hinge that turns is not marked rigid");
    assert!(balls(&scene).is_empty(), "a hinge has no ball");
    assert!(say(&mut scene, &mut player, "/joint decouple").contains("decoupled"));
    tick_and_follow(&mut scene, &mut player, 2);
    assert!(hinges(&scene).is_empty(), "parted, there is no cylinder");
}

#[test]
fn the_cylinder_runs_along_the_line_of_whichever_axis_the_hinge_turns_about() {
    for (line, axis) in [
        (FaceDirection::PositiveY, 1),
        (FaceDirection::NegativeY, 1),
        (FaceDirection::PositiveZ, 2),
        (FaceDirection::NegativeZ, 2),
    ] {
        let (mut scene, report, mut player) = two_hinge_cars(line, line);
        until_coupled(&mut scene, &mut player, &report, 1, 600);
        until_hinges(&mut scene, &mut player, 1, 5);
        let found = hinges(&scene);
        assert!(
            the_same_line(found[0].2, along(axis)),
            "{line:?}: {:?}",
            found[0].2
        );
    }
}

#[test]
fn hinges_whose_lines_cross_and_a_hinge_and_a_ball_have_a_ball_and_no_cylinder() {
    for (name, mut cars) in [
        (
            "lines along z and along y",
            two_hinge_cars(FaceDirection::PositiveZ, FaceDirection::PositiveY),
        ),
        (
            "a hinge and a ball",
            a_player_and_cars(
                22.0,
                vec![
                    (20.0, east_hinge_car(3, FaceDirection::PositiveZ)),
                    (27.0, west_car(3)),
                ],
            ),
        ),
    ] {
        until_coupled(&mut cars.0, &mut cars.2, &cars.1, 1, 600);
        tick_and_follow(&mut cars.0, &mut cars.2, 5);
        assert_eq!(balls(&cars.0).len(), 1, "{name}");
        assert!(hinges(&cars.0).is_empty(), "{name}");
    }
}

#[test]
fn a_ball_coupling_has_no_cylinder() {
    let (mut scene, report, mut player) = two_cars();
    until_coupled(&mut scene, &mut player, &report, 1, 600);
    tick_and_follow(&mut scene, &mut player, 5);
    assert_eq!(balls(&scene).len(), 1);
    assert!(hinges(&scene).is_empty());
}

/// The two hinge cars as one ship, as a lock makes them: the ends of their hinges are in cells that touch, at 3 and 4.
fn the_two_hinge_cars_locked(
    east: FaceDirection,
    west: FaceDirection,
) -> Vec<common::docking::Placed> {
    let mut blocks = east_hinge_car(3, east);
    blocks.extend(
        west_hinge_car(3, west)
            .into_iter()
            .map(|([x, y, z], block, look)| ([x + 4, y, z], block, look)),
    );
    blocks
}

#[test]
fn a_ship_whose_two_hinge_ends_face_each_other_on_one_line_has_a_rigid_cylinder_there_and_one_whose_lines_cross_has_a_ball()
 {
    let (mut scene, _report, mut player) = a_player_and_cars(
        24.0,
        vec![(
            20.0,
            the_two_hinge_cars_locked(FaceDirection::PositiveZ, FaceDirection::PositiveZ),
        )],
    );
    until_hinges(&mut scene, &mut player, 1, 10);
    let found = hinges(&scene);
    assert_eq!((found[0].0, found[0].1), (ship_id(0), [4.0, 0.5, 0.5]));
    assert!(the_same_line(found[0].2, along(2)), "{:?}", found[0].2);
    assert!(found[0].3, "a locked joint is rigid");
    assert!(balls(&scene).is_empty());

    let (mut crossed, _report, mut player) = a_player_and_cars(
        24.0,
        vec![(
            20.0,
            the_two_hinge_cars_locked(FaceDirection::PositiveZ, FaceDirection::PositiveY),
        )],
    );
    tick_and_follow(&mut crossed, &mut player, 10);
    assert_eq!(balls(&crossed), vec![(ship_id(0), [4.0, 0.5, 0.5])]);
    assert!(hinges(&crossed).is_empty());
}

#[test]
fn the_cylinder_is_marked_rigid_while_its_joint_is_rigid_and_is_the_same_entity_when_it_is_let_turn_again()
 {
    let (mut scene, report, mut player) =
        two_hinge_cars(FaceDirection::PositiveZ, FaceDirection::PositiveZ);
    until_coupled(&mut scene, &mut player, &report, 1, 600);
    until_hinges(&mut scene, &mut player, 1, 5);
    let id = the_cylinder(&scene);
    say(&mut scene, &mut player, "/joint immobilize 0");
    tick_and_follow(&mut scene, &mut player, 4);
    assert!(hinges(&scene)[0].3);
    // A player is shown the cylinder of a rigid joint in the lamp's colour, as the ball of one is, and in the hinge's own colour when it turns.
    tick_and_follow(&mut scene, &mut player, 4);
    assert_eq!(
        player.entity_boxes()[0].material,
        voxunami_cave::content::CaveContent::new().blocks.lamp
    );
    say(&mut scene, &mut player, "/joint mobilize");
    tick_and_follow(&mut scene, &mut player, 4);
    assert!(!hinges(&scene)[0].3);
    assert_eq!(
        the_cylinder(&scene),
        id,
        "marked and unmarked, not made again"
    );
}

#[test]
fn a_cylinder_that_belongs_to_no_coupling_or_lock_is_taken_away_and_so_is_a_second_one_or_a_ball_at_the_same_place()
 {
    let (mut scene, report, mut player) =
        two_hinge_cars(FaceDirection::PositiveZ, FaceDirection::PositiveZ);
    until_coupled(&mut scene, &mut player, &report, 1, 600);
    until_hinges(&mut scene, &mut player, 1, 5);
    let real = the_cylinder(&scene);
    scene.run(move |world| {
        let at = world.entities().get(real).unwrap().pose;
        // One aboard a car where no ends meet; a twin of the real one (the same place, the same turn); and a ball where the cylinder is.
        let aboard = EntityPose::new(
            ship_id(1),
            WorldPosition::new(BrickCoordinate::ORIGIN, [2.0, 0.5, 0.5]),
            at.orientation,
        );
        for (kind, pose) in [
            (joint_hinge_kind(), aboard),
            (joint_hinge_kind(), at),
            (joint_ball_kind(), at),
        ] {
            world
                .spawn_entity(&kind, pose, ComponentBag::new())
                .unwrap();
        }
    });
    tick_and_follow(&mut scene, &mut player, 2);
    assert_eq!(
        the_cylinder(&scene),
        real,
        "the one that is a coupling's stays, and it is the only one"
    );
    assert!(balls(&scene).is_empty(), "a hinge has no ball");
}

#[test]
fn a_ball_where_the_cylinder_should_be_is_taken_away_and_the_cylinder_is_made() {
    let (mut scene, report, mut player) =
        two_hinge_cars(FaceDirection::PositiveZ, FaceDirection::PositiveZ);
    until_coupled(&mut scene, &mut player, &report, 1, 600);
    until_hinges(&mut scene, &mut player, 1, 5);
    let real = the_cylinder(&scene);
    // In one tick the cylinder is taken away and a ball put at its place, turned as the cylinder was: only the kind says it is not what is wanted.
    scene.run(move |world| {
        let at = world.entities().get(real).unwrap().pose;
        world.despawn_entity(real);
        world
            .spawn_entity(&joint_ball_kind(), at, ComponentBag::new())
            .unwrap();
    });
    tick_and_follow(&mut scene, &mut player, 3);
    assert!(balls(&scene).is_empty(), "a hinge has no ball");
    assert_eq!(hinges(&scene).len(), 1);
}

#[test]
fn a_cylinder_whose_line_is_not_the_hinges_is_taken_away_and_made_again_along_it() {
    let (mut scene, report, mut player) =
        two_hinge_cars(FaceDirection::PositiveZ, FaceDirection::PositiveZ);
    until_coupled(&mut scene, &mut player, &report, 1, 600);
    until_hinges(&mut scene, &mut player, 1, 5);
    let (wrong, real) = (the_cylinder(&scene), the_cylinder(&scene));
    // The cylinder turned about the bar a quarter turn: its length runs along y, not along the line of the hinge.
    scene.run(move |world| {
        let mut at = world.entities().get(wrong).unwrap().pose;
        at.orientation = Orientation::from_components([
            0.0,
            0.0,
            std::f64::consts::FRAC_1_SQRT_2,
            std::f64::consts::FRAC_1_SQRT_2,
        ])
        .unwrap();
        world.set_entity_pose(wrong, at).unwrap();
    });
    tick_and_follow(&mut scene, &mut player, 3);
    let found = hinges(&scene);
    assert_eq!(found.len(), 1);
    assert!(the_same_line(found[0].2, along(2)), "{:?}", found[0].2);
    assert_ne!(
        the_cylinder(&scene),
        real,
        "the one that was turned wrongly is gone, and a right one is made"
    );
}

/// A turn about y by 90 degrees (the way a ship that lies along x lies along z).
fn a_quarter_turn_about_x() -> Orientation {
    let half = std::f64::consts::FRAC_1_SQRT_2;
    Orientation::from_components([half, 0.0, 0.0, half]).expect("a turn about x")
}

#[test]
fn a_player_is_shown_the_cylinder_in_the_hinge_blocks_colour_where_the_ends_meet_turned_with_the_cars()
 {
    // Two hinge cars turned a quarter turn about x (they still lie along x): the line of their hinge, z in the cars, is y in the world, which is neither the way an upright cylinder
    // lies (x) nor the way one that is not turned with its car does (z).
    let at = |x: f64| {
        GridPose::new(
            WorldPosition::new(BrickCoordinate::ORIGIN, [x, common::joints::LEVEL, 20.0]),
            a_quarter_turn_about_x(),
        )
    };
    let (mut scene, report, mut player) = a_player_at_and_cars_posed(
        [22.0, FLOOR + 5.0, 30.0],
        vec![
            (at(20.0), east_hinge_car(3, FaceDirection::PositiveZ)),
            (at(27.0), west_hinge_car(3, FaceDirection::PositiveZ)),
        ],
    );
    until_coupled(&mut scene, &mut player, &report, 1, 600);
    until_hinges(&mut scene, &mut player, 1, 5);
    tick_and_follow(&mut scene, &mut player, 5);
    let boxes = player.entity_boxes();
    assert_eq!(boxes.len(), 1, "{boxes:?}");
    let drawn = boxes[0];
    assert_eq!(drawn.shape, EntityShape::Cylinder);
    assert_eq!(drawn.material, hinge_block());
    let wanted = world_of(&scene, ship_id(0), [4.0, 0.5, 0.5]);
    let apart = length(minus(drawn.position.in_voxels(), wanted));
    assert!(
        apart < 0.01,
        "the cylinder is drawn {apart} voxels from the pivot"
    );
    assert!(
        the_same_line(the_length_of_a_cylinder_turned(drawn.orientation), along(1)),
        "drawn along the hinge, which is along y in the world: {:?}",
        the_length_of_a_cylinder_turned(drawn.orientation)
    );
}
