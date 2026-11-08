//! What limits the accuracy of impulse joints: the work the solver does (substeps, passes), or the numbers (64-bit; the same state rounded to 32 bits; a rig far from
//! the origin, where the numbers are coarser)? A ring of 4 cars with a chain of 12, every joint an ordinary impulse joint, pushed for a second and then left alone
//! (the harness of `rails.rs`). Run: `cargo run --release --bin accuracy -- [work|numbers|all]`.
//!
//! Columns: *gap* = the worst distance between the two anchors of any joint, *final gap* = at the end; *dv* = the largest change of the centre's velocity after the
//! push (nothing pushes, so 0); *dE/E* = kinetic energy, the worst and the final change.

use loop_probe::harness::*;
use loop_probe::*;

fn header() {
    println!("{:<50} {:>10} {:>10} {:>10} {:>11} {:>11} {:>8}", "case", "max gap", "final gap", "dv", "dE/E worst", "dE/E final", "us/tick");
}

fn row(label: &str, r: &Report) {
    if r.not_finite {
        println!("{label:<50} NOT FINITE");
        return;
    }
    println!(
        "{label:<50} {:>10.2e} {:>10.2e} {:>10.2e} {:>+11.2e} {:>+11.2e} {:>8.0}",
        r.max_gap, r.final_gap, r.max_velocity_drift, r.max_energy_drift, r.final_energy_drift, r.micros_per_tick
    );
}

fn case(settings: Settings) -> Case {
    let mut case = Case::new(Mode::AllImpulse);
    case.ring = 4;
    case.tail = 12;
    case.seconds = 40.0;
    case.settings = settings;
    case
}

fn main() {
    let which = std::env::args().nth(1).unwrap_or_else(|| "all".to_string());
    let all = which == "all";
    let game = Settings::default();
    if which == "work" || all {
        println!("\nall impulse joints, ring of 4 + chain of 12, 40 s: more substeps (Rapier's num_solver_iterations; the step is cut into that many)");
        header();
        for substeps in [1, 2, 4, 8, 16, 32] {
            row(&format!("{substeps} substeps (game: 4), 1 pass"), &run(case(Settings { substeps, ..game })));
        }
        println!("\nmore passes in each substep (num_internal_pgs_iterations), 4 substeps");
        header();
        for pgs_iterations in [1, 2, 4, 8, 16] {
            row(&format!("{pgs_iterations} pass(es) (game: 1)"), &run(case(Settings { pgs_iterations, ..game })));
        }
        println!("\nboth: 16 substeps, 8 passes; and the smaller tick of 3 x 4 steps");
        header();
        row("16 substeps, 8 passes", &run(case(Settings { substeps: 16, pgs_iterations: 8, ..game })));
        row("12 steps a tick, 4 substeps, 1 pass", &run(case(Settings { steps_per_tick: 12, ..game })));
    }
    if which == "numbers" || all {
        println!("\nthe same rig, game settings: the numbers. 'state in 32 bits' rounds every pose and velocity to 32 bits after each step (the arithmetic stays 64-bit)");
        header();
        let far = Vector::new(4000.0, -3000.0, 3500.0);
        for (name, offset, quantize) in [
            ("at the origin, 64-bit", Vector::ZERO, false),
            ("at the origin, state in 32 bits", Vector::ZERO, true),
            ("4 km out, 64-bit", far, false),
            ("4 km out, state in 32 bits", far, true),
        ] {
            let mut c = case(game);
            c.offset = offset;
            c.quantize = quantize;
            row(name, &run(c));
        }
        println!("\nand with the solver given a lot of work (16 substeps, 8 passes), so that its own error is small");
        header();
        for (name, offset, quantize) in [
            ("at the origin, 64-bit", Vector::ZERO, false),
            ("at the origin, state in 32 bits", Vector::ZERO, true),
            ("4 km out, 64-bit", far, false),
            ("4 km out, state in 32 bits", far, true),
        ] {
            let mut c = case(Settings { substeps: 16, pgs_iterations: 8, ..game });
            c.offset = offset;
            c.quantize = quantize;
            row(name, &run(c));
        }
    }
}
