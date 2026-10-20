//! Items, the things dropped on the ground, and the game modes of Voxunami Cave.
//!
//! Every block a player can place has an item of the same identifier (`base_blocks:stone` the item puts
//! down `base_blocks:stone` the block). In survival a broken block leaves its item on the ground as an
//! entity, the item falls a voxel at a time (whole voxels, so nothing here is floating point arithmetic
//! beyond placing it in the middle of a voxel), and comes into the inventory of a player who gets close.
//! In creative nothing is dropped or used up.

use entities::{EntityKindSpec, ItemSpec, ItemStack};
use namespaced_id::NamespacedId;
use voxel_space::{EntityPose, GridId, Orientation, VoxelCoordinate, WorldPosition};
use world_storage::Section;

use crate::game::PLACEABLE_BLOCKS;

/// How many of an item fit in a stack.
pub const STACK_SIZE: u32 = 64;

/// A dropped item falls a voxel every this many ticks, while there is nothing under it.
pub const FALL_INTERVAL_TICKS: u64 = 4;

/// How close, in voxels, a player's eyes must be to an item for it to come into their inventory.
pub const PICKUP_RADIUS: f32 = 3.0;

/// An item that has fallen below this height is gone (it fell out of the world).
pub const LOWEST_DROP_VOXEL: i64 = -1000;

fn identifier(text: &str) -> NamespacedId {
    text.parse().expect("constant identifier is valid")
}

/// The items there are: one for each block that can be placed.
pub fn item_specs() -> Vec<ItemSpec> {
    PLACEABLE_BLOCKS
        .iter()
        .map(|block| ItemSpec::new(block, STACK_SIZE))
        .collect()
}

/// The item a block leaves when it is broken, which is also the item that puts the block down: `None` for a
/// block no player can place.
pub fn item_for_block(block: &NamespacedId) -> Option<NamespacedId> {
    PLACEABLE_BLOCKS
        .iter()
        .any(|placeable| *placeable == block.as_str())
        .then(|| block.clone())
}

/// The kind of entity an item lying on the ground is.
pub fn dropped_item_kind() -> NamespacedId {
    identifier("voxunami_cave:dropped_item")
}

/// What a dropped item is made of: the stack it holds, which clients are told (so that they can draw it).
pub fn dropped_item_kind_spec() -> EntityKindSpec {
    EntityKindSpec::new(dropped_item_kind().as_str())
        .component(ItemStack::new(identifier(PLACEABLE_BLOCKS[0]), 1), true)
}

/// Whether an item on the ground falls in this tick.
pub fn falls_this_tick(tick: u64) -> bool {
    tick.is_multiple_of(FALL_INTERVAL_TICKS)
}

/// The voxel a position is in.
pub fn voxel_of(position: WorldPosition) -> VoxelCoordinate {
    let brick = position.brick();
    let offset = position.offset();
    VoxelCoordinate::new(
        brick.x() * 32 + offset[0].floor() as i64,
        brick.y() * 32 + offset[1].floor() as i64,
        brick.z() * 32 + offset[2].floor() as i64,
    )
}

/// The middle of a voxel.
pub fn centre_of(voxel: VoxelCoordinate) -> WorldPosition {
    let (brick, local) = voxel.split();
    WorldPosition::new(
        brick,
        [
            f32::from(local.x()) + 0.5,
            f32::from(local.y()) + 0.5,
            f32::from(local.z()) + 0.5,
        ],
    )
}

/// The pose of an item lying in a voxel of a grid.
pub fn drop_pose(grid: GridId, voxel: VoxelCoordinate) -> EntityPose {
    EntityPose::new(grid, centre_of(voxel), Orientation::IDENTITY)
}

/// What players can do in a world: build with all there is (creative), or have only what they have gathered
/// (survival).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameMode {
    Creative,
    Survival,
}

impl GameMode {
    pub fn name(self) -> &'static str {
        match self {
            Self::Creative => "creative",
            Self::Survival => "survival",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "creative" => Some(Self::Creative),
            "survival" => Some(Self::Survival),
            _ => None,
        }
    }
}

/// Worlds are creative until someone says otherwise.
impl Default for GameMode {
    fn default() -> Self {
        Self::Creative
    }
}

const GAME_MODE_SECTION_VERSION: u32 = 1;

fn game_mode_section_id() -> NamespacedId {
    identifier("voxunami_cave:game_mode")
}

/// The section a world saves its game mode in: one byte, 0 for creative and 1 for survival.
pub fn game_mode_section(mode: GameMode) -> Section {
    Section {
        identifier: game_mode_section_id(),
        version: GAME_MODE_SECTION_VERSION,
        payload: vec![match mode {
            GameMode::Creative => 0,
            GameMode::Survival => 1,
        }],
    }
}

/// The game mode a world was saved with, if it has one this program can read. A section of a newer version, or
/// a damaged one, counts as none (and is left in the file).
pub fn read_game_mode(sections: &[Section]) -> Option<GameMode> {
    let section = sections.iter().find(|section| {
        section.identifier == game_mode_section_id() && section.version == GAME_MODE_SECTION_VERSION
    })?;
    match section.payload[..] {
        [0] => Some(GameMode::Creative),
        [1] => Some(GameMode::Survival),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_is_an_item_for_each_block_that_can_be_placed_and_for_no_other() {
        let specs = item_specs();
        assert_eq!(specs.len(), PLACEABLE_BLOCKS.len());
        for block in PLACEABLE_BLOCKS {
            let block = identifier(block);
            assert_eq!(item_for_block(&block), Some(block.clone()));
            assert!(specs.iter().any(|spec| *spec.id() == block));
        }
        assert_eq!(item_for_block(&identifier("base_blocks:air")), None);
        assert_eq!(item_for_block(&identifier("other_game:stone")), None);
        assert!(specs.iter().all(|spec| spec.max_stack() == STACK_SIZE));
    }

    #[test]
    fn an_item_lies_in_the_middle_of_the_voxel_it_is_in_for_negative_voxels_too() {
        for voxel in [
            VoxelCoordinate::new(0, 0, 0),
            VoxelCoordinate::new(31, 32, 33),
            VoxelCoordinate::new(-1, -1, -1),
            VoxelCoordinate::new(-32, -33, 95),
            VoxelCoordinate::new(1_000_000, -7, -1_000_001),
        ] {
            let centre = centre_of(voxel);
            assert_eq!(voxel_of(centre), voxel, "{voxel:?}");
            for offset in centre.offset() {
                assert_eq!(offset.fract(), 0.5);
            }
        }
    }

    #[test]
    fn items_fall_on_every_fourth_tick() {
        let ticks: Vec<u64> = (0..13).filter(|tick| falls_this_tick(*tick)).collect();
        assert_eq!(ticks, [0, 4, 8, 12]);
    }

    #[test]
    fn the_game_mode_names_parse_and_a_world_is_creative_unless_it_says_otherwise() {
        for mode in [GameMode::Creative, GameMode::Survival] {
            assert_eq!(GameMode::parse(mode.name()), Some(mode));
        }
        assert_eq!(GameMode::parse("adventure"), None);
        assert_eq!(GameMode::parse("Survival"), None);
        assert_eq!(GameMode::default(), GameMode::Creative);
    }

    #[test]
    fn the_game_mode_survives_a_save_and_a_section_this_program_cannot_read_counts_as_none() {
        for mode in [GameMode::Creative, GameMode::Survival] {
            assert_eq!(read_game_mode(&[game_mode_section(mode)]), Some(mode));
        }
        assert_eq!(read_game_mode(&[]), None);
        let section = game_mode_section(GameMode::Survival);
        let newer = Section {
            version: 2,
            ..section.clone()
        };
        let damaged = Section {
            payload: vec![7],
            ..section.clone()
        };
        let long = Section {
            payload: vec![1, 1],
            ..section.clone()
        };
        let empty = Section {
            payload: Vec::new(),
            ..section
        };
        for unreadable in [newer, damaged, long, empty] {
            assert_eq!(read_game_mode(&[unreadable]), None);
        }
        // Among other sections.
        let other = Section {
            identifier: identifier("voxunami_cave:something_else"),
            version: 1,
            payload: vec![0],
        };
        assert_eq!(
            read_game_mode(&[other, game_mode_section(GameMode::Survival)]),
            Some(GameMode::Survival)
        );
    }

    #[test]
    fn the_dropped_item_kind_tells_clients_what_it_holds() {
        let spec = dropped_item_kind_spec();
        assert_eq!(*spec.id(), dropped_item_kind());
        let [held] = spec.components() else {
            panic!("{:?}", spec.components());
        };
        assert!(held.replicated);
    }
}
