//! The entity store: spawning with a kind's components, ids that are never reused, moving, and finding
//! what is near, checked against asking every entity.

use binary_codec::{ByteReader, DecodeError};
use entities::{ComponentBag, EntityError, EntityKindSpec, EntityKinds, EntityStore};
use namespaced_id::NamespacedId;
use proptest::prelude::*;
use protocol::{EntityId, Message};
use voxel_space::test_support::entity_pose;
use voxel_space::{BrickCoordinate, EntityPose, GridId, LevelIndex, Orientation, WorldPosition};

/// A component that is one number.
#[derive(Debug, PartialEq)]
struct Health(u32);

impl Message for Health {
    fn kind() -> NamespacedId {
        id("test:health")
    }
    fn encode(&self) -> Vec<u8> {
        self.0.to_le_bytes().to_vec()
    }
    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = ByteReader::new(payload);
        let value = reader.read_u32()?;
        reader.finish()?;
        Ok(Self(value))
    }
    fn samples() -> Vec<Self> {
        vec![Self(0), Self(u32::MAX)]
    }
}

/// A component the kind below does not have.
#[derive(Debug, PartialEq)]
struct Mood(u8);

impl Message for Mood {
    fn kind() -> NamespacedId {
        id("test:mood")
    }
    fn encode(&self) -> Vec<u8> {
        vec![self.0]
    }
    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        match payload {
            [value] => Ok(Self(*value)),
            _ => Err(DecodeError::Invalid("a mood is one byte")),
        }
    }
    fn samples() -> Vec<Self> {
        vec![Self(0)]
    }
}

fn id(text: &str) -> NamespacedId {
    text.parse().unwrap()
}

fn kinds() -> EntityKinds {
    let mut kinds = EntityKinds::new();
    kinds
        .register(EntityKindSpec::new("test:cow").component(Health(10), true))
        .unwrap();
    kinds
}

fn pose_at(level: u32, brick: [i64; 3], offset: [f32; 3]) -> EntityPose {
    EntityPose::new(
        GridId::root(LevelIndex::new(level)),
        WorldPosition::new(BrickCoordinate::new(brick[0], brick[1], brick[2]), offset),
        Orientation::IDENTITY,
    )
}

fn spawn(store: &mut EntityStore, pose: EntityPose) -> EntityId {
    store
        .spawn(&kinds(), &id("test:cow"), pose, ComponentBag::new())
        .unwrap()
}

#[test]
fn an_entity_starts_with_the_components_of_its_kind_and_the_ones_it_was_given() {
    let mut store = EntityStore::new();
    let plain = spawn(&mut store, pose_at(0, [0, 0, 0], [1.0, 2.0, 3.0]));
    assert_eq!(store.component::<Health>(plain), Ok(Some(Health(10))));
    let mut given = ComponentBag::new();
    given.set(&Health(3));
    let hurt = store
        .spawn(
            &kinds(),
            &id("test:cow"),
            pose_at(0, [0, 0, 0], [0.0; 3]),
            given,
        )
        .unwrap();
    assert_eq!(store.component::<Health>(hurt), Ok(Some(Health(3))));
    // A component the kind lacks reads as absent.
    assert_eq!(store.component::<Mood>(plain), Ok(None));
    assert_eq!(store.get(plain).unwrap().kind, id("test:cow"));
}

#[test]
fn what_the_kind_does_not_declare_is_refused_naming_both() {
    let mut store = EntityStore::new();
    let pose = pose_at(0, [0, 0, 0], [0.0; 3]);
    assert_eq!(
        store.spawn(&kinds(), &id("test:pig"), pose, ComponentBag::new()),
        Err(EntityError::UnknownKind(id("test:pig")))
    );
    let mut with_mood = ComponentBag::new();
    with_mood.set(&Mood(1));
    assert_eq!(
        store.spawn(&kinds(), &id("test:cow"), pose, with_mood),
        Err(EntityError::UndeclaredComponent {
            kind: id("test:cow"),
            component: id("test:mood")
        })
    );
    let cow = spawn(&mut store, pose);
    assert_eq!(
        store.set_component(&kinds(), cow, &Mood(2)),
        Err(EntityError::UndeclaredComponent {
            kind: id("test:cow"),
            component: id("test:mood")
        })
    );
    assert_eq!(store.len(), 1, "the refused spawns left nothing behind");
    store.set_component(&kinds(), cow, &Health(1)).unwrap();
    assert_eq!(store.component::<Health>(cow), Ok(Some(Health(1))));
    assert_eq!(
        store.set_component(&kinds(), EntityId::new(99), &Health(1)),
        Err(EntityError::UnknownEntity(EntityId::new(99)))
    );
}

#[test]
fn until_structures_exist_only_root_grids_hold_entities() {
    let mut store = EntityStore::new();
    let ship = GridId::new(GridId::FIRST_ALLOCATED);
    let mut pose = pose_at(0, [0, 0, 0], [0.0; 3]);
    pose.grid = ship;
    assert_eq!(
        store.spawn(&kinds(), &id("test:cow"), pose, ComponentBag::new()),
        Err(EntityError::NotARootGrid(ship))
    );
    let cow = spawn(&mut store, pose_at(0, [0, 0, 0], [0.0; 3]));
    assert_eq!(
        store.set_pose(cow, pose),
        Err(EntityError::NotARootGrid(ship))
    );
    assert_eq!(store.len(), 1, "nothing was added");
    assert_eq!(
        store.get(cow).unwrap().pose.grid,
        GridId::MAIN,
        "and nothing moved"
    );
}

#[test]
fn ids_count_up_from_one_and_a_despawned_id_is_never_given_again() {
    let mut store = EntityStore::new();
    assert_eq!(store.next_id(), 1);
    let pose = pose_at(0, [0, 0, 0], [0.0; 3]);
    let first = spawn(&mut store, pose);
    let second = spawn(&mut store, pose);
    assert_eq!((first.get(), second.get()), (1, 2));
    assert_eq!(store.despawn(second).map(|entity| entity.id), Some(second));
    assert!(store.despawn(second).is_none());
    let third = spawn(&mut store, pose);
    assert_eq!(third.get(), 3);
    assert_eq!(store.next_id(), 4);
    let order: Vec<u64> = store.iter().map(|entity| entity.id.get()).collect();
    assert_eq!(order, [1, 3]);
}

#[test]
fn near_finds_what_is_in_reach_in_that_grid_in_order_of_id() {
    let mut store = EntityStore::new();
    let here = spawn(&mut store, pose_at(0, [0, 0, 0], [10.0, 10.0, 10.0]));
    let close = spawn(&mut store, pose_at(0, [0, 0, 0], [12.0, 10.0, 10.0]));
    // Across a brick boundary, 3 voxels away.
    let across = spawn(&mut store, pose_at(0, [1, 0, 0], [1.0, 10.0, 10.0]));
    let _far = spawn(&mut store, pose_at(0, [0, 0, 0], [30.0, 30.0, 30.0]));
    let _elsewhere = spawn(&mut store, pose_at(1, [0, 0, 0], [10.0, 10.0, 10.0]));
    let centre = WorldPosition::new(BrickCoordinate::new(0, 0, 0), [30.0, 10.0, 10.0]);
    assert_eq!(store.near(GridId::MAIN, centre, 4.0), [across]);
    let centre = WorldPosition::new(BrickCoordinate::new(0, 0, 0), [10.0, 10.0, 10.0]);
    assert_eq!(store.near(GridId::MAIN, centre, 0.0), [here]);
    assert_eq!(store.near(GridId::MAIN, centre, 2.0), [here, close]);
    assert_eq!(store.near(GridId::MAIN, centre, 1.99), [here]);
    assert!(store.near(GridId::MAIN, centre, -1.0).is_empty());
    assert!(store.near(GridId::MAIN, centre, f32::NAN).is_empty());
}

#[test]
fn a_moved_entity_is_found_where_it_is_and_not_where_it_was() {
    let mut store = EntityStore::new();
    let cow = spawn(&mut store, pose_at(0, [0, 0, 0], [5.0, 5.0, 5.0]));
    let old = WorldPosition::new(BrickCoordinate::new(0, 0, 0), [5.0, 5.0, 5.0]);
    let new = WorldPosition::new(BrickCoordinate::new(4, 0, 0), [5.0, 5.0, 5.0]);
    assert_eq!(store.near(GridId::MAIN, old, 1.0), [cow]);
    store
        .set_pose(cow, pose_at(0, [4, 0, 0], [5.0, 5.0, 5.0]))
        .unwrap();
    assert!(store.near(GridId::MAIN, old, 1.0).is_empty());
    assert_eq!(store.near(GridId::MAIN, new, 1.0), [cow]);
    // Moving within a brick changes where, not which brick.
    store
        .set_pose(cow, pose_at(0, [4, 0, 0], [6.0, 5.0, 5.0]))
        .unwrap();
    assert_eq!(store.near(GridId::MAIN, new, 1.5), [cow]);
    // Into another level's grid.
    store
        .set_pose(cow, pose_at(2, [4, 0, 0], [6.0, 5.0, 5.0]))
        .unwrap();
    assert!(store.near(GridId::MAIN, new, 5.0).is_empty());
    assert_eq!(
        store.near(GridId::root(LevelIndex::new(2)), new, 5.0),
        [cow]
    );
    store.despawn(cow);
    assert!(
        store
            .near(GridId::root(LevelIndex::new(2)), new, 5.0)
            .is_empty()
    );
}

#[test]
fn a_despawned_entity_is_gone_from_the_index_with_others_still_in_its_brick() {
    let mut store = EntityStore::new();
    let staying = spawn(&mut store, pose_at(0, [0, 0, 0], [5.0, 5.0, 5.0]));
    let leaving = spawn(&mut store, pose_at(0, [0, 0, 0], [6.0, 5.0, 5.0]));
    let centre = WorldPosition::new(BrickCoordinate::new(0, 0, 0), [5.0, 5.0, 5.0]);
    assert_eq!(store.near(GridId::MAIN, centre, 3.0), [staying, leaving]);
    store.despawn(leaving);
    assert_eq!(store.near(GridId::MAIN, centre, 3.0), [staying]);
}

#[test]
fn a_radius_that_reaches_more_bricks_than_there_are_entities_gives_the_same_answer() {
    let mut store = EntityStore::new();
    let a = spawn(&mut store, pose_at(0, [0, 0, 0], [1.0, 1.0, 1.0]));
    let b = spawn(&mut store, pose_at(0, [3, 0, 0], [1.0, 1.0, 1.0]));
    let _c = spawn(&mut store, pose_at(0, [30, 0, 0], [1.0, 1.0, 1.0]));
    let centre = WorldPosition::new(BrickCoordinate::new(0, 0, 0), [1.0, 1.0, 1.0]);
    assert_eq!(store.near(GridId::MAIN, centre, 200.0), [a, b]);
}

proptest! {
    #[test]
    fn near_is_exactly_the_entities_asked_one_by_one(
        placed in proptest::collection::vec(
            (0u32..2, -2i64..3, -2i64..3, -2i64..3, 0.0f32..32.0, 0.0f32..32.0, 0.0f32..32.0),
            0..40,
        ),
        centre in (-2i64..3, -2i64..3, -2i64..3, 0.0f32..32.0, 0.0f32..32.0, 0.0f32..32.0),
        radius in 0.0f32..90.0,
        grid in 0u32..2,
    ) {
        let mut store = EntityStore::new();
        for (level, bx, by, bz, x, y, z) in placed {
            spawn(&mut store, pose_at(level, [bx, by, bz], [x, y, z]));
        }
        let centre = WorldPosition::new(
            BrickCoordinate::new(centre.0, centre.1, centre.2),
            [centre.3, centre.4, centre.5],
        );
        let grid = GridId::root(LevelIndex::new(grid));
        let expected: Vec<EntityId> = store
            .iter()
            .filter(|entity| {
                let to = entity.pose.position.relative_to(centre);
                entity.pose.grid == grid && to[0] * to[0] + to[1] * to[1] + to[2] * to[2] <= radius * radius
            })
            .map(|entity| entity.id)
            .collect();
        prop_assert_eq!(store.near(grid, centre, radius), expected);
    }

    #[test]
    fn whatever_is_done_to_the_store_the_index_agrees_with_the_entities(
        steps in proptest::collection::vec((0u8..3, entity_pose(), 0usize..8), 0..40),
    ) {
        let mut store = EntityStore::new();
        let mut live: Vec<EntityId> = Vec::new();
        for (step, pose, pick) in steps {
            match (step, live.is_empty()) {
                (0, _) | (_, true) => live.push(spawn(&mut store, pose)),
                (1, false) => {
                    let id = live[pick % live.len()];
                    store.set_pose(id, pose).unwrap();
                }
                (_, false) => {
                    let id = live.remove(pick % live.len());
                    prop_assert!(store.despawn(id).is_some());
                }
            }
        }
        // Everything alive is found at its own place, and nothing that is gone is found anywhere.
        let mut found = 0;
        for entity in store.iter() {
            prop_assert!(store.near(entity.pose.grid, entity.pose.position, 0.0).contains(&entity.id));
            found += 1;
        }
        prop_assert_eq!(found, live.len());
    }
}
