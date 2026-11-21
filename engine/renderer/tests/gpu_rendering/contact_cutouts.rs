//! Why the sun's exact short ray does not walk through leaves and glass (`docs/specs/contact-trace-spec.md`, section 17): the shadow map alone leaves no strip at their feet.
//!
//! Where a wall of cubes meets the floor the map leaves a lit strip, and a closed room of cubes leaks the sun at the foot of its wall; that is what the walk through the voxels is
//! for. A block that cuts holes (leaves, glass) casts its shadow from sheets that have no bias pushed on them (`cutout_shadow_vertex` does not do what `shadow_vertex_main` does),
//! and leaves show the sheets between their voxels too, so these tests measure none of it. They are a guard: if a change gave the cut-outs the bias, or a scene is found where a
//! gap shows, they fail, and the walk through the sheets, which was built and set aside, is the answer (branch `cutouts-walk-parked`).

use block_models::BlockTable;

use super::*;

const CUBE: u32 = 1;
const LEAVES: u32 = 2;
const GLASS: u32 = 3;

/// A picture 8 pixels across that is opaque everywhere (the shadow of such a block is a plain wall's).
fn opaque_picture() -> Vec<u8> {
    (0..64).flat_map(|_| [255, 255, 255, 255]).collect()
}

/// Air, a red cube, leaves and glass, the last two with the opaque picture on every face.
fn table() -> BlockTable {
    let tile = block_models::TileRef::new(0, 0).expect("a place for the picture");
    let with_picture = |definition: BlockDefinition| {
        definition
            .with_texture(namespaced_id::NamespacedId::new("test", "picture").unwrap())
            .with_pictures(block_models::FacePictures {
                top: tile,
                side: tile,
                bottom: tile,
            })
    };
    let air = BlockDefinition::air();
    let cube = BlockDefinition::cube([0.9, 0.1, 0.1]);
    let leaves = with_picture(
        BlockDefinition::cube([0.1, 0.6, 0.1])
            .with_sight_through(true)
            .with_light_through(true)
            .with_transparency(Transparency::Leaves),
    );
    let glass = with_picture(
        BlockDefinition::cube([0.7, 0.9, 1.0])
            .with_sight_through(true)
            .with_light_through(true)
            .with_transparency(Transparency::Glass),
    );
    BlockTable::from_definitions(&[Some(&air), Some(&cube), Some(&leaves), Some(&glass)])
}

/// A renderer drawing the table's pictures, told of no voxel that blocks the sun: the shadow map alone.
fn renderer_for_the_map_alone(context: &GraphicsContext, table: &BlockTable) -> VoxelRenderer {
    let mut renderer = new_renderer(context);
    let picture = opaque_picture();
    renderer.set_palette_with_pictures(
        &table.palette(),
        &table.palette_pictures(),
        &[PictureSize {
            size: 8,
            tiles: vec![&picture[..]],
            ..PictureSize::default()
        }],
    );
    renderer.set_picture_distance(100.0);
    renderer
}

/// How many pixels of the strip at the west foot of a wall of `block` (floor of cubes, wall at x 14 to 17, z 14 to 17, four high) are lit, over five suns and two heights of the
/// camera, with the map alone, and how many were counted.
fn strip_lit_beside_a_wall_of(context: &GraphicsContext, block: u32) -> (usize, usize) {
    let table = table();
    let mut renderer = renderer_for_the_map_alone(context, &table);
    let mut blocks: Vec<([u8; 3], u32, u32)> = (0..32u8)
        .flat_map(|z| (0..32u8).map(move |x| ([x, 0, z], CUBE, 0)))
        .collect();
    for z in 14..18u8 {
        for y in 1..5u8 {
            for x in 14..18u8 {
                blocks.push(([x, y, z], block, 0));
            }
        }
    }
    renderer.upload_brick(ORIGIN_BRICK, &mesh_of_blocks(&table, &blocks, Some(0)));
    let (mut lit, mut counted) = (0, 0);
    for height in [20, 60] {
        for elevation in [5.0, 15.0, 30.0, 53.0, 75.0] {
            let (found, total) =
                super::contact_trace::strip_holes(&mut renderer, elevation, height, 16.0, 0.05);
            lit += found;
            counted += total;
        }
    }
    (lit, counted)
}

#[test]
fn the_map_alone_leaves_a_strip_at_the_foot_of_a_wall_of_cubes_and_none_at_the_foot_of_a_wall_of_leaves_or_glass()
 {
    for context in contexts() {
        let name = adapter_name(&context);
        let (cubes, counted) = strip_lit_beside_a_wall_of(&context, CUBE);
        assert!(counted >= 1_000, "{name}: {counted} pixels counted");
        assert!(
            cubes >= 100,
            "{name}: only {cubes} pixels of the strip of a wall of cubes are lit: the test could not see a gap"
        );
        for (block, what) in [(LEAVES, "leaves"), (GLASS, "glass")] {
            let (lit, counted) = strip_lit_beside_a_wall_of(&context, block);
            assert!(
                lit <= 3,
                "{name}: {lit} of {counted} pixels at the foot of a wall of {what} are lit by the map alone (a wall of cubes: {cubes})"
            );
        }
    }
}

/// How many pixels at the foot of the east wall inside the closed room of `contact_trace::room_cell`, made of `block`, are lit, over four suns, with the map alone.
fn leaks_into_a_room_of(context: &GraphicsContext, block: u32) -> usize {
    let table = table();
    let mut renderer = renderer_for_the_map_alone(context, &table);
    let mut blocks = Vec::new();
    for z in 0..32usize {
        for y in 0..32usize {
            for x in 0..32usize {
                if super::contact_trace::room_cell(x, y, z).is_some() {
                    blocks.push(([x as u8, y as u8, z as u8], block, 0));
                }
            }
        }
    }
    renderer.upload_brick(ORIGIN_BRICK, &mesh_of_blocks(&table, &blocks, Some(0)));
    [10.0, 30.0, 60.0, 85.0]
        .into_iter()
        .map(|elevation| super::contact_trace::room_leaks(&mut renderer, elevation).0)
        .sum()
}

#[test]
fn the_map_alone_leaks_the_sun_into_a_closed_room_of_cubes_and_not_into_one_of_leaves_or_glass() {
    for context in contexts() {
        let name = adapter_name(&context);
        let cubes = leaks_into_a_room_of(&context, CUBE);
        assert!(
            cubes >= 8,
            "{name}: a room of cubes leaks only {cubes} pixels: the test could not see a leak"
        );
        for (block, what) in [(LEAVES, "leaves"), (GLASS, "glass")] {
            let leaked = leaks_into_a_room_of(&context, block);
            assert_eq!(
                leaked, 0,
                "{name}: the map alone leaks {leaked} pixels into a room of {what} (a room of cubes: {cubes})"
            );
        }
    }
}
