//! Rapier 0.36 runs a *simplified* friction model by default (one Coulomb constraint per group of four contacts plus a twist constraint), not one
//! Coulomb constraint per contact. Where do the two differ from each other and from the continuum answer? A square plate spun on the floor, and
//! slid on the floor. Run: `cargo run --release --bin models`.

use loop_probe::*;

fn world_with(model: FrictionModel, plate_half: Vector, mass: f64, friction: f64) -> (World, RigidBodyHandle) {
    let mut world = World::new(Settings::default(), Vector::new(0.0, -9.81, 0.0));
    world.parameters.friction_model = model;
    world.colliders.insert(ColliderBuilder::cuboid(100.0, 0.5, 100.0).translation(Vector::new(0.0, -0.5, 0.0)).friction(friction));
    let handle = world.bodies.insert(RigidBodyBuilder::dynamic().translation(Vector::new(0.0, plate_half.y, 0.0)).can_sleep(false));
    let volume = 8.0 * plate_half.x * plate_half.y * plate_half.z;
    world.colliders.insert_with_parent(ColliderBuilder::cuboid(plate_half.x, plate_half.y, plate_half.z).density(mass / volume).friction(friction), handle, &mut world.bodies);
    (world, handle)
}

/// Seconds a plate takes to lose a given share of its spin, and the mean deceleration over that time.
fn spin_down(model: FrictionModel, side: f64, thickness: f64) -> (f64, f64) {
    let (mut world, handle) = world_with(model, Vector::new(side / 2.0, thickness / 2.0, side / 2.0), 100.0, 0.6);
    for _ in 0..120 {
        world.step();
    }
    let omega0 = 3.0;
    world.bodies[handle].set_angvel(Vector::new(0.0, omega0, 0.0), true);
    let dt = world.parameters.dt;
    let target = 0.5 * omega0;
    let mut time = 0.0;
    while world.bodies[handle].angvel().y > target && time < 10.0 {
        world.step();
        time += dt;
    }
    (time, (omega0 - world.bodies[handle].angvel().y) / time)
}

/// The deceleration of a plate sliding straight, measured over the first half second.
fn slide(model: FrictionModel, pgs_iterations: usize) -> f64 {
    let (mut world, handle) = world_with(model, Vector::new(1.0, 0.1, 1.0), 100.0, 0.6);
    world.parameters.num_internal_pgs_iterations = pgs_iterations;
    for _ in 0..120 {
        world.step();
    }
    world.bodies[handle].set_linvel(Vector::new(8.0, 0.0, 0.0), true);
    let dt = world.parameters.dt;
    let steps = (0.5 / dt).round() as usize;
    for _ in 0..steps {
        world.step();
    }
    (8.0 - world.bodies[handle].linvel().x) / (steps as f64 * dt)
}

/// How far above (positive) or into (negative) the floor a plate settles: the "skin" a resting contact leaves, by mass and by step.
fn resting_offset(mass: f64, settings: Settings) -> f64 {
    let (mut world, handle) = world_with(FrictionModel::Simplified, Vector::new(1.0, 0.1, 1.0), mass, 0.6);
    world.parameters.dt = settings.tick_length / settings.steps_per_tick as f64;
    world.parameters.num_solver_iterations = settings.substeps;
    for _ in 0..(8.0 / world.parameters.dt) as usize {
        world.step();
    }
    world.bodies[handle].translation().y - 0.1
}

fn main() {
    println!("where a 2 x 2 x 0.2 m plate rests on the floor, after 8 s (positive: a gap above the floor, negative: sunk into it), millimetres:");
    let game = Settings::default();
    for mass in [1.0, 100.0, 1.0e4, 1.0e6] {
        println!(
            "  {mass:>9.0} kg: game step (3 x 4 substeps) {:+.3}, 12 x 4 substeps {:+.3}, 3 x 16 substeps {:+.3}",
            resting_offset(mass, game) * 1e3,
            resting_offset(mass, Settings { steps_per_tick: 12, ..game }) * 1e3,
            resting_offset(mass, Settings { substeps: 16, ..game }) * 1e3
        );
    }
    println!();
    let g = 9.81;
    let mu = 0.6;
    println!("straight slide, plate 2 x 2 m: expected deceleration mu g = {:.3} m/s^2", mu * g);
    for (name, model) in [("simplified (default)", FrictionModel::Simplified), ("coulomb per contact", FrictionModel::Coulomb)] {
        for iterations in [1, 4, 16] {
            println!("  {name:<22} {iterations:>2} pgs iteration(s): {:.3}", slide(model, iterations));
        }
    }
    println!();
    for side in [2.0, 6.0] {
        // Uniform pressure on a square of side L: the mean distance from the centre is 0.38260 L; the moment of inertia of a thin plate is m L^2 / 6.
        let mean_radius = 0.382597858 * side;
        let expected = mu * g * mean_radius / (side * side / 6.0);
        let corner = mu * g * (side / 2.0 * 2f64.sqrt()) / (side * side / 6.0);
        println!(
            "spin-down of a {side} x {side} m plate (thickness 0.2): continuum answer {expected:.3} rad/s^2; four point contacts at the corners would give {corner:.3}"
        );
        for (name, model) in [("simplified (default)", FrictionModel::Simplified), ("coulomb per contact", FrictionModel::Coulomb)] {
            let (time, deceleration) = spin_down(model, side, 0.2);
            println!("  {name:<22} mean deceleration {deceleration:.3} rad/s^2 (spin halved in {time:.3} s)");
        }
    }
}
