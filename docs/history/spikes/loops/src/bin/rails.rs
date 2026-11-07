//! A ship on a rail: the rail is the centre of mass, so the cars' total momentum, the centre and the spin about it must stay exactly as the push left
//! them. Does taking their errors from every car after each step (one rigid shift of the whole rig, so no joint is touched) remove the multibody's
//! momentum drift, and does it harm anything else? Run: `cargo run --release --bin rails -- [ring|tail|thrust|impulse|all]`.
//!
//! Columns: *dv bodies* / *dv general.* = largest change of the centre's velocity, from the velocities the bodies hold / from the generalized
//! velocities the multibody integrates (the first lags by about acceleration x step); *off (body)* / *off (gen.)* = the centre's distance from the
//! line it should follow, the line made from the first / the second; *spin* = angular momentum about the centre, relative change (generalized);
//! *dL/L* = angular momentum about the origin (bodies, as in `loop-probe.md`); *dE/E* = kinetic energy.

use loop_probe::harness::*;
use loop_probe::*;

fn header() {
    println!(
        "{:<58} {:>9} {:>10} {:>10} {:>10} {:>10} {:>9} {:>9} {:>10} {:>7}",
        "case", "max gap", "dv bodies", "dv general", "off (body)", "off (gen.)", "spin", "dL/L", "dE/E", "us/tick"
    );
}

fn row(label: &str, r: &Report) {
    if r.not_finite {
        println!("{label:<58} NOT FINITE");
        return;
    }
    println!(
        "{label:<58} {:>9.1e} {:>10.2e} {:>10.2e} {:>10.2e} {:>10.2e} {:>9.2e} {:>9.2e} {:>+10.2e} {:>7.0}",
        r.max_gap,
        r.max_velocity_drift,
        r.max_velocity_drift_generalized,
        r.max_centre_off_line,
        r.max_centre_off_generalized_line,
        r.max_spin_drift,
        r.max_angular_drift,
        r.max_energy_drift,
        r.micros_per_tick
    );
}

const HOLDS: [(RecentreMode, &str); 4] = [
    (RecentreMode::Off, "free"),
    (RecentreMode::Velocity, "velocity held"),
    (RecentreMode::VelocityAndPosition, "velocity + centre held"),
    (RecentreMode::VelocityPositionAndSpin, "velocity + centre + spin held"),
];

fn batch(title: &str, modes: &[(Mode, &str)], ring: usize, tail: usize, seconds: f64, settings: Settings, thrust: Vector, with_spin: bool) {
    println!("\n{title}");
    header();
    for (mode, name) in modes {
        for (hold, hold_name) in HOLDS {
            if hold == RecentreMode::VelocityPositionAndSpin && !with_spin {
                continue;
            }
            let mut case = Case::new(*mode);
            case.ring = ring;
            case.tail = tail;
            case.seconds = seconds;
            case.settings = settings;
            case.recentre = hold;
            case.thrust = thrust;
            row(&format!("{name}, {hold_name}"), &run(case));
        }
    }
}

fn main() {
    let which = std::env::args().nth(1).unwrap_or_else(|| "all".to_string());
    let all = which == "all";
    let game = Settings::default();
    let multibody = [(Mode::TreeOnly, "multibody open chain"), (Mode::MultibodyNative, "multibody + impulse closing")];
    if which == "ring" || all {
        batch("ring of 4, 40 s, game step (4.2 ms)", &multibody, 4, 0, 40.0, game, Vector::ZERO, true);
    }
    if which == "tail" || all {
        batch("ring of 4 + chain of 12 (16 cars), 100 s, game step", &multibody, 4, 12, 100.0, game, Vector::ZERO, true);
        batch(
            "ring of 4 + chain of 12, 100 s, coarse step (16.7 ms, 1 substep)",
            &multibody,
            4,
            12,
            100.0,
            Settings { substeps: 1, ..game },
            Vector::ZERO,
            true,
        );
    }
    if which == "thrust" || all {
        batch(
            "ring of 4 + chain of 12, 100 s, game step, 500 N thrust on one car after the push (the rail accelerates; spin, dL/L and dE/E mean nothing)",
            &multibody,
            4,
            12,
            100.0,
            game,
            Vector::new(0.0, 500.0, 0.0),
            false,
        );
    }
    if which == "impulse" || all {
        batch(
            "control: all impulse joints, ring of 4 + chain of 12, 100 s",
            &[(Mode::AllImpulse, "all impulse")],
            4,
            12,
            100.0,
            game,
            Vector::ZERO,
            true,
        );
    }
}
