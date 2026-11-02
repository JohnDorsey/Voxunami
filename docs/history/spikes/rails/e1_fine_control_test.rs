//! E1 of `docs/orbits-and-thrust-plan.md`: how exactly can an orbit be steered, and how exactly can it be integrated? THROWAWAY, kept to reproduce the numbers there. To run it,
//! copy it to `engine/world_storage/tests/` and `cargo test --release -p world_storage --test e1_fine_control_test -- --nocapture`; the positions it prints for checking are
//! read by `e1_check_positions.py` in this directory (run beside `engine/world_storage/tools/exact_ellipse_reference.py`, which it uses).
//!
//! The owner's question (2026-11-02): in an orbit round some body with the periapsis in kilometres and the apoapsis in light years, a player at the periapsis throttles a thruster far
//! below 1 percent and changes the apoapsis by amounts small enough to tune a rendezvous with another ship. Which parts of the game are too coarse for it?
//!
//! The orbit here is round a body of the Earth's `G M` (4e14 m^3/s^2; a voxel is a metre), nearest point 7,000 km, farthest one light year (9.4607e15 m). Parts:
//! - **A**: what the elements can say (the spacing of the numbers that hold the orbit);
//! - **B**: what making an orbit again from a state (`Orbit::from_state`, what every disturbance of a ship costs today) loses, at the nearest point and elsewhere;
//! - **C**: a push along the speed at the nearest point, of 1e-3 m/s down to 1e-14, and what each way of keeping the result says the apoapsis became;
//! - **D**: positions to be checked against the 120-digit reference;
//! - **E**: the original question of E1: a fixed-step integrator (leapfrog, and a fourth-order composition of it) against the exact rail, for 1e5 to 1e7 steps.

use std::f64::consts::TAU;

use world_storage::rail::{Centre, ExactOffset, Orbit, Rail, RailTime};

const MU: f64 = 4.0e14;
const NOWHERE: [f64; 3] = [0.0; 3];
const PER_SECOND: u32 = 20;
const LIGHT_YEAR: f64 = 9.4607e15;

fn time(tick: u64) -> RailTime {
    RailTime::of_tick(tick, PER_SECOND)
}

fn ulp(value: f64) -> f64 {
    f64::from_bits(value.to_bits() + 1) - value
}

fn period_of(axis: f64, mu: f64) -> f64 {
    TAU * (axis * axis * axis / mu).sqrt()
}

/// The orbit with these ends, round the body of `MU`, at its nearest point at tick 0, in the plane of x and y.
fn orbit_between(periapsis: f64, apoapsis: f64) -> Orbit {
    let axis = (periapsis + apoapsis) / 2.0;
    Orbit::elliptical(
        Centre::Point(NOWHERE),
        [0.0, 0.0, 1.0],
        axis,
        periapsis / axis,
        0.0,
        period_of(axis, MU),
        0.0,
    )
    .unwrap()
}

fn state(orbit: Orbit, tick: u64) -> (ExactOffset, [f64; 3]) {
    let state = Rail {
        spin: None,
        orbit: Some(orbit),
    }
    .state(time(tick), NOWHERE, NOWHERE);
    (state.offset.unwrap(), state.velocity)
}

fn size(vector: [f64; 3]) -> f64 {
    vector.iter().map(|part| part * part).sum::<f64>().sqrt()
}

fn nearest(orbit: Orbit) -> f64 {
    orbit.radius * orbit.ratio
}

fn farthest(orbit: Orbit) -> f64 {
    orbit.radius * (2.0 - orbit.ratio)
}

#[test]
fn a_what_the_elements_can_say() {
    let orbit = orbit_between(7.0e6, LIGHT_YEAR);
    println!("A: semi-major axis {:e} m, period {:e} s ({:.3e} years)", orbit.radius, orbit.period, orbit.period / 3.156e7);
    println!("A: ratio (1 - e) {:e}", orbit.ratio);
    println!("A: spacing of the axis {:e} m, so of the apoapsis about {:e} m", ulp(orbit.radius), 2.0 * ulp(orbit.radius));
    println!("A: spacing of the period {:e} s", ulp(orbit.period));
    println!("A: spacing of the nearest distance (axis x ratio) {:e} m", ulp(orbit.ratio) * orbit.radius);
    println!("A: spacing of the mass parameter the elements say: n^2 a^3 over MU is {:e} out", orbit.mass_parameter() / MU - 1.0);
    let (offset, velocity) = state(orbit, 0);
    println!("A: at tick 0 the place is {:?} m and the speed {:e} m/s; the spacing of that speed is {:e} m/s", offset.in_voxels(), size(velocity), ulp(size(velocity)));
    // How far the apoapsis moves for 1 m/s at the nearest point: da = 2 a^2 v dv / mu, and the apoapsis is 2 a - nearest.
    let v = size(velocity);
    let per_metre_a_second = 2.0 * orbit.radius * orbit.radius * v / MU;
    println!("A: 1 m/s along the speed at the nearest point moves the axis by {per_metre_a_second:e} m (the apoapsis by twice that)");
    println!("A: so one spacing of the axis ({:e} m) is {:e} m/s, and one spacing of the speed ({:e} m/s) is {:e} m of the apoapsis", ulp(orbit.radius), ulp(orbit.radius) / per_metre_a_second, ulp(v), 2.0 * per_metre_a_second * ulp(v));
    // The smallest push per tick that a speed of that size can take at all (half a spacing), as a throttle of the game's 24 m/s^2.
    let least_acceleration = ulp(v) / 2.0 / 0.05;
    println!("A: a push smaller than {:e} m/s^2 ({:e} of the game's 24) a tick is lost entirely in a speed of {:e} m/s", least_acceleration, least_acceleration / 24.0, v);
}

#[test]
fn b_making_the_orbit_again_from_a_state() {
    println!("B: the orbit of A made again from its exact state (`Orbit::from_state`, mass of MU) at each tick: how far each element is, as the apoapsis in metres, the nearest distance in metres, the period in seconds");
    let orbit = orbit_between(7.0e6, LIGHT_YEAR);
    let period_ticks = (orbit.period * f64::from(PER_SECOND)) as u64;
    let ticks: Vec<u64> = vec![
        0,
        1,
        20,
        600,
        12_000,
        200_000,
        2_000_000,
        20_000_000,
        2_000_000_000,
        20_000_000_000_000,
        period_ticks / 4,
        period_ticks / 2,
        period_ticks / 2 + 20,
    ];
    for tick in ticks {
        let (offset, velocity) = state(orbit, tick);
        let place = offset.in_voxels();
        let fitted = Orbit::from_state(Centre::Point(NOWHERE), offset, velocity, MU, time(tick));
        match fitted {
            Ok(found) => println!(
                "B: tick {tick:>20}  r {:>10.4e} m  apoapsis off {:>10.3e} m  nearest off {:>10.3e} m  period off {:>10.3e} s",
                size(place),
                farthest(found) - farthest(orbit),
                nearest(found) - nearest(orbit),
                found.period - orbit.period
            ),
            Err(error) => println!("B: tick {tick:>20}  r {:>10.4e} m  refused: {error}", size(place)),
        }
    }
    // Less extreme ellipses: how the error at the nearest point grows with the length of the ellipse (the ratio of the nearest distance to the axis).
    println!("B: at the nearest point, for ellipses of a nearest distance of 7,000 km and a growing apoapsis:");
    for apoapsis in [1.0e7, 1.0e8, 1.0e9, 1.0e10, 1.0e12, 1.0e14, 1.0e16, LIGHT_YEAR, 1.0e18] {
        let orbit = orbit_between(7.0e6, apoapsis);
        let (offset, velocity) = state(orbit, 0);
        let fitted = Orbit::from_state(Centre::Point(NOWHERE), offset, velocity, MU, time(0)).unwrap();
        println!(
            "B:   apoapsis {apoapsis:>9.3e}  made again: apoapsis off by {:>10.3e} m ({:.2e} of it), nearest off by {:>9.3e} m",
            farthest(fitted) - farthest(orbit),
            (farthest(fitted) / farthest(orbit) - 1.0).abs(),
            nearest(fitted) - nearest(orbit)
        );
    }
}

/// The orbit made from the place of `orbit` at its nearest point and a speed changed along itself by `push`, and what it says of the apoapsis.
fn after_a_push(orbit: Orbit, push: f64) -> Option<f64> {
    let (offset, velocity) = state(orbit, 0);
    let speed = size(velocity);
    let pushed = velocity.map(|part| part * (1.0 + push / speed));
    Orbit::from_state(Centre::Point(NOWHERE), offset, pushed, MU, time(0))
        .ok()
        .map(farthest)
}

#[test]
fn c_a_push_at_the_nearest_point() {
    let orbit = orbit_between(7.0e6, LIGHT_YEAR);
    let (_, velocity) = state(orbit, 0);
    let speed = size(velocity);
    let per = 2.0 * orbit.radius * orbit.radius * speed / MU;
    println!("C: a push along the speed at the nearest point (speed {speed:e} m/s): what the apoapsis becomes, by the f64 state (the way every disturbance makes the orbit today)");
    println!("C: (ideal change of the apoapsis: 2 x {per:e} m per m/s x the push, first order; the push is added to the speed as an f64, which has a spacing of {:e} m/s)", ulp(speed));
    for exponent in 3..=14 {
        let push = 10f64.powi(-exponent);
        let ideal = 2.0 * per * push;
        let realised = after_a_push(orbit, push).map(|apoapsis| apoapsis - farthest(orbit));
        println!(
            "C: push {push:>8.1e} m/s  ideal apoapsis change {ideal:>11.4e} m  by the f64 state {:>11.4e} m",
            realised.unwrap_or(f64::NAN)
        );
    }
    // The way that does not go through a state: Gauss's equation for the axis, da = (2 a^2 v / mu) dv, worked out from the push and added to the axis.
    println!("C: by the elements (the axis changed by Gauss's equation, the push being the one that is asked; the axis and the apoapsis are as an f64 has them):");
    for exponent in [3, 6, 9, 12, 14, 16, 18, 20, 22] {
        let push = 10f64.powi(-exponent);
        let change = per * push;
        let moved = orbit.radius + change;
        println!(
            "C: push {push:>8.1e} m/s  axis change {change:>11.4e} m  the f64 axis says {:>11.4e} m",
            moved - orbit.radius
        );
    }
    // How large a mistake in the mass parameter (the way a rail's period is rounded) is: the same orbit made with a period from the rounded axis.
    println!("C: the apoapsis change that a push of 1 mm/s at the nearest point makes is {:e} m, about {:.1} light years", 2.0 * per * 1.0e-3, 2.0 * per * 1.0e-3 / LIGHT_YEAR);
}

#[test]
fn d_positions_for_the_reference() {
    let orbit = orbit_between(7.0e6, LIGHT_YEAR);
    let period_ticks = (orbit.period * f64::from(PER_SECOND)) as u64;
    println!(
        "D: orbit {:016x} {:016x} {:016x} {:016x} {:016x}",
        orbit.radius.to_bits(),
        orbit.ratio.to_bits(),
        orbit.periapsis.to_bits(),
        orbit.period.to_bits(),
        orbit.phase.to_bits()
    );
    println!("D: normal {:016x} {:016x} {:016x}", orbit.normal[0].to_bits(), orbit.normal[1].to_bits(), orbit.normal[2].to_bits());
    for tick in [0u64, 1, 20, 600, 12_000, 2_000_000, 2_000_000_000, 20_000_000_000_000, period_ticks / 2 - 20, period_ticks / 2, period_ticks / 2 + 20, period_ticks - 20, period_ticks - 1] {
        let (offset, velocity) = state(orbit, tick);
        println!(
            "D: tick {tick} bricks {} {} {} rest {:016x} {:016x} {:016x} velocity {:016x} {:016x} {:016x}",
            offset.bricks[0],
            offset.bricks[1],
            offset.bricks[2],
            offset.rest[0].to_bits(),
            offset.rest[1].to_bits(),
            offset.rest[2].to_bits(),
            velocity[0].to_bits(),
            velocity[1].to_bits(),
            velocity[2].to_bits()
        );
    }
}

/// Where a body goes in `ticks` steps of `step_ticks` ticks, by a kick-drift-kick leapfrog (second order) or a fourth-order composition of three of them (Yoshida), from the
/// exact state at tick 0.
fn integrate(orbit: Orbit, step_ticks: u64, steps: u64, fourth_order: bool) -> ([f64; 3], [f64; 3]) {
    let (offset, velocity) = state(orbit, 0);
    let (mut x, mut v) = (offset.in_voxels(), velocity);
    let step = step_ticks as f64 / f64::from(PER_SECOND);
    let pull = |x: [f64; 3]| {
        let distance = size(x);
        let scale = -MU / (distance * distance * distance);
        x.map(|part| part * scale)
    };
    let leap = |x: &mut [f64; 3], v: &mut [f64; 3], h: f64| {
        let a = pull(*x);
        for axis in 0..3 {
            v[axis] += 0.5 * h * a[axis];
        }
        for axis in 0..3 {
            x[axis] += h * v[axis];
        }
        let a = pull(*x);
        for axis in 0..3 {
            v[axis] += 0.5 * h * a[axis];
        }
    };
    let cube_root_two = 2f64.powf(1.0 / 3.0);
    let (outer, inner) = (1.0 / (2.0 - cube_root_two), -cube_root_two / (2.0 - cube_root_two));
    for _ in 0..steps {
        if fourth_order {
            leap(&mut x, &mut v, outer * step);
            leap(&mut x, &mut v, inner * step);
            leap(&mut x, &mut v, outer * step);
        } else {
            leap(&mut x, &mut v, step);
        }
    }
    (x, v)
}

#[test]
fn e_a_fixed_step_integrator_against_the_rail() {
    // A moderate ellipse (nearest 7,000 km, farthest 14,000 km; a period of 5,800 s, 116,000 ticks), with steps of one tick (a twentieth of a second, the game's tick) and of ten.
    let orbit = orbit_between(7.0e6, 1.4e7);
    println!("E: ellipse of 7,000 to 14,000 km round MU: period {:.1} s = {:.0} ticks", orbit.period, orbit.period * f64::from(PER_SECOND));
    println!("E: error of the place (m) after N steps, against the exact rail, and of the energy (relative to the start's, which is the bound of a symplectic method)");
    for (step_ticks, label) in [(10u64, "step 0.5 s"), (1, "step 0.05 s")] {
        for steps in [100_000u64, 1_000_000, 10_000_000] {
            if step_ticks == 10 && steps == 10_000_000 {
                continue;
            }
            for fourth_order in [false, true] {
                let (x, v) = integrate(orbit, step_ticks, steps, fourth_order);
                let tick = step_ticks * steps;
                let (offset, velocity) = state(orbit, tick);
                let exact = offset.in_voxels();
                let error = size([x[0] - exact[0], x[1] - exact[1], x[2] - exact[2]]);
                let energy = |x: [f64; 3], v: [f64; 3]| 0.5 * size(v) * size(v) - MU / size(x);
                let first = {
                    let (offset, velocity) = state(orbit, 0);
                    energy(offset.in_voxels(), velocity)
                };
                println!(
                    "E: {label} {steps:>10} steps ({:>7.1} laps) {}: place off {:>10.3e} m  energy off {:>9.2e}  (speed off {:.2e} m/s)",
                    tick as f64 / (orbit.period * f64::from(PER_SECOND)),
                    if fourth_order { "fourth order" } else { "second order" },
                    error,
                    (energy(x, v) - first) / first.abs(),
                    size([v[0] - velocity[0], v[1] - velocity[1], v[2] - velocity[2]])
                );
            }
        }
    }
    // A thin ellipse, where a fixed step is worst: nearest 7,000 km, farthest 7e9 m (e 0.9990): the step near the nearest point is the same as far away.
    let thin = orbit_between(7.0e6, 7.0e9);
    println!("E: thin ellipse 7,000 km to 7e6 km: period {:.0} s", thin.period);
    for step_ticks in [1u64, 10] {
        for fourth_order in [false, true] {
            let steps = (2.0 * thin.period * f64::from(PER_SECOND) / step_ticks as f64) as u64;
            if steps > 300_000_000 {
                continue;
            }
            let (x, _) = integrate(thin, step_ticks, steps, fourth_order);
            let tick = step_ticks * steps;
            let exact = state(thin, tick).0.in_voxels();
            println!(
                "E: thin, step {} ticks, two laps ({steps} steps) {}: place off {:.3e} m",
                step_ticks,
                if fourth_order { "fourth order" } else { "second order" },
                size([x[0] - exact[0], x[1] - exact[1], x[2] - exact[2]])
            );
        }
    }
}
