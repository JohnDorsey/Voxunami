//! Two isolated Rapier steps, one for the joints and one for the collisions, against one step that does both. A rigid ship of cubes (fixed impulse joints) is either
//! standing on a floor under gravity (*tower*) or flies into a wall (*rod*). Three ways to step it:
//!
//! - *combined*: one world with the joints, the colliders and the terrain (what `BodyWorld` does).
//! - *per car*: a contact world (the cars as free bodies with colliders, the terrain, gravity, no joints) is stepped first and gives the velocities; a joint world (the
//!   cars without colliders, the joints, no gravity) is then stepped from the old poses and those velocities, and gives the new state. Each car meets the terrain as if it
//!   were alone.
//! - *whole ship*: the same, except that the contact world holds the ship as ONE body (all its colliders, its total mass and inertia), so a contact sees the ship's mass;
//!   the change of its velocity is given to every car as one rigid motion, which no joint resists.
//!
//! Run: `cargo run --release --bin split -- [tower|rod|all]`.

use std::time::{Duration, Instant};

use loop_probe::*;

const HALF: f64 = 0.5;
const MASS: f64 = 1000.0;
const SPEED: f64 = 5.0;
const GAP_TO_WALL: f64 = 5.0;
const GRAVITY: f64 = 9.81;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Scheme {
    Combined,
    PerCar,
    WholeShip,
}

impl Scheme {
    fn name(self) -> &'static str {
        match self {
            Scheme::Combined => "combined",
            Scheme::PerCar => "two worlds, per car",
            Scheme::WholeShip => "two worlds, whole ship",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Scene {
    Tower,
    Rod,
}

#[derive(Clone, Copy)]
struct Motion {
    pose: Pose,
    linvel: Vector,
    angvel: Vector,
}

fn axis(scene: Scene) -> Vector {
    match scene {
        Scene::Tower => Vector::Y,
        Scene::Rod => Vector::X,
    }
}

fn car_groups() -> InteractionGroups {
    InteractionGroups::new(Group::GROUP_1, Group::GROUP_2, InteractionTestMode::And)
}

fn terrain_groups() -> InteractionGroups {
    InteractionGroups::new(Group::GROUP_2, Group::GROUP_1, InteractionTestMode::And)
}

/// Car `k` of a column along the scene's axis, edge to edge, the first at the floor (tower) or at the rod's tail (rod).
fn rest_pose(scene: Scene, k: usize) -> Pose {
    Pose::from_translation(axis(scene) * (HALF + 2.0 * HALF * k as f64))
}

fn wall_face(count: usize) -> f64 {
    2.0 * HALF * count as f64 + GAP_TO_WALL
}

fn add_terrain(world: &mut World, scene: Scene, count: usize) {
    let builder = match scene {
        Scene::Tower => ColliderBuilder::cuboid(50.0, 0.5, 50.0).translation(Vector::new(0.0, -0.5, 0.0)),
        Scene::Rod => ColliderBuilder::cuboid(1.0, 20.0, 20.0).translation(Vector::new(wall_face(count) + 1.0, 0.0, 0.0)),
    };
    world.colliders.insert(builder.friction(0.6).collision_groups(terrain_groups()));
}

/// A cube's flat face stands on the floor (four corner contacts, as a ship's does); a sphere meets the wall at one point, so that a head-on hit has nothing in it that
/// could turn the rod (a cube hitting a wall face-on is turned by the order the solver takes its four contacts in, in every scheme, the combined one too).
fn car_collider(scene: Scene) -> ColliderBuilder {
    match scene {
        Scene::Tower => ColliderBuilder::cuboid(HALF, HALF, HALF),
        Scene::Rod => ColliderBuilder::ball(HALF),
    }
    .density(0.0)
    .friction(0.6)
    .collision_groups(car_groups())
}

fn add_cars(world: &mut World, scene: Scene, count: usize, colliders: bool, joints: bool) -> Vec<RigidBodyHandle> {
    let handles: Vec<RigidBodyHandle> = (0..count).map(|k| add_car(world, rest_pose(scene, k), Vector::splat(HALF), MASS).handle).collect();
    if colliders {
        for &handle in &handles {
            world.colliders.insert_with_parent(car_collider(scene), handle, &mut world.bodies);
        }
    }
    if joints {
        let along = axis(scene) * HALF;
        for pair in handles.windows(2) {
            let joint = FixedJointBuilder::new().local_frame1(Pose::from_translation(along)).local_frame2(Pose::from_translation(-along)).build();
            world.impulse_joints.insert(pair[0], pair[1], joint, true);
        }
    }
    handles
}

fn read(world: &World, handles: &[RigidBodyHandle]) -> Vec<Motion> {
    handles.iter().map(|&h| Motion { pose: *world.bodies[h].position(), linvel: world.bodies[h].linvel(), angvel: world.bodies[h].angvel() }).collect()
}

fn write(world: &mut World, handles: &[RigidBodyHandle], motions: &[Motion]) {
    for (&handle, motion) in handles.iter().zip(motions) {
        let body = &mut world.bodies[handle];
        body.set_position(motion.pose, false);
        body.set_linvel(motion.linvel, false);
        body.set_angvel(motion.angvel, false);
    }
}

fn momentum(motions: &[Motion]) -> Vector {
    motions.iter().fold(Vector::ZERO, |sum, m| sum + m.linvel * MASS)
}

struct Sim {
    scheme: Scheme,
    scene: Scene,
    /// The world that holds the truth: everything for `Combined`, the joints only for the two-world schemes.
    main: World,
    cars: Vec<RigidBodyHandle>,
    contact: Option<World>,
    contact_cars: Vec<RigidBodyHandle>,
    ship: Option<RigidBodyHandle>,
    ship_colliders: Vec<ColliderHandle>,
    /// The principal inertia of the ship about its centre (the scenes are symmetric about their axes, so the tensor is diagonal in the world's axes).
    ship_inertia: Vector,
    time: Duration,
    /// The largest change of the cars' total momentum in a joint pass (internal impulses only, no gravity: it must be 0).
    worst_joint_pass_change: f64,
}

impl Sim {
    fn new(scheme: Scheme, scene: Scene, count: usize, settings: Settings) -> Self {
        let gravity = match scene {
            Scene::Tower => Vector::new(0.0, -GRAVITY, 0.0),
            Scene::Rod => Vector::ZERO,
        };
        let mut sim = Sim {
            scheme,
            scene,
            main: World::new(settings, if scheme == Scheme::Combined { gravity } else { Vector::ZERO }),
            cars: Vec::new(),
            contact: None,
            contact_cars: Vec::new(),
            ship: None,
            ship_colliders: Vec::new(),
            ship_inertia: Vector::ONE,
            time: Duration::ZERO,
            worst_joint_pass_change: 0.0,
        };
        if scheme == Scheme::Combined {
            add_terrain(&mut sim.main, scene, count);
            sim.cars = add_cars(&mut sim.main, scene, count, true, true);
        } else {
            sim.cars = add_cars(&mut sim.main, scene, count, false, true);
            let mut contact = World::new(settings, gravity);
            add_terrain(&mut contact, scene, count);
            if scheme == Scheme::PerCar {
                sim.contact_cars = add_cars(&mut contact, scene, count, true, false);
            } else {
                // One body for the ship: its mass, and the inertia about its centre by the parallel-axis rule (a cube's is MASS / 6 about any axis through it).
                let along = axis(scene);
                let centre = HALF * count as f64;
                let mut inertia = Vector::ZERO;
                for k in 0..count {
                    let arm = HALF + 2.0 * HALF * k as f64 - centre;
                    inertia += Vector::splat(MASS / 6.0) + (Vector::ONE - along) * (MASS * arm * arm);
                }
                sim.ship_inertia = inertia;
                let ship = contact.bodies.insert(
                    RigidBodyBuilder::dynamic().can_sleep(false).additional_mass_properties(MassProperties::new(Vector::ZERO, MASS * count as f64, inertia)),
                );
                sim.ship_colliders = (0..count).map(|_| contact.colliders.insert_with_parent(car_collider(scene), ship, &mut contact.bodies)).collect();
                sim.ship = Some(ship);
            }
            sim.contact = Some(contact);
        }
        if scene == Scene::Rod {
            for &handle in &sim.cars {
                sim.main.bodies[handle].set_linvel(Vector::X * SPEED, true);
            }
        }
        sim
    }

    fn step(&mut self) {
        let started = Instant::now();
        match self.scheme {
            Scheme::Combined => self.main.step(),
            Scheme::PerCar => {
                let truth = read(&self.main, &self.cars);
                let contact = self.contact.as_mut().expect("contact world");
                write(contact, &self.contact_cars, &truth);
                contact.step();
                let after = read(contact, &self.contact_cars);
                let mixed: Vec<Motion> = truth.iter().zip(&after).map(|(old, new)| Motion { pose: old.pose, linvel: new.linvel, angvel: new.angvel }).collect();
                self.joint_pass(&mixed);
            }
            Scheme::WholeShip => {
                let truth = read(&self.main, &self.cars);
                let n = truth.len() as f64;
                let centre = truth.iter().fold(Vector::ZERO, |sum, m| sum + m.pose.translation) / n;
                let mean = momentum(&truth) / (MASS * n);
                let mut spin = Vector::ZERO;
                for m in &truth {
                    spin += m.angvel * (MASS / 6.0) + (m.pose.translation - centre).cross(m.linvel - mean) * MASS;
                }
                let turn = Vector::new(spin.x / self.ship_inertia.x, spin.y / self.ship_inertia.y, spin.z / self.ship_inertia.z);
                let contact = self.contact.as_mut().expect("contact world");
                for (handle, m) in self.ship_colliders.iter().zip(&truth) {
                    contact.colliders[*handle].set_position_wrt_parent(Pose::from_parts(m.pose.translation - centre, m.pose.rotation));
                }
                let ship = self.ship.expect("ship");
                contact.bodies[ship].set_position(Pose::from_translation(centre), false);
                contact.bodies[ship].set_linvel(mean, false);
                contact.bodies[ship].set_angvel(turn, false);
                contact.step();
                let (change, turn_change) = (contact.bodies[ship].linvel() - mean, contact.bodies[ship].angvel() - turn);
                let mixed: Vec<Motion> = truth
                    .iter()
                    .map(|m| Motion { pose: m.pose, linvel: m.linvel + change + turn_change.cross(m.pose.translation - centre), angvel: m.angvel + turn_change })
                    .collect();
                self.joint_pass(&mixed);
            }
        }
        self.time += started.elapsed();
    }

    fn joint_pass(&mut self, mixed: &[Motion]) {
        write(&mut self.main, &self.cars, mixed);
        let before = momentum(mixed);
        self.main.step();
        let after = momentum(&read(&self.main, &self.cars));
        self.worst_joint_pass_change = self.worst_joint_pass_change.max((after - before).length());
    }

    fn tick(&mut self) {
        for _ in 0..self.main.steps_per_tick {
            self.step();
        }
    }
}

struct Outcome {
    /// Tower: how far the lowest car's bottom is above (positive) or into (negative) the floor, mean over the last 5 s, in mm.
    sag_mm: f64,
    /// Tower: the largest speed of any car in the last 5 s, in mm/s (a standing tower is at rest).
    jitter_mm_per_s: f64,
    /// Rod: the deepest the front face went into the wall, in mm.
    penetration_mm: f64,
    /// Rod: the rod's speed at the end, mm/s (it must have stopped).
    final_speed_mm_per_s: f64,
    /// The largest distance between the anchors of two joined cars, in micrometres.
    gap_um: f64,
    /// The farthest any car got from the axis the scene is symmetric about, in mm (a standing tower and a rod that hits head-on stay on it).
    off_axis_mm: f64,
    joint_pass_change: f64,
    micros_per_tick: f64,
}

fn run(scene: Scene, scheme: Scheme, count: usize, settings: Settings) -> Outcome {
    let mut sim = Sim::new(scheme, scene, count, settings);
    let seconds = match scene {
        Scene::Tower => 15.0,
        Scene::Rod => 4.0,
    };
    let ticks = (seconds / settings.tick_length).round() as usize;
    let last_five = ticks - (5.0 / settings.tick_length).round() as usize;
    let along = axis(scene);
    let mut out = Outcome { sag_mm: 0.0, jitter_mm_per_s: 0.0, penetration_mm: f64::MIN, final_speed_mm_per_s: 0.0, gap_um: 0.0, off_axis_mm: 0.0, joint_pass_change: 0.0, micros_per_tick: 0.0 };
    let mut sag_sum = 0.0;
    let mut sag_count = 0usize;
    for tick in 0..ticks {
        sim.tick();
        let state = read(&sim.main, &sim.cars);
        for pair in state.windows(2) {
            let gap = (pair[1].pose.translation - pair[0].pose.translation - along * (2.0 * HALF)).length();
            out.gap_um = out.gap_um.max(gap * 1e6);
        }
        for m in &state {
            let off = (m.pose.translation - along * m.pose.translation.dot(along)).length();
            out.off_axis_mm = out.off_axis_mm.max(off * 1e3);
        }
        match scene {
            Scene::Tower => {
                if tick >= last_five {
                    sag_sum += state[0].pose.translation.y - HALF;
                    sag_count += 1;
                    let fastest = state.iter().map(|m| m.linvel.length()).fold(0.0, f64::max);
                    out.jitter_mm_per_s = out.jitter_mm_per_s.max(fastest * 1e3);
                }
            }
            Scene::Rod => {
                let front = state.last().map_or(0.0, |m| m.pose.translation.x + HALF);
                out.penetration_mm = out.penetration_mm.max((front - wall_face(count)) * 1e3);
                out.final_speed_mm_per_s = (momentum(&state) / (MASS * count as f64)).length() * 1e3;
            }
        }
    }
    out.sag_mm = sag_sum / sag_count.max(1) as f64 * 1e3;
    out.joint_pass_change = sim.worst_joint_pass_change;
    out.micros_per_tick = sim.time.as_secs_f64() * 1e6 / ticks as f64;
    out
}

fn table(scene: Scene, counts: &[usize], settings: Settings, title: &str) {
    println!("\n{title}");
    match scene {
        Scene::Tower => println!(
            "{:<6} {:<24} {:>10} {:>12} {:>10} {:>12} {:>13} {:>8}",
            "cars", "scheme", "sag (mm)", "jitter mm/s", "gap (um)", "off axis mm", "joint pass dP", "us/tick"
        ),
        Scene::Rod => println!(
            "{:<6} {:<24} {:>12} {:>14} {:>10} {:>12} {:>13} {:>8}",
            "cars", "scheme", "into wall mm", "end speed mm/s", "gap (um)", "off axis mm", "joint pass dP", "us/tick"
        ),
    }
    for &count in counts {
        for scheme in [Scheme::Combined, Scheme::PerCar, Scheme::WholeShip] {
            let o = run(scene, scheme, count, settings);
            match scene {
                Scene::Tower => println!(
                    "{count:<6} {:<24} {:>+10.3} {:>12.3} {:>10.2} {:>12.2} {:>13.1e} {:>8.0}",
                    scheme.name(),
                    o.sag_mm,
                    o.jitter_mm_per_s,
                    o.gap_um,
                    o.off_axis_mm,
                    o.joint_pass_change,
                    o.micros_per_tick
                ),
                Scene::Rod => println!(
                    "{count:<6} {:<24} {:>12.2} {:>14.3} {:>10.2} {:>12.2} {:>13.1e} {:>8.0}",
                    scheme.name(),
                    o.penetration_mm,
                    o.final_speed_mm_per_s,
                    o.gap_um,
                    o.off_axis_mm,
                    o.joint_pass_change,
                    o.micros_per_tick
                ),
            }
        }
    }
}

/// Prints the cars' places and speeds along the scene's axis, tick by tick: `split trace tower|rod combined|car|ship COUNT [FIRST_TICK LAST_TICK]`.
fn trace(arguments: &[String]) {
    let scene = if arguments[0] == "tower" { Scene::Tower } else { Scene::Rod };
    let scheme = match arguments[1].as_str() {
        "combined" => Scheme::Combined,
        "car" => Scheme::PerCar,
        _ => Scheme::WholeShip,
    };
    let count: usize = arguments[2].parse().expect("count");
    let first: usize = arguments.get(3).map_or(0, |s| s.parse().expect("first tick"));
    let last: usize = arguments.get(4).map_or(60, |s| s.parse().expect("last tick"));
    let mut sim = Sim::new(scheme, scene, count, Settings::default());
    let along = axis(scene);
    for tick in 0..last {
        sim.tick();
        if tick < first {
            continue;
        }
        let state = read(&sim.main, &sim.cars);
        let places: Vec<String> = state.iter().map(|m| format!("{:+.4}", m.pose.translation.dot(along))).collect();
        let speeds: Vec<String> = state.iter().map(|m| format!("{:+.3}", m.linvel.dot(along))).collect();
        let turn = state.iter().map(|m| m.angvel.length()).fold(0.0, f64::max);
        let off = state.iter().map(|m| (m.pose.translation - along * m.pose.translation.dot(along)).length()).fold(0.0, f64::max);
        println!("tick {tick:>3}  along [{}]  speed [{}]  max turn {turn:.3}  max off-axis {off:.4}", places.join(" "), speeds.join(" "));
    }
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.first().map(String::as_str) == Some("trace") {
        trace(&arguments[1..]);
        return;
    }
    let which = std::env::args().nth(1).unwrap_or_else(|| "all".to_string());
    let all = which == "all";
    let game = Settings::default();
    let fine = Settings { steps_per_tick: 12, ..game };
    if which == "passes" || all {
        println!("\nthe rebound of the combined step: a rod of fixed-jointed spheres at 5 m/s into a wall (a single sphere stops dead), by the solver's work");
        println!("{:<6} {:>8} {:>8} {:>16} {:>14} {:>10}", "cars", "substeps", "passes", "rebound mm/s", "into wall mm", "gap (um)");
        for count in [2, 4] {
            for (substeps, pgs_iterations) in [(4, 1), (4, 2), (4, 4), (4, 8), (4, 16), (4, 32), (16, 1), (16, 8)] {
                let o = run(Scene::Rod, Scheme::Combined, count, Settings { substeps, pgs_iterations, ..game });
                println!("{count:<6} {substeps:>8} {pgs_iterations:>8} {:>16.1} {:>14.2} {:>10.2}", o.final_speed_mm_per_s, o.penetration_mm, o.gap_um);
            }
        }
    }
    if which == "tower" || all {
        table(Scene::Tower, &[2, 4, 8], game, "tower of cubes (1000 kg each, fixed joints) standing on a floor, 15 s, game step (3 steps a tick, 4 substeps)");
        table(Scene::Tower, &[4], fine, "the same, 4 cars, 4 times as many steps (12 a tick)");
    }
    if which == "rod" || all {
        table(Scene::Rod, &[2, 4, 8], game, "rod of cubes (fixed joints) at 5 m/s into a wall, 4 s, game step; no gravity");
        table(Scene::Rod, &[4], fine, "the same, 4 cars, 4 times as many steps (12 a tick)");
    }
}
