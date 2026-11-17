//! What the blocks of a world mean to something that moves through or rests on them: the collision of each block, and the terrain of a level
//! and the voxels of a ship as sources of it for [`physics`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use block_models::{BlockTable, PropertyHandle, SharedBlockTable};
use physics::{BrickHint, Collision, CollisionSource};
use server::{GridView, ServerWorld};
use voxel_grid::{ChannelIndex, Grid};
use voxel_space::{BrickCoordinate, GridId, LevelIndex, VoxelCoordinate};

/// What each block's shape means to something moving through it, worked out once: the whole cube, the boxes in it, or nothing (air, and
/// the blocks a game says can be passed through, such as flames). The sources below share one, so a body that meets several ships at
/// once needs only the one table.
///
/// It follows the table it was made for: when that is replaced (a [`SharedBlockTable`]), what was worked out is forgotten and worked out again from the new one, at the
/// next question. (The colliders that were made of the old answers are the business of whoever made them.)
#[derive(Debug)]
pub struct BlockCollisions {
    shared: SharedBlockTable,
    passable: Option<PropertyHandle<bool>>,
    worked: RefCell<Worked>,
}

/// What was worked out, and from which table.
#[derive(Debug)]
struct Worked {
    table: Arc<BlockTable>,
    version: u64,
    cache: HashMap<u32, Collision>,
}

impl BlockCollisions {
    /// For a table that is not replaced. `passable` is the property of the game that says a block can be passed through, if it has one.
    pub fn new(table: impl Into<Arc<BlockTable>>, passable: Option<PropertyHandle<bool>>) -> Self {
        Self::following(&SharedBlockTable::new(table), passable)
    }

    /// For the table of a server, which is replaced while it runs. A replacement declares the properties the table did, so `passable` stays right.
    pub fn following(shared: &SharedBlockTable, passable: Option<PropertyHandle<bool>>) -> Self {
        let (table, version) = shared.snapshot();
        Self {
            shared: shared.clone(),
            passable,
            worked: RefCell::new(Worked {
                table,
                version,
                cache: HashMap::new(),
            }),
        }
    }

    /// The table that the answers are from now.
    pub fn table(&self) -> Arc<BlockTable> {
        let mut worked = self.worked.borrow_mut();
        self.keep_up(&mut worked);
        Arc::clone(&worked.table)
    }

    /// What the block in this state, standing in this orientation, is to a body.
    pub fn of(&self, block: u32, state: u32, orientation: u32) -> Collision {
        let mut worked = self.worked.borrow_mut();
        self.keep_up(&mut worked);
        let Worked { table, cache, .. } = &mut *worked;
        let model = table.model_of(block, state, orientation);
        let passable = self.passable;
        cache
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

    /// Whether the block is nothing to a body in every state it has (air, a block with no shape, a block that bodies pass through): a voxel of it can be left out of a look at
    /// what a brick holds. A block that the table does not know is a whole cube, and so is not. (Whether a shape is empty does not depend on how the block is turned.)
    pub fn is_always_empty(&self, block: u32) -> bool {
        let states = self.table().state_count(block);
        (0..states).all(|state| self.of(block, state, 0) == Collision::Empty)
    }

    /// Forgets what was worked out if the table was replaced since. (One atomic read when it was not: this is asked of every voxel.)
    fn keep_up(&self, worked: &mut Worked) {
        if self.shared.version() != worked.version {
            let (table, version) = self.shared.snapshot();
            *worked = Worked {
                table,
                version,
                cache: HashMap::new(),
            };
        }
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

    fn voxels_that_may_collide(&mut self, brick: BrickCoordinate) -> Option<Vec<VoxelCoordinate>> {
        match self {
            Self::Stored(hull) => hull.voxels_that_may_collide(brick),
            Self::Generated(hull) => hull.voxels_that_may_collide(brick),
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

    /// The voxels of the brick whose block is something to a body in some state, found from the palette of the block channel (a ship's brick is a few blocks in a space of
    /// 32,768 places, and asking a grid for the block, the state and the orientation at each took 2 milliseconds).
    fn voxels_that_may_collide(&mut self, brick: BrickCoordinate) -> Option<Vec<VoxelCoordinate>> {
        let contents = self.voxels.brick(brick)?;
        let collisions = self.collisions;
        Some(
            contents
                .channel(self.block_channel)
                .indices_where(|block| !collisions.is_always_empty(block))
                .into_iter()
                .map(|local| brick.voxel_at(local))
                .collect(),
        )
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

    /// A table like that of [`collisions`] in which the stone is the one shape it has (whole), and the mist, which is nothing in that table, is a whole block here.
    fn table_with_a_solid_mist() -> BlockTable {
        let air = BlockDefinition::air();
        let stone = BlockDefinition::cube([0.5; 3])
            .with_shapes(vec![Shape::half(FaceDirection::NegativeY)]);
        let mist = BlockDefinition::cube([0.5; 3]);
        BlockTable::from_definitions(&[Some(&air), Some(&stone), Some(&mist)])
    }

    #[test]
    fn a_block_that_is_nothing_in_every_state_is_always_empty_and_one_that_is_something_in_any_state_is_not()
     {
        let collisions = collisions();
        assert!(collisions.is_always_empty(AIR));
        assert!(collisions.is_always_empty(MIST));
        assert!(!collisions.is_always_empty(STONE));
        // Not in one state of two is not always.
        let air = BlockDefinition::air();
        let flickering =
            BlockDefinition::cube([0.5; 3]).with_shapes(vec![Shape::empty(), Shape::full()]);
        let ghost =
            BlockDefinition::cube([0.5; 3]).with_shapes(vec![Shape::empty(), Shape::empty()]);
        let shared = SharedBlockTable::new(BlockTable::from_definitions(&[
            Some(&air),
            Some(&flickering),
            Some(&ghost),
        ]));
        let following = BlockCollisions::following(&shared, None);
        assert!(!following.is_always_empty(1));
        assert!(following.is_always_empty(2));
        // A block the table does not know is a whole cube.
        assert!(!following.is_always_empty(77));
        // And it follows the table when that is replaced: what was worked out is forgotten.
        shared
            .replace(BlockTable::from_definitions(&[
                Some(&air),
                Some(&ghost),
                Some(&flickering),
            ]))
            .unwrap();
        assert!(following.is_always_empty(1));
        assert!(!following.is_always_empty(2));
    }

    /// A brick with some blocks in it at places that the loops of x, y and z do not find in order: whole and part stones, mist, and air.
    fn scattered() -> (Grid, ChannelIndex, ChannelIndex, ChannelIndex) {
        let (block, state, orientation) = channels();
        let layout = Arc::new(
            GridLayout::new(vec![
                ChannelDefinition::number("test:block".parse().unwrap(), AIR),
                ChannelDefinition::number("test:state".parse().unwrap(), 0),
                ChannelDefinition::number("test:orientation".parse().unwrap(), 0),
            ])
            .unwrap(),
        );
        let mut voxels = Grid::new(layout);
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        for _ in 0..600 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let at = VoxelCoordinate::new(
                (seed % 32) as i128,
                ((seed >> 8) % 32) as i128,
                ((seed >> 16) % 32) as i128,
            );
            voxels.set(block, at, [STONE, MIST, AIR][((seed >> 24) % 3) as usize]);
            voxels.set(state, at, ((seed >> 32) % 4) as u32);
            voxels.set(orientation, at, ((seed >> 40) % 24) as u32);
        }
        (voxels, block, state, orientation)
    }

    #[test]
    fn the_voxels_a_hull_lists_are_those_that_can_collide_in_the_order_the_bricks_are_made_in_and_all_of_them()
     {
        let collisions = collisions();
        let (voxels, block, state, orientation) = scattered();
        let mut hull = ShipHull {
            voxels: &voxels,
            block_channel: block,
            state_channel: state,
            orientation_channel: Some(orientation),
            collisions: &collisions,
        };
        let brick = BrickCoordinate::ORIGIN;
        assert_eq!(hull.brick_hint(brick), BrickHint::Unknown);
        let listed = hull
            .voxels_that_may_collide(brick)
            .expect("a hull says which voxels may collide");
        let order = |voxel: &VoxelCoordinate| (voxel.z(), voxel.y(), voxel.x());
        assert!(
            listed
                .windows(2)
                .all(|pair| order(&pair[0]) < order(&pair[1])),
            "in order of z, then y, then x, and none twice"
        );
        // Every voxel of the brick that is something to a body is listed: none that is not is the only way to be left out.
        let mut something = 0;
        for z in 0..32 {
            for y in 0..32 {
                for x in 0..32 {
                    let voxel = VoxelCoordinate::new(x, y, z);
                    if hull.collision_at(voxel) != Collision::Empty {
                        something += 1;
                        assert!(
                            listed.contains(&voxel),
                            "{voxel:?} collides and is not listed"
                        );
                    }
                }
            }
        }
        assert!(something > 50, "the brick has things in it: {something}");
        // Mist is not listed (nothing of it is ever anything to a body), so what is listed is the stone and no more.
        assert_eq!(listed.len(), something);
    }

    /// A hull that does not say which of its voxels may collide, so that every one is asked.
    struct Plain<'a, 'b>(&'a mut ShipHull<'b>);

    impl CollisionSource for Plain<'_, '_> {
        fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
            self.0.collision_at(voxel)
        }

        fn brick_hint(&mut self, brick: BrickCoordinate) -> BrickHint {
            self.0.brick_hint(brick)
        }
    }

    #[test]
    fn a_body_made_from_the_voxels_a_hull_lists_is_the_body_made_from_all_of_them_to_the_bit() {
        use physics::{BodyId, BodyKind, BodySettings, BodyState, BodyWorld};
        use voxel_space::{Orientation, WorldPosition};

        let collisions = collisions();
        let (voxels, block, state, orientation) = scattered();
        let mut hull = ShipHull {
            voxels: &voxels,
            block_channel: block,
            state_channel: state,
            orientation_channel: Some(orientation),
            collisions: &collisions,
        };
        let mut body = |listed: bool| {
            let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, BodySettings::default());
            let at = BodyState {
                position: WorldPosition::from_voxels([0.0, 40.0, 0.0]),
                orientation: Orientation::IDENTITY,
                linear_velocity: [0.0; 3],
                angular_velocity: [0.0; 3],
            };
            world.add_body(BodyId(1), at, BodyKind::Dynamic).unwrap();
            if listed {
                world
                    .set_brick(BodyId(1), BrickCoordinate::ORIGIN, &mut hull)
                    .unwrap();
            } else {
                world
                    .set_brick(BodyId(1), BrickCoordinate::ORIGIN, &mut Plain(&mut hull))
                    .unwrap();
            }
            world
        };
        let (mut listed, mut every) = (body(true), body(false));
        assert_eq!(listed.state_hash(), every.state_hash());
        for _ in 0..30 {
            listed.step();
            every.step();
        }
        assert_eq!(listed.state_hash(), every.state_hash());
    }

    #[test]
    fn collisions_that_follow_a_table_answer_by_the_one_that_replaced_it_and_a_copy_does_not() {
        let air = BlockDefinition::air();
        let stone = BlockDefinition::cube([0.5; 3]);
        let mist = BlockDefinition::cube([0.5; 3]).with_shapes(vec![Shape::empty()]);
        let shared = SharedBlockTable::new(BlockTable::from_definitions(&[
            Some(&air),
            Some(&stone),
            Some(&mist),
        ]));
        let following = BlockCollisions::following(&shared, None);
        let fixed = BlockCollisions::new(shared.current(), None);
        // (The answers are worked out and kept: the first asking is from the table there is.)
        assert_eq!(following.of(MIST, 0, 0), Collision::Empty);
        assert_eq!(following.of(STONE, 0, 0), Collision::Full);

        shared.replace(table_with_a_solid_mist()).unwrap();
        assert_eq!(
            following.of(MIST, 0, 0),
            Collision::Full,
            "what was kept of the mist is forgotten"
        );
        assert!(matches!(following.of(STONE, 0, 0), Collision::Partial(_)));
        assert_eq!(following.table().color(MIST), shared.current().color(MIST));
        assert!(
            Arc::ptr_eq(&following.table(), &shared.current()),
            "it holds the table that is current"
        );
        // A table that was given as it was is not the one that was replaced.
        assert_eq!(fixed.of(MIST, 0, 0), Collision::Empty);
        assert_eq!(fixed.of(STONE, 0, 0), Collision::Full);
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
