//! The record of what a split and a detach did (`StructureChange`); the record of a merge and a dock is tested with them, in `merge.rs`.

use std::collections::BTreeSet;

use voxel_space::LatticeRotation;

use super::*;
use crate::structure_change::{StructureChange, VoxelsCarried};

/// The voxels of a row of blocks along x, from `low` up to but not including `high`.
fn row(low: i128, high: i128) -> BTreeSet<VoxelCoordinate> {
    (low..high).map(|x| VoxelCoordinate::new(x, 0, 0)).collect()
}

fn carried(
    from: GridId,
    to: GridId,
    transform: LatticeTransform,
    voxels: BTreeSet<VoxelCoordinate>,
) -> VoxelsCarried {
    VoxelsCarried {
        from,
        to,
        transform,
        voxels,
    }
}

#[test]
fn a_split_leaves_a_record_naming_the_ship_the_parts_and_the_voxels_each_part_took() {
    let (mut world, ship, _, _) = row_with_walkers(&[]);
    let parts = world.split_grid(ship, halves()).unwrap();
    let identity = LatticeTransform::IDENTITY;
    assert_eq!(
        world.take_structure_changes(),
        vec![StructureChange::Split {
            from: ship,
            parts: vec![
                carried(ship, parts[0], identity, row(0, 5)),
                carried(ship, parts[1], identity, row(5, 10)),
            ],
        }]
    );
}

#[test]
fn what_the_record_says_a_part_took_is_what_the_part_holds() {
    // The record and the world are two accounts of one event: every voxel the record gives to a part is a block of that part, and the part has no other.
    let (mut world, ship, block, _) = row_with_walkers(&[]);
    let parts = world.split_grid(ship, halves()).unwrap();
    let changes = world.take_structure_changes();
    let [StructureChange::Split { parts: records, .. }] = changes.as_slice() else {
        panic!("one split: {changes:?}");
    };
    for (record, part) in records.iter().zip(&parts) {
        assert_eq!(record.to, *part);
        let held: BTreeSet<VoxelCoordinate> = (-3..13)
            .map(|x| VoxelCoordinate::new(x, 0, 0))
            .filter(|voxel| world.grid(*part).unwrap().get(block, *voxel).unwrap() != 0)
            .collect();
        let landed: BTreeSet<VoxelCoordinate> = record
            .voxels
            .iter()
            .map(|voxel| record.transform.apply(*voxel).unwrap())
            .collect();
        assert_eq!(held, landed);
    }
}

#[test]
fn the_records_come_out_once_in_the_order_the_operations_ran() {
    let (mut world, ship, _, _) = row_with_walkers(&[]);
    assert!(
        world.take_structure_changes().is_empty(),
        "nothing happened yet"
    );
    let parts = world.split_grid(ship, halves()).unwrap();
    let taken: Vec<VoxelCoordinate> = row(0, 2).into_iter().collect();
    let detached = world.detach_blocks(parts[0], &taken).unwrap();
    let changes = world.take_structure_changes();
    assert_eq!(changes.len(), 2, "{changes:?}");
    assert!(matches!(&changes[0], StructureChange::Split { from, .. } if *from == ship));
    assert!(
        matches!(&changes[1], StructureChange::Detach { carried } if carried.from == parts[0] && carried.to == detached)
    );
    assert!(
        world.take_structure_changes().is_empty(),
        "and they were taken"
    );
}

#[test]
fn a_split_that_is_refused_leaves_no_record() {
    let (mut world, ship, _, _) = row_with_walkers(&[]);
    let overlapping = {
        let mut parts = halves();
        parts[1].voxels.insert(VoxelCoordinate::new(0, 0, 0));
        parts
    };
    assert!(world.split_grid(ship, overlapping).is_err());
    assert!(world.split_grid(ship, halves()[..1].to_vec()).is_err());
    assert!(world.take_structure_changes().is_empty());
}

#[test]
fn detaching_blocks_leaves_a_record_with_the_voxels_taken_and_the_shift_to_the_new_ships_origin() {
    let (mut world, ship, block, _) = row_with_walkers(&[]);
    let taken: Vec<VoxelCoordinate> = row(6, 10).into_iter().collect();
    let part = world.detach_blocks(ship, &taken).unwrap();
    let shift = LatticeTransform {
        rotation: LatticeRotation::IDENTITY,
        offset: [-6, 0, 0],
    };
    assert_eq!(
        world.take_structure_changes(),
        vec![StructureChange::Detach {
            carried: carried(ship, part, shift, row(6, 10)),
        }]
    );
    // (The record and the new ship agree: voxel 6 of the ship is voxel 0 of the part, and holds the block.)
    assert_eq!(
        world
            .grid(part)
            .unwrap()
            .get(block, shift.apply(VoxelCoordinate::new(6, 0, 0)).unwrap())
            .unwrap(),
        7
    );
}

#[test]
fn detaching_out_of_the_terrain_of_a_level_names_the_root_grid_of_the_level() {
    let (mut world, _) = world_with_a_walker();
    let level = LevelIndex::MAIN;
    let taken = [
        VoxelCoordinate::new(10, 30, 10),
        VoxelCoordinate::new(11, 30, 10),
    ];
    let part = world.detach_blocks(GridId::root(level), &taken).unwrap();
    let changes = world.take_structure_changes();
    let [StructureChange::Detach { carried }] = changes.as_slice() else {
        panic!("one detach: {changes:?}");
    };
    assert_eq!(carried.from, GridId::root(level));
    assert_eq!(carried.to, part);
    assert_eq!(carried.voxels, taken.into_iter().collect());
    assert_eq!(carried.transform.offset, [-10, -30, -10]);
}

#[test]
fn a_detach_that_is_refused_leaves_no_record() {
    let (mut world, ship, _, _) = row_with_walkers(&[]);
    assert!(world.detach_blocks(ship, &[]).is_err());
    let nowhere = LevelIndex::new(99);
    assert!(
        world
            .detach_blocks(GridId::root(nowhere), &[VoxelCoordinate::ORIGIN])
            .is_err()
    );
    assert!(world.take_structure_changes().is_empty());
}
