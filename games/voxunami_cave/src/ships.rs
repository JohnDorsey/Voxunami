//! The rules of Cave's ships that need no server: which blocks make a ship, and what a pilot's keys do to one. The `/ship` commands
//! that use them are in [`crate::plugins`].

use std::collections::{BTreeSet, VecDeque};

use voxel_space::{BrickCoordinate, GridPose, Orientation, VoxelCoordinate, WorldPosition};

/// The most blocks `/ship create` takes into a ship.
pub const MAXIMUM_BLOCKS: usize = 4096;

/// How far, in voxels, `/ship create` looks for the block to take.
pub const LOOK_REACH: f32 = 24.0;

/// How near, in voxels, a ship must be to a player who is not aboard one for `/ship push`, `hold` and `pilot` to mean it.
pub const NEAR: f64 = 48.0;

/// How fast a pilot's keys speed a ship up, in voxels a second a second; the sprint key doubles it.
pub const THRUST: f32 = 24.0;

/// How much faster the sprint key makes the thrust.
pub const BOOST: f32 = 2.0;

/// How much of its speed a piloted ship loses in a second, so that it stops when the keys are let go: the fastest it goes is
/// [`THRUST`] over this.
pub const DRAG: f32 = 1.0;

/// The length of a tick in seconds: a push that is a change of speed is a force for one tick.
pub const TICK_SECONDS: f32 = 0.05;

/// The voxels joined to `start` through faces (6-connected) that `wanted` says yes to, in order, or `None` if there are more than
/// `limit`. Empty if `start` is not wanted.
pub fn connected_blocks(
    start: VoxelCoordinate,
    limit: usize,
    mut wanted: impl FnMut(VoxelCoordinate) -> bool,
) -> Option<Vec<VoxelCoordinate>> {
    let mut found = BTreeSet::new();
    if !wanted(start) {
        return Some(Vec::new());
    }
    found.insert(start);
    let mut next = VecDeque::from([start]);
    while let Some(voxel) = next.pop_front() {
        for [dx, dy, dz] in [
            [1, 0, 0],
            [-1, 0, 0],
            [0, 1, 0],
            [0, -1, 0],
            [0, 0, 1],
            [0, 0, -1],
        ] {
            let Ok(beside) =
                VoxelCoordinate::try_new(voxel.x() + dx, voxel.y() + dy, voxel.z() + dz)
            else {
                continue;
            };
            if !found.contains(&beside) && wanted(beside) {
                if found.len() == limit {
                    return None;
                }
                found.insert(beside);
                next.push_back(beside);
            }
        }
    }
    Some(found.into_iter().collect())
}

/// The keys a pilot holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PilotKeys {
    /// 1 to go the way they look, -1 for backwards.
    pub forward: i8,
    /// 1 to go to the right, -1 to the left.
    pub strafe: i8,
    /// 1 to go up, -1 to go down.
    pub vertical: i8,
    pub sprint: bool,
}

/// The force, in kilograms times voxels a second a second, that the keys of a pilot who looks as `view` says put on a ship of this
/// `mass` that is going at `velocity` in a place where gravity is `gravity` (all in the world's axes): the ship is held up against
/// gravity, speeds up toward where the keys point (forward is the way the pilot looks, up is the world's), and loses a part of its
/// speed each second, so that with no keys it hangs where it is.
pub fn pilot_force(
    mass: f32,
    gravity: [f32; 3],
    velocity: [f32; 3],
    keys: PilotKeys,
    view: Orientation,
) -> [f32; 3] {
    let turn = GridPose::new(WorldPosition::new(BrickCoordinate::ORIGIN, [0.0; 3]), view);
    let forward = turn.direction_to_world([0.0, 0.0, -1.0]);
    let right = turn.direction_to_world([1.0, 0.0, 0.0]);
    let boost = if keys.sprint { BOOST } else { 1.0 };
    [0, 1, 2].map(|axis| {
        let up = if axis == 1 {
            f32::from(keys.vertical)
        } else {
            0.0
        };
        let wish = THRUST
            * boost
            * (f32::from(keys.forward) * forward[axis] + f32::from(keys.strafe) * right[axis] + up);
        mass * (wish - gravity[axis] - DRAG * velocity[axis])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voxel(x: i64, y: i64, z: i64) -> VoxelCoordinate {
        VoxelCoordinate::new(x, y, z)
    }

    #[test]
    fn the_blocks_joined_through_faces_are_found_and_those_touching_by_an_edge_or_corner_are_not() {
        let blocks: BTreeSet<VoxelCoordinate> = [
            voxel(0, 0, 0),
            voxel(1, 0, 0),
            voxel(1, 1, 0),
            voxel(1, 1, -1),
            // Across a brick boundary and below zero.
            voxel(-1, 0, 0),
            // Only an edge away from the others.
            voxel(2, 2, 0),
            // Only a corner away.
            voxel(0, 1, 1),
            // Far away.
            voxel(50, 0, 0),
        ]
        .into_iter()
        .collect();
        let found = connected_blocks(voxel(1, 0, 0), 100, |at| blocks.contains(&at)).unwrap();
        assert_eq!(
            found,
            [
                voxel(-1, 0, 0),
                voxel(0, 0, 0),
                voxel(1, 0, 0),
                voxel(1, 1, -1),
                voxel(1, 1, 0)
            ]
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
        );
        // Starting from any of them gives the same blocks, whichever way the others are reached (through every face, in all six ways).
        for start in &found {
            assert_eq!(
                connected_blocks(*start, 100, |at| blocks.contains(&at)).as_ref(),
                Some(&found),
                "from {start:?}"
            );
        }
    }

    #[test]
    fn nothing_is_found_from_a_block_that_is_not_wanted_and_too_many_are_none() {
        assert_eq!(
            connected_blocks(voxel(5, 5, 5), 10, |_| false),
            Some(Vec::new())
        );
        let line = |at: VoxelCoordinate| at.y() == 0 && at.z() == 0 && (0..20).contains(&at.x());
        assert_eq!(
            connected_blocks(voxel(0, 0, 0), 20, line).unwrap().len(),
            20
        );
        assert_eq!(connected_blocks(voxel(0, 0, 0), 19, line), None);
        // The ground, which goes on for ever, is too many however large the limit.
        assert_eq!(
            connected_blocks(voxel(0, 0, 0), 500, |at| at.y() <= 0),
            None
        );
    }

    fn keys(forward: i8, strafe: i8, vertical: i8, sprint: bool) -> PilotKeys {
        PilotKeys {
            forward,
            strafe,
            vertical,
            sprint,
        }
    }

    #[test]
    fn with_no_keys_the_ship_hangs_against_gravity_and_slows_down() {
        let gravity = [0.0, -32.0, 0.0];
        let still = pilot_force(
            1000.0,
            gravity,
            [0.0; 3],
            keys(0, 0, 0, false),
            Orientation::IDENTITY,
        );
        assert_eq!(still, [0.0, 32_000.0, 0.0]);
        // Going 10 east, it is pulled back by a tenth of that a second, per kilogram.
        let drifting = pilot_force(
            1000.0,
            gravity,
            [10.0, 0.0, 0.0],
            keys(0, 0, 0, false),
            Orientation::IDENTITY,
        );
        assert_eq!(drifting, [-10_000.0, 32_000.0, 0.0]);
    }

    #[test]
    fn the_keys_point_where_the_pilot_looks_and_up_is_up() {
        let gravity = [0.0; 3];
        let none = [0.0; 3];
        // Looking along -z (yaw 0): forward is -z, strafing right is +x.
        let look = Orientation::IDENTITY;
        assert_eq!(
            pilot_force(1.0, gravity, none, keys(1, 0, 0, false), look),
            [0.0, 0.0, -THRUST]
        );
        assert_eq!(
            pilot_force(1.0, gravity, none, keys(-1, 0, 0, false), look),
            [0.0, 0.0, THRUST]
        );
        assert_eq!(
            pilot_force(1.0, gravity, none, keys(0, 1, 0, false), look),
            [THRUST, 0.0, 0.0]
        );
        assert_eq!(
            pilot_force(1.0, gravity, none, keys(0, 0, 1, false), look),
            [0.0, THRUST, 0.0]
        );
        // The sprint key doubles it, and a quarter turn to the left (positive yaw) points forward along -x.
        let left = Orientation::from_yaw_pitch(std::f32::consts::FRAC_PI_2, 0.0);
        let force = pilot_force(1.0, gravity, none, keys(1, 0, 0, true), left);
        assert!((force[0] + THRUST * BOOST).abs() < 1e-3, "{force:?}");
        assert!(force[1].abs() < 1e-3 && force[2].abs() < 1e-3, "{force:?}");
        // Looking down a little, forward goes down a little.
        let down = Orientation::from_yaw_pitch(0.0, -0.5);
        assert!(pilot_force(1.0, gravity, none, keys(1, 0, 0, false), down)[1] < 0.0);
    }

    #[test]
    fn the_force_is_in_proportion_to_the_mass() {
        let force = |mass| {
            pilot_force(
                mass,
                [0.0, -32.0, 0.0],
                [1.0, 2.0, 3.0],
                keys(1, 1, 1, false),
                Orientation::from_yaw_pitch(0.3, 0.2),
            )
        };
        let (small, large) = (force(100.0), force(400.0));
        for axis in 0..3 {
            assert!((large[axis] - 4.0 * small[axis]).abs() < 1e-2 * large[axis].abs().max(1.0));
        }
    }
}
