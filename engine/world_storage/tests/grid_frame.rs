//! Where a voxel of any grid is in the world.

use voxel_space::{
    BrickCoordinate, GridId, GridPose, LevelIndex, Orientation, VoxelCoordinate, WorldPosition,
};
use world_storage::GridFrame;
use world_storage::test_support::{ALL_PLUGINS, Game};

fn at(x: i128, y: i128, z: i128) -> VoxelCoordinate {
    VoxelCoordinate::new(x, y, z)
}

#[test]
fn a_voxel_of_terrain_is_where_it_is_and_a_voxel_of_a_ship_is_turned_and_placed_with_it() {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = game.new_world();
    let mut ship = game.sample_ship(&world, 0);
    // A quarter turn about the vertical axis: the ship's +x is the world's -z. Its origin is at (100, 50, 200).
    ship.pose = GridPose::new(
        WorldPosition::new(BrickCoordinate::new(3, 1, 6), [4.0, 18.0, 8.0]),
        Orientation::from_yaw_pitch(std::f32::consts::FRAC_PI_2, 0.0),
    );
    let id = world.grids_mut().create(ship);

    // Terrain: the middle of the voxel, in the level the grid is the terrain of.
    let (level, place) = world
        .voxel_in_world(GridId::MAIN, at(-1, 33, 70))
        .expect("the main level is there");
    assert_eq!(level, LevelIndex::MAIN);
    assert_eq!(
        place,
        WorldPosition::new(BrickCoordinate::new(-1, 1, 2), [31.5, 1.5, 6.5])
    );

    // A ship: the voxel's middle is 2.5 along the ship's x, so 2.5 toward -z of the origin; and one along y and z is 1.5 up and 0.5 along its z, which
    // is the world's x.
    let (level, place) = world.voxel_in_world(id, at(2, 1, 0)).unwrap();
    assert_eq!(level, LevelIndex::MAIN);
    let found = [
        place.brick().x() as f32 * 32.0 + place.offset()[0],
        place.brick().y() as f32 * 32.0 + place.offset()[1],
        place.brick().z() as f32 * 32.0 + place.offset()[2],
    ];
    let origin = [100.0, 50.0, 200.0];
    let wanted = [origin[0] + 0.5, origin[1] + 1.5, origin[2] - 2.5];
    for axis in 0..3 {
        assert!(
            (found[axis] - wanted[axis]).abs() < 1e-3,
            "{found:?} against {wanted:?}"
        );
    }
    assert_eq!(
        world.grid_frame(id).map(|frame| frame.level()),
        Some(LevelIndex::MAIN)
    );
}

#[test]
fn a_grid_that_is_not_there_has_no_place_and_a_level_is_its_own_frame() {
    let game = Game::new(&ALL_PLUGINS);
    let world = game.new_world();
    // A level and a ship that do not exist.
    assert_eq!(
        world.voxel_in_world(GridId::root(LevelIndex::new(7)), at(0, 0, 0)),
        None
    );
    assert_eq!(
        world.grid_frame(GridId::new(GridId::FIRST_ALLOCATED + 5)),
        None
    );
    assert_eq!(
        world.grid_frame(GridId::MAIN),
        Some(GridFrame::Level(LevelIndex::MAIN))
    );
}
