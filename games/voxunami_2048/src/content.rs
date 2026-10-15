//! What the world of 2048 is made of: blocks for tiles, frame and plates, and nothing else.

use std::sync::Arc;

use content_registry::{IdentifierRegistry, Registry};
use game_shell::GameContent;
use namespaced_id::NamespacedId;
use terrain::{TerrainGenerator, WorldTerrain};
use voxel_grid::{Brick, ChannelDefinition, ChannelIndex, Grid, GridLayout};
use voxel_space::{BrickCoordinate, VoxelCoordinate};
use world_storage::{LoadContext, WorldData};

use crate::board::{Board, MAXIMUM_EXPONENT, cell_index};
use crate::layout::{cells, frame_voxels, plate_voxels, tile_voxels};

/// Terrain generators are assigned to cubes of 8 bricks per side, as in Voxunami Cave.
const ASSIGNMENT_CUBE_LEVEL: u8 = 3;

fn identifier(text: &str) -> NamespacedId {
    text.parse().expect("constant identifiers are valid")
}

pub fn game_id() -> NamespacedId {
    identifier("voxunami_2048:game")
}

pub fn block_registry_kind() -> NamespacedId {
    identifier("voxunami_2048:block")
}

pub fn block_channel_identifier() -> NamespacedId {
    identifier("voxunami_2048:block")
}

pub fn void_generator_identifier() -> NamespacedId {
    identifier("voxunami_2048:void_v1")
}

/// How a block looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockAppearance {
    pub color: [f32; 3],
    pub opaque: bool,
}

/// Terrain that is empty everywhere: the board floats in the sky.
#[derive(Debug)]
struct VoidTerrain;

impl TerrainGenerator for VoidTerrain {
    fn generate_brick(&self, layout: &GridLayout, _coordinate: BrickCoordinate) -> Brick {
        Brick::new(layout)
    }
}

/// A colour (sRGB) for a tile, stepping around the hues so neighbouring values differ clearly.
fn tile_color(exponent: u8) -> [f32; 3] {
    // Hue in sixths, converted without trigonometry.
    let hue = (f32::from(exponent - 1) * 0.37).fract() * 6.0;
    let (saturation, value) = (
        0.75,
        0.55 + 0.4 * (f32::from(exponent) / f32::from(MAXIMUM_EXPONENT)),
    );
    let channel = |offset: f32| {
        let position = (hue + offset).rem_euclid(6.0);
        let shape = (position - 3.0).abs() - 1.0;
        value * (1.0 - saturation * shape.clamp(0.0, 1.0))
    };
    [channel(0.0), channel(4.0), channel(2.0)]
}

/// The blocks, registered in a fixed order: air, frame, push plate, then tiles of 2, 4, 8, ...
#[derive(Debug)]
pub struct Content2048 {
    blocks: Registry<BlockAppearance>,
}

impl Default for Content2048 {
    fn default() -> Self {
        Self::new()
    }
}

impl Content2048 {
    pub fn new() -> Self {
        let mut blocks = Registry::new();
        let mut register = |name: String, color: [f32; 3], opaque: bool| {
            blocks
                .register(
                    identifier(&format!("voxunami_2048:{name}")),
                    BlockAppearance { color, opaque },
                )
                .expect("the block names are distinct");
        };
        register("air".to_owned(), [0.0; 3], false);
        register("frame".to_owned(), [0.30, 0.32, 0.36], true);
        register("plate".to_owned(), [0.95, 0.55, 0.10], true);
        for exponent in 1..=MAXIMUM_EXPONENT {
            register(
                format!("tile_{}", 1u64 << exponent),
                tile_color(exponent),
                true,
            );
        }
        Self { blocks }
    }

    pub fn air() -> u32 {
        0
    }

    pub fn frame() -> u32 {
        1
    }

    pub fn plate() -> u32 {
        2
    }

    /// The block of a tile with this exponent.
    pub fn tile_block(exponent: u8) -> u32 {
        assert!((1..=MAXIMUM_EXPONENT).contains(&exponent));
        2 + u32::from(exponent)
    }

    /// The exponent of the tile a block is, if it is one.
    pub fn tile_exponent(block: u32) -> Option<u8> {
        u8::try_from(block.checked_sub(2)?)
            .ok()
            .filter(|exponent| (1..=MAXIMUM_EXPONENT).contains(exponent))
    }

    fn layout(&self) -> Arc<GridLayout> {
        Arc::new(
            GridLayout::new(vec![ChannelDefinition::content(
                block_channel_identifier(),
                Self::air(),
                block_registry_kind(),
            )])
            .expect("one channel is a valid layout"),
        )
    }

    /// The channel holding each voxel's block.
    pub fn block_channel(&self) -> ChannelIndex {
        self.layout()
            .channel_index(&block_channel_identifier())
            .expect("the layout has the block channel")
    }

    /// The block id for a block named by identifier, if this program has it.
    pub fn block_named(&self, name: &NamespacedId) -> Option<u32> {
        self.blocks.raw_id_of(name)
    }
}

/// Fills a cell with the tile of `exponent`, or clears it.
pub fn write_cell(grid: &mut Grid, channel: ChannelIndex, cell: usize, exponent: u8) {
    let block = if exponent == 0 {
        Content2048::air()
    } else {
        Content2048::tile_block(exponent)
    };
    for voxel in tile_voxels(cell) {
        grid.set(channel, voxel, block);
    }
}

/// The board as the voxels show it.
pub fn read_board(grid: &Grid, channel: ChannelIndex) -> Board {
    let mut board = Board::empty();
    for cell in cells() {
        let origin = crate::layout::cell_origin(cell);
        let block = grid.get(channel, origin);
        board.set(cell, Content2048::tile_exponent(block).unwrap_or(0));
    }
    board
}

/// The board a new game starts with: two tiles, placed from the seed and the game's number.
pub fn starting_board(seed: u64, game_number: u64) -> Board {
    let mut board = Board::empty();
    let game_seed = seed ^ terrain::mix(game_number);
    for step in 0..2 {
        board.spawn(game_seed, step);
    }
    board
}

/// The seed a game's tiles are spawned from.
pub fn game_seed(seed: u64, game_number: u64) -> u64 {
    seed ^ terrain::mix(game_number)
}

impl GameContent for Content2048 {
    fn material_channel(&self) -> ChannelIndex {
        self.block_channel()
    }

    fn palette(&self) -> Vec<[f32; 3]> {
        self.blocks
            .iter()
            .map(|(_, entry)| entry.content().map_or([1.0, 0.0, 1.0], |block| block.color))
            .collect()
    }

    fn opacity_table(&self) -> Vec<bool> {
        self.blocks
            .iter()
            .map(|(_, entry)| entry.content().is_none_or(|block| block.opaque))
            .collect()
    }

    /// A new world: the frame, the plates, and a new game's two tiles.
    fn new_world(&self, seed: u64) -> WorldData {
        let layout = self.layout();
        let channel = self.block_channel();
        let mut generators: Registry<Arc<dyn TerrainGenerator>> = Registry::new();
        let generator: Arc<dyn TerrainGenerator> = Arc::new(VoidTerrain);
        let default = generators
            .register(void_generator_identifier(), generator)
            .expect("the registry is empty");
        let terrain = WorldTerrain::new(generators, default, ASSIGNMENT_CUBE_LEVEL)
            .expect("the generator is available");
        let mut grid = Grid::new(layout);
        for voxel in frame_voxels() {
            grid.set(channel, voxel, Self::frame());
        }
        for toward in voxel_space::FaceDirection::ALL {
            for voxel in plate_voxels(toward) {
                grid.set(channel, voxel, Self::plate());
            }
        }
        let board = starting_board(seed, 0);
        for cell in cells() {
            write_cell(&mut grid, channel, cell, board.get(cell));
        }
        WorldData::new(seed, terrain, grid)
    }

    fn load_context(&mut self, _seed: u64) -> LoadContext<'_> {
        let mut generators: Registry<Arc<dyn TerrainGenerator>> = Registry::new();
        let generator: Arc<dyn TerrainGenerator> = Arc::new(VoidTerrain);
        generators
            .register(void_generator_identifier(), generator)
            .expect("the registry is empty");
        let channel = ChannelDefinition::content(
            block_channel_identifier(),
            Self::air(),
            block_registry_kind(),
        );
        LoadContext {
            channels: vec![channel],
            registries: vec![(block_registry_kind(), &mut self.blocks)],
            generators,
            fallback_default_generator: void_generator_identifier(),
            assignment_cube_level: ASSIGNMENT_CUBE_LEVEL,
        }
    }

    fn registries(&self) -> Vec<(NamespacedId, &dyn IdentifierRegistry)> {
        vec![(block_registry_kind(), &self.blocks)]
    }
}

/// The voxel at a cell's origin, for tests and readers that want one voxel.
pub fn cell_voxel(x: usize, y: usize, z: usize) -> VoxelCoordinate {
    crate::layout::cell_origin(cell_index(x, y, z))
}

#[cfg(test)]
mod tests {
    use world_storage::ParsedWorld;

    use super::*;
    use crate::board::cell_coordinates;

    #[test]
    fn blocks_are_registered_in_a_fixed_order_and_tiles_map_both_ways() {
        let content = Content2048::new();
        let name = |raw: u32| content.blocks.identifier_of_raw(raw).unwrap().to_string();
        assert_eq!(name(0), "voxunami_2048:air");
        assert_eq!(name(Content2048::frame()), "voxunami_2048:frame");
        assert_eq!(name(Content2048::plate()), "voxunami_2048:plate");
        assert_eq!(name(Content2048::tile_block(1)), "voxunami_2048:tile_2");
        assert_eq!(name(Content2048::tile_block(11)), "voxunami_2048:tile_2048");
        assert_eq!(content.palette().len(), 3 + MAXIMUM_EXPONENT as usize);
        for exponent in 1..=MAXIMUM_EXPONENT {
            assert_eq!(
                Content2048::tile_exponent(Content2048::tile_block(exponent)),
                Some(exponent)
            );
        }
        assert_eq!(Content2048::tile_exponent(Content2048::air()), None);
        assert_eq!(Content2048::tile_exponent(Content2048::plate()), None);
        assert_eq!(Content2048::tile_exponent(500), None);
    }

    #[test]
    fn tile_colours_are_distinct_valid_and_visible() {
        let colors: Vec<[f32; 3]> = (1..=MAXIMUM_EXPONENT).map(tile_color).collect();
        for color in &colors {
            assert!(color.iter().all(|c| (0.0..=1.0).contains(c)), "{color:?}");
            assert!(color.iter().any(|&c| c > 0.3), "{color:?} is nearly black");
        }
        // The first dozen tiles, the ones a game reaches, are all clearly different.
        for first in 0..12 {
            for second in first + 1..12 {
                let distance: f32 = (0..3)
                    .map(|i| (colors[first][i] - colors[second][i]).abs())
                    .sum();
                assert!(distance > 0.12, "tiles {first} and {second}: {distance}");
            }
        }
    }

    #[test]
    fn a_new_world_holds_the_frame_the_plates_and_two_tiles_and_reads_back() {
        let content = Content2048::new();
        let world = content.new_world(5);
        let channel = content.block_channel();
        let board = read_board(&world.overrides, channel);
        assert_eq!(board.tile_count(), 2);
        assert_eq!(board, starting_board(5, 0));
        assert_eq!(
            world.overrides.brick_count(),
            1,
            "everything is in one brick"
        );
        assert_eq!(
            world
                .overrides
                .get(channel, voxel_space::VoxelCoordinate::new(3, 3, 3)),
            Content2048::frame()
        );
        assert_eq!(
            world
                .overrides
                .get(channel, voxel_space::VoxelCoordinate::new(0, 13, 13)),
            Content2048::plate()
        );
        assert_ne!(
            starting_board(5, 0),
            starting_board(5, 1),
            "a new game differs"
        );
        assert_ne!(starting_board(5, 0), starting_board(6, 0));
    }

    #[test]
    fn writing_a_cell_fills_exactly_its_tile_and_clears_it_again() {
        let content = Content2048::new();
        let mut world = content.new_world(1);
        let channel = content.block_channel();
        let cell = cell_index(2, 1, 3);
        write_cell(&mut world.overrides, channel, cell, 4);
        let mut board = read_board(&world.overrides, channel);
        assert_eq!(board.get(cell), 4);
        let tile_block = Content2048::tile_block(4);
        let count = (0..32)
            .flat_map(|x| (0..32).flat_map(move |y| (0..32).map(move |z| (x, y, z))))
            .filter(|&(x, y, z)| {
                world
                    .overrides
                    .get(channel, voxel_space::VoxelCoordinate::new(x, y, z))
                    == tile_block
            })
            .count();
        assert_eq!(count, 64);
        write_cell(&mut world.overrides, channel, cell, 0);
        board = read_board(&world.overrides, channel);
        assert_eq!(board.get(cell), 0);
        let [x, y, z] = cell_coordinates(cell);
        assert_eq!(cell_voxel(x, y, z), crate::layout::cell_origin(cell));
    }

    #[test]
    fn a_world_saves_and_loads_back_the_same() {
        let content = Content2048::new();
        let world = content.new_world(11);
        let bytes = world.save(&content.registries()).unwrap();
        let mut later = Content2048::new();
        let parsed = ParsedWorld::parse(&bytes).unwrap();
        let seed = parsed.world_seed();
        let (loaded, report) = parsed.load(later.load_context(seed)).unwrap();
        assert!(report.missing_content.is_empty() && report.extra_channels.is_empty());
        assert_eq!(loaded.save(&later.registries()).unwrap(), bytes);
        assert_eq!(
            read_board(&loaded.overrides, later.block_channel()),
            starting_board(11, 0)
        );
    }
}
