//! Ships with docking ports, for the tests of docking and of what docks where the world moves.

use game_shell::GameContent;
use server::GridView;
use voxel_space::{FaceDirection, GridPose, LevelIndex, VoxelCoordinate};
use voxunami_cave::content::{CaveContent, dev_docking_port_id};
use voxunami_cave::placement::turn_carrying;

use super::Scene;

/// How a block is made in a ship: its state, and which way it is turned (the number of a turn of the lattice).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look {
    pub state: u32,
    pub orientation: u32,
}

/// A block as it is made, not turned.
pub const PLAIN: Look = Look {
    state: 0,
    orientation: 0,
};

/// A dev docking port turned so that its yellow face (the east, as it is made) is this one.
pub fn facing(face: FaceDirection) -> Look {
    Look {
        state: 0,
        orientation: u32::from(turn_carrying(FaceDirection::PositiveX, face).index()),
    }
}

pub fn port_block() -> u32 {
    CaveContent::new().registries()[0]
        .1
        .raw_id_of(&dev_docking_port_id())
        .expect("Cave has the dev docking port")
}

pub fn stone() -> u32 {
    CaveContent::new().blocks.stone
}

/// A block of a ship: where it is in the ship, which block, and how it is made.
pub type Placed = ([i128; 3], u32, Look);

pub fn stones(from: i128, to: i128) -> Vec<Placed> {
    (from..to).map(|x| ([x, 0, 0], stone(), PLAIN)).collect()
}

/// Makes a ship of these blocks at this pose, held if asked.
pub fn make(scene: &mut Scene, at: GridPose, held: bool, blocks: Vec<Placed>) {
    let content = CaveContent::new();
    scene.run(move |world| {
        let grid = world.create_grid(LevelIndex::MAIN, at).unwrap();
        world.set_grid_held(grid, held).unwrap();
        let mut view = world.grid(grid).unwrap();
        for ([x, y, z], block, look) in blocks {
            let voxel = VoxelCoordinate::new(x, y, z);
            view.set(content.state_channel(), voxel, look.state)
                .unwrap();
            view.set(content.orientation_channel(), voxel, look.orientation)
                .unwrap();
            view.set(content.block_channel(), voxel, block).unwrap();
        }
    });
}

/// A ship with a port on its east face at the end of a row of stone, and one with a port on its west face at the start: 4 and 3 blocks.
pub fn a_pair() -> (Vec<Placed>, Vec<Placed>) {
    let mut first = stones(0, 3);
    first.push(([3, 0, 0], port_block(), facing(FaceDirection::PositiveX)));
    let mut second = vec![([0, 0, 0], port_block(), facing(FaceDirection::NegativeX))];
    second.extend(stones(1, 3));
    (first, second)
}
