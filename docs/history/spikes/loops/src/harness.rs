//! The measurement loop shared by the probes: push a rig for one second, then watch what must not change.

use crate::*;

#[derive(Debug, Default)]
pub struct Report {
    pub max_gap: f64,
    pub final_gap: f64,
    /// Largest change of the centre's velocity after the push, m/s (nothing pushes, so it must be 0).
    pub max_velocity_drift: f64,
    /// The same measured on the multibody's generalized velocities (what it integrates) instead of the velocities its bodies hold; equal to the first for impulse joints.
    pub max_velocity_drift_generalized: f64,
    /// Largest distance of the centre from the straight line it should follow, m.
    pub max_centre_off_line: f64,
    /// The same against the line that the generalized velocities of the push's end give (the rail the hold follows).
    pub max_centre_off_generalized_line: f64,
    /// Largest change of angular momentum after the push, as a fraction of its size.
    pub max_angular_drift: f64,
    /// The same on the generalized velocities.
    pub max_angular_drift_generalized: f64,
    /// Largest change of the angular momentum about the rig's centre (generalized velocities), as a fraction of its size after the push.
    pub max_spin_drift: f64,
    /// Largest and final change of kinetic energy after the push, as a fraction of it.
    pub max_energy_drift: f64,
    pub final_energy_drift: f64,
    pub energy_after_push: f64,
    /// How far the ring flexed (largest minus smallest distance between two opposite cars), m.
    pub flex: f64,
    pub micros_per_tick: f64,
    pub direct_micros_per_tick: f64,
    pub failures: usize,
    pub not_finite: bool,
}

#[derive(Clone, Copy)]
pub struct Case {
    pub mode: Mode,
    pub ring: usize,
    pub tail: usize,
    pub ring_mass: f64,
    pub tail_mass: f64,
    pub settings: Settings,
    pub seconds: f64,
    pub iterations: usize,
    pub exact_mass: bool,
    pub mismatch: Vector,
    /// Hold the rig to the rail it has when the push ends (see `hold_to_rail`).
    pub recentre: RecentreMode,
    /// A constant outside force on the second car after the push (the rail's acceleration is this over the mass); zero for a coasting rig.
    pub thrust: Vector,
    /// Where the rig is, from the origin (the numbers get bigger, the rig does not change).
    pub offset: Vector,
    /// Keep the state in 32 bits (see `World::quantize`).
    pub quantize: bool,
}

impl Case {
    pub fn new(mode: Mode) -> Self {
        Self { mode, ring: 4, tail: 0, ring_mass: 1000.0, tail_mass: 1000.0, settings: Settings::default(), seconds: 100.0, iterations: 2, exact_mass: false, mismatch: Vector::ZERO, recentre: RecentreMode::Off, thrust: Vector::ZERO, offset: Vector::ZERO, quantize: false }
    }
}

pub fn run(case: Case) -> Report {
    let mut world = World::new(case.settings, Vector::ZERO);
    world.direct_iterations = case.iterations;
    world.direct_exact_mass = case.exact_mass;
    world.closing_mismatch = case.mismatch;
    let rig = build_ring(&mut world, case.mode, case.ring, case.tail, case.ring_mass, case.tail_mass);
    if case.offset != Vector::ZERO {
        for (_, body) in world.bodies.iter_mut() {
            let place = body.translation() + case.offset;
            body.set_translation(place, false);
        }
    }
    world.quantize = case.quantize;
    start_moving(&mut world, &rig.cars, Vector::new(0.4, 0.2, 0.1), Vector::new(0.15, 0.1, 0.45));
    let ticks = (case.seconds / case.settings.tick_length).round() as usize;
    let push_ticks = (1.0 / case.settings.tick_length).round() as usize;
    let opposite = rig.cars[rig.ring / 2].handle;
    let mut report = Report::default();
    let (mut after, mut flex_low, mut flex_high) = (None::<Totals>, f64::MAX, 0.0f64);
    let mut after_generalized = None::<Totals>;
    let mut spin_after = None::<Vector>;
    let mut timed_ticks = 0usize;
    for tick in 0..ticks {
        if tick >= push_ticks && case.thrust != Vector::ZERO {
            world.bodies[rig.cars[1].handle].add_force(case.thrust, true);
        }
        if tick < push_ticks {
            let scale = case.ring_mass / 1000.0;
            world.bodies[rig.cars[1].handle].add_force(Vector::new(0.0, 0.0, 300.0 * scale), true);
            world.bodies[rig.cars[2 % rig.ring].handle].add_torque(Vector::new(0.0, 200.0 * scale, 0.0), true);
        }
        world.tick();
        let t = totals(&world, &rig.cars);
        if !t.energy.is_finite() || !t.momentum.is_finite() {
            report.not_finite = true;
            break;
        }
        report.max_gap = report.max_gap.max(rig.pivots.iter().map(|p| gap(&world, p)).fold(0.0, f64::max));
        if tick + 1 == push_ticks {
            after = Some(t);
            after_generalized = Some(totals_from_generalized(&world, &rig.cars).unwrap_or(t));
            spin_after = Some(rig_spin(&world, &rig.cars).0);
            if case.recentre != RecentreMode::Off {
                world.recentre = Some(Recentre { mode: case.recentre, momentum: after_generalized.unwrap().momentum, centre: t.centre, elapsed: 0.0, force: case.thrust, spin: spin_after.unwrap() });
            }
            // Time only what comes after the push.
            world.step_time = Default::default();
            world.direct_time = Default::default();
            timed_ticks = 0;
        }
        if let Some(start) = after {
            if tick + 1 > push_ticks {
                timed_ticks += 1;
                let elapsed = (tick + 1 - push_ticks) as f64 * case.settings.tick_length;
                let push = case.thrust * elapsed;
                let bend = case.thrust / t.mass * (0.5 * elapsed * elapsed);
                report.max_velocity_drift = report.max_velocity_drift.max((t.momentum - start.momentum - push).length() / t.mass);
                let g = totals_from_generalized(&world, &rig.cars).unwrap_or(t);
                let g_start = after_generalized.unwrap_or(start);
                report.max_velocity_drift_generalized = report.max_velocity_drift_generalized.max((g.momentum - g_start.momentum - push).length() / g.mass);
                let line = start.centre + start.momentum / start.mass * elapsed + bend;
                report.max_centre_off_line = report.max_centre_off_line.max((t.centre - line).length());
                let g_line = start.centre + g_start.momentum / start.mass * elapsed + bend;
                report.max_centre_off_generalized_line = report.max_centre_off_generalized_line.max((t.centre - g_line).length());
                if let Some(spin_start) = spin_after {
                    report.max_spin_drift = report.max_spin_drift.max((rig_spin(&world, &rig.cars).0 - spin_start).length() / spin_start.length());
                }
                report.max_angular_drift_generalized = report.max_angular_drift_generalized.max((g.angular_momentum - g_start.angular_momentum).length() / g_start.angular_momentum.length());
                report.max_angular_drift = report.max_angular_drift.max((t.angular_momentum - start.angular_momentum).length() / start.angular_momentum.length());
                let drift = (t.energy - start.energy) / start.energy;
                if drift.abs() > report.max_energy_drift.abs() {
                    report.max_energy_drift = drift;
                }
                report.final_energy_drift = drift;
                let span = (world.bodies[opposite].translation() - world.bodies[rig.cars[0].handle].translation()).length();
                flex_low = flex_low.min(span);
                flex_high = flex_high.max(span);
            }
        }
    }
    report.final_gap = rig.pivots.iter().map(|p| gap(&world, p)).fold(0.0, f64::max);
    report.energy_after_push = after.map_or(0.0, |t| t.energy);
    report.flex = flex_high - flex_low;
    let timed = timed_ticks.max(1) as f64;
    report.micros_per_tick = (world.step_time + world.direct_time).as_secs_f64() * 1e6 / timed;
    report.direct_micros_per_tick = world.direct_time.as_secs_f64() * 1e6 / timed;
    report.failures = world.direct_failures;
    report
}

