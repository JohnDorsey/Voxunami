//! The lift of the whole scene (`lifted` in `voxel.wgsl`, `lifted_color` in `voxel_renderer.rs`): the light raised to the power of one over 1.2, so that the darks are brighter and
//! black and full brightness stay where they are. A sky is all there is to look at here (nothing is drawn but the sky), and the terrain goes through the same function.
//!
//! The sky is drawn by the shader where there are heavens (stars or a sun) and is the colour the picture is cleared to where there are none: the two are the same place twice,
//! and the test looks at both.

use renderer::Heavens;

use super::*;

/// The byte of an sRGB picture that a colour channel (itself sRGB, as the atmosphere's are) comes out as: decoded to light, lifted, encoded again. The rule written out for the
/// test, with `libm`.
pub(super) fn lifted_byte(channel: f64) -> u8 {
    let light = if channel <= 0.04045 {
        channel / 12.92
    } else {
        libm::pow((channel + 0.055) / 1.055, 2.4)
    };
    let lifted = libm::pow(light, 1.0 / 1.2);
    let encoded = if lifted <= 0.003_130_8 {
        lifted * 12.92
    } else {
        1.055 * libm::pow(lifted, 1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round() as u8
}

/// The middle pixel of the picture of a sky of this colour, with the heavens given.
fn sky_of(context: &GraphicsContext, colour: [f32; 3], heavens: Heavens) -> [u8; 4] {
    let mut renderer = new_renderer(context);
    renderer.set_atmosphere(Atmosphere {
        sky_color: colour,
        fog_start: 1000.0,
        fog_end: 2000.0,
        heavens,
        ..Atmosphere::default()
    });
    let camera = camera_at(BrickCoordinate::new(0, 0, 0), [16.0, 16.0, 40.0]);
    let picture = render(&mut renderer, &camera);
    pixel(&picture, SIZE / 2, SIZE / 2)
}

#[test]
fn the_scene_is_lifted_with_black_still_black_and_full_brightness_still_full() {
    // Stars so faint that they are nowhere to be seen, but not none: the sky is drawn by the shader.
    let by_the_shader = Heavens {
        star_light: 1.0e-9,
        ..Heavens::default()
    };
    assert!(!by_the_shader.is_empty());
    assert!(Heavens::default().is_empty());
    for context in contexts() {
        let name = adapter_name(&context);
        for (way, heavens) in [
            ("cleared to", Heavens::default()),
            ("drawn by the shader as", by_the_shader),
        ] {
            for colour in [
                [0.0, 0.0, 0.0],
                [1.0, 1.0, 1.0],
                [0.5, 0.5, 0.5],
                [0.1, 0.2, 0.9],
            ] {
                let expected = colour.map(|channel| lifted_byte(f64::from(channel)));
                let seen = sky_of(&context, colour, heavens);
                assert!(
                    close_to(seen, expected, 1),
                    "{name}: a sky {way} {colour:?} is {seen:?}, not {expected:?}"
                );
            }
        }
        // The two ends, by hand: black is exactly black and full brightness is exactly full.
        assert_eq!(lifted_byte(0.0), 0);
        assert_eq!(lifted_byte(1.0), 255);
        // And the lift is a lift: a middle grey is brighter than it was (128).
        assert!(
            lifted_byte(0.5) > 135,
            "a lifted 0.5 is {}",
            lifted_byte(0.5)
        );
    }
}
