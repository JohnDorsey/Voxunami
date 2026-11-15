//! Parallax occlusion (`Relief::ParallaxOcclusion`, `docs/history/parallax-spike.md`, "Parallax occlusion is the lumpy one"): the heights of a picture's normal picture are a smooth
//! surface (the four pixels round a place mixed as a bilinear filter would), the eye's ray is marched down through it in equal steps, and the picture is read where the ray meets
//! it. The face is not flat but it is not columns either: there are no walls, and the normal picture's tilts still light what is seen.
//!
//! The scene is that of the prisms' tests: a floor of 32 by 32 blocks of one picture, a sun with no shadow maps, and a camera over it.
use renderer::{Relief, SettingChoices, SettingValue};

use super::prisms::{
    DIRT, DIRT_NORMAL, HEIGHT, PICTURE_SIDE, WIDTH, all_at_the_top, decode, differing, floor,
    plateau, settled, view_of, with_relief,
};
use super::*;

fn choose(choices: &[(&str, SettingValue)]) -> SettingChoices {
    choices
        .iter()
        .map(|(name, value)| ((*name).to_owned(), value.clone()))
        .collect()
}

fn the_relief(word: &str) -> (&'static str, SettingValue) {
    ("relief", SettingValue::Choice(word.to_owned()))
}

#[test]
fn parallax_occlusion_is_a_relief_of_its_own_that_a_pack_asks_for_and_a_player_chooses() {
    let (dirt, normal) = (decode(DIRT), decode(DIRT_NORMAL));
    let pixels = (WIDTH * HEIGHT) as usize;
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = floor(&context, &dirt, &normal, [0.5, 0.5, 0.7], 100.0);
        let camera = view_of([16.0, 17.0, 16.0], 40.0, 0.0, 1.2, 0.38);
        let prisms = settled(&mut renderer, &camera);
        with_relief(&mut renderer, Some(Relief::Flat));
        let flat = settled(&mut renderer, &camera);
        with_relief(&mut renderer, Some(Relief::ParallaxOcclusion));
        let by_the_pack = settled(&mut renderer, &camera);
        for (other, picture) in [("flat", &flat), ("prisms", &prisms)] {
            let changed = differing(&by_the_pack, picture, 6);
            assert!(
                changed > pixels / 50,
                "{name}: parallax occlusion on dirt changes only {changed} of {pixels} pixels against {other}"
            );
        }
        // The player's word for the game's own shader is the same picture as a pack's, to the byte, and the other words bring the others back.
        with_relief(&mut renderer, None);
        renderer
            .set_shader_pack_with_settings(None, &choose(&[the_relief("parallax_occlusion")]))
            .expect("parallax_occlusion is a relief");
        assert_eq!(
            settled(&mut renderer, &camera),
            by_the_pack,
            "{name}: a player's word and a pack's are one picture"
        );
        renderer
            .set_shader_pack_with_settings(None, &choose(&[the_relief("prisms")]))
            .expect("prisms is a relief");
        assert_eq!(settled(&mut renderer, &camera), prisms, "{name}");
        renderer
            .set_shader_pack_with_settings(None, &choose(&[the_relief("flat")]))
            .expect("flat is a relief");
        assert_eq!(settled(&mut renderer, &camera), flat, "{name}");
    }
}

/// A picture with every height at the top has no relief to look past, and a face with nothing but that is drawn as flat is, byte for byte.
#[test]
fn where_every_height_is_at_the_top_parallax_occlusion_changes_nothing() {
    let dirt = decode(DIRT);
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = floor(&context, &dirt, &all_at_the_top(), [0.5, 0.5, 0.7], 100.0);
        let camera = view_of([16.0, 17.0, 16.0], 25.0, 30.0, 1.2, 0.38);
        with_relief(&mut renderer, Some(Relief::Flat));
        let flat = settled(&mut renderer, &camera);
        with_relief(&mut renderer, Some(Relief::ParallaxOcclusion));
        assert_eq!(settled(&mut renderer, &camera), flat, "{name}");
    }
}

/// The relief of no depth is the face: nothing is shifted, and the tilts of the normal picture bump it as they do with the flat relief (not as they are left out under prisms).
#[test]
fn a_relief_of_no_depth_is_the_flat_one_and_the_tilts_of_the_normal_picture_still_light_it() {
    let (dirt, normal) = (decode(DIRT), decode(DIRT_NORMAL));
    let pixels = (WIDTH * HEIGHT) as usize;
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = floor(&context, &dirt, &normal, [0.5, 0.5, 0.7], 100.0);
        let camera = view_of([16.0, 17.0, 16.0], 40.0, 0.0, 1.2, 0.38);
        let no_depth = ("relief_depth", SettingValue::Float(0.0));
        let mut pictures = Vec::new();
        for word in ["flat", "parallax_occlusion", "prisms"] {
            renderer
                .set_shader_pack_with_settings(None, &choose(&[the_relief(word), no_depth.clone()]))
                .expect("a relief of no depth");
            pictures.push(settled(&mut renderer, &camera));
        }
        assert_eq!(
            pictures[1], pictures[0],
            "{name}: with no depth parallax occlusion is the flat relief"
        );
        // (Prisms of no depth are flat on top: the tilts are what is left out, so the two are two pictures. This is what says the occlusion's tilts are in.)
        assert!(
            differing(&pictures[2], &pictures[1], 6) > pixels / 50,
            "{name}: prisms of no depth and the flat relief are alike"
        );
    }
}

/// Where the picture is not drawn (beyond the picture distance) or a picture pixel is smaller than a pixel of the screen, there is no relief to see, and none is walked.
#[test]
fn parallax_occlusion_is_left_out_beyond_the_picture_distance_and_where_a_picture_pixel_is_smaller_than_the_screens()
 {
    let (dirt, normal) = (decode(DIRT), decode(DIRT_NORMAL));
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = floor(&context, &dirt, &normal, [0.5, 0.5, 0.7], 100.0);
        let near = view_of([16.0, 17.0, 16.0], 40.0, 0.0, 1.2, 0.38);
        let far = view_of([16.0, 17.0, 16.0], 40.0, 0.0, 60.0, 0.5);
        with_relief(&mut renderer, Some(Relief::ParallaxOcclusion));
        let occlusion: Vec<_> = [&near, &far]
            .map(|camera| settled(&mut renderer, camera))
            .into();
        with_relief(&mut renderer, Some(Relief::Flat));
        let flat: Vec<_> = [&near, &far]
            .map(|camera| settled(&mut renderer, camera))
            .into();
        assert!(
            differing(&occlusion[0], &flat[0], 6) > 0,
            "{name}: the test's near view shows no parallax"
        );
        assert_eq!(
            occlusion[1], flat[1],
            "{name}: from far away parallax occlusion is not drawn"
        );
        renderer.set_picture_distance(0.3);
        with_relief(&mut renderer, Some(Relief::ParallaxOcclusion));
        let short = settled(&mut renderer, &near);
        with_relief(&mut renderer, Some(Relief::Flat));
        assert_eq!(
            settled(&mut renderer, &near),
            short,
            "{name}: beyond the picture distance there is no relief"
        );
    }
}

/// A picture whose one square plateau is white at the top of a dark floor 0.1 of a voxel lower. What is under the plateau's side is hidden from the eye by the plateau, so the plateau
/// reaches toward the eye: the white is farther down the picture with parallax than without, whichever side the eye is on.
#[test]
fn a_plateau_reaches_toward_the_eye_from_every_side() {
    let marked: Vec<u8> = (0..PICTURE_SIDE * PICTURE_SIDE)
        .flat_map(|index| {
            let (column, row) = (index % PICTURE_SIDE, index / PICTURE_SIDE);
            if (5..11).contains(&column) && (5..11).contains(&row) {
                [255, 255, 255, 255]
            } else {
                [30, 30, 30, 255]
            }
        })
        .collect();
    let plateau = plateau();
    // The white's number of pixels and the middle of their rows.
    let white = |image: &[u8]| {
        let (mut count, mut rows) = (0u32, 0.0f64);
        for (index, pixel) in image.chunks(4).enumerate() {
            if (u32::from(pixel[0]) + u32::from(pixel[1]) + u32::from(pixel[2])) / 3 > 100 {
                count += 1;
                rows += f64::from(index as u32 / WIDTH);
            }
        }
        (count, rows / f64::from(count.max(1)))
    };
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = floor(&context, &marked, &plateau, [0.5, 0.5, 0.7], 100.0);
        for azimuth in [0.0, 180.0, 90.0] {
            let camera = view_of([16.5, 16.9, 16.5], 35.0, azimuth, 1.0, 0.45);
            with_relief(&mut renderer, Some(Relief::Flat));
            let flat = white(&settled(&mut renderer, &camera));
            with_relief(&mut renderer, Some(Relief::ParallaxOcclusion));
            let parallax = white(&settled(&mut renderer, &camera));
            eprintln!("{name}: azimuth {azimuth}: white {flat:?} flat, {parallax:?} with parallax");
            assert!(
                parallax.0 > flat.0,
                "{name}: azimuth {azimuth}: the plateau does not reach out: {} white pixels against {}",
                parallax.0,
                flat.0
            );
            assert!(
                parallax.1 > flat.1 + 2.0,
                "{name}: azimuth {azimuth}: the white is not nearer the eye (the bottom of the picture): its middle is at row {} against {}",
                parallax.1,
                flat.1
            );
        }
    }
}
