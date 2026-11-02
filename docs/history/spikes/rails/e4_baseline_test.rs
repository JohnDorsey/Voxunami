//! E4 of `docs/orbits-and-thrust-plan.md`: what the 126-bit fixed-point rail costs now, for comparison with the big-float libraries of `e4/` (the owner's orbit of E1: nearest point
//! 7,000 km, farthest one light year, round the Earth's G M). THROWAWAY. To run it, copy it to `engine/world_storage/tests/` and
//! `cargo test --release -p world_storage --test e4_baseline_test -- --nocapture`.

use std::f64::consts::TAU;
use std::time::Instant;

use world_storage::rail::{Centre, Orbit, Rail, RailTime};

const MU: f64 = 4.0e14;

fn orbit_between(periapsis: f64, apoapsis: f64) -> Orbit {
    let axis = (periapsis + apoapsis) / 2.0;
    Orbit::elliptical(
        Centre::Point([0.0; 3]),
        [0.0, 0.0, 1.0],
        axis,
        periapsis / axis,
        0.0,
        TAU * (axis * axis * axis / MU).sqrt(),
        0.0,
    )
    .unwrap()
}

#[test]
fn the_cost_of_a_place_on_a_rail_at_126_bits() {
    for (label, apoapsis) in [("a moderate ellipse (7,000 to 14,000 km)", 1.4e7), ("the owner's orbit (7,000 km to a light year)", 9.4607e15)] {
        let rail = Rail {
            spin: None,
            orbit: Some(orbit_between(7.0e6, apoapsis)),
        };
        // A tick near the nearest point (hard for the solver), one a quarter of the way round, and one near the far end.
        let period_ticks = (rail.orbit.unwrap().period * 20.0) as u64;
        for (where_, tick) in [("near the nearest point", 12_000u64), ("a quarter round", period_ticks / 4), ("near the far end", period_ticks / 2 - 7)] {
            let calls = 3000u64;
            let began = Instant::now();
            let mut sink = 0.0;
            for call in 0..calls {
                let state = rail.state(RailTime::of_tick(tick + call, 20), [0.0; 3], [0.0; 3]);
                sink += state.velocity[0];
            }
            let each = began.elapsed().as_nanos() as f64 / calls as f64;
            println!("E4 baseline: {label}, {where_}: {:.1} us a place and speed (126-bit fixed point; {sink:e})", each / 1e3);
        }
    }
}
