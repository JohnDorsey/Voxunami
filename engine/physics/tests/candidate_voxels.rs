//! A source that says which voxels of a brick may collide (`CollisionSource::voxels_that_may_collide`) is asked about those and no others, and the colliders it makes are the
//! same, to the bit of the simulation, as those of a source that is asked about every voxel.
//!
//! Found by timing the reload of a server with many ships (`docs/hot-reload-plan.md`): a ship of 27 voxels made its colliders by asking a grid for the block, the state and the
//! orientation of each of the 32,768 places of its brick, 2.2 milliseconds each. What these tests hold is what the shortcut must not change.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use block_models::ShapeBox;
use physics::{
    BodyId, BodyKind, BodyMass, BodySettings, BodyState, BodyWorld, BrickHint, Collision,
    CollisionSource, MassMoments,
};
use voxel_space::{BrickCoordinate, Orientation, VoxelCoordinate, WorldPosition};

const FLOOR: BodyId = BodyId(1);
const FALLER: BodyId = BodyId(2);

/// The voxels of one brick, with what each is; where nothing is said, nothing is there. Counts the voxels it is asked about.
struct Voxels {
    voxels: BTreeMap<VoxelCoordinate, Collision>,
    /// Whether it says which voxels may collide.
    says_which: bool,
    /// What it says is a superset of that when it is: these voxels are listed too, though they are empty.
    also_lists: Vec<VoxelCoordinate>,
    asked: usize,
}

impl Voxels {
    fn new(
        voxels: impl IntoIterator<Item = (VoxelCoordinate, Collision)>,
        says_which: bool,
    ) -> Self {
        Self {
            voxels: voxels.into_iter().collect(),
            says_which,
            also_lists: Vec::new(),
            asked: 0,
        }
    }
}

impl CollisionSource for Voxels {
    fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
        self.asked += 1;
        self.voxels.get(&voxel).cloned().unwrap_or(Collision::Empty)
    }

    fn brick_hint(&mut self, _: BrickCoordinate) -> BrickHint {
        BrickHint::Unknown
    }

    fn voxels_that_may_collide(&mut self, _: BrickCoordinate) -> Option<Vec<VoxelCoordinate>> {
        self.says_which.then(|| {
            let mut listed: Vec<VoxelCoordinate> = self.voxels.keys().copied().collect();
            listed.extend(self.also_lists.iter().copied());
            // (In the order the voxels are visited in: z, then y, then x.)
            listed.sort_by_key(|voxel| (voxel.z(), voxel.y(), voxel.x()));
            listed.dedup();
            listed
        })
    }
}

fn slab() -> Collision {
    Collision::Partial(Arc::from(vec![ShapeBox {
        min: [0, 0, 0],
        max: [8, 4, 8],
    }]))
}

fn post() -> Collision {
    Collision::Partial(Arc::from(vec![ShapeBox {
        min: [3, 0, 3],
        max: [5, 8, 5],
    }]))
}

/// A floor with whole cubes, slabs and posts in it, in no order that the loops would find them in.
fn a_floor() -> Vec<(VoxelCoordinate, Collision)> {
    let mut floor = HashMap::new();
    for x in 0..9 {
        for z in 0..9 {
            let collision = match (x * 7 + z * 3) % 4 {
                0 | 1 => Collision::Full,
                2 => slab(),
                _ => post(),
            };
            floor.insert(VoxelCoordinate::new(x, 0, z), collision);
        }
    }
    floor.insert(VoxelCoordinate::new(31, 31, 31), Collision::Full);
    floor.insert(VoxelCoordinate::new(0, 31, 0), slab());
    floor.into_iter().collect()
}

fn block_mass(size: [i128; 3]) -> BodyMass {
    let voxels = (0..size[0]).flat_map(|x| {
        (0..size[1])
            .flat_map(move |y| (0..size[2]).map(move |z| MassMoments::of_voxel([x, y, z], 1.0)))
    });
    MassMoments::sum(voxels)
        .properties()
        .expect("a block has a mass")
}

fn at_rest(place: [f64; 3]) -> BodyState {
    BodyState {
        position: WorldPosition::from_voxels(place),
        orientation: Orientation::IDENTITY,
        linear_velocity: [0.0; 3],
        angular_velocity: [0.0; 3],
    }
}

/// A fixed floor made from `floor`, and a cube let go over it.
fn a_world_with(floor: &mut Voxels) -> BodyWorld {
    let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, BodySettings::default());
    world
        .add_body(FLOOR, at_rest([0.0; 3]), BodyKind::Fixed)
        .unwrap();
    world
        .set_brick(FLOOR, BrickCoordinate::ORIGIN, floor)
        .unwrap();
    world.set_mass(FLOOR, block_mass([9, 1, 9])).unwrap();
    world
        .add_body(FALLER, at_rest([3.5, 3.0, 3.5]), BodyKind::Dynamic)
        .unwrap();
    let mut cube = Voxels::new(
        (0..2).flat_map(|x| {
            (0..2).flat_map(move |y| {
                (0..2).map(move |z| (VoxelCoordinate::new(x, y, z), Collision::Full))
            })
        }),
        false,
    );
    world
        .set_brick(FALLER, BrickCoordinate::ORIGIN, &mut cube)
        .unwrap();
    world.set_mass(FALLER, block_mass([2, 2, 2])).unwrap();
    world
}

#[test]
fn the_colliders_made_from_the_voxels_a_source_lists_are_those_made_from_all_of_them_to_the_bit() {
    let mut asked_all = Voxels::new(a_floor(), false);
    let mut asked_some = Voxels::new(a_floor(), true);
    let (mut all, mut some) = (a_world_with(&mut asked_all), a_world_with(&mut asked_some));
    assert_eq!(
        asked_all.asked, 32_768,
        "without a list every voxel is asked"
    );
    assert_eq!(
        asked_some.asked,
        a_floor().len(),
        "with a list, those and no others"
    );
    assert_eq!(some.state_hash(), all.state_hash(), "as made");
    for _ in 0..80 {
        all.step();
        some.step();
    }
    assert_eq!(
        some.state_hash(),
        all.state_hash(),
        "after 80 ticks, with a cube that fell on them"
    );
}

#[test]
fn a_list_that_has_voxels_that_turn_out_empty_changes_nothing() {
    let mut exact = Voxels::new(a_floor(), true);
    let mut padded = Voxels::new(a_floor(), true);
    padded.also_lists = vec![
        VoxelCoordinate::new(20, 20, 20),
        VoxelCoordinate::new(0, 0, 31),
        VoxelCoordinate::new(15, 1, 0),
    ];
    let (mut one, mut other) = (a_world_with(&mut exact), a_world_with(&mut padded));
    assert_eq!(padded.asked, a_floor().len() + 3);
    for _ in 0..40 {
        one.step();
        other.step();
    }
    assert_eq!(one.state_hash(), other.state_hash());
}

#[test]
fn a_brick_made_again_from_a_list_loses_what_it_had_and_the_list_no_longer_has() {
    let mut world = a_world_with(&mut Voxels::new(a_floor(), true));
    for _ in 0..40 {
        world.step();
    }
    // The floor is now only its first row of cubes: everything else that was there is taken away, slabs and posts and cubes alike.
    let mut row = Voxels::new(
        (0..9).map(|x| (VoxelCoordinate::new(x, 0, 0), Collision::Full)),
        true,
    );
    world
        .set_brick(FLOOR, BrickCoordinate::ORIGIN, &mut row)
        .unwrap();
    let mut asked_all = Voxels::new(
        (0..9).map(|x| (VoxelCoordinate::new(x, 0, 0), Collision::Full)),
        false,
    );
    let mut control = a_world_with(&mut Voxels::new(a_floor(), false));
    for _ in 0..40 {
        control.step();
    }
    control
        .set_brick(FLOOR, BrickCoordinate::ORIGIN, &mut asked_all)
        .unwrap();
    for _ in 0..80 {
        world.step();
        control.step();
    }
    assert_eq!(world.state_hash(), control.state_hash());
}

#[test]
fn a_brick_with_a_hint_is_not_listed_and_a_list_is_not_asked_for_where_a_hint_says_all() {
    // A source that says its brick is empty, or whole cubes, is believed as before, and is not asked for a list.
    struct Whole {
        listed: usize,
    }
    impl CollisionSource for Whole {
        fn collision_at(&mut self, _: VoxelCoordinate) -> Collision {
            panic!("a brick that is all cubes is not looked through");
        }
        fn brick_hint(&mut self, _: BrickCoordinate) -> BrickHint {
            BrickHint::Solid
        }
        fn voxels_that_may_collide(&mut self, _: BrickCoordinate) -> Option<Vec<VoxelCoordinate>> {
            self.listed += 1;
            Some(Vec::new())
        }
    }
    let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, BodySettings::default());
    world
        .add_body(FLOOR, at_rest([0.0; 3]), BodyKind::Fixed)
        .unwrap();
    let mut whole = Whole { listed: 0 };
    world
        .set_brick(FLOOR, BrickCoordinate::ORIGIN, &mut whole)
        .unwrap();
    assert_eq!(whole.listed, 0);
}
