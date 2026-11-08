//! Throwaway probes of loop closure for jointed cars (see `docs/history/loop-probe.md`).
//!
//! `World` is a Rapier world set up like the game's `BodyWorld` (f64, a tick cut into steps, forces forgotten after each tick). `Rig` is a ring of
//! cars (the loop) with an optional chain of cars hanging off it (the tree), built in one of several ways. `totals` measures what must be
//! conserved when nothing pushes the rig from outside: linear momentum, angular momentum and kinetic energy.

pub mod harness;

use std::time::{Duration, Instant};

pub use rapier3d::prelude::*;

pub struct World {
    pub pipeline: PhysicsPipeline,
    pub parameters: IntegrationParameters,
    pub islands: IslandManager,
    pub broad_phase: BroadPhaseBvh,
    pub narrow_phase: NarrowPhase,
    pub bodies: RigidBodySet,
    pub colliders: ColliderSet,
    pub impulse_joints: ImpulseJointSet,
    pub multibody_joints: MultibodyJointSet,
    pub soft_bodies: SoftBodySet,
    pub ccd: CCDSolver,
    pub gravity: Vector,
    pub tick_length: f64,
    pub steps_per_tick: usize,
    /// Loops closed beside Rapier after every step (empty unless the rig was built with `Mode::MultibodyDirect`).
    pub direct_loops: Vec<LoopSpec>,
    pub direct_iterations: usize,
    pub direct_regularization: f64,
    /// Use the true mass matrix (built from the body jacobians and the cars' mass properties) instead of Rapier's factorization.
    pub direct_exact_mass: bool,
    pub cars: Vec<Car>,
    pub step_time: Duration,
    pub direct_time: Duration,
    pub direct_failures: usize,
    /// Softness to give the closing joint instead of Rapier's default.
    pub closing_softness: Option<SpringCoefficients<Real>>,
    /// Shifts the anchor of the closing joint in its second body, so that the loop is built inconsistent (strained).
    pub closing_mismatch: Vector,
    /// Holds the rig's centre and total momentum to a rail after every step (a ship on a rail: the rail is the centre of mass, and nothing outside pushes).
    pub recentre: Option<Recentre>,
    /// Round every dynamic body's pose and velocities to 32 bits after each step: the state a 32-bit build would keep (the arithmetic between steps stays 64-bit).
    pub quantize: bool,
}

/// How much of the rail the rig is held to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecentreMode {
    Off,
    /// The velocity error of the centre is taken from every car after every step.
    Velocity,
    /// That, and the position error of the centre too.
    VelocityAndPosition,
    /// That, and the error of the angular momentum about the centre, taken out as one rigid turn of the whole rig about its centre (which no joint resists).
    VelocityPositionAndSpin,
}

/// The rail: the straight line the centre must follow, and the total momentum it must keep.
#[derive(Clone, Copy, Debug)]
pub struct Recentre {
    pub mode: RecentreMode,
    pub momentum: Vector,
    pub centre: Vector,
    pub elapsed: f64,
    /// The sum of the outside forces on the rig, constant: the rail's own acceleration is this over the mass (zero for a coasting ship).
    pub force: Vector,
    /// The angular momentum about the centre that the rig must keep (nothing turns it from outside).
    pub spin: Vector,
}

#[derive(Clone, Copy, Debug)]
pub struct Settings {
    pub tick_length: f64,
    pub steps_per_tick: usize,
    /// Rapier's `num_solver_iterations`: the substeps of one step.
    pub substeps: usize,
    /// Rapier's `num_internal_pgs_iterations`.
    pub pgs_iterations: usize,
}

impl Default for Settings {
    /// The game's: a tick of 1/20 s in 3 steps, Rapier's own defaults inside.
    fn default() -> Self {
        Self { tick_length: 1.0 / 20.0, steps_per_tick: 3, substeps: 4, pgs_iterations: 1 }
    }
}

impl World {
    pub fn new(settings: Settings, gravity: Vector) -> Self {
        let mut parameters = IntegrationParameters::default();
        parameters.dt = settings.tick_length / settings.steps_per_tick as f64;
        parameters.num_solver_iterations = settings.substeps;
        parameters.num_internal_pgs_iterations = settings.pgs_iterations;
        parameters.normalized_max_linear_velocity = Real::MAX;
        Self {
            pipeline: PhysicsPipeline::new(),
            parameters,
            islands: IslandManager::new(),
            broad_phase: BroadPhaseBvh::new(),
            narrow_phase: NarrowPhase::new(),
            bodies: RigidBodySet::new(),
            colliders: ColliderSet::new(),
            impulse_joints: ImpulseJointSet::new(),
            multibody_joints: MultibodyJointSet::new(),
            soft_bodies: SoftBodySet::new(),
            ccd: CCDSolver::new(),
            gravity,
            tick_length: settings.tick_length,
            steps_per_tick: settings.steps_per_tick,
            direct_loops: Vec::new(),
            direct_iterations: 2,
            direct_regularization: 0.0,
            direct_exact_mass: false,
            cars: Vec::new(),
            step_time: Duration::ZERO,
            direct_time: Duration::ZERO,
            direct_failures: 0,
            closing_softness: None,
            closing_mismatch: Vector::ZERO,
            recentre: None,
            quantize: false,
        }
    }

    pub fn step(&mut self) {
        let started = Instant::now();
        self.pipeline.step(
            self.gravity,
            &self.parameters,
            &mut self.islands,
            &mut self.broad_phase,
            &mut self.narrow_phase,
            &mut self.bodies,
            &mut self.colliders,
            &mut self.impulse_joints,
            &mut self.multibody_joints,
            &mut self.soft_bodies,
            &mut self.ccd,
            &(),
            &(),
        );
        self.step_time += started.elapsed();
        if self.quantize {
            round_state_to_32_bits(self);
        }
        if !self.direct_loops.is_empty() {
            let started = Instant::now();
            let loops = self.direct_loops.clone();
            for spec in &loops {
                if !close_loop_directly(self, spec) {
                    self.direct_failures += 1;
                }
            }
            self.direct_time += started.elapsed();
        }
        hold_to_rail(self);
    }

    /// One tick: the steps, then the forces are forgotten (as `BodyWorld::step` does).
    pub fn tick(&mut self) {
        for _ in 0..self.steps_per_tick {
            self.step();
        }
        for (_, body) in self.bodies.iter_mut() {
            body.reset_forces(false);
            body.reset_torques(false);
        }
    }
}

/// Rounds the pose and velocities of every dynamic body to the nearest 32-bit float (a quaternion is renormalized afterwards, which puts back a few
/// digits below 32 bits; the error that matters is the one the rounding leaves, up to 6e-8 of each component).
fn round_state_to_32_bits(world: &mut World) {
    let round = |x: f64| x as f32 as f64;
    let round_vector = |v: Vector| Vector::new(round(v.x), round(v.y), round(v.z));
    for (_, body) in world.bodies.iter_mut() {
        if !body.is_dynamic() {
            continue;
        }
        let pose = *body.position();
        let q = pose.rotation;
        let rotation = Rotation::from_xyzw(round(q.x), round(q.y), round(q.z), round(q.w)).normalize();
        body.set_position(Pose::from_parts(round_vector(pose.translation), rotation), false);
        let (v, w) = (round_vector(body.linvel()), round_vector(body.angvel()));
        body.set_linvel(v, false);
        body.set_angvel(w, false);
    }
}

/// The velocity of every car as the integrator holds it: from the generalized velocities for a car in a multibody (the jacobian times the
/// generalized velocity, at the poses the bodies have now), else the body's own.
pub fn car_velocities(world: &World, cars: &[Car]) -> Vec<(Vector, Vector)> {
    cars.iter()
        .map(|car| {
            let body = &world.bodies[car.handle];
            let Some(link) = world.multibody_joints.rigid_body_link(car.handle).copied() else { return (body.linvel(), body.angvel()) };
            let Some(multibody) = world.multibody_joints.get_multibody(link.multibody) else { return (body.linvel(), body.angvel()) };
            let jacobian = multibody.body_jacobian(link.id);
            let q = multibody.generalized_velocity();
            let mut spatial = [0.0f64; 6];
            for row in 0..6 {
                for column in 0..multibody.ndofs() {
                    spatial[row] += jacobian[(row, column)] * q[column];
                }
            }
            (Vector::new(spatial[0], spatial[1], spatial[2]), Vector::new(spatial[3], spatial[4], spatial[5]))
        })
        .collect()
}

/// The rig's angular momentum about its centre, and its tensor of inertia about it (world axes), from the integrator's velocities.
pub fn rig_spin(world: &World, cars: &[Car]) -> (Vector, [[f64; 3]; 3]) {
    let velocities = car_velocities(world, cars);
    let t = totals(world, cars);
    let mean = velocities.iter().zip(cars).fold(Vector::ZERO, |sum, ((v, _), car)| sum + *v * car.mass) / t.mass;
    let mut spin = Vector::ZERO;
    let mut tensor = [[0.0f64; 3]; 3];
    for ((v, w), car) in velocities.iter().zip(cars) {
        let body = &world.bodies[car.handle];
        let rotation = *body.rotation();
        let arm = body.translation() - t.centre;
        let axes = [rotation * Vector::X, rotation * Vector::Y, rotation * Vector::Z];
        let inertia = [car.inertia.x, car.inertia.y, car.inertia.z];
        for k in 0..3 {
            spin += axes[k] * (inertia[k] * axes[k].dot(*w));
            let axis = [axes[k].x, axes[k].y, axes[k].z];
            for i in 0..3 {
                for j in 0..3 {
                    tensor[i][j] += inertia[k] * axis[i] * axis[j];
                }
            }
        }
        spin += arm.cross((*v - mean) * car.mass);
        let r = [arm.x, arm.y, arm.z];
        for i in 0..3 {
            for j in 0..3 {
                tensor[i][j] += car.mass * (if i == j { arm.dot(arm) } else { 0.0 } - r[i] * r[j]);
            }
        }
    }
    (spin, tensor)
}

/// Takes from every car the error of the centre's velocity (and, if asked, of its position and of the spin about it) against the rail, as one
/// common rigid shift: a rigid motion of the whole rig breaks no joint (the velocities of the two anchors of a joint change by the same amount,
/// because they are the same point), so the cars' relative motion and the loop are untouched. A multibody takes it through its root's first six
/// generalized velocities (the root is a free body whose translation is along the world's axes: its jacobian's linear columns are the identity,
/// read by a debug run) and first three coordinates. The error is measured on the generalized velocities, which are what the next step
/// integrates; the velocity a body holds is a copy from before the step's last move and differs from them a little (2e-6 of an error of 1e-5 m/s
/// in the first steps).
fn hold_to_rail(world: &mut World) {
    let dt = world.tick_length / world.steps_per_tick as f64;
    let Some(rail) = world.recentre.as_mut() else { return };
    rail.elapsed += dt;
    let rail = *rail;
    if rail.mode == RecentreMode::Off {
        return;
    }
    let cars = world.cars.clone();
    let t = totals_from_generalized(world, &cars).unwrap_or_else(|| totals(world, &cars));
    let centre = totals(world, &cars).centre;
    let momentum = rail.momentum + rail.force * rail.elapsed;
    let line = rail.centre + rail.momentum / t.mass * rail.elapsed + rail.force / t.mass * (0.5 * rail.elapsed * rail.elapsed);
    let velocity_error = (t.momentum - momentum) / t.mass;
    let position_error = centre - line;
    let with_position = rail.mode != RecentreMode::Velocity;
    let with_spin = rail.mode == RecentreMode::VelocityPositionAndSpin;
    let turn = if with_spin {
        let (spin, tensor) = rig_spin(world, &cars);
        let wanted = rail.spin - spin;
        solve3(&tensor, [wanted.x, wanted.y, wanted.z]).map_or(Vector::ZERO, |x| Vector::new(x[0], x[1], x[2]))
    } else {
        Vector::ZERO
    };
    // What every car's velocity changes by: the common shift, and the turn about the centre.
    let shifts: Vec<(Vector, Vector)> =
        cars.iter().map(|car| (turn.cross(world.bodies[car.handle].translation() - centre) - velocity_error, turn)).collect();
    for (car, (shift, spin_shift)) in cars.iter().zip(&shifts) {
        let Some(link) = world.multibody_joints.rigid_body_link(car.handle).copied() else { continue };
        if link.id != 0 {
            continue;
        }
        let World { multibody_joints, bodies, .. } = world;
        let Some(multibody) = multibody_joints.get_multibody_mut(link.multibody) else { continue };
        {
            let mut velocity = multibody.generalized_velocity_mut();
            velocity[0] += shift.x;
            velocity[1] += shift.y;
            velocity[2] += shift.z;
            velocity[3] += spin_shift.x;
            velocity[4] += spin_shift.y;
            velocity[5] += spin_shift.z;
        }
        if with_position {
            let mut displacement = vec![0.0f64; multibody.ndofs()];
            displacement[0] = -position_error.x;
            displacement[1] = -position_error.y;
            displacement[2] = -position_error.z;
            multibody.apply_displacements(&displacement);
        }
        multibody.forward_kinematics(bodies, false);
        multibody.update_rigid_bodies(bodies, true);
    }
    // The velocities the bodies hold follow the same shift (the multibody's links keep a copy that `update_rigid_bodies` does not refresh).
    for (car, (shift, spin_shift)) in cars.iter().zip(&shifts) {
        let in_multibody = world.multibody_joints.rigid_body_link(car.handle).is_some();
        let body = &mut world.bodies[car.handle];
        let velocity = body.linvel() + *shift;
        body.set_linvel(velocity, false);
        let spin = body.angvel() + *spin_shift;
        body.set_angvel(spin, false);
        if with_position && !in_multibody {
            let place = body.translation() - position_error;
            body.set_translation(place, false);
        }
    }
}

/// A car: a body with a mass and a principal tensor of inertia at its origin (which is its centre of mass).
#[derive(Clone, Copy)]
pub struct Car {
    pub handle: RigidBodyHandle,
    pub mass: f64,
    pub inertia: Vector,
}

/// A car shaped like a solid box with the given half extents.
pub fn add_car(world: &mut World, pose: Pose, half: Vector, mass: f64) -> Car {
    let inertia = Vector::new(
        mass / 3.0 * (half.y * half.y + half.z * half.z),
        mass / 3.0 * (half.x * half.x + half.z * half.z),
        mass / 3.0 * (half.x * half.x + half.y * half.y),
    );
    let handle = world.bodies.insert(
        RigidBodyBuilder::dynamic()
            .pose(pose)
            .can_sleep(false)
            .additional_mass_properties(MassProperties::new(Vector::ZERO, mass, inertia)),
    );
    Car { handle, mass, inertia }
}

#[derive(Clone, Copy, Debug)]
pub struct Totals {
    pub mass: f64,
    pub centre: Vector,
    pub momentum: Vector,
    pub angular_momentum: Vector,
    pub energy: f64,
}

/// What a set of cars carries, from the velocities their bodies hold.
pub fn totals(world: &World, cars: &[Car]) -> Totals {
    let mut t = Totals { mass: 0.0, centre: Vector::ZERO, momentum: Vector::ZERO, angular_momentum: Vector::ZERO, energy: 0.0 };
    for car in cars {
        let body = &world.bodies[car.handle];
        let (v, w, x) = (body.linvel(), body.angvel(), body.translation());
        let rotation = *body.rotation();
        let spin = rotation * (car.inertia * (rotation.inverse() * w));
        t.mass += car.mass;
        t.centre += x * car.mass;
        t.momentum += v * car.mass;
        t.angular_momentum += spin + x.cross(v) * car.mass;
        t.energy += 0.5 * car.mass * v.dot(v) + 0.5 * w.dot(spin);
    }
    t.centre /= t.mass;
    t
}

/// The same totals computed from the generalized velocities of the multibody that holds the cars (`J q'`), not from what the bodies hold.
/// Used to check that the two agree (`None` if a car is not in a multibody).
pub fn totals_from_generalized(world: &World, cars: &[Car]) -> Option<Totals> {
    let mut t = Totals { mass: 0.0, centre: Vector::ZERO, momentum: Vector::ZERO, angular_momentum: Vector::ZERO, energy: 0.0 };
    for car in cars {
        let link = *world.multibody_joints.rigid_body_link(car.handle)?;
        let multibody = world.multibody_joints.get_multibody(link.multibody)?;
        let jacobian = multibody.body_jacobian(link.id);
        let q = multibody.generalized_velocity();
        let mut spatial = [0.0f64; 6];
        for row in 0..6 {
            for column in 0..multibody.ndofs() {
                spatial[row] += jacobian[(row, column)] * q[column];
            }
        }
        let (v, w) = (Vector::new(spatial[0], spatial[1], spatial[2]), Vector::new(spatial[3], spatial[4], spatial[5]));
        let body = &world.bodies[car.handle];
        let (x, rotation) = (body.translation(), *body.rotation());
        let spin = rotation * (car.inertia * (rotation.inverse() * w));
        t.mass += car.mass;
        t.centre += x * car.mass;
        t.momentum += v * car.mass;
        t.angular_momentum += spin + x.cross(v) * car.mass;
        t.energy += 0.5 * car.mass * v.dot(v) + 0.5 * w.dot(spin);
    }
    t.centre /= t.mass;
    Some(t)
}

/// A pair of anchor points, one in each of two bodies, that a joint keeps together.
#[derive(Clone, Copy, Debug)]
pub struct LoopSpec {
    pub a: RigidBodyHandle,
    pub anchor_a: Vector,
    pub b: RigidBodyHandle,
    pub anchor_b: Vector,
}

pub fn gap(world: &World, spec: &LoopSpec) -> f64 {
    let pa = *world.bodies[spec.a].position() * spec.anchor_a;
    let pb = *world.bodies[spec.b].position() * spec.anchor_b;
    (pb - pa).length()
}

/// How a rig is held together.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The chain only, the closing joint left out: what the same cars do as an open tree (on multibody joints).
    TreeOnly,
    /// The same open chain on ordinary impulse joints.
    ImpulseOpen,
    /// Every joint an ordinary impulse joint, the ring closed.
    AllImpulse,
    /// The tree on multibody joints, the closing joint an impulse joint between two links of the one multibody (Rapier's own loop closure).
    MultibodyNative,
    /// The tree on multibody joints, the closing joint solved directly after every step by `close_loop_directly`.
    MultibodyDirect,
}

pub struct Rig {
    pub cars: Vec<Car>,
    pub pivots: Vec<LoopSpec>,
    pub closing: LoopSpec,
    pub ring: usize,
}

fn ball(a_anchor: Vector, b_anchor: Vector) -> SphericalJoint {
    SphericalJointBuilder::new().local_anchor1(a_anchor).local_anchor2(b_anchor).build()
}

/// A ball joint for a multibody. A multibody puts a child where the joint's coordinates say, not where its body happens to be, so the frames must be
/// chosen so that the zero coordinates are the pose the child is in now (found out by a ring that was straightened at its first step).
fn multibody_ball(parent: Pose, child: Pose, anchor_in_parent: Vector) -> SphericalJoint {
    let frame_in_parent = Pose::from_translation(anchor_in_parent);
    let frame_in_child = child.inverse() * parent * frame_in_parent;
    SphericalJointBuilder::new().local_frame1(frame_in_parent).local_frame2(frame_in_child).build()
}

/// A ring of `ring` cars (each a box of 3 x 1 x 1 m, joined end to end by ball joints) with a chain of `tail` more cars out from the first.
/// The ring's cars weigh `ring_mass` and the tail's `tail_mass`.
pub fn build_ring(world: &mut World, mode: Mode, ring: usize, tail: usize, ring_mass: f64, tail_mass: f64) -> Rig {
    let half = Vector::new(1.5, 0.5, 0.5);
    let start = Vector::new(-1.5, 0.0, 0.0);
    let end = Vector::new(1.5, 0.0, 0.0);
    let radius = 3.0 / (2.0 * (std::f64::consts::PI / ring as f64).sin());
    let vertex = |k: usize| {
        let angle = 2.0 * std::f64::consts::PI * (k % ring) as f64 / ring as f64;
        Vector::new(radius * angle.cos(), radius * angle.sin(), 0.0)
    };
    let mut cars: Vec<Car> = Vec::new();
    let mut poses: Vec<Pose> = Vec::new();
    for k in 0..ring {
        let (from, to) = (vertex(k), vertex(k + 1));
        let direction = (to - from) / 3.0;
        let rotation = Rotation::from_rotation_z(direction.y.atan2(direction.x));
        let pose = Pose::from_parts((from + to) * 0.5, rotation);
        poses.push(pose);
        cars.push(add_car(world, pose, half, ring_mass));
    }
    let outward = poses[0].translation.normalize();
    let attach = poses[0].translation + outward * 0.5;
    let tail_rotation = Rotation::from_rotation_z(outward.y.atan2(outward.x));
    for j in 0..tail {
        let pose = Pose::from_parts(attach + outward * (1.5 + 3.0 * j as f64), tail_rotation);
        poses.push(pose);
        cars.push(add_car(world, pose, half, tail_mass));
    }
    let mut pivots: Vec<LoopSpec> = Vec::new();
    let mut tree: Vec<LoopSpec> = Vec::new();
    for k in 0..ring - 1 {
        let spec = LoopSpec { a: cars[k].handle, anchor_a: end, b: cars[k + 1].handle, anchor_b: start };
        tree.push(spec);
    }
    if tail > 0 {
        let spec = LoopSpec { a: cars[0].handle, anchor_a: poses[0].inverse() * attach, b: cars[ring].handle, anchor_b: start };
        tree.push(spec);
        for j in 1..tail {
            tree.push(LoopSpec { a: cars[ring + j - 1].handle, anchor_a: end, b: cars[ring + j].handle, anchor_b: start });
        }
    }
    let closing = LoopSpec { a: cars[ring - 1].handle, anchor_a: end, b: cars[0].handle, anchor_b: start };
    pivots.extend(tree.iter().copied());
    pivots.push(closing);
    for spec in &tree {
        match mode {
            Mode::AllImpulse | Mode::ImpulseOpen => {
                world.impulse_joints.insert(spec.a, spec.b, ball(spec.anchor_a, spec.anchor_b), true);
            }
            _ => {
                let joint = multibody_ball(*world.bodies[spec.a].position(), *world.bodies[spec.b].position(), spec.anchor_a);
                world.multibody_joints.insert(spec.a, spec.b, joint, true).expect("a tree joint");
            }
        }
    }
    match mode {
        Mode::TreeOnly | Mode::ImpulseOpen => {}
        Mode::AllImpulse | Mode::MultibodyNative => {
            let mut joint = ball(closing.anchor_a, closing.anchor_b + world.closing_mismatch);
            if let Some(softness) = world.closing_softness {
                let _ = joint.set_softness(softness);
            }
            world.impulse_joints.insert(closing.a, closing.b, joint, true);
        }
        Mode::MultibodyDirect => world.direct_loops.push(closing),
    }
    world.cars = cars.clone();
    Rig { cars, pivots, closing, ring }
}

/// Gives every car the velocity of a rigid motion of the whole rig: the centre moves at `linear` and the rig turns at `angular` about it.
/// (Only the root of a multibody takes its velocity from its body; the others follow from their joints, which is what a rigid motion needs.)
pub fn start_moving(world: &mut World, cars: &[Car], linear: Vector, angular: Vector) {
    let centre = totals(world, cars).centre;
    for car in cars {
        let body = &mut world.bodies[car.handle];
        let at = body.translation();
        body.set_linvel(linear + angular.cross(at - centre), true);
        body.set_angvel(angular, true);
    }
    // A multibody ignores the velocity of its bodies: the root's velocity is its first six generalized velocities (found out by a rig whose push vanished).
    for car in cars {
        let Some(link) = world.multibody_joints.rigid_body_link(car.handle).copied() else { continue };
        if link.id != 0 {
            continue;
        }
        let (v, w) = (world.bodies[car.handle].linvel(), world.bodies[car.handle].angvel());
        if let Some(multibody) = world.multibody_joints.get_multibody_mut(link.multibody) {
            let mut velocity = multibody.generalized_velocity_mut();
            velocity[0] = v.x;
            velocity[1] = v.y;
            velocity[2] = v.z;
            velocity[3] = w.x;
            velocity[4] = w.y;
            velocity[5] = w.z;
        }
    }
}

/// Solves the loop closure of `spec` directly, beside Rapier, on the multibody that holds both of its bodies: the relative velocity of the two
/// anchor points is removed with the impulse that costs the least kinetic energy, and the gap is closed with the smallest displacement of the
/// joint coordinates in the mass metric. Each is the 3 x 3 system `(J M^-1 J^T) x = rhs`, with `M^-1` taken from Rapier's own factorization of the
/// multibody's mass matrix (it was made at the start of the step, and includes the step's gyroscopic and damping terms, so it is not exactly the
/// mass matrix of the end of the step: that mismatch is one of the things the probe measures). Returns false if the system could not be solved.
pub fn close_loop_directly(world: &mut World, spec: &LoopSpec) -> bool {
    let Some(link_a) = world.multibody_joints.rigid_body_link(spec.a).copied() else { return false };
    let Some(link_b) = world.multibody_joints.rigid_body_link(spec.b).copied() else { return false };
    let regularization = world.direct_regularization;
    let iterations = world.direct_iterations;
    let exact = world.direct_exact_mass;
    let cars = world.cars.clone();
    let car_links: Vec<Option<usize>> = cars.iter().map(|c| world.multibody_joints.rigid_body_link(c.handle).map(|l| l.id)).collect();
    let World { multibody_joints, bodies, .. } = world;
    let Some(multibody) = multibody_joints.get_multibody_mut(link_a.multibody) else { return false };
    let mut all_solved = true;
    for _ in 0..iterations {
        multibody.forward_kinematics(bodies, false);
        let n = multibody.ndofs();
        let pose_a = *multibody.link(link_a.id).unwrap().local_to_world();
        let pose_b = *multibody.link(link_b.id).unwrap().local_to_world();
        let point_a = pose_a * spec.anchor_a;
        let point_b = pose_b * spec.anchor_b;
        let gap = point_b - point_a;
        let arm_a = point_a - pose_a.translation;
        let arm_b = point_b - pose_b.translation;
        // Rows of J: the velocity of anchor b minus that of anchor a, for a unit velocity of each generalized coordinate.
        let mut rows = vec![vec![0.0f64; n]; 3];
        {
            let ja = multibody.body_jacobian(link_a.id);
            let jb = multibody.body_jacobian(link_b.id);
            for c in 0..n {
                let va = Vector::new(ja[(0, c)], ja[(1, c)], ja[(2, c)]) + Vector::new(ja[(3, c)], ja[(4, c)], ja[(5, c)]).cross(arm_a);
                let vb = Vector::new(jb[(0, c)], jb[(1, c)], jb[(2, c)]) + Vector::new(jb[(3, c)], jb[(4, c)], jb[(5, c)]).cross(arm_b);
                let d = vb - va;
                rows[0][c] = d.x;
                rows[1][c] = d.y;
                rows[2][c] = d.z;
            }
        }
        // Z = M^-1 J^T, one column per row of J.
        let mut z: Vec<DVector> = Vec::with_capacity(3);
        if exact {
            let mut m = DMatrix::zeros(n, n);
            for (car, link) in cars.iter().zip(&car_links) {
                let Some(link) = *link else { continue };
                let jacobian = multibody.body_jacobian(link);
                let Some(link_ref) = multibody.link(link) else { continue };
                let rotation = link_ref.local_to_world().rotation;
                let axes = [rotation * Vector::X, rotation * Vector::Y, rotation * Vector::Z];
                let inertia_times = |w: Vector| -> Vector { axes[0] * (car.inertia.x * axes[0].dot(w)) + axes[1] * (car.inertia.y * axes[1].dot(w)) + axes[2] * (car.inertia.z * axes[2].dot(w)) };
                for a in 0..n {
                    let (la, aa) = (Vector::new(jacobian[(0, a)], jacobian[(1, a)], jacobian[(2, a)]), Vector::new(jacobian[(3, a)], jacobian[(4, a)], jacobian[(5, a)]));
                    let (force_a, torque_a) = (la * car.mass, inertia_times(aa));
                    for b in 0..n {
                        let (lb, ab) = (Vector::new(jacobian[(0, b)], jacobian[(1, b)], jacobian[(2, b)]), Vector::new(jacobian[(3, b)], jacobian[(4, b)], jacobian[(5, b)]));
                        m[(a, b)] += force_a.dot(lb) + torque_a.dot(ab);
                    }
                }
            }
            let Some(cholesky) = m.cholesky() else { return false };
            for row in &rows {
                z.push(cholesky.solve(&DVector::from_row_slice(row)));
            }
        } else {
            for row in &rows {
                let rhs = DVector::from_row_slice(row);
                match multibody.inv_augmented_mass().solve(&rhs) {
                    Some(solution) => z.push(solution),
                    None => return false,
                }
            }
        }
        let mut a = [[0.0f64; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                a[i][j] = (0..n).map(|c| rows[i][c] * z[j][c]).sum::<f64>() + if i == j { regularization } else { 0.0 };
            }
        }
        let qdot = multibody.generalized_velocity();
        let u = [0, 1, 2].map(|i| (0..n).map(|c| rows[i][c] * qdot[c]).sum::<f64>());
        let Some(lambda) = solve3(&a, [-u[0], -u[1], -u[2]]) else {
            all_solved = false;
            break;
        };
        let Some(mu) = solve3(&a, [-gap.x, -gap.y, -gap.z]) else {
            all_solved = false;
            break;
        };
        {
            let mut velocity = multibody.generalized_velocity_mut();
            for c in 0..n {
                velocity[c] += z[0][c] * lambda[0] + z[1][c] * lambda[1] + z[2][c] * lambda[2];
            }
        }
        let displacement: Vec<f64> = (0..n).map(|c| z[0][c] * mu[0] + z[1][c] * mu[1] + z[2][c] * mu[2]).collect();
        multibody.apply_displacements(&displacement);
    }
    multibody.forward_kinematics(bodies, false);
    multibody.update_rigid_bodies(bodies, true);
    all_solved
}

/// Solves a 3 x 3 system by Gaussian elimination with partial pivoting; `None` if it is singular (a pivot below 1e-12 of the largest).
pub fn solve3(a: &[[f64; 3]; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    let mut m = [[0.0f64; 4]; 3];
    for i in 0..3 {
        m[i][..3].copy_from_slice(&a[i]);
        m[i][3] = b[i];
    }
    let scale = a.iter().flatten().fold(0.0f64, |s, v| s.max(v.abs()));
    for col in 0..3 {
        let pivot = (col..3).max_by(|&x, &y| m[x][col].abs().total_cmp(&m[y][col].abs())).unwrap();
        if m[pivot][col].abs() <= 1e-12 * scale.max(1e-300) {
            return None;
        }
        m.swap(col, pivot);
        for row in col + 1..3 {
            let factor = m[row][col] / m[col][col];
            for k in col..4 {
                m[row][k] -= factor * m[col][k];
            }
        }
    }
    let mut x = [0.0f64; 3];
    for i in (0..3).rev() {
        let mut s = m[i][3];
        for k in i + 1..3 {
            s -= m[i][k] * x[k];
        }
        x[i] = s / m[i][i];
    }
    Some(x)
}

/// The 3 x 3 effective mass `A = J M^-1 J^T` of the closing constraint of `spec`, two ways: with Rapier's factorization of its (augmented) mass matrix,
/// and with the true mass matrix `M = sum_i J_i^T W_i J_i` built from the body jacobians and the cars' mass properties. `None` if the bodies are not
/// in one multibody.
pub fn effective_mass(world: &mut World, cars: &[Car], spec: &LoopSpec) -> Option<([[f64; 3]; 3], [[f64; 3]; 3])> {
    let link_a = world.multibody_joints.rigid_body_link(spec.a).copied()?;
    let link_b = world.multibody_joints.rigid_body_link(spec.b).copied()?;
    let car_links: Vec<Option<usize>> = cars.iter().map(|c| world.multibody_joints.rigid_body_link(c.handle).map(|l| l.id)).collect();
    let World { multibody_joints, bodies, .. } = world;
    let multibody = multibody_joints.get_multibody_mut(link_a.multibody)?;
    multibody.forward_kinematics(bodies, false);
    let n = multibody.ndofs();
    let pose_a = *multibody.link(link_a.id)?.local_to_world();
    let pose_b = *multibody.link(link_b.id)?.local_to_world();
    let arm_a = pose_a * spec.anchor_a - pose_a.translation;
    let arm_b = pose_b * spec.anchor_b - pose_b.translation;
    let mut rows = vec![vec![0.0f64; n]; 3];
    {
        let ja = multibody.body_jacobian(link_a.id);
        let jb = multibody.body_jacobian(link_b.id);
        for c in 0..n {
            let va = Vector::new(ja[(0, c)], ja[(1, c)], ja[(2, c)]) + Vector::new(ja[(3, c)], ja[(4, c)], ja[(5, c)]).cross(arm_a);
            let vb = Vector::new(jb[(0, c)], jb[(1, c)], jb[(2, c)]) + Vector::new(jb[(3, c)], jb[(4, c)], jb[(5, c)]).cross(arm_b);
            let d = vb - va;
            rows[0][c] = d.x;
            rows[1][c] = d.y;
            rows[2][c] = d.z;
        }
    }
    let mut rapier = [[0.0f64; 3]; 3];
    let mut z: Vec<DVector> = Vec::new();
    for row in &rows {
        z.push(multibody.inv_augmented_mass().solve(&DVector::from_row_slice(row))?);
    }
    for i in 0..3 {
        for j in 0..3 {
            rapier[i][j] = (0..n).map(|c| rows[i][c] * z[j][c]).sum();
        }
    }
    // True mass matrix.
    let mut m = DMatrix::zeros(n, n);
    for (car, link) in cars.iter().zip(car_links) {
        let Some(link) = link else { continue };
        let jacobian = multibody.body_jacobian(link);
        let rotation = multibody.link(link)?.local_to_world().rotation;
        // World inertia tensor columns: R diag(I) R^T.
        let axes = [rotation * Vector::X, rotation * Vector::Y, rotation * Vector::Z];
        let inertia_times = |w: Vector| -> Vector { axes[0] * (car.inertia.x * axes[0].dot(w)) + axes[1] * (car.inertia.y * axes[1].dot(w)) + axes[2] * (car.inertia.z * axes[2].dot(w)) };
        for a in 0..n {
            let (la, aa) = (Vector::new(jacobian[(0, a)], jacobian[(1, a)], jacobian[(2, a)]), Vector::new(jacobian[(3, a)], jacobian[(4, a)], jacobian[(5, a)]));
            let weighted_a = (la * car.mass, inertia_times(aa));
            for b in 0..n {
                let (lb, ab) = (Vector::new(jacobian[(0, b)], jacobian[(1, b)], jacobian[(2, b)]), Vector::new(jacobian[(3, b)], jacobian[(4, b)], jacobian[(5, b)]));
                m[(a, b)] += weighted_a.0.dot(lb) + weighted_a.1.dot(ab);
            }
        }
    }
    let inverse = m.clone().try_inverse()?;
    let mut truth = [[0.0f64; 3]; 3];
    for i in 0..3 {
        let zi = &inverse * DVector::from_row_slice(&rows[i]);
        for j in 0..3 {
            truth[j][i] = (0..n).map(|c| rows[j][c] * zi[c]).sum();
        }
    }
    Some((rapier, truth))
}

/// The three rows of the closing constraint's jacobian (anchor b's velocity minus anchor a's, per generalized velocity) at the current pose.
pub fn closing_rows(world: &mut World, spec: &LoopSpec) -> Option<Vec<Vec<f64>>> {
    let link_a = world.multibody_joints.rigid_body_link(spec.a).copied()?;
    let link_b = world.multibody_joints.rigid_body_link(spec.b).copied()?;
    let World { multibody_joints, bodies, .. } = world;
    let multibody = multibody_joints.get_multibody_mut(link_a.multibody)?;
    multibody.forward_kinematics(bodies, false);
    let n = multibody.ndofs();
    let pose_a = *multibody.link(link_a.id)?.local_to_world();
    let pose_b = *multibody.link(link_b.id)?.local_to_world();
    let arm_a = pose_a * spec.anchor_a - pose_a.translation;
    let arm_b = pose_b * spec.anchor_b - pose_b.translation;
    let mut rows = vec![vec![0.0f64; n]; 3];
    let ja = multibody.body_jacobian(link_a.id);
    let jb = multibody.body_jacobian(link_b.id);
    for c in 0..n {
        let va = Vector::new(ja[(0, c)], ja[(1, c)], ja[(2, c)]) + Vector::new(ja[(3, c)], ja[(4, c)], ja[(5, c)]).cross(arm_a);
        let vb = Vector::new(jb[(0, c)], jb[(1, c)], jb[(2, c)]) + Vector::new(jb[(3, c)], jb[(4, c)], jb[(5, c)]).cross(arm_b);
        let d = vb - va;
        rows[0][c] = d.x;
        rows[1][c] = d.y;
        rows[2][c] = d.z;
    }
    Some(rows)
}
