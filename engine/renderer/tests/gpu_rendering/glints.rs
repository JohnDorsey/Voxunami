//! Glints from a picture's specular companion (`PictureSize::specular`, LabPBR `_s`, `docs/specs/block-textures-spec.md` section 4.4): the red byte is how smooth the pixel is
//! and the green byte how much light it throws back straight on (its reflectance, and from 230 on a metal, which throws back its own colour). The sun's glint is added to the
//! light of the face where the half-way vector between the sun and the eye meets the shading normal, a narrow bright patch on a smooth pixel and a wide dim one on a rough one.
//!
//! The scene is one block, a face of it seen straight on with a white picture of 8 pixels across that has the same specular pixel everywhere, and the sun right behind the
//! camera (along the face's normal), so that the half-way vector is the normal itself. The glint is what that face is lit by over what the same face is with no specular
//! picture, in linear light: the three channels of a white face add up, and the sun is made weak enough (100 of 255) for no channel to reach full.

use super::bumps::{PICTURE_PIXELS, PLUS_Z, Scene, sun_of};
use super::*;

const SUN_LIGHT: u8 = 100;
const SUN_SHARE: f64 = SUN_LIGHT as f64 / 255.0;

/// A specular picture with the same smoothness, reflectance and porosity bytes (and no emission: 255) everywhere.
fn specular(smoothness: u8, reflectance: u8) -> Vec<u8> {
    (0..PICTURE_PIXELS * PICTURE_PIXELS)
        .flat_map(|_| [smoothness, reflectance, 0, 255])
        .collect()
}

/// What the glint of a pixel of smoothness byte `smoothness` and reflectance `reflectance` (a number from 0 to 1) adds to a white face lit by a sun of `SUN_SHARE` straight
/// along the normal and seen from along it, as the rule of the format: the roughness is one minus the smoothness, the width of the glint (`alpha`) its square (no
/// narrower than 0.03), the strength of the patch at its middle `1 / (pi alpha^2)`, and the light thrown back is that, times the reflectance, times a quarter, times the sun.
/// Three channels.
fn expected_glint(smoothness: u8, reflectance: f64) -> f64 {
    let roughness = 1.0 - f64::from(smoothness) / 255.0;
    let alpha = (roughness * roughness).max(0.03);
    let strength = 1.0 / (std::f64::consts::PI * alpha * alpha);
    3.0 * SUN_SHARE * strength * reflectance * 0.25
}

fn scene<'a>(specular: Option<&'a [u8]>, sun: [f64; 3]) -> Scene<'a> {
    Scene {
        specular,
        sun_light: SUN_LIGHT,
        ..Scene::new(&PLUS_Z, None, sun)
    }
}

/// What the glint adds in `scene`, over the same scene with no specular picture.
fn glint(context: &GraphicsContext, name: &str, what: &str, with: Scene) -> f64 {
    let without = Scene {
        specular: None,
        ..with
    };
    let base = without.brightness(context, name, "glint_none");
    let shown = with.brightness(context, name, what);
    assert!(
        base > 0.2 && base < 2.5,
        "{name}: the face with no glint is lit {base:.3}, which is too little or too much to say anything by"
    );
    shown - base
}

const ALONG_THE_NORMAL: [f64; 3] = [0.0, 0.0, 1.0];

#[test]
fn a_smooth_pixel_throws_the_sun_back_by_its_smoothness_and_its_reflectance_where_the_half_way_vector_is_the_normal()
 {
    for context in contexts() {
        let name = adapter_name(&context);
        for (smoothness, reflectance_byte) in [(64, 100), (96, 100), (128, 100), (128, 200)] {
            let picture = specular(smoothness, reflectance_byte);
            let found = glint(
                &context,
                &name,
                &format!("glint_{smoothness}_{reflectance_byte}"),
                scene(Some(&picture), ALONG_THE_NORMAL),
            );
            let wanted = expected_glint(smoothness, f64::from(reflectance_byte) / 255.0);
            assert!(
                (found / wanted - 1.0).abs() < 0.07,
                "{name}: smoothness {smoothness}, reflectance {reflectance_byte}: the glint adds {found:.3}, wanted {wanted:.3}"
            );
        }
    }
}

#[test]
fn a_pixel_that_throws_back_nothing_or_whose_glint_is_elsewhere_adds_nothing() {
    for context in contexts() {
        let name = adapter_name(&context);
        let none = specular(200, 0);
        let found = glint(
            &context,
            &name,
            "glint_no_reflectance",
            scene(Some(&none), ALONG_THE_NORMAL),
        );
        assert!(
            found.abs() < 0.01,
            "{name}: a pixel of reflectance 0 adds {found:.3}"
        );
        // The very smooth pixel with the sun grazing it: the half-way vector is far from the normal (a patch of 0.03 wide is nowhere near).
        let smooth = specular(255, 200);
        let grazing = sun_of(&PLUS_Z, PLUS_Z.up);
        let found = glint(
            &context,
            &name,
            "glint_elsewhere",
            scene(Some(&smooth), grazing),
        );
        assert!(
            found.abs() < 0.01,
            "{name}: a smooth pixel with the glint elsewhere adds {found:.3}"
        );
    }
}

#[test]
fn from_230_a_pixel_is_a_metal_that_throws_back_its_own_colour() {
    for context in contexts() {
        let name = adapter_name(&context);
        let (mut dielectric, mut metal) = (specular(128, 229), specular(128, 230));
        dielectric.shrink_to_fit();
        metal.shrink_to_fit();
        let by_dielectric = glint(
            &context,
            &name,
            "glint_229",
            scene(Some(&dielectric), ALONG_THE_NORMAL),
        );
        let by_metal = glint(
            &context,
            &name,
            "glint_230",
            scene(Some(&metal), ALONG_THE_NORMAL),
        );
        // (The picture is white, so a metal throws back all of the light, 1 in place of 229/255; the dielectric's is its byte over 255.)
        let wanted_dielectric = expected_glint(128, 229.0 / 255.0);
        let wanted_metal = expected_glint(128, 1.0);
        assert!(
            (by_dielectric / wanted_dielectric - 1.0).abs() < 0.03
                && (by_metal / wanted_metal - 1.0).abs() < 0.03,
            "{name}: 229 adds {by_dielectric:.3} (wanted {wanted_dielectric:.3}), 230 adds {by_metal:.3} (wanted {wanted_metal:.3})"
        );
    }
}

#[test]
fn the_glint_is_the_suns_alone_and_goes_with_the_picture_and_the_walk_of_the_suns_ray_does_not_change_it()
 {
    for context in contexts() {
        let name = adapter_name(&context);
        let picture = specular(128, 100);
        let wanted = expected_glint(128, 100.0 / 255.0);
        // With no sun there is nothing to throw back.
        let dark = Scene {
            sun_light: 0,
            sky_light: 255,
            ambient_light: 255,
            ..scene(Some(&picture), ALONG_THE_NORMAL)
        };
        let found = {
            let without = Scene {
                specular: None,
                ..dark
            };
            dark.brightness(&context, &name, "glint_no_sun")
                - without.brightness(&context, &name, "glint_no_sun_none")
        };
        assert!(
            found.abs() < 0.01,
            "{name}: with no sun the pixel throws back {found:.3}"
        );
        // Beyond the picture distance the face is as flat as it is in colour: no glint.
        let far = Scene {
            picture_distance: 6.0,
            ..scene(Some(&picture), ALONG_THE_NORMAL)
        };
        let found = glint(&context, &name, "glint_beyond", far);
        assert!(
            found.abs() < 0.01,
            "{name}: beyond the picture distance the glint adds {found:.3}"
        );
        // The same with the walk of the sun's ray.
        let traced = Scene {
            traced: true,
            ..scene(Some(&picture), ALONG_THE_NORMAL)
        };
        let found = glint(&context, &name, "glint_traced", traced);
        assert!(
            (found / wanted - 1.0).abs() < 0.03,
            "{name}: with the walk the glint adds {found:.3}, wanted {wanted:.3}"
        );
    }
}
