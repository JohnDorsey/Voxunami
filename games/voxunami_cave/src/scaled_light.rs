//! How Cave draws the light kept in cells larger than a voxel: the words the cells are stored in, how
//! bright each is, and the hook that hands the view a brightness for every voxel of a brick. It used to
//! be in `world_view`, which no longer knows of it (the view has only the neutral [`BrickLight`]).

use voxel_grid::{Brick, ChannelIndex, ChannelStorage};
use voxel_space::{BRICK_VOXEL_COUNT, BrickCoordinate, LocalVoxelIndex};
use world_view::BrickLight;

/// The most scales of light a game can have besides its plain light channel.
pub const MAXIMUM_LIGHT_SCALES: usize = 7;

/// How a channel of scaled light holds its level in a word: `field_count` fields of `field_bits`
/// bits each from the lowest bit (a cell's level is the greatest of them), and above them a code
/// of `code_bits` bits at `code_shift` which says how many levels there are: a field holds up to
/// `2^field_bits - 1`, and the greatest level a cell can have is that shifted right by the code. So
/// a game can have several numbers of levels, and a word says which it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LevelFormat {
    pub field_bits: u8,
    pub field_count: u8,
    pub code_shift: u8,
    pub code_bits: u8,
}

impl LevelFormat {
    /// The level a word holds and the greatest level it could (which is 0 only for a word with
    /// no room for a level).
    pub fn decode(self, word: u32) -> (u32, u32) {
        let field_mask = (1u32 << self.field_bits) - 1;
        let level = (0..u32::from(self.field_count))
            .map(|field| (word >> (field * u32::from(self.field_bits))) & field_mask)
            .max()
            .unwrap_or(0);
        let code = (word >> self.code_shift) & ((1u32 << self.code_bits) - 1);
        (level, field_mask >> code)
    }
}

/// Light kept in cells larger than a voxel: cubes of `cell_width` voxels along each side, laid from the
/// world's origin (a cell starts where a coordinate is a multiple of the width). A channel holds a
/// word (see [`LevelFormat`]) for each cell, which lights every voxel of the cell. A cell may be in
/// more than one brick (a width that is not a divisor of 32, or one over 32), so the word is kept in each
/// brick it is in, at the first voxel of the cell in that brick (the one with the smallest
/// coordinates), where a brick can find it without looking at another. A voxel is drawn with the light
/// of the plain light channel and of every scale blended (see [`blend_brightness`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScaledLight {
    pub channel: ChannelIndex,
    /// The cells are this many voxels wide.
    pub cell_width: u16,
    /// How bright the channel is at its greatest level, as a weight: 255 is full brightness, and a
    /// channel with a larger weight reaches full brightness at a lower level. Brightness is
    /// proportional to level (and never more than 255).
    pub full_brightness: u16,
    pub format: LevelFormat,
}

impl ScaledLight {
    /// How bright, from 0 to 255, a channel that holds `word` is.
    pub fn brightness(&self, word: u32) -> u32 {
        let (level, greatest) = self.format.decode(word);
        if greatest == 0 {
            return 0;
        }
        (level * u32::from(self.full_brightness) / greatest).min(255)
    }
}

/// The channels of light the view blends: the plain light channel, and each scale of larger cells.
/// The view's [`BrickLight`] for a world with multiscale light.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MultiScaleView {
    pub light: Option<ChannelIndex>,
    pub scales: Vec<ScaledLight>,
}

impl BrickLight for MultiScaleView {
    fn brightness(&self, brick: &Brick, coordinate: BrickCoordinate) -> Option<ChannelStorage> {
        combined_light(brick, coordinate, self)
    }
}

/// Lights of different scales blended: each as a fraction of full brightness, and what is missing from
/// full brightness is the product of what is missing from each, as the renderer blends ambient light
/// with a face's light (like screen blending in an image editor). So lights add up, two of half
/// brightness are three quarters, and none goes beyond full. `lights` are 0 to 255.
pub fn blend_brightness(lights: impl IntoIterator<Item = u32>) -> u32 {
    let missing = lights.into_iter().fold(255, |missing, light| {
        (missing * (255 - light.min(255)) + 127) / 255
    });
    255 - missing
}

/// How bright each voxel of a brick is when the game has scaled light: the plain light channel and
/// each scale's level in the voxel's cell, blended (see [`blend_brightness`]). `None` for a game
/// without scaled light, whose light channel is drawn as it is.
pub fn combined_light(
    brick: &Brick,
    coordinate: BrickCoordinate,
    view: &MultiScaleView,
) -> Option<ChannelStorage> {
    if view.scales.is_empty() {
        return None;
    }
    let base = view.light.map(|light| brick.channel(light));
    // Most bricks hold no light in any channel, or the same everywhere: one value, with no scan.
    let uniform = |storage: &ChannelStorage| storage.uniform_value();
    let uniform_lights: Option<Vec<u32>> =
        std::iter::once(base.map_or(Some(0), uniform))
            .chain(view.scales.iter().map(|scale| {
                uniform(brick.channel(scale.channel)).map(|word| scale.brightness(word))
            }))
            .collect();
    if let Some(lights) = uniform_lights {
        return Some(ChannelStorage::uniform(blend_brightness(lights)));
    }
    // Where a voxel's cell starts in this brick, for each scale, along each axis: the start of the cell,
    // or the brick's edge if the cell began in the brick before.
    let origin = [
        coordinate.x() * 32,
        coordinate.y() * 32,
        coordinate.z() * 32,
    ];
    let corners: Vec<(&ScaledLight, [[u8; 32]; 3])> = view
        .scales
        .iter()
        .map(|scale| {
            let width = i64::from(scale.cell_width);
            let table = std::array::from_fn(|axis| {
                std::array::from_fn(|local| {
                    let at = origin[axis] + local as i64;
                    (at.div_euclid(width) * width - origin[axis]).max(0) as u8
                })
            });
            (scale, table)
        })
        .collect();
    let mut values = vec![0u32; BRICK_VOXEL_COUNT];
    for local in LocalVoxelIndex::all() {
        let lights = std::iter::once(base.map_or(0, |base| base.get(local))).chain(
            corners.iter().map(|(scale, corner)| {
                let anchor = LocalVoxelIndex::new(
                    corner[0][usize::from(local.x())],
                    corner[1][usize::from(local.y())],
                    corner[2][usize::from(local.z())],
                );
                scale.brightness(brick.channel(scale.channel).get(anchor))
            }),
        );
        values[local.index()] = blend_brightness(lights);
    }
    Some(ChannelStorage::from_values(&values))
}
