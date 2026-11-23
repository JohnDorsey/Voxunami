//! A probe of 2026-11-22 (`docs/specs/viewpoints-spec.md` section 9i), kept as a record and not compiled with the workspace: what `LodView::update` costs for a view that keeps
//! nothing warm, at rest and walking, so that the same file run in the tree before the warm viewpoint was made (e2dcfab) and in the tree after says what the code costs
//! while nobody uses it. Copy it to `engine/world_view/tests/probe_idle_update.rs` (it uses `tests/common`, which both trees have), then
//! `cargo test --release -p world_view --test probe_idle_update -- --nocapture --test-threads=1`, and remove it again.

use std::time::{Duration, Instant};

use coarse_cells::DetailSettings;
use voxel_space::{BrickCoordinate, GridId, WorldPosition};
use world_storage::test_support::brick;
use world_view::{LodConfig, LodSettings, LodView, MeshUpdate};

mod common;

fn settings() -> LodSettings {
    LodSettings {
        detail: DetailSettings {
            cell_pixels: 3.0,
            pixel_size: 2.0 * 0.700_207_538_209_062_3 / 1080.0,
        },
        zoom: None,
        maximum_distance: 4.0e4,
    }
}

fn viewer_in(at: BrickCoordinate) -> WorldPosition {
    WorldPosition::new(at, [16.0, 20.0, 16.0])
}

fn report(name: &str, mut microseconds: Vec<f64>) {
    microseconds.sort_by(f64::total_cmp);
    let calls = microseconds.len();
    let at = |fraction: f64| microseconds[((calls - 1) as f64 * fraction) as usize];
    println!(
        "{name}: {calls} calls: mean {:.2} us, median {:.2}, p99 {:.2}, p99.9 {:.2}, worst {:.1} us",
        microseconds.iter().sum::<f64>() / calls as f64,
        at(0.5),
        at(0.99),
        at(0.999),
        at(1.0)
    );
}

#[test]
fn probe_idle_update() {
    let mut scene = common::scene();
    scene.assign_around(brick(0, 0, 0), 400);
    let rounds: usize = std::env::var("PROBE_ROUNDS")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(5);
    for round in 0..rounds {
        let mut view = LodView::new(
            scene.mesher(3),
            LodConfig {
                unlit: 0,
                worker_count: 3,
            },
        );
        view.hold_hidden_pictures(64 << 20);
        let began = Instant::now();
        loop {
            let _ = view.update(
                GridId::MAIN,
                scene.world.main(),
                viewer_in(brick(0, 0, 0)),
                &settings(),
            );
            if view.is_settled() {
                break;
            }
            assert!(began.elapsed() < Duration::from_secs(600), "does not settle");
            std::thread::sleep(Duration::from_micros(300));
        }
        // At rest: nothing to do.
        let mut rest = Vec::new();
        for _ in 0..20_000 {
            let timed = Instant::now();
            let updates: Vec<MeshUpdate> = view.update(
                GridId::MAIN,
                scene.world.main(),
                viewer_in(brick(0, 0, 0)),
                &settings(),
            );
            rest.push(timed.elapsed().as_secs_f64() * 1e6);
            assert!(updates.is_empty());
        }
        report(&format!("round {round}: at rest"), rest);
        // Walking a brick at every update (the cubes are worked out again at each).
        let mut walking = Vec::new();
        for step in 1..=300 {
            let timed = Instant::now();
            let _ = view.update(
                GridId::MAIN,
                scene.world.main(),
                viewer_in(brick(step, 0, 0)),
                &settings(),
            );
            walking.push(timed.elapsed().as_secs_f64() * 1e6);
        }
        report(&format!("round {round}: walking a brick an update"), walking);
    }
}
