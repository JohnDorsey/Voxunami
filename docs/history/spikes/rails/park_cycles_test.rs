//! E3 of `docs/orbits-and-thrust-plan.md`: a ship that coasts through park and load many times. THROWAWAY, kept to reproduce the numbers there. To run it, copy it to
//! `engine/world_storage/tests/` and `cargo test --release -p world_storage --test park_cycles_test -- --nocapture`.
//!
//! Four ways to keep a ship's orbit through a cycle that saves it and loads it again:
//! - **kept elements**: the rail is written (`encode_rail`) and read (`decode_rail`) and nothing is made from a state;
//! - **state, f32 position and f32 velocity**: what a saved pose is today (a brick and an `f32` offset in it, `f32` velocities), made into an orbit again by `Orbit::from_state`;
//! - **state, f32 position and f64 velocity**: the velocity widened;
//! - **state, exact position and f64 velocity**: nothing but the `f64` working of `from_state` is lost.
//! (Today's code does none of them: `set_grid_frozen` zeroes the velocities, so a frozen ship loses its orbit at once. That is read from the code, not measured here.)

use std::f64::consts::TAU;

use world_storage::rail::{
    Centre, ExactOffset, Orbit, Rail, RailTime, decode_rail, encode_rail,
};

const NOWHERE: [f64; 3] = [0.0; 3];
const PER_SECOND: u32 = 20;

fn time(tick: u64) -> RailTime {
    RailTime::of_tick(tick, PER_SECOND)
}

fn state(orbit: Orbit, tick: u64) -> (ExactOffset, [f64; 3]) {
    let state = Rail {
        spin: None,
        orbit: Some(orbit),
    }
    .state(time(tick), NOWHERE, NOWHERE);
    (state.offset.unwrap(), state.velocity)
}

fn apart(first: ExactOffset, second: ExactOffset) -> f64 {
    (0..3)
        .map(|axis| {
            let difference = (first.bricks[axis] - second.bricks[axis]) as f64 * 32.0
                + (first.rest[axis] - second.rest[axis]);
            difference * difference
        })
        .sum::<f64>()
        .sqrt()
}

/// The offset as a saved pose holds it: the brick whole and the offset in it an `f32`.
fn as_saved_position(offset: ExactOffset) -> ExactOffset {
    ExactOffset {
        bricks: offset.bricks,
        rest: offset.rest.map(|part| f64::from(part as f32)),
    }
}

#[derive(Clone, Copy)]
enum Way {
    Kept,
    F32PositionF32Velocity,
    F32PositionF64Velocity,
    ExactPositionF64Velocity,
}

#[test]
fn park_cycles() {
    // (name, semi-major axis in voxels (metres), ratio, mu in voxels^3 / s^2, a cycle every this many ticks)
    let scenarios = [
        ("a low orbit round a planet of Earth's weight", 6.8e6, 0.9, 3.986e14, 1_200u64),
        ("a moon's orbit round it", 3.84e8, 0.945, 3.986e14, 12_000),
        ("a planet's year round a star", 1.5e11, 0.983, 1.327e20, 1_200_000),
    ];
    for (name, radius, ratio, mu, step) in scenarios {
        let period = TAU * libm::sqrt(radius * radius * radius / mu);
        let original = Orbit::elliptical(Centre::Point(NOWHERE), [0.3, 0.9, -0.2], radius, ratio, 1.1, period, 2.0).unwrap();
        println!("PARK {name}: a = {radius:e}, ratio {ratio}, period {period:.0} s, a cycle every {step} ticks ({:.4} of a period)", step as f64 / PER_SECOND as f64 / period);
        for (label, way) in [
            ("kept elements", Way::Kept),
            ("f32 place, f32 speed", Way::F32PositionF32Velocity),
            ("f32 place, f64 speed", Way::F32PositionF64Velocity),
            ("exact place, f64 speed", Way::ExactPositionF64Velocity),
        ] {
            let mut orbit = original;
            let mut tick = 1_000_000u64;
            let mut line = String::new();
            for cycle in 1..=1000u32 {
                tick += step;
                orbit = match way {
                    Way::Kept => {
                        let bytes = encode_rail(&Rail { spin: None, orbit: Some(orbit) });
                        decode_rail(&bytes).unwrap().orbit.unwrap()
                    }
                    _ => {
                        let (place, velocity) = state(orbit, tick);
                        let (place, velocity) = match way {
                            Way::F32PositionF32Velocity => (as_saved_position(place), velocity.map(|part| f64::from(part as f32))),
                            Way::F32PositionF64Velocity => (as_saved_position(place), velocity),
                            _ => (place, velocity),
                        };
                        Orbit::from_state(Centre::Point(NOWHERE), place, velocity, mu, time(tick)).unwrap()
                    }
                };
                if [1, 10, 100, 1000].contains(&cycle) {
                    // How far the ship is from where the original orbit puts it a thousand periods later, and how far its period and axis have moved.
                    let later = tick + (1000.0 * period * PER_SECOND as f64) as u64;
                    let (there, _) = state(original, later);
                    let (here, _) = state(orbit, later);
                    line += &format!(
                        " | {cycle:>4} cycles: axis {:+.1e}, period {:+.1e}, {:.2e} of a radius out after 1000 periods",
                        (orbit.radius - radius) / radius,
                        (orbit.period - period) / period,
                        apart(there, here) / radius
                    );
                }
            }
            println!("PARK   {label:<24}{line}");
        }
    }
}
