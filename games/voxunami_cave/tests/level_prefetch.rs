//! The terrain of a level round a walker is made by a worker before they walk into it.

mod common;

use common::{Scene, bricks_prefetched, tick_and_follow, tick_until, tick_until_quiet};

#[test]
fn the_terrain_round_a_walker_is_made_by_a_worker_and_more_of_it_as_they_move() {
    let mut scene = Scene::new();
    let mut player = scene.rig.join_walking([16.5, 110.0, 16.5], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    // (The worker is a thread, and ticks here take microseconds: they are given the time of a server's, until it has made all it was asked for.)
    let standing = tick_until_quiet(
        &mut scene,
        "the bricks round a walker standing",
        |scene| tick_and_follow(scene, &mut player, 1),
        bricks_prefetched,
    );
    assert!(standing >= 40, "{standing} made for a walker standing");
    // Sixty voxels along, there are bricks they had not got.
    assert!(common::say(&mut scene, &mut player, "/tp 16.5 110 -50").contains("teleported"));
    tick_until(
        &mut scene,
        "the bricks of the place they went to",
        |scene| tick_and_follow(scene, &mut player, 1),
        |scene| bricks_prefetched(scene) > standing + 20,
    );
}
