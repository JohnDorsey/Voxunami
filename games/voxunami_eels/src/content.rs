//! What the world of Eels is made of: air, the arena's wall, and the eels' bodies, and how worlds
//! are made and loaded.
//!
//! A voxel is a block and, as the state of the block, the colour slot of the eel it belongs to.

use std::sync::Arc;

use block_models::BlockDefinition;
use content_registry::{IdentifierRegistry, Registry};
use game_shell::{ContentPackage, GameContent, PackErrors, PackageData, PackageSpec, RawPack};
use namespaced_id::NamespacedId;
use terrain::TerrainGenerator;
use voxel_grid::{ChannelDefinition, ChannelIndex};
use world_storage::{LoadContext, WorldData};
use world_view::ViewChannels;

use crate::arena::ArenaWalls;

/// The blocks' raw ids, in the order they are registered.
pub mod blocks {
    pub const AIR: u32 = 0;
    /// A dark panel of the arena's wall.
    pub const WALL: u32 = 1;
    /// A bright line of the wall.
    pub const WALL_LINE: u32 = 2;
    /// The body an eel leaves behind; its state is the eel's colour.
    pub const TRAIL: u32 = 3;
    /// The part of an eel that swims; its state is the eel's colour.
    pub const HEAD: u32 = 4;
}

/// Terrain generators are assigned to cubes of 8 bricks per side.
const ASSIGNMENT_CUBE_LEVEL: u8 = 3;

/// The colours eels come in, by slot (sRGB), and what they are called.
pub const SLOT_COLORS: [[f32; 3]; 8] = [
    [0.90, 0.20, 0.20],
    [0.95, 0.60, 0.10],
    [0.95, 0.90, 0.20],
    [0.25, 0.80, 0.30],
    [0.20, 0.85, 0.85],
    [0.25, 0.45, 0.95],
    [0.65, 0.35, 0.90],
    [0.95, 0.45, 0.75],
];

pub const SLOT_NAMES: [&str; 8] = [
    "red", "orange", "yellow", "green", "cyan", "blue", "purple", "pink",
];

fn identifier(text: &str) -> NamespacedId {
    text.parse().expect("constant identifiers are valid")
}

pub fn game_id() -> NamespacedId {
    identifier("voxunami_eels:game")
}

/// The level players are in, and the only one a new world has.
pub fn arena_level_id() -> NamespacedId {
    identifier("voxunami_eels:arena")
}

pub fn block_registry_kind() -> NamespacedId {
    identifier("voxunami_eels:block")
}

/// The grid channel holding each voxel's block.
pub fn block_channel_identifier() -> NamespacedId {
    identifier("voxunami_eels:block")
}

/// The grid channel holding each voxel's eel colour, as the state of its block.
pub fn slot_channel_identifier() -> NamespacedId {
    identifier("voxunami_eels:slot")
}

pub fn arena_generator_identifier() -> NamespacedId {
    identifier("voxunami_eels:arena_v1")
}

/// A colour brightened toward white, for the part of an eel that swims.
fn brighter(color: [f32; 3]) -> [f32; 3] {
    color.map(|channel| channel + (1.0 - channel) * 0.55)
}

/// The blocks, registered in a fixed order (see [`blocks`]), declared as a package.
#[derive(Debug)]
pub struct ContentEels {
    package: ContentPackage,
}

impl Default for ContentEels {
    fn default() -> Self {
        Self::new()
    }
}

impl ContentEels {
    pub fn new() -> Self {
        let mut block_definitions = Registry::new();
        let mut register = |name: &str, definition: BlockDefinition, expected: u32| {
            let registered = block_definitions
                .register(identifier(&format!("voxunami_eels:{name}")), definition)
                .expect("the block names are distinct");
            assert_eq!(registered.raw(), expected);
        };
        register("air", BlockDefinition::air(), blocks::AIR);
        register(
            "wall",
            BlockDefinition::cube([0.10, 0.12, 0.22]),
            blocks::WALL,
        );
        register(
            "wall_line",
            BlockDefinition::cube([0.25, 0.75, 0.95]),
            blocks::WALL_LINE,
        );
        register(
            "trail",
            BlockDefinition::cube(SLOT_COLORS[0]).with_state_colors(SLOT_COLORS.to_vec()),
            blocks::TRAIL,
        );
        register(
            "head",
            BlockDefinition::cube(brighter(SLOT_COLORS[0]))
                .with_state_colors(SLOT_COLORS.map(brighter).to_vec()),
            blocks::HEAD,
        );
        let spec = PackageSpec {
            level: arena_level_id(),
            block_registry_kind: block_registry_kind(),
            channels: vec![
                ChannelDefinition::content(
                    block_channel_identifier(),
                    blocks::AIR,
                    block_registry_kind(),
                ),
                ChannelDefinition::number(slot_channel_identifier(), 0),
            ],
            block_channel: block_channel_identifier(),
            state_channel: Some(slot_channel_identifier()),
            light_channel: None,
            properties: Vec::new(),
            generators: Arc::new(|_seed, layout| {
                let block_channel = layout
                    .channel_index(&block_channel_identifier())
                    .expect("the layout has the block channel");
                let mut generators: Registry<Arc<dyn TerrainGenerator>> = Registry::new();
                generators
                    .register(
                        arena_generator_identifier(),
                        Arc::new(ArenaWalls::new(block_channel)),
                    )
                    .expect("the registry is empty");
                generators
            }),
            default_generator: arena_generator_identifier(),
            assignment_cube_level: ASSIGNMENT_CUBE_LEVEL,
            ambient_light: 0,
        };
        Self {
            package: ContentPackage::new(block_definitions, spec),
        }
    }

    /// The channel holding each voxel's block.
    pub fn block_channel(&self) -> ChannelIndex {
        self.package.block_channel()
    }

    /// The channel holding each voxel's eel colour.
    pub fn slot_channel(&self) -> ChannelIndex {
        self.package.state_channel()
    }
}

impl GameContent for ContentEels {
    fn view_channels(&self) -> ViewChannels {
        self.package.view_channels()
    }

    fn package_data(&self) -> PackageData {
        self.package.package_data()
    }

    fn apply_packs(&mut self, packs: &[RawPack]) -> Result<Vec<String>, PackErrors> {
        self.package.apply_packs(packs)
    }

    /// A new world: the arena, with nobody's body in it yet. The seed changes nothing.
    fn new_world(&self, seed: u64) -> WorldData {
        self.package.new_world(seed)
    }

    fn load_context(&mut self, seed: u64) -> LoadContext<'_> {
        self.package.load_context(seed)
    }

    fn registries(&self) -> Vec<(NamespacedId, &dyn IdentifierRegistry)> {
        self.package.registries()
    }
}

#[cfg(test)]
mod tests {
    use world_storage::ParsedWorld;

    use super::*;

    #[test]
    fn blocks_are_registered_in_the_order_the_constants_say() {
        let content = ContentEels::new();
        let name = |raw: u32| {
            let registries = content.registries();
            let registry = registries[0].1;
            registry.identifier_of_raw(raw).unwrap().to_string()
        };
        assert_eq!(name(blocks::AIR), "voxunami_eels:air");
        assert_eq!(name(blocks::WALL), "voxunami_eels:wall");
        assert_eq!(name(blocks::WALL_LINE), "voxunami_eels:wall_line");
        assert_eq!(name(blocks::TRAIL), "voxunami_eels:trail");
        assert_eq!(name(blocks::HEAD), "voxunami_eels:head");
    }

    #[test]
    fn every_eel_colour_is_drawn_for_bodies_and_for_heads_and_they_all_differ() {
        let table = ContentEels::new().block_table();
        assert_eq!(table.block_count(), 5);
        let palette = table.palette();
        let mut seen = Vec::new();
        for block in [blocks::TRAIL, blocks::HEAD] {
            for slot in 0..8u32 {
                assert!(table.is_opaque_cube(table.model_of(block, slot)));
                let drawn = palette[table.material_of(block, slot) as usize];
                assert!(!seen.contains(&drawn), "block {block} slot {slot}");
                seen.push(drawn);
            }
        }
        // A head is the lighter version of the same colour.
        for slot in 0..8u32 {
            let body = palette[table.material_of(blocks::TRAIL, slot) as usize];
            let head = palette[table.material_of(blocks::HEAD, slot) as usize];
            assert_eq!(body, SLOT_COLORS[slot as usize]);
            assert!(head.iter().zip(body).all(|(head, body)| *head >= body));
        }
        assert_eq!(SLOT_NAMES.len(), SLOT_COLORS.len());
    }

    #[test]
    fn a_new_world_has_nothing_edited_and_saves_and_loads_back_the_same() {
        let content = ContentEels::new();
        let world = content.new_world(5);
        assert_eq!(world.main().overrides.brick_count(), 0);
        let bytes = world.save(&content.registries()).unwrap();
        let mut later = ContentEels::new();
        let parsed = ParsedWorld::parse(&bytes).unwrap();
        let seed = parsed.world_seed();
        let (loaded, report) = parsed.load(later.load_context(seed)).unwrap();
        assert!(report.missing_content.is_empty() && report.extra_channels.is_empty());
        assert_eq!(loaded.save(&later.registries()).unwrap(), bytes);
    }
}
