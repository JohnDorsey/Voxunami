//! The terrain of a level round a walker is made by a worker before they walk into it.

mod common;

use common::{PlayerExt, Scene, tick_and_follow};
use std::sync::{Arc, Mutex};

fn prefetched_so_far(scene: &mut Scene) -> u64 {
    let found = Arc::new(Mutex::new(0));
    let slot = Arc::clone(&found);
    scene.run(move |world| *slot.lock().unwrap() = world.bricks_prefetched());
    std::mem::take(&mut *found.lock().unwrap())
}

#[test]
fn the_terrain_round_a_walker_is_made_by_a_worker_and_more_of_it_as_they_move() {
    let mut scene = Scene::new();
    let mut player = scene.rig.join_walking([16.5, 110.0, 16.5], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    // (The worker is a thread, and ticks here take microseconds: they are given the time of a server's.)
    for _ in 0..40 {
        tick_and_follow(&mut scene, &mut player, 1);
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    let standing = prefetched_so_far(&mut scene);
    assert!(standing >= 40, "{standing} made for a walker standing");
    // Sixty voxels along, there are bricks they had not got.
    assert!(common::say(&mut scene, &mut player, "/tp 16.5 110 -50").contains("teleported"));
    for _ in 0..40 {
        tick_and_follow(&mut scene, &mut player, 1);
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    assert!(prefetched_so_far(&mut scene) > standing + 20);
}
