//! The clock of a world in Cave: a tick of the server is a tick of the world's time, and a restart goes on from where it was.

mod common;

use common::Scene;
use std::sync::{Arc, Mutex};

fn world_tick(scene: &mut Scene) -> u64 {
    let found = Arc::new(Mutex::new(0));
    let slot = Arc::clone(&found);
    scene.run(move |world| *slot.lock().unwrap() = world.world_tick());
    std::mem::take(&mut *found.lock().unwrap())
}

#[test]
fn the_world_ticks_once_a_tick_and_a_world_that_is_saved_and_loaded_goes_on_from_there() {
    let mut scene = Scene::new();
    let first = world_tick(&mut scene);
    scene.ticks(10);
    let second = world_tick(&mut scene);
    // (The call itself takes a tick: one for the first, ten, and one for the second.)
    assert_eq!(second - first, 11);
    assert_eq!(scene.rig.server.world().clock(), second);
    let mut scene = scene.restarted();
    let third = world_tick(&mut scene);
    // The restarted world is at the time it was saved at, and one tick on for the call.
    assert_eq!(third, second + 1);
}
