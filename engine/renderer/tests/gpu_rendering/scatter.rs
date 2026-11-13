//! Subsurface scattering from a picture's specular companion (`PictureSize::specular`, LabPBR `_s`, `docs/specs/block-textures-spec.md` section 4.4): the blue byte of the
//! companion, from 65 up, is how much of the sun's light a thin block lets through to its far side (`(byte - 64) / 191`; up to 64 is porosity, which is not read). A face turned
//! from the sun is lit by the sun's light that reaches the far side of its block, as much as the pixel scatters, by the cosine of the sun's angle to the face, in the face's colour.
//!
//! The scene is one block, a face of it seen straight on with a white picture of 8 pixels across that has the same specular pixel everywhere, and the sun behind the block, so that
//! the face has no light of its own (no ambient light, no sky): what shows is the scattered light. Its brightness is the three channels of a white face added up, and the sun is made
//! weak enough (100 of 255) for no channel to reach full.

use super::bumps::{PLUS_Z, Scene, sun_of};
use super::*;

const SUN_LIGHT: u8 = 100;
const SUN_SHARE: f64 = SUN_LIGHT as f64 / 255.0;
/// The sun's direction straight behind the block, and the one that is 0.6 of the way behind it and 0.8 along the picture's top.
const STRAIGHT_BEHIND: [f64; 3] = [0.0, 0.0, -1.0];
const SLANTED_BEHIND: [f64; 3] = [0.0, 0.8, -0.6];

/// A specular picture with the blue byte `blue` everywhere (and no emission: alpha 255, no glint).
fn scattering(blue: u8) -> Vec<u8> {
    (0..super::bumps::PICTURE_PIXELS * super::bumps::PICTURE_PIXELS)
        .flat_map(|_| [0, 0, blue, 255])
        .collect()
}

/// The share of the sun's light a pixel of blue byte `blue` scatters, by the format: nothing up to 64 (porosity), then 0 to 1 over 65 to 255.
fn share_of(blue: u8) -> f64 {
    if blue <= 64 {
        0.0
    } else {
        f64::from(blue - 64) / 191.0
    }
}

/// What a white face lit only by the sun's scatter is: three channels of the sun's share, times the pixel's share, times the cosine of the sun's angle to the back of the face.
fn expected(blue: u8, cosine: f64) -> f64 {
    3.0 * SUN_SHARE * share_of(blue) * cosine
}

fn scene<'a>(specular: Option<&'a [u8]>, sun: [f64; 3]) -> Scene<'a> {
    Scene {
        specular,
        sun_light: SUN_LIGHT,
        ..Scene::new(&PLUS_Z, None, sun)
    }
}

#[test]
fn a_face_turned_from_the_sun_is_lit_by_what_the_pixel_scatters_and_the_cosine_of_the_sun() {
    for context in contexts() {
        let name = adapter_name(&context);
        for traced in [false, true] {
            for (sun, cosine) in [(STRAIGHT_BEHIND, 1.0), (SLANTED_BEHIND, 0.6)] {
                for blue in [0, 64, 128, 192, 255] {
                    let picture = scattering(blue);
                    let found = Scene {
                        traced,
                        ..scene(Some(&picture), sun)
                    }
                    .brightness(
                        &context,
                        &name,
                        &format!("scatter_{blue}_{traced}"),
                    );
                    let wanted = expected(blue, cosine);
                    assert!(
                        (found - wanted).abs() < 0.03 * wanted.max(0.2),
                        "{name}: blue {blue}, cosine {cosine}, walk {traced}: the face is lit {found:.3}, wanted {wanted:.3}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_face_that_looks_at_the_sun_is_lit_as_it_was_and_so_is_one_with_no_picture_for_it() {
    for context in contexts() {
        let name = adapter_name(&context);
        let facing = sun_of(&PLUS_Z, PLUS_Z.up);
        let full = scattering(255);
        let with = scene(Some(&full), facing).brightness(&context, &name, "scatter_facing");
        let without = scene(None, facing).brightness(&context, &name, "scatter_facing_none");
        assert!(
            without > 0.2,
            "{name}: the face in the sun is lit {without:.3}, which says nothing"
        );
        assert!(
            (with / without - 1.0).abs() < 0.005,
            "{name}: a face in the sun is lit {with:.4} with a scattering picture and {without:.4} with none"
        );
        // A face turned from the sun with no companion has none of its light.
        let dark = scene(None, STRAIGHT_BEHIND).brightness(&context, &name, "scatter_none");
        assert!(
            dark < 0.01,
            "{name}: a face with no companion, turned from the sun, is lit {dark:.3}"
        );
    }
}

#[test]
fn no_scatter_shows_beyond_the_picture_distance() {
    for context in contexts() {
        let name = adapter_name(&context);
        let full = scattering(255);
        let found = Scene {
            picture_distance: 6.0,
            ..scene(Some(&full), STRAIGHT_BEHIND)
        }
        .brightness(&context, &name, "scatter_beyond");
        assert!(
            found < 0.01,
            "{name}: beyond the picture distance the face is lit {found:.3} by scatter"
        );
    }
}

#[test]
fn what_blocks_the_sun_behind_the_block_takes_the_scattered_light_and_what_does_not_leaves_it() {
    for context in contexts() {
        let name = adapter_name(&context);
        let full = scattering(255);
        // The sun is a little over the horizon, behind the block (0.28 up, 0.96 behind): a sun on the horizon would not have its shadow drawn, and one this low leaves the block's
        // far face, a voxel behind the face looked at, by that face (a steeper one would leave it by the top).
        let high_behind = [0.0, 0.28, -0.96];
        let wanted = expected(255, 0.96);
        let lit = |what: &str, blockers: &[[u8; 3]]| {
            Scene {
                traced: true,
                blockers,
                ..scene(Some(&full), high_behind)
            }
            .brightness(&context, &name, what)
        };
        let open = lit("scatter_open", &[]);
        assert!(
            (open / wanted - 1.0).abs() < 0.03,
            "{name}: with nothing in the sun's way the face is lit {open:.3}, wanted {wanted:.3}"
        );
        // (A block against the far side, a little up from it where the ray goes (it casts from its underside: a block of the same kind flush behind would have no face there to
        // cast from), and one far along the ray, are in the sun's way; one to the side is not.)
        for (what, blockers) in [
            ("scatter_flush_blocker", [[16, 17, 15]]),
            ("scatter_far_blocker", [[16, 18, 10]]),
        ] {
            let found = lit(what, &blockers);
            assert!(
                found < 0.03 * wanted,
                "{name}: with a block in the way ({what}) the face is lit {found:.3}, wanted nothing of {wanted:.3}"
            );
        }
        let found = lit("scatter_beside_blocker", &[[20, 17, 15]]);
        assert!(
            (found / wanted - 1.0).abs() < 0.03,
            "{name}: with a block to the side the face is lit {found:.3}, wanted {wanted:.3}"
        );
    }
}
