//! The voxels that block the sun, in the form the shader walks them (`docs/specs/contact-trace-spec.md`, section 3.2). This is the memory's bookkeeping, on the CPU; the buffers
//! it describes are the renderer's.
//!
//! Each grid that has bricks in view has its bricks kept here, and the nearest few (at most [`MAXIMUM_WINDOWS`]) have a **window**, which the graphics card has: a table of 64 by 64 by 64 entries that holds one entry for each brick, at the place of the brick's coordinates modulo 64.
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
    /// Windows in use: grids that have a window of the graphics card's, and so are traced when they hold something that blocks the sun.
    pub windows: usize,
    /// Grids that have bricks told of, with a window or without: the ones without are kept to be given one when they are nearer than another.
    pub grids: usize,
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
    /// The window of the graphics card's this grid has, if it has one: the books of a grid that has none are kept, and its table is not (it is made when it is given one).
    slot: Option<usize>,
    bricks: HashMap<BrickCoordinate, Held>,
    /// Which brick stands at a place of the table.
    places: HashMap<usize, BrickCoordinate>,
    /// The partial slot of each brick that has one.
    partials: HashMap<BrickCoordinate, u32>,
    /// The table as the graphics card should have it: empty while the grid has no window.
    table: Vec<u32>,
    /// The rows (of [`WINDOW_BRICKS`] entries) that differ from what was last handed over.
    dirty_rows: BTreeSet<usize>,
    /// How many bricks hold something that blocks the sun.
    solid: usize,
    /// The box of those bricks, kept as bricks come and go (it is asked for every frame, and the bricks are many): `None` where it is not known and is to be found by looking at
    /// them all, the inner `None` for no brick at all.
    bounds: std::cell::Cell<Option<Option<[[i128; 3]; 2]>>>,
}

impl Window {
    fn new(key: VolumeKey) -> Self {
        Self {
            key,
            slot: None,
            bricks: HashMap::new(),
            places: HashMap::new(),
            partials: HashMap::new(),
            table: Vec::new(),
            dirty_rows: BTreeSet::new(),
            solid: 0,
            bounds: std::cell::Cell::new(Some(None)),
        }
    }

    /// Notes that the brick at `coordinate` came to hold something that blocks the sun.
    fn solid_added(&self, coordinate: BrickCoordinate) {
        if let Some(known) = self.bounds.get() {
            let at = [coordinate.x(), coordinate.y(), coordinate.z()];
            self.bounds.set(Some(Some(match known {
                None => [at, at],
                Some([low, high]) => [
                    std::array::from_fn(|axis| low[axis].min(at[axis])),
                    std::array::from_fn(|axis| high[axis].max(at[axis])),
                ],
            })));
        }
    }

    /// Notes that the brick at `coordinate` held something that blocks the sun and does not: the box is not known any more if the brick was on its edge.
    fn solid_removed(&self, coordinate: BrickCoordinate) {
        if let Some(Some([low, high])) = self.bounds.get() {
            let at = [coordinate.x(), coordinate.y(), coordinate.z()];
            if (0..3).any(|axis| at[axis] == low[axis] || at[axis] == high[axis]) {
                self.bounds.set(None);
            }
        }
    }

    /// The lowest and the highest brick that hold something that blocks the sun, if there are any.
    fn solid_bounds(&self) -> Option<[[i128; 3]; 2]> {
        if let Some(known) = self.bounds.get() {
            return known;
        }
        let mut bounds: Option<[[i128; 3]; 2]> = None;
        for (coordinate, held) in &self.bricks {
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
        self.bounds.set(Some(bounds));
        bounds
    }

    /// What the table says of a brick that is held.
    fn entry_of(&self, coordinate: BrickCoordinate, held: Held) -> u32 {
        held.entry()
            | self
                .partials
                .get(&coordinate)
                .map_or(0, |slot| (slot + 1) << PARTIAL_ENTRY_SHIFT)
    }

    /// Gives the grid the window of the graphics card's of this number: its table is made from its bricks and all of it is to be written (what the graphics card has at that
    /// place is what the grid before it left).
    fn give_slot(&mut self, slot: usize) {
        self.slot = Some(slot);
        let mut table = vec![ENTRY_UNKNOWN; WINDOW_ENTRIES];
        for (&coordinate, &held) in &self.bricks {
            table[table_index(coordinate)] = self.entry_of(coordinate, held);
        }
        self.table = table;
        self.dirty_rows = (0..WINDOW_ENTRIES / WINDOW_BRICKS).collect();
    }

    /// Takes the grid's window away (its bricks are kept) and says which it was.
    fn take_slot(&mut self) -> Option<usize> {
        self.table = Vec::new();
        self.dirty_rows.clear();
        self.slot.take()
    }

    fn write_entry(&mut self, place: usize, entry: u32) {
        if self.slot.is_none() {
            return;
        }
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
            self.solid_removed(coordinate);
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

/// The grids' bricks, the windows of the graphics card's and the pool.
pub(crate) struct Volume {
    /// Every grid that has bricks told of, with the books of them.
    grids: HashMap<VolumeKey, Window>,
    /// Whose bricks each window of the graphics card's holds: a grid that is held here has its table written there.
    holders: [Option<VolumeKey>; MAXIMUM_WINDOWS],
    slots: Slots,
    /// The slots whose bits were set since they were last handed over.
    pending: HashMap<u32, SlotBits>,
    /// The partial slots, and those whose numbers were set since they were last handed over.
    partial_slots: Slots,
    pending_partials: HashMap<u32, PartialBits>,
    rejected: usize,
}

/// A grid that has no window takes the window of one that has only if it is nearer than half its distance, less this many voxels: so that two grids that are about as near do not
/// take it from each other, and every time, and have the whole of a table written each time.
const NEARER_BY_MORE_THAN_HALF_LESS: f64 = 64.0;

impl Volume {
    /// A volume whose pool has room for `maximum_slots` mixed bricks (and the partial pool, for as much memory, a sixteenth as many bricks that have partial voxels).
    pub(crate) fn new(maximum_slots: usize) -> Self {
        Self {
            grids: HashMap::new(),
            holders: [None; MAXIMUM_WINDOWS],
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

    /// A window of the graphics card's that nothing holds.
    fn free_window(&self) -> Option<usize> {
        self.holders.iter().position(Option::is_none)
    }

    /// Gives the grid the window, which it takes from no one (the one it takes from has lost it already).
    fn hand_over(&mut self, key: VolumeKey, slot: usize) {
        self.holders[slot] = Some(key);
        self.grids
            .get_mut(&key)
            .expect("the grid is there")
            .give_slot(slot);
    }

    /// Forgets a grid that holds no brick any more, and frees its window.
    fn free_if_empty(&mut self, key: VolumeKey) {
        if self
            .grids
            .get(&key)
            .is_some_and(|window| window.bricks.is_empty())
        {
            self.forget(key);
        }
    }

    fn forget(&mut self, key: VolumeKey) {
        self.grids.remove(&key);
        for holder in &mut self.holders {
            if *holder == Some(key) {
                *holder = None;
            }
        }
    }

    /// Tells that the brick of a grid is as `occupancy` says. A brick that stood at the same place of the table a window away is forgotten. A grid that is told of for the first
    /// time has a window if there is one that nothing holds; if there is not, it is kept without one until [`Volume::rebalance`] gives it one.
    pub(crate) fn set(
        &mut self,
        key: VolumeKey,
        coordinate: BrickCoordinate,
        occupancy: &BrickOccupancy,
    ) {
        if let std::collections::hash_map::Entry::Vacant(vacant) = self.grids.entry(key) {
            vacant.insert(Window::new(key));
            if let Some(slot) = self.free_window() {
                self.hand_over(key, slot);
            }
        }
        let window = self.grids.get_mut(&key).expect("the grid was just made");
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
                        self.free_if_empty(key);
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
            window.solid_removed(coordinate);
        }
        if held.blocks_something() {
            window.solid += 1;
            window.solid_added(coordinate);
        }
        window.places.insert(place, coordinate);
        let entry = window.entry_of(coordinate, held);
        window.write_entry(place, entry);
    }

    /// Tells that the brick of a grid is gone. Nothing if it was not told of, or was forgotten for one a window away.
    pub(crate) fn clear(&mut self, key: VolumeKey, coordinate: BrickCoordinate) {
        let Some(window) = self.grids.get_mut(&key) else {
            return;
        };
        window.remove(
            coordinate,
            &mut self.slots,
            &mut self.pending,
            &mut self.partial_slots,
            &mut self.pending_partials,
        );
        self.free_if_empty(key);
    }

    /// Forgets a whole grid: its bricks, its slots and its window.
    pub(crate) fn clear_grid(&mut self, key: VolumeKey) {
        let Some(mut window) = self.grids.remove(&key) else {
            return;
        };
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
        self.forget(key);
    }

    /// Gives the windows that the graphics card has to the grids that are nearest, by what `distance` says of each (in voxels, from the camera to the nearest of the grid's
    /// bricks that block the sun): a grid that holds something that blocks the sun and has no window takes a free one; if there is none, the window of the grid that is
    /// farthest of those that have one (a grid that holds nothing that blocks the sun is at an infinite distance), if the grid that waits is nearer than half of that distance less
    /// [`NEARER_BY_MORE_THAN_HALF_LESS`] voxels. What a grid that loses its window held is kept, and the grid has a window again when it is the nearer. A window that is given is
    /// written whole.
    pub(crate) fn rebalance(&mut self, distance: &dyn Fn(VolumeKey) -> f64) {
        let mut waiting: Vec<(f64, VolumeKey)> = self
            .grids
            .values()
            .filter(|window| window.slot.is_none() && window.solid > 0)
            .map(|window| (distance(window.key), window.key))
            .collect();
        waiting.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        for (wanted_distance, key) in waiting {
            if let Some(slot) = self.free_window() {
                self.hand_over(key, slot);
                continue;
            }
            // The holder that is farthest.
            let farthest = self
                .holders
                .iter()
                .enumerate()
                .filter_map(|(slot, holder)| {
                    let window = &self.grids[&(*holder)?];
                    let held_distance = if window.solid == 0 {
                        f64::INFINITY
                    } else {
                        distance(window.key)
                    };
                    Some((held_distance, slot))
                })
                .max_by(|a, b| a.0.total_cmp(&b.0).then(b.1.cmp(&a.1)));
            let Some((held_distance, slot)) = farthest else {
                break;
            };
            if wanted_distance * 2.0 + NEARER_BY_MORE_THAN_HALF_LESS >= held_distance {
                // (The grids that wait are in order of distance: the ones after this are not nearer.)
                break;
            }
            let loser = self.holders[slot].expect("the window is held");
            self.grids
                .get_mut(&loser)
                .expect("the grid is there")
                .take_slot();
            self.hand_over(key, slot);
        }
    }

    /// The number of the grid's window, if it has one and it holds something that blocks the sun (a grid of only empty bricks is not traced: there is nothing to find).
    pub(crate) fn window_of(&self, key: VolumeKey) -> Option<usize> {
        let window = self.grids.get(&key)?;
        (window.solid > 0).then_some(window.slot?)
    }

    /// The lowest and the highest brick coordinates (both included) of the bricks of the grid that hold something that blocks the sun, if there are any, whether or not the grid
    /// has a window: a ray that starts farther than its length from this box cannot meet anything of the grid.
    pub(crate) fn solid_bounds(&self, key: VolumeKey) -> Option<[[i128; 3]; 2]> {
        self.grids.get(&key)?.solid_bounds()
    }

    /// Every grid that has bricks told of, with or without a window.
    pub(crate) fn grid_keys(&self) -> Vec<VolumeKey> {
        let mut keys: Vec<VolumeKey> = self.grids.keys().copied().collect();
        keys.sort();
        keys
    }

    /// Whose bricks each window holds, by window (none for a window not in use).
    pub(crate) fn window_keys(&self) -> [Option<VolumeKey>; MAXIMUM_WINDOWS] {
        self.holders
    }

    /// How many slots the pool may have.
    pub(crate) fn maximum_slots(&self) -> usize {
        self.slots.maximum
    }

    /// Whether any brick of any grid that has a window has voxels that block the sun in part.
    pub(crate) fn has_partials(&self) -> bool {
        self.holders
            .iter()
            .flatten()
            .any(|key| !self.grids[key].partials.is_empty())
    }

    /// Whether any grid that has a window holds something that blocks the sun.
    pub(crate) fn is_active(&self) -> bool {
        self.holders
            .iter()
            .flatten()
            .any(|key| self.grids[key].solid > 0)
    }

    /// What is known of a brick of a grid.
    #[cfg(test)]
    pub(crate) fn held(&self, key: VolumeKey, coordinate: BrickCoordinate) -> Option<Held> {
        self.grids.get(&key)?.bricks.get(&coordinate).copied()
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
            windows: self.holders.iter().flatten().count(),
            grids: self.grids.len(),
            slots_in_use: self.slots.in_use,
            slots_reached: self.slots.next as usize,
            partial_slots_in_use: self.partial_slots.in_use,
            partial_slots_reached: self.partial_slots.next as usize,
            rejected: self.rejected,
            ..VolumeStatistics::default()
        };
        for window in self.grids.values() {
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

    /// What changed since the last call: the rows of the tables of the windows that are held that differ (neighbouring rows in one run) and the bits of the slots that were set.
    pub(crate) fn drain(&mut self) -> Changes {
        let mut changes = Changes::default();
        for (index, holder) in self.holders.iter().enumerate() {
            let Some(key) = holder else {
                continue;
            };
            let window = self.grids.get_mut(key).expect("the grid is there");
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
