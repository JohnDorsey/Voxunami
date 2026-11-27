//! What the crosshair meets of a block's surface, as a session over a real server sees it: the side, the point, and the picture and the place on it. (`Session::target` finds
//! a voxel; `Session::target_surface` finds what the block fills of it.)

mod common;

use common::{PlayerExt, Rig, Scene, above_the_ground, raw_in, voxels_of};
use game_shell::view::camera_at;
use server::GridView;
use voxel_space::{FaceDirection, GridId, LatticeRotation, LevelIndex, VoxelCoordinate};
use voxunami_cave::content::CaveContent;
use voxunami_cave::game::request_block_with_state;

const STONE: &str = "base_blocks:stone";
const BOTTOM_SLAB: u32 = 1;

fn near(value: f64, expected: f64, within: f64) -> bool {
    (value - expected).abs() < within
}

#[test]
fn looking_down_at_a_slab_meets_its_top_in_the_middle_of_the_voxel_and_the_picture_there() {
    let mut rig = Rig::new();
    let (mut player, above) = above_the_ground(&mut rig, None);
    request_block_with_state(&mut player, above, STONE, BOTTOM_SLAB);
    rig.settle(&mut [&mut player]);

    let content = CaveContent::new();
    let found = player
        .target_surface()
        .expect("the slab is under the crosshair");
    assert_eq!(found.hit.voxel, above);
    assert_eq!(found.hit.surface.face, FaceDirection::PositiveY);
    // The top of a bottom slab is half way up the voxel, not at its top (what `target` would say is the voxel's).
    assert_eq!(found.hit.surface.point[1], 0.5);
    // (The camera cannot look exactly down: it is held a little short of it, so the ray is a little slanted, and meets the top a few hundredths off the middle.)
    assert!(
        near(found.hit.surface.point[0], 0.5, 0.05),
        "{:?}",
        found.hit
    );
    assert!(
        near(found.hit.surface.point[2], 0.5, 0.05),
        "{:?}",
        found.hit
    );
    assert_eq!(
        (
            found.content.block,
            found.content.state,
            found.content.orientation
        ),
        (raw_in(&content, STONE), BOTTOM_SLAB, 0)
    );
    assert_eq!(player.target().unwrap().voxel, above);
    // The eyes are two voxels over the ground that the slab's voxel is on, so a little over a voxel and a half above the slab's top (a little more along the slant).
    assert!(
        near(found.hit.surface.distance, 1.55, 0.1),
        "{:?}",
        found.hit
    );
    // The stone has a picture on its top, and the place on it is the middle of it (the top as it is made: across +x, up -z).
    let picture = found.picture.expect("stone has pictures");
    assert_eq!(picture.picture.across, FaceDirection::PositiveX);
    assert_eq!(picture.picture.up, FaceDirection::NegativeZ);
    assert_eq!(
        picture.place,
        picture.picture.place_of(found.hit.surface.point)
    );
    assert!(
        near(picture.place.across, 0.5, 0.05) && near(picture.place.down, 0.5, 0.05),
        "{picture:?}"
    );
}

#[test]
fn a_ray_over_a_slab_meets_the_block_behind_it_where_a_voxel_walk_stops_at_the_slab() {
    let mut rig = Rig::new();
    let (mut player, above) = above_the_ground(&mut rig, None);
    // Ask for eyes three quarters of the way up the voxel over the ground, and see where the server puts them (it decides how high a player's eyes are over a position it is
    // told). They are over the top of a slab that stands in their own row of voxels (which is at a half) if they are more than half way up their voxel.
    player.camera = camera_at(
        [
            above.x() as f64 + 0.5,
            above.y() as f64 + 0.75,
            above.z() as f64 + 0.5,
        ],
        -90.0,
        0.0,
    );
    player.teleport_to_camera();
    rig.settle(&mut [&mut player]);
    let eyes = voxels_of(&player.camera);
    let row = eyes[1].floor() as i128;
    let rise = eyes[1] - row as f64;
    assert!(
        rise > 0.5 && rise < 1.0,
        "the eyes are {rise} of the way up their voxel"
    );
    // A bottom slab in the next voxel, then two empty voxels, then a whole block.
    let slab = VoxelCoordinate::new(eyes[0].floor() as i128 + 1, row, eyes[2].floor() as i128);
    let wall = VoxelCoordinate::new(slab.x() + 3, row, slab.z());
    request_block_with_state(&mut player, slab, STONE, BOTTOM_SLAB);
    request_block_with_state(&mut player, wall, STONE, 0);
    player.camera = camera_at(eyes, -90.0, 0.0);
    let forward = player.camera.forward();
    assert!(
        forward.x > 0.99 && forward.y.abs() < 1e-6 && forward.z.abs() < 0.01,
        "a yaw of -90 degrees looks along +x: {forward:?}"
    );
    rig.settle(&mut [&mut player]);
    assert_eq!(voxels_of(&player.camera), eyes, "the eyes did not move");

    // What a walk of voxels says: the slab's voxel, since there is something in it.
    assert_eq!(player.target().unwrap().voxel, slab);
    // What the shapes say: the ray goes over the slab and meets the wall's side that looks back at the player, as high up it as the eyes are in their voxel.
    let found = player
        .target_surface()
        .expect("the wall is behind the slab");
    assert_eq!(found.hit.voxel, wall);
    assert_eq!(found.hit.surface.face, FaceDirection::NegativeX);
    assert_eq!(found.hit.surface.point[0], 0.0);
    assert!(
        near(found.hit.surface.point[1], rise, 1e-9),
        "{:?}",
        found.hit
    );
    assert!(
        near(found.hit.surface.point[2], 0.5, 1e-9),
        "{:?}",
        found.hit
    );
    assert!(
        near(found.hit.surface.distance, wall.x() as f64 - eyes[0], 1e-9),
        "{:?}",
        found.hit
    );
    // The side of a block as it is made looks like this: across is +z and up +y for the face that looks toward -x. So the place is across the middle, and as far from the top as the eyes are below the top of their voxel.
    let picture = found.picture.expect("stone has pictures");
    assert_eq!(picture.picture.across, FaceDirection::PositiveZ);
    assert_eq!(picture.picture.up, FaceDirection::PositiveY);
    assert!(near(picture.place.across, 0.5, 1e-9), "{picture:?}");
    assert!(near(picture.place.down, 1.0 - rise, 1e-9), "{picture:?}");
}

#[test]
fn nothing_under_the_crosshair_is_no_surface() {
    let mut rig = Rig::new();
    // Looking at the sky from high above the ground.
    let mut player = rig.join([16.5, 150.0, 16.5], 0.0, 80.0);
    rig.settle(&mut [&mut player]);
    assert_eq!(player.target_surface(), None);
    assert_eq!(player.target(), None);
}

/// The number of the turn that takes up to +x (a log standing up that is laid on its side, its end toward +x).
fn turn_up_to_positive_x() -> u32 {
    LatticeRotation::all()
        .into_iter()
        .position(|rotation| rotation.apply([0, 1, 0]) == [1, 0, 0])
        .expect("a turn that lays the top on its side") as u32
}

#[test]
fn a_block_turned_on_its_side_shows_the_picture_of_the_face_it_turned_to_and_says_how_it_is_turned()
{
    let mut scene = Scene::new();
    let (mut player, above) = above_the_ground(&mut scene.rig, None);
    let content = CaveContent::new();
    let (log, turned) = (content.blocks.wood, turn_up_to_positive_x());
    scene.run(move |world| {
        let mut terrain = world.grid(GridId::root(LevelIndex::MAIN)).unwrap();
        terrain.set(content.block_channel(), above, log).unwrap();
        terrain
            .set(content.orientation_channel(), above, turned)
            .unwrap();
    });
    scene.rig.settle(&mut [&mut player]);

    let found = player
        .target_surface()
        .expect("the log is under the crosshair");
    assert_eq!(found.hit.voxel, above);
    assert_eq!(found.hit.surface.face, FaceDirection::PositiveY);
    assert_eq!(found.content.orientation, turned);
    // A log on its side has its bark on the face that was its top (the picture of its sides, turned), not the picture of its end.
    let table = player.block_table();
    let wanted = table
        .face_picture(log, 0, turned, FaceDirection::PositiveY)
        .unwrap();
    assert_eq!(found.picture.unwrap().picture, wanted);
    assert_ne!(
        wanted,
        table
            .face_picture(log, 0, 0, FaceDirection::PositiveY)
            .unwrap(),
        "the turn changes the picture of that face"
    );
}
