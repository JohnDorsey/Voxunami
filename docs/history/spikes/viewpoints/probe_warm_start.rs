//! A probe of 2026-11-22 (`docs/specs/viewpoints-spec.md` section 9i), kept as a record and not compiled with the workspace: how long the call of `LodView::update` takes that has the
//! cubes of a warm viewpoint to work out (the first after the viewpoint is set or has moved), and the longest call while what it wants is made, with the cubes worked out on the
//! thread of the update (e2dcfab..7df5ff2) and on a thread of their own. Copy it to `engine/world_view/tests/probe_warm_start.rs` (it uses `tests/common`), then
//! `cargo test --release -p world_view --test probe_warm_start -- --nocapture --test-threads=1`, and remove it again.

use std::time::{Duration, Instant};

use coarse_cells::DetailSettings;
use voxel_space::{BrickCoordinate, GridId, WorldPosition};
use world_storage::test_support::brick;
use world_view::{LodConfig, LodSettings, LodView, Viewpoint};

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

#[test]
fn probe_warm_start() {
    let mut scene = common::scene();
    scene.assign_around(brick(0, 0, 0), 400);
    {
        let terrain = &scene.world.main().terrain;
        for _ in 0..3 {
            let began = Instant::now();
            let copy = terrain.clone();
            println!(
                "the assignment tree has {} nodes; a copy of the terrain takes {:.3} ms",
                copy.assignments().node_count(),
                began.elapsed().as_secs_f64() * 1e3
            );
        }
    }
    let rounds: usize = std::env::var("PROBE_ROUNDS")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(3);
    for round in 0..rounds {
        let mut view = LodView::new(
            scene.mesher(3),
            LodConfig {
                unlit: 0,
                worker_count: 3,
            },
        );
        view.hold_hidden_pictures(256 << 20);
        let drawn = viewer_in(brick(0, 100, 0));
        let began = Instant::now();
        // The sky is drawn, and everything is made.
        let settle = |view: &mut LodView, longest: &mut Duration, set_at: Instant| {
            let _ = set_at;
            loop {
                let timed = Instant::now();
                let _ = view.update(GridId::MAIN, scene.world.main(), drawn, &settings());
                *longest = (*longest).max(timed.elapsed());
                if view.is_settled() {
                    return;
                }
                assert!(began.elapsed() < Duration::from_secs(900), "does not settle");
                std::thread::sleep(Duration::from_micros(300));
            }
        };
        let mut longest = Duration::ZERO;
        settle(&mut view, &mut longest, Instant::now());
        for (index, place) in [
            brick(0, 0, 0),
            brick(48, 0, 0),
            brick(0, 0, 96),
            brick(150, 0, 0),
            brick(0, 0, 200),
        ]
        .into_iter()
        .enumerate()
        {
            view.set_warm_viewpoint(Some(Viewpoint {
                viewer: viewer_in(place),
                settings: settings(),
            }));
            let timed = Instant::now();
            let _ = view.update(GridId::MAIN, scene.world.main(), drawn, &settings());
            let first = timed.elapsed();
            let mut longest_after = Duration::ZERO;
            settle(&mut view, &mut longest_after, Instant::now());
            println!(
                "round {round}, warm place {index} ({place:?}): the update that has the cubes to work out takes {:.2} ms; the longest of the others while the rest is made {:.2} ms",
                first.as_secs_f64() * 1e3,
                longest_after.as_secs_f64() * 1e3
            );
        }
    }
}
