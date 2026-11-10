use std::collections::HashMap;

use proptest::prelude::*;
use voxel_meshing::{BrickOccupancy, OCCUPANCY_WORDS, PartialVoxel};
use voxel_space::{BrickCoordinate, GridId};

use super::*;

const GRID_A: GridId = GridId::new(GridId::FIRST_ALLOCATED);
const GRID_B: GridId = GridId::new(GridId::FIRST_ALLOCATED + 1);

fn brick(x: i128, y: i128, z: i128) -> BrickCoordinate {
    BrickCoordinate::new(x, y, z)
}

/// A brick with exactly the bits of these voxels set (as `(x, y, z)` in the brick).
fn mixed_with(voxels: &[[usize; 3]]) -> BrickOccupancy {
    let mut words = Box::new([0u32; OCCUPANCY_WORDS]);
    for &[x, y, z] in voxels {
        words[z * 32 + y] |= 1 << x;
    }
    BrickOccupancy::from_words(words)
}

/// What the graphics card holds once the changes drained so far are written to it: the tables of the windows and the pool of slots.
struct Memory {
    tables: Vec<Vec<u32>>,
    pool: Vec<u32>,
    partial_pool: Vec<u32>,
}

impl Memory {
    fn new() -> Self {
        Self {
            tables: vec![vec![0; WINDOW_ENTRIES]; MAXIMUM_WINDOWS],
            pool: Vec::new(),
            partial_pool: Vec::new(),
        }
    }

    fn write(&mut self, changes: Changes) {
        for write in changes.tables {
            let table = &mut self.tables[write.window];
            table[write.first..write.first + write.entries.len()].copy_from_slice(&write.entries);
        }
        for write in changes.slots {
            let first = write.slot as usize * OCCUPANCY_WORDS;
            if self.pool.len() < first + OCCUPANCY_WORDS {
                self.pool.resize(first + OCCUPANCY_WORDS, 0);
            }
            self.pool[first..first + OCCUPANCY_WORDS].copy_from_slice(&write.words[..]);
        }
        for write in changes.partial_slots {
            let first = write.slot as usize * PARTIAL_SLOT_WORDS;
            if self.partial_pool.len() < first + PARTIAL_SLOT_WORDS {
                self.partial_pool.resize(first + PARTIAL_SLOT_WORDS, 0);
            }
            self.partial_pool[first..first + PARTIAL_SLOT_WORDS].copy_from_slice(&write.words[..]);
        }
    }

    /// What the walk of the shader reads of the shape of a voxel: the shape of a voxel that blocks the sun in part (`partial_shape` in `contact_on.wgsl`).
    fn partial_shape(&self, window: usize, voxel: [i128; 3]) -> Option<u16> {
        let coordinate = brick(
            voxel[0].div_euclid(32),
            voxel[1].div_euclid(32),
            voxel[2].div_euclid(32),
        );
        let entry = self.tables[window][table_index(coordinate)];
        let slot = (entry >> PARTIAL_ENTRY_SHIFT) as usize;
        if slot == 0 {
            return None;
        }
        let index = (voxel[2].rem_euclid(32) * 1024
            + voxel[1].rem_euclid(32) * 32
            + voxel[0].rem_euclid(32)) as usize;
        let word = self.partial_pool[(slot - 1) * PARTIAL_SLOT_WORDS + index / 2];
        let number = (word >> (16 * (index % 2))) & 0xFFFF;
        number.checked_sub(1).map(|shape| shape as u16)
    }

    /// What the walk of the shader reads for a voxel of the grid of `window`: `None` where nothing is known, else whether it blocks the sun. (The same lookup as
    /// `occupancy_at` in `contact_on.wgsl`, from the voxel's own coordinates.)
    fn blocks(&self, window: usize, voxel: [i128; 3]) -> Option<bool> {
        let coordinate = brick(
            voxel[0].div_euclid(32),
            voxel[1].div_euclid(32),
            voxel[2].div_euclid(32),
        );
        match self.tables[window][table_index(coordinate)] & BITS_ENTRY_MASK {
            ENTRY_UNKNOWN => None,
            ENTRY_EMPTY => Some(false),
            ENTRY_FULL => Some(true),
            entry => {
                let slot = (entry - FIRST_SLOT_ENTRY) as usize;
                let (x, y, z) = (
                    voxel[0].rem_euclid(32) as usize,
                    voxel[1].rem_euclid(32) as usize,
                    voxel[2].rem_euclid(32) as usize,
                );
                let word = self.pool[slot * OCCUPANCY_WORDS + z * 32 + y];
                Some(word >> x & 1 == 1)
            }
        }
    }
}

fn drained(volume: &mut Volume, memory: &mut Memory) {
    memory.write(volume.drain());
}

#[test]
fn nothing_is_known_at_first_and_nothing_is_active() {
    let mut volume = Volume::new(100);
    assert!(!volume.is_active());
    assert_eq!(volume.window_of(VolumeKey::Terrain), None);
    assert_eq!(volume.held(VolumeKey::Terrain, brick(0, 0, 0)), None);
    let changes = volume.drain();
    assert!(changes.tables.is_empty() && changes.slots.is_empty());
    assert_eq!(volume.slots_wanted(), 0);
}

#[test]
fn an_empty_brick_a_full_brick_and_a_mixed_one_are_told_as_they_are() {
    let mut volume = Volume::new(100);
    let mut memory = Memory::new();
    volume.set(VolumeKey::Terrain, brick(0, 0, 0), &BrickOccupancy::Empty);
    volume.set(VolumeKey::Terrain, brick(1, 0, 0), &BrickOccupancy::Full);
    volume.set(
        VolumeKey::Terrain,
        brick(2, 0, 0),
        &mixed_with(&[[3, 4, 5], [31, 31, 31], [0, 0, 0]]),
    );
    drained(&mut volume, &mut memory);
    let window = volume.window_of(VolumeKey::Terrain).expect("a window");
    assert_eq!(memory.blocks(window, [5, 5, 5]), Some(false));
    assert_eq!(memory.blocks(window, [32 + 5, 5, 5]), Some(true));
    assert_eq!(memory.blocks(window, [64 + 3, 4, 5]), Some(true));
    assert_eq!(memory.blocks(window, [64 + 31, 31, 31]), Some(true));
    assert_eq!(memory.blocks(window, [64, 0, 0]), Some(true));
    assert_eq!(memory.blocks(window, [64 + 4, 4, 5]), Some(false));
    assert_eq!(memory.blocks(window, [64, 0, 1]), Some(false));
    // A brick nothing was told of is not known, which is clear for the walk.
    assert_eq!(memory.blocks(window, [96 + 1, 1, 1]), None);
    assert_eq!(volume.slots_wanted(), 1);
    let statistics = volume.statistics();
    assert_eq!(
        (statistics.empty, statistics.full, statistics.mixed),
        (1, 1, 1)
    );
}

#[test]
fn bricks_below_zero_are_found_where_they_were_put() {
    let mut volume = Volume::new(100);
    let mut memory = Memory::new();
    volume.set(VolumeKey::Terrain, brick(-1, -1, -1), &BrickOccupancy::Full);
    volume.set(
        VolumeKey::Terrain,
        brick(-33, 0, 0),
        &mixed_with(&[[31, 0, 0]]),
    );
    drained(&mut volume, &mut memory);
    let window = volume.window_of(VolumeKey::Terrain).expect("a window");
    assert_eq!(memory.blocks(window, [-1, -1, -1]), Some(true));
    assert_eq!(memory.blocks(window, [-32, -32, -32]), Some(true));
    // (-33 * 32 + 31 = -1025.)
    assert_eq!(memory.blocks(window, [-1025, 0, 0]), Some(true));
    assert_eq!(memory.blocks(window, [-1026, 0, 0]), Some(false));
}

#[test]
fn a_window_with_only_empty_bricks_is_not_active() {
    let mut volume = Volume::new(100);
    volume.set(VolumeKey::Terrain, brick(0, 0, 0), &BrickOccupancy::Empty);
    assert!(!volume.is_active());
    assert_eq!(volume.window_of(VolumeKey::Terrain), None);
    volume.set(VolumeKey::Terrain, brick(1, 0, 0), &BrickOccupancy::Full);
    assert!(volume.is_active());
    assert!(volume.window_of(VolumeKey::Terrain).is_some());
    // Digging it out again makes the window inactive without forgetting its other bricks.
    volume.set(VolumeKey::Terrain, brick(1, 0, 0), &BrickOccupancy::Empty);
    assert!(!volume.is_active());
    assert_eq!(
        volume.held(VolumeKey::Terrain, brick(0, 0, 0)),
        Some(Held::Empty)
    );
    // And the same when the solid brick goes away altogether, not only when it is dug out.
    volume.set(
        VolumeKey::Terrain,
        brick(2, 0, 0),
        &mixed_with(&[[1, 1, 1]]),
    );
    assert!(volume.is_active());
    volume.clear(VolumeKey::Terrain, brick(2, 0, 0));
    assert!(!volume.is_active());
    assert_eq!(volume.window_of(VolumeKey::Terrain), None);
}

#[test]
fn a_mixed_brick_keeps_its_slot_when_it_changes_and_gives_it_back_when_it_does_not_need_one() {
    let mut volume = Volume::new(100);
    let mut memory = Memory::new();
    volume.set(
        VolumeKey::Terrain,
        brick(0, 0, 0),
        &mixed_with(&[[1, 1, 1]]),
    );
    let first = volume.held(VolumeKey::Terrain, brick(0, 0, 0));
    volume.set(
        VolumeKey::Terrain,
        brick(0, 0, 0),
        &mixed_with(&[[2, 2, 2]]),
    );
    assert_eq!(volume.held(VolumeKey::Terrain, brick(0, 0, 0)), first);
    drained(&mut volume, &mut memory);
    let window = volume.window_of(VolumeKey::Terrain).expect("a window");
    assert_eq!(memory.blocks(window, [1, 1, 1]), Some(false));
    assert_eq!(memory.blocks(window, [2, 2, 2]), Some(true));
    assert_eq!(volume.slots_wanted(), 1);
    // Filled with stone it needs none, and the slot is the next brick's.
    volume.set(VolumeKey::Terrain, brick(0, 0, 0), &BrickOccupancy::Full);
    volume.set(
        VolumeKey::Terrain,
        brick(1, 0, 0),
        &mixed_with(&[[9, 9, 9]]),
    );
    assert_eq!(volume.slots_wanted(), 1);
    assert_eq!(volume.statistics().slots_in_use, 1);
}

#[test]
fn a_cleared_brick_is_unknown_again_and_its_slot_is_freed_once() {
    let mut volume = Volume::new(100);
    let mut memory = Memory::new();
    volume.set(
        VolumeKey::Terrain,
        brick(0, 0, 0),
        &mixed_with(&[[1, 1, 1]]),
    );
    volume.set(VolumeKey::Terrain, brick(1, 0, 0), &BrickOccupancy::Full);
    drained(&mut volume, &mut memory);
    volume.clear(VolumeKey::Terrain, brick(0, 0, 0));
    // (Clearing what is not there, or twice, does nothing.)
    volume.clear(VolumeKey::Terrain, brick(0, 0, 0));
    volume.clear(VolumeKey::Terrain, brick(5, 5, 5));
    drained(&mut volume, &mut memory);
    let window = volume.window_of(VolumeKey::Terrain).expect("a window");
    assert_eq!(memory.blocks(window, [1, 1, 1]), None);
    assert_eq!(memory.blocks(window, [33, 1, 1]), Some(true));
    assert_eq!(volume.statistics().slots_in_use, 0);
    // Two bricks that need slots now take two different ones: the freed slot was not freed twice.
    volume.set(
        VolumeKey::Terrain,
        brick(2, 0, 0),
        &mixed_with(&[[1, 1, 1]]),
    );
    volume.set(
        VolumeKey::Terrain,
        brick(3, 0, 0),
        &mixed_with(&[[2, 2, 2]]),
    );
    assert_eq!(volume.statistics().slots_in_use, 2);
    assert_eq!(volume.slots_wanted(), 2);
    drained(&mut volume, &mut memory);
    assert_eq!(memory.blocks(window, [64 + 1, 1, 1]), Some(true));
    assert_eq!(memory.blocks(window, [64 + 2, 2, 2]), Some(false));
    assert_eq!(memory.blocks(window, [96 + 2, 2, 2]), Some(true));
    assert_eq!(memory.blocks(window, [96 + 1, 1, 1]), Some(false));
}

#[test]
fn a_brick_a_whole_window_away_takes_the_place_of_the_one_it_would_be_mistaken_for() {
    let mut volume = Volume::new(100);
    let mut memory = Memory::new();
    let near = brick(3, 4, 5);
    let far = brick(3 + WINDOW_BRICKS as i128, 4, 5);
    assert_eq!(table_index(near), table_index(far));
    volume.set(VolumeKey::Terrain, near, &mixed_with(&[[1, 1, 1]]));
    volume.set(VolumeKey::Terrain, far, &BrickOccupancy::Full);
    drained(&mut volume, &mut memory);
    let window = volume.window_of(VolumeKey::Terrain).expect("a window");
    // The older one is forgotten (it is unknown, never the newer one's answer), and its slot is free.
    assert_eq!(volume.held(VolumeKey::Terrain, near), None);
    assert_eq!(volume.held(VolumeKey::Terrain, far), Some(Held::Full));
    assert_eq!(volume.statistics().slots_in_use, 0);
    assert_eq!(
        memory.blocks(window, [far.x() * 32 + 1, 4 * 32 + 1, 5 * 32 + 1]),
        Some(true)
    );
    // Telling that the forgotten one is gone does not wipe the newer one.
    volume.clear(VolumeKey::Terrain, near);
    drained(&mut volume, &mut memory);
    assert_eq!(volume.held(VolumeKey::Terrain, far), Some(Held::Full));
    assert_eq!(
        memory.blocks(window, [far.x() * 32 + 1, 4 * 32 + 1, 5 * 32 + 1]),
        Some(true)
    );
    // And bricks that are a window apart in different directions do not meet at all.
    assert_ne!(table_index(brick(0, 0, 0)), table_index(brick(1, 0, 0)));
    assert_ne!(table_index(brick(0, 0, 0)), table_index(brick(0, 1, 0)));
    assert_ne!(table_index(brick(0, 0, 0)), table_index(brick(0, 0, 1)));
}

#[test]
fn every_brick_of_a_window_has_a_place_of_its_own() {
    let half = WINDOW_BRICKS as i128 / 2;
    let mut seen = vec![false; WINDOW_ENTRIES];
    for z in -half..half {
        for y in -half..half {
            for x in -half..half {
                let index = table_index(brick(x, y, z));
                assert!(!seen[index], "({x}, {y}, {z}) shares its place");
                seen[index] = true;
            }
        }
    }
    assert!(seen.iter().all(|&taken| taken));
}

#[test]
fn each_grid_has_a_window_and_the_window_goes_with_its_last_brick() {
    let mut volume = Volume::new(100);
    let mut memory = Memory::new();
    volume.set(VolumeKey::Terrain, brick(0, 0, 0), &BrickOccupancy::Full);
    volume.set(
        VolumeKey::Grid(GRID_A),
        brick(0, 0, 0),
        &mixed_with(&[[7, 7, 7]]),
    );
    volume.set(
        VolumeKey::Grid(GRID_B),
        brick(0, 0, 0),
        &BrickOccupancy::Full,
    );
    drained(&mut volume, &mut memory);
    let terrain = volume.window_of(VolumeKey::Terrain).expect("terrain");
    let first = volume.window_of(VolumeKey::Grid(GRID_A)).expect("a");
    let second = volume.window_of(VolumeKey::Grid(GRID_B)).expect("b");
    assert!(terrain != first && first != second && terrain != second);
    // The same coordinates in different grids are different voxels.
    assert_eq!(memory.blocks(terrain, [7, 7, 7]), Some(true));
    assert_eq!(memory.blocks(first, [7, 7, 7]), Some(true));
    assert_eq!(memory.blocks(first, [8, 7, 7]), Some(false));
    volume.clear(VolumeKey::Grid(GRID_A), brick(0, 0, 0));
    assert_eq!(volume.window_of(VolumeKey::Grid(GRID_A)), None);
    assert_eq!(volume.statistics().windows, 2);
    assert_eq!(volume.statistics().slots_in_use, 0);
    // The window that was freed is the next grid's, and starts from nothing.
    let third = GridId::new(GridId::FIRST_ALLOCATED + 9);
    volume.set(
        VolumeKey::Grid(third),
        brick(4, 4, 4),
        &BrickOccupancy::Full,
    );
    drained(&mut volume, &mut memory);
    let reused = volume.window_of(VolumeKey::Grid(third)).expect("third");
    assert_eq!(reused, first);
    assert_eq!(memory.blocks(reused, [7, 7, 7]), None);
    assert_eq!(memory.blocks(reused, [4 * 32, 4 * 32, 4 * 32]), Some(true));
}

#[test]
fn a_grid_taken_whole_frees_its_slots_and_its_window() {
    let mut volume = Volume::new(100);
    for index in 0..5 {
        volume.set(
            VolumeKey::Grid(GRID_A),
            brick(index, 0, 0),
            &mixed_with(&[[1, 2, 3]]),
        );
    }
    volume.set(
        VolumeKey::Terrain,
        brick(0, 0, 0),
        &mixed_with(&[[1, 2, 3]]),
    );
    assert_eq!(volume.statistics().slots_in_use, 6);
    volume.clear_grid(VolumeKey::Grid(GRID_A));
    assert_eq!(volume.window_of(VolumeKey::Grid(GRID_A)), None);
    assert_eq!(volume.statistics().slots_in_use, 1);
    assert_eq!(volume.statistics().windows, 1);
    // (A grid that is not there is nothing to take.)
    volume.clear_grid(VolumeKey::Grid(GRID_B));
    assert_eq!(volume.statistics().windows, 1);
}

#[test]
fn more_grids_than_windows_leave_the_extra_ones_untraced_and_counted() {
    let mut volume = Volume::new(100);
    for grid in 0..MAXIMUM_WINDOWS as u64 + 2 {
        volume.set(
            VolumeKey::Grid(GridId::new(GridId::FIRST_ALLOCATED + grid)),
            brick(0, 0, 0),
            &BrickOccupancy::Full,
        );
    }
    let statistics = volume.statistics();
    assert_eq!(statistics.windows, MAXIMUM_WINDOWS);
    assert_eq!(statistics.rejected, 2);
    let last = GridId::new(GridId::FIRST_ALLOCATED + MAXIMUM_WINDOWS as u64 + 1);
    assert_eq!(volume.window_of(VolumeKey::Grid(last)), None);
    // Clearing a brick of a grid that has no window is nothing.
    volume.clear(VolumeKey::Grid(last), brick(0, 0, 0));
    assert_eq!(volume.statistics().windows, MAXIMUM_WINDOWS);
}

#[test]
fn when_there_are_no_slots_left_a_mixed_brick_is_unknown_and_counted_and_a_slot_given_back_is_used()
{
    let mut volume = Volume::new(2);
    let mut memory = Memory::new();
    for index in 0..3 {
        volume.set(
            VolumeKey::Terrain,
            brick(index, 0, 0),
            &mixed_with(&[[index as usize, 0, 0]]),
        );
    }
    assert_eq!(volume.statistics().slots_in_use, 2);
    assert_eq!(volume.statistics().rejected, 1);
    assert_eq!(volume.held(VolumeKey::Terrain, brick(2, 0, 0)), None);
    assert_eq!(volume.slots_wanted(), 2);
    // A brick that needs no slot is still taken.
    volume.set(VolumeKey::Terrain, brick(3, 0, 0), &BrickOccupancy::Full);
    assert_eq!(
        volume.held(VolumeKey::Terrain, brick(3, 0, 0)),
        Some(Held::Full)
    );
    volume.clear(VolumeKey::Terrain, brick(0, 0, 0));
    volume.set(
        VolumeKey::Terrain,
        brick(2, 0, 0),
        &mixed_with(&[[2, 0, 0]]),
    );
    assert!(volume.held(VolumeKey::Terrain, brick(2, 0, 0)).is_some());
    drained(&mut volume, &mut memory);
    let window = volume.window_of(VolumeKey::Terrain).expect("a window");
    assert_eq!(memory.blocks(window, [64 + 2, 0, 0]), Some(true));
    assert_eq!(memory.blocks(window, [64 + 3, 0, 0]), Some(false));
}

#[test]
fn what_is_drained_is_drained_once_and_only_the_rows_that_changed_are_written() {
    let mut volume = Volume::new(100);
    volume.set(VolumeKey::Terrain, brick(0, 0, 0), &BrickOccupancy::Full);
    let first = volume.drain();
    // A new window is written whole (it must not show what a window before it held), as one run.
    assert_eq!(first.tables.len(), 1);
    assert_eq!(first.tables[0].first, 0);
    assert_eq!(first.tables[0].entries.len(), WINDOW_ENTRIES);
    assert!(volume.drain().tables.is_empty());
    // After that one brick is one row of the table.
    volume.set(VolumeKey::Terrain, brick(5, 6, 7), &BrickOccupancy::Full);
    let second = volume.drain();
    assert_eq!(second.tables.len(), 1);
    assert_eq!(second.tables[0].entries.len(), WINDOW_BRICKS);
    assert_eq!(second.tables[0].first % WINDOW_BRICKS, 0);
    assert!(volume.drain().tables.is_empty());
    // Setting a brick to what it already is writes nothing new that matters, but never writes less than the truth: a row is written whole.
    volume.set(VolumeKey::Terrain, brick(5, 6, 7), &BrickOccupancy::Full);
    volume.set(VolumeKey::Terrain, brick(6, 6, 7), &BrickOccupancy::Full);
    let third = volume.drain();
    assert_eq!(
        third.tables.len(),
        1,
        "neighbouring bricks of a row are one write"
    );
}

#[test]
fn the_slots_wanted_follow_the_highest_slot_ever_used_so_that_the_pool_only_grows() {
    let mut volume = Volume::new(100);
    for index in 0..10 {
        volume.set(
            VolumeKey::Terrain,
            brick(index, 0, 0),
            &mixed_with(&[[1, 1, 1]]),
        );
    }
    assert_eq!(volume.slots_wanted(), 10);
    for index in 0..10 {
        volume.clear(VolumeKey::Terrain, brick(index, 0, 0));
    }
    assert_eq!(volume.statistics().slots_in_use, 0);
    assert_eq!(volume.slots_wanted(), 10);
    volume.set(
        VolumeKey::Terrain,
        brick(0, 0, 0),
        &mixed_with(&[[1, 1, 1]]),
    );
    assert_eq!(volume.slots_wanted(), 10);
}

/// A brick with these voxels blocking the sun in part, each of the shape given, and no bits set.
fn shaped_with(partials: &[(usize, u16)]) -> BrickOccupancy {
    BrickOccupancy::from_parts(
        Box::new([0u32; OCCUPANCY_WORDS]),
        partials
            .iter()
            .map(|&(voxel, shape)| PartialVoxel {
                voxel: voxel as u16,
                shape,
            })
            .collect(),
    )
}

#[test]
fn a_brick_with_partial_voxels_has_a_partial_slot_whose_number_is_in_the_upper_half_of_its_entry_and_that_is_all_the_shader_needs()
 {
    let mut volume = Volume::new(64);
    let mut memory = Memory::new();
    // A slab (shape 0, which must not be mistaken for none) at the voxel (3, 4, 5), and a stair (shape 41) at (31, 31, 31), the last voxel of the last word.
    let at = |x: usize, y: usize, z: usize| z * 1024 + y * 32 + x;
    volume.set(
        VolumeKey::Terrain,
        brick(2, 0, 0),
        &shaped_with(&[(at(3, 4, 5), 0), (at(31, 31, 31), 41)]),
    );
    drained(&mut volume, &mut memory);
    let window = volume
        .window_of(VolumeKey::Terrain)
        .expect("slabs are something that blocks the sun");
    let voxel = |x: i128, y: i128, z: i128| [64 + x, y, z];
    assert_eq!(memory.partial_shape(window, voxel(3, 4, 5)), Some(0));
    assert_eq!(memory.partial_shape(window, voxel(31, 31, 31)), Some(41));
    assert_eq!(memory.partial_shape(window, voxel(3, 4, 6)), None);
    assert_eq!(memory.partial_shape(window, voxel(2, 4, 5)), None);
    // Its bits say nothing blocks whole, and its entry is the slot of the bits.
    assert_eq!(memory.blocks(window, voxel(3, 4, 5)), Some(false));
    let statistics = volume.statistics();
    assert_eq!((statistics.shaped, statistics.partial_slots_in_use), (1, 1));
    // A brick with no partial voxel has no upper half to its entry.
    volume.set(
        VolumeKey::Terrain,
        brick(3, 0, 0),
        &mixed_with(&[[1, 1, 1]]),
    );
    drained(&mut volume, &mut memory);
    assert_eq!(
        memory.tables[window][table_index(brick(3, 0, 0))] >> PARTIAL_ENTRY_SHIFT,
        0
    );
}

#[test]
fn a_window_that_holds_only_slabs_is_active_and_goes_inactive_when_the_slabs_do() {
    let mut volume = Volume::new(64);
    volume.set(VolumeKey::Terrain, brick(0, 0, 0), &shaped_with(&[(7, 3)]));
    assert!(volume.is_active());
    assert!(volume.window_of(VolumeKey::Terrain).is_some());
    // Told again as air: the slot is given back and nothing blocks the sun.
    volume.set(VolumeKey::Terrain, brick(0, 0, 0), &BrickOccupancy::Empty);
    assert!(!volume.is_active());
    assert_eq!(volume.statistics().partial_slots_in_use, 0);
    // Told as slabs again, then cleared: also given back, once.
    volume.set(VolumeKey::Terrain, brick(0, 0, 0), &shaped_with(&[(7, 3)]));
    volume.clear(VolumeKey::Terrain, brick(0, 0, 0));
    assert_eq!(volume.statistics().partial_slots_in_use, 0);
    assert!(!volume.is_active());
}

#[test]
fn a_brick_keeps_its_partial_slot_while_it_has_partial_voxels_and_a_change_of_shape_is_written() {
    let mut volume = Volume::new(64);
    let mut memory = Memory::new();
    volume.set(VolumeKey::Terrain, brick(0, 0, 0), &shaped_with(&[(10, 2)]));
    drained(&mut volume, &mut memory);
    volume.set(
        VolumeKey::Terrain,
        brick(0, 0, 0),
        &shaped_with(&[(10, 5), (11, 6)]),
    );
    assert_eq!(volume.partial_slots_wanted(), 1, "the brick kept its slot");
    drained(&mut volume, &mut memory);
    let window = volume.window_of(VolumeKey::Terrain).unwrap();
    assert_eq!(memory.partial_shape(window, [10, 0, 0]), Some(5));
    assert_eq!(memory.partial_shape(window, [11, 0, 0]), Some(6));
    // A brick of bits and slabs together: both are held, in slots of their own.
    let mut words = Box::new([0u32; OCCUPANCY_WORDS]);
    words[0] = 1;
    volume.set(
        VolumeKey::Terrain,
        brick(1, 0, 0),
        &BrickOccupancy::from_parts(words, vec![PartialVoxel { voxel: 5, shape: 1 }]),
    );
    drained(&mut volume, &mut memory);
    assert_eq!(memory.blocks(window, [32, 0, 0]), Some(true));
    assert_eq!(memory.partial_shape(window, [32 + 5, 0, 0]), Some(1));
}

#[test]
fn when_there_are_no_partial_slots_left_the_brick_is_traced_by_its_bits_and_counted_and_a_slot_given_back_is_used()
 {
    // A pool for 16 mixed bricks has room for one partial brick.
    let mut volume = Volume::new(SLOTS_PER_PARTIAL_SLOT);
    let mut memory = Memory::new();
    volume.set(VolumeKey::Terrain, brick(0, 0, 0), &shaped_with(&[(1, 1)]));
    let mut words = Box::new([0u32; OCCUPANCY_WORDS]);
    words[0] = 0b100;
    volume.set(
        VolumeKey::Terrain,
        brick(1, 0, 0),
        &BrickOccupancy::from_parts(words, vec![PartialVoxel { voxel: 5, shape: 1 }]),
    );
    assert_eq!(volume.statistics().rejected, 1);
    drained(&mut volume, &mut memory);
    let window = volume.window_of(VolumeKey::Terrain).unwrap();
    // (The second brick's bits are known; its partial voxel is not.)
    assert_eq!(memory.blocks(window, [32 + 2, 0, 0]), Some(true));
    assert_eq!(memory.partial_shape(window, [32 + 5, 0, 0]), None);
    volume.clear(VolumeKey::Terrain, brick(0, 0, 0));
    let mut words = Box::new([0u32; OCCUPANCY_WORDS]);
    words[0] = 0b100;
    volume.set(
        VolumeKey::Terrain,
        brick(1, 0, 0),
        &BrickOccupancy::from_parts(words, vec![PartialVoxel { voxel: 5, shape: 1 }]),
    );
    drained(&mut volume, &mut memory);
    assert_eq!(memory.partial_shape(window, [32 + 5, 0, 0]), Some(1));
}

#[test]
fn the_bounds_of_a_window_are_those_of_the_bricks_that_hold_something_that_blocks_the_sun() {
    let mut volume = Volume::new(64);
    assert_eq!(volume.solid_bounds(VolumeKey::Terrain), None);
    // Empty bricks do not count: there is nothing in them for a ray from another grid to meet.
    volume.set(VolumeKey::Terrain, brick(9, 9, 9), &BrickOccupancy::Empty);
    assert_eq!(volume.solid_bounds(VolumeKey::Terrain), None);
    volume.set(VolumeKey::Terrain, brick(-2, 5, 0), &BrickOccupancy::Full);
    assert_eq!(
        volume.solid_bounds(VolumeKey::Terrain),
        Some([[-2, 5, 0], [-2, 5, 0]])
    );
    volume.set(
        VolumeKey::Terrain,
        brick(3, 1, 7),
        &mixed_with(&[[1, 1, 1]]),
    );
    volume.set(VolumeKey::Terrain, brick(0, 8, -4), &shaped_with(&[(3, 1)]));
    assert_eq!(
        volume.solid_bounds(VolumeKey::Terrain),
        Some([[-2, 1, -4], [3, 8, 7]])
    );
    // The bounds shrink when the brick that made them goes, or becomes air.
    volume.clear(VolumeKey::Terrain, brick(3, 1, 7));
    volume.set(VolumeKey::Terrain, brick(0, 8, -4), &BrickOccupancy::Empty);
    assert_eq!(
        volume.solid_bounds(VolumeKey::Terrain),
        Some([[-2, 5, 0], [-2, 5, 0]])
    );
    // Each grid has its own, and a grid with no window has none.
    volume.set(
        VolumeKey::Grid(GRID_A),
        brick(40, 40, 40),
        &BrickOccupancy::Full,
    );
    assert_eq!(
        volume.solid_bounds(VolumeKey::Grid(GRID_A)),
        Some([[40, 40, 40], [40, 40, 40]])
    );
    assert_eq!(volume.solid_bounds(VolumeKey::Grid(GRID_B)), None);
}

/// One thing done to a volume.
#[derive(Clone, Debug)]
enum Operation {
    Set(u8, BrickCoordinate, BrickOccupancy),
    Clear(u8, BrickCoordinate),
    ClearGrid(u8),
}

fn key_of(grid: u8) -> VolumeKey {
    match grid {
        0 => VolumeKey::Terrain,
        other => VolumeKey::Grid(GridId::new(GridId::FIRST_ALLOCATED + u64::from(other))),
    }
}

fn arbitrary_occupancy() -> impl Strategy<Value = BrickOccupancy> {
    prop_oneof![
        2 => Just(BrickOccupancy::Empty),
        2 => Just(BrickOccupancy::Full),
        2 => (
            proptest::collection::btree_map(0u16..32 * 32 * 32, 0u16..300, 1..30),
            proptest::collection::vec(0u32..32 * 32 * 32, 0..10),
        )
            .prop_map(|(partials, wholes)| {
                // (A voxel is partial or whole, not both.)
                let mut words = Box::new([0u32; OCCUPANCY_WORDS]);
                for voxel in wholes {
                    if !partials.contains_key(&(voxel as u16)) {
                        words[voxel as usize / 32] |= 1 << (voxel % 32);
                    }
                }
                let partials = partials
                    .into_iter()
                    .map(|(voxel, shape)| PartialVoxel { voxel, shape })
                    .collect();
                BrickOccupancy::from_parts(words, partials)
            }),
        3 => proptest::collection::vec(0u32..32 * 32 * 32, 1..40).prop_map(|voxels| {
            let mut words = Box::new([0u32; OCCUPANCY_WORDS]);
            for voxel in voxels {
                words[voxel as usize / 32] |= 1 << (voxel % 32);
            }
            BrickOccupancy::from_words(words)
        }),
    ]
}

fn arbitrary_operation() -> impl Strategy<Value = Operation> {
    // (Bricks within a window and a half of the origin, so that bricks a window apart meet now and then; two grids besides the terrain.)
    let span = -(WINDOW_BRICKS as i128)..(WINDOW_BRICKS as i128);
    let coordinate = (span.clone(), -2i128..3, span).prop_map(|(x, y, z)| brick(x, y, z));
    prop_oneof![
        8 => (0u8..3, coordinate.clone(), arbitrary_occupancy())
            .prop_map(|(grid, coordinate, occupancy)| Operation::Set(grid, coordinate, occupancy)),
        4 => (0u8..3, coordinate).prop_map(|(grid, coordinate)| Operation::Clear(grid, coordinate)),
        1 => (0u8..3).prop_map(Operation::ClearGrid),
    ]
}

proptest! {
    /// Whatever is done, what the graphics card holds after the changes are written says for every place of a window what the plain map of bricks says: the bits of the brick
    /// told of last at that place, nothing for a place whose brick was cleared or pushed out by one a window away, and nothing of a grid that was taken whole. (The table
    /// answers for the brick that stands at a place, whichever brick that is: telling a brick from one a window away is for the shader, which asks only about the bricks
    /// within half a window of its origin, while the bricks that are held are within a quarter of it.)
    #[test]
    fn the_memory_the_shader_reads_is_what_a_plain_map_of_bricks_says(
        operations in proptest::collection::vec(arbitrary_operation(), 1..120),
        drain_every in 1usize..40,
        probes in proptest::collection::vec((0u8..3, -2100i128..2100, -70i128..100, -2100i128..2100), 40),
    ) {
        let mut volume = Volume::new(100_000);
        let mut memory = Memory::new();
        // The map: per grid, the brick that stands at each place of the window.
        let mut model: HashMap<(u8, usize), (BrickCoordinate, BrickOccupancy)> = HashMap::new();
        for (count, operation) in operations.iter().enumerate() {
            match operation {
                Operation::Set(grid, coordinate, occupancy) => {
                    volume.set(key_of(*grid), *coordinate, occupancy);
                    model.insert((*grid, table_index(*coordinate)), (*coordinate, occupancy.clone()));
                }
                Operation::Clear(grid, coordinate) => {
                    volume.clear(key_of(*grid), *coordinate);
                    let place = (*grid, table_index(*coordinate));
                    if model.get(&place).is_some_and(|(held, _)| held == coordinate) {
                        model.remove(&place);
                    }
                }
                Operation::ClearGrid(grid) => {
                    volume.clear_grid(key_of(*grid));
                    model.retain(|(held_grid, _), _| held_grid != grid);
                }
            }
            if (count + 1) % drain_every == 0 {
                drained(&mut volume, &mut memory);
            }
        }
        drained(&mut volume, &mut memory);
        for &(grid, x, y, z) in &probes {
            let voxel = [x, y, z];
            let coordinate = brick(x.div_euclid(32), y.div_euclid(32), z.div_euclid(32));
            let expected = model
                .get(&(grid, table_index(coordinate)))
                .map(|(_, occupancy)| {
                    occupancy.blocks(voxel_space::LocalVoxelIndex::new(
                        x.rem_euclid(32) as u8,
                        y.rem_euclid(32) as u8,
                        z.rem_euclid(32) as u8,
                    ))
                });
            let window = volume.window_of(key_of(grid));
            let solid_somewhere = model.iter().any(|((held_grid, _), (_, occupancy))| {
                *held_grid == grid && *occupancy != BrickOccupancy::Empty
            });
            prop_assert_eq!(window.is_some(), solid_somewhere, "grid {} is active only while it holds something solid", grid);
            // (A window with nothing solid is not given to the shader, so only an active one is read.)
            if let Some(window) = window {
                prop_assert_eq!(memory.blocks(window, voxel), expected, "grid {} voxel {:?}", grid, voxel);
                let expected_shape = model
                    .get(&(grid, table_index(coordinate)))
                    .and_then(|(_, occupancy)| {
                        occupancy.partial_shape(voxel_space::LocalVoxelIndex::new(
                            x.rem_euclid(32) as u8,
                            y.rem_euclid(32) as u8,
                            z.rem_euclid(32) as u8,
                        ))
                    });
                prop_assert_eq!(memory.partial_shape(window, voxel), expected_shape, "grid {} voxel {:?}", grid, voxel);
            }
        }
        // Every slot in use is some mixed brick's, once.
        let mixed = model
            .values()
            .filter(|(_, occupancy)| matches!(occupancy, BrickOccupancy::Mixed(_) | BrickOccupancy::Shaped { .. }))
            .count();
        prop_assert_eq!(volume.statistics().slots_in_use, mixed);
        // And every partial slot in use is some shaped brick's, once.
        let shaped = model
            .values()
            .filter(|(_, occupancy)| matches!(occupancy, BrickOccupancy::Shaped { .. }))
            .count();
        prop_assert_eq!(volume.statistics().partial_slots_in_use, shaped);
        // And the bounds of each window are those of the solid bricks that stand in it.
        for grid in 0u8..3 {
            let mut expected: Option<[[i128; 3]; 2]> = None;
            for ((held_grid, _), (coordinate, occupancy)) in &model {
                if *held_grid != grid || *occupancy == BrickOccupancy::Empty {
                    continue;
                }
                let at = [coordinate.x(), coordinate.y(), coordinate.z()];
                expected = Some(match expected {
                    None => [at, at],
                    Some([low, high]) => [
                        std::array::from_fn(|axis| low[axis].min(at[axis])),
                        std::array::from_fn(|axis| high[axis].max(at[axis])),
                    ],
                });
            }
            prop_assert_eq!(volume.solid_bounds(key_of(grid)), expected, "grid {}", grid);
        }
    }
}
