//! A spike: block light flooded at several scales at once, in cells of 1, 2, 4 ... voxels along each
//! side. It is the light rule of [`crate::lighting`] with the cell size as a parameter, to see what
//! light at several scales in one world is like (what it costs, how it looks, whether the rule's
//! shape holds), not a decision about how Cave is lit. A [`LightScheme`] says how many levels the
//! light has and how large the cells get: 32 levels with cells up to 4 voxels, 16 up to 8, 8 up to 16,
//! 4 up to 32 (the same reach, 31 cells at the finest scale or 3 at the coarsest, so the schemes trade
//! the number of levels against how many scales there are).
//!
//! At scale `k` the world is a grid of cells of `2^k` voxels along each side. A cell is named by
//! its anchor, its first voxel (the one with the smallest coordinates), and what a cell holds is kept
//! in the anchor voxel of its cell, in a channel of its own for each scale: the level it gives to the
//! cell on the other side of each of its six faces (a cell's own level is the greatest of them).
//!
//! Light goes through the air, so a cell is looked into voxel by voxel. A voxel is *open* if light can
//! get into it (it is not a whole block that lets none through, and can be read). Light passes
//! between two cells across a face where an open voxel of one touches a voxel of the other that is
//! open or gives off light (a lamp lights every face of itself), at the same place in the face. Inside
//! a cell it goes from face to face along open voxels that touch: a *component* is a set of open
//! voxels joined face to face, and light that comes in at a face goes out by another only if one
//! component reaches both, at the place it came in. So a cell that is mostly ground, with air over
//! it that touches the air of the cell above, is lit; one that a lamp's cell touches only by stone is
//! not; and a wall across a cell with a hole in it lights what is beyond the hole. The rule, from the
//! cells as they were at the start of the tick:
//!
//! - A cell's *source* is the brightest light given off by any voxel in it, the same at every scale:
//!   a block's emission scaled to the scheme's levels (see [`emission_level`]). A lamp is the greatest
//!   level at every scale, and a cell is as bright as the brightest thing in it, however big the cell.
//! - What comes in across a face is what the cell on the other side gives across it, or its own source
//!   if that is more, if they touch (as above).
//! - A cell gives across a face its source, if it has anything on that face's layer that gives light;
//!   and one less than what came in at another face that one component joins to it. Never back the way
//!   it came in, so two cells do not hold each other up.
//! - Light travels one cell a tick, so coarse light spreads faster in metres: in cells of 8, 8
//!   voxels a tick.
//!
//! What is drawn is the plain light and each scale's level at each voxel, blended (see
//! `world_view::combined_light`). A cell has one level for all its air, however many components it has.
//! What a cell gives across a face is one level for every place on that face where the cells touch, so
//! light in one pocket of air passes sideways into a second pocket of the next cell that touches the
//! same face (it is not kept to the places the light is at: that would need a level for each place).

use std::collections::HashMap;
use std::sync::Arc;

use block_models::BlockTable;
use lattice::ActiveVoxels;
use namespaced_id::NamespacedId;
use voxel_space::{FaceDirection, LevelIndex, VoxelCoordinate};
use world_storage::Section;
use world_view::LevelFormat;

/// The most scales any scheme has: cells of 1, 2, 4, 8, 16 and 32.
pub const MAXIMUM_SCALES: usize = 6;

/// How many levels a light has and how big its cells get.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LightScheme {
    code: u8,
}

impl LightScheme {
    /// 32 levels, cells of 1, 2 and 4 voxels.
    pub const LEVELS_32: Self = Self { code: 0 };
    /// 16 levels, cells of 1, 2, 4 and 8 voxels.
    pub const LEVELS_16: Self = Self { code: 1 };
    /// 8 levels, cells of 1 to 16 voxels.
    pub const LEVELS_8: Self = Self { code: 2 };
    /// 4 levels, cells of 1 to 32 voxels.
    pub const LEVELS_4: Self = Self { code: 3 };

    pub const ALL: [Self; 4] = [
        Self::LEVELS_32,
        Self::LEVELS_16,
        Self::LEVELS_8,
        Self::LEVELS_4,
    ];

    /// The scheme with this many levels (32, 16, 8 or 4).
    pub fn from_levels(levels: u32) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|scheme| scheme.levels() == levels)
    }

    /// The scheme a code (as kept in a world and in a cell's word) stands for.
    pub fn from_code(code: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|scheme| scheme.code == code)
    }

    pub fn code(self) -> u8 {
        self.code
    }

    /// How many levels there are, counting 0.
    pub fn levels(self) -> u32 {
        32 >> self.code
    }

    /// The greatest level.
    pub fn maximum_level(self) -> u8 {
        (31 >> self.code) as u8
    }

    /// How many scales there are.
    pub fn scales(self) -> usize {
        3 + usize::from(self.code)
    }

    /// The width of the biggest cells, in voxels.
    pub fn largest_cell(self) -> i64 {
        1 << (self.scales() - 1)
    }

    pub fn name(self) -> String {
        format!(
            "{} levels, cells up to {}",
            self.levels(),
            self.largest_cell()
        )
    }
}

/// How a cell's word holds what it gives: five bits for each face, and above them two bits for the
/// scheme, so that what draws it knows how many levels there are.
pub const LEVEL_FORMAT: LevelFormat = LevelFormat {
    field_bits: 5,
    field_count: 6,
    code_shift: 30,
    code_bits: 2,
};

/// The weights of the scales in drawing, from the finest: how bright each channel is at its greatest
/// level, where 255 is full brightness (and a channel with more reaches full brightness before its
/// greatest level). Fine cells count for more than coarse: light that is nearby is told apart by fine
/// cells, and the coarse ones, many cells wide, are a faint glow behind.
pub const FULL_BRIGHTNESS: [u16; MAXIMUM_SCALES] = [382, 255, 178, 128, 89, 64];

/// What a block's emission is divided by, as a share of the greatest level: an emission of 60 or more
/// is the greatest level.
const EMISSION_FOR_GREATEST_LEVEL: u32 = 60;

/// The level of the light a block of this emission gives off: the greatest level for a lamp (64), two
/// thirds of it for a fire (40), and 0 for a block that gives off none.
pub fn emission_level(emission: u8, scheme: LightScheme) -> u8 {
    let greatest = u32::from(scheme.maximum_level());
    ((u32::from(emission) * greatest).div_ceil(EMISSION_FOR_GREATEST_LEVEL)).min(greatest) as u8
}

/// The cells each scale looks at in a tick at most: coarse cells cost more to look at (a cell of 8
/// reads 512 voxels and a cell of 32 reads 32 768), so a scale's share of the voxels looked at in a
/// tick buys fewer of them. About 20 ms of looking at voxels.
pub fn default_budgets(scheme: LightScheme) -> Vec<usize> {
    const VOXELS_PER_SCALE_AND_TICK: i64 = 150_000;
    (0..scheme.scales())
        .map(|scale| {
            let volume = 1i64 << (3 * scale);
            (VOXELS_PER_SCALE_AND_TICK / (2 * volume + 14)).max(1) as usize
        })
        .collect()
}

/// Which light a world has. Switching clears the light of the other (see `CaveLogic`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightModel {
    /// One light channel, a voxel a cell with 255 levels (see [`crate::lighting`]).
    Classic,
    /// Several scales of cells (this module).
    MultiScale(LightScheme),
}

impl LightModel {
    pub fn name(self) -> String {
        match self {
            Self::Classic => "classic".to_owned(),
            Self::MultiScale(scheme) => format!("multiscale ({})", scheme.name()),
        }
    }

    /// The model a command asks for: `classic`, `multiscale` (16 levels) or `multiscale 32|16|8|4`.
    pub fn from_arguments(arguments: &[&str]) -> Option<Self> {
        match arguments {
            ["classic"] => Some(Self::Classic),
            ["multiscale"] => Some(Self::MultiScale(LightScheme::LEVELS_16)),
            ["multiscale", levels] => {
                LightScheme::from_levels(levels.parse().ok()?).map(Self::MultiScale)
            }
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
            payload: match self {
                Self::Classic => vec![0],
                Self::MultiScale(scheme) => vec![1, scheme.code()],
            },
        }
    }

    /// The choice saved in a world's sections: classic if there is none, or one this program cannot
    /// read. (A world saved before the schemes existed says multiscale alone: 16 levels.)
    pub fn read_section(sections: &[Section]) -> Self {
        sections
            .iter()
            .find(|section| section.identifier == Self::section_id() && section.version == 1)
            .and_then(|section| match section.payload[..] {
                [0] => Some(Self::Classic),
                [1] => Some(Self::MultiScale(LightScheme::LEVELS_16)),
                [1, code] => LightScheme::from_code(code).map(Self::MultiScale),
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

/// The identifier of the channel that holds the words of a scale.
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

    /// What is stored at the anchor of a cell of a scale (see [`face_output`]); 0 where there is
    /// nothing to read.
    fn word(&mut self, scale: usize, level: LevelIndex, anchor: VoxelCoordinate) -> u32;

    /// Stores what a cell holds. Only called for cells whose anchor was just read.
    fn set_word(&mut self, scale: usize, level: LevelIndex, anchor: VoxelCoordinate, value: u32);
}

/// What a cell stores: for each of its six faces (by [`FaceDirection::index`]) the level it gives to
/// the cell on the other side of that face, five bits each, and above them which scheme it is. A cell
/// that gives no light is 0, whatever the scheme (so a cell never looked at and one that has gone dark
/// are the same, and a cell is only changed when its light is).
fn pack(scheme: LightScheme, outputs: [u8; 6]) -> u32 {
    if outputs == [0; 6] {
        return 0;
    }
    let mut word = u32::from(scheme.code()) << LEVEL_FORMAT.code_shift;
    for (face, output) in outputs.into_iter().enumerate() {
        word |= u32::from(output) << (5 * face);
    }
    word
}

/// The level a stored cell gives to its neighbour across `face`.
pub fn face_output(word: u32, face: FaceDirection) -> u8 {
    ((word >> (5 * usize::from(face.index()))) & 0x1F) as u8
}

/// The level a stored cell holds: the greatest it gives.
pub fn level_of_word(word: u32) -> u8 {
    FaceDirection::ALL
        .into_iter()
        .map(|face| face_output(word, face))
        .max()
        .unwrap_or(0)
}

/// Whether a stored cell gives any light at all.
pub fn gives_light(word: u32) -> bool {
    word & ((1 << LEVEL_FORMAT.code_shift) - 1) != 0
}

/// A set of the voxels of one face layer of a cell, as bits, by position in the layer, in as many
/// words as the layer needs.
type Layer = Vec<u64>;

fn words_for(width: usize) -> usize {
    (width * width).div_ceil(64)
}

fn set_bit(layer: &mut [u64], bit: usize) {
    layer[bit / 64] |= 1 << (bit % 64);
}

fn is_empty(layer: &[u64]) -> bool {
    layer.iter().all(|&word| word == 0)
}

fn intersects(first: &[u64], second: &[u64]) -> bool {
    first.iter().zip(second).any(|(&a, &b)| a & b != 0)
}

fn intersection(first: &[u64], second: &[u64]) -> Layer {
    first.iter().zip(second).map(|(&a, &b)| a & b).collect()
}

/// What is in a cell, as far as light goes.
///
/// A voxel is *open* if light can get into it: it is not a whole block that lets none through, and
/// can be read. Light passes between two cells across a face where an open voxel of one touches a
/// voxel of the other that is open or gives off light (a lamp lights every face of itself). Inside a
/// cell it goes from face to face by the cell's open voxels that touch: a *component* is a set of
/// open voxels joined face to face, and a component that reaches two faces connects them.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Contents {
    /// How many words a face's layer takes.
    words: usize,
    /// The brightest light any voxel gives off, as a level.
    source: u8,
    /// For each face, the open voxels of that layer of the cell (`words` words a face).
    open: Vec<u64>,
    /// For each face, the voxels of that layer that are open or give off light.
    gives: Vec<u64>,
    /// For each component of open voxels, the voxels of it on each face's layer.
    components: Vec<Vec<u64>>,
}

impl Contents {
    fn layer<'a>(&self, layers: &'a [u64], face: usize) -> &'a [u64] {
        &layers[face * self.words..(face + 1) * self.words]
    }
}

type CellKey = (usize, LevelIndex, VoxelCoordinate);

/// The position of a voxel with local coordinates `local` in the layer of the face on `axis`.
fn layer_bit(width: usize, axis: usize, local: [usize; 3]) -> usize {
    let (first, second) = match axis {
        0 => (local[1], local[2]),
        1 => (local[0], local[2]),
        _ => (local[0], local[1]),
    };
    first * width + second
}

/// The light rule at every scale, and the cells it has yet to look at.
#[derive(Debug)]
pub struct MultiScaleLighting {
    table: BlockTable,
    scheme: LightScheme,
    /// Cells (by anchor) whose light may be out of date, at each scale, in a fixed order.
    pending: Vec<ActiveVoxels>,
    /// What is in the cells looked into so far; a cell is forgotten when a block in it changes.
    contents_known: HashMap<CellKey, Arc<Contents>>,
}

impl MultiScaleLighting {
    /// Light for a world of these blocks, with nothing to look at: the world's light is taken to be
    /// settled.
    pub fn new(table: BlockTable, scheme: LightScheme) -> Self {
        Self {
            table,
            scheme,
            pending: (0..scheme.scales()).map(|_| ActiveVoxels::new()).collect(),
            contents_known: HashMap::new(),
        }
    }

    /// Light that carries on from cells that were still to be looked at, as when a world is loaded
    /// (one set for each of the scheme's scales).
    pub fn with_pending(
        table: BlockTable,
        scheme: LightScheme,
        pending: Vec<ActiveVoxels>,
    ) -> Self {
        assert_eq!(pending.len(), scheme.scales());
        Self {
            table,
            scheme,
            pending,
            contents_known: HashMap::new(),
        }
    }

    pub fn scheme(&self) -> LightScheme {
        self.scheme
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
        for scale in 0..self.scheme.scales() {
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
        let unlimited = vec![usize::MAX; self.scheme.scales()];
        self.step_within(world, &unlimited)
    }

    /// One tick: at each scale, works out what every pending cell should hold (no more than that
    /// scale's share of `budgets`, in their fixed order; the rest wait) from the world as it is now,
    /// then writes the ones that changed, and so has the cells around them to look at next time.
    /// Returns how many cells changed. See [`default_budgets`].
    pub fn step_within(&mut self, world: &mut impl ScaledLightWorld, budgets: &[usize]) -> usize {
        // What is in a cell changes only when a block in it does (see `voxel_changed`), so it is kept
        // from one tick to the next: a cell of 8 costs 512 reads to look into.
        let mut known = std::mem::take(&mut self.contents_known);
        let mut changed_in_all = 0;
        assert!(
            budgets.len() >= self.scheme.scales(),
            "a budget for every scale"
        );
        for (scale, &budget) in budgets.iter().enumerate().take(self.scheme.scales()) {
            let mut looked_at = self.pending[scale].take();
            if looked_at.len() > budget {
                for (level, anchor) in looked_at.split_off(budget) {
                    self.pending[scale].insert(level, anchor);
                }
            }
            let changes: Vec<_> = looked_at
                .into_iter()
                .filter_map(|(level, anchor)| {
                    let new = self.word_of(world, &mut known, scale, level, anchor);
                    (new != world.word(scale, level, anchor)).then_some((level, anchor, new))
                })
                .collect();
            for &(level, anchor, new) in &changes {
                world.set_word(scale, level, anchor, new);
                self.cell_changed(scale, level, anchor);
            }
            changed_in_all += changes.len();
        }
        self.contents_known = known;
        changed_in_all
    }

    /// What is in a cell, looked into if it is not known.
    fn contents(
        &self,
        world: &mut impl ScaledLightWorld,
        known: &mut HashMap<CellKey, Arc<Contents>>,
        scale: usize,
        level: LevelIndex,
        anchor: VoxelCoordinate,
    ) -> Arc<Contents> {
        if let Some(contents) = known.get(&(scale, level, anchor)) {
            return Arc::clone(contents);
        }
        let contents = Arc::new(self.look_into(world, scale, level, anchor));
        known.insert((scale, level, anchor), Arc::clone(&contents));
        contents
    }

    fn look_into(
        &self,
        world: &mut impl ScaledLightWorld,
        scale: usize,
        level: LevelIndex,
        anchor: VoxelCoordinate,
    ) -> Contents {
        let width = 1usize << scale;
        let words = words_for(width);
        let index = |x: usize, y: usize, z: usize| (x * width + y) * width + z;
        let mut open = vec![false; width * width * width];
        let mut source = 0u8;
        let mut emitters: Vec<[usize; 3]> = Vec::new();
        for x in 0..width {
            for y in 0..width {
                for z in 0..width {
                    let read = anchor
                        .checked_offset(x as i64, y as i64, z as i64)
                        .and_then(|at| world.block_and_state(level, at));
                    // What cannot be read lets no light in.
                    let Some((block, state)) = read else { continue };
                    let connections = self.table.connections(self.table.model_of(block, state));
                    open[index(x, y, z)] = FaceDirection::ALL
                        .iter()
                        .any(|&face| connections.is_open(face));
                    let light = emission_level(self.table.emission(block), self.scheme);
                    if light > 0 {
                        source = source.max(light);
                        emitters.push([x, y, z]);
                    }
                }
            }
        }
        // The faces whose layers a voxel is on, and its place in each.
        let faces_of = |local: [usize; 3]| -> Vec<(usize, usize)> {
            let mut faces = Vec::new();
            for face in FaceDirection::ALL {
                let axis = face.axis();
                let edge = if face.is_positive() { width - 1 } else { 0 };
                if local[axis] == edge {
                    faces.push((usize::from(face.index()), layer_bit(width, axis, local)));
                }
            }
            faces
        };
        let mut contents = Contents {
            words,
            source,
            open: vec![0; 6 * words],
            gives: vec![0; 6 * words],
            components: Vec::new(),
        };
        let mut component_of = vec![usize::MAX; open.len()];
        for x in 0..width {
            for y in 0..width {
                for z in 0..width {
                    if !open[index(x, y, z)] {
                        continue;
                    }
                    for (face, bit) in faces_of([x, y, z]) {
                        set_bit(&mut contents.open[face * words..(face + 1) * words], bit);
                        set_bit(&mut contents.gives[face * words..(face + 1) * words], bit);
                    }
                    if component_of[index(x, y, z)] != usize::MAX {
                        continue;
                    }
                    // A new component: everything open that joins this voxel, face to face.
                    let id = contents.components.len();
                    let mut layers_of_component = vec![0u64; 6 * words];
                    let mut stack = vec![[x, y, z]];
                    component_of[index(x, y, z)] = id;
                    while let Some(at) = stack.pop() {
                        for (face, bit) in faces_of(at) {
                            set_bit(
                                &mut layers_of_component[face * words..(face + 1) * words],
                                bit,
                            );
                        }
                        for direction in FaceDirection::ALL {
                            let [dx, dy, dz] = direction.offset();
                            let next = [at[0] as i64 + dx, at[1] as i64 + dy, at[2] as i64 + dz];
                            if next.iter().any(|&c| c < 0 || c >= width as i64) {
                                continue;
                            }
                            let next = next.map(|c| c as usize);
                            let at_next = index(next[0], next[1], next[2]);
                            if open[at_next] && component_of[at_next] == usize::MAX {
                                component_of[at_next] = id;
                                stack.push(next);
                            }
                        }
                    }
                    contents.components.push(layers_of_component);
                }
            }
        }
        // A voxel that gives off light lights every face of itself, open or not.
        for emitter in emitters {
            for (face, bit) in faces_of(emitter) {
                set_bit(&mut contents.gives[face * words..(face + 1) * words], bit);
            }
        }
        contents
    }

    /// What a cell should hold, given its neighbours as they are now: what it gives each neighbour.
    fn word_of(
        &self,
        world: &mut impl ScaledLightWorld,
        known: &mut HashMap<CellKey, Arc<Contents>>,
        scale: usize,
        level: LevelIndex,
        anchor: VoxelCoordinate,
    ) -> u32 {
        let own = self.contents(world, known, scale, level, anchor);
        // What comes in across each face, and where in the layer it touches.
        let mut incoming = [0u8; 6];
        let mut entering: [Layer; 6] = std::array::from_fn(|_| vec![0; own.words]);
        for face in FaceDirection::ALL {
            let Some(neighbour) = neighbour_anchor(scale, anchor, face) else {
                continue;
            };
            let next = self.contents(world, known, scale, level, neighbour);
            let (here, across) = (
                usize::from(face.index()),
                usize::from(face.opposite().index()),
            );
            let touching =
                intersection(next.layer(&next.gives, across), own.layer(&own.open, here));
            if is_empty(&touching) {
                continue;
            }
            // What it gives: what it holds for us, and at least its own source, at once.
            let word = world.word(scale, level, neighbour);
            incoming[here] = face_output(word, face.opposite()).max(next.source);
            entering[here] = touching;
        }
        let mut outputs = [0u8; 6];
        for (toward, output_slot) in outputs.iter_mut().enumerate() {
            // Its own light, to every face where it has anything that gives light.
            let mut output = if is_empty(own.layer(&own.gives, toward)) {
                0
            } else {
                own.source
            };
            // Light that came in, by a face that an open component joins to this one. Not back
            // the way it came.
            for from in (0..6).filter(|&from| from != toward && incoming[from] > 1) {
                let joined = own.components.iter().any(|component| {
                    intersects(own.layer(component, from), &entering[from])
                        && !is_empty(own.layer(component, toward))
                });
                if joined {
                    output = output.max(incoming[from] - 1);
                }
            }
            *output_slot = output;
        }
        pack(self.scheme, outputs)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use block_models::BlockDefinition;
    use proptest::prelude::*;

    use super::*;

    const SCHEME: LightScheme = LightScheme::LEVELS_16;
    const SCALES: usize = 4;

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

    /// A box of `EDGE` voxels along each side in which everything not set is air; outside it there is
    /// nothing to read.
    struct Box3 {
        edge: i64,
        blocks: HashMap<VoxelCoordinate, u32>,
        words: [HashMap<VoxelCoordinate, u32>; MAXIMUM_SCALES],
    }

    impl Default for Box3 {
        fn default() -> Self {
            Self::with_edge(EDGE)
        }
    }

    impl ScaledLightWorld for Box3 {
        fn block_and_state(&mut self, _: LevelIndex, at: VoxelCoordinate) -> Option<(u32, u32)> {
            self.contains(at)
                .then(|| (self.blocks.get(&at).copied().unwrap_or(AIR), 0))
        }

        fn word(&mut self, scale: usize, _: LevelIndex, anchor: VoxelCoordinate) -> u32 {
            self.words[scale].get(&anchor).copied().unwrap_or(0)
        }

        fn set_word(&mut self, scale: usize, _: LevelIndex, anchor: VoxelCoordinate, value: u32) {
            self.words[scale].insert(anchor, value);
        }
    }

    impl Box3 {
        fn with_edge(edge: i64) -> Self {
            Self {
                edge,
                blocks: HashMap::new(),
                words: Default::default(),
            }
        }

        fn contains(&self, at: VoxelCoordinate) -> bool {
            [at.x(), at.y(), at.z()]
                .iter()
                .all(|component| (0..self.edge).contains(component))
        }

        fn place(&mut self, voxel: VoxelCoordinate, block: u32, lighting: &mut MultiScaleLighting) {
            self.blocks.insert(voxel, block);
            lighting.voxel_changed(MAIN, voxel);
        }

        /// The block at a voxel, and the filling of a whole box of voxels with one block.
        fn fill(
            &mut self,
            from: (i64, i64, i64),
            to: (i64, i64, i64),
            block: u32,
            lighting: &mut MultiScaleLighting,
        ) {
            for x in from.0..=to.0 {
                for y in from.1..=to.1 {
                    for z in from.2..=to.2 {
                        self.place(at(x, y, z), block, lighting);
                    }
                }
            }
        }

        fn level_at(&self, scale: usize, x: i64, y: i64, z: i64) -> u8 {
            level_of_word(self.words[scale].get(&at(x, y, z)).copied().unwrap_or(0))
        }

        /// The cells of a scale that hold any light, with their levels.
        fn lit(&self, scale: usize) -> Vec<(VoxelCoordinate, u8)> {
            let mut lit: Vec<_> = self.words[scale]
                .iter()
                .map(|(&anchor, &word)| (anchor, level_of_word(word)))
                .filter(|&(_, level)| level > 0)
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

    /// Whether the voxel is open to light, worked out from the block table alone.
    fn is_open(world: &Box3, table: &BlockTable, voxel: VoxelCoordinate) -> bool {
        world.contains(voxel) && {
            let block = world.blocks.get(&voxel).copied().unwrap_or(AIR);
            let connections = table.connections(table.model_of(block, 0));
            FaceDirection::ALL
                .iter()
                .any(|&face| connections.is_open(face))
        }
    }

    /// The light the rule should settle to, worked out from the world alone, with none of the rule's
    /// bit sets or caches: for every cell the voxels of it are looked at one by one (union-find for the
    /// components, and explicit coordinates for what is on a face's layer), and the equations of the
    /// module's notes are relaxed from darkness, all cells together, until nothing changes. (The
    /// light has no loops to hold it up: each step along one costs a level, so there is one answer.)
    fn reference(
        world: &Box3,
        table: &BlockTable,
        scheme: LightScheme,
        scale: usize,
    ) -> Vec<(VoxelCoordinate, u8)> {
        let width = 1i64 << scale;
        let cells: Vec<VoxelCoordinate> = (0..world.edge / width)
            .flat_map(|x| {
                (0..world.edge / width).flat_map(move |y| {
                    (0..world.edge / width).map(move |z| at(x * width, y * width, z * width))
                })
            })
            .collect();
        let block_at = |voxel: VoxelCoordinate| world.blocks.get(&voxel).copied().unwrap_or(AIR);
        let open = |voxel: VoxelCoordinate| is_open(world, table, voxel);
        let light = |voxel: VoxelCoordinate| {
            if world.contains(voxel) {
                emission_level(table.emission(block_at(voxel)), scheme)
            } else {
                0
            }
        };
        let gives =
            |voxel: VoxelCoordinate| world.contains(voxel) && (open(voxel) || light(voxel) > 0);
        let voxels_of = |cell: VoxelCoordinate| -> Vec<VoxelCoordinate> {
            let mut voxels = Vec::new();
            for dx in 0..width {
                for dy in 0..width {
                    for dz in 0..width {
                        voxels.push(at(cell.x() + dx, cell.y() + dy, cell.z() + dz));
                    }
                }
            }
            voxels
        };
        // Whether a voxel is on the layer of a face of its cell.
        let on_layer = |cell: VoxelCoordinate, voxel: VoxelCoordinate, face: FaceDirection| {
            let axis = face.axis();
            let coordinate = [voxel.x(), voxel.y(), voxel.z()][axis];
            let start = [cell.x(), cell.y(), cell.z()][axis];
            coordinate
                == if face.is_positive() {
                    start + width - 1
                } else {
                    start
                }
        };
        // The components of open voxels of each cell: a map from voxel to a representative voxel.
        let mut parent: HashMap<VoxelCoordinate, VoxelCoordinate> = HashMap::new();
        fn find(
            parent: &mut HashMap<VoxelCoordinate, VoxelCoordinate>,
            voxel: VoxelCoordinate,
        ) -> VoxelCoordinate {
            let up = parent[&voxel];
            if up == voxel {
                return voxel;
            }
            let root = find(parent, up);
            parent.insert(voxel, root);
            root
        }
        for &cell in &cells {
            for voxel in voxels_of(cell) {
                if open(voxel) {
                    parent.insert(voxel, voxel);
                }
            }
        }
        for &cell in &cells {
            for voxel in voxels_of(cell) {
                if !open(voxel) {
                    continue;
                }
                for face in FaceDirection::ALL {
                    let [dx, dy, dz] = face.offset();
                    let next = at(voxel.x() + dx, voxel.y() + dy, voxel.z() + dz);
                    if anchor_of(scale, next) == cell && open(next) {
                        let (a, b) = (find(&mut parent, voxel), find(&mut parent, next));
                        parent.insert(a, b);
                    }
                }
            }
        }
        let source_of =
            |cell: VoxelCoordinate| voxels_of(cell).into_iter().map(light).max().unwrap_or(0);
        // The state of every cell: its level and what it gives across each face.
        let mut state: HashMap<VoxelCoordinate, (u8, [u8; 6])> =
            cells.iter().map(|&cell| (cell, (0, [0; 6]))).collect();
        for _ in 0..400 {
            let snapshot = state.clone();
            let mut next_state = snapshot.clone();
            for &cell in &cells {
                let source = source_of(cell);
                let mut incoming = [0u8; 6];
                // For each face, the voxels of this cell that light comes in at.
                let mut entering: [Vec<VoxelCoordinate>; 6] = Default::default();
                for face in FaceDirection::ALL {
                    let [dx, dy, dz] = face.offset();
                    let neighbour = at(
                        cell.x() + dx * width,
                        cell.y() + dy * width,
                        cell.z() + dz * width,
                    );
                    if !world.contains(neighbour) {
                        continue;
                    }
                    for voxel in voxels_of(cell) {
                        if !on_layer(cell, voxel, face) || !open(voxel) {
                            continue;
                        }
                        let across = at(voxel.x() + dx, voxel.y() + dy, voxel.z() + dz);
                        if gives(across) {
                            entering[usize::from(face.index())].push(voxel);
                        }
                    }
                    if !entering[usize::from(face.index())].is_empty() {
                        let outputs = snapshot[&neighbour].1;
                        incoming[usize::from(face.index())] =
                            outputs[usize::from(face.opposite().index())].max(source_of(neighbour));
                    }
                }
                let mut outputs = [0u8; 6];
                for face in FaceDirection::ALL {
                    let toward = usize::from(face.index());
                    let mut output = if voxels_of(cell)
                        .into_iter()
                        .any(|voxel| on_layer(cell, voxel, face) && gives(voxel))
                    {
                        source
                    } else {
                        0
                    };
                    for from in FaceDirection::ALL {
                        let from_index = usize::from(from.index());
                        if from_index == toward || incoming[from_index] <= 1 {
                            continue;
                        }
                        // Joined if one open component has a voxel where light comes in and an
                        // open voxel on the layer of the face it goes out by.
                        let joined = entering[from_index].iter().any(|&entry| {
                            let root = find(&mut parent, entry);
                            voxels_of(cell).into_iter().any(|voxel| {
                                open(voxel)
                                    && on_layer(cell, voxel, face)
                                    && find(&mut parent, voxel) == root
                            })
                        });
                        if joined {
                            output = output.max(incoming[from_index] - 1);
                        }
                    }
                    outputs[toward] = output;
                }
                next_state.insert(cell, (0, outputs));
            }
            if next_state == state {
                break;
            }
            state = next_state;
        }
        let mut lit: Vec<_> = state
            .into_iter()
            .map(|(cell, (_, outputs))| (cell, outputs.into_iter().max().unwrap_or(0)))
            .filter(|&(_, level)| level > 0)
            .collect();
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
    fn what_a_cell_stores_is_what_it_gives_across_each_face_and_which_scheme_it_is() {
        let word = pack(LightScheme::LEVELS_16, [1, 2, 3, 4, 5, 6]);
        for face in FaceDirection::ALL {
            assert_eq!(face_output(word, face), face.index() + 1);
        }
        // A cell's level is the most it gives.
        assert_eq!(level_of_word(word), 6);
        assert!(gives_light(word));
        assert_eq!(
            pack(LightScheme::LEVELS_32, [31; 6]) & 0x3FFF_FFFF,
            0x3FFF_FFFF
        );
        // The scheme is in the top two bits, and is not light; a cell that gives none is 0.
        let lit = pack(LightScheme::LEVELS_4, [0, 0, 0, 0, 0, 3]);
        assert_eq!(lit >> 30, 3);
        assert!(gives_light(lit));
        let empty = pack(LightScheme::LEVELS_4, [0; 6]);
        assert_eq!(empty, 0);
        assert!(!gives_light(empty));
        assert_eq!(level_of_word(empty), 0);
        assert_eq!(level_of_word(0), 0);
        // What draws a word reads the same: the level, and how many levels there can be.
        assert_eq!(LEVEL_FORMAT.decode(word), (6, 15));
        assert_eq!(
            LEVEL_FORMAT.decode(pack(LightScheme::LEVELS_32, [31, 0, 0, 0, 0, 0])),
            (31, 31)
        );
        assert_eq!(
            LEVEL_FORMAT.decode(pack(LightScheme::LEVELS_8, [0, 0, 7, 0, 0, 0])),
            (7, 7)
        );
        assert_eq!(
            LEVEL_FORMAT.decode(pack(LightScheme::LEVELS_4, [0, 0, 0, 0, 0, 3])),
            (3, 3)
        );
    }

    #[test]
    fn the_schemes_trade_levels_for_scales_and_have_codes_that_come_back() {
        let describe = |scheme: LightScheme| {
            (
                scheme.levels(),
                scheme.maximum_level(),
                scheme.scales(),
                scheme.largest_cell(),
            )
        };
        assert_eq!(describe(LightScheme::LEVELS_32), (32, 31, 3, 4));
        assert_eq!(describe(LightScheme::LEVELS_16), (16, 15, 4, 8));
        assert_eq!(describe(LightScheme::LEVELS_8), (8, 7, 5, 16));
        assert_eq!(describe(LightScheme::LEVELS_4), (4, 3, 6, 32));
        for scheme in LightScheme::ALL {
            assert_eq!(LightScheme::from_code(scheme.code()), Some(scheme));
            assert_eq!(LightScheme::from_levels(scheme.levels()), Some(scheme));
            assert!(scheme.scales() <= MAXIMUM_SCALES);
            assert_eq!(
                scheme.name(),
                format!(
                    "{} levels, cells up to {}",
                    scheme.levels(),
                    scheme.largest_cell()
                )
            );
        }
        assert_eq!(LightScheme::from_levels(5), None);
        assert_eq!(LightScheme::from_code(4), None);
    }

    #[test]
    fn a_lamp_is_the_greatest_level_in_every_scheme_and_a_fire_two_thirds_of_it() {
        let table = table();
        for scheme in LightScheme::ALL {
            let greatest = scheme.maximum_level();
            assert_eq!(emission_level(table.emission(LAMP), scheme), greatest);
            assert_eq!(emission_level(0, scheme), 0);
            assert_eq!(emission_level(255, scheme), greatest);
            // A candle (16) is about a quarter; a fire (40) two thirds, rounded up.
            let fire = emission_level(40, scheme);
            assert_eq!(u32::from(fire), (40 * u32::from(greatest)).div_ceil(60));
            assert!(emission_level(1, scheme) >= 1, "any light is some");
        }
        assert_eq!(emission_level(40, LightScheme::LEVELS_16), 10);
    }

    #[test]
    fn fine_scales_weigh_more_than_coarse_ones_and_the_finest_reaches_full_brightness_early() {
        assert!(
            FULL_BRIGHTNESS.windows(2).all(|pair| pair[0] > pair[1]),
            "{FULL_BRIGHTNESS:?}"
        );
        // The finest is over 255: it is as bright as can be before its greatest level. The coarsest is a
        // faint glow even at its greatest.
        assert!(FULL_BRIGHTNESS[0] > 255);
        assert!(FULL_BRIGHTNESS[MAXIMUM_SCALES - 1] < 128);
        // Weights come with every scale a scheme can have.
        assert_eq!(FULL_BRIGHTNESS.len(), MAXIMUM_SCALES);
    }

    #[test]
    fn budgets_buy_fewer_cells_at_coarser_scales_and_never_none() {
        for scheme in LightScheme::ALL {
            let budgets = default_budgets(scheme);
            assert_eq!(budgets.len(), scheme.scales());
            assert!(
                budgets.windows(2).all(|pair| pair[0] > pair[1]),
                "{budgets:?}"
            );
            assert!(budgets.iter().all(|&budget| budget >= 1));
        }
        assert_eq!(
            default_budgets(LightScheme::LEVELS_16),
            [9375, 5000, 1056, 144]
        );
    }

    #[test]
    fn the_light_model_is_asked_for_by_name_and_levels_and_saved_with_its_scheme() {
        let arguments =
            |text: &str| LightModel::from_arguments(&text.split_whitespace().collect::<Vec<_>>());
        assert_eq!(arguments("classic"), Some(LightModel::Classic));
        assert_eq!(
            arguments("multiscale"),
            Some(LightModel::MultiScale(LightScheme::LEVELS_16))
        );
        assert_eq!(
            arguments("multiscale 32"),
            Some(LightModel::MultiScale(LightScheme::LEVELS_32))
        );
        assert_eq!(
            arguments("multiscale 4"),
            Some(LightModel::MultiScale(LightScheme::LEVELS_4))
        );
        for bad in [
            "",
            "multiscale 5",
            "multiscale x",
            "classic 8",
            "multiscale 8 8",
            "sideways",
        ] {
            assert_eq!(arguments(bad), None, "{bad:?}");
        }
        for model in std::iter::once(LightModel::Classic)
            .chain(LightScheme::ALL.into_iter().map(LightModel::MultiScale))
        {
            assert_eq!(LightModel::read_section(&[model.to_section()]), model);
        }
        // Nothing saved, something unreadable, and a world saved before schemes (multiscale alone).
        assert_eq!(LightModel::read_section(&[]), LightModel::Classic);
        let saved = |payload: Vec<u8>| Section {
            identifier: LightModel::section_id(),
            version: 1,
            payload,
        };
        assert_eq!(
            LightModel::read_section(&[saved(vec![9])]),
            LightModel::Classic
        );
        assert_eq!(
            LightModel::read_section(&[saved(vec![1, 9])]),
            LightModel::Classic
        );
        assert_eq!(
            LightModel::read_section(&[saved(vec![1])]),
            LightModel::MultiScale(LightScheme::LEVELS_16)
        );
        assert_eq!(
            LightModel::MultiScale(LightScheme::LEVELS_8).name(),
            "multiscale (8 levels, cells up to 16)"
        );
    }

    #[test]
    fn a_change_wakes_the_cell_that_holds_it_and_its_six_neighbours_at_every_scale() {
        let mut lighting = MultiScaleLighting::new(table(), SCHEME);
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
        let mut lighting = MultiScaleLighting::new(table(), SCHEME);
        world.place(at(8, 8, 8), LAMP, &mut lighting);
        settle(&mut lighting, &mut world);
        // Scale 0: the lamp is a whole block and gives 15 to what is around it:
        // 14 beside it, one less for each voxel further, 8 at the far side of the box.
        assert_eq!(
            world.level_at(0, 8, 8, 8),
            15,
            "a lamp gives the greatest level to every side"
        );
        assert_eq!(world.level_at(0, 9, 8, 8), 14);
        assert_eq!(world.level_at(0, 10, 8, 8), 13);
        assert_eq!(world.level_at(0, 15, 8, 8), 8);
        // Scales 1 to 3: the cell with the lamp in it has air in it, so it holds 15, and the cells
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
    }

    #[test]
    fn a_cell_gives_the_light_of_its_own_source_to_its_neighbours_the_tick_it_is_evaluated() {
        // A cell that is lit by what is in it, rather than by a lamp that holds none, gives that to its
        // neighbour in the same tick that it first holds it, as a lamp does at once.
        let mut world = Box3::default();
        let mut lighting = MultiScaleLighting::new(table(), SCHEME);
        world.place(at(8, 8, 8), LAMP, &mut lighting);
        lighting.step(&mut world);
        assert_eq!(world.level_at(1, 8, 8, 8), 15, "the source of the cell");
        assert_eq!(world.level_at(1, 10, 8, 8), 14, "given at once to the next");
    }

    #[test]
    fn a_full_cell_of_lamps_is_as_bright_as_a_cell_can_be_and_gives_it_to_the_cells_around() {
        let mut world = Box3::default();
        let mut lighting = MultiScaleLighting::new(table(), SCHEME);
        world.fill((8, 8, 8), (9, 9, 9), LAMP, &mut lighting);
        settle(&mut lighting, &mut world);
        // At scale 1 the cell is all lamps: nothing in it is open, but it gives 15.
        assert_eq!(world.level_at(1, 8, 8, 8), 15);
        assert_eq!(world.level_at(1, 10, 8, 8), 14);
        assert_eq!(world.level_at(1, 6, 8, 8), 14);
        // At scale 3 they are a small part of the cell, which is open: it holds 15, as many lamps as one.
        assert_eq!(world.level_at(3, 8, 8, 8), 15);
    }

    #[test]
    fn a_wall_across_the_whole_box_stops_light_at_every_scale_however_thin() {
        let mut world = Box3::default();
        let mut lighting = MultiScaleLighting::new(table(), SCHEME);
        // A wall one voxel thick across the box at x = 8: all of a layer of cells of 1, and half of
        // the cells of 2, 4 and 8 that it is in. Light cannot come in at the face on the wall's side, because
        // nothing there is open, so the cells of the wall are reached from the far side if at all.
        world.fill((8, 0, 0), (8, EDGE - 1, EDGE - 1), STONE, &mut lighting);
        // A cell's worth of candles of 16 on one side.
        world.fill((4, 4, 4), (5, 5, 5), CANDLE, &mut lighting);
        settle(&mut lighting, &mut world);
        // Scale 0: the light of a candle (4, 3 beside it, 2 at the wall face) goes no further.
        assert_eq!(world.level_at(0, 7, 5, 5), 2);
        assert!((0..EDGE).all(|y| (0..EDGE).all(|z| world.level_at(0, 9, y, z) == 0)));
        // Scale 1: the cell of candles gives 4, the one next to it holds 3, the wall's cell (the
        // wall is its first layer, on the side the light comes from) holds nothing, and nothing is
        // beyond.
        assert_eq!(world.level_at(1, 6, 4, 4), 3);
        assert_eq!(world.level_at(1, 8, 4, 4), 0);
        assert_eq!(world.level_at(1, 10, 4, 4), 0);
        for scale in 2..SCALES {
            let width = 1i64 << scale;
            assert!(
                world
                    .lit(scale)
                    .iter()
                    .all(|&(cell, _)| cell.x() + width <= 8),
                "scale {scale}: {:?}",
                world.lit(scale)
            );
        }
    }

    #[test]
    fn a_hole_in_a_thin_wall_lets_coarse_light_through_and_only_there() {
        let mut world = Box3::default();
        let mut lighting = MultiScaleLighting::new(table(), SCHEME);
        world.fill((8, 0, 0), (8, EDGE - 1, EDGE - 1), STONE, &mut lighting);
        world.place(at(8, 4, 4), AIR, &mut lighting);
        world.fill((4, 4, 4), (5, 5, 5), CANDLE, &mut lighting);
        settle(&mut lighting, &mut world);
        // At scale 0 the light gets as far as the hole (4 at the candle, one less each voxel) and no
        // further by it: nothing is lit beyond.
        assert_eq!(world.level_at(0, 8, 4, 4), 1);
        assert_eq!(world.level_at(0, 9, 4, 4), 0);
        // At scale 1 the cell with the hole has an open voxel at the face the light comes at and
        // an open component that goes on to the far face, so light goes through to the cell beyond.
        assert_eq!(world.level_at(1, 8, 4, 4), 2);
        assert_eq!(world.level_at(1, 10, 4, 4), 1);
        // A cell of the wall away from the hole gets nothing.
        assert_eq!(world.level_at(1, 8, 10, 10), 0);
        assert_eq!(world.level_at(1, 10, 10, 10), 0);
    }

    #[test]
    fn a_cell_that_is_mostly_ground_with_air_over_it_is_lit_from_the_cell_above() {
        let mut world = Box3::default();
        let mut lighting = MultiScaleLighting::new(table(), SCHEME);
        // Ground up to y = 4 across the lower cells of 8: five layers of the eight, more than half.
        world.fill((0, 0, 0), (EDGE - 1, 4, EDGE - 1), STONE, &mut lighting);
        // A lamp in the cell above.
        world.place(at(4, 12, 4), LAMP, &mut lighting);
        settle(&mut lighting, &mut world);
        // The lamp's cell holds 15, and the cell below it, whose top three layers are air touching the
        // lamp's cell, holds 14, though the cell is mostly ground.
        assert_eq!(world.level_at(3, 0, 8, 0), 15);
        assert_eq!(world.level_at(3, 0, 0, 0), 14);
        // Also at the finer scales: the cell of 4 that is all ground has no air to hold light, and the
        // one over it (a layer of ground, three of air) is lit.
        assert_eq!(
            world.level_at(2, 0, 0, 0),
            0,
            "ground to the top of the cell: no air in it"
        );
        assert!(world.level_at(2, 0, 4, 0) > 0);
    }

    #[test]
    fn a_cell_whose_air_does_not_touch_a_lit_cell_by_air_is_not_lit() {
        let mut world = Box3::default();
        let mut lighting = MultiScaleLighting::new(table(), SCHEME);
        // The lower cells of 8 have air below and a roof of stone in their top two layers: the layer
        // that touches the lamp's cell is all stone.
        world.fill((0, 6, 0), (EDGE - 1, 7, EDGE - 1), STONE, &mut lighting);
        world.place(at(4, 12, 4), LAMP, &mut lighting);
        settle(&mut lighting, &mut world);
        assert_eq!(world.level_at(3, 0, 8, 0), 15);
        assert_eq!(world.level_at(3, 0, 0, 0), 0, "the air below is shut in");
    }

    #[test]
    fn air_that_touches_only_other_air_in_other_places_is_not_a_connection() {
        let mut world = Box3::default();
        let mut lighting = MultiScaleLighting::new(table(), SCHEME);
        // Walls across the whole box at x = 7 and x = 8, apart from one opening in each: at (7, 0, 0) and
        // at (8, 3, 3). The cells of 4 either side of x = 8 touch there, and the openings are not opposite
        // each other, so no voxel of one touches the other. A lamp on the far side.
        for y in 0..EDGE {
            for z in 0..EDGE {
                if (y, z) != (0, 0) {
                    world.place(at(7, y, z), STONE, &mut lighting);
                }
                if (y, z) != (3, 3) {
                    world.place(at(8, y, z), STONE, &mut lighting);
                }
            }
        }
        world.place(at(10, 1, 1), LAMP, &mut lighting);
        settle(&mut lighting, &mut world);
        assert!(world.level_at(2, 8, 0, 0) > 0, "the lamp's cell is lit");
        assert_eq!(
            world.level_at(2, 4, 0, 0),
            0,
            "no voxel of one opening touches the other"
        );
        // With the opening opposite, the light goes through.
        world.place(at(7, 3, 3), AIR, &mut lighting);
        settle(&mut lighting, &mut world);
        assert!(world.level_at(2, 4, 0, 0) > 0, "now they touch");
    }

    #[test]
    fn light_goes_through_a_cell_only_between_faces_that_the_same_open_air_reaches() {
        // A tunnel of air 4 voxels square along x through solid rock, so that light has nowhere else to
        // go: cells of 4 along x at 0, 4 and 8, a lamp in the one at 8. With a wall across the tunnel at
        // x = 6, air in the middle cell reaches the face toward the lamp (x = 7) and, apart from that, the
        // far face (x = 4), but not from one to the other.
        let tunnel = |wall: bool| {
            let mut world = Box3::default();
            let mut lighting = MultiScaleLighting::new(table(), SCHEME);
            world.fill(
                (0, 0, 0),
                (EDGE - 1, EDGE - 1, EDGE - 1),
                STONE,
                &mut lighting,
            );
            world.fill((0, 0, 0), (11, 3, 3), AIR, &mut lighting);
            if wall {
                world.fill((6, 0, 0), (6, 3, 3), STONE, &mut lighting);
            }
            world.place(at(10, 1, 1), LAMP, &mut lighting);
            settle(&mut lighting, &mut world);
            world
        };
        let walled = tunnel(true);
        assert_eq!(
            walled.level_at(2, 4, 0, 0),
            14,
            "the middle cell is lit by the air that touches the lamp's"
        );
        assert_eq!(
            walled.level_at(2, 0, 0, 0),
            0,
            "and gives none on, through a wall"
        );
        // Without the wall it goes through.
        assert_eq!(tunnel(false).level_at(2, 0, 0, 0), 13);
    }

    #[test]
    fn a_lamp_shut_in_a_cell_gives_nothing_across_faces_that_have_nothing_open() {
        // A lamp in a little room (2 voxels square inside) in a hollow cube of stone that is exactly a
        // cell of 4, so that every face of the cell is stone: no light comes out of it at that scale.
        let mut world = Box3::default();
        let mut lighting = MultiScaleLighting::new(table(), SCHEME);
        world.fill((4, 4, 4), (7, 7, 7), STONE, &mut lighting);
        world.fill((5, 5, 5), (6, 6, 6), AIR, &mut lighting);
        world.place(at(5, 5, 5), LAMP, &mut lighting);
        settle(&mut lighting, &mut world);
        assert!(world.lit(2).is_empty(), "{:?}", world.lit(2));
        // At scale 1 the cells inside it do light each other, and nothing outside the stone.
        assert!(world.level_at(1, 6, 6, 6) > 0);
        assert_eq!(world.level_at(1, 8, 4, 4), 0);
        assert_eq!(world.level_at(0, 8, 5, 5), 0);
    }

    #[test]
    fn light_goes_on_from_a_cell_only_by_the_air_it_came_in_by() {
        // Rock with narrow tunnels through it: one along x at y = 0, z = 0 from x = 0 to x = 7, and, by
        // the lamp, a little air at (7, 3, 3) in the same cell of 4 as the end of that tunnel (the cell at
        // x = 4), touching the lamp's cell at (8, 3, 3) and nothing else of it. The lamp's light comes in
        // at the little pocket, which does not join the tunnel, so it goes no further.
        let mut world = Box3::default();
        let mut lighting = MultiScaleLighting::new(table(), SCHEME);
        world.fill(
            (0, 0, 0),
            (EDGE - 1, EDGE - 1, EDGE - 1),
            STONE,
            &mut lighting,
        );
        world.fill((0, 0, 0), (7, 0, 0), AIR, &mut lighting);
        world.fill((7, 3, 3), (11, 3, 3), AIR, &mut lighting);
        world.place(at(10, 3, 3), LAMP, &mut lighting);
        settle(&mut lighting, &mut world);
        assert_eq!(world.level_at(2, 8, 0, 0), 15, "the lamp's cell");
        assert_eq!(
            world.level_at(2, 4, 0, 0),
            14,
            "the cell with the tunnel and the pocket is lit by the pocket"
        );
        assert_eq!(
            world.level_at(2, 0, 0, 0),
            0,
            "but it gives none on by the tunnel, which the light did not come in by"
        );
    }

    #[test]
    fn a_wall_of_glass_stops_no_light_at_any_scale() {
        let mut world = Box3::default();
        let mut lighting = MultiScaleLighting::new(table(), SCHEME);
        world.fill((8, 0, 0), (8, EDGE - 1, EDGE - 1), GLASS, &mut lighting);
        world.fill((4, 4, 4), (5, 5, 5), LAMP, &mut lighting);
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
        let mut lighting = MultiScaleLighting::new(table(), SCHEME);
        world.place(at(8, 8, 8), LAMP, &mut lighting);
        settle(&mut lighting, &mut world);
        assert!(!world.lit(0).is_empty() && !world.lit(3).is_empty());
        world.place(at(8, 8, 8), AIR, &mut lighting);
        // At a level a tick in a box this size: not held up by cells that give each other light back.
        let steps = settle(&mut lighting, &mut world);
        assert!(steps <= 18, "{steps} steps to go out");
        for scale in 0..SCALES {
            assert!(
                world.lit(scale).is_empty(),
                "scale {scale}: {:?}",
                world.lit(scale)
            );
            assert!(
                world.words[scale].values().all(|&word| !gives_light(word)),
                "scale {scale}: nothing is given on either"
            );
        }
    }

    #[test]
    fn light_spreads_a_cell_a_tick_so_coarse_light_is_faster_in_voxels() {
        let mut world = Box3::default();
        let mut lighting = MultiScaleLighting::new(table(), SCHEME);
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
        let mut quick_rule = MultiScaleLighting::new(table(), SCHEME);
        let mut slow = Box3::default();
        let mut slow_rule = MultiScaleLighting::new(table(), SCHEME);
        for (x, y, z) in [(3, 3, 3), (12, 9, 5), (5, 12, 12)] {
            quick.place(at(x, y, z), LAMP, &mut quick_rule);
            slow.place(at(x, y, z), LAMP, &mut slow_rule);
        }
        quick.place(at(8, 8, 8), STONE, &mut quick_rule);
        slow.place(at(8, 8, 8), STONE, &mut slow_rule);
        let quick_steps = settle(&mut quick_rule, &mut quick);
        let mut slow_steps = 0;
        while !slow_rule.is_settled() {
            slow_rule.step_within(&mut slow, &[40, 10, 5, 2]);
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
        #![proptest_config(ProptestConfig::with_cases(16))]

        #[test]
        fn the_rule_settles_to_what_the_equations_say_at_every_scale(
            first in edits(),
            second in edits(),
        ) {
            let table = table();
            let mut world = Box3::default();
            let mut lighting = MultiScaleLighting::new(table.clone(), SCHEME);
            // Two rounds of edits, settling between, so that light is also taken away and moved,
            // and what the rule remembers of cells is tested against edits.
            for round in [&first, &second] {
                for Edit::Place(x, y, z, block) in round {
                    world.place(at(*x, *y, *z), *block, &mut lighting);
                }
                settle(&mut lighting, &mut world);
                for scale in 0..SCALES {
                    prop_assert_eq!(world.lit(scale), reference(&world, &table, SCHEME, scale), "scale {}", scale);
                }
            }
        }

        /// Walls of stone of any pattern across a box with a lamp: the equations again, where there is
        /// a lot of stone and so a lot of cells that are partly open.
        #[test]
        fn partly_solid_cells_settle_to_what_the_equations_say(
            solid in prop::collection::vec((0..EDGE, 0..EDGE, 0..EDGE), 60..400),
            lamps in prop::collection::vec((0..EDGE, 0..EDGE, 0..EDGE), 1..4),
        ) {
            let table = table();
            let mut world = Box3::default();
            let mut lighting = MultiScaleLighting::new(table.clone(), SCHEME);
            for (x, y, z) in solid {
                world.place(at(x, y, z), STONE, &mut lighting);
            }
            for (x, y, z) in lamps {
                world.place(at(x, y, z), LAMP, &mut lighting);
            }
            settle(&mut lighting, &mut world);
            for scale in 0..SCALES {
                prop_assert_eq!(world.lit(scale), reference(&world, &table, SCHEME, scale), "scale {}", scale);
            }
        }
    }

    /// A box as wide as the largest cells of a scheme need (at least 16).
    fn box_for(scheme: LightScheme) -> Box3 {
        Box3::with_edge(scheme.largest_cell().max(EDGE))
    }

    #[test]
    fn in_every_scheme_a_lamp_is_the_greatest_level_at_every_scale_and_one_less_a_cell_away() {
        for scheme in LightScheme::ALL {
            let mut world = box_for(scheme);
            let mut lighting = MultiScaleLighting::new(table(), scheme);
            let middle = world.edge / 2;
            world.place(at(middle, middle, middle), LAMP, &mut lighting);
            settle(&mut lighting, &mut world);
            let greatest = scheme.maximum_level();
            for scale in 0..scheme.scales() {
                let width = 1i64 << scale;
                let cell = anchor_of(scale, at(middle, middle, middle));
                assert_eq!(
                    world.level_at(scale, cell.x(), cell.y(), cell.z()),
                    greatest,
                    "{scheme:?} scale {scale}"
                );
                if cell.x() + width < world.edge {
                    assert_eq!(
                        world.level_at(scale, cell.x() + width, cell.y(), cell.z()),
                        greatest - 1,
                        "{scheme:?} scale {scale}"
                    );
                }
            }
            // Beyond the scales there are none: nothing is stored at a scale a scheme does not have.
            for scale in scheme.scales()..MAXIMUM_SCALES {
                assert!(world.words[scale].is_empty());
            }
        }
    }

    #[test]
    fn in_every_scheme_light_that_loses_its_source_dies_out() {
        for scheme in LightScheme::ALL {
            let mut world = box_for(scheme);
            let mut lighting = MultiScaleLighting::new(table(), scheme);
            let middle = world.edge / 2;
            world.place(at(middle, middle, middle), LAMP, &mut lighting);
            settle(&mut lighting, &mut world);
            world.place(at(middle, middle, middle), AIR, &mut lighting);
            settle(&mut lighting, &mut world);
            for scale in 0..scheme.scales() {
                assert!(world.lit(scale).is_empty(), "{scheme:?} scale {scale}");
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(6))]

        /// The equations again for each of the other schemes, in boxes as wide as their largest cells.
        /// (The finest scales of a box of 32 are left to the boxes of 16: the reference looks at every
        /// cell and there are 32 768 of them.)
        #[test]
        fn every_scheme_settles_to_what_the_equations_say(
            scheme_index in 0usize..4,
            solid in prop::collection::vec((0..32i64, 0..32i64, 0..32i64), 30..200),
            lamps in prop::collection::vec((0..32i64, 0..32i64, 0..32i64), 1..3),
        ) {
            let scheme = LightScheme::ALL[scheme_index];
            let table = table();
            let mut world = box_for(scheme);
            let edge = world.edge;
            let mut lighting = MultiScaleLighting::new(table.clone(), scheme);
            for (x, y, z) in solid {
                world.place(at(x % edge, y % edge, z % edge), STONE, &mut lighting);
            }
            for (x, y, z) in lamps {
                world.place(at(x % edge, y % edge, z % edge), LAMP, &mut lighting);
            }
            settle(&mut lighting, &mut world);
            let first_scale = if edge > 16 { 2 } else { 0 };
            for scale in first_scale..scheme.scales() {
                prop_assert_eq!(
                    world.lit(scale),
                    reference(&world, &table, scheme, scale),
                    "{} scale {}",
                    scheme.name(),
                    scale
                );
            }
        }
    }
}
