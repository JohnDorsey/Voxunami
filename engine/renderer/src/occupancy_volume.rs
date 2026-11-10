//! The voxels that block the sun, in the form the shader walks them (`docs/specs/contact-trace-spec.md`, section 3.2). This is the memory's bookkeeping, on the CPU; the buffers
//! it describes are the renderer's.
//!
//! Each grid that has bricks in view has a **window**: a table of 64 by 64 by 64 entries that holds one entry for each brick, at the place of the brick's coordinates modulo 64.
//! An entry says that nothing is known of the brick (the walk ends there, clear), that no voxel of it blocks the sun, that every voxel does, or which **slot** of the pool holds
//! its bits (4 KB, one bit for each voxel, [`voxel_meshing::BrickOccupancy`]'s layout). A brick that has voxels that block the sun in part (a slab, a stair) has also a **partial
//! slot** (64 KB: a 16-bit number for each voxel, the shape of the voxel plus one, or 0 for a voxel that is not partial), whose number plus one is in the entry's upper half.
//! Two bricks that are a window apart share a place in the table, and the one told of last
//! wins: the older is forgotten, so the table never answers for a brick other than the one asked about, which is an unknown brick and so clear.
//!
//! The changes are collected here and handed over by [`Volume::drain`], to be written into the buffers in one go before a picture is drawn.

use std::collections::{BTreeSet, HashMap};

use voxel_meshing::{BrickOccupancy, OCCUPANCY_WORDS, PartialVoxel};
use voxel_space::{BrickCoordinate, GridId};

/// How many bricks a window is across, along each axis. A power of two, and more than the bricks of the widest view (33 for a radius of 16).
pub(crate) const WINDOW_BRICKS: usize = 64;
/// The entries of a window's table.
pub(crate) const WINDOW_ENTRIES: usize = WINDOW_BRICKS * WINDOW_BRICKS * WINDOW_BRICKS;
/// How many grids can be traced at once: the terrain of the level, planets, ships.
pub(crate) const MAXIMUM_WINDOWS: usize = 4;
/// The bytes of a slot of the pool.
pub(crate) const SLOT_BYTES: usize = OCCUPANCY_WORDS * 4;
/// The words of a partial slot: two voxels (16 bits each) to a word.
pub(crate) const PARTIAL_SLOT_WORDS: usize = 32 * 32 * 32 / 2;
/// The bytes of a partial slot, which is as much memory as this many slots of the pool: the partial pool may have a sixteenth as many.
pub(crate) const PARTIAL_SLOT_BYTES: usize = PARTIAL_SLOT_WORDS * 4;
pub(crate) const SLOTS_PER_PARTIAL_SLOT: usize = PARTIAL_SLOT_BYTES / SLOT_BYTES;
/// Where the partial slot's number plus one is in an entry; the lower half is what the entry says of the bits.
pub(crate) const PARTIAL_ENTRY_SHIFT: u32 = 16;
#[cfg(test)]
pub(crate) const BITS_ENTRY_MASK: u32 = (1 << PARTIAL_ENTRY_SHIFT) - 1;

/// What a table entry says (the same numbers as `ENTRY_*` in `contact_on.wgsl`): nothing is known of the brick,
pub(crate) const ENTRY_UNKNOWN: u32 = 0;
/// no voxel of the brick blocks the sun,
pub(crate) const ENTRY_EMPTY: u32 = 1;
/// every voxel does,
pub(crate) const ENTRY_FULL: u32 = 2;
/// or the bits are in the slot of this number minus this one.
pub(crate) const FIRST_SLOT_ENTRY: u32 = 3;

/// Whose bricks a window holds: the terrain of the level that is watched, or the grid of a ship or planet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum VolumeKey {
    Terrain,
    Grid(GridId),
}

/// What is known of a brick that is in a window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Held {
    Empty,
    Full,
    /// Its bits are in this slot of the pool.
    Slot(u32),
}

impl Held {
    fn entry(self) -> u32 {
        match self {
            Held::Empty => ENTRY_EMPTY,
            Held::Full => ENTRY_FULL,
            Held::Slot(slot) => FIRST_SLOT_ENTRY + slot,
        }
    }

    fn blocks_something(self) -> bool {
        self != Held::Empty
    }
}

/// Where a brick's entry is in a window's table: its coordinates modulo [`WINDOW_BRICKS`], x fastest. A row of 64 entries along x is the unit the table is written in.
pub(crate) fn table_index(coordinate: BrickCoordinate) -> usize {
    let wrap = |component: i128| component.rem_euclid(WINDOW_BRICKS as i128) as usize;
    (wrap(coordinate.z()) * WINDOW_BRICKS + wrap(coordinate.y())) * WINDOW_BRICKS
        + wrap(coordinate.x())
}

/// How much the volume holds, for tests and for a look at what a scene costs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VolumeStatistics {
    /// Windows in use: grids that have bricks told of.
    pub windows: usize,
    /// Bricks told of, by kind.
    pub empty: usize,
    pub full: usize,
    pub mixed: usize,
    /// Bricks that have voxels that block the sun in part (they are also counted above, by their bits).
    pub shaped: usize,
    /// Slots of the pool that hold a mixed brick's bits, and slots the pool has had to have at once.
    pub slots_in_use: usize,
    pub slots_reached: usize,
    /// The same of the partial pool.
    pub partial_slots_in_use: usize,
    pub partial_slots_reached: usize,
    /// Bricks that were not taken, and so are not traced: a grid when all the windows were in use, a mixed brick when the pool was full.
    pub rejected: usize,
}

/// The slots of the pool: handed out lowest first, given back to be handed out again, never more than a limit.
struct Slots {
    free: Vec<u32>,
    /// The number of the next slot never handed out, which is also how many the pool has had to have.
    next: u32,
    maximum: usize,
    in_use: usize,
}

impl Slots {
    fn take(&mut self) -> Option<u32> {
        let slot = match self.free.pop() {
            Some(slot) => slot,
            None if (self.next as usize) < self.maximum => {
                self.next += 1;
                self.next - 1
            }
            None => return None,
        };
        self.in_use += 1;
        Some(slot)
    }

    fn give(&mut self, slot: u32) {
        self.in_use -= 1;
        self.free.push(slot);
    }
}

/// The bits to write into a slot that was filled or changed.
type SlotBits = Box<[u32; OCCUPANCY_WORDS]>;

/// The shape numbers (plus one) to write into a partial slot.
type PartialBits = Box<[u32; PARTIAL_SLOT_WORDS]>;

/// The words of a partial slot that say these voxels are of these shapes.
fn pack_partials(partials: &[PartialVoxel]) -> PartialBits {
    let mut words = PartialBits::try_from(vec![0u32; PARTIAL_SLOT_WORDS].into_boxed_slice())
        .expect("a partial slot has this many words");
    for partial in partials {
        let voxel = usize::from(partial.voxel);
        words[voxel / 2] |= (u32::from(partial.shape) + 1) << (16 * (voxel % 2));
    }
    words
}

struct Window {
    key: VolumeKey,
    bricks: HashMap<BrickCoordinate, Held>,
    /// Which brick stands at a place of the table.
    places: HashMap<usize, BrickCoordinate>,
    /// The partial slot of each brick that has one.
    partials: HashMap<BrickCoordinate, u32>,
    /// The table as the graphics card should have it.
    table: Vec<u32>,
    /// The rows (of [`WINDOW_BRICKS`] entries) that differ from what was last handed over.
    dirty_rows: BTreeSet<usize>,
    /// How many bricks hold something that blocks the sun.
    solid: usize,
}

impl Window {
    fn new(key: VolumeKey) -> Self {
        Self {
            key,
            bricks: HashMap::new(),
            places: HashMap::new(),
            partials: HashMap::new(),
            table: vec![ENTRY_UNKNOWN; WINDOW_ENTRIES],
            // (A window is written whole when it is made: what the graphics card has at its place may be what an earlier grid left.)
            dirty_rows: (0..WINDOW_ENTRIES / WINDOW_BRICKS).collect(),
            solid: 0,
        }
    }

    fn write_entry(&mut self, place: usize, entry: u32) {
        if self.table[place] != entry {
            self.table[place] = entry;
            self.dirty_rows.insert(place / WINDOW_BRICKS);
        }
    }

    /// Forgets a brick: its slot is given back, its entry is unknown. Nothing if it is not there.
    fn remove(
        &mut self,
        coordinate: BrickCoordinate,
        slots: &mut Slots,
        pending: &mut HashMap<u32, SlotBits>,
        partial_slots: &mut Slots,
        pending_partials: &mut HashMap<u32, PartialBits>,
    ) {
        let Some(held) = self.bricks.remove(&coordinate) else {
            return;
        };
        let place = table_index(coordinate);
        self.places.remove(&place);
        if let Held::Slot(slot) = held {
            slots.give(slot);
            pending.remove(&slot);
        }
        let partial = self.partials.remove(&coordinate);
        if let Some(slot) = partial {
            partial_slots.give(slot);
            pending_partials.remove(&slot);
        }
        if held.blocks_something() {
            self.solid -= 1;
        }
        self.write_entry(place, ENTRY_UNKNOWN);
    }
}

/// A run of entries of a window's table to write: `first` is the entry's number in the window's table.
pub(crate) struct TableWrite {
    pub window: usize,
    pub first: usize,
    pub entries: Vec<u32>,
}

/// The bits of a slot to write.
pub(crate) struct SlotWrite {
    pub slot: u32,
    pub words: SlotBits,
}

/// The shape numbers of a partial slot to write.
pub(crate) struct PartialWrite {
    pub slot: u32,
    pub words: PartialBits,
}

/// What changed since the last time, to be written into the buffers.
#[derive(Default)]
pub(crate) struct Changes {
    pub tables: Vec<TableWrite>,
    /// By slot, lowest first.
    pub slots: Vec<SlotWrite>,
    pub partial_slots: Vec<PartialWrite>,
}

/// The windows and the pool.
pub(crate) struct Volume {
    windows: [Option<Window>; MAXIMUM_WINDOWS],
    slots: Slots,
    /// The slots whose bits were set since they were last handed over.
    pending: HashMap<u32, SlotBits>,
    /// The partial slots, and those whose numbers were set since they were last handed over.
    partial_slots: Slots,
    pending_partials: HashMap<u32, PartialBits>,
    rejected: usize,
}

impl Volume {
    /// A volume whose pool has room for `maximum_slots` mixed bricks (and the partial pool, for as much memory, a sixteenth as many bricks that have partial voxels).
    pub(crate) fn new(maximum_slots: usize) -> Self {
        Self {
            windows: std::array::from_fn(|_| None),
            slots: Slots {
                free: Vec::new(),
                next: 0,
                maximum: maximum_slots,
                in_use: 0,
            },
            pending: HashMap::new(),
            partial_slots: Slots {
                free: Vec::new(),
                next: 0,
                maximum: maximum_slots / SLOTS_PER_PARTIAL_SLOT,
                in_use: 0,
            },
            pending_partials: HashMap::new(),
            rejected: 0,
        }
    }

    fn index_of(&self, key: VolumeKey) -> Option<usize> {
        self.windows
            .iter()
            .position(|window| window.as_ref().is_some_and(|window| window.key == key))
    }

    /// The window of the grid, made if the grid has none and there is a window to spare.
    fn window_for(&mut self, key: VolumeKey) -> Option<usize> {
        if let Some(index) = self.index_of(key) {
            return Some(index);
        }
        let index = self.windows.iter().position(Option::is_none)?;
        self.windows[index] = Some(Window::new(key));
        Some(index)
    }

    fn free_if_empty(&mut self, index: usize) {
        if self.windows[index]
            .as_ref()
            .is_some_and(|window| window.bricks.is_empty())
        {
            self.windows[index] = None;
        }
    }

    /// Tells that the brick of a grid is as `occupancy` says. A brick that stood at the same place of the table a window away is forgotten.
    pub(crate) fn set(
        &mut self,
        key: VolumeKey,
        coordinate: BrickCoordinate,
        occupancy: &BrickOccupancy,
    ) {
        let Some(index) = self.window_for(key) else {
            self.rejected += 1;
            return;
        };
        let window = self.windows[index]
            .as_mut()
            .expect("the window was just found");
        let place = table_index(coordinate);
        if let Some(&other) = window.places.get(&place)
            && other != coordinate
        {
            window.remove(
                other,
                &mut self.slots,
                &mut self.pending,
                &mut self.partial_slots,
                &mut self.pending_partials,
            );
        }
        let held = match occupancy {
            BrickOccupancy::Empty => Held::Empty,
            BrickOccupancy::Full => Held::Full,
            BrickOccupancy::Mixed(words) | BrickOccupancy::Shaped { words, .. } => {
                let own = match window.bricks.get(&coordinate) {
                    Some(&Held::Slot(slot)) => Some(slot),
                    _ => None,
                };
                match own.or_else(|| self.slots.take()) {
                    Some(slot) => {
                        self.pending.insert(slot, words.clone());
                        Held::Slot(slot)
                    }
                    None => {
                        // No room for its bits: it is not known, and so not traced, rather than traced wrongly.
                        self.rejected += 1;
                        window.remove(
                            coordinate,
                            &mut self.slots,
                            &mut self.pending,
                            &mut self.partial_slots,
                            &mut self.pending_partials,
                        );
                        self.free_if_empty(index);
                        return;
                    }
                }
            }
        };
        // The partial slot: the brick keeps its own while it has partial voxels, takes one when it gets them and gives it back when it loses them. With no room for one the
        // brick is traced as its bits say, and its partial voxels are counted as not told of.
        let had_partial = window.partials.get(&coordinate).copied();
        let partial = match occupancy.partials() {
            [] => None,
            partials => match had_partial.or_else(|| self.partial_slots.take()) {
                Some(slot) => {
                    self.pending_partials.insert(slot, pack_partials(partials));
                    Some(slot)
                }
                None => {
                    self.rejected += 1;
                    None
                }
            },
        };
        match (had_partial, partial) {
            (Some(old), None) => {
                self.partial_slots.give(old);
                self.pending_partials.remove(&old);
                window.partials.remove(&coordinate);
            }
            (_, Some(slot)) => {
                window.partials.insert(coordinate, slot);
            }
            (None, None) => {}
        }
        let previous = window.bricks.insert(coordinate, held);
        match previous {
            Some(Held::Slot(old)) if held != Held::Slot(old) => {
                self.slots.give(old);
                self.pending.remove(&old);
            }
            _ => {}
        }
        // (A brick with partial voxels is `Held::Slot` whatever its bits are, so it blocks something and is counted with the others.)
        if previous.is_some_and(Held::blocks_something) {
            window.solid -= 1;
        }
        if held.blocks_something() {
            window.solid += 1;
        }
        window.places.insert(place, coordinate);
        let entry = held.entry() | partial.map_or(0, |slot| (slot + 1) << PARTIAL_ENTRY_SHIFT);
        window.write_entry(place, entry);
    }

    /// Tells that the brick of a grid is gone. Nothing if it was not told of, or was forgotten for one a window away.
    pub(crate) fn clear(&mut self, key: VolumeKey, coordinate: BrickCoordinate) {
        let Some(index) = self.index_of(key) else {
            return;
        };
        let window = self.windows[index].as_mut().expect("the window was found");
        window.remove(
            coordinate,
            &mut self.slots,
            &mut self.pending,
            &mut self.partial_slots,
            &mut self.pending_partials,
        );
        self.free_if_empty(index);
    }

    /// Forgets a whole grid: its bricks, its slots and its window.
    pub(crate) fn clear_grid(&mut self, key: VolumeKey) {
        let Some(index) = self.index_of(key) else {
            return;
        };
        let mut window = self.windows[index].take().expect("the window was found");
        let coordinates: Vec<BrickCoordinate> = window.bricks.keys().copied().collect();
        for coordinate in coordinates {
            window.remove(
                coordinate,
                &mut self.slots,
                &mut self.pending,
                &mut self.partial_slots,
                &mut self.pending_partials,
            );
        }
    }

    /// The number of the grid's window, if it has one that holds something that blocks the sun (a grid of only empty bricks is not traced: there is nothing to find).
    pub(crate) fn window_of(&self, key: VolumeKey) -> Option<usize> {
        let index = self.index_of(key)?;
        (self.windows[index].as_ref()?.solid > 0).then_some(index)
    }

    /// The lowest and the highest brick coordinates (both included) of the bricks of the grid's window that hold something that blocks the sun, if there are any: a ray that
    /// starts farther than its length from this box cannot meet anything of the grid.
    pub(crate) fn solid_bounds(&self, key: VolumeKey) -> Option<[[i128; 3]; 2]> {
        let window = self.windows[self.index_of(key)?].as_ref()?;
        let mut bounds: Option<[[i128; 3]; 2]> = None;
        for (coordinate, held) in &window.bricks {
            if !held.blocks_something() {
                continue;
            }
            let at = [coordinate.x(), coordinate.y(), coordinate.z()];
            bounds = Some(match bounds {
                None => [at, at],
                Some([low, high]) => [
                    std::array::from_fn(|axis| low[axis].min(at[axis])),
                    std::array::from_fn(|axis| high[axis].max(at[axis])),
                ],
            });
        }
        bounds
    }

    /// Whose bricks each window holds, by window (none for a window not in use).
    pub(crate) fn window_keys(&self) -> [Option<VolumeKey>; MAXIMUM_WINDOWS] {
        std::array::from_fn(|index| self.windows[index].as_ref().map(|window| window.key))
    }

    /// How many slots the pool may have.
    pub(crate) fn maximum_slots(&self) -> usize {
        self.slots.maximum
    }

    /// Whether any brick of any window has voxels that block the sun in part.
    pub(crate) fn has_partials(&self) -> bool {
        self.windows
            .iter()
            .flatten()
            .any(|window| !window.partials.is_empty())
    }

    /// Whether any window holds something that blocks the sun.
    pub(crate) fn is_active(&self) -> bool {
        self.windows.iter().flatten().any(|window| window.solid > 0)
    }

    /// What is known of a brick of a grid.
    #[cfg(test)]
    pub(crate) fn held(&self, key: VolumeKey, coordinate: BrickCoordinate) -> Option<Held> {
        let window = self.windows[self.index_of(key)?].as_ref()?;
        window.bricks.get(&coordinate).copied()
    }

    /// How many slots the pool has to have, at least: the most it has had in use at once, so it only grows.
    pub(crate) fn slots_wanted(&self) -> usize {
        self.slots.next as usize
    }

    /// The same of the partial pool, and how many slots it may have.
    pub(crate) fn partial_slots_wanted(&self) -> usize {
        self.partial_slots.next as usize
    }

    pub(crate) fn maximum_partial_slots(&self) -> usize {
        self.partial_slots.maximum
    }

    pub(crate) fn statistics(&self) -> VolumeStatistics {
        let mut statistics = VolumeStatistics {
            slots_in_use: self.slots.in_use,
            slots_reached: self.slots.next as usize,
            partial_slots_in_use: self.partial_slots.in_use,
            partial_slots_reached: self.partial_slots.next as usize,
            rejected: self.rejected,
            ..VolumeStatistics::default()
        };
        for window in self.windows.iter().flatten() {
            statistics.windows += 1;
            statistics.shaped += window.partials.len();
            for held in window.bricks.values() {
                match held {
                    Held::Empty => statistics.empty += 1,
                    Held::Full => statistics.full += 1,
                    Held::Slot(_) => statistics.mixed += 1,
                }
            }
        }
        statistics
    }

    /// What changed since the last call: the rows of the tables that differ (neighbouring rows in one run) and the bits of the slots that were set.
    pub(crate) fn drain(&mut self) -> Changes {
        let mut changes = Changes::default();
        for (index, window) in self.windows.iter_mut().enumerate() {
            let Some(window) = window else {
                continue;
            };
            let rows = std::mem::take(&mut window.dirty_rows);
            let mut rows = rows.into_iter().peekable();
            while let Some(first) = rows.next() {
                let mut last = first;
                while rows.next_if_eq(&(last + 1)).is_some() {
                    last += 1;
                }
                changes.tables.push(TableWrite {
                    window: index,
                    first: first * WINDOW_BRICKS,
                    entries: window.table[first * WINDOW_BRICKS..(last + 1) * WINDOW_BRICKS]
                        .to_vec(),
                });
            }
        }
        let mut slots: Vec<SlotWrite> = self
            .pending
            .drain()
            .map(|(slot, words)| SlotWrite { slot, words })
            .collect();
        slots.sort_by_key(|write| write.slot);
        changes.slots = slots;
        let mut partial_slots: Vec<PartialWrite> = self
            .pending_partials
            .drain()
            .map(|(slot, words)| PartialWrite { slot, words })
            .collect();
        partial_slots.sort_by_key(|write| write.slot);
        changes.partial_slots = partial_slots;
        changes
    }
}

#[cfg(test)]
mod tests;
