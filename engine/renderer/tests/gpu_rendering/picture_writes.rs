//! One picture written in place (`VoxelRenderer::set_picture`, `docs/hot-reload-plan.md` step 3): a renderer that has been given a picture this way draws exactly as one that was
//! given it at the start, on every adapter, near (the picture itself) and far (the smaller levels under it), with its companions, and a picture that does not fit is refused
//! with nothing changed.

use renderer::PictureWrite;

use super::emission::{table, white_picture};
use super::*;

const PIXELS: usize = 8;

/// A picture of 8 by 8 whose pixels differ one from another (so that a mistake of order, of rows or of levels shows) and is opaque: red grows to the right, green down.
fn gradient() -> Vec<u8> {
    (0..PIXELS * PIXELS)
        .flat_map(|index| {
            let (column, row) = (index % PIXELS, index / PIXELS);
            [30 + 28 * column as u8, 30 + 28 * row as u8, 90, 255]
        })
        .collect()
}

/// A specular picture with an emission that grows to the right, and other channels that are not zero.
fn glowing() -> Vec<u8> {
    (0..PIXELS * PIXELS)
        .flat_map(|index| [200, 150, 100, 254 - 30 * (index % PIXELS) as u8])
        .collect()
}

/// A normal picture whose height is below the top in half its pixels (a relief to walk), and whose tilt leans.
fn relief() -> Vec<u8> {
    (0..PIXELS * PIXELS)
        .flat_map(|index| {
            let column = index % PIXELS;
            [160, 100, 255, if column < PIXELS / 2 { 80 } else { 255 }]
        })
        .collect()
}

/// The renderer with one block whose face has the picture `picture` (and the companions) from the start, in the dark.
fn started_with(
    context: &GraphicsContext,
    picture: &[u8],
    specular: Option<&[u8]>,
    normal: Option<&[u8]>,
) -> VoxelRenderer {
    let table = table();
    let mut renderer = new_renderer(context);
    renderer.set_palette_with_pictures(
        &table.palette(),
        &table.palette_pictures(),
        &[PictureSize {
            size: PIXELS as u32,
            tiles: vec![picture],
            specular: vec![specular],
            normal: vec![normal],
        }],
    );
    prepare(&mut renderer, &table);
    renderer
}

/// What is the same for every renderer of these tests: the picture drawn far, the block in a lit cell, the sun over the east.
fn prepare(renderer: &mut VoxelRenderer, table: &block_models::BlockTable) {
    renderer.set_picture_distance(400.0);
    place_blocks_in(
        renderer,
        table,
        ORIGIN_BRICK,
        &[([16, 16, 16], 1, 0)],
        Some(200),
    );
    renderer.set_atmosphere(Atmosphere {
        sun_direction: SUN_OVER_THE_EAST,
        sun_light: 255,
        sky_light: 60,
        ambient_light: 40,
        ..Atmosphere::default()
    });
}

/// The face that looks at +z, seen near and far (the smaller levels), and slanted (the relief, if it has any).
fn views(renderer: &mut VoxelRenderer) -> Vec<Vec<u8>> {
    [
        ([16.5, 16.5, 19.0], [16.5, 16.5, 17.0]),
        ([16.5, 16.5, 120.0], [16.5, 16.5, 17.0]),
        ([19.0, 18.5, 20.0], [16.5, 16.5, 17.0]),
    ]
    .into_iter()
    .map(|(from, to)| settled(renderer, &camera_aimed_at(from, to)))
    .collect()
}

fn picture_of(rgba: &[u8]) -> PictureWrite<'_> {
    PictureWrite {
        rgba,
        specular: None,
        normal: None,
    }
}

#[test]
fn a_picture_written_in_place_draws_as_one_given_at_the_start_near_and_far() {
    for context in contexts() {
        let name = adapter_name(&context);
        let wanted = views(&mut started_with(&context, &gradient(), None, None));
        let mut renderer = started_with(&context, &white_picture(), None, None);
        let before = views(&mut renderer);
        assert_ne!(before, wanted, "{name}: the two pictures draw differently");
        renderer
            .set_picture(0, 0, &picture_of(&gradient()))
            .expect("a picture that fits");
        let after = views(&mut renderer);
        for (view, (found, wanted)) in after.iter().zip(&wanted).enumerate() {
            assert_eq!(
                found, wanted,
                "{name}: view {view} is not the picture given at the start"
            );
        }
    }
}

#[test]
fn writing_the_first_picture_back_restores_the_first_picture_to_the_pixel() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = started_with(&context, &white_picture(), None, None);
        let first = views(&mut renderer);
        renderer
            .set_picture(0, 0, &picture_of(&gradient()))
            .expect("a picture that fits");
        renderer
            .set_picture(0, 0, &picture_of(&white_picture()))
            .expect("a picture that fits");
        assert_eq!(views(&mut renderer), first, "{name}");
    }
}

#[test]
fn the_companions_are_written_with_the_picture_and_a_picture_with_none_clears_them() {
    for context in contexts() {
        let name = adapter_name(&context);
        let (specular, normal) = (glowing(), relief());
        let wanted = views(&mut started_with(
            &context,
            &gradient(),
            Some(&specular),
            Some(&normal),
        ));
        let mut renderer = started_with(&context, &white_picture(), None, None);
        renderer
            .set_picture(
                0,
                0,
                &PictureWrite {
                    rgba: &gradient(),
                    specular: Some(&specular),
                    normal: Some(&normal),
                },
            )
            .expect("a picture that fits");
        let with = views(&mut renderer);
        assert_eq!(
            with, wanted,
            "{name}: the same as one started with the picture and its companions (the relief too)"
        );
        // The same picture with no companions is the picture alone.
        renderer
            .set_picture(0, 0, &picture_of(&gradient()))
            .expect("a picture that fits");
        let alone = views(&mut started_with(&context, &gradient(), None, None));
        assert_eq!(
            views(&mut renderer),
            alone,
            "{name}: the companions are gone"
        );
        assert_ne!(with, alone, "{name}: and they made a difference");
    }
}

#[test]
fn a_picture_that_does_not_fit_is_refused_and_nothing_is_changed() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = started_with(&context, &gradient(), None, None);
        let before = views(&mut renderer);
        let sixteen = vec![255u8; 16 * 16 * 4];
        let one = gradient();
        let short_companion = vec![0u8; 12];
        let refused = [
            (
                "a size that is not the size's",
                renderer.set_picture(0, 0, &picture_of(&sixteen)),
            ),
            (
                "a size that is not there",
                renderer.set_picture(1, 0, &picture_of(&one)),
            ),
            (
                "a layer that is not there",
                renderer.set_picture(0, 1, &picture_of(&one)),
            ),
            (
                "a companion of another size",
                renderer.set_picture(
                    0,
                    0,
                    &PictureWrite {
                        rgba: &one,
                        specular: Some(&short_companion),
                        normal: None,
                    },
                ),
            ),
        ];
        for (what, result) in refused {
            let error = result.expect_err(what).to_string();
            assert!(!error.is_empty(), "{name}: {what} is said");
        }
        assert_eq!(views(&mut renderer), before, "{name}: nothing was changed");
    }
}

/// One block whose top is the picture at layer 0 and whose sides are the picture at layer 1 (the bottom is layer 0).
fn two_layer_table() -> block_models::BlockTable {
    let place = |layer| block_models::TileRef::new(0, layer).expect("a place");
    let block = BlockDefinition::cube([0.5, 0.5, 0.5])
        .with_texture(namespaced_id::NamespacedId::new("test", "two").unwrap())
        .with_pictures(block_models::FacePictures {
            top: place(0),
            side: place(1),
            bottom: place(0),
        });
    block_models::BlockTable::from_definitions(&[Some(&BlockDefinition::air()), Some(&block)])
}

/// The renderer with the two-layer block, its top picture `top` and its side picture `side` from the start.
fn started_with_layers(context: &GraphicsContext, top: &[u8], side: &[u8]) -> VoxelRenderer {
    let table = two_layer_table();
    let mut renderer = new_renderer(context);
    renderer.set_palette_with_pictures(
        &table.palette(),
        &table.palette_pictures(),
        &[PictureSize {
            size: PIXELS as u32,
            tiles: vec![top, side],
            ..PictureSize::default()
        }],
    );
    prepare(&mut renderer, &table);
    renderer
}

/// The side (+z) and the top of the block, near.
fn side_and_top(renderer: &mut VoxelRenderer) -> [Vec<u8>; 2] {
    [
        ([16.5, 16.5, 19.0], [16.5, 16.5, 17.0]),
        ([16.5, 19.0, 16.5], [16.5, 17.0, 16.5]),
    ]
    .map(|(from, to)| settled(renderer, &camera_aimed_at(from, to)))
}

#[test]
fn a_picture_is_written_over_its_own_layer_and_no_other() {
    for context in contexts() {
        let name = adapter_name(&context);
        let (white, gradient) = (white_picture(), gradient());
        let [side_of_white, top_of_white] =
            side_and_top(&mut started_with_layers(&context, &white, &white));
        let [side_of_gradient, top_of_gradient] =
            side_and_top(&mut started_with_layers(&context, &white, &gradient));
        assert_ne!(
            side_of_white, side_of_gradient,
            "{name}: the control: the side shows its picture"
        );
        assert_eq!(
            top_of_white, top_of_gradient,
            "{name}: the control: the top does not show the side's"
        );
        // Written over the second layer: the sides change and the top does not.
        let mut renderer = started_with_layers(&context, &white, &white);
        renderer
            .set_picture(0, 1, &picture_of(&gradient))
            .expect("a picture that fits");
        let [side, top] = side_and_top(&mut renderer);
        assert_eq!(
            side, side_of_gradient,
            "{name}: the side is the picture written"
        );
        assert_eq!(top, top_of_white, "{name}: the top is as it was");
        // And over the first: the top changes and the side stays.
        renderer
            .set_picture(0, 0, &picture_of(&gradient))
            .expect("a picture that fits");
        let [side_then, top_then] = side_and_top(&mut renderer);
        assert_eq!(side_then, side_of_gradient, "{name}: the side is as it was");
        assert_ne!(top_then, top_of_white, "{name}: the top changed");
    }
}
