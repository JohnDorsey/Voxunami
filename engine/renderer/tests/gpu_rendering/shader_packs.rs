//! Shader packs (`docs/specs/shader-pack-spec.md`): a pack's `pack_shade` makes the colour of a face of what the engine works out of it.
//!
//! * the engine's answers reach the pack as they are: the visibilities of the sun and the sky, the colour and the normal of the face;
//! * a pack changes the colour of every kind of voxel shader (plain, with pictures, with the walk of the sun's ray, with other grids' windows and entities), and the fog is the
//!   engine's still;
//! * giving the pack back and forth leaves the pictures as they were, and a pack that is the default's text is drawn as the default is.

use renderer::ShaderPack;

use super::contact_trace::occupancy_where;
use super::*;

fn pack(body: &str) -> ShaderPack {
    ShaderPack::new("test:pack", "Test pack", body).expect("the pack is valid")
}

/// A pack that makes every face one colour.
fn paints(red: f32, green: f32, blue: f32) -> ShaderPack {
    pack(&format!(
        "fn pack_shade(s: Surface) -> vec3<f32> {{ return vec3<f32>({red:?}, {green:?}, {blue:?}); }}"
    ))
}

const MAGENTA: [u8; 3] = [255, 0, 255];

/// The byte of an 8-bit sRGB picture that a linear value comes out as.
fn srgb_byte(linear: f64) -> u8 {
    let encoded = if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        1.055 * libm::pow(linear, 1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round() as u8
}

/// The shadow tests' block scene, lit by the sun over the east with the shadows on, and the camera and the places probed.
fn block_scene(context: &GraphicsContext) -> (VoxelRenderer, Camera, Vec<(u32, u32, bool)>) {
    let mut renderer = new_renderer(context);
    upload_shadow_scene(&mut renderer, ShadowScene::Block, 0);
    let (camera, cells, probes) = shadow_scene_setup(ShadowScene::Block, 0);
    renderer.set_atmosphere(shadow_atmosphere(0, SUN_OVER_THE_EAST, true));
    let pixels = probes
        .iter()
        .filter_map(|&(place, shadowed)| {
            let relative = std::array::from_fn(|axis| place[axis] - cells[axis] as f64);
            pixel_of(&camera, relative).map(|(x, y)| (x, y, shadowed))
        })
        .collect::<Vec<_>>();
    assert_eq!(pixels.len(), probes.len(), "every probe is in the picture");
    (renderer, camera, pixels)
}

#[test]
fn a_pack_is_given_the_visibilities_the_engine_works_out_as_they_are() {
    for context in contexts() {
        let name = adapter_name(&context);
        let (mut renderer, camera, probes) = block_scene(&context);
        // Red is how much of the sun reaches the place, green how much of the sky's light, blue the share that is not hidden by what is over the place (nothing is counted: 1).
        renderer
            .set_shader_pack(Some(&pack(
                "fn pack_shade(s: Surface) -> vec3<f32> { return vec3<f32>(s.sun_visible, s.sky_visible, s.uncovered); }",
            )))
            .expect("the graphics card takes the pack");
        let image = render(&mut renderer, &camera);
        let (mut shadowed_seen, mut lit_seen) = (0, 0);
        for (x, y, shadowed) in probes {
            let [red, green, blue, _] = pixel(&image, x, y);
            assert_eq!(blue, 255, "{name}: nothing hides the sky from ({x}, {y})");
            if shadowed {
                // In the shadow of the block: none of the sun, and the share of the sky's light that the atmosphere leaves a place the terrain puts in shadow.
                assert!(
                    red <= 2,
                    "{name}: the sun reaches a place in the shadow ({x}, {y}): {red}"
                );
                let fill = srgb_byte(f64::from(SHADOWED_ATMOSPHERE_FILL) / 255.0);
                assert!(
                    green.abs_diff(fill) <= 2,
                    "{name}: the sky's light in a shadow is {green}, not {fill} ({x}, {y})"
                );
                shadowed_seen += 1;
            } else {
                assert!(
                    red >= 253,
                    "{name}: the sun does not reach a lit place ({x}, {y}): {red}"
                );
                assert!(
                    green >= 253,
                    "{name}: the sky does not light a lit place ({x}, {y}): {green}"
                );
                lit_seen += 1;
            }
        }
        assert!(
            shadowed_seen >= 4 && lit_seen >= 6,
            "{name}: {shadowed_seen} shadowed, {lit_seen} lit"
        );
    }
}

#[test]
fn a_pack_is_given_the_colour_and_the_normal_of_the_face() {
    for context in contexts() {
        let name = adapter_name(&context);
        let (mut renderer, camera, probes) = block_scene(&context);
        // The floor is a green cube's top face: its colour is the palette's, and its normal is up.
        let &(x, y, _) = probes.iter().find(|probe| !probe.2).expect("a lit probe");
        renderer
            .set_shader_pack(Some(&pack(
                "fn pack_shade(s: Surface) -> vec3<f32> { return s.albedo; }",
            )))
            .unwrap();
        let [red, green, blue, _] = pixel(&render(&mut renderer, &camera), x, y);
        // The palette's green, (0.1, 0.8, 0.1) in sRGB as the palette is given, comes out as the same bytes, within two.
        assert!(
            close_to([red, green, blue, 255], [26, 204, 26], 2),
            "{name}: the albedo is ({red}, {green}, {blue})"
        );
        renderer
            .set_shader_pack(Some(&pack(
                "fn pack_shade(s: Surface) -> vec3<f32> { return s.normal * 0.5 + vec3<f32>(0.5); }",
            )))
            .unwrap();
        let [red, green, blue, _] = pixel(&render(&mut renderer, &camera), x, y);
        // (0.5, 1, 0.5) in linear light is (188, 255, 188) as bytes.
        assert!(
            close_to([red, green, blue, 255], [188, 255, 188], 2),
            "{name}: the normal is ({red}, {green}, {blue})"
        );
    }
}

#[test]
fn a_pack_colours_every_face_and_leaves_the_sky_and_the_fog_to_the_engine() {
    for context in contexts() {
        let name = adapter_name(&context);
        let (mut renderer, camera, probes) = block_scene(&context);
        renderer
            .set_shader_pack(Some(&paints(1.0, 0.0, 1.0)))
            .unwrap();
        let image = render(&mut renderer, &camera);
        for (x, y, _) in &probes {
            let [red, green, blue, _] = pixel(&image, *x, *y);
            assert_eq!(
                [red, green, blue],
                MAGENTA,
                "{name}: ({x}, {y}) is not the pack's colour"
            );
        }
        // The sky is the engine's: looking up at it the picture is the same with the pack and without.
        let up = camera_aimed_at([16.0, 10.0, 16.0], [16.0, 60.0, 16.0]);
        let sky_with = render(&mut renderer, &up);
        renderer.set_shader_pack(None).unwrap();
        assert_eq!(
            render(&mut renderer, &up),
            sky_with,
            "{name}: the sky is the pack's"
        );
        assert!(
            is_sky(pixel(&sky_with, SIZE / 2, SIZE / 2)),
            "{name}: nothing but the sky is in the picture"
        );
        renderer
            .set_shader_pack(Some(&paints(1.0, 0.0, 1.0)))
            .unwrap();
        // The fog is the engine's: where the fog is whole the terrain is the sky's colour, the pack's colour or not.
        renderer.set_atmosphere(Atmosphere {
            fog_start: 0.0,
            fog_end: 1.0,
            ..shadow_atmosphere(0, SUN_OVER_THE_EAST, true)
        });
        let image = render(&mut renderer, &camera);
        let (x, y, _) = probes[0];
        assert!(
            close_to(pixel(&image, x, y), SKY_BYTES, 3),
            "{name}: fog does not cover the pack's colour: {:?}",
            pixel(&image, x, y)
        );
    }
}

#[test]
fn giving_a_pack_and_taking_it_away_leaves_the_pictures_as_they_were_and_the_default_text_draws_as_the_default_does()
 {
    for context in contexts() {
        let name = adapter_name(&context);
        let (mut renderer, camera, _) = block_scene(&context);
        let before = render(&mut renderer, &camera);
        renderer
            .set_shader_pack(Some(&paints(0.0, 1.0, 0.0)))
            .unwrap();
        let during = render(&mut renderer, &camera);
        assert_ne!(before, during, "{name}: the pack changed nothing");
        renderer.set_shader_pack(None).unwrap();
        assert_eq!(
            render(&mut renderer, &camera),
            before,
            "{name}: the default is not what it was"
        );
        // A pack that is the default's equation, with a comment added: made again as a shader of its own, and the same pixels.
        let default = ShaderPack::new(
            "test:default",
            "The default",
            &format!("// the same\n{}", renderer::default_lighting()),
        )
        .unwrap();
        renderer.set_shader_pack(Some(&default)).unwrap();
        assert_eq!(
            render(&mut renderer, &camera),
            before,
            "{name}: a pack of the default's text draws otherwise"
        );
    }
}

#[test]
fn the_pack_makes_the_colour_in_the_shaders_with_pictures_with_the_walk_of_the_sun_and_with_other_grids_and_entities()
 {
    for context in contexts() {
        let name = adapter_name(&context);
        // With pictures: a block with the half-clear picture, looked at straight on.
        let table = half_clear_table(Transparency::Opaque);
        let mut renderer = renderer_with_half_clear(&context, &table);
        place_blocks_in(
            &mut renderer,
            &table,
            ORIGIN_BRICK,
            &[([16, 16, 16], 1, 0)],
            None,
        );
        renderer
            .set_shader_pack(Some(&paints(1.0, 0.0, 1.0)))
            .unwrap();
        let camera = camera_aimed_at([16.25, 16.5, 30.0], [16.25, 16.5, 17.0]);
        let image = settled(&mut renderer, &camera);
        assert_eq!(
            &pixel(&image, SIZE / 2, SIZE / 2)[..3],
            &MAGENTA,
            "{name}: with pictures"
        );
        renderer.set_shader_pack(None).unwrap();
        assert!(
            is_red(pixel(&settled(&mut renderer, &camera), SIZE / 2, SIZE / 2)),
            "{name}: the picture again"
        );

        // With the walk of the sun's ray, through shapes, other grids' windows and entities: the heaviest kind.
        let mut renderer = new_renderer(&context);
        upload_shadow_scene(&mut renderer, ShadowScene::Block, 0);
        renderer.set_occupancy(
            ORIGIN_BRICK,
            &occupancy_where(|x, y, z| ShadowScene::Block.cell(x, y, z).is_some()),
        );
        renderer.set_grid_pose(a_ship(), pose([0, 0, 0], [0.0; 3], Orientation::IDENTITY));
        renderer.set_grid_occupancy(
            a_ship(),
            ORIGIN_BRICK,
            &occupancy_where(|x, y, z| x == 3 && y == 3 && z == 3),
        );
        renderer.set_avatars(&[avatar_at(ORIGIN_BRICK, [8.0, 3.5, 8.0], 1)]);
        renderer.set_atmosphere(shadow_atmosphere(0, SUN_OVER_THE_EAST, true));
        let (camera, cells, probes) = shadow_scene_setup(ShadowScene::Block, 0);
        // (Drawn once first, so that the shaders with the walk are made, with the default pack, before the pack comes: they are made again.)
        let (before, _) = renderer.render_to_rgba(&camera, SIZE, SIZE);
        renderer
            .set_shader_pack(Some(&paints(1.0, 0.0, 1.0)))
            .unwrap();
        let (image, statistics) = renderer.render_to_rgba(&camera, SIZE, SIZE);
        assert_ne!(image, before, "{name}: the pack changed nothing");
        assert!(
            statistics.traced_windows == 2
                && statistics.walked_other_windows
                && statistics.walked_entities,
            "{name}: the scene does not draw with the heaviest kind: {statistics:?}"
        );
        let (place, _) = probes[0];
        let relative = std::array::from_fn(|axis| place[axis] - cells[axis] as f64);
        let (x, y) = pixel_of(&camera, relative).expect("the probe is in the picture");
        assert_eq!(&pixel(&image, x, y)[..3], &MAGENTA, "{name}: with the walk");
    }
}

#[test]
fn a_pack_is_told_how_much_of_the_sun_is_over_the_horizon() {
    for context in contexts() {
        let name = adapter_name(&context);
        let (mut renderer, camera, probes) = block_scene(&context);
        renderer
            .set_shader_pack(Some(&pack(
                "fn pack_shade(s: Surface) -> vec3<f32> { return vec3<f32>(s.above_horizon, s.sun_visible, 0.0); }",
            )))
            .unwrap();
        let &(x, y, _) = probes.iter().find(|probe| !probe.2).expect("a lit probe");
        // The level's ground is flat: a sun under it is hidden, and over it is not.
        renderer.set_sun_occluders(&[SunOccluder::FlatGround]);
        let by_the_sun = |renderer: &mut VoxelRenderer, sun: [f32; 3]| {
            renderer.set_atmosphere(shadow_atmosphere(0, sun, true));
            let [red, green, _, _] = pixel(&render(renderer, &camera), x, y);
            (red, green)
        };
        assert_eq!(
            by_the_sun(&mut renderer, SUN_OVER_THE_EAST),
            (255, 255),
            "{name}: a sun 53 degrees up"
        );
        let (above, visible) = by_the_sun(&mut renderer, [0.6, -0.8, 0.0]);
        assert_eq!(
            (above, visible),
            (0, 0),
            "{name}: a sun 53 degrees under the ground"
        );
    }
}

#[test]
fn a_pack_is_given_the_lamp_light_the_side_of_the_face_and_where_the_place_is() {
    for context in contexts() {
        let name = adapter_name(&context);
        let (mut renderer, camera, probes) = block_scene(&context);
        renderer.set_atmosphere(Atmosphere {
            ambient_light: 100,
            ..shadow_atmosphere(0, SUN_OVER_THE_EAST, true)
        });
        // Red is the light of the lamps and the ambient light, green how bright the side of the face makes it, blue how far under the camera the place is, over 32 voxels.
        renderer
            .set_shader_pack(Some(&pack(
                "fn pack_shade(s: Surface) -> vec3<f32> { return vec3<f32>(s.lamp, s.side, abs(s.relative.y) / 32.0); }",
            )))
            .unwrap();
        let image = render(&mut renderer, &camera);
        let &(x, y, _) = probes.iter().find(|probe| !probe.2).expect("a lit probe");
        let [red, green, blue, _] = pixel(&image, x, y);
        // (The mesh has no light of its own here, so the lamps give nothing and the ambient light is all.)
        let wanted = [
            srgb_byte(100.0 / 255.0),
            // The top of a floor, 0.8 along the sun the sides are shaded by.
            srgb_byte(0.4 + 0.6 * 0.8),
            // The floor's top is at y = 1 and the camera at 21: 20 voxels.
            srgb_byte(20.0 / 32.0),
        ];
        for (found, wanted, what) in [
            (red, wanted[0], "lamp"),
            (green, wanted[1], "side"),
            (blue, wanted[2], "place"),
        ] {
            assert!(
                found.abs_diff(wanted) <= 2,
                "{name}: the {what} is {found}, not {wanted}"
            );
        }
    }
}

#[test]
fn a_pack_is_told_how_much_of_the_sky_is_over_the_place() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        upload_shadow_scene(&mut renderer, ShadowScene::Roof, 0);
        renderer.set_sun_occluders(&[SunOccluder::FlatGround]);
        renderer.set_atmosphere(Atmosphere {
            sky_light_under_cover: 0,
            ..shadow_atmosphere(0, SUN_OVER_THE_EAST, true)
        });
        let (camera, cells, probes) = shadow_scene_setup(ShadowScene::Roof, 0);
        renderer
            .set_shader_pack(Some(&pack(
                "fn pack_shade(s: Surface) -> vec3<f32> { return vec3<f32>(s.uncovered, 0.0, 0.0); }",
            )))
            .unwrap();
        let image = render(&mut renderer, &camera);
        // The first four probes are the floor in the roof's shadow, well under it: the terrain over them hides the sky.
        let mut under = 0;
        for &(place, shadowed) in probes.iter().filter(|probe| probe.1) {
            let relative = std::array::from_fn(|axis| place[axis] - cells[axis] as f64);
            let (x, y) = pixel_of(&camera, relative).expect("in the picture");
            let [red, _, _, _] = pixel(&image, x, y);
            // (Not none of it: the roof ends 2 to 6 voxels from these places, and the columns that are looked at go out to 8.)
            assert!(
                shadowed && red < 230,
                "{name}: the sky is not hidden from {place:?}: {red}"
            );
            under += 1;
        }
        assert_eq!(under, 4);
        // With the cover not counted the same places are open.
        renderer.set_atmosphere(shadow_atmosphere(0, SUN_OVER_THE_EAST, true));
        let image = render(&mut renderer, &camera);
        let (place, _) = probes[0];
        let relative = std::array::from_fn(|axis| place[axis] - cells[axis] as f64);
        let (x, y) = pixel_of(&camera, relative).unwrap();
        assert_eq!(
            pixel(&image, x, y)[0],
            255,
            "{name}: open with no cover counted"
        );
    }
}

/// The text of the lighting file of an example pack of the renderer's `example_shader_packs` folder.
fn example(name: &str) -> ShaderPack {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("example_shader_packs")
        .join(name);
    let lighting =
        std::fs::read_to_string(folder.join("lighting.wgsl")).expect("the example's lighting file");
    ShaderPack::new(&format!("voxunami:{name}"), name, &lighting)
        .expect("an example is a good pack")
}

#[test]
fn the_example_packs_are_taken_by_every_graphics_card_and_do_what_they_say() {
    for context in contexts() {
        let name = adapter_name(&context);
        let (mut renderer, camera, probes) = block_scene(&context);
        let default = render(&mut renderer, &camera);
        let brightness_at = |image: &[u8], probe: &(u32, u32, bool)| {
            linear_brightness(pixel(image, probe.0, probe.1))
        };
        let shadowed = probes.iter().find(|probe| probe.2).unwrap();
        let lit = probes.iter().find(|probe| !probe.2).unwrap();
        // The default: the block's shadow is about a ninth as bright as the light.
        assert!(
            brightness_at(&default, shadowed) < 0.2 * brightness_at(&default, lit),
            "{name}: the default's shadow"
        );
        // Cel shading: taken, a different picture, the shadow still a shadow.
        renderer
            .set_shader_pack(Some(&example("cel")))
            .expect("the card takes the cel pack");
        let cel = render(&mut renderer, &camera);
        assert_ne!(cel, default, "{name}: cel shading changes nothing");
        assert!(
            brightness_at(&cel, shadowed) < 0.2 * brightness_at(&cel, lit),
            "{name}: the cel pack's shadow"
        );
        // No shadows: the same brightness in the block's shadow as in the light.
        renderer
            .set_shader_pack(Some(&example("shadowless")))
            .expect("the card takes the shadowless pack");
        let flat = render(&mut renderer, &camera);
        let (in_shadow, in_light) = (brightness_at(&flat, shadowed), brightness_at(&flat, lit));
        assert!(
            (in_shadow - in_light).abs() < 0.02,
            "{name}: the shadowless pack: {in_shadow} against {in_light}"
        );
        renderer.set_shader_pack(None).unwrap();
        assert_eq!(
            render(&mut renderer, &camera),
            default,
            "{name}: the default again"
        );
    }
}
