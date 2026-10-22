//! What the blocks of a world mean to something that moves through or rests on them: the collision of each block, and the terrain of a level
//! and the voxels of a ship as sources of it for [`physics`].

use std::cell::RefCell;
use std::collections::HashMap;

use block_models::{BlockTable, PropertyHandle};
use physics::{BrickHint, Collision, CollisionSource};
use server::{GridView, ServerWorld};
use voxel_grid::{ChannelIndex, Grid};
use voxel_space::{BrickCoordinate, GridId, LevelIndex, VoxelCoordinate};

/// What each block's shape means to something moving through it, worked out once: the whole cube, the boxes in it, or nothing (air, and
/// the blocks a game says can be passed through, such as flames). The sources below share one, so a body that meets several ships at
/// once needs only the one table.
#[derive(Debug)]
pub struct BlockCollisions {
    table: BlockTable,
    passable: Option<PropertyHandle<bool>>,
    cache: RefCell<HashMap<u32, Collision>>,
}

impl BlockCollisions {
    /// `passable` is the property of the game that says a block can be passed through, if it has one.
    pub fn new(table: BlockTable, passable: Option<PropertyHandle<bool>>) -> Self {
        Self {
            table,
            passable,
            cache: RefCell::new(HashMap::new()),
        }
    }

    pub fn table(&self) -> &BlockTable {
        &self.table
    }

    /// What the block in this state is to a body.
    pub fn of(&self, block: u32, state: u32) -> Collision {
        let model = self.table.model_of(block, state);
        let table = &self.table;
        let passable = self.passable;
        self.cache
            .borrow_mut()
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
    pub collisions: &'a BlockCollisions,
}

impl CollisionSource for WorldTerrain<'_> {
    fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
        let Ok(mut terrain) = self.world.grid(GridId::root(self.level)) else {
            return Collision::Full;
        };
        let (Ok(block), Ok(state)) = (
            terrain.get(self.block_channel, voxel),
            terrain.get(self.state_channel, voxel),
        ) else {
            return Collision::Full;
        };
        self.collisions.of(block, state)
    }

    fn brick_hint(&mut self, brick: BrickCoordinate) -> BrickHint {
        let Ok(mut terrain) = self.world.grid(GridId::root(self.level)) else {
            return BrickHint::Unknown;
        };
        let Ok(Some(block)) = terrain.uniform_value(self.block_channel, brick) else {
            return BrickHint::Unknown;
        };
        if block == self.air {
            return BrickHint::Empty;
        }
        match terrain.uniform_value(self.state_channel, brick) {
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
    pub collisions: &'a BlockCollisions,
}

impl ShipHull<'_> {
    /// Whether a place in the ship's own coordinates, in voxels, is inside a block that something can stand on or bump into: the whole of
    /// a cube, or one of the boxes of a block that fills only part of its cell. A place exactly on the top face of a box is not inside it.
    pub fn solid_at(&mut self, place: [f64; 3]) -> bool {
        let cell = place.map(f64::floor);
        let Ok(voxel) = VoxelCoordinate::try_new(cell[0] as i64, cell[1] as i64, cell[2] as i64)
        else {
            return false;
        };
        let inside = [0, 1, 2].map(|axis| place[axis] - cell[axis]);
        match self.collision_at(voxel) {
            Collision::Empty => false,
            Collision::Full => true,
            Collision::Partial(boxes) => boxes.iter().any(|part| {
                (0..3).all(|axis| {
                    inside[axis] >= f64::from(part.min[axis]) / 8.0
                        && inside[axis] < f64::from(part.max[axis]) / 8.0
                })
            }),
        }
    }
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use block_models::{BlockDefinition, Shape};
    use voxel_grid::{ChannelDefinition, GridLayout};
    use voxel_space::FaceDirection;

    use super::*;

    const AIR: u32 = 0;
    const STONE: u32 = 1;

    /// A table with air and a stone that has three shapes: whole, the bottom half, the top half.
    fn collisions() -> BlockCollisions {
        let air = BlockDefinition::air();
        let stone = BlockDefinition::cube([0.5; 3]).with_shapes(vec![
            Shape::full(),
            Shape::half(FaceDirection::NegativeY),
            Shape::half(FaceDirection::PositiveY),
        ]);
        BlockCollisions::new(
            BlockTable::from_definitions(&[Some(&air), Some(&stone)]),
            None,
        )
    }

    /// A grid with a whole stone at (3, 4, 5), a bottom half at (6, 4, 5) and a top half at (9, 4, 5).
    fn grid() -> (Grid, ChannelIndex, ChannelIndex) {
        let layout = Arc::new(
            GridLayout::new(vec![
                ChannelDefinition::number("test:block".parse().unwrap(), AIR),
                ChannelDefinition::number("test:state".parse().unwrap(), 0),
            ])
            .unwrap(),
        );
        let mut voxels = Grid::new(layout);
        let mut channels = voxels.layout().channel_indices();
        let (block, state) = (channels.next().unwrap(), channels.next().unwrap());
        for (x, shape) in [(3, 0), (6, 1), (9, 2)] {
            let voxel = VoxelCoordinate::new(x, 4, 5);
            voxels.set(block, voxel, STONE);
            voxels.set(state, voxel, shape);
        }
        (voxels, block, state)
    }

    #[test]
    fn a_place_is_solid_inside_a_whole_block_and_inside_the_boxes_of_a_part_block_and_nowhere_else()
    {
        let collisions = collisions();
        let (voxels, block_channel, state_channel) = grid();
        let mut hull = ShipHull {
            voxels: &voxels,
            block_channel,
            state_channel,
            collisions: &collisions,
        };
        // A whole block: all of it, its low faces, and not past the high ones.
        assert!(hull.solid_at([3.5, 4.5, 5.5]));
        assert!(hull.solid_at([3.0, 4.0, 5.0]));
        assert!(!hull.solid_at([4.0, 4.5, 5.5]));
        assert!(!hull.solid_at([3.5, 5.0, 5.5]));
        // The lowest corner of a box is in it: the boxes include their low faces.
        assert!(hull.solid_at([6.0, 4.0, 5.0]));
        // The bottom half: the lower half of the cell, and the top face of the box is not in it.
        assert!(hull.solid_at([6.5, 4.25, 5.5]));
        assert!(!hull.solid_at([6.5, 4.5, 5.5]));
        assert!(!hull.solid_at([6.5, 4.75, 5.5]));
        // The top half: the upper half.
        assert!(hull.solid_at([9.5, 4.75, 5.5]));
        assert!(hull.solid_at([9.5, 4.5, 5.5]));
        assert!(!hull.solid_at([9.5, 4.25, 5.5]));
        // Air, and places a long way from the ship, and negative ones.
        assert!(!hull.solid_at([0.5, 0.5, 0.5]));
        assert!(!hull.solid_at([-100.5, -3.5, 1.0e6]));
        // Sources made at once with one table share what it worked out.
        let mut other = ShipHull {
            voxels: &voxels,
            block_channel,
            state_channel,
            collisions: &collisions,
        };
        assert!(other.solid_at([3.5, 4.5, 5.5]) && hull.solid_at([3.5, 4.5, 5.5]));
    }
}
