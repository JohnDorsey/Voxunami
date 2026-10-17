//! A spike: block light flooded at four scales at once, in cells of 1, 2, 4 and 8 voxels along each
//! side, each with 16 levels. It is the light rule of [`crate::lighting`] with the cell size as a
//! parameter, to see what light at several scales in one world is like (what it costs, how it
//! looks, whether the rule's shape holds), not a decision about how Cave is lit.
//!
//! At scale `k` the world is a grid of cells of `2^k` voxels along each side. A cell is named by
//! its anchor, its first voxel (the one with the smallest coordinates), and a level 0 to 15 is kept
//! in the anchor voxel of its cell, in a channel of its own for each scale. The rule for a cell is the
//! one the single-voxel rule has, from the cells as they were at the start of the tick:
//!
//! - A cell is *blocked* if more than half of its voxels are whole blocks that let no light through
//!   (or cannot be read). A blocked cell holds no light, but gives its source to its neighbours,
//!   as a lamp does. So a wall one voxel thick stops light at scale 0, and does not at scale 1,
//!   where it is half of a cell: coarse light leaks through thin walls and does not enter
//!   caves narrower than the cell.
//! - A cell's *source* is the brightest light given off by any voxel in it, the same at every scale:
//!   a block's emission divided by [`EMISSION_PER_LEVEL`], rounded up, at most 15. A lamp (64) is 15
//!   at every scale, a fire (40) is 10, and a cell is as bright as the brightest thing in it, however
//!   big the cell.
//! - Otherwise a cell holds its source, or one less than the brightest neighbouring cell (of the six
//!   that share a face), if that is more. Light travels one cell a tick, so coarse light spreads
//!   faster in metres: at scale 3, 8 voxels a tick.
//!
//! What is drawn is the brightest level that reaches each voxel, of every scale (see
//! `world_view::combined_light`).

use std::collections::HashMap;

use block_models::BlockTable;
use lattice::ActiveVoxels;
use namespaced_id::NamespacedId;
use voxel_space::{FaceDirection, LevelIndex, VoxelCoordinate};
use world_storage::Section;

/// How many scales there are: cells of 1, 2, 4 and 8 voxels.
pub const SCALES: usize = 4;

/// The brightest a cell's light can be.
pub const MAXIMUM_LEVEL: u8 = 15;

/// What a block's emission is divided by to give the level of the light it gives off, at every scale.
pub const EMISSION_PER_LEVEL: u8 = 4;

/// The level of the light a block of this emission gives off: 15 for a lamp (64), 10 for a fire (40),
/// at most 15, and 0 for a block that gives off none.
pub fn emission_level(emission: u8) -> u8 {
    emission.div_ceil(EMISSION_PER_LEVEL).min(MAXIMUM_LEVEL)
}

/// Which light a world has. Switching clears the light of the other (see `CaveLogic`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightModel {
    /// One light channel, a voxel a cell with 255 levels (see [`crate::lighting`]).
    Classic,
    /// Four scales, cells of 1, 2, 4 and 8 voxels with 16 levels each (this module).
    MultiScale,
}

impl LightModel {
    pub fn name(self) -> &'static str {
        match self {
            Self::Classic => "classic",
            Self::MultiScale => "multiscale",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "classic" => Some(Self::Classic),
            "multiscale" => Some(Self::MultiScale),
            _ => None,
        }
    }

    /// The section a world's choice is saved in.
    pub fn section_id() -> NamespacedId {
        "voxunami_cave:light_model"
            .parse()
            .expect("constant identifier is valid")
    }

    pub fn to_section(self) -> Section {
        Section {
            identifier: Self::section_id(),
            version: 1,
            payload: vec![self as u8],
        }
    }

    /// The choice saved in a world's sections: classic if there is none, or one this program cannot
    /// read.
    pub fn read_section(sections: &[Section]) -> Self {
        sections
            .iter()
            .find(|section| section.identifier == Self::section_id() && section.version == 1)
            .and_then(|section| match section.payload[..] {
                [0] => Some(Self::Classic),
                [1] => Some(Self::MultiScale),
                _ => None,
            })
            .unwrap_or(Self::Classic)
    }
}

/// The identifier of the section the cells still to be looked at are saved in, at a scale.
pub fn section_id(scale: usize) -> NamespacedId {
    format!("voxunami_cave:light_scale_{scale}")
        .parse()
        .expect("constant identifier is valid")
}

/// The identifier of the channel that holds the levels of a scale.
pub fn channel_identifier(scale: usize) -> NamespacedId {
    format!("voxunami_cave:light_scale_{scale}")
        .parse()
        .expect("constant identifier is valid")
}

/// The anchor of the cell of a scale that holds a voxel.
pub fn anchor_of(scale: usize, voxel: VoxelCoordinate) -> VoxelCoordinate {
    let mask = !((1i64 << scale) - 1);
    VoxelCoordinate::new(voxel.x() & mask, voxel.y() & mask, voxel.z() & mask)
}

/// The anchor of the cell next to a cell on `face`, if it is within the world's range.
fn neighbour_anchor(
    scale: usize,
    anchor: VoxelCoordinate,
    face: FaceDirection,
) -> Option<VoxelCoordinate> {
    let [dx, dy, dz] = face.offset();
    let width = 1i64 << scale;
    anchor.checked_offset(dx * width, dy * width, dz * width)
}

/// The voxels of a world, as far as light needs to know.
pub trait ScaledLightWorld {
    /// The block and state of a voxel, or `None` where there is nothing to read.
    fn block_and_state(&mut self, level: LevelIndex, at: VoxelCoordinate) -> Option<(u32, u32)>;

    /// The level stored at the anchor of a cell of a scale, 0 to 15; 0 where there is nothing to read.
    fn level(&mut self, scale: usize, level: LevelIndex, anchor: VoxelCoordinate) -> u8;

    /// Stores the level of a cell. Only called for cells whose anchor was just read.
    fn set_level(&mut self, scale: usize, level: LevelIndex, anchor: VoxelCoordinate, value: u8);
}

/// What is in a cell, as far as light goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Contents {
    blocked: bool,
    source: u8,
}

type CellKey = (usize, LevelIndex, VoxelCoordinate);

/// The light rule at every scale, and the cells it has yet to look at.
#[derive(Debug)]
pub struct MultiScaleLighting {
    table: BlockTable,
    /// Cells (by anchor) whose light may be out of date, at each scale, in a fixed order.
    pending: [ActiveVoxels; SCALES],
    /// What is in the cells looked into so far; a cell is forgotten when a block in it changes.
    contents_known: HashMap<CellKey, Contents>,
}

impl MultiScaleLighting {
    /// Light for a world of these blocks, with nothing to look at: the world's light is taken to be
    /// settled.
    pub fn new(table: BlockTable) -> Self {
        Self {
            table,
            pending: std::array::from_fn(|_| ActiveVoxels::new()),
            contents_known: HashMap::new(),
        }
    }

    /// Light that carries on from cells that were still to be looked at, as when a world is loaded.
    pub fn with_pending(table: BlockTable, pending: [ActiveVoxels; SCALES]) -> Self {
        Self {
            table,
            pending,
            contents_known: HashMap::new(),
        }
    }

    /// The cells (their anchors) still to be looked at, at a scale.
    pub fn pending(&self, scale: usize) -> &ActiveVoxels {
        &self.pending[scale]
    }

    /// Whether every cell's light is what the rule says it should be.
    pub fn is_settled(&self) -> bool {
        self.pending.iter().all(ActiveVoxels::is_empty)
    }

    /// Tells the rule that the block or state at a voxel changed from outside, so that the cells that
    /// hold it, at every scale, and their neighbours are looked at on the next step.
    pub fn voxel_changed(&mut self, level: LevelIndex, at: VoxelCoordinate) {
        for scale in 0..SCALES {
            self.contents_known
                .remove(&(scale, level, anchor_of(scale, at)));
            self.cell_changed(scale, level, anchor_of(scale, at));
        }
    }

    fn cell_changed(&mut self, scale: usize, level: LevelIndex, anchor: VoxelCoordinate) {
        self.pending[scale].insert(level, anchor);
        for face in FaceDirection::ALL {
            if let Some(neighbour) = neighbour_anchor(scale, anchor, face) {
                self.pending[scale].insert(level, neighbour);
            }
        }
    }

    /// One tick, with no limit on the work.
    pub fn step(&mut self, world: &mut impl ScaledLightWorld) -> usize {
        self.step_within(world, [usize::MAX; SCALES])
    }

    /// One tick: at each scale, works out the level of every pending cell (no more than that scale's
    /// share of `budgets`, in their fixed order; the rest wait) from the world as it is now, then
    /// writes the ones that changed, and so has the cells around them to look at next time. Returns
    /// how many cells changed. Coarse cells cost more to look at (a cell of 8 reads 512 voxels), so
    /// they get smaller budgets.
    pub fn step_within(
        &mut self,
        world: &mut impl ScaledLightWorld,
        budgets: [usize; SCALES],
    ) -> usize {
        // What is in a cell changes only when a block in it does (see `voxel_changed`), so it is kept
        // from one tick to the next: a cell of 8 costs 512 reads to look into.
        let mut memo = std::mem::take(&mut self.contents_known);
        let mut changed_in_all = 0;
        for (scale, &budget) in budgets.iter().enumerate() {
            let mut looked_at = self.pending[scale].take();
            if looked_at.len() > budget {
                for (level, anchor) in looked_at.split_off(budget) {
                    self.pending[scale].insert(level, anchor);
                }
            }
            let changes: Vec<_> = looked_at
                .into_iter()
                .filter_map(|(level, anchor)| {
                    let new = self.level_of(world, &mut memo, scale, level, anchor);
                    (new != world.level(scale, level, anchor)).then_some((level, anchor, new))
                })
                .collect();
            for &(level, anchor, new) in &changes {
                world.set_level(scale, level, anchor, new);
                self.cell_changed(scale, level, anchor);
            }
            changed_in_all += changes.len();
        }
        self.contents_known = memo;
        changed_in_all
    }

    /// What is in a cell: whether it is blocked, and how bright its own light is.
    fn contents(
        &self,
        world: &mut impl ScaledLightWorld,
        memo: &mut HashMap<CellKey, Contents>,
        scale: usize,
        level: LevelIndex,
        anchor: VoxelCoordinate,
    ) -> Contents {
        if let Some(&known) = memo.get(&(scale, level, anchor)) {
            return known;
        }
        let width = 1i64 << scale;
        let (mut opaque, mut source) = (0u32, 0u8);
        for dx in 0..width {
            for dy in 0..width {
                for dz in 0..width {
                    let read = anchor
                        .checked_offset(dx, dy, dz)
                        .and_then(|at| world.block_and_state(level, at));
                    match read {
                        // What cannot be read lets no light through.
                        None => opaque += 1,
                        Some((block, state)) => {
                            let connections =
                                self.table.connections(self.table.model_of(block, state));
                            if !FaceDirection::ALL
                                .iter()
                                .any(|&face| connections.is_open(face))
                            {
                                opaque += 1;
                            }
                            source = source.max(emission_level(self.table.emission(block)));
                        }
                    }
                }
            }
        }
        let volume = (width * width * width) as u32;
        let contents = Contents {
            blocked: opaque * 2 > volume,
            source,
        };
        memo.insert((scale, level, anchor), contents);
        contents
    }

    /// The level a cell should hold, given its neighbours as they are now.
    fn level_of(
        &self,
        world: &mut impl ScaledLightWorld,
        memo: &mut HashMap<CellKey, Contents>,
        scale: usize,
        level: LevelIndex,
        anchor: VoxelCoordinate,
    ) -> u8 {
        let own = self.contents(world, memo, scale, level, anchor);
        if own.blocked {
            return 0; // a blocked cell holds no light, though it may give some
        }
        let mut value = own.source;
        for face in FaceDirection::ALL {
            let Some(neighbour) = neighbour_anchor(scale, anchor, face) else {
                continue;
            };
            let next = self.contents(world, memo, scale, level, neighbour);
            let given = if next.blocked {
                next.source
            } else {
                world.level(scale, level, neighbour).max(next.source)
            };
            value = value.max(given.saturating_sub(1));
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use block_models::BlockDefinition;
    use proptest::prelude::*;

    use super::*;

    const AIR: u32 = 0;
    const STONE: u32 = 1;
    /// A whole block that lets light through.
    const GLASS: u32 = 2;
    /// A whole lamp of 64, the lamp of the game.
    const LAMP: u32 = 3;
    /// A whole lamp of 16.
    const CANDLE: u32 = 4;

    /// Voxels along each side of the box the tests are in: a whole number of cells of the largest
    /// scale, so no cell is part in and part out of it.
    const EDGE: i64 = 16;

    fn table() -> BlockTable {
        let air = BlockDefinition::air();
        let stone = BlockDefinition::cube([0.5; 3]);
        let glass = BlockDefinition::cube([0.7; 3]).with_see_through(true);
        let lamp = BlockDefinition::cube([1.0; 3]).with_emission(64);
        let candle = BlockDefinition::cube([1.0; 3]).with_emission(16);
        BlockTable::from_definitions(&[
            Some(&air),
            Some(&stone),
            Some(&glass),
            Some(&lamp),
            Some(&candle),
        ])
    }

    const MAIN: LevelIndex = LevelIndex::MAIN;

    /// The offsets of the eight voxels of a cell of 2.
    const CELL_OF_EIGHT: [(i64, i64, i64); 8] = [
        (0, 0, 0),
        (1, 0, 0),
        (0, 1, 0),
        (1, 1, 0),
        (0, 0, 1),
        (1, 0, 1),
        (0, 1, 1),
        (1, 1, 1),
    ];

    fn at(x: i64, y: i64, z: i64) -> VoxelCoordinate {
        VoxelCoordinate::new(x, y, z)
    }

    fn inside(at: VoxelCoordinate) -> bool {
        [at.x(), at.y(), at.z()]
            .iter()
            .all(|component| (0..EDGE).contains(component))
    }

    /// A box of `EDGE` voxels along each side in which everything not set is air; outside it there is
    /// nothing to read.
    #[derive(Default)]
    struct Box3 {
        blocks: HashMap<VoxelCoordinate, u32>,
        levels: [HashMap<VoxelCoordinate, u8>; SCALES],
    }

    impl ScaledLightWorld for Box3 {
        fn block_and_state(&mut self, _: LevelIndex, at: VoxelCoordinate) -> Option<(u32, u32)> {
            inside(at).then(|| (self.blocks.get(&at).copied().unwrap_or(AIR), 0))
        }

        fn level(&mut self, scale: usize, _: LevelIndex, anchor: VoxelCoordinate) -> u8 {
            self.levels[scale].get(&anchor).copied().unwrap_or(0)
        }

        fn set_level(&mut self, scale: usize, _: LevelIndex, anchor: VoxelCoordinate, value: u8) {
            self.levels[scale].insert(anchor, value);
        }
    }

    impl Box3 {
        fn place(&mut self, voxel: VoxelCoordinate, block: u32, lighting: &mut MultiScaleLighting) {
            self.blocks.insert(voxel, block);
            lighting.voxel_changed(MAIN, voxel);
        }

        fn level_at(&self, scale: usize, x: i64, y: i64, z: i64) -> u8 {
            self.levels[scale].get(&at(x, y, z)).copied().unwrap_or(0)
        }

        /// The cells of a scale that hold any light, with their levels.
        fn lit(&self, scale: usize) -> Vec<(VoxelCoordinate, u8)> {
            let mut lit: Vec<_> = self.levels[scale]
                .iter()
                .filter(|(_, level)| **level > 0)
                .map(|(&anchor, &level)| (anchor, level))
                .collect();
            lit.sort();
            lit
        }
    }

    /// Steps until nothing is pending, returning how many steps it took.
    fn settle(lighting: &mut MultiScaleLighting, world: &mut Box3) -> usize {
        for steps in 1..=500 {
            lighting.step(world);
            if lighting.is_settled() {
                return steps;
            }
        }
        panic!("the light never settled");
    }

    /// What the rule should settle to, worked out the other way round: the contents of every cell
    /// counted from scratch, then the brightest light carried to every cell by search from the
    /// sources, taking the brightest first.
    fn reference(world: &Box3, table: &BlockTable, scale: usize) -> Vec<(VoxelCoordinate, u8)> {
        let width = 1i64 << scale;
        let cells = EDGE / width;
        let mut blocked = HashMap::new();
        let mut source = HashMap::new();
        for cx in 0..cells {
            for cy in 0..cells {
                for cz in 0..cells {
                    let anchor = at(cx * width, cy * width, cz * width);
                    let (mut opaque, mut level) = (0i64, 0u8);
                    for dx in 0..width {
                        for dy in 0..width {
                            for dz in 0..width {
                                let block = world
                                    .blocks
                                    .get(&at(anchor.x() + dx, anchor.y() + dy, anchor.z() + dz))
                                    .copied()
                                    .unwrap_or(AIR);
                                let model = table.model_of(block, 0);
                                let closed = !FaceDirection::ALL
                                    .iter()
                                    .any(|&face| table.connections(model).is_open(face));
                                opaque += i64::from(closed);
                                level = level.max(emission_level(table.emission(block)));
                            }
                        }
                    }
                    blocked.insert(anchor, opaque * 2 > width * width * width);
                    source.insert(anchor, level);
                }
            }
        }
        // Light carried from cell to cell, one less each time; a blocked cell gives its source and
        // holds none.
        let mut best: HashMap<VoxelCoordinate, u8> = HashMap::new();
        let mut changed = true;
        while changed {
            changed = false;
            for (&anchor, &is_blocked) in &blocked {
                if is_blocked {
                    continue;
                }
                let mut value = source[&anchor];
                for face in FaceDirection::ALL {
                    let Some(next) = neighbour_anchor(scale, anchor, face) else {
                        continue;
                    };
                    let Some(&next_blocked) = blocked.get(&next) else {
                        continue; // outside the box
                    };
                    let given = if next_blocked {
                        source[&next]
                    } else {
                        best.get(&next).copied().unwrap_or(0).max(source[&next])
                    };
                    value = value.max(given.saturating_sub(1));
                }
                if best.get(&anchor).copied().unwrap_or(0) != value {
                    best.insert(anchor, value);
                    changed = true;
                }
            }
        }
        let mut lit: Vec<_> = best.into_iter().filter(|&(_, level)| level > 0).collect();
        lit.sort();
        lit
    }

    #[test]
    fn a_cell_is_named_by_its_first_voxel_even_where_coordinates_are_negative() {
        assert_eq!(anchor_of(0, at(5, -3, 9)), at(5, -3, 9));
        assert_eq!(anchor_of(1, at(5, -3, 9)), at(4, -4, 8));
        assert_eq!(anchor_of(2, at(5, -3, 9)), at(4, -4, 8));
        assert_eq!(anchor_of(3, at(5, -3, 9)), at(0, -8, 8));
        assert_eq!(anchor_of(3, at(-1, -8, -9)), at(-8, -8, -16));
    }

    #[test]
    fn a_change_wakes_the_cell_that_holds_it_and_its_six_neighbours_at_every_scale() {
        let mut lighting = MultiScaleLighting::new(table());
        lighting.voxel_changed(MAIN, at(-1, 9, 5));
        for scale in 0..SCALES {
            let width = 1i64 << scale;
            let anchor = anchor_of(scale, at(-1, 9, 5));
            let mut expected = vec![anchor];
            for face in FaceDirection::ALL {
                let [dx, dy, dz] = face.offset();
                expected.push(at(
                    anchor.x() + dx * width,
                    anchor.y() + dy * width,
                    anchor.z() + dz * width,
                ));
            }
            expected.sort();
            let mut woken: Vec<_> = lighting.pending(scale).clone().take();
            woken.sort();
            assert_eq!(
                woken.iter().map(|&(_, voxel)| voxel).collect::<Vec<_>>(),
                expected,
                "scale {scale}"
            );
        }
    }

    #[test]
    fn one_lamp_is_a_source_of_15_at_every_scale() {
        let mut world = Box3::default();
        let mut lighting = MultiScaleLighting::new(table());
        world.place(at(8, 8, 8), LAMP, &mut lighting);
        settle(&mut lighting, &mut world);
        // Scale 0: the lamp is a whole block, so it holds no light and gives 15 to what is around it:
        // 14 beside it, one less for each voxel further, 8 at the far side of the box.
        assert_eq!(world.level_at(0, 8, 8, 8), 0);
        assert_eq!(world.level_at(0, 9, 8, 8), 14);
        assert_eq!(world.level_at(0, 10, 8, 8), 13);
        assert_eq!(world.level_at(0, 15, 8, 8), 8);
        // Scales 1 to 3: the cell with the lamp in it is not blocked, so it holds 15, and the cells
        // beyond it one less for each cell.
        assert_eq!(world.level_at(1, 8, 8, 8), 15);
        assert_eq!(world.level_at(1, 10, 8, 8), 14);
        assert_eq!(world.level_at(1, 14, 8, 8), 12);
        assert_eq!(world.level_at(1, 6, 8, 8), 14);
        // At scale 3 the box is eight cells (2 by 2 by 2) and the lamp is in the one at (8, 8, 8): a level
        // less for each cell between, 15 minus the number of cells away.
        let mut expected = Vec::new();
        for (x, y, z) in [
            (0, 0, 0),
            (0, 0, 8),
            (0, 8, 0),
            (0, 8, 8),
            (8, 0, 0),
            (8, 0, 8),
            (8, 8, 0),
            (8, 8, 8),
        ] {
            let cells_away = (8 - x) / 8 + (8 - y) / 8 + (8 - z) / 8;
            expected.push((at(x, y, z), 15 - cells_away as u8));
        }
        assert_eq!(world.lit(3), expected);
        // The same lamp is 15 at every scale, so none of them is dimmer.
        assert!((0..SCALES).all(|scale| world.level_at(scale, 9 - (9 % (1 << scale)), 8, 8) >= 14));
    }

    #[test]
    fn a_cell_gives_the_light_of_its_own_source_to_its_neighbours_the_tick_it_is_evaluated() {
        // A cell that is lit by what is in it, rather than by a blocked lamp, gives that to its neighbour in
        // the same tick that it first holds it, as a lamp (which holds none) does at once.
        let mut world = Box3::default();
        let mut lighting = MultiScaleLighting::new(table());
        world.place(at(8, 8, 8), LAMP, &mut lighting);
        lighting.step(&mut world);
        assert_eq!(world.level_at(1, 8, 8, 8), 15, "the source of the cell");
        assert_eq!(world.level_at(1, 10, 8, 8), 14, "given at once to the next");
    }

    #[test]
    fn a_full_cell_of_lamps_is_as_bright_as_a_cell_can_be_and_gives_it_to_the_cells_around() {
        let mut world = Box3::default();
        let mut lighting = MultiScaleLighting::new(table());
        for (dx, dy, dz) in CELL_OF_EIGHT {
            world.place(at(8 + dx, 8 + dy, 8 + dz), LAMP, &mut lighting);
        }
        settle(&mut lighting, &mut world);
        // At scale 1 the cell is all lamps, so blocked and holding none, but it gives 15.
        assert_eq!(world.level_at(1, 8, 8, 8), 0);
        assert_eq!(world.level_at(1, 10, 8, 8), 14);
        assert_eq!(world.level_at(1, 6, 8, 8), 14);
        // At scale 3 they are a small part of the cell, which is not blocked: it holds 15, as many
        // lamps as one.
        assert_eq!(world.level_at(3, 8, 8, 8), 15);
    }

    #[test]
    fn a_wall_one_voxel_thick_stops_the_finest_light_and_not_the_coarse() {
        let mut world = Box3::default();
        let mut lighting = MultiScaleLighting::new(table());
        // A wall across the box at x = 8: all of a layer of cells of 1, half of the cells of 2 that
        // it is in (not more than half), and less than that of larger cells.
        for y in 0..EDGE {
            for z in 0..EDGE {
                world.place(at(8, y, z), STONE, &mut lighting);
            }
        }
        // A cell's worth of candles of 16, on one side: 4 at scale 0, a cell of 2 of them giving 4 at
        // scale 1 (128 over 32).
        for (dx, dy, dz) in CELL_OF_EIGHT {
            world.place(at(4 + dx, 4 + dy, 4 + dz), CANDLE, &mut lighting);
        }
        settle(&mut lighting, &mut world);
        // Scale 0: the light of a candle reaches the wall (4, 3 beside it, 2 at the wall face) and
        // goes no further.
        assert_eq!(world.level_at(0, 7, 5, 5), 2);
        assert!((0..EDGE).all(|y| (0..EDGE).all(|z| world.level_at(0, 9, y, z) == 0)));
        // Scale 1: the cell with the wall in it is lit, 2, and light is beyond it, 1.
        assert_eq!(world.level_at(1, 6, 4, 4), 3);
        assert_eq!(world.level_at(1, 8, 4, 4), 2);
        assert_eq!(world.level_at(1, 10, 4, 4), 1);
    }

    #[test]
    fn a_wall_of_glass_stops_no_light_at_any_scale() {
        let mut world = Box3::default();
        let mut lighting = MultiScaleLighting::new(table());
        for y in 0..EDGE {
            for z in 0..EDGE {
                world.place(at(8, y, z), GLASS, &mut lighting);
            }
        }
        for (dx, dy, dz) in CELL_OF_EIGHT {
            world.place(at(4 + dx, 4 + dy, 4 + dz), LAMP, &mut lighting);
        }
        settle(&mut lighting, &mut world);
        assert!(world.level_at(0, 9, 5, 5) > 0);
        assert!(world.level_at(1, 10, 4, 4) > 0);
        assert!(
            world.level_at(0, 9, 5, 5) < world.level_at(0, 7, 5, 5),
            "dimmer beyond"
        );
    }

    #[test]
    fn light_that_loses_its_source_dies_out_at_every_scale() {
        let mut world = Box3::default();
        let mut lighting = MultiScaleLighting::new(table());
        world.place(at(8, 8, 8), LAMP, &mut lighting);
        settle(&mut lighting, &mut world);
        assert!(!world.lit(0).is_empty() && !world.lit(3).is_empty());
        world.place(at(8, 8, 8), AIR, &mut lighting);
        settle(&mut lighting, &mut world);
        for scale in 0..SCALES {
            assert!(
                world.lit(scale).is_empty(),
                "scale {scale}: {:?}",
                world.lit(scale)
            );
        }
    }

    #[test]
    fn light_spreads_a_cell_a_tick_so_coarse_light_is_faster_in_voxels() {
        let mut world = Box3::default();
        let mut lighting = MultiScaleLighting::new(table());
        for (dx, dy, dz) in CELL_OF_EIGHT {
            world.place(at(dx, dy, dz), LAMP, &mut lighting);
        }
        for tick in 1..=4i64 {
            lighting.step(&mut world);
            // The lamps are voxels 0 and 1 along x, and the cell of 2 that holds them is at 0. After
            // `tick` ticks the light has gone `tick` cells beyond them, at a level one less for each.
            let level = (15 - tick) as u8;
            assert_eq!(
                world.level_at(0, 1 + tick, 0, 0),
                level,
                "scale 0, tick {tick}"
            );
            assert_eq!(world.level_at(0, 2 + tick, 0, 0), 0, "scale 0, tick {tick}");
            assert_eq!(
                world.level_at(1, 2 * tick, 0, 0),
                level,
                "scale 1, tick {tick}"
            );
            assert_eq!(
                world.level_at(1, 2 * tick + 2, 0, 0),
                0,
                "scale 1, tick {tick}"
            );
        }
    }

    #[test]
    fn a_budget_slows_the_light_and_settles_to_the_same_levels() {
        let mut quick = Box3::default();
        let mut quick_rule = MultiScaleLighting::new(table());
        let mut slow = Box3::default();
        let mut slow_rule = MultiScaleLighting::new(table());
        for (x, y, z) in [(3, 3, 3), (12, 9, 5), (5, 12, 12)] {
            quick.place(at(x, y, z), LAMP, &mut quick_rule);
            slow.place(at(x, y, z), LAMP, &mut slow_rule);
        }
        quick.place(at(8, 8, 8), STONE, &mut quick_rule);
        slow.place(at(8, 8, 8), STONE, &mut slow_rule);
        let quick_steps = settle(&mut quick_rule, &mut quick);
        let mut slow_steps = 0;
        while !slow_rule.is_settled() {
            slow_rule.step_within(&mut slow, [40, 10, 5, 2]);
            slow_steps += 1;
            assert!(slow_steps < 5000);
        }
        assert!(slow_steps > quick_steps);
        for scale in 0..SCALES {
            assert_eq!(slow.lit(scale), quick.lit(scale), "scale {scale}");
        }
    }

    #[derive(Clone, Debug)]
    enum Edit {
        Place(i64, i64, i64, u32),
    }

    fn edits() -> impl Strategy<Value = Vec<Edit>> {
        prop::collection::vec(
            (0..EDGE, 0..EDGE, 0..EDGE, 0..5u32)
                .prop_map(|(x, y, z, block)| Edit::Place(x, y, z, block)),
            1..24,
        )
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(32))]

        #[test]
        fn the_rule_settles_to_what_a_search_from_the_sources_says_at_every_scale(
            first in edits(),
            second in edits(),
        ) {
            let table = table();
            let mut world = Box3::default();
            let mut lighting = MultiScaleLighting::new(table.clone());
            // Two rounds of edits, settling between, so that light is also taken away and moved.
            for round in [&first, &second] {
                for Edit::Place(x, y, z, block) in round {
                    world.place(at(*x, *y, *z), *block, &mut lighting);
                }
                settle(&mut lighting, &mut world);
                for scale in 0..SCALES {
                    prop_assert_eq!(world.lit(scale), reference(&world, &table, scale), "scale {}", scale);
                }
            }
        }
    }
}
