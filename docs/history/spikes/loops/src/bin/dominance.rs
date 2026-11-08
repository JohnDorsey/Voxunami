//! Can a contact change the momentum of one body and leave the other's exactly as it was? Rapier's *dominance groups*: in a contact between bodies of different
//! dominance the lower one is solved against the higher one as if the higher had infinite mass. Is the higher one also *moving* (carrying the lower along), or is it a
//! wall at rest?  A heavy "ship" box flies at 2 m/s; a light "player" box is on its +x side moving at -1 m/s (3 m/s towards the ship, relative). Zero gravity.
//!
//! Run: `cargo run --release --bin dominance`.

use loop_probe::*;

const SHIP_MASS: f64 = 100_000.0;
const PLAYER_MASS: f64 = 80.0;
const SHIP_SPEED: f64 = 2.0;
const PLAYER_SPEED: f64 = -1.0;

struct Outcome {
    ship_speed_change: f64,
    ship_spin: f64,
    player_speed: f64,
    penetration: f64,
}

fn run(ship_dominance: i8, player_dominance: i8, off_centre: f64, kinematic: bool, ship_spin: f64) -> Outcome {
    let mut world = World::new(Settings::default(), Vector::ZERO);
    let ship = world.bodies.insert(
        (if kinematic { RigidBodyBuilder::kinematic_velocity_based() } else { RigidBodyBuilder::dynamic() })
            .linvel(Vector::new(SHIP_SPEED, 0.0, 0.0))
            .angvel(Vector::new(0.0, 0.0, ship_spin))
            .dominance_group(ship_dominance)
            .can_sleep(false),
    );
    world.colliders.insert_with_parent(ColliderBuilder::cuboid(2.0, 2.0, 2.0).mass(SHIP_MASS).friction(0.6), ship, &mut world.bodies);
    // The player's face starts 0.5 m from the ship's +x face (at x = 2).
    let player = world.bodies.insert(
        RigidBodyBuilder::dynamic()
            .translation(Vector::new(2.0 + 0.5 + 0.4 + SHIP_SPEED * 0.0, off_centre, 0.0))
            .linvel(Vector::new(PLAYER_SPEED, 0.0, 0.0))
            .dominance_group(player_dominance)
            .can_sleep(false),
    );
    world.colliders.insert_with_parent(ColliderBuilder::cuboid(0.4, 0.4, 0.4).mass(PLAYER_MASS).friction(0.6), player, &mut world.bodies);
    for _ in 0..40 {
        world.tick();
    }
    let ship_body = &world.bodies[ship];
    let player_body = &world.bodies[player];
    let gap = (player_body.translation().x - 0.4) - (ship_body.translation().x + 2.0);
    Outcome {
        ship_speed_change: ship_body.linvel().x - SHIP_SPEED,
        ship_spin: ship_body.angvel().length(),
        player_speed: player_body.linvel().x,
        penetration: (-gap).max(0.0),
    }
}

fn main() {
    println!("ship {SHIP_MASS} kg at {SHIP_SPEED} m/s, player {PLAYER_MASS} kg at {PLAYER_SPEED} m/s, 2 s (40 ticks)\n");
    println!("{:<44} {:>16} {:>12} {:>14} {:>10}", "case", "ship dv (m/s)", "ship spin", "player v (m/s)", "overlap");
    for (name, ship_dominance, player_dominance, off_centre, kinematic, spin) in [
        ("equal dominance (EVA bonk)", 0, 0, 0.0, false, 0.0),
        ("ship dominates (player inside)", 1, 0, 0.0, false, 0.0),
        ("ship dominates, hit 1.5 m off centre", 1, 0, 1.5, false, 0.0),
        ("equal, hit 1.5 m off centre", 0, 0, 1.5, false, 0.0),
        ("player dominates (control)", 0, 1, 0.0, false, 0.0),
        ("ship KINEMATIC (velocity-based)", 0, 0, 0.0, true, 0.0),
        ("ship KINEMATIC, spinning 0.5 rad/s, 1.5 m off", 0, 0, 1.5, true, 0.5),
        ("ship dominant dynamic, spinning 0.5 rad/s, 1.5 m off", 1, 0, 1.5, false, 0.5),
    ] {
        let o = run(ship_dominance, player_dominance, off_centre, kinematic, spin);
        println!("{name:<44} {:>16.3e} {:>12.3e} {:>14.4} {:>9.1} mm", o.ship_speed_change, o.ship_spin, o.player_speed, o.penetration * 1000.0);
    }
}
