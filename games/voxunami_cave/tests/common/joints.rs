//! Ships with joints, for the tests of the dev ball joint (the ships and the way a block is made in them are those of `docking`).

use game_shell::GameContent;
use voxel_space::FaceDirection;
use voxunami_cave::content::{CaveContent, dev_ball_joint_id};
use voxunami_cave::placement::turn_carrying;

use super::docking::Look;

pub fn joint_block() -> u32 {
    CaveContent::new().registries()[0]
        .1
        .raw_id_of(&dev_ball_joint_id())
        .expect("Cave has the dev ball joint")
}

/// A dev ball joint turned so that its ball end (the face that is its joint face, which is up as it is made) looks this way.
pub fn ball_end_toward(face: FaceDirection) -> Look {
    Look {
        state: 0,
        orientation: u32::from(turn_carrying(FaceDirection::PositiveY, face).index()),
    }
}
