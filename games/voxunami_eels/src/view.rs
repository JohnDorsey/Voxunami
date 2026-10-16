//! Where each player's camera is: behind their eel, or for a player with no eel, in a corner of
//! the arena looking across it.
//!
//! The server works these out and tells everyone (as the players' poses), so the camera is the
//! same for the player, for other players, and in a recording.

use game_shell::PlayerPose;
use voxel_space::{VoxelCoordinate, WorldPosition};

use crate::arena::{ARENA_EDGE, Cell};
use crate::round::Eel;

/// The middle of a cell, moved by `offset`.
fn position_of(cell: Cell, offset: [f32; 3]) -> WorldPosition {
    let (brick, local) = VoxelCoordinate::new(cell[0], cell[1], cell[2]).split();
    WorldPosition::new(
        brick,
        [
            f32::from(local.x()) + 0.5,
            f32::from(local.y()) + 0.5,
            f32::from(local.z()) + 0.5,
        ],
    )
    .translated(offset)
}

/// The camera of a player whose eel is `eel`, which has moved `fraction` (from 0 to just under 1)
/// of the way to its next cell. A dead eel does not move.
pub fn eel_pose(eel: &Eel, fraction: f32) -> PlayerPose {
    let orientation = eel.orientation();
    let advance = if eel.is_alive() { fraction } else { 0.0 };
    let heading = orientation.heading().offset();
    let behind = orientation.view_offset();
    let offset = std::array::from_fn(|axis| behind[axis] + heading[axis] as f32 * advance);
    PlayerPose {
        position: position_of(eel.head(), offset),
        yaw: orientation.view_yaw(),
        pitch: orientation.view_pitch(),
    }
}

/// The camera of a player who is not swimming: in the top corner looking toward the middle.
pub fn spectator_pose() -> PlayerPose {
    let corner = ARENA_EDGE - 4;
    PlayerPose {
        position: position_of([3, corner, 3], [0.0; 3]),
        yaw: -3.0 * std::f32::consts::FRAC_PI_4,
        pitch: -0.45,
    }
}

#[cfg(test)]
mod tests {
    use voxel_space::FaceDirection;

    use super::*;
    use crate::arena::Cell;
    use crate::round::{Arena, Match, Paint, STEP_TICKS, start_of};

    /// An arena of nothing, for getting an eel to look at.
    struct Nothing;

    impl Arena for Nothing {
        fn is_occupied(&mut self, _cell: Cell) -> bool {
            false
        }
        fn paint(&mut self, _cell: Cell, _paint: Paint) {}
        fn clear_inside(&mut self) {}
    }

    fn voxels_of(position: WorldPosition) -> [f32; 3] {
        let brick = position.brick();
        std::array::from_fn(|axis| {
            [brick.x(), brick.y(), brick.z()][axis] as f32 * 32.0 + position.offset()[axis]
        })
    }

    #[test]
    fn the_camera_is_behind_and_above_the_eel_and_moves_with_it_between_steps() {
        let mut game = Match::new();
        game.join(0);
        game.tick(&mut Nothing);
        let eel = game.eel(0).unwrap();
        let (head, heading) = start_of(0);
        assert_eq!(heading, FaceDirection::PositiveZ);
        let pose = eel_pose(eel, 0.0);
        let at = voxels_of(pose.position);
        // The head's middle is half a cell in; the camera is 6 behind (-z) and 2.5 above.
        assert_eq!(
            at,
            [
                head[0] as f32 + 0.5,
                head[1] as f32 + 0.5 + 2.5,
                head[2] as f32 + 0.5 - 6.0
            ]
        );
        assert!(
            (pose.yaw - std::f32::consts::PI).abs() < 1e-6,
            "looking toward +z"
        );
        assert!(pose.pitch < 0.0);
        // A third of the way to the next cell is a third of a cell on.
        let later = voxels_of(eel_pose(eel, 1.0 / STEP_TICKS as f32).position);
        assert!((later[2] - at[2] - 1.0 / 3.0).abs() < 1e-5);
        assert_eq!((later[0], later[1]), (at[0], at[1]));
    }

    #[test]
    fn a_dead_eels_camera_stays_where_it_was() {
        let (mut game, mut arena) = (Match::new(), Nothing);
        game.join(0);
        for _ in 0..crate::round::COUNTDOWN_TICKS + 2 {
            game.tick(&mut arena);
        }
        // Swim into the wall.
        for _ in 0..60 * STEP_TICKS {
            game.tick(&mut arena);
        }
        let eel = game.eel(0).unwrap();
        assert!(!eel.is_alive());
        assert_eq!(eel_pose(eel, 0.0), eel_pose(eel, 0.9));
    }

    #[test]
    fn spectators_look_across_the_arena_from_inside_it() {
        let pose = spectator_pose();
        let at = voxels_of(pose.position);
        assert!(
            at.iter().all(|&c| (0.0..ARENA_EDGE as f32).contains(&c)),
            "{at:?}"
        );
        assert!(pose.pitch < 0.0 && pose.yaw < 0.0);
    }
}
