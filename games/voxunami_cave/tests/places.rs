//! `/tp` to a place (`docs/places-spec.md`): a body by its name, on the ground nearest the player or at a latitude and longitude or at coordinates of its own; the
//! great rivers' landmarks; and from a level to another. The old `/tp X Y Z` is in `commands.rs`.

mod common;

use std::sync::{Arc, Mutex};

use common::{Scene, player_pose, say, ship_id, tick_and_follow, top_of_the_column};
use game_shell::{GameContent, GameDefinition, RawFile, RawPack};
use voxel_space::{GridId, LevelIndex, Orientation, WorldPosition};
use voxunami_cave::Cave;
use voxunami_cave::aboard::in_voxels;
use voxunami_cave::content::CaveContent;

const PLANET: &str = "voxunami_cave:planet";
/// The radius of the first planet and the most its hills add to it (320 where one face's hills are at the highest; the blend of three can be more, so a little over).
const RADIUS: f64 = 63_710.0;
const HILLS: f64 = 400.0;

/// Where the player's eyes are in the world (a player aboard a planet is where it puts them), by the server's record.
fn world_eyes(scene: &mut Scene) -> [f64; 3] {
    let pose = player_pose(scene, 1);
    let found = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&found);
    scene.run(move |world| {
        let position = match pose.grid.root_level() {
            Some(_) => Some(pose.position),
            None => world
                .data()
                .grids()
                .frame_of(pose.grid)
                .and_then(|(_, frame)| frame.local_to_world(pose.position)),
        };
        *slot.lock().unwrap() = position;
    });
    let position = found.lock().unwrap().expect("the player is somewhere");
    in_voxels(position)
}

fn length(vector: [f64; 3]) -> f64 {
    (vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2]).sqrt()
}

/// The cosine of the angle between two vectors.
fn cosine(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]) / (length(a) * length(b))
}

/// The player lets go and settles for ten seconds: they must come to rest close to where they were put (they were put over the ground, not high over it and not in it: a
/// player who is not held up falls a hundred metres in that time, and a player put in the ground is pushed out of it). Whether they are aboard the body then is the rule
/// of the ships and not of the arrival (it probes one point under the feet, which on a slope can miss).
fn stays_where_they_were_put(scene: &mut Scene, player: &mut common::Player, what: &str) {
    let arrived = length(world_eyes(scene));
    tick_and_follow(scene, player, 200);
    let settled = length(world_eyes(scene));
    assert!(
        (settled - arrived).abs() < 3.0,
        "{what}: {arrived} from the centre on arrival, {settled} after ten seconds"
    );
}

/// What a player is told when they are put somewhere is never that they were moved out of the ground: they are put over it.
fn unmoved(reply: &str) {
    assert!(!reply.contains("moved"), "{reply}");
}

/// A scene in a new world with a player standing at the pole of the planet, who has said nothing yet.
fn scene_with_a_player() -> (Scene, common::Player) {
    let mut scene = Scene::in_a_new_world();
    let mut player = scene
        .rig
        .join_walking(Cave::default_camera().position, 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 5);
    player.take_command_replies();
    (scene, player)
}

fn same_world_with_a_moon() -> (Scene, common::Player) {
    let mut content = CaveContent::new();
    content.apply_packs(&moons()).unwrap();
    let world = content.new_world(common::SEED);
    let mut scene = Scene::in_this_world(content, world);
    let mut player = scene
        .rig
        .join_walking(Cave::default_camera().position, 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 5);
    player.take_command_replies();
    (scene, player)
}

/// A pack with a moon of 10,000 voxels 400 kilometres along x from the planet (the same as the tests of the bodies have).
fn moons() -> Vec<RawPack> {
    vec![RawPack {
        folder: "moons".to_owned(),
        manifest: r#"{ "id": "moons" }"#.to_owned(),
        blocks: Vec::new(),
        patches: Vec::new(),
        bodies: vec![RawFile {
            name: "moon".to_owned(),
            text: r#"{
                "level": "voxunami_cave:mini_cosmos",
                "position": [400000, 0, 0],
                "terrain": {
                    "family": "base_blocks:planet_v1",
                    "parameters": { "seed": 99, "radius": 10000, "hill_height": 20,
                        "largest_hill_wavelength_log2": 7, "hill_octave_count": 4,
                        "cave_wavelength_log2": 5, "cave_half_thickness": 0.05 }
                },
                "extent_bricks": 340,
                "surface_gravity": 8
            }"#
            .to_owned(),
        }],
    }]
}

#[test]
fn tp_with_no_words_lists_the_places_of_the_world_and_the_ways_to_say_where() {
    let (mut scene, mut player) = scene_with_a_player();
    let reply = say(&mut scene, &mut player, "/tp");
    for name in [
        PLANET,
        "base_blocks:great_river_narrowest",
        "base_blocks:great_river_widest",
    ] {
        assert!(reply.contains(name), "{name} is not in {reply}");
    }
    assert!(reply.contains("/tp X Y Z"), "{reply}");
    assert!(reply.contains("LATITUDE LONGITUDE"), "{reply}");
}

#[test]
fn a_second_planet_is_a_place_by_its_own_name() {
    let (mut scene, mut player) = same_world_with_a_moon();
    let reply = say(&mut scene, &mut player, "/tp");
    assert!(
        reply.contains("moons:moon") && reply.contains(PLANET),
        "{reply}"
    );
}

#[test]
fn the_planet_by_name_puts_the_player_on_the_ground_nearest_to_where_they_are() {
    let (mut scene, mut player) = scene_with_a_player();
    // 36 kilometres over the +x side.
    say(&mut scene, &mut player, "/tp 100000 0 0");
    let reply = say(&mut scene, &mut player, &format!("/tp {PLANET}"));
    assert!(
        reply.starts_with(&format!(
            "you are on {PLANET} at latitude 0.0, longitude 0.0"
        )),
        "{reply}"
    );
    unmoved(&reply);
    let eyes = world_eyes(&mut scene);
    assert!(cosine(eyes, [1.0, 0.0, 0.0]) > 0.999_999_99, "{eyes:?}");
    let radius = length(eyes);
    assert!(
        (RADIUS - HILLS..=RADIUS + HILLS).contains(&radius),
        "{radius} from the centre"
    );
    stays_where_they_were_put(&mut scene, &mut player, "nearest to the +x side");
    // Over the other side and up: the ground under the line from the centre to the player.
    say(&mut scene, &mut player, "/tp -100000 30000 0");
    let reply = say(&mut scene, &mut player, &format!("/tp {PLANET}"));
    assert!(reply.contains("latitude 16.7, longitude 180.0"), "{reply}");
    unmoved(&reply);
    let eyes = world_eyes(&mut scene);
    assert!(
        cosine(eyes, [-100_000.0, 30_000.0, 0.0]) > 0.999_999_99,
        "{eyes:?}"
    );
    assert!((RADIUS - HILLS..=RADIUS + HILLS).contains(&length(eyes)));
    stays_where_they_were_put(&mut scene, &mut player, "nearest from over -x and up");
}

#[test]
fn two_numbers_are_a_latitude_and_a_longitude_and_the_pole_is_where_players_start() {
    let (mut scene, mut player) = scene_with_a_player();
    // 90 degrees north is the planet's +y axis, the column players arrive in: the same place as the one where a new player stands.
    let reply = say(&mut scene, &mut player, &format!("/tp {PLANET} 90 0"));
    assert!(
        reply.starts_with(&format!(
            "you are on {PLANET} at latitude 90.0, longitude 0.0"
        )),
        "{reply}"
    );
    unmoved(&reply);
    let eyes = world_eyes(&mut scene);
    let top = top_of_the_column(0, 0);
    assert!(
        (eyes[0] - 0.5).abs() < 0.01 && (eyes[2] - 0.5).abs() < 0.01,
        "{eyes:?}"
    );
    assert!(
        (eyes[1] - ((top + 1) as f64 + 1.64)).abs() < 0.1,
        "{eyes:?} over {top}"
    );
    // The equator: longitude 0 is along +x and longitude 90 along -z; the south pole is -y.
    for (words, direction) in [
        ("0 0", [1.0, 0.0, 0.0]),
        ("0 90", [0.0, 0.0, -1.0]),
        ("0 180", [-1.0, 0.0, 0.0]),
        ("0 -90", [0.0, 0.0, 1.0]),
        ("-90 0", [0.0, -1.0, 0.0]),
        (
            "45 0",
            [
                std::f64::consts::FRAC_1_SQRT_2,
                std::f64::consts::FRAC_1_SQRT_2,
                0.0,
            ],
        ),
    ] {
        let reply = say(&mut scene, &mut player, &format!("/tp {PLANET} {words}"));
        unmoved(&reply);
        let eyes = world_eyes(&mut scene);
        assert!(cosine(eyes, direction) > 0.999_999_99, "{words}: {eyes:?}");
        assert!(
            (RADIUS - HILLS..=RADIUS + HILLS).contains(&length(eyes)),
            "{words}: {}",
            length(eyes)
        );
        stays_where_they_were_put(&mut scene, &mut player, words);
    }
}

#[test]
fn three_numbers_are_a_place_in_the_planets_own_axes() {
    let (mut scene, mut player) = scene_with_a_player();
    // Well over the ground, in air: the player is put where they say.
    let reply = say(&mut scene, &mut player, &format!("/tp {PLANET} 0 64600 0"));
    assert!(
        reply.starts_with(&format!("you are at 0 64600 0 of {PLANET}")),
        "{reply}"
    );
    let eyes = world_eyes(&mut scene);
    assert!(
        eyes[0].abs() < 0.1 && (eyes[1] - 64_600.0).abs() < 0.2 && eyes[2].abs() < 0.1,
        "{eyes:?}"
    );
    // Fractions are kept.
    say(
        &mut scene,
        &mut player,
        &format!("/tp {PLANET} 100.5 64650.25 -20"),
    );
    let eyes = world_eyes(&mut scene);
    assert!(
        (eyes[0] - 100.5).abs() < 0.1
            && (eyes[1] - 64_650.25).abs() < 0.2
            && (eyes[2] + 20.0).abs() < 0.1,
        "{eyes:?}"
    );
}

#[test]
fn a_planet_that_is_turned_has_its_pole_where_it_points_and_its_axes_are_its_own() {
    // A quarter turn about the x axis takes the planet's +y (its pole) to the world's +z and its +z to the world's -y.
    let mut scene = Scene::in_a_new_world();
    let quarter = std::f64::consts::FRAC_1_SQRT_2;
    let turned = Orientation::from_components([quarter, 0.0, 0.0, quarter]).unwrap();
    scene.run(move |world| {
        world
            .set_grid_pose(
                ship_id(0),
                voxel_space::GridPose::new(
                    WorldPosition::new(voxel_space::BrickCoordinate::ORIGIN, [0.0; 3]),
                    turned,
                ),
            )
            .unwrap();
    });
    let mut player = scene.rig.join_walking([0.0, 0.0, 100_000.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 5);
    player.take_command_replies();
    // The north pole of the planet is the world's +z.
    say(&mut scene, &mut player, &format!("/tp {PLANET} 90 0"));
    let eyes = world_eyes(&mut scene);
    assert!(cosine(eyes, [0.0, 0.0, 1.0]) > 0.999_999_99, "{eyes:?}");
    assert!((RADIUS - HILLS..=RADIUS + HILLS).contains(&length(eyes)));
    // A place in the planet's axes: (0, 64600, 0) of it is (0, 0, 64600) in the world. (The player's body turns upright to the pull of the planet as it falls, which
    // moves the eyes a metre or so in the first ticks: the same as for `/tp X Y Z`.)
    say(&mut scene, &mut player, &format!("/tp {PLANET} 0 64600 0"));
    let eyes = world_eyes(&mut scene);
    assert!(
        eyes[0].abs() < 2.5 && eyes[1].abs() < 2.5 && (eyes[2] - 64_600.0).abs() < 2.5,
        "{eyes:?}"
    );
}

#[test]
fn a_moon_by_its_name_is_reached_on_its_ground_nearest_to_the_player() {
    let (mut scene, mut player) = same_world_with_a_moon();
    let reply = say(&mut scene, &mut player, "/tp moons:moon");
    assert!(
        reply.starts_with("you are on moons:moon at latitude"),
        "{reply}"
    );
    let eyes = world_eyes(&mut scene);
    let from_the_moon = [eyes[0] - 400_000.0, eyes[1], eyes[2]];
    let distance = length(from_the_moon);
    assert!(
        (9_950.0..=10_060.0).contains(&distance),
        "{distance} from the moon's centre"
    );
    // The player stood at the pole of the planet, 63,712 up: the near side of the moon is toward there, to -x and up.
    assert!(
        cosine(from_the_moon, [-400_000.0, 63_712.0, 0.0]) > 0.999,
        "{from_the_moon:?}"
    );
    // And the planet again from the moon: the side of it that faces where the player is.
    let from_the_moon = eyes;
    say(&mut scene, &mut player, &format!("/tp {PLANET}"));
    let eyes = world_eyes(&mut scene);
    assert!(
        cosine(eyes, from_the_moon) > 0.999_99,
        "{eyes:?} against {from_the_moon:?}"
    );
    assert!((RADIUS - HILLS..=RADIUS + HILLS).contains(&length(eyes)));
}

#[test]
fn from_another_level_a_planet_is_reached_at_its_pole_in_its_level() {
    let (mut scene, mut player) = scene_with_a_player();
    say(&mut scene, &mut player, "/level go overworld");
    let reply = say(&mut scene, &mut player, &format!("/tp {PLANET}"));
    assert!(reply.contains("voxunami_cave:mini_cosmos"), "{reply}");
    // (The client is told, and not only the server: the player's own view is of the level of space.)
    assert_eq!(player.level(), LevelIndex::MAIN);
    let pose = player_pose(&scene, 1);
    assert!(
        pose.grid == GridId::root(LevelIndex::MAIN) || pose.grid == ship_id(0),
        "in the level of space or aboard the planet: {:?}",
        pose.grid
    );
    let eyes = world_eyes(&mut scene);
    assert!(
        cosine(eyes, [0.0, 1.0, 0.0]) > 0.999_999_99,
        "the pole: {eyes:?}"
    );
}

#[test]
fn what_cannot_be_done_is_said() {
    let (mut scene, mut player) = scene_with_a_player();
    let wrong = [
        (
            "/tp nowhere:planet".to_owned(),
            "there is no place nowhere:planet; /tp alone lists the places".to_owned(),
        ),
        (
            format!("/tp {PLANET} 10"),
            "usage: /tp, /tp X Y Z, /tp PLACE [LATITUDE LONGITUDE | X Y Z]".to_owned(),
        ),
        (
            format!("/tp {PLANET} 1 2 3 4"),
            "usage: /tp, /tp X Y Z, /tp PLACE [LATITUDE LONGITUDE | X Y Z]".to_owned(),
        ),
        (
            format!("/tp {PLANET} 91 0"),
            "a latitude is from -90 to 90".to_owned(),
        ),
        (
            format!("/tp {PLANET} -90.5 0"),
            "a latitude is from -90 to 90".to_owned(),
        ),
        (
            format!("/tp {PLANET} north 0"),
            "LATITUDE must be a number".to_owned(),
        ),
        (
            format!("/tp {PLANET} 0 nan"),
            "LONGITUDE must be a number".to_owned(),
        ),
        (
            format!("/tp {PLANET} 1 2 three"),
            "Z must be a number".to_owned(),
        ),
        (
            "/tp base_blocks:great_river_widest 1 2".to_owned(),
            "base_blocks:great_river_widest has no coordinates; /tp it alone".to_owned(),
        ),
    ];
    for (command, wanted) in wrong {
        assert_eq!(say(&mut scene, &mut player, &command), wanted, "{command}");
    }
    // Nothing moved the player.
    let eyes = world_eyes(&mut scene);
    assert!((length(eyes) - RADIUS).abs() < HILLS, "{eyes:?}");
}

#[test]
fn a_planet_is_not_a_place_in_a_world_that_has_none() {
    // The overworld alone (the world the tests of the overworld start in) has no planet and no river.
    let mut rig = common::Rig::new();
    let mut player = rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    player.take_command_replies();
    player.send_command(&format!("/tp {PLANET}"));
    rig.step();
    rig.step();
    rig.settle(&mut [&mut player]);
    assert_eq!(
        player.take_command_replies(),
        [format!(
            "there is no place {PLANET}; /tp alone lists the places"
        )]
    );
}
