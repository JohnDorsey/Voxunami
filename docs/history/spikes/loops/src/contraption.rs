//! A kit for comparing ways to hand a hit back to a mechanism (the study of `docs/history/loop-probe.md`, section 15; the owner's idea of 2026-11-13).
//!
//! A *scene* is cars (bodies with one collider each, in *groups*: a group is a mechanism with its own joints, a ship), joints (fixed, ball, hinge, slider, the last two with
//! limits and motors that stand for doors, landing gear and wheels), a ground, a gravity and a few scripted pushes and motor changes. A *scheme* decides how it is stepped:
//!
//! - `Hi`, `Game`: one world with everything, at a converged setting and at the game's (what `BodyWorld` does today).
//! - `Proxy`: two kinds of world. Each group has a *joint world* (its cars and joints, no colliders; the group's own frame, the truth). One *contact world* holds the ground and
//!   a *proxy* body per set of cars (`Partition`: the whole group as one body, each rigid cluster, each car), re-posed from the truth every step. The contact world is stepped, and
//!   what it found is given back to the truth (`Handback`): the proxies' change of velocity as one rigid motion, or the impulses of the contact pairs, at the cars that were touched.
//!   `rounds` is the number of extra passes of block Gauss-Seidel between the two kinds of world (each is a predicted step of the joint worlds).
//! - `Ghost`: the contact world is a full copy of the one world (cars, joints, motors, colliders, ground) at the game's setting, synced from the truth every step. Only the
//!   *contact impulses* are read from it (never its joint state) and given to the truth, whose joint worlds do the mechanism in the group's own frame.
//!
//! Gravity is uniform, so it is in the joint worlds as a force on every car, and in the contact world only as far as the contact needs to see the approach it causes (a world
//! in free fall and a world with a uniform pull are the same to the joints, by Galilean invariance).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use rapier3d::math::Matrix;
use rapier3d::utils::OrthonormalBasis;

use crate::*;

#[derive(Clone, Copy, Debug)]
pub enum Shape {
    Ball(f64),
    /// Half extents.
    Cuboid(Vector),
}

impl Shape {
    fn builder(self) -> ColliderBuilder {
        match self {
            Shape::Ball(radius) => ColliderBuilder::ball(radius),
            Shape::Cuboid(half) => ColliderBuilder::cuboid(half.x, half.y, half.z),
        }
    }

    /// The principal moments of inertia of a solid of this shape (about its centre, in its own axes).
    pub fn inertia(self, mass: f64) -> Vector {
        match self {
            Shape::Ball(radius) => Vector::splat(0.4 * mass * radius * radius),
            Shape::Cuboid(h) => Vector::new(mass / 3.0 * (h.y * h.y + h.z * h.z), mass / 3.0 * (h.x * h.x + h.z * h.z), mass / 3.0 * (h.x * h.x + h.y * h.y)),
        }
    }
}

/// A joint motor in Rapier's force-based model: a spring and a damper toward a target, and at most `max_force` (N, or N m for a hinge).
#[derive(Clone, Copy, Debug)]
pub struct Motor {
    pub target_position: f64,
    pub target_velocity: f64,
    pub stiffness: f64,
    pub damping: f64,
    pub max_force: f64,
}

impl Motor {
    /// A spring and damper that hold a position.
    pub fn hold(target_position: f64, stiffness: f64, damping: f64, max_force: f64) -> Self {
        Self { target_position, target_velocity: 0.0, stiffness, damping, max_force }
    }

    /// A drive: it pulls toward a speed with at most `max_force`.
    pub fn drive(target_velocity: f64, damping: f64, max_force: f64) -> Self {
        Self { target_position: 0.0, target_velocity, stiffness: 0.0, damping, max_force }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum JointKind {
    Fixed,
    Ball,
    /// One axis of turning, `axis` in both cars' own frames.
    Hinge,
    /// One axis of sliding, `axis` in both cars' own frames.
    Slider,
}

#[derive(Clone, Debug)]
pub struct JointSpec {
    pub a: usize,
    pub b: usize,
    pub anchor_a: Vector,
    pub anchor_b: Vector,
    pub kind: JointKind,
    pub axis: Vector,
    pub limits: Option<[f64; 2]>,
    pub motor: Option<Motor>,
}

impl JointSpec {
    pub fn new(kind: JointKind, a: usize, anchor_a: Vector, b: usize, anchor_b: Vector) -> Self {
        Self { a, b, anchor_a, anchor_b, kind, axis: Vector::X, limits: None, motor: None }
    }

    pub fn axis(mut self, axis: Vector) -> Self {
        self.axis = axis;
        self
    }

    pub fn limits(mut self, low: f64, high: f64) -> Self {
        self.limits = Some([low, high]);
        self
    }

    pub fn motor(mut self, motor: Motor) -> Self {
        self.motor = Some(motor);
        self
    }
}

#[derive(Clone, Debug)]
pub struct CarSpec {
    pub name: String,
    pub group: usize,
    pub shape: Shape,
    pub mass: f64,
    pub pose: Pose,
    pub linvel: Vector,
    pub angvel: Vector,
    pub friction: f64,
    /// The principal moments of inertia, if not those of a solid of the shape.
    pub inertia: Option<Vector>,
    /// Who yields in a contact between two cars of different groups, when the priority rule is on: the car of the higher priority is not given the impulse (a thing
    /// inside a ship does not move the ship).
    pub priority: i8,
}

impl CarSpec {
    pub fn new(name: &str, group: usize, shape: Shape, mass: f64, place: Vector) -> Self {
        Self { name: name.to_string(), group, shape, mass, pose: Pose::from_translation(place), linvel: Vector::ZERO, angvel: Vector::ZERO, friction: 0.8, inertia: None, priority: 0 }
    }

    pub fn with_inertia(mut self, inertia: Vector) -> Self {
        self.inertia = Some(inertia);
        self
    }

    pub fn priority(mut self, priority: i8) -> Self {
        self.priority = priority;
        self
    }

    pub fn principal_inertia(&self) -> Vector {
        self.inertia.unwrap_or_else(|| self.shape.inertia(self.mass))
    }

    pub fn moving(mut self, linvel: Vector) -> Self {
        self.linvel = linvel;
        self
    }

    pub fn friction(mut self, friction: f64) -> Self {
        self.friction = friction;
        self
    }
}

#[derive(Clone, Debug)]
pub struct Ground {
    pub shape: Shape,
    pub pose: Pose,
    pub friction: f64,
}

/// A constant force on a car's centre from `from` to `until` seconds (an engine).
#[derive(Clone, Debug)]
pub struct Push {
    pub car: usize,
    pub force: Vector,
    pub from: f64,
    pub until: f64,
}

/// At `at` seconds the motor of `joint` is replaced (a door told to open).
#[derive(Clone, Debug)]
pub struct MotorChange {
    pub at: f64,
    pub joint: usize,
    pub motor: Motor,
}

#[derive(Clone, Debug)]
pub struct Scene {
    pub title: String,
    pub cars: Vec<CarSpec>,
    pub joints: Vec<JointSpec>,
    pub ground: Vec<Ground>,
    pub gravity: Vector,
    pub pushes: Vec<Push>,
    pub changes: Vec<MotorChange>,
    pub seconds: f64,
    /// Joints that are to be taken as rigid where a scheme welds the rigid parts of a mechanism into one body (fixed joints always are; this is for a joint that is
    /// immobilized or locked: it still moves in the group's own world).
    pub welds: Vec<usize>,
}

impl Scene {
    pub fn new(title: &str, seconds: f64) -> Self {
        Self { title: title.to_string(), cars: Vec::new(), joints: Vec::new(), ground: Vec::new(), gravity: Vector::ZERO, pushes: Vec::new(), changes: Vec::new(), seconds, welds: Vec::new() }
    }

    pub fn add_car(&mut self, car: CarSpec) -> usize {
        self.cars.push(car);
        self.cars.len() - 1
    }

    pub fn add_joint(&mut self, joint: JointSpec) -> usize {
        self.joints.push(joint);
        self.joints.len() - 1
    }

    pub fn groups(&self) -> usize {
        self.cars.iter().map(|c| c.group + 1).max().unwrap_or(0)
    }

    pub fn cars_of(&self, group: usize) -> Vec<usize> {
        (0..self.cars.len()).filter(|&i| self.cars[i].group == group).collect()
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Partition {
    /// One proxy for each group.
    Whole,
    /// One proxy for each set of cars joined by fixed joints.
    Clusters,
    PerCar,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Handback {
    /// The change of each proxy's velocity, as one rigid motion of its cars.
    Rigid,
    /// The impulses of the contact pairs, at the cars they touched, at the points.
    Impulse,
}

/// What a proxy is told of the load its car's joints carry (the contact pass cannot see a load that comes through a joint).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Feedback {
    None,
    /// The net joint force of the last step.
    Last,
    /// Only the part that the last two steps agree on (per component: the smaller if the signs are the same, else none), so that a shock is not fed back.
    Persistent,
    /// A running average of the last steps' forces: `new = alpha * last + (1 - alpha) * used before`.
    Relaxed(f64),
    /// `Last`, but only the part along the direction the car's contacts pushed it in the last step (a support load), and no torque.
    LastNormal,
    /// `Persistent`, but only the part along the direction the car's contacts pushed it in the last step, and no torque.
    PersistentNormal,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Scheme {
    Hi,
    /// Twice as fine as `Hi` (steps, substeps, passes): what the check of `Hi`'s own convergence compares it with.
    HiFiner,
    /// Twice as fine again: the check of `HiFiner`.
    HiFinest,
    Game,
    /// `feedback`: each car's net joint force of the last step (from its change of velocity) is put on its proxy as an outside force, so that the contact pass sees the load a joint carries.
    Proxy { partition: Partition, handback: Handback, rounds: usize, feedback: Feedback },
    Ghost,
    /// Two ghost worlds, one with the contacts and one without; what the contacts did (the difference of the two, in places and velocities) is given to the truth.
    GhostDifference,
    /// `Ghost`, with the rigid parts (fixed joints, and the joints of `Scene::welds`) of every group welded into one body in the ghost.
    GhostWelded,
    /// `GhostDifference`, with the rigid parts welded in both ghosts.
    GhostWeldedDifference,
}

impl Scheme {
    pub fn name(self) -> String {
        match self {
            Scheme::Hi => "one world, converged".to_string(),
            Scheme::HiFiner => "one world, finer than converged".to_string(),
            Scheme::HiFinest => "one world, finest".to_string(),
            Scheme::Game => "one world, game (today)".to_string(),
            Scheme::Ghost => "ghost world, impulses".to_string(),
            Scheme::GhostDifference => "ghost worlds, difference".to_string(),
            Scheme::GhostWelded => "welded ghost, impulses".to_string(),
            Scheme::GhostWeldedDifference => "welded ghosts, difference".to_string(),
            Scheme::Proxy { partition, handback, rounds, feedback } => {
                let part = match partition {
                    Partition::Whole => "whole",
                    Partition::Clusters => "cluster",
                    Partition::PerCar => "per car",
                };
                let back = match handback {
                    Handback::Rigid => "rigid",
                    Handback::Impulse => "impulse",
                };
                let load = match feedback {
                    Feedback::None => String::new(),
                    Feedback::Last => ", load".to_string(),
                    Feedback::Persistent => ", load (persistent)".to_string(),
                    Feedback::Relaxed(alpha) => format!(", load (relaxed {alpha})"),
                    Feedback::LastNormal => ", load (normal)".to_string(),
                    Feedback::PersistentNormal => ", load (persistent, normal)".to_string(),
                };
                if rounds == 0 { format!("{part}, {back}{load}") } else { format!("{part}, {back}{load}, +{rounds} rounds") }
            }
        }
    }
}

pub const WHOLE_RIGID: Scheme = Scheme::Proxy { partition: Partition::Whole, handback: Handback::Rigid, rounds: 0, feedback: Feedback::None };
pub const WHOLE_IMPULSE: Scheme = Scheme::Proxy { partition: Partition::Whole, handback: Handback::Impulse, rounds: 0, feedback: Feedback::None };
pub const CLUSTER_IMPULSE: Scheme = Scheme::Proxy { partition: Partition::Clusters, handback: Handback::Impulse, rounds: 0, feedback: Feedback::None };
pub const PER_CAR_LOAD: Scheme = Scheme::Proxy { partition: Partition::PerCar, handback: Handback::Impulse, rounds: 0, feedback: Feedback::Last };
pub const WHOLE_LOAD: Scheme = Scheme::Proxy { partition: Partition::Whole, handback: Handback::Impulse, rounds: 0, feedback: Feedback::Last };
pub const PER_CAR: Scheme = Scheme::Proxy { partition: Partition::PerCar, handback: Handback::Impulse, rounds: 0, feedback: Feedback::None };

pub fn hi_settings() -> Settings {
    Settings { steps_per_tick: 12, substeps: 16, pgs_iterations: 32, ..Settings::default() }
}

/// Twice as fine as `hi_settings` in every count (the convergence check of the reference).
pub fn finer_settings() -> Settings {
    Settings { steps_per_tick: 24, substeps: 32, pgs_iterations: 64, ..Settings::default() }
}

/// Twice as fine again.
pub fn finest_settings() -> Settings {
    Settings { steps_per_tick: 48, substeps: 64, pgs_iterations: 128, ..Settings::default() }
}

#[derive(Clone, Copy, Debug)]
pub struct CarState {
    pub pose: Pose,
    pub linvel: Vector,
    pub angvel: Vector,
}

fn car_filter(group: usize) -> InteractionGroups {
    let own = Group::from_bits_truncate(1 << group);
    InteractionGroups::new(own, Group::ALL.difference(own), InteractionTestMode::And)
}

fn ground_filter() -> InteractionGroups {
    InteractionGroups::new(Group::GROUP_32, Group::ALL, InteractionTestMode::And)
}

/// A car that meets the ground and no other car (not even one of another group).
fn car_isolated_filter(group: usize) -> InteractionGroups {
    InteractionGroups::new(Group::from_bits_truncate(1 << group), Group::GROUP_32, InteractionTestMode::And)
}

fn axis_of(kind: JointKind) -> JointAxis {
    if kind == JointKind::Slider { JointAxis::LinX } else { JointAxis::AngX }
}

fn make_joint(spec: &JointSpec) -> GenericJoint {
    match spec.kind {
        JointKind::Fixed => FixedJointBuilder::new()
            .local_frame1(Pose::from_translation(spec.anchor_a))
            .local_frame2(Pose::from_translation(spec.anchor_b))
            .contacts_enabled(false)
            .build()
            .into(),
        JointKind::Ball => SphericalJointBuilder::new().local_anchor1(spec.anchor_a).local_anchor2(spec.anchor_b).contacts_enabled(false).build().into(),
        JointKind::Hinge => {
            let mut builder = RevoluteJointBuilder::new(spec.axis).local_anchor1(spec.anchor_a).local_anchor2(spec.anchor_b).contacts_enabled(false);
            if let Some(limits) = spec.limits {
                builder = builder.limits(limits);
            }
            if let Some(m) = spec.motor {
                builder = builder.motor_model(MotorModel::ForceBased).motor(m.target_position, m.target_velocity, m.stiffness, m.damping);
                if m.max_force.is_finite() {
                    builder = builder.motor_max_force(m.max_force);
                }
            }
            builder.build().into()
        }
        JointKind::Slider => {
            let mut builder = PrismaticJointBuilder::new(spec.axis).local_anchor1(spec.anchor_a).local_anchor2(spec.anchor_b).contacts_enabled(false);
            if let Some(limits) = spec.limits {
                builder = builder.limits(limits);
            }
            if let Some(m) = spec.motor {
                builder = builder.motor_model(MotorModel::ForceBased).set_motor(m.target_position, m.target_velocity, m.stiffness, m.damping);
                if m.max_force.is_finite() {
                    builder = builder.motor_max_force(m.max_force);
                }
            }
            builder.build().into()
        }
    }
}

/// A world with some of a scene's cars (and the joints between them), and optionally colliders and the ground.
pub struct Built {
    pub world: World,
    pub bodies: Vec<Option<RigidBodyHandle>>,
    pub colliders: Vec<Option<ColliderHandle>>,
    pub joints: Vec<Option<ImpulseJointHandle>>,
    /// Sets of cars welded into one body (empty: every car is its own body).
    pub clusters: Vec<Cluster>,
}

/// Cars welded into one body: the body's frame is that of the first member, the others sit at fixed offsets from it.
pub struct Cluster {
    pub body: RigidBodyHandle,
    pub members: Vec<usize>,
    pub offsets: Vec<Vector>,
    pub local_com: Vector,
}

impl Built {
    /// Puts the cars' states into the world (a welded body takes the first member's place and the members' momenta).
    fn sync(&mut self, scene: &Scene, truth: &[CarState]) {
        if self.clusters.is_empty() {
            for (i, state) in truth.iter().enumerate() {
                if let Some(handle) = self.bodies[i] {
                    write_body(&mut self.world, handle, *state);
                }
            }
            return;
        }
        let velocities: Vec<(Vector, Vector)> = truth.iter().map(|s| (s.linvel, s.angvel)).collect();
        for cluster in &self.clusters {
            let l = lump(scene, &cluster.members, truth, &velocities);
            let body = &mut self.world.bodies[cluster.body];
            body.set_position(truth[cluster.members[0]].pose, false);
            body.set_linvel(l.velocity, false);
            body.set_angvel(l.turn, false);
        }
    }

    /// Every car's state, in the scene's order.
    fn states(&self, scene: &Scene) -> Vec<CarState> {
        if self.clusters.is_empty() {
            return self.bodies.iter().map(|b| read_body(&self.world, b.expect("body"))).collect();
        }
        let mut out = vec![CarState { pose: Pose::IDENTITY, linvel: Vector::ZERO, angvel: Vector::ZERO }; scene.cars.len()];
        for cluster in &self.clusters {
            let body = &self.world.bodies[cluster.body];
            let pose = *body.position();
            let centre = pose.transform_point(cluster.local_com);
            for (&i, &offset) in cluster.members.iter().zip(&cluster.offsets) {
                let place = pose.transform_point(offset);
                out[i] = CarState { pose: Pose::from_parts(place, pose.rotation), linvel: body.linvel() + body.angvel().cross(place - centre), angvel: body.angvel() };
            }
        }
        out
    }

    /// Sets the pushes (outside forces on cars) on the world's bodies.
    fn set_pushes(&mut self, scene: &Scene, truth: &[CarState], forces: &[Vector]) {
        if self.clusters.is_empty() {
            let none = vec![(Vector::ZERO, Vector::ZERO); forces.len()];
            Sim::apply_forces(&mut self.world, &self.bodies, forces, &none);
            return;
        }
        let velocities: Vec<(Vector, Vector)> = truth.iter().map(|s| (s.linvel, s.angvel)).collect();
        for cluster in &self.clusters {
            let l = lump(scene, &cluster.members, truth, &velocities);
            let (mut force, mut torque) = (Vector::ZERO, Vector::ZERO);
            for &i in &cluster.members {
                force += forces[i];
                torque += (truth[i].pose.translation - l.centre).cross(forces[i]);
            }
            let body = &mut self.world.bodies[cluster.body];
            body.reset_forces(false);
            body.reset_torques(false);
            if force.length_squared() > 0.0 {
                body.add_force(force, true);
            }
            if torque.length_squared() > 0.0 {
                body.add_torque(torque, true);
            }
        }
    }

    /// Extra solver work for the islands these bodies are in (substeps, and passes inside each).
    fn set_effort(&mut self, effort: (usize, usize)) {
        for (_, body) in self.world.bodies.iter_mut() {
            body.set_additional_solver_iterations(effort.0);
            body.set_additional_pgs_iterations(effort.1);
        }
    }
}

/// The cars joined by fixed joints (and the joints of `Scene::welds`), as sets.
pub fn weld_sets(scene: &Scene) -> Vec<Vec<usize>> {
    clusters_of(scene)
}

/// The contact copy of the world with every set of rigidly joined cars welded into one body (the cars must start unrotated). Joints between sets are kept, with their
/// anchors put in the sets' frames.
pub fn build_welded(scene: &Scene, settings: Settings, colliders: bool, ground: bool) -> Built {
    let mut world = World::new(settings, scene.gravity);
    world.parameters.friction_model = FrictionModel::Coulomb;
    let n = scene.cars.len();
    let mut bodies = vec![None; n];
    let mut collider_handles = vec![None; n];
    let mut clusters = Vec::new();
    let mut offset_of = vec![Vector::ZERO; n];
    for members in weld_sets(scene) {
        let reference = members[0];
        let origin = scene.cars[reference].pose.translation;
        let mass: f64 = members.iter().map(|&i| scene.cars[i].mass).sum();
        let com = members.iter().fold(Vector::ZERO, |sum, &i| sum + scene.cars[i].pose.translation * scene.cars[i].mass) / mass;
        let mut inertia = Matrix::ZERO;
        for &i in &members {
            let car = &scene.cars[i];
            assert!(car.pose.rotation.angle_between(Rotation::IDENTITY) < 1e-9, "welded cars must start unrotated");
            let arm = car.pose.translation - com;
            inertia += world_inertia(car, &car.pose) + (Matrix::IDENTITY * arm.dot(arm) - outer(arm, arm)) * car.mass;
        }
        let properties = MassProperties::with_inertia_matrix(com - origin, mass, inertia);
        let body = world.bodies.insert(RigidBodyBuilder::dynamic().pose(Pose::from_translation(origin)).can_sleep(false).additional_mass_properties(properties));
        let mut offsets = Vec::new();
        for &i in &members {
            let car = &scene.cars[i];
            let offset = car.pose.translation - origin;
            offset_of[i] = offset;
            offsets.push(offset);
            bodies[i] = Some(body);
            if colliders {
                let collider = world.colliders.insert_with_parent(
                    car.shape.builder().density(0.0).friction(car.friction).restitution(0.0).collision_groups(car_filter(car.group)).position_wrt_parent(Pose::from_translation(offset)),
                    body,
                    &mut world.bodies,
                );
                collider_handles[i] = Some(collider);
            }
        }
        clusters.push(Cluster { body, members, offsets, local_com: com - origin });
    }
    if ground {
        add_ground(&mut world, scene);
    }
    let welded: Vec<Vec<usize>> = clusters.iter().map(|c| c.members.clone()).collect();
    let same_set = |a: usize, b: usize| welded.iter().any(|set| set.contains(&a) && set.contains(&b));
    let mut joints = Vec::new();
    for spec in &scene.joints {
        if same_set(spec.a, spec.b) {
            joints.push(None);
            continue;
        }
        let mut moved = spec.clone();
        moved.anchor_a = offset_of[spec.a] + spec.anchor_a;
        moved.anchor_b = offset_of[spec.b] + spec.anchor_b;
        joints.push(Some(world.impulse_joints.insert(bodies[spec.a].expect("body"), bodies[spec.b].expect("body"), make_joint(&moved), true)));
    }
    world.tick_length = settings.tick_length;
    Built { world, bodies, colliders: collider_handles, joints, clusters }
}

pub fn build(scene: &Scene, settings: Settings, only_group: Option<usize>, colliders: bool, ground: bool, coulomb: bool) -> Built {
    build_with(scene, settings, only_group, colliders, ground, coulomb, false)
}

/// `isolated`: the cars collide with the ground and with nothing else (a world with the ground and without the contacts between groups).
pub fn build_with(scene: &Scene, settings: Settings, only_group: Option<usize>, colliders: bool, ground: bool, coulomb: bool, isolated: bool) -> Built {
    let mut world = World::new(settings, scene.gravity);
    if coulomb {
        world.parameters.friction_model = FrictionModel::Coulomb;
    }
    let mut bodies = Vec::new();
    let mut collider_handles = Vec::new();
    for car in &scene.cars {
        if only_group.is_some_and(|g| g != car.group) {
            bodies.push(None);
            collider_handles.push(None);
            continue;
        }
        let body = world.bodies.insert(
            RigidBodyBuilder::dynamic()
                .pose(car.pose)
                .linvel(car.linvel)
                .angvel(car.angvel)
                .can_sleep(false)
                .additional_mass_properties(MassProperties::new(Vector::ZERO, car.mass, car.principal_inertia())),
        );
        bodies.push(Some(body));
        if colliders {
            let filter = if isolated { car_isolated_filter(car.group) } else { car_filter(car.group) };
            let collider = world.colliders.insert_with_parent(car.shape.builder().density(0.0).friction(car.friction).restitution(0.0).collision_groups(filter), body, &mut world.bodies);
            collider_handles.push(Some(collider));
        } else {
            collider_handles.push(None);
        }
    }
    if ground {
        add_ground(&mut world, scene);
    }
    let mut joints = Vec::new();
    for spec in &scene.joints {
        let (a, b) = (bodies[spec.a], bodies[spec.b]);
        match (a, b) {
            (Some(a), Some(b)) => joints.push(Some(world.impulse_joints.insert(a, b, make_joint(spec), true))),
            _ => joints.push(None),
        }
    }
    world.tick_length = settings.tick_length;
    Built { world, bodies, colliders: collider_handles, joints, clusters: Vec::new() }
}

fn add_ground(world: &mut World, scene: &Scene) {
    for ground in &scene.ground {
        world.colliders.insert(ground.shape.builder().position(ground.pose).friction(ground.friction).restitution(0.0).collision_groups(ground_filter()));
    }
}

fn read_body(world: &World, handle: RigidBodyHandle) -> CarState {
    let body = &world.bodies[handle];
    CarState { pose: *body.position(), linvel: body.linvel(), angvel: body.angvel() }
}

fn write_body(world: &mut World, handle: RigidBodyHandle, state: CarState) {
    let body = &mut world.bodies[handle];
    body.set_position(state.pose, false);
    body.set_linvel(state.linvel, false);
    body.set_angvel(state.angvel, false);
}

fn rotation_matrix(pose: &Pose) -> Matrix {
    Matrix::from_quat(pose.rotation)
}

/// A car's inertia tensor in the world's axes.
fn world_inertia(spec: &CarSpec, pose: &Pose) -> Matrix {
    let r = rotation_matrix(pose);
    r * Matrix::from_diagonal(spec.principal_inertia()) * r.transpose()
}

fn world_inverse_inertia(spec: &CarSpec, pose: &Pose) -> Matrix {
    let r = rotation_matrix(pose);
    let inertia = spec.principal_inertia();
    r * Matrix::from_diagonal(Vector::new(1.0 / inertia.x, 1.0 / inertia.y, 1.0 / inertia.z)) * r.transpose()
}

fn outer(a: Vector, b: Vector) -> Matrix {
    Matrix::from_cols(a * b.x, a * b.y, a * b.z)
}

/// What a set of cars is as one rigid body: mass, centre, inertia tensor about the centre, and the velocity and the turn the cars' momenta give it.
struct Lump {
    mass: f64,
    centre: Vector,
    inertia: Matrix,
    velocity: Vector,
    turn: Vector,
}

fn lump(scene: &Scene, members: &[usize], states: &[CarState], velocities: &[(Vector, Vector)]) -> Lump {
    let mass: f64 = members.iter().map(|&i| scene.cars[i].mass).sum();
    let centre = members.iter().fold(Vector::ZERO, |sum, &i| sum + states[i].pose.translation * scene.cars[i].mass) / mass;
    let velocity = members.iter().fold(Vector::ZERO, |sum, &i| sum + velocities[i].0 * scene.cars[i].mass) / mass;
    let mut inertia = Matrix::ZERO;
    let mut spin = Vector::ZERO;
    for &i in members {
        let arm = states[i].pose.translation - centre;
        let own = world_inertia(&scene.cars[i], &states[i].pose);
        inertia += own + (Matrix::IDENTITY * arm.dot(arm) - outer(arm, arm)) * scene.cars[i].mass;
        spin += own * velocities[i].1 + arm.cross(velocities[i].0 - velocity) * scene.cars[i].mass;
    }
    let turn = inertia.inverse() * spin;
    Lump { mass, centre, inertia, velocity, turn }
}

pub struct Proxy {
    pub cars: Vec<usize>,
    pub body: RigidBodyHandle,
    pub colliders: Vec<ColliderHandle>,
}

struct Hit {
    car: usize,
    key: (usize, u32, u32, usize, usize),
    impulse: Vector,
    point: Vector,
    /// The part of the impulse along the contact normal (the rest is friction), for diagnostics.
    normal_part: Vector,
    /// The car on the other side of the contact, if it is one.
    other: Option<usize>,
}

/// A step's results for the cars: the change of velocity (linear, angular) the contact gave each.
type Kick = (Vector, Vector);

pub struct Sim {
    pub scene: Scene,
    pub scheme: Scheme,
    pub settings: Settings,
    /// Calls of the contact world's step per step of the joint worlds (each with the step's time over this).
    pub contact_calls: usize,
    pub time: f64,
    pub dt: f64,
    /// One world: the whole thing (`Hi`, `Game`).
    one: Option<Built>,
    /// The groups' joint worlds (the truth for the two-world schemes).
    groups: Vec<Built>,
    /// Scratch copies of the groups' joint worlds, for predicted steps.
    scratch: Vec<Built>,
    contact: Option<World>,
    proxies: Vec<Proxy>,
    owner: HashMap<ColliderHandle, usize>,
    ghost: Option<Built>,
    /// The ghost world without the contacts (`GhostDifference`).
    free_ghost: Option<Built>,
    /// Extra solver work for the ghost worlds (substeps, passes), and whether only while something was touched in the last step.
    pub ghost_effort: (usize, usize),
    pub effort_when_touched: bool,
    touched_last_step: bool,
    /// Whether the car of the higher priority is left out of the impulses of a contact (see `CarSpec::priority`).
    pub priority_rule: bool,
    /// Calls of the ghost worlds' step per step of the joint worlds (a ghost lives in another frame, with its own number of steps).
    pub ghost_calls: usize,
    /// The ghosts come into being at this time (seconds); before it the groups' joint worlds step alone.
    pub ghost_born_at: f64,
    born: bool,
    /// The groups' joint worlds keep the ground and their cars' colliders; the ghost's other world has the ground and not the contacts between groups.
    pub own_ground: bool,
    pub adaptive: Option<Adaptive>,
    pub calls_rule: Option<f64>,
    /// How many ghost pairs were solved in all (the difference form solves one pair for each level it used in a step).
    pub ghost_pair_solves: usize,
    /// Cut a step where the first touch is (`Options::toi`), the shapes of the cars and the ground for the question, and how many pieces were cut.
    pub toi: bool,
    shapes: Vec<SharedShape>,
    ground_shapes: Vec<SharedShape>,
    pub cuts: std::cell::Cell<usize>,
    /// The cars' states at the start of the last step, and the outside force and torque on each over it (the pushes and what was handed back), for the load feedback.
    previous: Option<Vec<CarState>>,
    last_outside: Vec<(Vector, Vector)>,
    /// The last two estimates of each car's joint load, and the one that was used last (for the filters of the load feedback).
    load_history: Vec<[(Vector, Vector); 2]>,
    load_used: Vec<(Vector, Vector)>,
    /// The direction each car's contacts pushed it in the last step (zero: no contact).
    last_push: Vec<Vector>,
    /// The worst difference between a proxy's change of momentum and the sum of its contact impulses, over the change (0 if there was none to speak of).
    pub worst_balance: f64,
    pub spent: Duration,
    /// The most any one step's impulses were (N s) and how many steps had any (a measure of chatter).
    pub steps_with_hits: usize,
}

pub fn clusters_of(scene: &Scene) -> Vec<Vec<usize>> {
    let mut parent: Vec<usize> = (0..scene.cars.len()).collect();
    fn find(parent: &mut Vec<usize>, x: usize) -> usize {
        let mut root = x;
        while parent[root] != root {
            root = parent[root];
        }
        let mut walk = x;
        while parent[walk] != root {
            let next = parent[walk];
            parent[walk] = root;
            walk = next;
        }
        root
    }
    for (index, joint) in scene.joints.iter().enumerate() {
        if joint.kind == JointKind::Fixed || scene.welds.contains(&index) {
            let (a, b) = (find(&mut parent, joint.a), find(&mut parent, joint.b));
            if a != b {
                parent[a.max(b)] = a.min(b);
            }
        }
    }
    let mut sets: Vec<Vec<usize>> = Vec::new();
    let mut seen: HashMap<usize, usize> = HashMap::new();
    for i in 0..scene.cars.len() {
        let root = find(&mut parent, i);
        let index = *seen.entry(root).or_insert_with(|| {
            sets.push(Vec::new());
            sets.len() - 1
        });
        sets[index].push(i);
    }
    sets
}

fn partition_of(scene: &Scene, partition: Partition) -> Vec<Vec<usize>> {
    match partition {
        Partition::Whole => (0..scene.groups()).map(|g| scene.cars_of(g)).collect(),
        Partition::PerCar => (0..scene.cars.len()).map(|i| vec![i]).collect(),
        Partition::Clusters => clusters_of(scene),
    }
}

impl Sim {
    pub fn new(scene: &Scene, scheme: Scheme, settings: Settings) -> Self {
        let settings = match scheme {
            Scheme::Hi => hi_settings(),
            Scheme::HiFiner => finer_settings(),
            Scheme::HiFinest => finest_settings(),
            _ => settings,
        };
        let mut sim = Sim {
            scene: scene.clone(),
            scheme,
            settings,
            contact_calls: 1,
            time: 0.0,
            dt: settings.tick_length / settings.steps_per_tick as f64,
            one: None,
            groups: Vec::new(),
            scratch: Vec::new(),
            contact: None,
            proxies: Vec::new(),
            owner: HashMap::new(),
            ghost: None,
            free_ghost: None,
            ghost_effort: (0, 0),
            effort_when_touched: false,
            touched_last_step: false,
            priority_rule: false,
            ghost_calls: 1,
            ghost_born_at: 0.0,
            born: true,
            own_ground: false,
            adaptive: None,
            calls_rule: None,
            ghost_pair_solves: 0,
            toi: false,
            shapes: scene.cars.iter().map(|c| parry_shape(c.shape)).collect(),
            ground_shapes: scene.ground.iter().map(|g| parry_shape(g.shape)).collect(),
            cuts: std::cell::Cell::new(0),
            previous: None,
            last_outside: Vec::new(),
            load_history: Vec::new(),
            load_used: Vec::new(),
            last_push: Vec::new(),
            worst_balance: 0.0,
            spent: Duration::ZERO,
            steps_with_hits: 0,
        };
        match scheme {
            Scheme::Hi | Scheme::HiFiner | Scheme::HiFinest | Scheme::Game => sim.one = Some(build(scene, settings, None, true, true, true)),
            Scheme::GhostWelded | Scheme::GhostWeldedDifference => {
                sim.groups = (0..scene.groups()).map(|g| build(scene, settings, Some(g), false, false, true)).collect();
                sim.ghost = Some(build_welded(scene, settings, true, true));
                if scheme == Scheme::GhostWeldedDifference {
                    sim.free_ghost = Some(build_welded(scene, settings, false, false));
                }
            }
            Scheme::Ghost | Scheme::GhostDifference => {
                sim.groups = (0..scene.groups()).map(|g| build(scene, settings, Some(g), false, false, true)).collect();
                sim.ghost = Some(build(scene, settings, None, true, true, true));
                if scheme == Scheme::GhostDifference {
                    sim.free_ghost = Some(build(scene, settings, None, false, false, true));
                }
            }
            Scheme::Proxy { partition, rounds, .. } => {
                sim.groups = (0..scene.groups()).map(|g| build(scene, settings, Some(g), false, false, true)).collect();
                if rounds > 0 {
                    sim.scratch = (0..scene.groups()).map(|g| build(scene, settings, Some(g), false, false, true)).collect();
                }
                let mut contact = World::new(settings, scene.gravity);
                contact.parameters.friction_model = FrictionModel::Coulomb;
                add_ground(&mut contact, scene);
                for members in partition_of(scene, partition) {
                    let mass: f64 = members.iter().map(|&i| scene.cars[i].mass).sum();
                    let body = contact.bodies.insert(RigidBodyBuilder::dynamic().can_sleep(false).additional_mass_properties(MassProperties::new(Vector::ZERO, mass, Vector::splat(mass))));
                    let mut colliders = Vec::new();
                    for &i in &members {
                        let car = &scene.cars[i];
                        let collider = contact.colliders.insert_with_parent(
                            car.shape.builder().density(0.0).friction(car.friction).restitution(0.0).collision_groups(car_filter(car.group)),
                            body,
                            &mut contact.bodies,
                        );
                        sim.owner.insert(collider, i);
                        colliders.push(collider);
                    }
                    sim.proxies.push(Proxy { cars: members, body, colliders });
                }
                sim.contact = Some(contact);
            }
        }
        sim
    }

    /// Every car's state, in the scene's order.
    pub fn states(&self) -> Vec<CarState> {
        match &self.one {
            Some(built) => built.bodies.iter().map(|b| read_body(&built.world, b.expect("body"))).collect(),
            None => self.truth_states(),
        }
    }

    fn truth_states(&self) -> Vec<CarState> {
        (0..self.scene.cars.len())
            .map(|i| {
                let group = &self.groups[self.scene.cars[i].group];
                read_body(&group.world, group.bodies[i].expect("body"))
            })
            .collect()
    }

    fn forces_at(&self, time: f64) -> Vec<Vector> {
        let mut forces = vec![Vector::ZERO; self.scene.cars.len()];
        for push in &self.scene.pushes {
            if time >= push.from && time < push.until {
                forces[push.car] += push.force;
            }
        }
        forces
    }

    /// Sets the forces and torques on the bodies (what is given back from a contact is a force over the step, so that it acts together with gravity and the joints' own forces).
    fn apply_forces(world: &mut World, bodies: &[Option<RigidBodyHandle>], forces: &[Vector], handed_back: &[(Vector, Vector)]) {
        for (index, handle) in bodies.iter().enumerate() {
            if let Some(handle) = handle {
                let body = &mut world.bodies[*handle];
                body.reset_forces(false);
                body.reset_torques(false);
                let force = forces[index] + handed_back[index].0;
                if force.length_squared() > 0.0 {
                    body.add_force(force, true);
                }
                if handed_back[index].1.length_squared() > 0.0 {
                    body.add_torque(handed_back[index].1, true);
                }
            }
        }
    }

    /// What each car's kick (a change of velocity over the step) is as a force and a torque over the step.
    fn kick_forces(&self, kicks: &[Kick], states: &[CarState]) -> Vec<(Vector, Vector)> {
        (0..self.scene.cars.len())
            .map(|i| {
                let spec = &self.scene.cars[i];
                (kicks[i].0 * (spec.mass / self.dt), world_inertia(spec, &states[i].pose) * kicks[i].1 / self.dt)
            })
            .collect()
    }

    fn apply_changes(&mut self) {
        let due: Vec<MotorChange> = self.scene.changes.iter().filter(|c| c.at <= self.time && c.at > self.time - self.dt).cloned().collect();
        for change in due {
            let spec = self.scene.joints[change.joint].clone();
            let worlds: Vec<&mut Built> = if let Some(one) = self.one.as_mut() {
                vec![one]
            } else {
                let group = self.scene.cars[spec.a].group;
                let mut worlds: Vec<&mut Built> = Vec::new();
                for (index, built) in self.groups.iter_mut().enumerate() {
                    if index == group {
                        worlds.push(built);
                    }
                }
                for (index, built) in self.scratch.iter_mut().enumerate() {
                    if index == group {
                        worlds.push(built);
                    }
                }
                if let Some(ghost) = self.ghost.as_mut() {
                    worlds.push(ghost);
                }
                if let Some(free) = self.free_ghost.as_mut() {
                    worlds.push(free);
                }
                worlds
            };
            for built in worlds {
                if let Some(handle) = built.joints[change.joint] {
                    if let Some(joint) = built.world.impulse_joints.get_mut(handle, true) {
                        let m = change.motor;
                        joint.data.set_motor(axis_of(spec.kind), m.target_position, m.target_velocity, m.stiffness, m.damping);
                        joint.data.set_motor_model(axis_of(spec.kind), MotorModel::ForceBased);
                        if m.max_force.is_finite() {
                            joint.data.set_motor_max_force(axis_of(spec.kind), m.max_force);
                        }
                    }
                }
            }
        }
    }

    pub fn step(&mut self) {
        let started = Instant::now();
        if !self.born && self.time + 1e-9 >= self.ghost_born_at {
            self.rebuild_ghosts();
        }
        self.apply_changes();
        let forces = self.forces_at(self.time);
        match self.scheme {
            Scheme::Hi | Scheme::HiFiner | Scheme::HiFinest | Scheme::Game => {
                let mut built = self.one.take().expect("one world");
                let none = vec![(Vector::ZERO, Vector::ZERO); forces.len()];
                Self::apply_forces(&mut built.world, &built.bodies, &forces, &none);
                if self.toi {
                    let start = built.states(&self.scene);
                    self.advance(&mut built, &start, &mut Vec::new(), true, &mut |_, _| {});
                } else {
                    built.world.step();
                }
                self.one = Some(built);
            }
            Scheme::Ghost => self.ghost_step(&forces),
            Scheme::GhostDifference | Scheme::GhostWeldedDifference => self.difference_step(&forces),
            Scheme::GhostWelded => self.ghost_step(&forces),
            Scheme::Proxy { partition: _, handback, rounds, feedback } => self.proxy_step(&forces, handback, rounds, feedback),
        }
        self.time += self.dt;
        self.spent += started.elapsed();
    }

    pub fn tick(&mut self) {
        for _ in 0..self.settings.steps_per_tick {
            self.step();
        }
    }

    fn step_joint_worlds(&mut self, forces: &[Vector], handed_back: &[(Vector, Vector)]) {
        for built in &mut self.groups {
            Self::apply_forces(&mut built.world, &built.bodies, forces, handed_back);
            built.world.step();
        }
    }

    /// The impulses of every contact pair that touches a car of the contact world, with the points, in a canonical order (so that their sums do not depend on the order Rapier keeps the pairs in).
    fn read_hits(&self, contact: &World, call: usize, hits: &mut Vec<Hit>) {
        for pair in contact.narrow_phase.contact_pairs() {
            let (first, second) = (pair.collider1, pair.collider2);
            let (car1, car2) = (self.owner.get(&first).copied(), self.owner.get(&second).copied());
            if car1.is_none() && car2.is_none() {
                continue;
            }
            let (pose1, pose2) = (*contact.colliders[first].position(), *contact.colliders[second].position());
            for (manifold_index, manifold) in pair.solver_manifolds().iter().enumerate() {
                // The solver pushes the first body along `-normal` (and in the plane across it by the friction impulses).
                let force_direction = -manifold.data.normal;
                let tangent = force_direction.orthonormal_vector();
                let bitangent = force_direction.cross(tangent);
                // Only the points the solver used this step (a point of the manifold that it did not use keeps the impulse of an earlier step).
                let active: Vec<usize> = manifold.data.solver_contacts.iter().map(|c| (c.contact_id[0] & 0x7FFF_FFFF) as usize).collect();
                for (point_index, point) in manifold.points.iter().enumerate() {
                    if !active.contains(&point_index) {
                        continue;
                    }
                    let on_first = force_direction * point.data.impulse + tangent * point.data.tangent_impulse.x + bitangent * point.data.tangent_impulse.y;
                    let key = (call, first.into_raw_parts().0, second.into_raw_parts().0, manifold_index, point_index);
                    let normal_on_first = force_direction * point.data.impulse;
                    if let Some(car) = car1 {
                        hits.push(Hit { car, key, impulse: on_first, point: pose1.transform_point(point.local_p1), normal_part: normal_on_first, other: car2 });
                    }
                    if let Some(car) = car2 {
                        hits.push(Hit { car, key, impulse: -on_first, point: pose2.transform_point(point.local_p2), normal_part: -normal_on_first, other: car1 });
                    }
                }
            }
        }
        hits.sort_by(|a, b| (a.car, a.key).cmp(&(b.car, b.key)));
    }

    /// The change of velocity each car gets from impulses at points.
    fn kicks_from_hits(&self, hits: &[Hit], states: &[CarState]) -> Vec<Kick> {
        let mut impulses = vec![Vector::ZERO; self.scene.cars.len()];
        let mut moments = vec![Vector::ZERO; self.scene.cars.len()];
        for hit in hits {
            if self.priority_rule && hit.other.is_some_and(|other| self.scene.cars[other].priority < self.scene.cars[hit.car].priority) {
                continue;
            }
            impulses[hit.car] += hit.impulse;
            moments[hit.car] += (hit.point - states[hit.car].pose.translation).cross(hit.impulse);
        }
        (0..self.scene.cars.len())
            .map(|i| {
                let spec = &self.scene.cars[i];
                (impulses[i] / spec.mass, world_inverse_inertia(spec, &states[i].pose) * moments[i])
            })
            .collect()
    }

    /// The worlds of a ghost (and, for the difference form, the one without the contacts between groups) in the layout of this scheme.
    fn make_ghosts(&self) -> (Built, Option<Built>) {
        let (scene, settings) = (&self.scene, self.settings);
        let difference = matches!(self.scheme, Scheme::GhostDifference | Scheme::GhostWeldedDifference);
        match self.scheme {
            Scheme::GhostWelded | Scheme::GhostWeldedDifference => (build_welded(scene, settings, true, true), difference.then(|| build_welded(scene, settings, false, false))),
            _ => (
                build(scene, settings, None, true, true, true),
                difference.then(|| if self.own_ground { build_with(scene, settings, None, true, true, true, true) } else { build(scene, settings, None, false, false, true) }),
            ),
        }
    }

    /// Fresh ghost worlds (nothing warm in them): the ghost is born now, from the truth's state at the next sync.
    fn rebuild_ghosts(&mut self) {
        assert!(self.scene.changes.iter().all(|c| c.at > self.time), "a ghost born late cannot be told of motor changes that came before");
        let (ghost, free) = self.make_ghosts();
        self.ghost = Some(ghost);
        self.free_ghost = free;
        self.owner.clear();
        self.born = true;
        self.touched_last_step = false;
    }

    /// The groups' joint worlds keep the ground and their own colliders, and the ghost gives back only what the other groups did (`Options::own_ground`).
    pub fn set_own_ground(&mut self) {
        assert!(matches!(self.scheme, Scheme::Ghost | Scheme::GhostDifference), "the own-ground layout is for the unwelded ghosts");
        self.own_ground = true;
        self.groups = (0..self.scene.groups()).map(|g| build(&self.scene, self.settings, Some(g), true, true, true)).collect();
        let (ghost, free) = self.make_ghosts();
        self.ghost = Some(ghost);
        self.free_ghost = free;
        self.owner.clear();
    }

    /// Extra solver work for the worlds that hold the mechanisms (the one world, or the groups' own).
    pub fn set_world_effort(&mut self, effort: (usize, usize)) {
        for built in self.one.iter_mut().chain(self.groups.iter_mut()).chain(self.scratch.iter_mut()) {
            built.set_effort(effort);
        }
    }

    /// The ghosts come into being at `seconds` (0: from the start).
    pub fn set_birth(&mut self, seconds: f64) {
        self.ghost_born_at = seconds;
        self.born = seconds <= 0.0;
    }

    /// Changes the length of a step, in every world (a frame whose number of steps a tick follows its fastest body).
    pub fn set_step_length(&mut self, dt: f64) {
        self.dt = dt;
        for built in self.one.iter_mut().chain(self.groups.iter_mut()).chain(self.scratch.iter_mut()).chain(self.ghost.iter_mut()).chain(self.free_ghost.iter_mut()) {
            built.world.parameters.dt = dt;
        }
        if let Some(contact) = self.contact.as_mut() {
            contact.parameters.dt = dt;
        }
    }

    /// Calls of a ghost world's step in this step: as set, or as many as the fastest car needs to move no more than `calls_rule` in each (at most 8, a tick of 24 steps).
    fn calls_now(&self, truth: &[CarState]) -> usize {
        match self.calls_rule {
            None => self.ghost_calls,
            Some(most) => {
                let fastest = truth.iter().map(|s| s.linvel.length()).fold(0.0, f64::max);
                ((fastest * self.dt / most).ceil() as usize).max(self.ghost_calls).clamp(1, 8)
            }
        }
    }

    /// When, within `within` seconds, the first pair that is apart and closing (cars of different groups, or a car and the ground) would touch, each going at its velocity.
    fn first_touch(&self, states: &[CarState], within: f64) -> Option<f64> {
        let options = rapier3d::parry::query::ShapeCastOptions { max_time_of_impact: within, target_distance: 0.0, stop_at_penetration: true, compute_impact_geometry_on_penetration: false };
        let mut earliest: Option<f64> = None;
        let mut consider = |hit: Option<rapier3d::parry::query::ShapeCastHit>| {
            // A pair that touches now (a time of impact of 0) is no event: only a touch still to come is.
            if let Some(hit) = hit {
                if hit.time_of_impact > 1e-9 && earliest.is_none_or(|t| hit.time_of_impact < t) {
                    earliest = Some(hit.time_of_impact);
                }
            }
        };
        let n = states.len();
        for a in 0..n {
            for b in a + 1..n {
                if self.scene.cars[a].group != self.scene.cars[b].group {
                    consider(rapier3d::parry::query::cast_shapes(&states[a].pose, states[a].linvel, &*self.shapes[a], &states[b].pose, states[b].linvel, &*self.shapes[b], options).ok().flatten());
                }
            }
            for (ground, shape) in self.scene.ground.iter().zip(&self.ground_shapes) {
                consider(rapier3d::parry::query::cast_shapes(&states[a].pose, states[a].linvel, &*self.shapes[a], &ground.pose, Vector::ZERO, &**shape, options).ok().flatten());
            }
        }
        earliest
    }

    /// Steps a world (a ghost, or the one world) over this step: in `calls_now` pieces, each cut in two where the first touch is if `toi` is set. With `decide` the pieces are chosen on
    /// the way and put in `plan`; without it the pieces of `plan` are replayed (the world without the contacts goes the same way as the world with them). `after` is told of each piece.
    fn advance(&self, built: &mut Built, truth: &[CarState], plan: &mut Vec<f64>, decide: bool, after: &mut dyn FnMut(&World, usize)) {
        let saved = built.world.parameters.dt;
        if decide {
            plan.clear();
            let calls = self.calls_now(truth);
            let nominal = self.dt / calls as f64;
            for _ in 0..calls {
                let mut pieces = vec![nominal];
                if self.toi {
                    let states = built.states(&self.scene);
                    let touch = self.first_touch(&states, nominal);
                    if std::env::var("CUT_DEBUG").is_ok() && touch.is_some() {
                        println!("time {:.4}: first touch in {:?} of a piece of {:.5}", self.time, touch, nominal);
                    }
                    if let Some(t) = touch {
                        if t > 0.02 * nominal && t < 0.98 * nominal {
                            pieces = vec![t, nominal - t];
                            self.cuts.set(self.cuts.get() + 1);
                        }
                    }
                }
                for piece in pieces {
                    built.world.parameters.dt = piece;
                    built.world.step();
                    after(&built.world, plan.len());
                    plan.push(piece);
                }
            }
        } else {
            for (call, &piece) in plan.iter().enumerate() {
                built.world.parameters.dt = piece;
                built.world.step();
                after(&built.world, call);
            }
        }
        built.world.parameters.dt = saved;
    }

    fn effort_now(&self) -> (usize, usize) {
        if self.effort_when_touched && !self.touched_last_step { (0, 0) } else { self.ghost_effort }
    }

    fn ghost_step(&mut self, forces: &[Vector]) {
        let truth = self.truth_states();
        if !self.born {
            let none = vec![(Vector::ZERO, Vector::ZERO); forces.len()];
            self.step_joint_worlds(forces, &none);
            return;
        }
        let mut ghost = self.ghost.take().expect("ghost");
        ghost.sync(&self.scene, &truth);
        ghost.set_pushes(&self.scene, &truth, forces);
        ghost.set_effort(self.effort_now());
        self.owner_from(&ghost);
        let mut hits = Vec::new();
        self.advance(&mut ghost, &truth, &mut Vec::new(), true, &mut |world, call| self.read_hits(world, call, &mut hits));
        if self.own_ground {
            // The groups' own worlds have the ground: only what the other groups did is given to them.
            hits.retain(|hit| hit.other.is_some());
        }
        self.ghost = Some(ghost);
        self.touched_last_step = !hits.is_empty();
        if !hits.is_empty() {
            self.steps_with_hits += 1;
        }
        let kicks = self.kicks_from_hits(&hits, &truth);
        let handed_back = self.kick_forces(&kicks, &truth);
        self.step_joint_worlds(forces, &handed_back);
    }

    /// Steps a ghost world from the truth's state (in `ghost_calls` pieces); returns the cars' states after.
    fn ghost_states(&self, built: &mut Built, truth: &[CarState], forces: &[Vector], effort: (usize, usize), plan: &mut Vec<f64>, decide: bool) -> Vec<CarState> {
        built.sync(&self.scene, truth);
        built.set_pushes(&self.scene, truth, forces);
        built.set_effort(effort);
        self.advance(built, truth, plan, decide, &mut |_, _| {});
        built.states(&self.scene)
    }

    fn difference_step(&mut self, forces: &[Vector]) {
        let truth = self.truth_states();
        let none = vec![(Vector::ZERO, Vector::ZERO); forces.len()];
        if !self.born {
            self.step_joint_worlds(forces, &none);
            return;
        }
        let mut with = self.ghost.take().expect("ghost");
        let mut without = self.free_ghost.take().expect("free ghost");
        let levels: Vec<(usize, usize)> = match self.adaptive {
            Some(adaptive) => adaptive.levels.to_vec(),
            None => vec![self.effort_now()],
        };
        let mut previous: Option<Vec<Delta>> = None;
        let mut deltas: Vec<Delta> = Vec::new();
        for &level in &levels {
            let mut plan = Vec::new();
            let a = self.ghost_states(&mut with, &truth, forces, level, &mut plan, true);
            let b = self.ghost_states(&mut without, &truth, forces, level, &mut plan, false);
            self.ghost_pair_solves += 1;
            deltas = a.iter().zip(&b).map(|(a, b)| Delta::between(a, b)).collect();
            let nothing = deltas.iter().all(|d| !d.moved());
            let settled = match (&previous, self.adaptive) {
                (Some(before), Some(adaptive)) => before.iter().zip(&deltas).all(|(x, y)| {
                    let (places, velocities) = x.apart(y);
                    let (size_places, size_velocities) = (x.dx.length().max(y.dx.length()), x.dv.length().max(y.dv.length()));
                    places <= adaptive.position.max(adaptive.relative * size_places) && velocities <= adaptive.velocity.max(adaptive.relative * size_velocities)
                }),
                _ => false,
            };
            if nothing || settled {
                break;
            }
            previous = Some(deltas.clone());
        }
        self.ghost = Some(with);
        self.free_ghost = Some(without);
        self.step_joint_worlds(forces, &none);
        let after = self.truth_states();
        let mut changed = false;
        for i in 0..after.len() {
            let delta = deltas[i];
            if !delta.moved() {
                continue;
            }
            changed = true;
            let group = &mut self.groups[self.scene.cars[i].group];
            let body = &mut group.world.bodies[group.bodies[i].expect("body")];
            body.set_position(Pose::from_parts(after[i].pose.translation + delta.dx, (delta.dq * after[i].pose.rotation).normalize()), false);
            body.set_linvel(after[i].linvel + delta.dv, false);
            body.set_angvel(after[i].angvel + delta.dw, false);
        }
        self.touched_last_step = changed;
        if changed {
            self.steps_with_hits += 1;
        }
    }

    /// The ghost's colliders belong to the cars in the scene's order (set once).
    fn owner_from(&mut self, ghost: &Built) {
        if self.owner.is_empty() {
            for (i, collider) in ghost.colliders.iter().enumerate() {
                if let Some(collider) = collider {
                    self.owner.insert(*collider, i);
                }
            }
        }
    }

    fn proxy_step(&mut self, forces: &[Vector], handback: Handback, rounds: usize, feedback: Feedback) {
        let dt = self.dt;
        let truth = self.truth_states();
        let count = self.scene.cars.len();
        // The velocities the contact world starts the step with: the truth's own (and the pushes' share of the step), to begin with. The contact world has gravity.
        let pull = self.scene.gravity * dt;
        let mut free: Vec<(Vector, Vector)> = (0..count).map(|i| (truth[i].linvel + forces[i] / self.scene.cars[i].mass * dt, truth[i].angvel)).collect();
        let mut kicks: Vec<Kick> = vec![(Vector::ZERO, Vector::ZERO); count];
        let mut final_hits: Vec<Hit> = Vec::new();
        let loads: Vec<(Vector, Vector)> = if feedback != Feedback::None { self.joint_loads(&truth, feedback) } else { vec![(Vector::ZERO, Vector::ZERO); count] };
        for round in 0..=rounds {
            // 1. the contact world, from the truth's places and the free velocities.
            let mut contact = self.contact.take().expect("contact world");
            let lumps: Vec<Lump> = self.proxies.iter().map(|p| lump(&self.scene, &p.cars, &truth, &free)).collect();
            for (proxy, l) in self.proxies.iter().zip(&lumps) {
                for (collider, &i) in proxy.colliders.iter().zip(&proxy.cars) {
                    let pose = Pose::from_parts(truth[i].pose.translation - l.centre, truth[i].pose.rotation);
                    contact.colliders[*collider].set_position_wrt_parent(pose);
                }
                let body = &mut contact.bodies[proxy.body];
                body.set_additional_mass_properties(MassProperties::with_inertia_matrix(Vector::ZERO, l.mass, l.inertia), false);
                body.set_position(Pose::from_translation(l.centre), false);
                body.set_linvel(l.velocity, false);
                body.set_angvel(l.turn, false);
                // The load the joints carried over the last step, as an outside force (what a joint will hold up or pull during this one).
                body.reset_forces(false);
                body.reset_torques(false);
                if feedback != Feedback::None && round == 0 {
                    let (mut force, mut torque) = (Vector::ZERO, Vector::ZERO);
                    for &i in &proxy.cars {
                        force += loads[i].0;
                        torque += loads[i].1 + (truth[i].pose.translation - l.centre).cross(loads[i].0);
                    }
                    if force.length_squared() > 0.0 {
                        body.add_force(force, true);
                    }
                    if torque.length_squared() > 0.0 {
                        body.add_torque(torque, true);
                    }
                }
            }
            // 2. the step (in `contact_calls` pieces), and what the pairs say.
            let mut hits: Vec<Hit> = Vec::new();
            let piece = dt / self.contact_calls as f64;
            let saved = contact.parameters.dt;
            contact.parameters.dt = piece;
            for call in 0..self.contact_calls {
                contact.step();
                self.read_hits_unsorted(&contact, call, &mut hits);
            }
            contact.parameters.dt = saved;
            hits.sort_by(|a, b| (a.car, a.key).cmp(&(b.car, b.key)));
            // a self-check: a proxy's change of momentum is the sum of the impulses on its cars.
            for (proxy, l) in self.proxies.iter().zip(&lumps) {
                let after = contact.bodies[proxy.body].linvel();
                let wanted = (after - l.velocity - pull) * l.mass;
                let mut sum = Vector::ZERO;
                for hit in hits.iter().filter(|h| proxy.cars.contains(&h.car)) {
                    sum += hit.impulse;
                }
                if wanted.length() > 1e-6 * l.mass {
                    let mismatch = (sum - wanted).length() / wanted.length();
                    if mismatch > 1e-3 && std::env::var("HANDBACK_DEBUG").is_ok() {
                        let normal_sum = hits.iter().filter(|h| proxy.cars.contains(&h.car)).fold(Vector::ZERO, |s, h| s + h.normal_part);
                        let detail: Vec<String> = hits.iter().filter(|h| proxy.cars.contains(&h.car)).map(|h| format!("car{}:{:.0}", h.car, h.normal_part.y)).collect();
                        println!("   body mass {:.1} (lump {:.1}), normal impulses by car: {}", contact.bodies[proxy.body].mass(), l.mass, detail.join(" "));
                        println!(
                            "t={:.4} proxy of cars {:?}: wanted {:.1?} sum {:.1?} of which along the normals {:.1?} (mismatch {:.3}), {} hits",
                            self.time,
                            proxy.cars,
                            wanted,
                            sum,
                            normal_sum,
                            mismatch,
                            hits.iter().filter(|h| proxy.cars.contains(&h.car)).count()
                        );
                    }
                    self.worst_balance = self.worst_balance.max(mismatch);
                }
            }
            let result: Vec<(Vector, Vector)> = match handback {
                Handback::Rigid => {
                    let mut result = vec![(Vector::ZERO, Vector::ZERO); count];
                    for (proxy, l) in self.proxies.iter().zip(&lumps) {
                        let body = &contact.bodies[proxy.body];
                        let (change, turn_change) = (body.linvel() - l.velocity - pull, body.angvel() - l.turn);
                        for &i in &proxy.cars {
                            result[i] = (change + turn_change.cross(truth[i].pose.translation - l.centre), turn_change);
                        }
                    }
                    result
                }
                Handback::Impulse => self.kicks_from_hits(&hits, &truth),
            };
            self.contact = Some(contact);
            kicks = result;
            if round == rounds {
                final_hits = hits;
                break;
            }
            // 3. predict what the joints will do over the step with those kicks (as forces over it), and take the kicks and gravity out of it again: the next round's start velocities.
            let handed_back = self.kick_forces(&kicks, &truth);
            for g in 0..self.scene.groups() {
                let cars = self.scene.cars_of(g);
                let scratch = &mut self.scratch[g];
                for &i in &cars {
                    write_body(&mut scratch.world, scratch.bodies[i].expect("body"), truth[i]);
                }
                Self::apply_forces(&mut scratch.world, &scratch.bodies, forces, &handed_back);
                scratch.world.step();
                for &i in &cars {
                    let state = read_body(&scratch.world, scratch.bodies[i].expect("body"));
                    free[i] = (state.linvel - kicks[i].0 - pull, state.angvel - kicks[i].1);
                }
            }
        }
        if !final_hits.is_empty() {
            self.steps_with_hits += 1;
        }
        if std::env::var("HANDBACK_WHEEL").is_ok() && self.time < 0.6 {
            // The first wheel (car 1): its place, its speed and spin, the loads fed back, and what the contact gave it.
            let wheel = 1;
            let (mut normal, mut tangent) = (Vector::ZERO, Vector::ZERO);
            for hit in final_hits.iter().filter(|h| h.car == wheel) {
                normal += hit.normal_part;
                tangent += hit.impulse - hit.normal_part;
            }
            println!(
                "t {:.3} wheel y {:.4} vx {:+.4} spin {:+.3} rim {:+.3} slip {:+.4}  load {:+.1?} {:+.1?}  contact normal {:+.1} tangent x {:+.2}  chassis vx {:+.4}",
                self.time,
                truth[wheel].pose.translation.y,
                truth[wheel].linvel.x,
                truth[wheel].angvel.z,
                -truth[wheel].angvel.z * 0.4,
                truth[wheel].linvel.x + truth[wheel].angvel.z * 0.4,
                loads[wheel].0.x,
                loads[wheel].0.y,
                normal.y,
                tangent.x,
                truth[0].linvel.x
            );
        }
        let handed_back = self.kick_forces(&kicks, &truth);
        self.last_outside = (0..count).map(|i| (forces[i] + handed_back[i].0, handed_back[i].1)).collect();
        let mut pushes = vec![Vector::ZERO; count];
        for hit in &final_hits {
            pushes[hit.car] += hit.normal_part;
        }
        self.last_push = pushes.into_iter().map(|p| if p.length_squared() > 1e-12 { p.normalize() } else { Vector::ZERO }).collect();
        self.previous = Some(truth);
        self.step_joint_worlds(forces, &handed_back);
    }

    /// What each car's joints (and their motors) did to it over the last step: its change of velocity over the step, times its mass, less gravity and the outside forces that were put on it.
    fn joint_loads(&mut self, truth: &[CarState], mode: Feedback) -> Vec<(Vector, Vector)> {
        let n = truth.len();
        let Some(previous) = &self.previous else { return vec![(Vector::ZERO, Vector::ZERO); n] };
        let estimate: Vec<(Vector, Vector)> = (0..n)
            .map(|i| {
                let spec = &self.scene.cars[i];
                let force = (truth[i].linvel - previous[i].linvel) * (spec.mass / self.dt) - self.scene.gravity * spec.mass - self.last_outside[i].0;
                let torque = world_inertia(spec, &truth[i].pose) * ((truth[i].angvel - previous[i].angvel) / self.dt) - self.last_outside[i].1;
                (force, torque)
            })
            .collect();
        if self.load_history.len() != n {
            self.load_history = vec![[(Vector::ZERO, Vector::ZERO); 2]; n];
            self.load_used = vec![(Vector::ZERO, Vector::ZERO); n];
        }
        let agree = |a: f64, b: f64| if a * b > 0.0 { a.signum() * a.abs().min(b.abs()) } else { 0.0 };
        let used: Vec<(Vector, Vector)> = (0..n)
            .map(|i| {
                let [before, _] = self.load_history[i];
                let along = |load: (Vector, Vector)| {
                    let direction = self.last_push.get(i).copied().unwrap_or(Vector::ZERO);
                    (direction * direction.dot(load.0), Vector::ZERO)
                };
                match mode {
                    Feedback::None => (Vector::ZERO, Vector::ZERO),
                    Feedback::Last => estimate[i],
                    Feedback::LastNormal => along(estimate[i]),
                    Feedback::PersistentNormal => {
                        let persistent = (
                            Vector::new(agree(estimate[i].0.x, before.0.x), agree(estimate[i].0.y, before.0.y), agree(estimate[i].0.z, before.0.z)),
                            Vector::ZERO,
                        );
                        along(persistent)
                    }
                    Feedback::Persistent => (
                        Vector::new(agree(estimate[i].0.x, before.0.x), agree(estimate[i].0.y, before.0.y), agree(estimate[i].0.z, before.0.z)),
                        Vector::new(agree(estimate[i].1.x, before.1.x), agree(estimate[i].1.y, before.1.y), agree(estimate[i].1.z, before.1.z)),
                    ),
                    Feedback::Relaxed(alpha) => (estimate[i].0 * alpha + self.load_used[i].0 * (1.0 - alpha), estimate[i].1 * alpha + self.load_used[i].1 * (1.0 - alpha)),
                }
            })
            .collect();
        for i in 0..n {
            self.load_history[i] = [estimate[i], self.load_history[i][0]];
        }
        self.load_used = used.clone();
        used
    }

    fn read_hits_unsorted(&self, contact: &World, call: usize, hits: &mut Vec<Hit>) {
        self.read_hits(contact, call, hits);
    }
}

/// A run's record: the cars' states after every step (the first is the start).
pub struct Trace {
    pub dt: f64,
    pub frames: Vec<Vec<CarState>>,
    pub worst_balance: f64,
    pub micros_per_tick: f64,
    pub steps_with_hits: usize,
    /// Pieces a step was cut into at a first touch (`Options::toi`), and ghost pairs solved (the difference form, one for each level used).
    pub cuts: usize,
    pub ghost_pair_solves: usize,
}

pub fn run(scene: &Scene, scheme: Scheme, settings: Settings) -> Trace {
    run_options(scene, scheme, settings, Options::default())
}

pub fn run_with(scene: &Scene, scheme: Scheme, settings: Settings, contact_calls: usize) -> Trace {
    run_options(scene, scheme, settings, Options { contact_calls, ..Options::default() })
}

/// How a scheme's helper worlds are stepped.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Calls of the contact world's step per step of the joint worlds.
    pub contact_calls: usize,
    /// Calls of a ghost world's step per step of the joint worlds (a ghost lives in another frame, with its own number of steps).
    pub ghost_calls: usize,
    /// Extra solver work (substeps, passes) for the ghost worlds, and whether only in a step after one in which something was touched.
    pub ghost_effort: (usize, usize),
    pub effort_when_touched: bool,
    /// The car of the higher priority is not given the impulse of a contact with a car of a lower one.
    pub priority_rule: bool,
    /// The ghosts come into being at this time (seconds); before it the groups' joint worlds step alone. A ghost born in a contact is a cold one.
    pub ghost_born_at: f64,
    /// Extra solver work for a ghost pair in levels, tried in order until the effect of the contacts stops changing (the difference form only).
    pub adaptive: Option<Adaptive>,
    /// The most (metres) that the fastest car moves in one call of a ghost world's step: the ghost's calls follow the fastest car, as `BodyWorld` cuts a tick by its fastest body.
    pub calls_rule: Option<f64>,
    /// The groups' joint worlds keep the ground (and their cars' colliders), and the ghost has the contacts between groups and the ground: the stranger's effect is all that is given back.
    pub own_ground: bool,
    /// Extra solver work (substeps, passes) for the world that holds the mechanisms: the one world of `Hi` and `Game`, or the groups' own joint worlds (the truth). Nothing: today's.
    pub world_effort: (usize, usize),
    /// Cut a step of the world that does the contacts (the one world, or a ghost) where two cars of different groups, or a car and the ground, would first touch, so that every
    /// hit begins at the start of a piece of a step whatever moment of the tick it falls at.
    pub toi: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { contact_calls: 1, ghost_calls: 1, ghost_effort: (0, 0), effort_when_touched: false, priority_rule: false, ghost_born_at: 0.0, adaptive: None, calls_rule: None, own_ground: false, world_effort: (0, 0), toi: false }
    }
}

/// Extra solver work for a ghost pair, in levels (substeps, passes), tried in order until what the contacts did is the same at two levels in a row (to these tolerances in every car).
#[derive(Clone, Copy, Debug)]
pub struct Adaptive {
    pub levels: &'static [(usize, usize)],
    /// Places, metres: the least a difference between two levels must be to count.
    pub position: f64,
    /// Velocities, metres a second: the same.
    pub velocity: f64,
    /// And what part of the effect itself (the larger of the two levels' places and velocities) a difference may be: 0 is the absolute tolerances alone.
    pub relative: f64,
}

/// What the contacts did to one car in one step: the difference of the two ghosts' results.
#[derive(Clone, Copy, Debug)]
struct Delta {
    dx: Vector,
    dv: Vector,
    dw: Vector,
    dq: Rotation,
}

impl Delta {
    fn between(a: &CarState, b: &CarState) -> Self {
        Self { dx: a.pose.translation - b.pose.translation, dv: a.linvel - b.linvel, dw: a.angvel - b.angvel, dq: a.pose.rotation * b.pose.rotation.inverse() }
    }

    fn moved(&self) -> bool {
        self.dx.length_squared() > 0.0 || self.dv.length_squared() > 0.0 || self.dw.length_squared() > 0.0 || self.dq != Rotation::IDENTITY
    }

    /// How far two effects of a step are from each other: places (m, a turn counted at one metre), velocities (m/s).
    fn apart(&self, other: &Delta) -> (f64, f64) {
        let turn = (self.dq * other.dq.inverse()).to_axis_angle().1.abs();
        ((self.dx - other.dx).length().max(turn), (self.dv - other.dv).length().max((self.dw - other.dw).length()))
    }
}

pub fn run_options(scene: &Scene, scheme: Scheme, settings: Settings, options: Options) -> Trace {
    let mut sim = Sim::new(scene, scheme, settings);
    sim.contact_calls = options.contact_calls;
    sim.ghost_calls = options.ghost_calls;
    sim.ghost_effort = options.ghost_effort;
    sim.effort_when_touched = options.effort_when_touched;
    sim.priority_rule = options.priority_rule;
    sim.adaptive = options.adaptive;
    sim.calls_rule = options.calls_rule;
    sim.toi = options.toi;
    if options.own_ground {
        sim.set_own_ground();
    }
    sim.set_birth(options.ghost_born_at);
    if options.world_effort != (0, 0) {
        sim.set_world_effort(options.world_effort);
    }
    let mut frames = vec![sim.states()];
    let ticks = (scene.seconds / sim.settings.tick_length).round() as usize;
    for _ in 0..ticks {
        for _ in 0..sim.settings.steps_per_tick {
            sim.step();
            frames.push(sim.states());
        }
    }
    Trace { dt: sim.dt, frames, worst_balance: sim.worst_balance, micros_per_tick: sim.spent.as_secs_f64() * 1e6 / ticks as f64, steps_with_hits: sim.steps_with_hits, cuts: sim.cuts.get(), ghost_pair_solves: sim.ghost_pair_solves }
}

impl Trace {
    /// A hash of every bit of every car's pose and velocities in every frame (FNV-1a), to compare runs bit for bit, here and on another machine.
    pub fn fingerprint(&self) -> u64 {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        let mut feed = |x: f64| {
            for byte in x.to_bits().to_le_bytes() {
                hash ^= byte as u64;
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        for frame in &self.frames {
            for s in frame {
                for x in [s.pose.translation.x, s.pose.translation.y, s.pose.translation.z, s.pose.rotation.x, s.pose.rotation.y, s.pose.rotation.z, s.pose.rotation.w, s.linvel.x, s.linvel.y, s.linvel.z, s.angvel.x, s.angvel.y, s.angvel.z] {
                    feed(x);
                }
            }
        }
        hash
    }

    pub fn last(&self) -> &Vec<CarState> {
        self.frames.last().expect("a frame")
    }

    pub fn at(&self, seconds: f64) -> &Vec<CarState> {
        let index = ((seconds / self.dt).round() as usize).min(self.frames.len() - 1);
        &self.frames[index]
    }

    /// The frames from `from` seconds on.
    pub fn since(&self, from: f64) -> &[Vec<CarState>] {
        let index = ((from / self.dt).round() as usize).min(self.frames.len() - 1);
        &self.frames[index..]
    }
}

/// The coordinate of a hinge (radians) or slider (metres, along the axis in the first car's frame) in a frame.
pub fn joint_coordinate(spec: &JointSpec, frame: &[CarState]) -> f64 {
    let (a, b) = (&frame[spec.a], &frame[spec.b]);
    match spec.kind {
        JointKind::Slider => {
            let (pa, pb) = (a.pose * spec.anchor_a, b.pose * spec.anchor_b);
            (pb - pa).dot(a.pose.rotation * spec.axis)
        }
        JointKind::Hinge => {
            let relative = a.pose.rotation.inverse() * b.pose.rotation;
            let axis = spec.axis.normalize();
            let sine = Vector::new(relative.x, relative.y, relative.z).dot(axis);
            2.0 * sine.atan2(relative.w)
        }
        _ => 0.0,
    }
}

/// The worst distance between the two anchors of a joint in a frame (m).
pub fn joint_gap(spec: &JointSpec, frame: &[CarState]) -> f64 {
    let (a, b) = (&frame[spec.a], &frame[spec.b]);
    let (pa, pb) = (a.pose * spec.anchor_a, b.pose * spec.anchor_b);
    let mut gap = pb - pa;
    if spec.kind == JointKind::Slider {
        let axis = a.pose.rotation * spec.axis;
        gap -= axis * gap.dot(axis);
    }
    gap.length()
}

pub fn total_momentum(scene: &Scene, frame: &[CarState]) -> Vector {
    frame.iter().zip(&scene.cars).fold(Vector::ZERO, |sum, (s, c)| sum + s.linvel * c.mass)
}

pub fn kinetic_energy(scene: &Scene, frame: &[CarState]) -> f64 {
    frame
        .iter()
        .zip(&scene.cars)
        .map(|(s, c)| 0.5 * c.mass * s.linvel.length_squared() + 0.5 * s.angvel.dot(world_inertia(c, &s.pose) * s.angvel))
        .sum()
}

/// A run whose steps a tick follow `steps_for_tick(index)` (a frame cuts its tick by its fastest body), the states recorded at the end of every tick (the first entry is the start).
pub fn run_scheduled(scene: &Scene, scheme: Scheme, settings: Settings, options: Options, steps_for_tick: &dyn Fn(usize) -> usize) -> Vec<Vec<CarState>> {
    run_scheduled_with(scene, scheme, settings, options, steps_for_tick, &|_| options.world_effort)
}

/// `run_scheduled`, with the extra solver work of the mechanisms' world following `effort_for_tick(index)` as well.
pub fn run_scheduled_with(scene: &Scene, scheme: Scheme, settings: Settings, options: Options, steps_for_tick: &dyn Fn(usize) -> usize, effort_for_tick: &dyn Fn(usize) -> (usize, usize)) -> Vec<Vec<CarState>> {
    let mut sim = Sim::new(scene, scheme, settings);
    sim.ghost_calls = options.ghost_calls;
    sim.ghost_effort = options.ghost_effort;
    sim.adaptive = options.adaptive;
    sim.calls_rule = options.calls_rule;
    sim.toi = options.toi;
    if options.own_ground {
        sim.set_own_ground();
    }
    sim.set_birth(options.ghost_born_at);
    let mut frames = vec![sim.states()];
    let ticks = (scene.seconds / sim.settings.tick_length).round() as usize;
    for tick in 0..ticks {
        let steps = steps_for_tick(tick).max(1);
        sim.set_step_length(sim.settings.tick_length / steps as f64);
        sim.set_world_effort(effort_for_tick(tick));
        for _ in 0..steps {
            sim.step();
        }
        frames.push(sim.states());
    }
    frames
}

fn parry_shape(shape: Shape) -> SharedShape {
    match shape {
        Shape::Ball(radius) => SharedShape::ball(radius),
        Shape::Cuboid(half) => SharedShape::cuboid(half.x, half.y, half.z),
    }
}

/// How deep (metres, 0 if apart) two cars of a frame overlap.
pub fn overlap(scene: &Scene, frame: &[CarState], a: usize, b: usize) -> f64 {
    let (shape_a, shape_b) = (parry_shape(scene.cars[a].shape), parry_shape(scene.cars[b].shape));
    match rapier3d::parry::query::contact(&frame[a].pose, &*shape_a, &frame[b].pose, &*shape_b, 0.0) {
        Ok(Some(contact)) => (-contact.dist).max(0.0),
        _ => 0.0,
    }
}

/// How deep (metres, 0 if clear) a car of a frame is in the ground.
pub fn overlap_ground(scene: &Scene, frame: &[CarState], car: usize) -> f64 {
    let shape_car = parry_shape(scene.cars[car].shape);
    scene
        .ground
        .iter()
        .map(|ground| {
            let shape_ground = parry_shape(ground.shape);
            match rapier3d::parry::query::contact(&frame[car].pose, &*shape_car, &ground.pose, &*shape_ground, 0.0) {
                Ok(Some(contact)) => (-contact.dist).max(0.0),
                _ => 0.0,
            }
        })
        .fold(0.0, f64::max)
}
