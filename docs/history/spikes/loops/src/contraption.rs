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
}

impl CarSpec {
    pub fn new(name: &str, group: usize, shape: Shape, mass: f64, place: Vector) -> Self {
        Self { name: name.to_string(), group, shape, mass, pose: Pose::from_translation(place), linvel: Vector::ZERO, angvel: Vector::ZERO, friction: 0.8, inertia: None }
    }

    pub fn with_inertia(mut self, inertia: Vector) -> Self {
        self.inertia = Some(inertia);
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
}

impl Scene {
    pub fn new(title: &str, seconds: f64) -> Self {
        Self { title: title.to_string(), cars: Vec::new(), joints: Vec::new(), ground: Vec::new(), gravity: Vector::ZERO, pushes: Vec::new(), changes: Vec::new(), seconds }
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

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Scheme {
    Hi,
    Game,
    Proxy { partition: Partition, handback: Handback, rounds: usize },
    Ghost,
    /// Two ghost worlds, one with the contacts and one without; what the contacts did (the difference of the two, in places and velocities) is given to the truth.
    GhostDifference,
}

impl Scheme {
    pub fn name(self) -> String {
        match self {
            Scheme::Hi => "one world, converged".to_string(),
            Scheme::Game => "one world, game (today)".to_string(),
            Scheme::Ghost => "ghost world, impulses".to_string(),
            Scheme::GhostDifference => "ghost worlds, difference".to_string(),
            Scheme::Proxy { partition, handback, rounds } => {
                let part = match partition {
                    Partition::Whole => "whole",
                    Partition::Clusters => "cluster",
                    Partition::PerCar => "per car",
                };
                let back = match handback {
                    Handback::Rigid => "rigid",
                    Handback::Impulse => "impulse",
                };
                if rounds == 0 { format!("{part}, {back}") } else { format!("{part}, {back}, +{rounds} rounds") }
            }
        }
    }
}

pub const WHOLE_RIGID: Scheme = Scheme::Proxy { partition: Partition::Whole, handback: Handback::Rigid, rounds: 0 };
pub const WHOLE_IMPULSE: Scheme = Scheme::Proxy { partition: Partition::Whole, handback: Handback::Impulse, rounds: 0 };
pub const CLUSTER_IMPULSE: Scheme = Scheme::Proxy { partition: Partition::Clusters, handback: Handback::Impulse, rounds: 0 };
pub const PER_CAR: Scheme = Scheme::Proxy { partition: Partition::PerCar, handback: Handback::Impulse, rounds: 0 };

pub fn hi_settings() -> Settings {
    Settings { steps_per_tick: 12, substeps: 16, pgs_iterations: 32, ..Settings::default() }
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
}

pub fn build(scene: &Scene, settings: Settings, only_group: Option<usize>, colliders: bool, ground: bool, coulomb: bool) -> Built {
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
            let collider = world.colliders.insert_with_parent(
                car.shape.builder().density(0.0).friction(car.friction).restitution(0.0).collision_groups(car_filter(car.group)),
                body,
                &mut world.bodies,
            );
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
    Built { world, bodies, colliders: collider_handles, joints }
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
    /// Calls of the ghost worlds' step per step of the joint worlds (a ghost lives in another frame, with its own number of steps).
    pub ghost_calls: usize,
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
    for joint in &scene.joints {
        if joint.kind == JointKind::Fixed {
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
        let settings = if scheme == Scheme::Hi { hi_settings() } else { settings };
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
            ghost_calls: 1,
            worst_balance: 0.0,
            spent: Duration::ZERO,
            steps_with_hits: 0,
        };
        match scheme {
            Scheme::Hi | Scheme::Game => sim.one = Some(build(scene, settings, None, true, true, true)),
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
        self.apply_changes();
        let forces = self.forces_at(self.time);
        match self.scheme {
            Scheme::Hi | Scheme::Game => {
                let built = self.one.as_mut().expect("one world");
                let none = vec![(Vector::ZERO, Vector::ZERO); forces.len()];
                Self::apply_forces(&mut built.world, &built.bodies, &forces, &none);
                built.world.step();
            }
            Scheme::Ghost => self.ghost_step(&forces),
            Scheme::GhostDifference => self.difference_step(&forces),
            Scheme::Proxy { partition: _, handback, rounds } => self.proxy_step(&forces, handback, rounds),
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
                        hits.push(Hit { car, key, impulse: on_first, point: pose1.transform_point(point.local_p1), normal_part: normal_on_first });
                    }
                    if let Some(car) = car2 {
                        hits.push(Hit { car, key, impulse: -on_first, point: pose2.transform_point(point.local_p2), normal_part: -normal_on_first });
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

    fn ghost_step(&mut self, forces: &[Vector]) {
        let truth = self.truth_states();
        let mut ghost = self.ghost.take().expect("ghost");
        for (i, state) in truth.iter().enumerate() {
            write_body(&mut ghost.world, ghost.bodies[i].expect("body"), *state);
        }
        let none = vec![(Vector::ZERO, Vector::ZERO); forces.len()];
        Self::apply_forces(&mut ghost.world, &ghost.bodies, forces, &none);
        self.owner_from(&ghost);
        let mut hits = Vec::new();
        let saved = ghost.world.parameters.dt;
        ghost.world.parameters.dt = self.dt / self.ghost_calls as f64;
        for call in 0..self.ghost_calls {
            ghost.world.step();
            self.read_hits(&ghost.world, call, &mut hits);
        }
        ghost.world.parameters.dt = saved;
        self.ghost = Some(ghost);
        if !hits.is_empty() {
            self.steps_with_hits += 1;
        }
        let kicks = self.kicks_from_hits(&hits, &truth);
        let handed_back = self.kick_forces(&kicks, &truth);
        self.step_joint_worlds(forces, &handed_back);
    }

    /// Steps a ghost world from the truth's state (in `ghost_calls` pieces); returns the cars' states after.
    fn ghost_states(&self, built: &mut Built, truth: &[CarState], forces: &[Vector]) -> Vec<CarState> {
        for (i, state) in truth.iter().enumerate() {
            write_body(&mut built.world, built.bodies[i].expect("body"), *state);
        }
        let none = vec![(Vector::ZERO, Vector::ZERO); forces.len()];
        Self::apply_forces(&mut built.world, &built.bodies, forces, &none);
        let saved = built.world.parameters.dt;
        built.world.parameters.dt = self.dt / self.ghost_calls as f64;
        for _ in 0..self.ghost_calls {
            built.world.step();
        }
        built.world.parameters.dt = saved;
        built.bodies.iter().map(|b| read_body(&built.world, b.expect("body"))).collect()
    }

    fn difference_step(&mut self, forces: &[Vector]) {
        let truth = self.truth_states();
        let mut with = self.ghost.take().expect("ghost");
        let mut without = self.free_ghost.take().expect("free ghost");
        let a = self.ghost_states(&mut with, &truth, forces);
        let b = self.ghost_states(&mut without, &truth, forces);
        self.ghost = Some(with);
        self.free_ghost = Some(without);
        let none = vec![(Vector::ZERO, Vector::ZERO); forces.len()];
        self.step_joint_worlds(forces, &none);
        let after = self.truth_states();
        let mut changed = false;
        for i in 0..after.len() {
            let (dx, dv, dw) = (a[i].pose.translation - b[i].pose.translation, a[i].linvel - b[i].linvel, a[i].angvel - b[i].angvel);
            let dq = a[i].pose.rotation * b[i].pose.rotation.inverse();
            let moved = dx.length_squared() > 0.0 || dv.length_squared() > 0.0 || dw.length_squared() > 0.0 || dq != Rotation::IDENTITY;
            if !moved {
                continue;
            }
            changed = true;
            let group = &mut self.groups[self.scene.cars[i].group];
            let body = &mut group.world.bodies[group.bodies[i].expect("body")];
            body.set_position(Pose::from_parts(after[i].pose.translation + dx, (dq * after[i].pose.rotation).normalize()), false);
            body.set_linvel(after[i].linvel + dv, false);
            body.set_angvel(after[i].angvel + dw, false);
        }
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

    fn proxy_step(&mut self, forces: &[Vector], handback: Handback, rounds: usize) {
        let dt = self.dt;
        let truth = self.truth_states();
        let count = self.scene.cars.len();
        // The velocities the contact world starts the step with: the truth's own (and the pushes' share of the step), to begin with. The contact world has gravity.
        let pull = self.scene.gravity * dt;
        let mut free: Vec<(Vector, Vector)> = (0..count).map(|i| (truth[i].linvel + forces[i] / self.scene.cars[i].mass * dt, truth[i].angvel)).collect();
        let mut kicks: Vec<Kick> = vec![(Vector::ZERO, Vector::ZERO); count];
        let mut final_hits: Vec<Hit> = Vec::new();
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
        let handed_back = self.kick_forces(&kicks, &truth);
        self.step_joint_worlds(forces, &handed_back);
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
}

impl Default for Options {
    fn default() -> Self {
        Self { contact_calls: 1, ghost_calls: 1 }
    }
}

pub fn run_options(scene: &Scene, scheme: Scheme, settings: Settings, options: Options) -> Trace {
    let mut sim = Sim::new(scene, scheme, settings);
    sim.contact_calls = options.contact_calls;
    sim.ghost_calls = options.ghost_calls;
    let mut frames = vec![sim.states()];
    let ticks = (scene.seconds / sim.settings.tick_length).round() as usize;
    for _ in 0..ticks {
        for _ in 0..sim.settings.steps_per_tick {
            sim.step();
            frames.push(sim.states());
        }
    }
    Trace { dt: sim.dt, frames, worst_balance: sim.worst_balance, micros_per_tick: sim.spent.as_secs_f64() * 1e6 / ticks as f64, steps_with_hits: sim.steps_with_hits }
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
