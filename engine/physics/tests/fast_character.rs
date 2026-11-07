//! A character that goes at the speed of a rocket (a walker who has stepped off a ship) is given colliders for the bricks ahead of it, and not for a cube of them.
//!
//! Found by the owner, who got off the dev rocket at some kilometres a second and saw themselves and it "float apart at 2 jumps a second": each tick of the server took three seconds
//! at 6,000 voxels a second. The bricks within a margin of the character that has colliders is `8 + 0.25 * speed` voxels every way, a cube of 94 bricks across (830,000 bricks) in
//! each tick. It is the cube up to a cap and past it a tube the way the character goes (`SweptBricks`). Counted in bricks, which the source of the terrain is asked about once each:
//! what is simulated must not depend on how fast a machine is.

use physics::{BrickHint, CharacterBody, CharacterSettings, Collision, CollisionSource};
use voxel_space::{BrickCoordinate, VoxelCoordinate, WorldPosition};

/// Space with nothing in it, that counts the bricks it is asked about as a whole.
#[derive(Default)]
struct Space {
    bricks_asked: usize,
}

impl CollisionSource for Space {
    fn collision_at(&mut self, _voxel: VoxelCoordinate) -> Collision {
        Collision::Empty
    }

    fn brick_hint(&mut self, _brick: BrickCoordinate) -> BrickHint {
        self.bricks_asked += 1;
        BrickHint::Empty
    }
}

fn going_up_at(speed: f64) -> CharacterBody {
    let place = WorldPosition::new(BrickCoordinate::new(0, 100, 0), [10.0, 10.0, 10.0]);
    let mut body = CharacterBody::new(place, CharacterSettings::default());
    // As a walker who stepped off a ship is put in the level: where they were, at the speed of the ship.
    body.reframe(place, [0.0, speed, 0.0], [0.0, 0.0, 0.0, 1.0]);
    body
}

#[test]
fn a_character_going_six_kilometres_a_second_has_colliders_for_a_tube_of_bricks_and_not_a_cube_of_them()
 {
    let mut body = going_up_at(6000.0);
    let mut space = Space::default();
    body.update_terrain(&mut space);
    // The way ahead for a quarter of a second, 1,500 voxels, 47 bricks, through a tube 9 across: some thousands. The cube of that margin is 94 across: 830,000.
    let first = space.bricks_asked;
    assert!(first < 8_000, "{first} bricks were asked about at once");
    // A tick later the character has gone 300 voxels, 9 or 10 bricks, which is more than the 6 bricks it may be from the origin of its frame: the frame is built again round it, and its
    // colliders with it, so it is the whole tube again (4,212 measured), and not only the 760 or so bricks that have come into it. The cube of the margin was 830,000 in each of these.
    body.displace([0.0, 300.0, 0.0]);
    space.bricks_asked = 0;
    body.update_terrain(&mut space);
    let later = space.bricks_asked;
    assert!(later < 8_000, "{later} bricks were asked about in a tick");
    assert!(
        later > 1_000,
        "the way ahead was not made as it came: {later}"
    );
}

#[test]
fn a_character_that_is_slow_has_the_cube_it_always_had() {
    // 300 voxels a second: the margin is 8 + 75 = 83 voxels every way from the middle of the body, which is at about 10 in each axis of the brick it is in: -73 to 93, bricks -3 to 2,
    // 6 across, 216 in all. (Under the cap, whatever the cap is.)
    let mut body = going_up_at(300.0);
    let mut space = Space::default();
    body.update_terrain(&mut space);
    assert_eq!(space.bricks_asked, 216);
    // At rest the margin is 8 voxels: the brick it is in alone.
    let mut still = going_up_at(0.0);
    let mut space = Space::default();
    still.update_terrain(&mut space);
    assert_eq!(space.bricks_asked, 1);
}
