//! The far nodes of ships that are in files: they are read without bringing the ship in, they belong to one state of the ship, and a file that is damaged,
//! missing or of another state costs the ship's far view and nothing else.

use std::collections::BTreeSet;

use coarse_cells::{CELLS_PER_NODE, CoarseCell, CoarseNode};
use namespaced_id::NamespacedId;
use region_octree::BrickCube;
use voxel_space::{BrickCoordinate, GridId};
use world_storage::far_nodes::{
    FarFileName, FarFiles, FarNodes, FarNodesError, FarNodesRead, MemoryFarFiles, read_far_nodes,
    remove_far_files_without_ship_file,
};
use world_storage::test_support::{ALL_PLUGINS, Game, sample_world};
use world_storage::{MemoryShipFiles, ShipFiles, WorldData};

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

#[test]
fn a_save_cleans_away_the_far_nodes_of_ship_files_it_no_longer_names_and_keeps_those_of_the_ships_in_files()
 {
    let (game, mut world, mut files, parked) = world_with_a_ship_in_a_file();
    let kept = FarFileName::of(&parked.file);
    // The nodes of an earlier file of the same ship, and of a ship that is gone: nothing names their ship files.
    let earlier = FarFileName::new(parked.file.grid, parked.file.serial + 100);
    let gone = FarFileName::new(GridId::new(GridId::FIRST_ALLOCATED + 900), 3);
    for name in [kept, earlier, gone] {
        files
            .far_files_mut()
            .write(&name, &nodes().encode(parked.checksum, name_of).unwrap())
            .unwrap();
    }
    let ship_files_before = files.names().unwrap();

    let save = game.save_with_ships(&mut world);
    save.write_with(&mut files, |_| Ok(())).unwrap();

    assert_eq!(files.far_files_mut().names().unwrap(), [kept]);
    // (The ship files were cleaned as before, and the far nodes files did not take their place in the listing.)
    assert_eq!(files.names().unwrap(), ship_files_before);
}

#[test]
fn the_numbers_of_blocks_and_their_names_are_the_formats_both_ways_for_the_channels_that_hold_content_only()
 {
    let (game, world, _, _) = world_with_a_ship_in_a_file();
    let format = game.ship_format(&world);
    let layout = format.layout();
    let block = layout
        .channel_index(&"core:block".parse().unwrap())
        .unwrap();
    for name in ["base_blocks:air", "base_blocks:stone"] {
        let id: NamespacedId = name.parse().unwrap();
        let number = game.raw_block(name);
        assert_eq!(format.content_identifier(block, number), Some(id.clone()));
        assert_eq!(format.content_number(block, &id), Some(number));
    }
    let unknown: NamespacedId = "base_blocks:not_a_block".parse().unwrap();
    assert_eq!(format.content_number(block, &unknown), None);
    assert_eq!(format.content_identifier(block, u32::MAX), None);
    // A channel that holds plain numbers (a temperature) has no names.
    let plain = layout
        .channels()
        .iter()
        .position(|channel| {
            !matches!(
                channel.value_kind,
                voxel_grid::ChannelValueKind::Content { .. }
            )
        })
        .expect("the sample layout has a channel of plain numbers");
    let plain = layout
        .channel_index(&layout.channels()[plain].identifier)
        .unwrap();
    assert_eq!(format.content_identifier(plain, 0), None);
    assert_eq!(
        format.content_number(plain, &"base_blocks:air".parse().unwrap()),
        None
    );
}

#[test]
fn a_save_writes_the_far_files_it_carries_beside_the_ship_files_and_removes_those_of_ship_files_it_does_not_name()
 {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = sample_world(&game);
    let made = game.sample_ship(&world, 0);
    let first = world.grids_mut().create(made);
    let mut save = game.save_with_ships(&mut world);
    // The ships the save made files of, with the checksums of the files: what a far file is made from is the state of the file.
    let ships = save.new_ships();
    assert_eq!(ships.len(), 1);
    let (grid, file, checksum) = (ships[0].grid, ships[0].file, ships[0].checksum);
    assert_eq!(grid, first);
    assert!(!ships[0].has_terrain);
    assert_eq!(checksum, crc32fast::hash(&save.new_ship_files[0].1));
    save.new_far_files.push((
        FarFileName::of(&file),
        nodes().encode(checksum, name_of).unwrap(),
    ));

    let mut files = MemoryShipFiles::new();
    // (A far file of an earlier state of the ship is left by a save that did not finish.)
    let stale = FarFileName::new(first, file.serial + 50);
    files.far_files_mut().write(&stale, &[1, 2, 3]).unwrap();
    save.write_with(&mut files, |_| Ok(())).unwrap();
    match read_far_nodes(
        files.far_files_mut(),
        &FarFileName::of(&file),
        checksum,
        number_of,
    ) {
        FarNodesRead::Found(read) => assert_eq!(read, nodes()),
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        read_far_nodes(files.far_files_mut(), &stale, checksum, number_of),
        FarNodesRead::Absent
    ));
}

#[test]
fn a_save_says_which_of_the_ships_it_made_files_of_are_planets() {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = sample_world(&game);
    let made = game.sample_ship(&world, 0);
    let ship = world.grids_mut().create(made);
    let made = game.sample_planet(&world, 0);
    let planet = world.grids_mut().create(made);
    let save = game.save_with_ships(&mut world);
    let ships = save.new_ships();
    assert_eq!(ships.len(), 2);
    for saved in &ships {
        assert_eq!(saved.has_terrain, saved.grid == planet, "{saved:?}");
        assert!(saved.grid == ship || saved.grid == planet);
    }
}
