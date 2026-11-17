use block_models::{Shape, SurfaceHit};
use voxel_space::{GridId, LocalVoxelIndex, VoxelCoordinate, WorldPosition};

/// The first solid voxel a ray passes through, and the empty voxel in front of it, which is where a
/// block placed against the hit face would go.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayHit {
    /// The grid the voxels are of: the root grid of a level, or a ship (whose voxels are in its own coordinates).
    pub grid: GridId,
    pub voxel: VoxelCoordinate,
    pub previous: VoxelCoordinate,
    /// How far along the ray, in voxels, it enters the voxel it hit.
    pub distance: f64,
}

/// Finds the first solid voxel within `maximum_distance` voxels along `direction` from `start`, in the grid `grid`, whose voxels `is_solid`
/// answers for (and `start` and `direction` are in the coordinates of).
///
/// `is_solid` answers for a voxel, or `None` where the world is not loaded; the ray stops there
/// without a hit, since nothing can be said beyond what is known. The voxel the ray starts in is
/// never a hit, so a viewer inside a block can still look out. The direction need not be a unit
/// vector; a zero direction hits nothing.
pub fn cast_ray(
    grid: GridId,
    start: WorldPosition,
    direction: [f64; 3],
    maximum_distance: f64,
    mut is_solid: impl FnMut(VoxelCoordinate) -> Option<bool>,
) -> Option<RayHit> {
    let mut walk = VoxelWalk::new(start, direction, maximum_distance)?;
    while let Some(crossing) = walk.advance() {
        if is_solid(crossing.voxel)? {
            return Some(RayHit {
                grid,
                voxel: crossing.voxel,
                previous: crossing.previous,
                distance: crossing.entered,
            });
        }
    }
    None
}

/// The place where a ray first meets what a block fills of its voxel: the side of a slab inside the voxel, the riser of a stair, the face of a whole cube.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceRayHit {
    /// The grid the voxel is of (see [`RayHit::grid`]).
    pub grid: GridId,
    pub voxel: VoxelCoordinate,
    /// Which side, where in the voxel (its own coordinates, 0 to 1), and how far along the ray, in voxels.
    pub surface: SurfaceHit,
}

/// Finds the first place within `maximum_distance` voxels along `direction` from `start` where the ray meets the shape of a voxel of the grid `grid`, which `shape_of`
/// answers for: `Some(Some(shape))` for a voxel with a shape (a block's, in its state and orientation), `Some(None)` for one with nothing in it, and `None` where the world
/// is not loaded, where the ray stops without a hit. It is [`cast_ray`] for a partial block: a ray that goes over a slab or through the open side of a stair goes on, and
/// the voxel that is hit is the first whose filled eighths the ray crosses. The voxel the ray starts in is never a hit, as in `cast_ray`.
pub fn cast_surface<'a>(
    grid: GridId,
    start: WorldPosition,
    direction: [f64; 3],
    maximum_distance: f64,
    mut shape_of: impl FnMut(VoxelCoordinate) -> Option<Option<&'a Shape>>,
) -> Option<SurfaceRayHit> {
    let mut walk = VoxelWalk::new(start, direction, maximum_distance)?;
    while let Some(crossing) = walk.advance() {
        let Some(shape) = shape_of(crossing.voxel)? else {
            continue;
        };
        let origin = walk.start_in(crossing.voxel);
        if let Some(surface) = shape.first_surface(origin, walk.direction, maximum_distance) {
            return Some(SurfaceRayHit {
                grid,
                voxel: crossing.voxel,
                surface,
            });
        }
    }
    None
}

/// A voxel the ray has entered by crossing one side of it.
struct Crossing {
    previous: VoxelCoordinate,
    voxel: VoxelCoordinate,
    /// How far along the ray, in voxels, it crosses into the voxel.
    entered: f64,
}

/// The voxels a ray passes through, in order, each by the side it crosses (the voxel it starts in is not one of them).
struct VoxelWalk {
    /// A unit vector.
    direction: [f64; 3],
    start_voxel: VoxelCoordinate,
    /// Where the start is in its voxel, 0 to 1 along each axis.
    fraction: [f64; 3],
    voxel: VoxelCoordinate,
    step: [i128; 3],
    next_boundary: [f64; 3],
    boundary_spacing: [f64; 3],
    maximum_distance: f64,
}

impl VoxelWalk {
    /// `None` for a direction that is zero or not a number.
    fn new(start: WorldPosition, direction: [f64; 3], maximum_distance: f64) -> Option<Self> {
        let length = (direction[0] * direction[0]
            + direction[1] * direction[1]
            + direction[2] * direction[2])
            .sqrt();
        if length == 0.0 || !length.is_finite() {
            return None;
        }
        let direction = direction.map(|component| component / length);

        // Work in voxels relative to the start brick, so the arithmetic stays small and exact enough.
        let offset = start.offset();
        let cell = offset.map(|component| component.floor());
        let voxel = start.brick().voxel_at(LocalVoxelIndex::new(
            cell[0] as u8,
            cell[1] as u8,
            cell[2] as u8,
        ));
        let fraction: [f64; 3] = std::array::from_fn(|axis| offset[axis] - cell[axis]);

        let mut step = [0i128; 3];
        let mut next_boundary = [f64::INFINITY; 3];
        let mut boundary_spacing = [f64::INFINITY; 3];
        for axis in 0..3 {
            if direction[axis] > 0.0 {
                step[axis] = 1;
                next_boundary[axis] = (1.0 - fraction[axis]) / direction[axis];
                boundary_spacing[axis] = 1.0 / direction[axis];
            } else if direction[axis] < 0.0 {
                step[axis] = -1;
                next_boundary[axis] = fraction[axis] / -direction[axis];
                boundary_spacing[axis] = 1.0 / -direction[axis];
            }
        }
        Some(Self {
            direction,
            start_voxel: voxel,
            fraction,
            voxel,
            step,
            next_boundary,
            boundary_spacing,
            maximum_distance,
        })
    }

    /// The next voxel, or `None` when the next side is beyond the range (or the voxel is beyond the coordinates).
    fn advance(&mut self) -> Option<Crossing> {
        let axis = (0..3)
            .min_by(|&a, &b| self.next_boundary[a].total_cmp(&self.next_boundary[b]))
            .expect("three axes");
        if self.next_boundary[axis] > self.maximum_distance {
            return None;
        }
        let previous = self.voxel;
        let entered = self.next_boundary[axis];
        let moved = [
            previous.x() + if axis == 0 { self.step[0] } else { 0 },
            previous.y() + if axis == 1 { self.step[1] } else { 0 },
            previous.z() + if axis == 2 { self.step[2] } else { 0 },
        ];
        self.voxel = VoxelCoordinate::try_new(moved[0], moved[1], moved[2]).ok()?;
        self.next_boundary[axis] += self.boundary_spacing[axis];
        Some(Crossing {
            previous,
            voxel: self.voxel,
            entered,
        })
    }

    /// Where the ray starts, in the coordinates of `voxel` (its own corner is 0 and the far corner 1 on each axis).
    fn start_in(&self, voxel: VoxelCoordinate) -> [f64; 3] {
        let beyond = [
            voxel.x() - self.start_voxel.x(),
            voxel.y() - self.start_voxel.y(),
            voxel.z() - self.start_voxel.z(),
        ];
        std::array::from_fn(|axis| self.fraction[axis] - beyond[axis] as f64)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use voxel_space::{BrickCoordinate, FaceDirection};

    use super::*;

    fn at(x: f64, y: f64, z: f64) -> WorldPosition {
        // Positions given in voxels around the origin; split into a brick and an offset.
        let brick = |value: f64| (value / 32.0).floor() as i128;
        let b = BrickCoordinate::new(brick(x), brick(y), brick(z));
        WorldPosition::new(
            b,
            [
                x - (b.x() * 32) as f64,
                y - (b.y() * 32) as f64,
                z - (b.z() * 32) as f64,
            ],
        )
    }

    fn voxel(x: i128, y: i128, z: i128) -> VoxelCoordinate {
        VoxelCoordinate::new(x, y, z)
    }

    fn world_with(
        solids: &'static [(i128, i128, i128)],
    ) -> impl FnMut(VoxelCoordinate) -> Option<bool> {
        move |at| Some(solids.contains(&(at.x(), at.y(), at.z())))
    }

    #[test]
    fn a_ray_along_an_axis_hits_the_first_solid_voxel_and_reports_the_one_before_it() {
        let hit = cast_ray(
            GridId::MAIN,
            at(0.5, 0.5, 0.5),
            [1.0, 0.0, 0.0],
            10.0,
            world_with(&[(4, 0, 0), (6, 0, 0)]),
        );
        assert_eq!(
            hit,
            Some(RayHit {
                grid: GridId::MAIN,
                voxel: voxel(4, 0, 0),
                previous: voxel(3, 0, 0),
                distance: 3.5,
            })
        );
        let backwards = cast_ray(
            GridId::MAIN,
            at(0.5, 0.5, 0.5),
            [-1.0, 0.0, 0.0],
            10.0,
            world_with(&[(-3, 0, 0)]),
        );
        assert_eq!(
            backwards,
            Some(RayHit {
                grid: GridId::MAIN,
                voxel: voxel(-3, 0, 0),
                previous: voxel(-2, 0, 0),
                distance: 2.5,
            })
        );
    }

    #[test]
    fn the_distance_is_where_the_ray_enters_the_voxel_along_a_ray_that_is_not_along_an_axis() {
        // Straight up from a quarter of the way into a voxel, voxel 3 starts at height 3.
        let hit = cast_ray(
            GridId::MAIN,
            at(0.5, 0.25, 0.5),
            [0.0, 2.0, 0.0],
            10.0,
            world_with(&[(0, 3, 0)]),
        )
        .unwrap();
        assert_eq!(hit.distance, 2.75);
        // A slanted ray reaches the first face it crosses at its length along the ray, not along an axis: from (0.5, 0.5) at 45
        // degrees the voxel (2, 2) is entered at the corner (2, 2), 1.5 across and up, which is 1.5 * sqrt(2) along the ray.
        let hit = cast_ray(
            GridId::MAIN,
            at(0.5, 0.5, 0.5),
            [1.0, 1.0, 0.0],
            10.0,
            world_with(&[(2, 1, 0), (2, 2, 0), (1, 2, 0)]),
        )
        .unwrap();
        assert!(
            (hit.distance - 1.5 * 2.0f64.sqrt()).abs() < 1e-5,
            "{}",
            hit.distance
        );
    }

    #[test]
    fn the_range_is_respected_and_the_starting_voxel_is_ignored() {
        let solids: &'static [(i128, i128, i128)] = &[(0, 0, 0), (7, 0, 0)];
        assert_eq!(
            cast_ray(
                GridId::MAIN,
                at(0.5, 0.5, 0.5),
                [1.0, 0.0, 0.0],
                6.0,
                world_with(solids)
            ),
            None
        );
        assert!(
            cast_ray(
                GridId::MAIN,
                at(0.5, 0.5, 0.5),
                [1.0, 0.0, 0.0],
                7.0,
                world_with(solids)
            )
            .is_some()
        );
    }

    #[test]
    fn unloaded_world_stops_the_ray_without_a_hit() {
        let hit = cast_ray(
            GridId::MAIN,
            at(0.5, 0.5, 0.5),
            [0.0, 1.0, 0.0],
            50.0,
            |at| {
                if at.y() >= 3 { None } else { Some(false) }
            },
        );
        assert_eq!(hit, None);
    }

    #[test]
    fn a_zero_direction_hits_nothing() {
        assert_eq!(
            cast_ray(GridId::MAIN, at(0.0, 0.0, 0.0), [0.0; 3], 10.0, |_| Some(
                true
            )),
            None
        );
        assert_eq!(
            cast_ray(
                GridId::MAIN,
                at(0.0, 0.0, 0.0),
                [f64::NAN, 0.0, 0.0],
                10.0,
                |_| Some(true)
            ),
            None
        );
    }

    #[test]
    fn rays_cross_brick_boundaries_and_negative_coordinates() {
        let hit = cast_ray(
            GridId::MAIN,
            at(30.5, 0.5, 0.5),
            [1.0, 0.0, 0.0],
            10.0,
            world_with(&[(34, 0, 0)]),
        );
        assert_eq!(hit.map(|h| h.voxel), Some(voxel(34, 0, 0)));
        let hit = cast_ray(
            GridId::MAIN,
            at(-30.5, -40.5, 0.5),
            [0.0, -1.0, 0.0],
            10.0,
            world_with(&[(-31, -45, 0)]),
        );
        assert_eq!(
            hit.map(|h| (h.voxel, h.previous)),
            Some((voxel(-31, -45, 0), voxel(-31, -44, 0)))
        );
    }

    #[test]
    fn a_diagonal_ray_passes_through_adjacent_voxels_only() {
        // Every step changes exactly one coordinate by one: the ray never skips a voxel.
        let mut visited = Vec::new();
        let _ = cast_ray(
            GridId::MAIN,
            at(0.2, 0.3, 0.1),
            [1.0, 0.7, 0.4],
            12.0,
            |at| {
                visited.push(at);
                Some(false)
            },
        );
        assert!(visited.len() > 10);
        for pair in visited.windows(2) {
            let distance = (pair[0].x() - pair[1].x()).abs()
                + (pair[0].y() - pair[1].y()).abs()
                + (pair[0].z() - pair[1].z()).abs();
            assert_eq!(distance, 1, "{:?}", pair);
        }
    }

    fn shapes_in<'a>(
        shapes: &'a [((i128, i128, i128), Shape)],
    ) -> impl FnMut(VoxelCoordinate) -> Option<Option<&'a Shape>> {
        move |at| {
            Some(
                shapes
                    .iter()
                    .find(|(place, _)| *place == (at.x(), at.y(), at.z()))
                    .map(|(_, shape)| shape),
            )
        }
    }

    #[test]
    fn a_ray_goes_over_a_slab_and_meets_what_is_behind_it_and_one_that_goes_low_meets_the_slab() {
        let shapes = [
            ((3, 0, 0), Shape::half(FaceDirection::NegativeY)),
            ((5, 0, 0), Shape::full()),
        ];
        // At the height of the slab's top and above it the ray misses the slab and meets the cube at x = 5.
        let over = cast_surface(
            GridId::MAIN,
            at(0.5, 0.75, 0.5),
            [1.0, 0.0, 0.0],
            20.0,
            shapes_in(&shapes),
        )
        .unwrap();
        assert_eq!(over.voxel, voxel(5, 0, 0));
        assert_eq!(over.surface.face, FaceDirection::NegativeX);
        assert_eq!(over.surface.distance, 4.5);
        // Lower, it meets the slab's side.
        let low = cast_surface(
            GridId::MAIN,
            at(0.5, 0.25, 0.5),
            [1.0, 0.0, 0.0],
            20.0,
            shapes_in(&shapes),
        )
        .unwrap();
        assert_eq!(low.voxel, voxel(3, 0, 0));
        assert_eq!(low.surface.distance, 2.5);
        assert_eq!(low.surface.point, [0.0, 0.25, 0.5]);
    }

    #[test]
    fn a_ray_down_onto_a_slab_meets_its_top_in_the_middle_of_the_voxel_and_the_point_is_where_it_is_in_the_grid()
     {
        let shapes = [((10, 4, 10), Shape::half(FaceDirection::NegativeY))];
        let found = cast_surface(
            GridId::MAIN,
            at(10.25, 9.0, 10.75),
            [0.0, -1.0, 0.0],
            20.0,
            shapes_in(&shapes),
        )
        .unwrap();
        assert_eq!(found.voxel, voxel(10, 4, 10));
        assert_eq!(found.surface.face, FaceDirection::PositiveY);
        assert_eq!(found.surface.point, [0.25, 0.5, 0.75]);
        // 9.0 down to 4.5.
        assert_eq!(found.surface.distance, 4.5);
    }

    #[test]
    fn the_surface_is_found_across_bricks_and_in_negative_coordinates_and_the_range_and_the_unloaded_world_stop_it()
     {
        let shapes = [((-33, -1, 0), Shape::stair(FaceDirection::NegativeX))];
        let found = cast_surface(
            GridId::MAIN,
            at(-30.5, -0.25, 0.5),
            [-1.0, 0.0, 0.0],
            10.0,
            shapes_in(&shapes),
        )
        .unwrap();
        assert_eq!(found.voxel, voxel(-33, -1, 0));
        // The stair rises toward -x: from the +x side at the height of its upper step (-0.25 is the voxel's 0.75) the ray meets the step's open side in the middle.
        assert_eq!(found.surface.face, FaceDirection::PositiveX);
        assert_eq!(found.surface.point[0], 0.5);
        // (The voxel's near side is 1.5 along, the riser half a voxel further.)
        assert_eq!(found.surface.distance, 2.0);
        for (range, expected) in [(1.9, false), (2.0, true)] {
            assert_eq!(
                cast_surface(
                    GridId::MAIN,
                    at(-30.5, -0.25, 0.5),
                    [-1.0, 0.0, 0.0],
                    range,
                    shapes_in(&shapes)
                )
                .is_some(),
                expected,
                "range {range}"
            );
        }
        assert_eq!(
            cast_surface(
                GridId::MAIN,
                at(0.5, 0.5, 0.5),
                [0.0, 1.0, 0.0],
                50.0,
                |at| if at.y() >= 3 { None } else { Some(None) }
            ),
            None
        );
        assert_eq!(
            cast_surface(GridId::MAIN, at(0.5, 0.5, 0.5), [0.0; 3], 5.0, |_| Some(
                None
            )),
            None
        );
    }

    proptest! {
        /// Where every voxel with something in it is a whole cube, the surface is `cast_ray`'s hit: the same voxel at the same distance, and the side that looks back at the
        /// voxel the ray came from.
        #[test]
        fn among_whole_cubes_the_surface_is_the_hit_of_cast_ray(
            solids in proptest::collection::vec([-4i128..5, -4i128..5, -4i128..5], 0..40),
            origin in [-4.0f64..5.0, -4.0f64..5.0, -4.0f64..5.0],
            direction in [-1.0f64..1.0, -1.0f64..1.0, -1.0f64..1.0],
        ) {
            prop_assume!(direction.iter().any(|part| part.abs() > 1e-3));
            let full = Shape::full();
            let shapes: Vec<((i128, i128, i128), Shape)> =
                solids.iter().map(|at| ((at[0], at[1], at[2]), full.clone())).collect();
            let start = at(origin[0], origin[1], origin[2]);
            let by_ray = cast_ray(GridId::MAIN, start, direction, 30.0, |at| {
                Some(solids.contains(&[at.x(), at.y(), at.z()]))
            });
            let by_surface = cast_surface(GridId::MAIN, start, direction, 30.0, shapes_in(&shapes));
            prop_assert_eq!(by_ray.map(|hit| hit.voxel), by_surface.map(|hit| hit.voxel));
            if let (Some(ray), Some(surface)) = (by_ray, by_surface) {
                prop_assert!((ray.distance - surface.surface.distance).abs() < 1e-9);
                let toward_previous = [
                    ray.previous.x() - ray.voxel.x(),
                    ray.previous.y() - ray.voxel.y(),
                    ray.previous.z() - ray.voxel.z(),
                ];
                prop_assert_eq!(surface.surface.face.offset(), toward_previous);
            }
        }
    }

    proptest! {
        /// Aiming from anywhere at the centre of a single solid voxel hits exactly that voxel.
        #[test]
        fn aiming_at_a_lone_voxel_hits_it(
            origin in [-30.0f64..30.0, -30.0f64..30.0, -30.0f64..30.0],
            target in [-8i128..8, -8i128..8, -8i128..8],
        ) {
            let center = [target[0] as f64 + 0.5, target[1] as f64 + 0.5, target[2] as f64 + 0.5];
            let direction = [center[0] - origin[0], center[1] - origin[1], center[2] - origin[2]];
            let start_voxel = voxel(origin[0].floor() as i128, origin[1].floor() as i128, origin[2].floor() as i128);
            prop_assume!(start_voxel != voxel(target[0], target[1], target[2]));
            let distance = (direction[0] * direction[0] + direction[1] * direction[1] + direction[2] * direction[2]).sqrt();
            let hit = cast_ray(
                GridId::MAIN,
                at(origin[0], origin[1], origin[2]),
                direction,
                distance + 1.0,
                |candidate| Some(candidate == voxel(target[0], target[1], target[2])),
            );
            prop_assert_eq!(hit.map(|h| h.voxel), Some(voxel(target[0], target[1], target[2])));
        }
    }
}
