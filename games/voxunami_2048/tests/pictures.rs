//! What the numbers look like: every number has a colour, from the number in a voxel's state to the
//! pixels, on every physical graphics adapter.
//!
//! These need a GPU. Without one they print a notice and pass, unless `VOX_REQUIRE_GPU` is set.

use game_shell::GameContent;
use game_shell::view::{camera_at, configure_renderer};
use renderer::{GraphicsContext, VoxelRenderer};
use voxel_grid::ChannelStorage;
use voxel_meshing::{BrickChannels, BrickNeighbourhood, mesh_brick};
use voxel_space::{BRICK_VOXEL_COUNT, BrickCoordinate, LocalVoxelIndex};
use voxunami_2048::content::Content2048;
use voxunami_2048::material::Material;
use voxunami_2048::numbered_block::NumberedBlock;

const SIZE: u32 = 64;
const VIEW_RADIUS: i64 = 3;

fn contexts() -> Vec<GraphicsContext> {
    let contexts = GraphicsContext::all_physical_adapters();
    if contexts.is_empty() {
        assert!(
            std::env::var_os("VOX_REQUIRE_GPU").is_none(),
            "VOX_REQUIRE_GPU is set but no graphics adapter is available"
        );
        eprintln!("no graphics adapter available; skipping GPU test");
    }
    contexts
}

/// The colour of the middle of a picture of one block, seen from the front, as the game draws it.
fn color_of_block_seen_from_the_front(context: &GraphicsContext, block: NumberedBlock) -> [u8; 4] {
    let content = Content2048::new();
    let table = content.block_table();
    let channels = content.view_channels();
    // A brick of sky with the block in the middle of it, meshed the way the game meshes it.
    let mut blocks = vec![0u32; BRICK_VOXEL_COUNT];
    let mut numbers = vec![0u32; BRICK_VOXEL_COUNT];
    let index = LocalVoxelIndex::new(16, 16, 16).index();
    blocks[index] = block.block_id();
    numbers[index] = block.state();
    let (blocks, numbers) = (
        ChannelStorage::from_values(&blocks),
        ChannelStorage::from_values(&numbers),
    );
    assert!(channels.states.is_some(), "the game draws by number");
    let mesh = mesh_brick(
        &BrickNeighbourhood {
            center: BrickChannels {
                blocks: &blocks,
                states: Some(&numbers),
                orientations: None,
                light: None,
            },
            neighbours: [None; 6],
        },
        &table,
    );
    let mut renderer = VoxelRenderer::new(context, wgpu::TextureFormat::Rgba8UnormSrgb);
    configure_renderer(&mut renderer, &table, content.ambient_light(), VIEW_RADIUS);
    renderer.upload_brick(BrickCoordinate::new(0, 0, 0), &mesh.pack());
    // Looking along -z at the block from four voxels in front of its face.
    let camera = camera_at([16.5, 16.5, 21.0], 0.0, 0.0, VIEW_RADIUS);
    let (image, statistics) = renderer.render_to_rgba(&camera, SIZE, SIZE);
    assert_eq!(statistics.quads_drawn, 6);
    let offset = ((SIZE / 2 * SIZE + SIZE / 2) * 4) as usize;
    [
        image[offset],
        image[offset + 1],
        image[offset + 2],
        image[offset + 3],
    ]
}

fn is_magenta(pixel: [u8; 4]) -> bool {
    pixel[0] > 200 && pixel[1] < 60 && pixel[2] > 200
}

#[test]
fn numbers_are_drawn_in_colours_of_their_own_and_never_as_missing_content() {
    for context in contexts() {
        let color = |material, number| {
            color_of_block_seen_from_the_front(&context, NumberedBlock::new(material, number))
        };
        let powers: Vec<[u8; 4]> = (1..=12)
            .map(|exponent| color(Material::Stone, 1 << exponent))
            .collect();
        for (position, pixel) in powers.iter().enumerate() {
            assert!(!is_magenta(*pixel), "2^{}: {pixel:?}", position + 1);
            for (other, other_pixel) in powers.iter().enumerate().skip(position + 1) {
                assert_ne!(pixel, other_pixel, "2^{} and 2^{}", position + 1, other + 1);
            }
        }
        // Numbers that are not powers of two have colours too, and one far beyond the old limit.
        for number in [3, 1000, 3_000_000_001] {
            assert!(!is_magenta(color(Material::Ice, number)), "{number}");
        }
        // The colour depends on the material as well as on the number.
        assert_ne!(color(Material::Stone, 8), color(Material::Ice, 8));
        assert_ne!(color(Material::Ice, 8), color(Material::SpringIce, 8));
        // Numbers a colour-cycle apart share their colour, which shows that the colour really
        // follows the number the voxel holds.
        assert_eq!(color(Material::Stone, 5), color(Material::Stone, 5 + 61));
        assert_ne!(color(Material::Stone, 5), color(Material::Stone, 6));
    }
}
