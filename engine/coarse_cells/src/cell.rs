use voxel_space::LevelOfDetail;

/// How bright a cell is when nothing says otherwise: the scale of the mesher's brightness, 0 to 255.
pub const FULL_LIGHT: u8 = 255;

/// A cell with at least this much fill (half) is drawn as a solid cube; with less it is not drawn.
pub const DRAWN_FILL: u8 = 128;

/// The coarsest detail level whose parent cell takes the brightest of its children's light: a cell of 32 voxels, the width of a brick. Above it
/// the light is an average, so that one lamp does not light a cube a thousand kilometres wide.
pub const BRIGHTEST_LIGHT_UP_TO_LEVEL: u8 = 5;

/// Exposure is a count of faces, in fixed point with this many fractional bits.
const EXPOSURE_BITS: u32 = 8;

/// What a cell of `2^n` voxels along each edge is drawn as, far away (`lod-spec.md`, section 3.2). It holds the dominant block, how much of the cell is
/// filled, how bright it is, and how much of its surface touches air (working data: a parent is made from it).
///
/// A cell is never saved. Its block is the game's number for it, in memory, so that the "saved files hold only namespaced ids" rule is not touched.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CoarseCell {
    /// The dominant block: of the blocks that touch air, the one with the most surface. The air block for a cell that is empty.
    pub block: u32,
    /// How much of the cell is not air, 0 to 255. A cell with anything in it has at least 1.
    pub fill: u8,
    /// How bright the cell is, 0 to 255, on the scale of the mesher's brightness.
    pub light: u8,
    /// The area of the cell's surface that touches air, in units of one face of the cell (so a flat floor has 1 at every detail level), in fixed
    /// point with 8 fractional bits ([`CoarseCell::EXPOSURE_ONE`] is one face). Saturates.
    pub exposure: u32,
}

impl CoarseCell {
    /// One face's worth of exposure.
    pub const EXPOSURE_ONE: u32 = 1 << EXPOSURE_BITS;

    /// A cell with nothing in it, `light` bright.
    pub fn empty(air: u32, light: u8) -> Self {
        Self {
            block: air,
            fill: 0,
            light,
            exposure: 0,
        }
    }

    /// Whether the cell is drawn: it is filled by at least half.
    pub fn is_drawn(self) -> bool {
        self.fill >= DRAWN_FILL
    }
}

/// The cell that eight cells make: the cells of the eight octants of a cell one detail level up, in the order `x + 2 y + 4 z`. `parent_level` is the
/// detail level of the result.
///
/// - *fill*: the sum of the children's divided by 8, rounded up if there is any, so a speck is not lost.
/// - *exposure*: the sum of the children's divided by 4 (a child's face is a quarter of its parent's), rounded to the nearest, saturating.
/// - *block*: of the children that have anything in them, the block whose children together have the most exposure (a child with no surface at all
///   counts by fill instead, and only when no child has any surface). Ties go to the block of the lowest octant, which does not depend on how a game
///   numbers its blocks. A cell with nothing in it is `air`.
/// - *light*: the brightest child's up to [`BRIGHTEST_LIGHT_UP_TO_LEVEL`]; above that the average of the children's that have something in them,
///   weighted by exposure (by fill if none has a surface), or the brightest if none has anything in it.
pub fn combine(children: &[CoarseCell; 8], parent_level: LevelOfDetail, air: u32) -> CoarseCell {
    let brightest = children
        .iter()
        .map(|child| child.light)
        .max()
        .expect("there are eight children");
    let fill_sum: u32 = children.iter().map(|child| u32::from(child.fill)).sum();
    if fill_sum == 0 {
        return CoarseCell::empty(air, brightest);
    }
    let fill = fill_sum.div_ceil(8).min(255) as u8;
    let exposure_sum: u64 = children.iter().map(|child| u64::from(child.exposure)).sum();
    let exposure = ((exposure_sum + 2) / 4).min(u64::from(u32::MAX)) as u32;
    // What counts for a child is its surface; if none of the children with something in them has a surface, its fill.
    let any_surface = children
        .iter()
        .any(|child| child.fill > 0 && child.exposure > 0);
    let weight = |child: &CoarseCell| -> u64 {
        if child.fill == 0 {
            0
        } else if any_surface {
            u64::from(child.exposure)
        } else {
            u64::from(child.fill)
        }
    };
    let mut block = air;
    let mut best = 0u64;
    for child in children.iter().filter(|child| child.fill > 0) {
        let total: u64 = children
            .iter()
            .filter(|other| other.fill > 0 && other.block == child.block)
            .map(weight)
            .sum();
        if total > best {
            best = total;
            block = child.block;
        }
    }
    if best == 0 {
        // (Only possible when every weight is 0, which fill makes impossible; the lowest octant with anything in it is the answer all the same.)
        block = children
            .iter()
            .find(|child| child.fill > 0)
            .map_or(air, |child| child.block);
    }
    let light = if parent_level.level() <= BRIGHTEST_LIGHT_UP_TO_LEVEL {
        brightest
    } else {
        let total: u64 = children.iter().map(weight).sum();
        let weighted: u64 = children
            .iter()
            .map(|child| u64::from(child.light) * weight(child))
            .sum();
        // Rounded to the nearest.
        ((weighted + total / 2) / total) as u8
    };
    CoarseCell {
        block,
        fill,
        light,
        exposure,
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const AIR: u32 = 0;
    const STONE: u32 = 1;
    const DIRT: u32 = 2;
    const GRASS: u32 = 3;
    const SNOW: u32 = 4;
    const LAMP: u32 = 5;

    fn level(level: u8) -> LevelOfDetail {
        LevelOfDetail::new(level).unwrap()
    }

    fn air(light: u8) -> CoarseCell {
        CoarseCell::empty(AIR, light)
    }

    /// A cell that is full of `block`, with `faces` faces' worth of surface, at this light.
    fn solid(block: u32, faces: u32, light: u8) -> CoarseCell {
        CoarseCell {
            block,
            fill: 255,
            light,
            exposure: faces * CoarseCell::EXPOSURE_ONE,
        }
    }

    /// Eight cells in the order of the octants `x + 2 y + 4 z`: the lower four (y = 0) as given, and the upper four air.
    fn ground(lower: [CoarseCell; 4]) -> [CoarseCell; 8] {
        let [a, b, c, d] = lower;
        // Octants 0 and 1 are (0,0,0) and (1,0,0); 4 and 5 are (0,0,1) and (1,0,1); y = 1 octants are 2, 3, 6, 7.
        [a, b, air(0), air(0), c, d, air(0), air(0)]
    }

    #[test]
    fn eight_empty_cells_make_an_empty_cell_that_is_as_bright_as_the_brightest() {
        let mut children = [air(0); 8];
        children[5] = air(200);
        for parent in [0, 5, 6, 40] {
            let made = combine(&children, level(parent), AIR);
            assert_eq!(made, CoarseCell::empty(AIR, 200), "level {parent}");
        }
    }

    #[test]
    fn eight_cells_of_one_block_make_a_cell_of_it_whatever_the_surface() {
        for faces in [0, 1, 7] {
            let children = [solid(STONE, faces, 10); 8];
            let made = combine(&children, level(3), AIR);
            assert_eq!((made.block, made.fill, made.light), (STONE, 255, 10));
        }
    }

    #[test]
    fn a_flat_floor_has_one_face_of_surface_at_every_level_it_is_made_into() {
        // The four top children of a floor each have one face, the four under them none: the parent has (4 * 1) / 4 = one face, and so on up.
        let mut cell = [
            solid(GRASS, 1, 0),
            solid(GRASS, 1, 0),
            solid(GRASS, 1, 0),
            solid(GRASS, 1, 0),
        ];
        for up in 1..=40 {
            let made = combine(
                &[
                    cell[0],
                    cell[1],
                    cell[2],
                    cell[3],
                    solid(STONE, 0, 0),
                    solid(STONE, 0, 0),
                    solid(STONE, 0, 0),
                    solid(STONE, 0, 0),
                ],
                level(up),
                AIR,
            );
            assert_eq!(made.exposure, CoarseCell::EXPOSURE_ONE, "level {up}");
            assert_eq!(made.block, GRASS);
            cell = [made; 4];
        }
    }

    #[test]
    fn a_layer_of_snow_one_block_thick_on_stone_is_a_cell_of_snow_from_above() {
        // Four children on the ground: a voxel of snow on top of stone for each (the snow has the surface, the stone under it none), and the same four
        // below, all stone. The snow touches air and the stone does not.
        let snow_on_stone = [
            solid(SNOW, 1, 0),
            solid(SNOW, 1, 0),
            solid(SNOW, 1, 0),
            solid(SNOW, 1, 0),
        ];
        let mut children = [air(0); 8];
        for (index, child) in snow_on_stone.into_iter().enumerate() {
            children[[2, 3, 6, 7][index]] = child;
        }
        for index in [0, 1, 4, 5] {
            children[index] = solid(STONE, 0, 0);
        }
        let made = combine(&children, level(1), AIR);
        assert_eq!(made.block, SNOW);
        // Counted by volume, the stone would have won: half of the cell is stone and the snow is a quarter.
        assert_eq!(made.fill, 255);
    }

    #[test]
    fn the_wall_of_a_cave_does_not_outvote_the_ground_it_is_under_a_whole_hill_of() {
        // A hill: grass over a large area, with a cave under it whose walls are stone with a smaller area than the hill's top.
        let children = [
            solid(GRASS, 1, 0),
            solid(GRASS, 1, 0),
            solid(GRASS, 1, 0),
            solid(GRASS, 1, 0),
            solid(STONE, 0, 0),
            solid(STONE, 0, 0),
            solid(STONE, 1, 0),
            solid(STONE, 1, 0),
        ];
        // Grass has four faces of surface and stone two.
        assert_eq!(combine(&children, level(2), AIR).block, GRASS);
        // The same four stone faces of cave wall against two of grass: the stone is what is seen.
        let cave = [
            solid(GRASS, 1, 0),
            solid(GRASS, 1, 0),
            solid(STONE, 1, 0),
            solid(STONE, 1, 0),
            solid(STONE, 1, 0),
            solid(STONE, 1, 0),
            solid(STONE, 0, 0),
            solid(STONE, 0, 0),
        ];
        assert_eq!(combine(&cave, level(2), AIR).block, STONE);
    }

    #[test]
    fn blocks_are_counted_together_across_the_children_that_have_them() {
        // Grass has the most surface in no single child (each stone child has 3 faces, each grass child 2) but 4 grass children make 8 against 3 for
        // the one stone child.
        let children = [
            solid(STONE, 3, 0),
            solid(GRASS, 2, 0),
            solid(GRASS, 2, 0),
            solid(GRASS, 2, 0),
            solid(GRASS, 2, 0),
            air(0),
            air(0),
            air(0),
        ];
        assert_eq!(combine(&children, level(1), AIR).block, GRASS);
    }

    #[test]
    fn a_tie_goes_to_the_block_of_the_lowest_octant_whatever_the_numbers_of_the_blocks() {
        for (low, high) in [(STONE, DIRT), (DIRT, STONE), (GRASS, SNOW), (SNOW, GRASS)] {
            let mut children = [air(0); 8];
            children[1] = solid(low, 1, 0);
            children[6] = solid(high, 1, 0);
            assert_eq!(combine(&children, level(1), AIR).block, low);
        }
    }

    #[test]
    fn with_no_surface_anywhere_the_block_with_the_most_fill_wins() {
        let mut children = [air(0); 8];
        let half = |block, fill| CoarseCell {
            block,
            fill,
            light: 0,
            exposure: 0,
        };
        children[0] = half(DIRT, 100);
        children[1] = half(STONE, 200);
        children[2] = half(STONE, 10);
        assert_eq!(combine(&children, level(1), AIR).block, STONE);
    }

    #[test]
    fn a_speck_is_not_rounded_to_nothing_and_a_full_cell_stays_full() {
        let mut children = [air(0); 8];
        children[3] = CoarseCell {
            block: STONE,
            fill: 1,
            light: 0,
            exposure: 0,
        };
        assert_eq!(combine(&children, level(1), AIR).fill, 1);
        assert_eq!(combine(&[solid(STONE, 0, 0); 8], level(1), AIR).fill, 255);
        let mut one_short = [solid(STONE, 0, 0); 8];
        one_short[0].fill = 254;
        assert_eq!(combine(&one_short, level(1), AIR).fill, 255);
        // Half full: four full children.
        let mut half = [air(0); 8];
        for index in [0, 1, 4, 5] {
            half[index] = solid(STONE, 0, 0);
        }
        assert_eq!(combine(&half, level(1), AIR).fill, 128);
        assert!(combine(&half, level(1), AIR).is_drawn());
        let mut less = half;
        less[5] = air(0);
        assert!(!combine(&less, level(1), AIR).is_drawn());
    }

    #[test]
    fn light_is_the_brightest_child_up_to_cells_of_a_brick_and_an_average_above() {
        // A lamp in one corner child (bright), the rest of the surface dark.
        let mut children = ground([
            solid(STONE, 1, 0),
            solid(STONE, 1, 0),
            solid(STONE, 1, 0),
            solid(STONE, 1, 0),
        ]);
        children[0] = solid(LAMP, 1, 240);
        for parent in 0..=5 {
            assert_eq!(
                combine(&children, level(parent), AIR).light,
                240,
                "level {parent}"
            );
        }
        // Above 32 voxels: the average, weighted by surface: 240 of four equal surfaces is 60.
        for parent in [6, 7, 20, 40] {
            assert_eq!(
                combine(&children, level(parent), AIR).light,
                60,
                "level {parent}"
            );
        }
        // A cell with all its surface lit is as lit above as below.
        let lit = ground([solid(STONE, 1, 90); 4]);
        assert_eq!(combine(&lit, level(9), AIR).light, 90);
    }

    #[test]
    fn the_light_of_the_air_beside_the_ground_counts_up_to_a_brick_and_not_above() {
        // Bright air in the upper corner (a lamp in the air of a cave), dark ground.
        let mut children = ground([solid(STONE, 1, 0); 4]);
        children[7] = air(255);
        assert_eq!(combine(&children, level(5), AIR).light, 255);
        // (Above it the average is of the children with something in them, and the air has none.)
        assert_eq!(combine(&children, level(6), AIR).light, 0);
    }

    #[test]
    fn the_light_average_falls_back_to_fill_when_nothing_has_a_surface() {
        let mut children = [air(0); 8];
        children[0] = CoarseCell {
            block: STONE,
            fill: 255,
            light: 100,
            exposure: 0,
        };
        children[1] = CoarseCell {
            block: STONE,
            fill: 255,
            light: 200,
            exposure: 0,
        };
        assert_eq!(combine(&children, level(7), AIR).light, 150);
    }

    #[test]
    fn exposure_is_a_quarter_of_the_sum_rounded_and_never_overflows() {
        let mut children = [air(0); 8];
        children[0] = CoarseCell {
            block: STONE,
            fill: 255,
            light: 0,
            exposure: 5,
        };
        // 5 / 4 = 1.25: rounds to 1; 6 / 4 = 1.5: rounds to 2.
        assert_eq!(combine(&children, level(1), AIR).exposure, 1);
        children[0].exposure = 6;
        assert_eq!(combine(&children, level(1), AIR).exposure, 2);
        let big = [CoarseCell {
            block: STONE,
            fill: 255,
            light: 0,
            exposure: u32::MAX,
        }; 8];
        assert_eq!(combine(&big, level(1), AIR).exposure, u32::MAX);
    }

    fn any_cell() -> impl Strategy<Value = CoarseCell> {
        (0u32..6, any::<u8>(), any::<u8>(), 0u32..2_000).prop_map(
            |(block, fill, light, exposure)| {
                if fill == 0 {
                    CoarseCell::empty(AIR, light)
                } else {
                    CoarseCell {
                        block: block + 1,
                        fill,
                        light,
                        exposure,
                    }
                }
            },
        )
    }

    proptest! {
        #[test]
        fn what_is_summed_does_not_depend_on_the_order_of_the_children_and_what_is_chosen_is_always_one_of_them(
            children in proptest::array::uniform8(any_cell()),
            rotation in 0usize..8,
            parent in 0u8..45,
        ) {
            let made = combine(&children, level(parent), AIR);
            let mut turned = children;
            turned.rotate_left(rotation);
            let again = combine(&turned, level(parent), AIR);
            // Fill and exposure are sums, and the light (for levels where it is the brightest, or a weighted average) too.
            prop_assert_eq!(made.fill, again.fill);
            prop_assert_eq!(made.exposure, again.exposure);
            prop_assert_eq!(made.light, again.light);
            // Fill is 0 exactly when every child is empty, and then so is the block.
            prop_assert_eq!(made.fill == 0, children.iter().all(|child| child.fill == 0));
            if made.fill == 0 {
                prop_assert_eq!(made.block, AIR);
            } else {
                prop_assert!(children.iter().any(|child| child.fill > 0 && child.block == made.block));
            }
            // The light is never outside what the children have.
            let (lowest, highest) = (
                children.iter().map(|child| child.light).min().unwrap(),
                children.iter().map(|child| child.light).max().unwrap(),
            );
            prop_assert!(made.light >= lowest && made.light <= highest);
        }

        #[test]
        fn a_region_all_one_cell_is_that_cell_at_every_level_but_for_its_exposure(
            block in 1u32..6,
            fill in 1u8..=255,
            light in any::<u8>(),
            parent in 0u8..45,
        ) {
            let cell = CoarseCell { block, fill, light, exposure: 0 };
            let made = combine(&[cell; 8], level(parent), AIR);
            prop_assert_eq!((made.block, made.fill, made.light, made.exposure), (block, fill, light, 0));
        }
    }
}
