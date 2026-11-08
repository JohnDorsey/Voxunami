//! A planet that goes round the home planet fast (125 metres a second): the `wanderer` of a data pack, for the tests of what happens near a planet on a rail.

use game_shell::{GameContent, RawFile, RawPack};
use voxunami_cave::content::CaveContent;

use super::{SEED, Scene};

/// The wanderer is the second ship of the world.
pub const WANDERER: u64 = 1;

/// What the pack of a test says of the planet it adds: where it is and how it moves (lines of the body file), how far its ground reaches in bricks and how hard it
/// pulls at the surface. It is a planet of 10,000 voxels.
pub fn wanderer_pack(motion: &str, extent_bricks: u32, surface_gravity: f64) -> Vec<RawPack> {
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
                    "family": "voxunami_cave:planet_v1",
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
pub fn orbit_of_the_home_planet(spin: Option<f64>) -> String {
    let spin = spin.map_or(String::new(), |period| {
        format!(r#""spin": {{ "axis": [0, 1, 0], "period": {period} }},"#)
    });
    format!(
        r#""rail": {{ {spin} "orbit": {{ "around": "voxunami_cave:planet", "radius": 400000, "period": 20000 }} }}"#
    )
}

pub fn scene_with(pack: &[RawPack]) -> Scene {
    let mut content = CaveContent::new();
    content.apply_packs(pack).unwrap();
    let world = content.new_world(SEED);
    Scene::in_this_world(content, world)
}

/// A new world with the usual wanderer in it.
pub fn a_scene(spin: Option<f64>) -> Scene {
    scene_with(&wanderer_pack(&orbit_of_the_home_planet(spin), 340, 8.0))
}

/// A new world with the wanderer on the same rail, so slow that it stands still (it goes round once in 10^15 seconds, thirty million years, at a billionth of a voxel
/// a second): the same planet in the same place and with the same sphere that carries what is near it, and what a test finds near the usual one is the speed alone.
/// (It is on a rail, and not a planet that has none: what a ship on a rail round it feels of the pull of the home planet is the same, which a ship round a planet that has no
/// rail does not take off, and a walker aboard it is turned by.)
pub fn a_barely_moving_scene() -> Scene {
    scene_with(&wanderer_pack(
        r#""rail": { "orbit": { "around": "voxunami_cave:planet", "radius": 400000, "period": 1e15 } }"#,
        340,
        8.0,
    ))
}
