//! Emission from a picture's specular companion (`PictureSize::specular`, `docs/specs/block-textures-spec.md` section 4.4): the alpha of a LabPBR `_s` picture says how much of
//! each pixel shines by itself (0 to 254 is none to all, 255 is none), so that a pixel shows its colour whatever light reaches it.
//!
//! The scene is one block in the dark (no sun, no lamp light reaches it), whose face has a white picture of 8 pixels across and a specular companion that gives each
//! column its own emission. The face is seen through a narrow view, one column at a time.

use block_models::BlockTable;

use super::*;

const PICTURE_PIXELS: usize = 8;
/// The alpha of the specular picture in each column of the picture: none, none, half (127 of 254), half, all, all, nothing (0), nothing.
const EMISSION_ALPHA: [u8; PICTURE_PIXELS] = [255, 255, 127, 127, 254, 254, 0, 0];
/// The brightness the same columns must have, from their alpha: `linear_brightness` adds the three channels, so a white pixel that shines wholly is 3, and one that
/// shines by 127 of 254 is half of that.
const EXPECTED: [f64; PICTURE_PIXELS] = [0.0, 0.0, 1.5, 1.5, 3.0, 3.0, 0.0, 0.0];

fn white_picture() -> Vec<u8> {
    (0..PICTURE_PIXELS * PICTURE_PIXELS)
        .flat_map(|_| [255, 255, 255, 255])
        .collect()
}

/// A specular picture whose columns have the emission of `EMISSION_ALPHA` and whose other channels are something other than zero (so that a shader that mistook them for
/// emission would show).
fn specular_picture() -> Vec<u8> {
    (0..PICTURE_PIXELS * PICTURE_PIXELS)
        .flat_map(|index| [200, 150, 100, EMISSION_ALPHA[index % PICTURE_PIXELS]])
        .collect()
}

fn table() -> BlockTable {
    let tile = block_models::TileRef::new(0, 0).expect("a place for the picture");
    let block = BlockDefinition::cube([0.5, 0.5, 0.5])
        .with_texture(namespaced_id::NamespacedId::new("test", "picture").unwrap())
        .with_pictures(block_models::FacePictures {
            top: tile,
            side: tile,
            bottom: tile,
        });
    BlockTable::from_definitions(&[Some(&BlockDefinition::air()), Some(&block)])
}

/// The renderer with the block in the dark, its picture with `specular` as its companion (or none).
fn dark_block(context: &GraphicsContext, specular: Option<&[u8]>) -> (VoxelRenderer, BlockTable) {
    let table = table();
    let mut renderer = new_renderer(context);
    let picture = white_picture();
    renderer.set_palette_with_pictures(
        &table.palette(),
        &table.palette_pictures(),
        &[PictureSize {
            size: PICTURE_PIXELS as u32,
            tiles: vec![&picture],
            specular: vec![specular],
        }],
    );
    renderer.set_picture_distance(100.0);
    // (A light of 0 in the cell in front of the face: only what the picture gives out can show.)
    place_blocks_in(
        &mut renderer,
        &table,
        ORIGIN_BRICK,
        &[([16, 16, 16], 1, 0)],
        Some(0),
    );
    (renderer, table)
}

/// The linear brightness of each column of the picture on the face that looks at +z, in the middle of its row.
fn columns(renderer: &mut VoxelRenderer, what: &str, name: &str) -> Vec<f64> {
    (0..PICTURE_PIXELS)
        .map(|column| {
            let x = 16.0 + (column as f64 + 0.5) / PICTURE_PIXELS as f64;
            let camera = camera_aimed_at([x, 16.5, 30.0], [x, 16.5, 17.0]);
            let image = settled(renderer, &camera);
            if column == 0 {
                save_image(&format!("emission_{what}"), name, &image);
            }
            linear_brightness(pixel(&image, SIZE / 2, SIZE / 2))
        })
        .collect()
}

#[test]
fn a_pixel_shines_by_the_emission_of_its_specular_picture_whatever_light_reaches_it_and_the_other_channels_are_not_emission()
 {
    for context in contexts() {
        let name = adapter_name(&context);
        let specular = specular_picture();
        let (mut renderer, _table) = dark_block(&context, Some(&specular));
        let seen = columns(&mut renderer, "with", &name);
        for (column, (&found, &wanted)) in seen.iter().zip(&EXPECTED).enumerate() {
            assert!(
                (found - wanted).abs() < 0.1,
                "{name}: column {column} (alpha {}) shines {found:.3}, wanted {wanted:.3}; all: {seen:.3?}",
                EMISSION_ALPHA[column]
            );
        }
    }
}

#[test]
fn a_picture_with_no_specular_companion_does_not_shine_and_is_drawn_as_it_was() {
    for context in contexts() {
        let name = adapter_name(&context);
        let (mut renderer, _table) = dark_block(&context, None);
        let seen = columns(&mut renderer, "without", &name);
        assert!(
            seen.iter().all(|&brightness| brightness < 0.02),
            "{name}: a picture with no companion is black in the dark: {seen:.3?}"
        );
    }
}
