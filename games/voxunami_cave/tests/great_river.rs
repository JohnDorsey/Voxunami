//! The landmarks of the great rivers as places (`docs/specs/great-rivers-spec.md`, `docs/specs/places-spec.md`): `/tp base_blocks:great_river_narrowest` and `..._widest` put a player in view of that knot of the
//! river, with the knot ahead of where they look, and tell them how far.

mod common;

use common::{SEED, Scene, player_pose, say};
use game_shell::GameDefinition;
use terrain::DensityField;
use voxel_space::{GridId, LevelIndex, LevelOfDetail, VoxelCoordinate, WorldPosition};
use voxunami_cave::Cave;
use voxunami_cave::generators::{GreatRivers, RiverKnot};
use voxunami_cave::rivers::{forward, viewing_distance};

/// Where the eyes are, as whole voxels (exact, which `in_voxels` is not at this distance from the origin).
fn voxel_of(position: WorldPosition) -> [i128; 3] {
    let (brick, offset) = (position.brick(), position.offset());
    [
        brick.x() * 32 + offset[0].floor() as i128,
        brick.y() * 32 + offset[1].floor() as i128,
        brick.z() * 32 + offset[2].floor() as i128,
    ]
}

fn distance_between(a: [i128; 3], b: [i128; 3]) -> f64 {
    let squared: f64 = (0..3)
        .map(|axis| {
            let difference = (a[axis] - b[axis]) as f64;
            difference * difference
        })
        .sum();
    squared.sqrt()
}

fn is_water(rivers: &GreatRivers, place: [i128; 3]) -> bool {
    rivers.field().sample(
        VoxelCoordinate::new(place[0], place[1], place[2]),
        LevelOfDetail::FULL_DETAIL,
    ) > 0.0
}

/// A scene in a new world with a player in it, who has said nothing yet.
fn scene_with_a_player() -> (Scene, common::Player) {
    let mut scene = Scene::in_a_new_world();
    let start = Cave::default_camera().position;
    let mut player = scene.rig.join_walking(start, 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    player.take_command_replies();
    (scene, player)
}

fn eyes_of_the_player(scene: &Scene) -> [i128; 3] {
    let pose = player_pose(scene, 1);
    assert_eq!(
        pose.grid,
        GridId::root(LevelIndex::MAIN),
        "in the level, not aboard the planet"
    );
    voxel_of(pose.position)
}

#[test]
fn the_player_is_put_the_viewing_distance_from_the_narrowest_knot_outside_the_water() {
    let (mut scene, mut player) = scene_with_a_player();
    let reply = say(
        &mut scene,
        &mut player,
        "/tp base_blocks:great_river_narrowest",
    );
    let rivers = GreatRivers::new(SEED);
    let knot: RiverKnot = rivers.narrowest_knot();
    assert!(
        reply.starts_with("you are 320 m from base_blocks:great_river_narrowest (knot "),
        "{reply}"
    );
    assert!(reply.ends_with(", 80 m across)"), "{reply}");
    let eyes = eyes_of_the_player(&scene);
    let distance = distance_between(eyes, knot.position);
    assert!(
        (distance - viewing_distance(&knot)).abs() < 2.0,
        "{distance} from the knot"
    );
    assert!(!is_water(&rivers, eyes));
    // The player is at rest where they were put: another tick moves them nowhere near the water.
    scene.rig.settle(&mut [&mut player]);
    let later = eyes_of_the_player(&scene);
    assert!(distance_between(later, eyes) < 50.0);
}

#[test]
fn the_knot_is_straight_ahead_of_the_way_the_player_looks() {
    let rivers = GreatRivers::new(SEED);
    let knot = rivers.narrowest_knot();
    // (If the place back along the view were in the river the player would be put at right angles to it instead, which is tested in `rivers`.)
    let (mut scene, mut player) = scene_with_a_player();
    say(
        &mut scene,
        &mut player,
        "/tp base_blocks:great_river_narrowest",
    );
    let eyes = eyes_of_the_player(&scene);
    let back = forward(0.0, 0.0).map(|component| -component);
    let wanted =
        [0, 1, 2].map(|axis| knot.position[axis] + (back[axis] * viewing_distance(&knot)) as i128);
    let at_right_angles = knot.viewpoint();
    let near = |a: [i128; 3], b: [i128; 3]| distance_between(a, b) < 3.0;
    assert!(
        near(eyes, wanted) || near(eyes, at_right_angles),
        "{eyes:?} is neither {wanted:?} nor {at_right_angles:?}"
    );
}

#[test]
fn the_server_puts_the_player_back_along_the_way_they_look_whichever_way_that_is() {
    // Four ways round: the knot is straight ahead for each, except when the place back along the view is in the river (then the player is put at right angles to
    // it). At least one of the four is not in the river, or the test would show nothing.
    let rivers = GreatRivers::new(SEED);
    let knot = rivers.narrowest_knot();
    let mut straight_ahead = 0;
    for turns in 0..4 {
        // (The rig takes degrees.)
        let degrees = 90.0 * turns as f32;
        let yaw = degrees.to_radians();
        let mut scene = Scene::in_a_new_world();
        let mut player = scene
            .rig
            .join_walking(Cave::default_camera().position, degrees, 0.0);
        scene.rig.settle(&mut [&mut player]);
        say(
            &mut scene,
            &mut player,
            "/tp base_blocks:great_river_narrowest",
        );
        let eyes = eyes_of_the_player(&scene);
        let back = forward(yaw, 0.0).map(|component| -component);
        let wanted = [0, 1, 2]
            .map(|axis| knot.position[axis] + (back[axis] * viewing_distance(&knot)) as i128);
        if distance_between(eyes, wanted) < 3.0 {
            straight_ahead += 1;
        } else {
            assert!(
                distance_between(eyes, knot.viewpoint()) < 3.0,
                "yaw {yaw}: {eyes:?} is neither {wanted:?} nor the viewpoint"
            );
            assert!(
                is_water(&rivers, wanted),
                "yaw {yaw}: the place back was not in the river"
            );
        }
    }
    assert!(straight_ahead >= 2, "{straight_ahead} of four ways round");
}

#[test]
fn the_player_is_put_three_radii_and_two_hundred_metres_from_the_widest_knot() {
    let (mut scene, mut player) = scene_with_a_player();
    let reply = say(
        &mut scene,
        &mut player,
        "/tp base_blocks:great_river_widest",
    );
    let rivers = GreatRivers::new(SEED);
    let knot = rivers.widest_knot();
    assert!(
        reply.contains("light years from base_blocks:great_river_widest (knot")
            && reply.ends_with(", 1.000e9 light years across)"),
        "{reply}"
    );
    let eyes = eyes_of_the_player(&scene);
    let distance = distance_between(eyes, knot.position);
    let wanted = viewing_distance(&knot);
    assert!(
        (distance / wanted - 1.0).abs() < 1.0e-9,
        "{distance} against {wanted}"
    );
    assert!(!is_water(&rivers, eyes));
}

#[test]
fn the_player_can_go_from_the_river_to_the_overworld_and_back_to_the_planet() {
    // The river is a place in the level like any other: leaving the level and coming back puts the player at the pole of the planet again.
    let (mut scene, mut player) = scene_with_a_player();
    say(
        &mut scene,
        &mut player,
        "/tp base_blocks:great_river_narrowest",
    );
    say(&mut scene, &mut player, "/level go overworld");
    let reply = say(&mut scene, &mut player, "/level go mini_cosmos");
    assert!(
        reply.contains("you are now in voxunami_cave:mini_cosmos"),
        "{reply}"
    );
    common::tick_and_follow(&mut scene, &mut player, 60);
    assert_eq!(
        player_pose(&scene, 1).grid,
        common::ship_id(0),
        "aboard the planet"
    );
}

#[test]
fn from_the_overworld_the_player_is_taken_to_the_river_in_the_level_of_space() {
    let (mut scene, mut player) = scene_with_a_player();
    let reply = say(&mut scene, &mut player, "/level go overworld");
    assert!(
        reply.contains("you are now in voxunami_cave:overworld"),
        "{reply}"
    );
    let reply = say(
        &mut scene,
        &mut player,
        "/tp base_blocks:great_river_narrowest",
    );
    assert!(
        reply.starts_with("you are 320 m from base_blocks:great_river_narrowest"),
        "{reply}"
    );
    assert!(reply.contains("voxunami_cave:mini_cosmos"), "{reply}");
    assert_eq!(player.level(), LevelIndex::MAIN, "the client is told");
    let rivers = GreatRivers::new(SEED);
    let eyes = eyes_of_the_player(&scene);
    assert!((distance_between(eyes, rivers.narrowest_knot().position) - 320.0).abs() < 2.0);
}

#[test]
fn a_world_without_the_rivers_has_no_landmark_of_them() {
    // The overworld alone has no great river: its landmarks are no places.
    let mut rig = common::Rig::new();
    let mut player = rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    player.take_command_replies();
    player.send_command("/tp base_blocks:great_river_widest");
    rig.step();
    rig.step();
    rig.settle(&mut [&mut player]);
    assert_eq!(
        player.take_command_replies(),
        ["there is no place base_blocks:great_river_widest; /tp alone lists the places"]
    );
}
