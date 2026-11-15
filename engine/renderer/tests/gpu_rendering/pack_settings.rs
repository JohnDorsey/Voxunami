//! The settings of a shader pack (`ShaderPack::new_with_settings`, `ShaderPack::with_engine_defaults`), looked at in what they do to a picture on every adapter: a pack's own
//! setting is a constant of its lighting, and the defaults a pack sets for the engine's settings are the ones its shaders are made with unless the player has chosen.

use renderer::{Heavens, SettingChoices, SettingValue, ShaderPack, default_lighting};

use super::lift::{lifted_byte, lifted_byte_by};
use super::shader_packs::{block_scene, example, example_settings};
use super::*;

fn choose(name: &str, value: SettingValue) -> SettingChoices {
    SettingChoices::from([(name.to_owned(), value)])
}

#[test]
fn the_steps_of_the_cel_pack_are_a_setting_that_changes_the_picture_and_the_default_is_three() {
    for context in contexts() {
        let name = adapter_name(&context);
        let (mut renderer, camera, _) = block_scene(&context);
        let cel = example("cel");
        renderer.set_shader_pack(Some(&cel)).expect("cel is taken");
        let by_default = render(&mut renderer, &camera);
        let mut seen = Vec::new();
        for steps in [2, 8] {
            renderer
                .set_shader_pack_with_settings(
                    Some(&cel),
                    &choose("steps", SettingValue::Integer(steps)),
                )
                .expect("steps in range");
            seen.push(render(&mut renderer, &camera));
        }
        assert_ne!(seen[0], by_default, "{name}: two steps are three");
        assert_ne!(seen[1], by_default, "{name}: eight steps are three");
        assert_ne!(seen[0], seen[1], "{name}: two steps are eight");
        // Saying the default is the default, and so is not saying.
        renderer
            .set_shader_pack_with_settings(Some(&cel), &choose("steps", SettingValue::Integer(3)))
            .expect("the default");
        assert_eq!(render(&mut renderer, &camera), by_default, "{name}");
        // A value out of range is refused and the picture is as it was.
        let error = renderer
            .set_shader_pack_with_settings(Some(&cel), &choose("steps", SettingValue::Integer(9)))
            .expect_err("nine is out of range")
            .to_string();
        assert!(error.contains("steps"), "{name}: {error}");
        assert_eq!(render(&mut renderer, &camera), by_default, "{name}");
        // Without the pack there is no setting called steps.
        assert!(
            renderer
                .set_shader_pack_with_settings(None, &choose("steps", SettingValue::Integer(4)))
                .is_err(),
            "{name}: the game's own shader has no steps"
        );
        assert_eq!(example_settings("cel").len(), 1);
    }
}

/// The middle pixel of a sky of this colour drawn with this pack (or none) and these choices, with the heavens given.
fn sky_with(
    context: &GraphicsContext,
    pack: Option<&ShaderPack>,
    choices: &SettingChoices,
    heavens: Heavens,
) -> [u8; 4] {
    let mut renderer = new_renderer(context);
    renderer
        .set_shader_pack_with_settings(pack, choices)
        .expect("a pack and choices the card takes");
    renderer.set_atmosphere(Atmosphere {
        sky_color: [0.5, 0.5, 0.5],
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
fn a_pack_sets_the_default_of_an_engine_setting_and_the_player_chooses_over_it() {
    let by_the_shader = Heavens {
        star_light: 1.0e-9,
        ..Heavens::default()
    };
    let unlifted = ShaderPack::new("test:unlifted", "Unlifted", default_lighting())
        .expect("a pack")
        .with_engine_defaults(choose("scene_lift", SettingValue::Float(1.0)))
        .expect("a lift in range");
    let plain = ShaderPack::new("test:plain", "Plain", default_lighting()).expect("a pack");
    for context in contexts() {
        let name = adapter_name(&context);
        for (way, heavens) in [
            ("cleared to", Heavens::default()),
            ("drawn by the shader as", by_the_shader),
        ] {
            let none = SettingChoices::new();
            let seen = sky_with(&context, Some(&unlifted), &none, heavens);
            assert!(
                close_to(seen, [128, 128, 128], 1),
                "{name}: a grey sky {way} is {seen:?} under a pack that sets no lift"
            );
            let seen = sky_with(
                &context,
                Some(&unlifted),
                &choose("scene_lift", SettingValue::Float(2.0)),
                heavens,
            );
            let wanted = [lifted_byte_by(0.5, 2.0); 3];
            assert!(
                close_to(seen, wanted, 1),
                "{name}: a grey sky {way} is {seen:?} when the player chooses 2, not {wanted:?}"
            );
            // The same lighting in a pack that sets nothing is the engine's own lift.
            let seen = sky_with(&context, Some(&plain), &none, heavens);
            assert!(
                close_to(seen, [lifted_byte(0.5); 3], 1),
                "{name}: a grey sky {way} is {seen:?} under a pack that says nothing"
            );
        }
        // One renderer, one pack and then the other with the same lighting: the defaults are the pack's, not what was made first.
        let mut renderer = new_renderer(&context);
        renderer.set_atmosphere(Atmosphere {
            sky_color: [0.5, 0.5, 0.5],
            ..Atmosphere::default()
        });
        let camera = camera_at(BrickCoordinate::new(0, 0, 0), [16.0, 16.0, 40.0]);
        let none = SettingChoices::new();
        renderer
            .set_shader_pack_with_settings(Some(&unlifted), &none)
            .expect("a pack");
        let first = pixel(&render(&mut renderer, &camera), SIZE / 2, SIZE / 2);
        renderer
            .set_shader_pack_with_settings(Some(&plain), &none)
            .expect("a pack");
        let second = pixel(&render(&mut renderer, &camera), SIZE / 2, SIZE / 2);
        assert!(close_to(first, [128, 128, 128], 1), "{name}: {first:?}");
        assert!(
            close_to(second, [lifted_byte(0.5); 3], 1),
            "{name}: {second:?}"
        );
    }
}
