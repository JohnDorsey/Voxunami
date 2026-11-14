//! The scenes of the study of `docs/history/loop-probe.md` (sections 15 to 18), as builders, shared by the tables of `contraptions` and the sweeps of `ghosts`: chains hit
//! along and across, a tower, a ship on sprung legs, a door on a motor, a rover, a carrier with a ship on its deck. A scene is data (`contraption::Scene`); nothing here steps one.

use crate::contraption::Shape;
use crate::contraption::*;
use crate::*;

pub fn v(x: f64, y: f64, z: f64) -> Vector {
    Vector::new(x, y, z)
}

pub fn max_of(values: impl Iterator<Item = f64>) -> f64 {
    values.fold(f64::NEG_INFINITY, f64::max)
}

pub fn min_of(values: impl Iterator<Item = f64>) -> f64 {
    values.fold(f64::INFINITY, f64::min)
}

pub fn mean_of(values: impl Iterator<Item = f64>) -> f64 {
    let (sum, count) = values.fold((0.0, 0usize), |(s, n), x| (s + x, n + 1));
    sum / count.max(1) as f64
}

pub fn fastest(frame: &[CarState]) -> f64 {
    frame.iter().map(|s| s.linvel.length()).fold(0.0, f64::max)
}

#[derive(Clone, Copy, PartialEq)]
pub enum Hit {
    Axial,
    Transverse,
}

pub fn chain_scene(kind: JointKind, hit: Hit, stranger_mass: f64, cars: usize) -> Scene {
    let mut scene = Scene::new("chain", 1.0);
    let own_inertia = Vector::splat(1000.0 / 6.0);
    for k in 0..cars {
        scene.add_car(CarSpec::new(&format!("car{k}"), 0, Shape::Ball(0.5), 1000.0, v(k as f64, 0.0, 0.0)).friction(0.0).with_inertia(own_inertia));
    }
    for k in 0..cars - 1 {
        let joint = JointSpec::new(kind, k, v(0.5, 0.0, 0.0), k + 1, v(-0.5, 0.0, 0.0)).axis(Vector::Z);
        scene.add_joint(joint);
    }
    let last = (cars - 1) as f64;
    let (place, velocity) = match hit {
        Hit::Axial => (v(last + 1.5, 0.0, 0.0), v(-5.0, 0.0, 0.0)),
        Hit::Transverse => (v(last, 1.5, 0.0), v(0.0, -5.0, 0.0)),
    };
    scene.add_car(CarSpec::new("stranger", 1, Shape::Ball(0.5), stranger_mass, place).moving(velocity).friction(0.0));
    scene
}

pub fn ground_floor(friction: f64) -> Ground {
    Ground { shape: Shape::Cuboid(v(200.0, 0.5, 200.0)), pose: Pose::from_translation(v(0.0, -0.5, 0.0)), friction }
}

pub fn tower_scene(cars: usize, seconds: f64) -> Scene {
    let mut scene = Scene::new("tower", seconds);
    scene.gravity = v(0.0, -9.81, 0.0);
    scene.ground.push(ground_floor(0.6));
    for k in 0..cars {
        scene.add_car(CarSpec::new(&format!("cube{k}"), 0, Shape::Cuboid(Vector::splat(0.5)), 1000.0, v(0.0, 0.5 + k as f64, 0.0)).friction(0.6));
    }
    for k in 0..cars - 1 {
        scene.add_joint(JointSpec::new(JointKind::Fixed, k, v(0.0, 0.5, 0.0), k + 1, v(0.0, -0.5, 0.0)));
    }
    scene
}

#[derive(Clone)]
pub struct Legs {
    pub hull_half: Vector,
    pub hull_mass: f64,
    pub foot_half: Vector,
    pub foot_mass: f64,
    /// Offsets of the legs from the hull centre in x and z.
    pub stance: Vec<(f64, f64)>,
    /// The strut: its free length (the extension the spring wants), its range, and the spring and damper (N/m, N s/m).
    pub free_extension: f64,
    pub range: [f64; 2],
    pub stiffness: f64,
    pub damping: f64,
    pub max_force: f64,
    pub friction: f64,
}

pub struct ShipIds {
    pub hull: usize,
    pub feet: Vec<usize>,
    pub struts: Vec<usize>,
}

impl Legs {
    /// How far the lowest point of the ship is below its hull centre, with the struts at their free length.
    pub fn drop_below_hull(&self) -> f64 {
        self.hull_half.y + self.free_extension + 2.0 * self.foot_half.y
    }
}

pub fn add_ship(scene: &mut Scene, group: usize, name: &str, legs: &Legs, hull_centre: Vector, velocity: Vector) -> ShipIds {
    let hull = scene.add_car(CarSpec::new(&format!("{name} hull"), group, Shape::Cuboid(legs.hull_half), legs.hull_mass, hull_centre).moving(velocity).friction(legs.friction));
    let mut feet = Vec::new();
    let mut struts = Vec::new();
    for (k, &(x, z)) in legs.stance.iter().enumerate() {
        let top = hull_centre + v(x, -legs.hull_half.y - legs.free_extension, z);
        let foot = scene.add_car(
            CarSpec::new(&format!("{name} foot{k}"), group, Shape::Cuboid(legs.foot_half), legs.foot_mass, top - v(0.0, legs.foot_half.y, 0.0)).moving(velocity).friction(legs.friction),
        );
        let strut = JointSpec::new(JointKind::Slider, hull, v(x, -legs.hull_half.y, z), foot, v(0.0, legs.foot_half.y, 0.0))
            .axis(v(0.0, -1.0, 0.0))
            .limits(legs.range[0], legs.range[1])
            .motor(Motor::hold(legs.free_extension, legs.stiffness, legs.damping, legs.max_force));
        feet.push(foot);
        struts.push(scene.add_joint(strut));
    }
    ShipIds { hull, feet, struts }
}

/// A 8.4-tonne ship: an 8 t hull on four 100 kg feet; struts of 78 kN/m (0.25 m of sag under a leg's share of the weight), damped to 0.7 of critical.
pub fn small_legs() -> Legs {
    Legs {
        hull_half: v(1.5, 0.5, 1.5),
        hull_mass: 8000.0,
        foot_half: v(0.3, 0.1, 0.3),
        foot_mass: 100.0,
        stance: vec![(1.2, 1.2), (-1.2, 1.2), (1.2, -1.2), (-1.2, -1.2)],
        free_extension: 0.7,
        range: [0.05, 0.8],
        stiffness: 78e3,
        damping: 17.5e3,
        max_force: f64::INFINITY,
        friction: 0.8,
    }
}

pub fn gear_scene(landing_speed: f64, seconds: f64) -> (Scene, ShipIds) {
    let legs = small_legs();
    let mut scene = Scene::new("landing gear", seconds);
    scene.gravity = v(0.0, -9.81, 0.0);
    scene.ground.push(ground_floor(0.8));
    let hull_height = legs.drop_below_hull() + 0.2;
    let ids = add_ship(&mut scene, 0, "ship", &legs, v(0.0, hull_height, 0.0), v(0.0, -landing_speed, 0.0));
    (scene, ids)
}

pub fn door_scene(stiff: bool, stranger_mass: f64, speed: f64) -> Scene {
    let mut scene = Scene::new(if stiff { "door, locked" } else { "door, on a spring" }, 3.0);
    let hull = scene.add_car(CarSpec::new("hull", 0, Shape::Cuboid(v(2.0, 1.0, 2.0)), 6000.0, v(0.0, 0.0, 0.0)));
    let door = scene.add_car(CarSpec::new("door", 0, Shape::Cuboid(v(1.0, 1.0, 0.05)), 150.0, v(3.0, 0.0, 0.0)));
    let motor = if stiff { Motor::hold(0.0, 5e6, 5e5, f64::INFINITY) } else { Motor::hold(0.0, 30e3, 6e3, 15e3) };
    scene.add_joint(JointSpec::new(JointKind::Hinge, hull, v(2.0, 0.0, 0.0), door, v(-1.0, 0.0, 0.0)).axis(Vector::Y).limits(-1.7, 1.7).motor(motor));
    scene.add_car(CarSpec::new("stranger", 1, Shape::Ball(0.5), stranger_mass, v(4.0, 0.0, 1.05)).moving(v(0.0, 0.0, -speed)).friction(0.0));
    scene
}

pub struct RoverIds {
    pub chassis: usize,
    pub wheels: Vec<usize>,
}

pub fn rover_scene(drive: bool, coasting: bool) -> (Scene, RoverIds) {
    let mut scene = Scene::new(if coasting { "rover coasting" } else { "rover driving" }, 8.0);
    scene.gravity = v(0.0, -9.81, 0.0);
    scene.ground.push(ground_floor(1.0));
    let speed = if coasting { 5.0 } else { 0.0 };
    let chassis = scene.add_car(CarSpec::new("chassis", 0, Shape::Cuboid(v(1.5, 0.3, 1.0)), 1500.0, v(0.0, 0.7, 0.0)).moving(v(speed, 0.0, 0.0)).friction(0.3));
    let mut wheels = Vec::new();
    for (k, &(x, z)) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)].iter().enumerate() {
        let mut wheel = CarSpec::new(&format!("wheel{k}"), 0, Shape::Ball(0.4), 40.0, v(x, 0.4, z * 1.4)).moving(v(speed, 0.0, 0.0)).friction(1.0);
        if coasting {
            wheel.angvel = v(0.0, 0.0, -speed / 0.4);
        }
        let wheel = scene.add_car(wheel);
        let mut hinge = JointSpec::new(JointKind::Hinge, chassis, v(x, -0.3, z), wheel, v(0.0, 0.0, -z * 0.4)).axis(Vector::Z);
        if drive && x < 0.0 {
            hinge = hinge.motor(Motor::drive(-12.5, 300.0, 500.0));
        }
        wheels.push(wheel);
        scene.add_joint(hinge);
    }
    (scene, RoverIds { chassis, wheels })
}

#[derive(Clone, Copy, PartialEq)]
pub enum CarrierRun {
    Rest,
    Drive,
    Land,
}

pub fn carrier_legs() -> Legs {
    Legs {
        hull_half: v(4.0, 0.5, 6.0),
        hull_mass: 20000.0,
        foot_half: v(0.4, 0.1, 0.4),
        foot_mass: 200.0,
        stance: vec![(3.0, 5.0), (-3.0, 5.0), (3.0, -5.0), (-3.0, -5.0)],
        free_extension: 0.7,
        range: [0.05, 0.8],
        stiffness: 240e3,
        damping: 53e3,
        max_force: f64::INFINITY,
        friction: 0.8,
    }
}

pub fn carried_legs() -> Legs {
    Legs {
        hull_half: v(1.0, 0.4, 1.5),
        hull_mass: 3000.0,
        foot_half: v(0.25, 0.1, 0.25),
        foot_mass: 60.0,
        stance: vec![(0.8, 1.2), (-0.8, 1.2), (0.8, -1.2), (-0.8, -1.2)],
        free_extension: 0.5,
        range: [0.05, 0.6],
        stiffness: 40e3,
        damping: 8e3,
        max_force: f64::INFINITY,
        friction: 0.8,
    }
}

pub fn carrier_scene(run_kind: CarrierRun) -> (Scene, ShipIds, ShipIds) {
    let carrier_legs = carrier_legs();
    let carried_legs = carried_legs();
    let mut scene = Scene::new(
        match run_kind {
            CarrierRun::Rest => "carrier at rest",
            CarrierRun::Drive => "carrier accelerating",
            CarrierRun::Land => "ship landing on the carrier",
        },
        if run_kind == CarrierRun::Rest { 10.0 } else { 7.0 },
    );
    scene.gravity = v(0.0, -9.81, 0.0);
    scene.ground.push(ground_floor(0.8));
    let carrier_height = carrier_legs.drop_below_hull() + 0.01;
    let carrier = add_ship(&mut scene, 0, "carrier", &carrier_legs, v(0.0, carrier_height, 0.0), Vector::ZERO);
    let deck = carrier_height + carrier_legs.hull_half.y;
    let (gap, speed) = if run_kind == CarrierRun::Land { (0.3, 1.5) } else { (0.01, 0.0) };
    let carried_height = deck + carried_legs.drop_below_hull() + gap;
    let carried = add_ship(&mut scene, 1, "carried", &carried_legs, v(0.0, carried_height, 0.0), v(0.0, -speed, 0.0));
    if run_kind == CarrierRun::Drive {
        scene.pushes.push(Push { car: carrier.hull, force: v(30e3, 0.0, 0.0), from: 1.0, until: 4.0 });
    }
    (scene, carrier, carried)
}

pub fn stiff_approach_scene(speed: f64) -> Scene {
    let mut scene = chain_scene(JointKind::Fixed, Hit::Axial, 1000.0, 4);
    let last = scene.cars.len() - 1;
    scene.cars[last].pose = Pose::from_translation(v(3.0 + 0.5 + 5.0 + 0.5, 0.0, 0.0));
    scene.cars[last].linvel = v(-speed, 0.0, 0.0);
    scene.seconds = (5.0 / speed + 1.0).max(1.0);
    scene
}
