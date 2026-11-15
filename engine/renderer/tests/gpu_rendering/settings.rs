//! The engine's settings (`renderer::engine_settings`, `VoxelRenderer::set_shader_pack_with_settings`): numbers of taste that reach the shaders as constants. Each is looked at in
//! what it does to a picture, on every adapter, and a choice that is no setting or is out of range is refused and changes nothing.

use renderer::{Heavens, SettingChoices, SettingValue};

use super::lift::{lifted_byte_by, sky_with_lift};
use super::prisms::{DIRT, DIRT_NORMAL, HEIGHT, WIDTH, decode, differing, floor, settled, view_of};
use super::*;

fn choose(name: &str, value: f32) -> SettingChoices {
    SettingChoices::from([(name.to_owned(), SettingValue::Float(value))])
}

/// The sum of every colour byte of a picture: how much light there is in it, as a number to compare.
fn total_light(image: &[u8]) -> u64 {
    image
        .chunks(4)
        .map(|pixel| u64::from(pixel[0]) + u64::from(pixel[1]) + u64::from(pixel[2]))
        .sum()
}

#[test]
fn the_lift_is_the_setting_scene_lift_and_a_lift_of_one_is_none_with_black_and_white_where_they_were()
 {
    let by_the_shader = Heavens {
        star_light: 1.0e-9,
        ..Heavens::default()
    };
    for context in contexts() {
        let name = adapter_name(&context);
        for lift in [1.0f32, 2.0, 0.5] {
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
                    let expected =
                        colour.map(|channel| lifted_byte_by(f64::from(channel), f64::from(lift)));
                    let seen = sky_with_lift(&context, colour, heavens, Some(lift));
                    assert!(
                        close_to(seen, expected, 1),
                        "{name}: with a lift of {lift} a sky {way} {colour:?} is {seen:?}, not {expected:?}"
                    );
                }
            }
        }
        // No lift is no change at all: a grey of 128 comes out as 128 (and not 143, which is what 1.2 makes of it).
        let grey = sky_with_lift(&context, [0.5, 0.5, 0.5], Heavens::default(), Some(1.0));
        assert!(close_to(grey, [128, 128, 128], 1), "{name}: {grey:?}");
    }
}

#[test]
fn the_walls_of_prisms_are_darker_with_more_wall_darkness_and_a_setting_changed_and_changed_back_is_the_picture_it_was()
 {
    let (dirt, normal) = (decode(DIRT), decode(DIRT_NORMAL));
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = floor(&context, &dirt, &normal, [0.5, 0.5, 0.7], 100.0);
        let camera = view_of([16.0, 17.0, 16.0], 40.0, 0.0, 1.2, 0.38);
        let by_default = settled(&mut renderer, &camera);
        let mut lights = Vec::new();
        for darkness in [0.0, 1.0] {
            renderer
                .set_shader_pack_with_settings(None, &choose("wall_darkness", darkness))
                .expect("a darkness in its range");
            lights.push(total_light(&settled(&mut renderer, &camera)));
        }
        let default_light = total_light(&by_default);
        assert!(
            lights[0] > default_light && default_light > lights[1],
            "{name}: the light of the picture is {} with no darkness at the foot of a wall, {default_light} with the default 0.2 and {} with all of it",
            lights[0],
            lights[1]
        );
        // The ends are far apart: the walls are a share of the picture, and it is a share that shows.
        assert!(
            lights[0] - lights[1] > default_light / 200,
            "{name}: the darkness of walls changes only {} of {default_light}",
            lights[0] - lights[1]
        );
        renderer
            .set_shader_pack_with_settings(None, &choose("wall_darkness", 1.0))
            .expect("a darkness in its range");
        renderer.set_shader_pack(None).expect("the defaults");
        assert_eq!(
            settled(&mut renderer, &camera),
            by_default,
            "{name}: the defaults again are the picture of the defaults"
        );
    }
}

#[test]
fn a_deeper_relief_moves_what_the_eye_sees_and_none_leaves_the_face_to_its_own_tilts() {
    let (dirt, normal) = (decode(DIRT), decode(DIRT_NORMAL));
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = floor(&context, &dirt, &normal, [0.5, 0.5, 0.7], 100.0);
        let camera = view_of([16.0, 17.0, 16.0], 40.0, 0.0, 1.2, 0.38);
        let by_default = settled(&mut renderer, &camera);
        let mut seen = Vec::new();
        for depth in [0.0, 0.2] {
            renderer
                .set_shader_pack_with_settings(None, &choose("relief_depth", depth))
                .expect("a depth in its range");
            seen.push(settled(&mut renderer, &camera));
        }
        let pixels = (WIDTH * HEIGHT) as usize;
        for (depth, picture) in [(0.0, &seen[0]), (0.2, &seen[1])] {
            let changed = differing(&by_default, picture, 6);
            assert!(
                changed > pixels / 50,
                "{name}: a relief {depth} deep changes only {changed} of {pixels} pixels against the 0.1 one"
            );
        }
        assert!(
            differing(&seen[0], &seen[1], 6) > pixels / 20,
            "{name}: a relief of no depth and of 0.2 are alike"
        );
    }
}

fn choose_relief(word: &str) -> SettingChoices {
    SettingChoices::from([("relief".to_owned(), SettingValue::Choice(word.to_owned()))])
}

#[test]
fn choosing_the_relief_flat_for_the_games_own_shader_brings_back_the_tilts_of_the_normal_pictures()
{
    let (dirt, normal) = (decode(DIRT), decode(DIRT_NORMAL));
    // The same normal picture with no tilt at all and every height at the top: what a block with no normal picture has.
    let no_normal: Vec<u8> = (0..16 * 16).flat_map(|_| [128u8, 128, 255, 255]).collect();
    let pixels = (WIDTH * HEIGHT) as usize;
    for context in contexts() {
        let name = adapter_name(&context);
        let camera = view_of([16.0, 17.0, 16.0], 40.0, 0.0, 1.2, 0.38);
        let mut renderer = floor(&context, &dirt, &normal, [0.5, 0.5, 0.7], 100.0);
        let by_default = settled(&mut renderer, &camera);
        renderer
            .set_shader_pack_with_settings(None, &choose_relief("flat"))
            .expect("flat is a relief");
        let flat = settled(&mut renderer, &camera);
        assert!(
            differing(&by_default, &flat, 6) > pixels / 10,
            "{name}: the game's own shader with the relief flat is drawn as it is with prisms"
        );
        // It is what a pack that asks for a flat relief draws, to the byte.
        let pack = renderer::ShaderPack::new("test:flat", "Flat", renderer::default_lighting())
            .expect("the default lighting is a good pack")
            .with_relief(renderer::Relief::Flat);
        renderer
            .set_shader_pack(Some(&pack))
            .expect("a card takes the relief");
        assert_eq!(
            settled(&mut renderer, &camera),
            flat,
            "{name}: a choice of flat and a pack that asks for it are one picture"
        );
        // The player's word is over the pack's: a pack that asks for prisms with the player's flat is flat.
        let prisms =
            renderer::ShaderPack::new("test:prisms", "Prisms", renderer::default_lighting())
                .expect("the default lighting is a good pack")
                .with_relief(renderer::Relief::Prisms);
        renderer
            .set_shader_pack_with_settings(Some(&prisms), &choose_relief("flat"))
            .expect("flat is a relief");
        assert_eq!(settled(&mut renderer, &camera), flat, "{name}");
        renderer
            .set_shader_pack_with_settings(None, &choose_relief("prisms"))
            .expect("prisms is a relief");
        assert_eq!(
            settled(&mut renderer, &camera),
            by_default,
            "{name}: the word of the default is the default"
        );
        // The tilts are what flat brings back: with the dirt's normal picture and with a flat one it is two pictures, because the sun's light is the cosine of the tilted normal.
        renderer
            .set_shader_pack_with_settings(None, &choose_relief("flat"))
            .expect("flat is a relief");
        let mut bare = floor(&context, &dirt, &no_normal, [0.5, 0.5, 0.7], 100.0);
        bare.set_shader_pack_with_settings(None, &choose_relief("flat"))
            .expect("flat is a relief");
        let untilted = settled(&mut bare, &camera);
        assert!(
            differing(&flat, &untilted, 4) > pixels / 10,
            "{name}: the normal picture's tilts change only {} of {pixels} pixels with the relief flat",
            differing(&flat, &untilted, 4)
        );
    }
}

#[test]
fn a_choice_that_is_no_setting_or_is_out_of_range_is_refused_and_changes_nothing() {
    let (dirt, normal) = (decode(DIRT), decode(DIRT_NORMAL));
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = floor(&context, &dirt, &normal, [0.5, 0.5, 0.7], 100.0);
        let camera = view_of([16.0, 17.0, 16.0], 40.0, 0.0, 1.2, 0.38);
        let by_default = settled(&mut renderer, &camera);
        let refused = |renderer: &mut VoxelRenderer, choices: &SettingChoices| {
            renderer
                .set_shader_pack_with_settings(None, choices)
                .expect_err("the choice is refused")
                .to_string()
        };
        let error = refused(&mut renderer, &choose("brightness", 1.0));
        assert!(
            error.contains("\"brightness\" is not a setting") && error.contains("scene_lift"),
            "{name}: {error}"
        );
        let error = refused(&mut renderer, &choose("relief_depth", 5.0));
        assert!(
            error.contains("relief_depth") && error.contains("a number from 0 to 0.2"),
            "{name}: {error}"
        );
        let error = refused(
            &mut renderer,
            &SettingChoices::from([("scene_lift".to_owned(), SettingValue::Switch(true))]),
        );
        assert!(error.contains("scene_lift"), "{name}: {error}");
        assert_eq!(
            settled(&mut renderer, &camera),
            by_default,
            "{name}: a refused choice leaves the picture as it was"
        );
    }
}

#[test]
fn making_the_shaders_again_for_a_setting_is_a_pause_and_not_a_wait() {
    let (dirt, normal) = (decode(DIRT), decode(DIRT_NORMAL));
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = floor(&context, &dirt, &normal, [0.5, 0.5, 0.7], 100.0);
        let camera = view_of([16.0, 17.0, 16.0], 40.0, 0.0, 1.2, 0.38);
        settled(&mut renderer, &camera);
        let began = std::time::Instant::now();
        renderer
            .set_shader_pack_with_settings(None, &choose("scene_lift", 1.5))
            .expect("a lift in its range");
        settled(&mut renderer, &camera);
        let took = began.elapsed();
        eprintln!("{name}: a setting changed and the picture drawn again took {took:?}");
        // (Measured, not promised: a generous bound that only a shader that is made over and over would pass.)
        assert!(
            took < std::time::Duration::from_secs(20),
            "{name}: a setting took {took:?}"
        );
    }
}
