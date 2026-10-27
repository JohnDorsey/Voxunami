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
    pub distance: f32,
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
    direction: [f32; 3],
    maximum_distance: f32,
    mut is_solid: impl FnMut(VoxelCoordinate) -> Option<bool>,
) -> Option<RayHit> {
    let length =
        (direction[0] * direction[0] + direction[1] * direction[1] + direction[2] * direction[2])
            .sqrt();
    if length == 0.0 || !length.is_finite() {
        return None;
    }
    let direction = direction.map(|component| component / length);

    // Work in voxels relative to the start brick, so the arithmetic stays small and exact enough.
    let offset = start.offset();
    let cell = offset.map(|component| component.floor());
    let mut voxel = start.brick().voxel_at(LocalVoxelIndex::new(
        cell[0] as u8,
        cell[1] as u8,
        cell[2] as u8,
    ));
    let fraction: [f32; 3] = std::array::from_fn(|axis| offset[axis] - cell[axis]);

    let mut step = [0i128; 3];
    let mut next_boundary = [f32::INFINITY; 3];
    let mut boundary_spacing = [f32::INFINITY; 3];
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

    loop {
        let axis = (0..3)
            .min_by(|&a, &b| next_boundary[a].total_cmp(&next_boundary[b]))
            .expect("three axes");
        if next_boundary[axis] > maximum_distance {
            return None;
        }
        let previous = voxel;
        let entered = next_boundary[axis];
        let moved = [
            voxel.x() + if axis == 0 { step[0] } else { 0 },
            voxel.y() + if axis == 1 { step[1] } else { 0 },
            voxel.z() + if axis == 2 { step[2] } else { 0 },
        ];
        voxel = VoxelCoordinate::try_new(moved[0], moved[1], moved[2]).ok()?;
        next_boundary[axis] += boundary_spacing[axis];
        if is_solid(voxel)? {
            return Some(RayHit {
                grid,
                voxel,
                previous,
                distance: entered,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use voxel_space::BrickCoordinate;

    use super::*;

    fn at(x: f32, y: f32, z: f32) -> WorldPosition {
        // Positions given in voxels around the origin; split into a brick and an offset.
        let brick = |value: f32| (value / 32.0).floor() as i128;
        let b = BrickCoordinate::new(brick(x), brick(y), brick(z));
        WorldPosition::new(
            b,
            [
                x - (b.x() * 32) as f32,
                y - (b.y() * 32) as f32,
                z - (b.z() * 32) as f32,
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
            (hit.distance - 1.5 * 2.0f32.sqrt()).abs() < 1e-5,
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
                [f32::NAN, 0.0, 0.0],
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

    proptest! {
        /// Aiming from anywhere at the centre of a single solid voxel hits exactly that voxel.
        #[test]
        fn aiming_at_a_lone_voxel_hits_it(
            origin in [-30.0f32..30.0, -30.0f32..30.0, -30.0f32..30.0],
            target in [-8i128..8, -8i128..8, -8i128..8],
        ) {
            let center = [target[0] as f32 + 0.5, target[1] as f32 + 0.5, target[2] as f32 + 0.5];
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
