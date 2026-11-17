//! The far nodes of ships that are in files: they are read without bringing the ship in, they belong to one state of the ship, and a file that is damaged,
//! missing or of another state costs the ship's far view and nothing else.

use std::collections::BTreeSet;

use coarse_cells::{CELLS_PER_NODE, CoarseCell, CoarseNode};
use namespaced_id::NamespacedId;
use region_octree::BrickCube;
use voxel_space::BrickCoordinate;
use world_storage::far_nodes::{
    FarFileName, FarFiles, FarNodes, FarNodesError, FarNodesRead, MemoryFarFiles, read_far_nodes,
    remove_far_files_without_ship_file,
};
use world_storage::test_support::{ALL_PLUGINS, Game, sample_world};
use world_storage::{MemoryShipFiles, WorldData};

fn name_of(number: u32) -> Option<NamespacedId> {
    ["base_blocks:air", "base_blocks:stone"]
        .get(number as usize)
        .map(|name| name.parse().unwrap())
}

fn number_of(id: &NamespacedId) -> Option<u32> {
    (0..2).find(|&number| name_of(number).as_ref() == Some(id))
}

fn nodes() -> FarNodes {
    let mut nodes = FarNodes::new();
    let cube = BrickCube::new(BrickCoordinate::new(64, 0, 0), 6).unwrap();
    let cells = (0..CELLS_PER_NODE)
        .map(|index| CoarseCell {
            block: u32::from(index % 7 == 0),
            fill: 200,
            light: 255,
            exposure: index as u32 % 300,
        })
        .collect();
    nodes.insert(cube, CoarseNode::from_cells(cells));
    nodes
}

fn world_with_a_ship_in_a_file() -> (Game, WorldData, MemoryShipFiles, world_storage::ParkedGrid) {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = sample_world(&game);
    let made = game.sample_ship(&world, 0);
    let first = world.grids_mut().create(made);
    let format = game.ship_format(&world);
    let mut files = MemoryShipFiles::new();
    world.park_ship(first, false, &mut files, &format).unwrap();
    let parked = world.grids().get_parked(first).unwrap().clone();
    (game, world, files, parked)
}

#[test]
fn the_nodes_of_a_ship_in_a_file_are_read_without_bringing_the_ship_in() {
    let (_, world, _, parked) = world_with_a_ship_in_a_file();
    let mut far = MemoryFarFiles::new();
    let name = FarFileName::of(&parked.file);
    far.write(&name, &nodes().encode(parked.checksum, name_of).unwrap())
        .unwrap();
    match read_far_nodes(&far, &name, parked.checksum, number_of) {
        FarNodesRead::Found(read) => assert_eq!(read, nodes()),
        other => panic!("{other:?}"),
    }
    // The ship is still in its file.
    assert!(world.grids().get_parked(parked.file.grid).is_some());
}

#[test]
fn a_ship_whose_far_nodes_are_damaged_or_missing_or_of_another_state_comes_back_as_it_was() {
    let (game, mut world, files, parked) = world_with_a_ship_in_a_file();
    let format = game.ship_format(&world);
    let name = FarFileName::of(&parked.file);
    let good = nodes().encode(parked.checksum, name_of).unwrap();

    let mut far = MemoryFarFiles::new();
    assert!(matches!(
        read_far_nodes(&far, &name, parked.checksum, number_of),
        FarNodesRead::Absent
    ));
    // Damaged in the middle, cut short, and not a far nodes file at all.
    for damaged in [
        {
            let mut bytes = good.clone();
            let middle = bytes.len() / 2;
            bytes[middle] ^= 0x10;
            bytes
        },
        good[..good.len() - 3].to_vec(),
        b"VOXUNAMI".to_vec(),
        Vec::new(),
    ] {
        far.write(&name, &damaged).unwrap();
        assert!(matches!(
            read_far_nodes(&far, &name, parked.checksum, number_of),
            FarNodesRead::Unusable(_)
        ));
    }
    // Made for another state of the ship.
    far.write(
        &name,
        &nodes().encode(parked.checksum ^ 1, name_of).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        read_far_nodes(&far, &name, parked.checksum, number_of),
        FarNodesRead::Unusable(FarNodesError::OfAnotherShipFile { .. })
    ));

    // None of that touched the ship.
    world
        .load_ship(parked.file.grid, &files, &format)
        .expect("the ship is read from its own file");
    assert!(world.grids().get(parked.file.grid).is_some());
}

#[test]
fn a_ship_put_away_again_has_a_file_of_its_own_and_the_nodes_of_the_old_one_are_cleaned_away() {
    let (game, mut world, mut files, parked) = world_with_a_ship_in_a_file();
    let format = game.ship_format(&world);
    let id = parked.file.grid;
    let mut far = MemoryFarFiles::new();
    far.write(
        &FarFileName::of(&parked.file),
        &nodes().encode(parked.checksum, name_of).unwrap(),
    )
    .unwrap();

    // Brought in, changed, and put away: another file, another checksum.
    world.load_ship(id, &files, &format).unwrap();
    let ship = world.grids_mut().get_mut(id).unwrap();
    let layout = std::sync::Arc::clone(ship.voxels.layout());
    let block = layout
        .channel_index(&"core:block".parse().unwrap())
        .unwrap();
    let changed = ship
        .voxels
        .set(block, voxel_space::VoxelCoordinate::new(0, 700, 700), 1);
    assert!(changed);
    world.park_ship(id, false, &mut files, &format).unwrap();
    let again = world.grids().get_parked(id).unwrap().clone();
    assert_ne!(again.file, parked.file);
    assert_ne!(again.checksum, parked.checksum);

    // The nodes of the old file are not the new file's: not found under its name, and refused if offered for it.
    let new_name = FarFileName::of(&again.file);
    assert!(matches!(
        read_far_nodes(&far, &new_name, again.checksum, number_of),
        FarNodesRead::Absent
    ));
    let old_name = FarFileName::of(&parked.file);
    assert!(matches!(
        read_far_nodes(&far, &old_name, again.checksum, number_of),
        FarNodesRead::Unusable(FarNodesError::OfAnotherShipFile { .. })
    ));

    // Nodes made for the new file are kept beside them; the world names the new ship file only, and the cleaning keeps what belongs to it.
    far.write(&new_name, &nodes().encode(again.checksum, name_of).unwrap())
        .unwrap();
    let named: BTreeSet<_> = [again.file].into();
    assert_eq!(
        remove_far_files_without_ship_file(&mut far, &named).unwrap(),
        1
    );
    assert_eq!(far.names().unwrap(), [new_name]);
    assert_eq!(
        remove_far_files_without_ship_file(&mut far, &named).unwrap(),
        0
    );
}
