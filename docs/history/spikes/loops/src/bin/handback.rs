//! Handing a hit back to a mechanism as an impulse, against one step that does both (probe for step 4c-5 of `docs/decisions/joints-and-cars.md`; the owner's idea of 2026-11-13).
//!
//! A chain of cars (spheres of 1000 kg, joined by fixed joints or hinges) is hit by a stranger (a sphere) that comes along it or across it. Schemes:
//!
//! - *hi*: one world with everything, with so much solver work (12 steps a tick, 16 substeps, 32 passes) that the chain's answer has stopped changing. The reference.
//! - *game*: the same world at the game's settings (3 steps a tick, 4 substeps, 1 pass): what the engine does today.
//! - *whole, rigid*: two worlds (`split.rs`). A contact world holds the stranger and the chain as ONE body (its mass and inertia, at the pose and with the motion that the
//!   cars' momenta give it); a joint world holds the cars and their joints and never sees the stranger. The change of the one body's velocity is given back to every car as one
//!   rigid motion (loop-probe section 14, "whole ship").
//! - *whole, impulse*: the same contact world; what is given back is the impulse, to the cars that touched the stranger, at the points where they touched it (read from Rapier's
//!   contact pairs), and the joint world's own step spreads it through the joints. This is the owner's proposal.
//! - *per car*: the contact world holds each car as its own free body; each car gets back the change of its own velocity (which is the same as an impulse: m dv).
//!
//! Run: `cargo run --release --bin handback`.

use loop_probe::*;

const HALF: f64 = 0.5;
const MASS: f64 = 1000.0;
const SPEED: f64 = 5.0;
const CARS: usize = 4;
/// A car's moment of inertia about any axis through its centre (the box of `add_car`).
const OWN_INERTIA: f64 = MASS / 6.0;
const SECONDS: f64 = 1.0;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Joints {
    Fixed,
    Hinge,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Hit {
    /// Along the chain, onto its last car's end.
    Axial,
    /// Across the chain, onto its last car's side (the chain turns).
    Transverse,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Scheme {
    Hi,
    Game,
    WholeRigid,
    WholeImpulse,
    PerCar,
    /// Per car, then the joint pass and the contact pass again that many times in a step, the contact pass each time from the velocities without its own last part (block
    /// Gauss-Seidel across the two worlds), the stranger put back where the step began each time.
    Iterated(usize),
}

impl Scheme {
    fn name(self) -> String {
        match self {
            Scheme::Hi => "one world, converged".to_string(),
            Scheme::Game => "one world, game settings".to_string(),
            Scheme::WholeRigid => "whole ship, rigid handback".to_string(),
            Scheme::WholeImpulse => "whole ship, impulse handback".to_string(),
            Scheme::PerCar => "per car".to_string(),
            Scheme::Iterated(rounds) => format!("per car, {rounds} more rounds"),
        }
    }
}

#[derive(Clone, Copy)]
struct Motion {
    pose: Pose,
    linvel: Vector,
    angvel: Vector,
}

fn car_groups() -> InteractionGroups {
    InteractionGroups::new(Group::GROUP_1, Group::GROUP_2, InteractionTestMode::And)
}

fn stranger_groups() -> InteractionGroups {
    InteractionGroups::new(Group::GROUP_2, Group::GROUP_1, InteractionTestMode::And)
}

fn rest_pose(k: usize) -> Pose {
    Pose::from_translation(Vector::X * (2.0 * HALF * k as f64))
}

fn car_collider() -> ColliderBuilder {
    ColliderBuilder::ball(HALF).density(0.0).friction(0.0).restitution(0.0).collision_groups(car_groups())
}

fn add_stranger(world: &mut World, hit: Hit, mass: f64) -> RigidBodyHandle {
    let last = 2.0 * HALF * (CARS - 1) as f64;
    let (place, velocity) = match hit {
        Hit::Axial => (Vector::new(last + 3.0 * HALF, 0.0, 0.0), Vector::new(-SPEED, 0.0, 0.0)),
        Hit::Transverse => (Vector::new(last, 3.0 * HALF, 0.0), Vector::new(0.0, -SPEED, 0.0)),
    };
    let inertia = Vector::splat(0.4 * mass * HALF * HALF);
    let body = world.bodies.insert(
        RigidBodyBuilder::dynamic()
            .translation(place)
            .linvel(velocity)
            .can_sleep(false)
            .additional_mass_properties(MassProperties::new(Vector::ZERO, mass, inertia)),
    );
    world.colliders.insert_with_parent(
        ColliderBuilder::ball(HALF).density(0.0).friction(0.0).restitution(0.0).collision_groups(stranger_groups()),
        body,
        &mut world.bodies,
    );
    body
}

fn add_cars(world: &mut World, colliders: bool, joints: Option<Joints>) -> Vec<RigidBodyHandle> {
    let handles: Vec<RigidBodyHandle> = (0..CARS).map(|k| add_car(world, rest_pose(k), Vector::splat(HALF), MASS).handle).collect();
    if colliders {
        for &handle in &handles {
            world.colliders.insert_with_parent(car_collider(), handle, &mut world.bodies);
        }
    }
    if let Some(kind) = joints {
        let along = Vector::X * HALF;
        for pair in handles.windows(2) {
            match kind {
                Joints::Fixed => {
                    let joint = FixedJointBuilder::new().local_frame1(Pose::from_translation(along)).local_frame2(Pose::from_translation(-along)).contacts_enabled(false).build();
                    world.impulse_joints.insert(pair[0], pair[1], joint, true);
                }
                Joints::Hinge => {
                    let joint = RevoluteJointBuilder::new(Vector::Z).local_anchor1(along).local_anchor2(-along).contacts_enabled(false).build();
                    world.impulse_joints.insert(pair[0], pair[1], joint, true);
                }
            }
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

/// The inertia about the centre of the cars of this state (diagonal, in the world's axes: the scenes move in the xy plane, so only the z turn is ever used).
fn chain_inertia(truth: &[Motion], centre: Vector) -> Vector {
    let mut inertia = Vector::ZERO;
    for m in truth {
        let arm = m.pose.translation - centre;
        inertia += Vector::splat(OWN_INERTIA) + Vector::new(arm.y * arm.y + arm.z * arm.z, arm.x * arm.x + arm.z * arm.z, arm.x * arm.x + arm.y * arm.y) * MASS;
    }
    inertia
}

struct Sim {
    scheme: Scheme,
    /// Holds the cars' truth: everything for the one-world schemes, the cars and joints only for the others.
    main: World,
    cars: Vec<RigidBodyHandle>,
    contact: Option<World>,
    contact_cars: Vec<RigidBodyHandle>,
    proxy: Option<RigidBodyHandle>,
    proxy_colliders: Vec<ColliderHandle>,
    /// The stranger, in `main` for the one-world schemes, else in `contact`.
    stranger: RigidBodyHandle,
    stranger_mass: f64,
    /// Which way Rapier's contact impulse points (found when the first one is read).
    sign: Option<f64>,
    /// The worst difference between the impulses read from the contact pairs and M dv of the one body, over |M dv| (whole ship, impulse).
    worst_mismatch: f64,
}

impl Sim {
    fn new(scheme: Scheme, joints: Joints, hit: Hit, stranger_mass: f64) -> Self {
        let game = Settings::default();
        let hi = Settings { steps_per_tick: 12, substeps: 16, pgs_iterations: 32, ..game };
        let combined = matches!(scheme, Scheme::Hi | Scheme::Game);
        let settings = if scheme == Scheme::Hi { hi } else { game };
        let mut main = World::new(settings, Vector::ZERO);
        let mut sim;
        if combined {
            let cars = add_cars(&mut main, true, Some(joints));
            let stranger = add_stranger(&mut main, hit, stranger_mass);
            sim = Sim { scheme, main, cars, contact: None, contact_cars: Vec::new(), proxy: None, proxy_colliders: Vec::new(), stranger, stranger_mass, sign: None, worst_mismatch: 0.0 };
        } else {
            let cars = add_cars(&mut main, false, Some(joints));
            let mut contact = World::new(settings, Vector::ZERO);
            let stranger = add_stranger(&mut contact, hit, stranger_mass);
            let (mut contact_cars, mut proxy, mut proxy_colliders) = (Vec::new(), None, Vec::new());
            if matches!(scheme, Scheme::PerCar | Scheme::Iterated(_)) {
                contact_cars = add_cars(&mut contact, true, None);
            } else {
                let body = contact.bodies.insert(RigidBodyBuilder::dynamic().can_sleep(false).additional_mass_properties(MassProperties::new(Vector::ZERO, MASS * CARS as f64, Vector::ONE * 1e4)));
                proxy_colliders = (0..CARS).map(|_| contact.colliders.insert_with_parent(car_collider(), body, &mut contact.bodies)).collect();
                proxy = Some(body);
            }
            sim = Sim { scheme, main, cars, contact: Some(contact), contact_cars, proxy, proxy_colliders, stranger, stranger_mass, sign: None, worst_mismatch: 0.0 };
        }
        sim.main.tick_length = settings.tick_length;
        sim
    }

    fn steps_per_tick(&self) -> usize {
        self.main.steps_per_tick
    }

    fn stranger_motion(&self) -> Motion {
        let world = self.contact.as_ref().unwrap_or(&self.main);
        read(world, &[self.stranger])[0]
    }

    fn step(&mut self) {
        match self.scheme {
            Scheme::Hi | Scheme::Game => self.main.step(),
            Scheme::PerCar => {
                let truth = read(&self.main, &self.cars);
                let contact = self.contact.as_mut().expect("contact world");
                write(contact, &self.contact_cars, &truth);
                contact.step();
                let after = read(contact, &self.contact_cars);
                let mixed: Vec<Motion> = truth.iter().zip(&after).map(|(old, new)| Motion { pose: old.pose, linvel: new.linvel, angvel: new.angvel }).collect();
                self.joint_pass(&mixed);
            }
            Scheme::WholeRigid | Scheme::WholeImpulse => self.whole_step(),
            Scheme::Iterated(rounds) => self.iterated_step(rounds),
        }
    }

    fn iterated_step(&mut self, rounds: usize) {
        let truth = read(&self.main, &self.cars);
        let stranger_start = read(self.contact.as_ref().expect("contact world"), &[self.stranger]);
        let mut velocities: Vec<(Vector, Vector)> = truth.iter().map(|m| (m.linvel, m.angvel)).collect();
        let mut contact_part = vec![(Vector::ZERO, Vector::ZERO); truth.len()];
        for round in 0..=rounds {
            let free: Vec<Motion> = truth
                .iter()
                .zip(velocities.iter().zip(&contact_part))
                .map(|(m, (v, part))| Motion { pose: m.pose, linvel: v.0 - part.0, angvel: v.1 - part.1 })
                .collect();
            let contact = self.contact.as_mut().expect("contact world");
            write(contact, &self.contact_cars, &free);
            write(contact, &[self.stranger], &stranger_start);
            contact.step();
            let after = read(contact, &self.contact_cars);
            for ((part, old), new) in contact_part.iter_mut().zip(&free).zip(&after) {
                *part = (new.linvel - old.linvel, new.angvel - old.angvel);
            }
            let mixed: Vec<Motion> = truth.iter().zip(&after).map(|(m, new)| Motion { pose: m.pose, linvel: new.linvel, angvel: new.angvel }).collect();
            if round == rounds {
                self.joint_pass(&mixed);
            } else {
                // A pass for the velocities only: the joints' answer, and the poses put back.
                write(&mut self.main, &self.cars, &mixed);
                self.main.step();
                let solved = read(&self.main, &self.cars);
                velocities = solved.iter().map(|m| (m.linvel, m.angvel)).collect();
                let back: Vec<Motion> = truth.iter().zip(&solved).map(|(m, s)| Motion { pose: m.pose, linvel: s.linvel, angvel: s.angvel }).collect();
                write(&mut self.main, &self.cars, &back);
            }
        }
    }

    fn whole_step(&mut self) {
        let truth = read(&self.main, &self.cars);
        let n = truth.len() as f64;
        let mass = MASS * n;
        let centre = truth.iter().fold(Vector::ZERO, |sum, m| sum + m.pose.translation) / n;
        let mean = truth.iter().fold(Vector::ZERO, |sum, m| sum + m.linvel * MASS) / mass;
        let inertia = chain_inertia(&truth, centre);
        let mut spin = Vector::ZERO;
        for m in &truth {
            spin += m.angvel * OWN_INERTIA + (m.pose.translation - centre).cross(m.linvel - mean) * MASS;
        }
        let turn = Vector::new(spin.x / inertia.x, spin.y / inertia.y, spin.z / inertia.z);
        let contact = self.contact.as_mut().expect("contact world");
        let ship = self.proxy.expect("proxy");
        for (handle, m) in self.proxy_colliders.iter().zip(&truth) {
            contact.colliders[*handle].set_position_wrt_parent(Pose::from_parts(m.pose.translation - centre, m.pose.rotation));
        }
        contact.bodies[ship].set_additional_mass_properties(MassProperties::new(Vector::ZERO, mass, inertia), false);
        contact.bodies[ship].set_position(Pose::from_translation(centre), false);
        contact.bodies[ship].set_linvel(mean, false);
        contact.bodies[ship].set_angvel(turn, false);
        contact.step();
        let (change, turn_change) = (contact.bodies[ship].linvel() - mean, contact.bodies[ship].angvel() - turn);
        let mixed: Vec<Motion> = if self.scheme == Scheme::WholeRigid {
            truth
                .iter()
                .map(|m| Motion { pose: m.pose, linvel: m.linvel + change + turn_change.cross(m.pose.translation - centre), angvel: m.angvel + turn_change })
                .collect()
        } else {
            // The impulse each car's colliders were given by the stranger, from Rapier's contact pairs, and where.
            let mut impulses = vec![Vector::ZERO; truth.len()];
            let mut moments = vec![Vector::ZERO; truth.len()];
            let mut sum = Vector::ZERO;
            for pair in contact.narrow_phase.contact_pairs() {
                let (index, first) = if let Some(i) = self.proxy_colliders.iter().position(|&c| c == pair.collider1) {
                    (i, true)
                } else if let Some(i) = self.proxy_colliders.iter().position(|&c| c == pair.collider2) {
                    (i, false)
                } else {
                    continue;
                };
                let first_collider = &contact.colliders[pair.collider1];
                let (place, rotation) = (first_collider.position().translation, first_collider.position().rotation);
                let own_centre = {
                    let handle = self.proxy_colliders[index];
                    contact.colliders[handle].position().translation
                };
                for manifold in pair.manifolds() {
                    for point in &manifold.points {
                        // Rapier: the impulse "applied by this contact to the first collider's rigid-body", along the manifold's normal.
                        let along_normal = manifold.data.normal * point.data.impulse;
                        let on_proxy = if first { along_normal } else { -along_normal };
                        let world_point = place + rotation * point.local_p1;
                        impulses[index] += on_proxy;
                        moments[index] += (world_point - own_centre).cross(on_proxy);
                        sum += on_proxy;
                    }
                }
            }
            let wanted = mass * change;
            if wanted.length() > 1e-6 {
                let s = *self.sign.get_or_insert(if sum.dot(wanted) >= 0.0 { 1.0 } else { -1.0 });
                self.worst_mismatch = self.worst_mismatch.max((sum * s - wanted).length() / wanted.length());
            }
            let s = self.sign.unwrap_or(1.0);
            truth
                .iter()
                .enumerate()
                .map(|(i, m)| Motion { pose: m.pose, linvel: m.linvel + impulses[i] * (s / MASS), angvel: m.angvel + moments[i] * (s / OWN_INERTIA) })
                .collect()
        };
        self.joint_pass(&mixed);
    }

    fn joint_pass(&mut self, mixed: &[Motion]) {
        write(&mut self.main, &self.cars, mixed);
        self.main.step();
    }

    fn tick(&mut self) {
        for _ in 0..self.steps_per_tick() {
            self.step();
        }
    }
}

/// Everything measured at one moment.
#[derive(Clone, Copy, Default)]
struct Snapshot {
    stranger_velocity: [f64; 2],
    chain_velocity: [f64; 2],
    chain_spin: f64,
    last_car: [f64; 2],
    first_car: [f64; 2],
    energy: f64,
    internal_energy: f64,
    momentum_x: f64,
    momentum_y: f64,
}

fn snapshot(sim: &Sim) -> Snapshot {
    let cars = read(&sim.main, &sim.cars);
    let stranger = sim.stranger_motion();
    let n = cars.len() as f64;
    let mass = MASS * n;
    let centre = cars.iter().fold(Vector::ZERO, |sum, m| sum + m.pose.translation) / n;
    let mean = cars.iter().fold(Vector::ZERO, |sum, m| sum + m.linvel * MASS) / mass;
    let inertia = chain_inertia(&cars, centre);
    let mut spin = Vector::ZERO;
    let mut energy = 0.0;
    for m in &cars {
        spin += m.angvel * OWN_INERTIA + (m.pose.translation - centre).cross(m.linvel - mean) * MASS;
        energy += 0.5 * MASS * m.linvel.dot(m.linvel) + 0.5 * OWN_INERTIA * m.angvel.dot(m.angvel);
    }
    let rigid = 0.5 * mass * mean.dot(mean) + 0.5 * spin.z * spin.z / inertia.z;
    let stranger_energy = 0.5 * sim.stranger_mass * stranger.linvel.dot(stranger.linvel) + 0.5 * (0.4 * sim.stranger_mass * HALF * HALF) * stranger.angvel.dot(stranger.angvel);
    let momentum = mean * mass + stranger.linvel * sim.stranger_mass;
    Snapshot {
        stranger_velocity: [stranger.linvel.x, stranger.linvel.y],
        chain_velocity: [mean.x, mean.y],
        chain_spin: spin.z,
        last_car: [cars[CARS - 1].pose.translation.x, cars[CARS - 1].pose.translation.y],
        first_car: [cars[0].pose.translation.x, cars[0].pose.translation.y],
        energy: energy + stranger_energy,
        internal_energy: energy - rigid,
        momentum_x: momentum.x,
        momentum_y: momentum.y,
    }
}

/// The joints' worst gap (anchor to anchor), in millimetres.
fn joint_gap_mm(sim: &Sim) -> f64 {
    let cars = read(&sim.main, &sim.cars);
    let along = Vector::X * HALF;
    cars.windows(2)
        .map(|pair| {
            let a = pair[0].pose.translation + pair[0].pose.rotation * along;
            let b = pair[1].pose.translation + pair[1].pose.rotation * -along;
            (a - b).length() * 1e3
        })
        .fold(0.0, f64::max)
}

/// How far the stranger's sphere reaches into the nearest car's, in millimetres (negative: apart).
fn overlap_mm(sim: &Sim) -> f64 {
    let cars = read(&sim.main, &sim.cars);
    let stranger = sim.stranger_motion().pose.translation;
    cars.iter().map(|m| (2.0 * HALF - (m.pose.translation - stranger).length()) * 1e3).fold(f64::MIN, f64::max)
}

struct Outcome {
    end: Snapshot,
    start_energy: f64,
    start_momentum: [f64; 2],
    worst_gap_mm: f64,
    deepest_overlap_mm: f64,
    mismatch: f64,
}

fn run(scheme: Scheme, joints: Joints, hit: Hit, stranger_mass: f64) -> Outcome {
    let mut sim = Sim::new(scheme, joints, hit, stranger_mass);
    let first = snapshot(&sim);
    let ticks = (SECONDS / sim.main.tick_length).round() as usize;
    let (mut worst_gap_mm, mut deepest_overlap_mm) = (0.0f64, f64::MIN);
    for _ in 0..ticks {
        sim.tick();
        worst_gap_mm = worst_gap_mm.max(joint_gap_mm(&sim));
        deepest_overlap_mm = deepest_overlap_mm.max(overlap_mm(&sim));
    }
    Outcome { end: snapshot(&sim), start_energy: first.energy, start_momentum: [first.momentum_x, first.momentum_y], worst_gap_mm, deepest_overlap_mm, mismatch: sim.worst_mismatch }
}

fn scene(joints: Joints, hit: Hit, stranger_mass: f64) {
    println!("\n{joints:?} chain of {CARS} cars, hit {hit:?}ly by a stranger of {stranger_mass} kg at {SPEED} m/s; after {SECONDS} s");
    println!(
        "{:<30} {:>9} {:>9} {:>9} {:>9} {:>8} {:>8} {:>9} {:>8} {:>8} {:>9}",
        "scheme", "stranger", "stranger", "chain", "chain", "spin", "energy", "internal", "end car", "gap mm", "overlap"
    );
    println!(
        "{:<30} {:>9} {:>9} {:>9} {:>9} {:>8} {:>8} {:>9} {:>8} {:>8} {:>9}",
        "", "vx", "vy", "vx", "vy", "rad/s", "kept", "KE kJ", "off mm", "worst", "mm"
    );
    let reference = run(Scheme::Hi, joints, hit, stranger_mass);
    for scheme in [Scheme::Hi, Scheme::Game, Scheme::WholeRigid, Scheme::WholeImpulse, Scheme::PerCar, Scheme::Iterated(2), Scheme::Iterated(8), Scheme::Iterated(32)] {
        let o = if scheme == Scheme::Hi { run(Scheme::Hi, joints, hit, stranger_mass) } else { run(scheme, joints, hit, stranger_mass) };
        let off = ((o.end.last_car[0] - reference.end.last_car[0]).powi(2) + (o.end.last_car[1] - reference.end.last_car[1]).powi(2)).sqrt() * 1e3;
        let momentum_drift = ((o.end.momentum_x - o.start_momentum[0]).powi(2) + (o.end.momentum_y - o.start_momentum[1]).powi(2)).sqrt();
        println!(
            "{:<30} {:>+9.3} {:>+9.3} {:>+9.3} {:>+9.3} {:>+8.3} {:>8.3} {:>9.2} {:>8.1} {:>8.2} {:>9.1}   dP {:.1e}{}",
            scheme.name(),
            o.end.stranger_velocity[0],
            o.end.stranger_velocity[1],
            o.end.chain_velocity[0],
            o.end.chain_velocity[1],
            o.end.chain_spin / (MASS * CARS as f64 * HALF * HALF).max(1.0),
            o.end.energy / o.start_energy,
            o.end.internal_energy / 1e3,
            off,
            o.worst_gap_mm,
            o.deepest_overlap_mm,
            momentum_drift,
            if scheme == Scheme::WholeImpulse { format!("  contact-pair impulses vs M dv: {:.1e}", o.mismatch) } else { String::new() }
        );
    }
}

fn main() {
    for stranger_mass in [1000.0, 4000.0] {
        scene(Joints::Fixed, Hit::Axial, stranger_mass);
        scene(Joints::Fixed, Hit::Transverse, stranger_mass);
        scene(Joints::Hinge, Hit::Axial, stranger_mass);
        scene(Joints::Hinge, Hit::Transverse, stranger_mass);
    }
}
