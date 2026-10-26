//! Planets as data (Phase 40c): the planet of a new world is a body from Cave's own data and makes what `base_blocks:small_planet_v1` makes, a pack can put another
//! planet in the world with a file, and `check-pack` and `pack-schema` know the kind of file.

mod common;

use base_blocks::{planet_family_identifier, small_planet_generator_identifier};
use common::{SEED, Scene, player_pose, ship_id, tick_and_follow};
use game_shell::{GameContent, GameDefinition, RawFile, RawPack};
use namespaced_id::NamespacedId;
use terrain::PlanetSettings;
use voxel_space::{BrickBox, BrickCoordinate, GridPose, LevelIndex};
use voxunami_cave::aboard::in_voxels;
use voxunami_cave::content::CaveContent;
use voxunami_cave::game::Cave;
use world_storage::GeneratorRecipe;

fn id(text: &str) -> NamespacedId {
    text.parse().unwrap()
}

fn file(name: &str, text: &str) -> RawFile {
    RawFile {
        name: name.to_owned(),
        text: text.to_owned(),
    }
}

#[test]
fn the_planet_of_a_new_world_is_a_body_of_the_games_own_data_and_makes_the_small_planet_bit_for_bit()
 {
    let content = CaveContent::new();
    assert_eq!(content.body_names(), [id("voxunami_cave:planet")]);
    let world = content.new_world(SEED);

    // A recipe of the planet family with the settings of the small planet, made for the seed of the world.
    assert_eq!(
        world.generator_recipes(),
        &[GeneratorRecipe {
            instance: id("voxunami_cave:planet"),
            family: planet_family_identifier(),
            parameters: PlanetSettings::one_percent_of_earth(SEED).to_bytes(),
        }]
    );
    // And so the bricks of the small planet of the code (which worlds made before have): a few of the top, the side and the inside, and the coarse cells.
    let layout = std::sync::Arc::clone(world.main().overrides.layout());
    let generators = world.main().terrain.generators();
    let by_name = |name: &str| {
        let number = generators.runtime_id(&name.parse().unwrap()).unwrap();
        generators.entry(number).unwrap().content().unwrap()
    };
    let (data, code) = (
        by_name("voxunami_cave:planet"),
        by_name(small_planet_generator_identifier().as_str()),
    );
    for coordinate in [
        BrickCoordinate::new(0, 1990, 0),
        BrickCoordinate::new(1989, 3, -2),
        BrickCoordinate::new(-1147, 1147, 1147),
        BrickCoordinate::new(40, 1000, -40),
    ] {
        assert_eq!(
            data.generate_brick(&layout, coordinate),
            code.generate_brick(&layout, coordinate),
            "at {coordinate:?}"
        );
    }

    // The ship: the first, in space, at the origin, held, as heavy as it must be to pull ten metres a second a second at the surface of 63,710 voxels, with
    // the ground reaching 2,100 bricks every way (the numbers the code had).
    assert_eq!(world.grids().len(), 1);
    let planet = world
        .grids()
        .get(ship_id(0))
        .expect("the planet is the first ship");
    assert_eq!(planet.level, LevelIndex::MAIN);
    assert_eq!(planet.pose, GridPose::identity());
    assert!(planet.held && !planet.frozen);
    let pull = f64::from(Cave::gravity().pull_strength);
    let mass = (10.0 * 63_710.0 * 63_710.0 / pull) as f32;
    assert_eq!(planet.mass.to_bits(), mass.to_bits());
    let terrain = planet.terrain.expect("the planet has terrain");
    assert_eq!(terrain.mass.to_bits(), mass.to_bits());
    assert_eq!(
        terrain.extent,
        BrickBox::around(BrickCoordinate::ORIGIN, 2_100)
    );
    assert_eq!(
        generators
            .entry(terrain.generator)
            .unwrap()
            .identifier()
            .as_str(),
        "voxunami_cave:planet"
    );

    // Another seed is another planet, as it was.
    let other = content.new_world(SEED + 1);
    assert_eq!(
        other.generator_recipes()[0].parameters,
        PlanetSettings::one_percent_of_earth(SEED + 1).to_bytes()
    );
}

/// A pack with a moon of 10,000 voxels, 400 kilometres along x from the planet, pulling 8 metres a second a second at its surface (a body that pulls at all
/// weighs at least 10^19 kilograms).
fn moons() -> Vec<RawPack> {
    vec![RawPack {
        folder: "moons".to_owned(),
        manifest: r#"{ "id": "moons" }"#.to_owned(),
        blocks: Vec::new(),
        patches: Vec::new(),
        bodies: vec![file(
            "moon",
            r#"{
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
            }"#,
        )],
    }]
}

fn content_with_the_moons() -> CaveContent {
    let mut content = CaveContent::new();
    content.apply_packs(&moons()).unwrap();
    content
}

#[test]
fn a_pack_puts_a_second_planet_in_a_new_world_after_the_first() {
    let content = content_with_the_moons();
    assert_eq!(
        content.body_names(),
        [id("voxunami_cave:planet"), id("moons:moon")]
    );
    let world = content.new_world(SEED);
    assert_eq!(world.grids().len(), 2);
    let moon = world.grids().get(ship_id(1)).unwrap();
    assert_eq!(moon.level, LevelIndex::MAIN);
    assert!(moon.held);
    let pull = f64::from(Cave::gravity().pull_strength);
    assert_eq!(moon.mass, (8.0 * 10_000.0 * 10_000.0 / pull) as f32);
    assert_eq!(
        moon.terrain.unwrap().extent,
        BrickBox::around(BrickCoordinate::ORIGIN, 340)
    );
    // 400,000 voxels along x is brick 12,500.
    assert_eq!(
        moon.pose.position.brick(),
        BrickCoordinate::new(12_500, 0, 0)
    );
    let names: Vec<String> = world
        .generator_recipes()
        .iter()
        .map(|recipe| recipe.instance.to_string())
        .collect();
    assert_eq!(names, ["voxunami_cave:planet", "moons:moon"]);
}

#[test]
fn a_walker_dropped_over_the_moon_of_a_pack_lands_on_it_and_stands_on_its_ground() {
    let content = content_with_the_moons();
    let world = content.new_world(SEED);
    let mut scene = Scene::in_this_world(content, world);
    // Over the top of the moon (10,000 voxels and hills of 20 or so) at its pole.
    let mut player = scene.rig.join_walking([400_000.5, 10_100.0, 0.5], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 300);

    let pose = player_pose(&scene, 1);
    assert_eq!(pose.grid, ship_id(1), "aboard the moon");
    let [x, y, z] = in_voxels(pose.position);
    let distance = (x * x + y * y + z * z).sqrt();
    assert!(
        (9_950.0..=10_060.0).contains(&distance),
        "the eyes are at the surface of a moon of 10,000 voxels: {distance}"
    );
    let standing = pose;
    tick_and_follow(&mut scene, &mut player, 40);
    assert_eq!(player_pose(&scene, 1), standing);
}

// ---- check-pack and pack-schema ----

/// A folder of the tests' own that goes away when dropped.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "voxunami_cave_bodies_{name}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, relative: &str, text: &str) {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn argument(&self) -> String {
        self.0.display().to_string()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn check_pack_accepts_a_pack_with_a_body_and_says_every_problem_of_one_that_is_wrong() {
    let good = Scratch::new("good");
    good.write("moons/pack.json", r#"{ "id": "moons" }"#);
    good.write("moons/bodies/moon.json", &moons()[0].bodies[0].text);
    voxunami_cave::run_check_packs([good.argument()]).unwrap();

    let bad = Scratch::new("bad");
    bad.write("moons/pack.json", r#"{ "id": "moons" }"#);
    bad.write(
        "moons/bodies/moon.json",
        &moons()[0].bodies[0]
            .text
            .replace("mini_cosmos", "nowhere")
            .replace("\"radius\": 10000", "\"radius\": -4")
            .replace(
                "\"surface_gravity\": 8",
                "\"surface_gravity\": 8, \"mass\": 5",
            ),
    );
    let error = voxunami_cave::run_check_packs([bad.argument()]).unwrap_err();
    assert!(
        error.contains(
            "moons/bodies/moon.json: /level: the game has no level \"voxunami_cave:nowhere\"; its levels are \"voxunami_cave:mini_cosmos\", \"voxunami_cave:overworld\""
        ),
        "{error}"
    );
    assert!(
        error.contains(
            "moons/bodies/moon.json: /terrain/parameters/radius: the radius of a planet is above nothing"
        ),
        "{error}"
    );
    assert!(
        error.contains("moons/bodies/moon.json: give how heavy it is as \"mass\""),
        "{error}"
    );
    assert!(error.contains("3 problems"), "{error}");
}

#[test]
fn pack_schema_writes_the_schema_of_bodies_with_cave_s_families_and_levels() {
    let directory = Scratch::new("schema");
    voxunami_cave::run_pack_schemas([directory.argument()]).unwrap();
    let text = std::fs::read_to_string(directory.0.join("body.schema.json")).unwrap();
    let schema: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        schema["$defs"]["family"]["enum"],
        serde_json::json!(["base_blocks:planet_v1"])
    );
    assert_eq!(
        schema["$defs"]["level"]["enum"],
        serde_json::json!(["voxunami_cave:mini_cosmos", "voxunami_cave:overworld"])
    );
    // The planet family's own parameters are in it.
    let allowed = &schema["properties"]["terrain"]["allOf"][0]["then"]["properties"]["parameters"];
    assert_eq!(allowed["required"], serde_json::json!(["seed", "radius"]));
    assert!(allowed["properties"]["hill_height"].is_object());
}
