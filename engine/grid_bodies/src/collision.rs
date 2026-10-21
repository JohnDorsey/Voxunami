//! What the blocks of a world mean to something that moves through or rests on them: the collision of each block, and the terrain of a level
//! and the voxels of a ship as sources of it for [`physics`].

use std::collections::HashMap;

use block_models::{BlockTable, PropertyHandle};
use physics::{BrickHint, Collision, CollisionSource};
use server::ServerWorld;
use voxel_grid::{ChannelIndex, Grid};
use voxel_space::{BrickCoordinate, LevelIndex, VoxelCoordinate};

/// What each block's shape means to something moving through it, worked out once: the whole cube, the boxes in it, or nothing (air, and
/// the blocks a game says can be passed through, such as flames).
#[derive(Debug)]
pub struct BlockCollisions {
    table: BlockTable,
    passable: Option<PropertyHandle<bool>>,
    cache: HashMap<u32, Collision>,
}

impl BlockCollisions {
    /// `passable` is the property of the game that says a block can be passed through, if it has one.
    pub fn new(table: BlockTable, passable: Option<PropertyHandle<bool>>) -> Self {
        Self {
            table,
            passable,
            cache: HashMap::new(),
        }
    }

    pub fn table(&self) -> &BlockTable {
        &self.table
    }

    /// What the block in this state is to a body.
    pub fn of(&mut self, block: u32, state: u32) -> Collision {
        let model = self.table.model_of(block, state);
        let table = &self.table;
        let passable = self.passable;
        self.cache
            .entry(model.get())
            .or_insert_with(|| {
                let shape = table.shape(model);
                if passable.is_some_and(|handle| table.get(handle, model)) || shape.is_empty() {
                    Collision::Empty
                } else if shape.is_full() {
                    Collision::Full
                } else {
                    Collision::Partial(shape.boxes().to_vec().into())
                }
            })
            .clone()
    }
}

/// One level of the server's world, as terrain for a body to move through. Terrain that cannot be read is a wall, not a void.
#[derive(Debug)]
pub struct WorldTerrain<'a> {
    pub world: &'a mut ServerWorld,
    pub level: LevelIndex,
    pub block_channel: ChannelIndex,
    pub state_channel: ChannelIndex,
    pub air: u32,
    pub collisions: &'a mut BlockCollisions,
}

impl CollisionSource for WorldTerrain<'_> {
    fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
        let (Ok(block), Ok(state)) = (
            self.world.voxel(self.level, self.block_channel, voxel),
            self.world.voxel(self.level, self.state_channel, voxel),
        ) else {
            return Collision::Full;
        };
        self.collisions.of(block, state)
    }

    fn brick_hint(&mut self, brick: BrickCoordinate) -> BrickHint {
        let Ok(Some(block)) = self
            .world
            .uniform_value(self.level, self.block_channel, brick)
        else {
            return BrickHint::Unknown;
        };
        if block == self.air {
            return BrickHint::Empty;
        }
        match self
            .world
            .uniform_value(self.level, self.state_channel, brick)
        {
            Ok(Some(state)) => match self.collisions.of(block, state) {
                Collision::Empty => BrickHint::Empty,
                Collision::Full => BrickHint::Solid,
                Collision::Partial(_) => BrickHint::Unknown,
            },
            _ => BrickHint::Unknown,
        }
    }
}

/// The voxels of a ship, in the ship's own coordinates: where nothing was put there is nothing.
#[derive(Debug)]
pub struct ShipHull<'a> {
    pub voxels: &'a Grid,
    pub block_channel: ChannelIndex,
    pub state_channel: ChannelIndex,
    pub collisions: &'a mut BlockCollisions,
}

impl CollisionSource for ShipHull<'_> {
    fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
        let block = self.voxels.get(self.block_channel, voxel);
        let state = self.voxels.get(self.state_channel, voxel);
        self.collisions.of(block, state)
    }

    fn brick_hint(&mut self, brick: BrickCoordinate) -> BrickHint {
        if self.voxels.brick(brick).is_none() {
            BrickHint::Empty
        } else {
            BrickHint::Unknown
        }
    }
}
