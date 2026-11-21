//! Ambient occlusion from a picture's normal companion (`PictureSize::normal`, LabPBR `_n` blue, `docs/specs/block-textures-spec.md` section 4.4): the blue byte is the share of
//! the ambient light (the light of the lamps and the ambient light, and the sky's) that reaches a pixel, 255 all of it and 0 none. The sun's light is not ambient and is not
//! changed. The scene is the one of `bumps.rs`: a face of a block with a white picture, here the same blue everywhere and no tilt, and the brightness of the face with the
//! occluded picture over its brightness with an open one is the share (the pictures are data, so the share is of the light and not of the encoded colour).

use super::bumps::{PLUS_Z, Scene, occluded, sun_of};
use super::*;

/// The blue bytes tried, and the share of the light each lets through (the byte over 255).
const OCCLUSIONS: [u8; 3] = [64, 128, 192];

fn ratio_of(
    context: &GraphicsContext,
    name: &str,
    occlusion: u8,
    make: impl Fn(Scene) -> Scene,
) -> f64 {
    let face = &PLUS_Z;
    let sun = sun_of(face, face.up);
    let (open_picture, shut_picture) = (occluded(255), occluded(occlusion));
    let open = make(Scene::new(face, Some(&open_picture), sun)).brightness(context, name, "open");
    let shut = make(Scene::new(face, Some(&shut_picture), sun)).brightness(
        context,
        name,
        &format!("occluded_{occlusion}"),
    );
    assert!(
        open > 0.05,
        "{name}: the open face is lit {open:.3}, which is too little to say anything by"
    );
    shut / open
}

#[test]
fn the_ambient_light_and_the_skys_are_cut_to_the_share_the_blue_byte_says_and_the_suns_is_not() {
    for context in contexts() {
        let name = adapter_name(&context);
        for occlusion in OCCLUSIONS {
            let wanted = f64::from(occlusion) / 255.0;
            let by_ambient = ratio_of(&context, &name, occlusion, |scene| Scene {
                sun_light: 0,
                ambient_light: 150,
                ..scene
            });
            assert!(
                (by_ambient - wanted).abs() < 0.03,
                "{name}: blue {occlusion}: the ambient light is {by_ambient:.3} of the open face's, wanted {wanted:.3}"
            );
            let by_sky = ratio_of(&context, &name, occlusion, |scene| Scene {
                sun_light: 0,
                sky_light: 200,
                ..scene
            });
            assert!(
                (by_sky - wanted).abs() < 0.03,
                "{name}: blue {occlusion}: the sky's light is {by_sky:.3} of the open face's, wanted {wanted:.3}"
            );
            let by_sun = ratio_of(&context, &name, occlusion, |scene| scene);
            assert!(
                (by_sun - 1.0).abs() < 0.005,
                "{name}: blue {occlusion}: the sun's light is {by_sun:.4} of the open face's, wanted 1"
            );
        }
    }
}

#[test]
fn the_same_holds_in_the_shader_that_walks_the_suns_ray() {
    for context in contexts() {
        let name = adapter_name(&context);
        let occlusion = 64;
        let wanted = f64::from(occlusion) / 255.0;
        let by_ambient = ratio_of(&context, &name, occlusion, |scene| Scene {
            // (The walk is drawn only where there is a sun to walk toward, so the sun is behind the face: it gives the face none of its light.)
            sun: PLUS_Z.normal.map(|part| -part),
            ambient_light: 150,
            traced: true,
            ..scene
        });
        assert!(
            (by_ambient - wanted).abs() < 0.03,
            "{name}: with the walk, the ambient light is {by_ambient:.3} of the open face's, wanted {wanted:.3}"
        );
    }
}

#[test]
fn no_occlusion_shows_beyond_the_picture_distance_or_on_a_picture_with_none() {
    for context in contexts() {
        let name = adapter_name(&context);
        let beyond = ratio_of(&context, &name, 64, |scene| Scene {
            sun_light: 0,
            ambient_light: 150,
            picture_distance: 6.0,
            ..scene
        });
        assert!(
            (beyond - 1.0).abs() < 0.005,
            "{name}: beyond the picture distance the occlusion still shows: {beyond:.4}"
        );
        // A face whose picture has no normal companion is as lit as one with an open one.
        let face = &PLUS_Z;
        let sun = sun_of(face, face.up);
        let open_picture = occluded(255);
        let none = Scene {
            sun_light: 0,
            ambient_light: 150,
            ..Scene::new(face, None, sun)
        }
        .brightness(&context, &name, "none");
        let open = Scene {
            sun_light: 0,
            ambient_light: 150,
            ..Scene::new(face, Some(&open_picture), sun)
        }
        .brightness(&context, &name, "open_again");
        assert!(
            (none / open - 1.0).abs() < 0.005,
            "{name}: a picture with no normal companion is lit {none:.4}, an open one {open:.4}"
        );
    }
}
