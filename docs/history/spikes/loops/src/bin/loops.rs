//! Does a loop of jointed cars gain energy or momentum from nothing? A ring of cars (with an optional chain hung off it) is pushed for one second and
//! then left alone in empty space. From then on nothing pushes it, so its linear momentum, angular momentum and kinetic energy must not change.
//! Run: `cargo run --release --bin loops -- [a|b|c|d|all]`.

use loop_probe::harness::*;
use loop_probe::*;

fn print_header() {
    println!(
        "{:<44} {:>10} {:>10} {:>11} {:>10} {:>10} {:>10} {:>9} {:>8} {:>9}",
        "case", "max gap m", "end gap m", "dv centre", "centre off", "dL/L max", "dE/E max", "dE/E end", "flex m", "us/tick"
    );
}

fn print_row(label: &str, r: &Report) {
    if r.not_finite {
        println!("{label:<44} NOT FINITE");
        return;
    }
    println!(
        "{label:<44} {:>10.2e} {:>10.2e} {:>11.2e} {:>10.2e} {:>10.2e} {:>+10.2e} {:>+9.2e} {:>8.3} {:>9.1}{}",
        r.max_gap,
        r.final_gap,
        r.max_velocity_drift,
        r.max_centre_off_line,
        r.max_angular_drift,
        r.max_energy_drift,
        r.final_energy_drift,
        r.flex,
        r.micros_per_tick,
        if r.failures > 0 { format!("  ({} failed solves)", r.failures) } else { String::new() }
    );
}

const MODES: [(Mode, &str); 4] = [
    (Mode::TreeOnly, "open chain (no loop)"),
    (Mode::AllImpulse, "all impulse joints"),
    (Mode::MultibodyNative, "multibody + impulse closing joint"),
    (Mode::MultibodyDirect, "multibody + direct closing solve"),
];

fn settings_variants() -> Vec<(&'static str, Settings)> {
    let game = Settings::default();
    vec![
        ("game (3 steps x 4 substeps, 4.2 ms)", game),
        ("3 steps x 1 substep (16.7 ms)", Settings { substeps: 1, ..game }),
        ("12 steps x 1 substep (4.2 ms)", Settings { steps_per_tick: 12, substeps: 1, ..game }),
        ("12 steps x 4 substeps (1 ms)", Settings { steps_per_tick: 12, substeps: 4, ..game }),
        ("3 steps x 4 substeps, pgs 4", Settings { pgs_iterations: 4, ..game }),
    ]
}

fn main() {
    let which = std::env::args().nth(1).unwrap_or_else(|| "a".to_string());
    let all = which == "all";
    if which == "check" {
        check();
        return;
    }
    if which == "explode" {
        explode();
        return;
    }
    if which == "e" {
        convergence();
        return;
    }
    if which == "mass" {
        mass();
        return;
    }
    if which == "why" {
        why();
        return;
    }
    if which == "sweep" {
        sweep();
        return;
    }
    if which == "a" || all {
        println!("== A. Ring of 4 cars (1000 kg each), pushed for 1 s, then free for 100 s. Rates are after the push. ==");
        print_header();
        for (name, settings) in settings_variants() {
            for (mode, mode_name) in MODES {
                let mut case = Case::new(mode);
                case.settings = settings;
                print_row(&format!("{mode_name} | {name}"), &run(case));
            }
        }
    }
    if which == "b" || all {
        println!("== B. A ship-like rig: ring of 4 plus a chain of 12 cars. ==");
        print_header();
        for (name, settings) in settings_variants().into_iter().take(2) {
            for (mode, mode_name) in MODES {
                let mut case = Case::new(mode);
                case.settings = settings;
                case.tail = 12;
                print_row(&format!("{mode_name} | {name}"), &run(case));
            }
        }
    }
    if which == "c" || all {
        println!("== C. Mass ratios (ring 4 + chain 4): light ring, heavy chain (1 kg : 10^6 kg) and the reverse. ==");
        print_header();
        for (ring_mass, tail_mass, label) in [(1.0, 1.0e6, "ring 1 kg, chain 1e6 kg"), (1.0e6, 1.0, "ring 1e6 kg, chain 1 kg")] {
            for (mode, mode_name) in MODES {
                let mut case = Case::new(mode);
                case.tail = 4;
                case.ring_mass = ring_mass;
                case.tail_mass = tail_mass;
                case.seconds = 50.0;
                print_row(&format!("{mode_name} | {label}"), &run(case));
            }
        }
    }
    if which == "d" || all {
        println!("== D. Cost: microseconds per 50 ms tick (3 steps), ring of 4 plus a chain of n cars, 10 s after the push. ==");
        println!("{:<8} {:>14} {:>14} {:>18} {:>18} {:>14}", "chain", "open chain", "all impulse", "multibody+impulse", "multibody+direct", "(direct part)");
        for tail in [0usize, 8, 24, 56, 120] {
            let mut cells = Vec::new();
            let mut direct_part = 0.0;
            for (mode, _) in MODES {
                let mut case = Case::new(mode);
                case.tail = tail;
                case.seconds = 11.0;
                let r = run(case);
                if mode == Mode::MultibodyDirect {
                    direct_part = r.direct_micros_per_tick;
                }
                cells.push(r.micros_per_tick);
            }
            println!("{tail:<8} {:>14.1} {:>14.1} {:>18.1} {:>18.1} {:>14.1}", cells[0], cells[1], cells[2], cells[3], direct_part);
        }
    }
}

/// Checks the tools before the measurements are trusted: do the bodies hold the velocities the multibody has, does the start give the momentum asked for?
fn check() {
    for mode in [Mode::TreeOnly, Mode::MultibodyNative] {
        let mut world = World::new(Settings::default(), Vector::ZERO);
        let rig = build_ring(&mut world, mode, 4, 3, 1000.0, 1000.0);
        let before = totals(&world, &rig.cars);
        start_moving(&mut world, &rig.cars, Vector::new(0.4, 0.2, 0.1), Vector::new(0.15, 0.1, 0.45));
        let started = totals(&world, &rig.cars);
        println!("{mode:?}: mass {} centre {:?} | asked momentum/mass {:?}, bodies hold {:?}", before.mass, before.centre, Vector::new(0.4, 0.2, 0.1), started.momentum / started.mass);
        for tick in 0..3 {
            world.tick();
            let held = totals(&world, &rig.cars);
            let general = totals_from_generalized(&world, &rig.cars);
            println!("  tick {tick}: bodies P/m {:?} E {:.6}", held.momentum / held.mass, held.energy);
            if let Some(g) = general {
                println!("          J q' P/m {:?} E {:.6}", g.momentum / g.mass, g.energy);
            }
        }
    }
}

/// Does a ring at rest, with nothing pushing it, stay at rest? (It must.) Prints the fastest body and the worst gap after each of a few ticks.
fn explode() {
    for mode in [Mode::AllImpulse, Mode::MultibodyNative] {
        for (ring, tail) in [(4usize, 0usize), (4, 3), (5, 0), (6, 0), (8, 0)] {
            for settings in [Settings::default(), Settings { steps_per_tick: 12, substeps: 1, ..Settings::default() }] {
                let mut world = World::new(settings, Vector::ZERO);
                let rig = build_ring(&mut world, mode, ring, tail, 1000.0, 1000.0);
                let initial_gap = rig.pivots.iter().map(|p| gap(&world, p)).fold(0.0, f64::max);
                let mut line = format!("{mode:?} ring {ring} tail {tail} steps {} substeps {} | initial gap {initial_gap:.1e} | fastest after ticks:", settings.steps_per_tick, settings.substeps);
                for _ in 0..4 {
                    world.tick();
                    let fastest = rig.cars.iter().map(|c| world.bodies[c.handle].linvel().length().max(world.bodies[c.handle].angvel().length())).fold(0.0, f64::max);
                    line += &format!(" {fastest:.1e}");
                }
                println!("{line}");
            }
        }
    }
}

/// Which rigs at rest blow up? The fastest body after 6 ticks, for rings of 3 to 10 cars and chains of 0 to 3 (multibody + impulse closing joint).
fn sweep() {
    println!("fastest body after 6 ticks at rest (game settings); 'BOOM' above 1e-3");
    print!("{:<10}", "ring\tail");
    for tail in 0..4 {
        print!("{tail:>12}");
    }
    println!();
    for ring in 3..=10usize {
        print!("{ring:<10}");
        for tail in 0..4usize {
            let mut world = World::new(Settings::default(), Vector::ZERO);
            let rig = build_ring(&mut world, Mode::MultibodyNative, ring, tail, 1000.0, 1000.0);
            for _ in 0..6 {
                world.tick();
            }
            let fastest = rig.cars.iter().map(|c| world.bodies[c.handle].linvel().length().max(world.bodies[c.handle].angvel().length())).fold(0.0, f64::max);
            print!("{:>12}", if fastest > 1e-3 || !fastest.is_finite() { format!("BOOM {fastest:.0e}") } else { format!("{fastest:.0e}") });
        }
        println!();
    }
}

/// What does the blow-up of a ring of 6 at rest depend on? The fastest body after each of the first 12 steps, then after 6 ticks.
fn why() {
    let game = Settings::default();
    let variants: Vec<(&str, Settings, Option<SpringCoefficients<Real>>)> = vec![
        ("game settings", game, None),
        ("steps 60 (dt 0.8 ms), 1 substep", Settings { steps_per_tick: 60, substeps: 1, ..game }, None),
        ("steps 3, 16 substeps", Settings { substeps: 16, ..game }, None),
        ("pgs 8", Settings { pgs_iterations: 8, ..game }, None),
        ("closing joint softness 1e3 Hz", game, Some(SpringCoefficients::new(1.0e3, 1.0))),
        ("closing joint softness 30 Hz", game, Some(SpringCoefficients::new(30.0, 1.0))),
        ("closing joint softness 30 Hz, damping 5", game, Some(SpringCoefficients::new(30.0, 5.0))),
    ];
    for (name, settings, softness) in variants {
        let mut world = World::new(settings, Vector::ZERO);
        world.closing_softness = softness;
        let rig = build_ring(&mut world, Mode::MultibodyNative, 6, 0, 1000.0, 1000.0);
        let fastest = |world: &World| rig.cars.iter().map(|c| world.bodies[c.handle].linvel().length().max(world.bodies[c.handle].angvel().length())).fold(0.0, f64::max);
        let mut per_step = String::new();
        for _ in 0..12 {
            world.step();
            per_step += &format!(" {:.0e}", fastest(&world));
        }
        for _ in 0..6 {
            world.tick();
        }
        println!("{name:<42} steps:{per_step} | after 6 more ticks {:.1e}", fastest(&world));
    }
}

/// The effective mass of the closing constraint of a ring of 6 at rest, Rapier's and the true one.
fn mass() {
    for ring in [4usize, 6] {
        let mut world = World::new(Settings::default(), Vector::ZERO);
        let rig = build_ring(&mut world, Mode::MultibodyNative, ring, 0, 1000.0, 1000.0);
        world.step();
        let (rapier, truth) = effective_mass(&mut world, &rig.cars, &rig.closing).expect("one multibody");
        let rows = closing_rows(&mut world, &rig.closing).unwrap();
        println!("ring {ring}: jacobian of the closing constraint ({} generalized velocities):", rows[0].len());
        for row in &rows {
            println!("   {}", row.iter().map(|v| format!("{v:>7.3}")).collect::<Vec<_>>().join(" "));
        }
        let singular = DMatrix::from_row_slice(3, rows[0].len(), &rows.concat()).svd(false, false).singular_values;
        println!("   singular values of the jacobian: {:?}", singular.as_slice());
        println!("ring {ring}: effective mass of the closing constraint (kg), Rapier's factorization:");
        for row in rapier {
            println!("   {:>14.6e} {:>14.6e} {:>14.6e}", row[0], row[1], row[2]);
        }
        println!("   from the true mass matrix:");
        for row in truth {
            println!("   {:>14.6e} {:>14.6e} {:>14.6e}", row[0], row[1], row[2]);
        }
    }
}

/// How do the errors of a free, open chain of 4 cars shrink with the step? Rapier's multibody and impulse joints, from steps of 16.7 ms down to 0.26 ms.
/// The chain is pushed for 1 s and then left alone for 19 s. The position of the last car at 10 s is compared with that of a reference run
/// (impulse joints, steps of 0.13 ms).
fn convergence() {
    let game = Settings::default();
    let probe = |mode: Mode, settings: Settings, seconds: f64| -> (Report, Vector) {
        let mut world = World::new(settings, Vector::ZERO);
        let rig = build_ring(&mut world, mode, 4, 0, 1000.0, 1000.0);
        start_moving(&mut world, &rig.cars, Vector::new(0.4, 0.2, 0.1), Vector::new(0.15, 0.1, 0.45));
        let ticks = (seconds / settings.tick_length).round() as usize;
        let push = (1.0 / settings.tick_length).round() as usize;
        let mut report = Report::default();
        let mut after = None;
        let mut at_ten = Vector::ZERO;
        for tick in 0..ticks {
            if tick < push {
                world.bodies[rig.cars[1].handle].add_force(Vector::new(0.0, 0.0, 300.0), true);
                world.bodies[rig.cars[2].handle].add_torque(Vector::new(0.0, 200.0, 0.0), true);
            }
            world.tick();
            let t = totals(&world, &rig.cars);
            if tick + 1 == push {
                after = Some(t);
            }
            if tick + 1 == (10.0 / settings.tick_length).round() as usize {
                at_ten = world.bodies[rig.cars[3].handle].translation();
            }
            if let Some(start) = after {
                if tick + 1 > push {
                    let elapsed = (tick + 1 - push) as f64 * settings.tick_length;
                    report.max_velocity_drift = report.max_velocity_drift.max((t.momentum - start.momentum).length() / t.mass);
                    let line = start.centre + start.momentum / start.mass * elapsed;
                    report.max_centre_off_line = report.max_centre_off_line.max((t.centre - line).length());
                    report.max_angular_drift = report.max_angular_drift.max((t.angular_momentum - start.angular_momentum).length() / start.angular_momentum.length());
                    report.final_energy_drift = (t.energy - start.energy) / start.energy;
                }
            }
        }
        (report, at_ten)
    };
    // Settings with a step of dt = 50 ms / (steps x substeps).
    let with_dt = |divisions: usize| Settings { steps_per_tick: divisions, substeps: 1, ..game };
    let (_, reference) = probe(Mode::ImpulseOpen, with_dt(384), 11.0);
    let (_, reference_multibody) = probe(Mode::TreeOnly, with_dt(384), 11.0);
    println!("reference (impulse, 0.13 ms) vs multibody at 0.13 ms, last car at 10 s: differ by {:.2e} m", (reference - reference_multibody).length());
    println!("{:<12} {:<12} {:>12} {:>12} {:>12} {:>12} {:>16}", "step", "joints", "dv centre", "centre off", "dL/L", "dE/E end", "last car vs ref");
    for divisions in [3usize, 6, 12, 24, 48, 96] {
        for (mode, name) in [(Mode::TreeOnly, "multibody"), (Mode::ImpulseOpen, "impulse")] {
            let (r, at_ten) = probe(mode, with_dt(divisions), 20.0);
            println!(
                "{:<12} {:<12} {:>12.2e} {:>12.2e} {:>12.2e} {:>+12.2e} {:>16.2e}",
                format!("{:.2} ms", 50.0 / divisions as f64),
                name,
                r.max_velocity_drift,
                r.max_centre_off_line,
                r.max_angular_drift,
                r.final_energy_drift,
                (at_ten - reference).length()
            );
        }
    }
}
