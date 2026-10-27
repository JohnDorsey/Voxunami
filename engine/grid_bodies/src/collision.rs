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

    /// What the block in this state, standing in this orientation, is to a body.
    pub fn of(&self, block: u32, state: u32, orientation: u32) -> Collision {
        let model = self.table.model_of(block, state, orientation);
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

/// What the block at a voxel is to a body, given how to read the value a channel has there (`None` when it cannot be read: terrain that cannot be read is a wall, not
/// a void). The sources below differ in where the values come from and in nothing else.
fn collision_of_voxel(
    collisions: &BlockCollisions,
    block_channel: ChannelIndex,
    state_channel: ChannelIndex,
    orientation_channel: Option<ChannelIndex>,
    mut read: impl FnMut(ChannelIndex) -> Option<u32>,
) -> Collision {
    let (Some(block), Some(state)) = (read(block_channel), read(state_channel)) else {
        return Collision::Full;
    };
    let orientation = match orientation_channel {
        Some(channel) => match read(channel) {
            Some(orientation) => orientation,
            None => return Collision::Full,
        },
        None => 0,
    };
    collisions.of(block, state, orientation)
}

/// What a brick is to a body without looking at its voxels, given how to ask for the value that every voxel of it has in a channel (`None` when they are not all the
/// same, or it cannot be known): empty when it is all air, solid when it is all one block that fills its cell, and otherwise not known. (Whether a shape is empty, a
/// whole cell or part of one does not depend on how it is turned, so the orientations are not asked about.)
fn hint_of_brick(
    collisions: &BlockCollisions,
    air: u32,
    block_channel: ChannelIndex,
    state_channel: ChannelIndex,
    mut uniform: impl FnMut(ChannelIndex) -> Option<u32>,
) -> BrickHint {
    let Some(block) = uniform(block_channel) else {
        return BrickHint::Unknown;
    };
    if block == air {
        return BrickHint::Empty;
    }
    let Some(state) = uniform(state_channel) else {
        return BrickHint::Unknown;
    };
    match collisions.of(block, state, 0) {
        Collision::Empty => BrickHint::Empty,
        Collision::Full => BrickHint::Solid,
        Collision::Partial(_) => BrickHint::Unknown,
    }
}

/// One level of the server's world, as terrain for a body to move through. Terrain that cannot be read is a wall, not a void.
#[derive(Debug)]
pub struct WorldTerrain<'a> {
    pub world: &'a mut ServerWorld,
    pub level: LevelIndex,
    pub block_channel: ChannelIndex,
    pub state_channel: ChannelIndex,
    /// Where the game keeps the orientations of its blocks, if it has them.
    pub orientation_channel: Option<ChannelIndex>,
    pub air: u32,
    pub collisions: &'a BlockCollisions,
}

impl CollisionSource for WorldTerrain<'_> {
    fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
        let Ok(mut terrain) = self.world.grid(GridId::root(self.level)) else {
            return Collision::Full;
        };
        collision_of_voxel(
            self.collisions,
            self.block_channel,
            self.state_channel,
            self.orientation_channel,
            |channel| terrain.get(channel, voxel).ok(),
        )
    }

    fn brick_hint(&mut self, brick: BrickCoordinate) -> BrickHint {
        let Ok(mut terrain) = self.world.grid(GridId::root(self.level)) else {
            return BrickHint::Unknown;
        };
        hint_of_brick(
            self.collisions,
            self.air,
            self.block_channel,
            self.state_channel,
            |channel| terrain.uniform_value(channel, brick).ok().flatten(),
        )
    }
}

/// The blocks of a ship that has terrain (a planet), in the ship's own coordinates: the stored brick if there is one, else the brick its generator makes. Terrain that
/// cannot be read is a wall. It needs only a shared borrow of the world, so a body can meet it alongside the other ships.
#[derive(Debug)]
pub struct GeneratedHull<'a> {
    pub world: &'a ServerWorld,
    pub grid: GridId,
    pub block_channel: ChannelIndex,
    pub state_channel: ChannelIndex,
    pub orientation_channel: Option<ChannelIndex>,
    pub air: u32,
    pub collisions: &'a BlockCollisions,
}

impl CollisionSource for GeneratedHull<'_> {
    fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
        collision_of_voxel(
            self.collisions,
            self.block_channel,
            self.state_channel,
            self.orientation_channel,
            |channel| self.world.moving_voxel(self.grid, channel, voxel).ok(),
        )
    }

    fn brick_hint(&mut self, brick: BrickCoordinate) -> BrickHint {
        hint_of_brick(
            self.collisions,
            self.air,
            self.block_channel,
            self.state_channel,
            |channel| {
                self.world
                    .moving_uniform_value(self.grid, channel, brick)
                    .ok()
                    .flatten()
            },
        )
    }
}

/// What a body meets of a ship: the blocks stored in its voxels, or for a ship with terrain the blocks its generator makes as well.
#[derive(Debug)]
pub enum ShipSource<'a> {
    Stored(ShipHull<'a>),
    Generated(GeneratedHull<'a>),
}

impl CollisionSource for ShipSource<'_> {
    fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
        match self {
            Self::Stored(hull) => hull.collision_at(voxel),
            Self::Generated(hull) => hull.collision_at(voxel),
        }
    }

    fn brick_hint(&mut self, brick: BrickCoordinate) -> BrickHint {
        match self {
            Self::Stored(hull) => hull.brick_hint(brick),
            Self::Generated(hull) => hull.brick_hint(brick),
        }
    }
}

/// Whether a place in a source's own coordinates, in voxels, is inside a block that something can stand on or bump into: the whole of a cube, or one of the
/// boxes of a block that fills only part of its cell. A place exactly on the top face of a box is not inside it.
pub trait SolidAt: CollisionSource {
    fn solid_at(&mut self, place: [f64; 3]) -> bool {
        let cell = place.map(f64::floor);
        let Ok(voxel) = VoxelCoordinate::try_new(cell[0] as i128, cell[1] as i128, cell[2] as i128)
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

impl<T: CollisionSource + ?Sized> SolidAt for T {}

/// The voxels of a ship, in the ship's own coordinates: where nothing was put there is nothing.
#[derive(Debug)]
pub struct ShipHull<'a> {
    pub voxels: &'a Grid,
    pub block_channel: ChannelIndex,
    pub state_channel: ChannelIndex,
    pub orientation_channel: Option<ChannelIndex>,
    pub collisions: &'a BlockCollisions,
}

impl CollisionSource for ShipHull<'_> {
    fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
        let block = self.voxels.get(self.block_channel, voxel);
        let state = self.voxels.get(self.state_channel, voxel);
        let orientation = self
            .orientation_channel
            .map_or(0, |channel| self.voxels.get(channel, voxel));
        self.collisions.of(block, state, orientation)
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
    /// A block that is not air and that nothing touches.
    const MIST: u32 = 2;

    /// A table with air, a stone that has four shapes (whole, the bottom half, the top half, the half on the +x side: no turn but none leaves all of them as
    /// they were) and a mist that has no shape.
    fn collisions() -> BlockCollisions {
        let air = BlockDefinition::air();
        let stone = BlockDefinition::cube([0.5; 3]).with_shapes(vec![
            Shape::full(),
            Shape::half(FaceDirection::NegativeY),
            Shape::half(FaceDirection::PositiveY),
            Shape::half(FaceDirection::PositiveX),
        ]);
        let mist = BlockDefinition::cube([0.5; 3]).with_shapes(vec![Shape::empty()]);
        BlockCollisions::new(
            BlockTable::from_definitions(&[Some(&air), Some(&stone), Some(&mist)]),
            None,
        )
    }

    /// The channels of a source: the block, the state and the orientation.
    fn channels() -> (ChannelIndex, ChannelIndex, ChannelIndex) {
        let layout = GridLayout::new(vec![
            ChannelDefinition::number("test:block".parse().unwrap(), AIR),
            ChannelDefinition::number("test:state".parse().unwrap(), 0),
            ChannelDefinition::number("test:orientation".parse().unwrap(), 0),
        ])
        .unwrap();
        let mut indices = layout.channel_indices();
        (
            indices.next().unwrap(),
            indices.next().unwrap(),
            indices.next().unwrap(),
        )
    }

    /// Answers from these values, and notes which channels were asked about.
    fn answering<'a>(
        values: &'a [(ChannelIndex, u32)],
        asked: &'a RefCell<Vec<ChannelIndex>>,
    ) -> impl FnMut(ChannelIndex) -> Option<u32> + 'a {
        move |channel| {
            asked.borrow_mut().push(channel);
            values
                .iter()
                .find(|(known, _)| *known == channel)
                .map(|(_, value)| *value)
        }
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
            orientation_channel: None,
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
            orientation_channel: None,
            collisions: &collisions,
        };
        assert!(other.solid_at([3.5, 4.5, 5.5]) && hull.solid_at([3.5, 4.5, 5.5]));
    }

    #[test]
    fn a_voxel_is_the_block_in_the_state_and_orientation_that_the_channels_hold_and_a_wall_where_one_cannot_be_read()
     {
        let collisions = collisions();
        let (block, state, orientation) = channels();
        let asked = RefCell::new(Vec::new());
        let collision = |values: &[(ChannelIndex, u32)], with_orientation: bool| {
            asked.borrow_mut().clear();
            collision_of_voxel(
                &collisions,
                block,
                state,
                with_orientation.then_some(orientation),
                answering(values, &asked),
            )
        };

        assert_eq!(
            collision(&[(block, STONE), (state, 0)], false),
            Collision::Full
        );
        assert_eq!(
            collision(&[(block, AIR), (state, 0)], false),
            Collision::Empty
        );
        assert_eq!(
            collision(&[(block, MIST), (state, 0)], false),
            Collision::Empty
        );
        // Without a channel for orientations none is asked about, and a block is as it is made.
        assert_eq!(*asked.borrow(), [block, state]);
        for shape in 0..4 {
            assert_eq!(
                collision(&[(block, STONE), (state, shape)], false),
                collisions.of(STONE, shape, 0),
                "{shape}"
            );
        }

        // A value that cannot be read is a wall, whichever it is (and air in the other channels is no reason to think otherwise).
        assert_eq!(collision(&[(state, 0)], false), Collision::Full);
        assert_eq!(collision(&[(block, AIR)], false), Collision::Full);
        assert_eq!(
            collision(&[(block, AIR), (state, 0)], true),
            Collision::Full
        );

        // The orientation is the one the channel holds: no turn but none leaves all the shapes of the stone as they were.
        for turn in 1..24 {
            let mut changed = 0;
            for shape in 0..4 {
                let values = [(block, STONE), (state, shape), (orientation, turn)];
                assert_eq!(
                    collision(&values, true),
                    collisions.of(STONE, shape, turn),
                    "shape {shape} turned {turn}"
                );
                changed += usize::from(collision(&values, true) != collisions.of(STONE, shape, 0));
            }
            assert!(changed > 0, "turn {turn} changes some shape");
            assert_eq!(*asked.borrow(), [block, state, orientation]);
        }
    }

    #[test]
    fn a_brick_is_known_to_be_empty_or_solid_when_all_of_it_is_one_block_that_says_so_and_unknown_otherwise()
     {
        let collisions = collisions();
        let (block, state, orientation) = channels();
        let asked = RefCell::new(Vec::new());
        let hint = |values: &[(ChannelIndex, u32)]| {
            asked.borrow_mut().clear();
            hint_of_brick(&collisions, AIR, block, state, answering(values, &asked))
        };

        // All air is empty, and nothing else is asked once that is known.
        assert_eq!(hint(&[(block, AIR)]), BrickHint::Empty);
        assert_eq!(*asked.borrow(), [block]);
        // All whole stone is solid; all of a block that is part of a cell is not known (however it is turned: the orientations are not asked about); all of a block
        // with no shape is empty.
        assert_eq!(hint(&[(block, STONE), (state, 0)]), BrickHint::Solid);
        assert_eq!(*asked.borrow(), [block, state]);
        for shape in 1..4 {
            assert_eq!(
                hint(&[(block, STONE), (state, shape), (orientation, 5)]),
                BrickHint::Unknown,
                "{shape}"
            );
        }
        assert_eq!(
            hint(&[(block, STONE), (state, 0), (orientation, 5)]),
            BrickHint::Solid
        );
        assert_eq!(hint(&[(block, MIST), (state, 0)]), BrickHint::Empty);
        // A brick whose voxels are not all the same block or not all in the same state is not known, whatever else is.
        assert_eq!(hint(&[(state, 0)]), BrickHint::Unknown);
        assert_eq!(hint(&[(block, STONE)]), BrickHint::Unknown);
        assert_eq!(hint(&[(block, MIST)]), BrickHint::Unknown);
    }
}
