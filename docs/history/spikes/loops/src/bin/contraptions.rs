//! Ways to hand a hit back to a mechanism, tried on what players build: chains, landing gear, doors, rovers, a ship carrying a ship (the study of
//! `docs/history/loop-probe.md`, section 15). The kit is `loop_probe::contraption`; this file is the scenes and the tables.
//!
//! Run: `cargo run --release --bin contraptions -- regress|rest|gear|door|rover|carrier|friction|long|squeeze|approach|cost|all`.

use loop_probe::contraption::Shape;
use loop_probe::contraption::*;
use loop_probe::*;

const PER_CAR_ROUNDS_2: Scheme = Scheme::Proxy { partition: Partition::PerCar, handback: Handback::Impulse, rounds: 2, feedback: Feedback::None };
const WHOLE_ROUNDS_2: Scheme = Scheme::Proxy { partition: Partition::Whole, handback: Handback::Impulse, rounds: 2, feedback: Feedback::None };

/// The schemes of the main tables, in the order they are printed (the first is the reference).
const SCHEMES: [Scheme; 10] = [Scheme::Hi, Scheme::Game, Scheme::Ghost, Scheme::GhostDifference, WHOLE_RIGID, WHOLE_IMPULSE, CLUSTER_IMPULSE, PER_CAR, PER_CAR_ROUNDS_2, WHOLE_ROUNDS_2];

fn v(x: f64, y: f64, z: f64) -> Vector {
    Vector::new(x, y, z)
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------------------
// Tables.

fn print_table(title: &str, columns: &[&str], rows: &[(String, Vec<f64>)], precision: &[usize]) {
    println!("\n{title}");
    print!("{:<30}", "scheme");
    for c in columns {
        print!(" {c:>10}");
    }
    println!();
    for (name, values) in rows {
        print!("{name:<30}");
        for (value, &p) in values.iter().zip(precision) {
            if value.is_nan() {
                print!(" {:>10}", "-");
            } else {
                print!(" {value:>10.p$}");
            }
        }
        println!();
    }
}

fn max_of(values: impl Iterator<Item = f64>) -> f64 {
    values.fold(f64::NEG_INFINITY, f64::max)
}

fn min_of(values: impl Iterator<Item = f64>) -> f64 {
    values.fold(f64::INFINITY, f64::min)
}

fn mean_of(values: impl Iterator<Item = f64>) -> f64 {
    let (sum, count) = values.fold((0.0, 0usize), |(s, n), x| (s + x, n + 1));
    sum / count.max(1) as f64
}

fn fastest(frame: &[CarState]) -> f64 {
    frame.iter().map(|s| s.linvel.length()).fold(0.0, f64::max)
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------------------
// The chain of `handback.rs`, in the kit (the regression: the kit must give those numbers).

#[derive(Clone, Copy, PartialEq)]
enum Hit {
    Axial,
    Transverse,
}

fn chain_scene(kind: JointKind, hit: Hit, stranger_mass: f64, cars: usize) -> Scene {
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

fn regress() {
    println!("regression against handback.rs (hinged chain of 4, hit across by 1000 kg at 5 m/s, after 1 s): stranger exit velocity, chain spin, end car off the converged one (mm)");
    println!("old kit: converged (+0.195, -2.118) spin -4.288; game (+0.175, -2.131); whole rigid (+0.001, -1.939); whole impulse (+0.073, -1.949) spin -4.561 end car 109 mm; per car (+0.163, -2.119)");
    let scene = chain_scene(JointKind::Hinge, Hit::Transverse, 1000.0, 4);
    let reference = run(&scene, Scheme::Hi, Settings::default());
    for scheme in [Scheme::Hi, Scheme::Game, WHOLE_RIGID, WHOLE_IMPULSE, PER_CAR] {
        let t = run(&scene, scheme, Settings::default());
        let end = t.last();
        let stranger = end[4];
        let off = (end[3].pose.translation - reference.last()[3].pose.translation).length() * 1e3;
        println!("{:<28} stranger ({:+.3}, {:+.3})  end car off {:>6.1} mm  balance {:.1e}", scheme.name(), stranger.linvel.x, stranger.linvel.y, off, t.worst_balance);
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------------------
// A tower of fixed-jointed cubes on a floor (split.rs), at rest: does each scheme keep it standing?

fn ground_floor(friction: f64) -> Ground {
    Ground { shape: Shape::Cuboid(v(200.0, 0.5, 200.0)), pose: Pose::from_translation(v(0.0, -0.5, 0.0)), friction }
}

fn tower_scene(cars: usize, seconds: f64) -> Scene {
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

fn tower() {
    let scene = tower_scene(4, 15.0);
    let mut rows = Vec::new();
    for scheme in SCHEMES {
        let t = run(&scene, scheme, Settings::default());
        let last = t.since(10.0);
        let sag = mean_of(last.iter().map(|f| f[0].pose.translation.y - 0.5)) * 1e3;
        let jitter = max_of(last.iter().map(|f| fastest(f))) * 1e3;
        let gap = max_of(t.frames.iter().map(|f| scene.joints.iter().map(|j| joint_gap(j, f)).fold(0.0, f64::max))) * 1e6;
        let off = max_of(t.frames.iter().map(|f| f.iter().map(|s| (s.pose.translation.x.powi(2) + s.pose.translation.z.powi(2)).sqrt()).fold(0.0, f64::max))) * 1e3;
        rows.push((scheme.name(), vec![sag, jitter, gap, off, t.worst_balance, t.micros_per_tick]));
    }
    print_table(
        "tower of 4 cubes (1000 kg, fixed joints) on a floor, 15 s, game step; sag of the lowest cube (mm, + above its rest), jitter (mm/s), joint gap (um), off axis (mm)",
        &["sag mm", "jitter", "gap um", "off axis", "balance", "us/tick"],
        &rows,
        &[3, 3, 1, 2, 1, 0],
    );
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------------------
// A ship on legs: a hull, and feet on struts (sliders with a spring, a damper and stops).

#[derive(Clone)]
struct Legs {
    hull_half: Vector,
    hull_mass: f64,
    foot_half: Vector,
    foot_mass: f64,
    /// Offsets of the legs from the hull centre in x and z.
    stance: Vec<(f64, f64)>,
    /// The strut: its free length (the extension the spring wants), its range, and the spring and damper (N/m, N s/m).
    free_extension: f64,
    range: [f64; 2],
    stiffness: f64,
    damping: f64,
    max_force: f64,
    friction: f64,
}

struct ShipIds {
    hull: usize,
    feet: Vec<usize>,
    struts: Vec<usize>,
}

impl Legs {
    /// How far the lowest point of the ship is below its hull centre, with the struts at their free length.
    fn drop_below_hull(&self) -> f64 {
        self.hull_half.y + self.free_extension + 2.0 * self.foot_half.y
    }
}

fn add_ship(scene: &mut Scene, group: usize, name: &str, legs: &Legs, hull_centre: Vector, velocity: Vector) -> ShipIds {
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
fn small_legs() -> Legs {
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

fn gear_scene(landing_speed: f64, seconds: f64) -> (Scene, ShipIds) {
    let legs = small_legs();
    let mut scene = Scene::new("landing gear", seconds);
    scene.gravity = v(0.0, -9.81, 0.0);
    scene.ground.push(ground_floor(0.8));
    let hull_height = legs.drop_below_hull() + 0.2;
    let ids = add_ship(&mut scene, 0, "ship", &legs, v(0.0, hull_height, 0.0), v(0.0, -landing_speed, 0.0));
    (scene, ids)
}

struct GearReport {
    peak_g: f64,
    least_extension: f64,
    final_height: f64,
    final_height_error_mm: f64,
    jitter_mm_s: f64,
    foot_below_mm: f64,
    rebound: f64,
    tilt_mrad: f64,
    worst_gap_mm: f64,
}

fn gear_report(scene: &Scene, ids: &ShipIds, t: &Trace, reference: &Trace) -> GearReport {
    let hull = ids.hull;
    let dt = t.dt;
    let mut peak = 0.0f64;
    for pair in t.frames.windows(2) {
        let a = (pair[1][hull].linvel.y - pair[0][hull].linvel.y) / dt;
        peak = peak.max((a + 9.81) / 9.81);
    }
    let least_extension = min_of(t.frames.iter().flat_map(|f| ids.struts.iter().map(move |&j| joint_coordinate(&scene.joints[j], f))));
    let last = t.since(scene.seconds - 3.0);
    let final_height = mean_of(last.iter().map(|f| f[hull].pose.translation.y));
    let reference_height = mean_of(reference.since(scene.seconds - 3.0).iter().map(|f| f[hull].pose.translation.y));
    let jitter = max_of(last.iter().map(|f| fastest(f))) * 1e3;
    let foot_below = -min_of(t.frames.iter().flat_map(|f| ids.feet.iter().map(move |&c| f[c].pose.translation.y - 0.1))) * 1e3;
    let first_contact = t.frames.iter().position(|f| ids.feet.iter().any(|&c| f[c].pose.translation.y - 0.1 < 0.01)).unwrap_or(0);
    let rebound = max_of(t.frames[first_contact..].iter().map(|f| f[hull].linvel.y));
    let tilt = max_of(t.frames.iter().map(|f| {
        let up = f[hull].pose.rotation * v(0.0, 1.0, 0.0);
        (up.x * up.x + up.z * up.z).sqrt()
    })) * 1e3;
    let worst_gap = max_of(t.frames.iter().map(|f| ids.struts.iter().map(|&j| joint_gap(&scene.joints[j], f)).fold(0.0, f64::max))) * 1e3;
    GearReport { peak_g: peak, least_extension, final_height, final_height_error_mm: (final_height - reference_height) * 1e3, jitter_mm_s: jitter, foot_below_mm: foot_below, rebound, tilt_mrad: tilt, worst_gap_mm: worst_gap }
}

fn gear() {
    for speed in [2.0, 4.0] {
        let (scene, ids) = gear_scene(speed, 8.0);
        let reference = run(&scene, Scheme::Hi, Settings::default());
        let mut rows = Vec::new();
        for scheme in SCHEMES {
            let t = run(&scene, scheme, Settings::default());
            let r = gear_report(&scene, &ids, &t, &reference);
            rows.push((scheme.name(), vec![r.peak_g, r.least_extension, r.final_height, r.final_height_error_mm, r.jitter_mm_s, r.foot_below_mm, r.rebound, r.tilt_mrad, r.worst_gap_mm, t.worst_balance]));
        }
        print_table(
            &format!("landing gear: 8.4 t ship on four sprung legs lands at {speed} m/s, 8 s. peak g felt, least strut extension (m; 0.05 is bottomed), settled hull height (m), error vs converged (mm), jitter in the last 3 s (mm/s), deepest foot below the floor (mm), rebound (m/s), tilt (mrad), strut gap (mm)"),
            &["peak g", "least ext", "height", "err mm", "jitter", "foot below", "rebound", "tilt mrad", "gap mm", "balance"],
            &rows,
            &[2, 3, 3, 1, 2, 1, 2, 2, 2, 1],
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------------------
// A door: a flap on a hinge held by a motor, hit by a stranger.

fn door_scene(stiff: bool, stranger_mass: f64, speed: f64) -> Scene {
    let mut scene = Scene::new(if stiff { "door, locked" } else { "door, on a spring" }, 3.0);
    let hull = scene.add_car(CarSpec::new("hull", 0, Shape::Cuboid(v(2.0, 1.0, 2.0)), 6000.0, v(0.0, 0.0, 0.0)));
    let door = scene.add_car(CarSpec::new("door", 0, Shape::Cuboid(v(1.0, 1.0, 0.05)), 150.0, v(3.0, 0.0, 0.0)));
    let motor = if stiff { Motor::hold(0.0, 5e6, 5e5, f64::INFINITY) } else { Motor::hold(0.0, 30e3, 6e3, 15e3) };
    scene.add_joint(JointSpec::new(JointKind::Hinge, hull, v(2.0, 0.0, 0.0), door, v(-1.0, 0.0, 0.0)).axis(Vector::Y).limits(-1.7, 1.7).motor(motor));
    scene.add_car(CarSpec::new("stranger", 1, Shape::Ball(0.5), stranger_mass, v(4.0, 0.0, 1.05)).moving(v(0.0, 0.0, -speed)).friction(0.0));
    scene
}

fn door() {
    for (stiff, stranger_mass) in [(false, 1000.0), (false, 100.0), (true, 1000.0)] {
        let scene = door_scene(stiff, stranger_mass, 4.0);
        let reference = run(&scene, Scheme::Hi, Settings::default());
        let mut rows = Vec::new();
        for scheme in SCHEMES {
            let t = run(&scene, scheme, Settings::default());
            let angle = |f: &Vec<CarState>| joint_coordinate(&scene.joints[0], f);
            let peak_angle = max_of(t.frames.iter().map(|f| angle(f).abs()));
            let angle_1s = angle(t.at(1.0));
            let angle_ref_1s = angle(reference.at(1.0));
            let end = t.last();
            let hull_speed = end[0].linvel.length();
            let exit = end[2].linvel.z;
            let gap = max_of(t.frames.iter().map(|f| joint_gap(&scene.joints[0], f))) * 1e3;
            let energy = kinetic_energy(&scene, end) / kinetic_energy(&scene, &t.frames[0]);
            rows.push((scheme.name(), vec![exit, hull_speed, peak_angle, angle_1s, angle_1s - angle_ref_1s, gap, energy, t.worst_balance]));
        }
        print_table(
            &format!("door ({}), stranger {stranger_mass} kg at 4 m/s on the door's free end, 3 s: stranger's exit speed (m/s), hull speed, door's peak angle (rad), angle at 1 s, error vs converged, hinge gap (mm), energy kept", scene.title),
            &["exit vz", "hull m/s", "peak ang", "ang 1 s", "err", "gap mm", "energy", "balance"],
            &rows,
            &[3, 3, 3, 3, 3, 2, 3, 1],
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------------------
// A rover: a chassis on four wheels (hinges), the rear pair driven by motors; and the same coasting.

struct RoverIds {
    chassis: usize,
    wheels: Vec<usize>,
}

fn rover_scene(drive: bool, coasting: bool) -> (Scene, RoverIds) {
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

fn rover() {
    for (drive, coasting) in [(true, false), (false, true)] {
        let (scene, ids) = rover_scene(drive, coasting);
        let reference = run(&scene, Scheme::Hi, Settings::default());
        let mut rows = Vec::new();
        for scheme in SCHEMES {
            let t = run(&scene, scheme, Settings::default());
            let speed = |seconds: f64| t.at(seconds)[ids.chassis].linvel.x;
            let x_end = t.last()[ids.chassis].pose.translation.x;
            let x_error = x_end - reference.last()[ids.chassis].pose.translation.x;
            let bounce = max_of(t.since(1.0).iter().map(|f| f[ids.chassis].linvel.y.abs())) * 1e3;
            let sideways = max_of(t.frames.iter().map(|f| f[ids.chassis].pose.translation.z.abs())) * 1e3;
            let spin = mean_of(ids.wheels.iter().map(|&w| t.last()[w].angvel.z)) * 0.4;
            rows.push((scheme.name(), vec![speed(2.0), speed(4.0), speed(8.0), x_end, x_error, bounce, sideways, -spin, t.worst_balance]));
        }
        print_table(
            &format!("{}: 1.5 t chassis on four 40 kg wheels, friction 1, 8 s. chassis speed at 2, 4 and 8 s (m/s), x at 8 s (m), error vs converged (m), bounce (mm/s), sideways drift (mm), wheel rim speed at 8 s (m/s)", scene.title),
            &["v 2 s", "v 4 s", "v 8 s", "x 8 s", "x err", "bounce", "sideways", "rim m/s", "balance"],
            &rows,
            &[3, 3, 3, 2, 3, 1, 1, 3, 1],
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------------------
// A ship carrying a ship: a carrier on its landing gear with a smaller ship (its own group, its own legs) standing on its deck.

#[derive(Clone, Copy, PartialEq)]
enum CarrierRun {
    Rest,
    Drive,
    Land,
}

fn carrier_legs() -> Legs {
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

fn carried_legs() -> Legs {
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

fn carrier_scene(run_kind: CarrierRun) -> (Scene, ShipIds, ShipIds) {
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

fn carrier() {
    for run_kind in [CarrierRun::Rest, CarrierRun::Drive, CarrierRun::Land] {
        let (scene, carrier, carried) = carrier_scene(run_kind);
        let reference = run(&scene, Scheme::Hi, Settings::default());
        let mut rows = Vec::new();
        for scheme in SCHEMES {
            let t = run(&scene, scheme, Settings::default());
            let end = t.last();
            let start = &t.frames[0];
            let height_of = |f: &Vec<CarState>, ship: &ShipIds| f[ship.hull].pose.translation.y;
            let carrier_error = (height_of(end, &carrier) - height_of(reference.last(), &carrier)) * 1e3;
            let carried_error = (height_of(end, &carried) - height_of(reference.last(), &carried)) * 1e3;
            let slide = |f: &Vec<CarState>, s: &Vec<CarState>| {
                let relative = f[carried.hull].pose.translation - f[carrier.hull].pose.translation;
                let initial = s[carried.hull].pose.translation - s[carrier.hull].pose.translation;
                ((relative.x - initial.x).powi(2) + (relative.z - initial.z).powi(2)).sqrt() * 1e3
            };
            let slide_end = slide(end, start);
            let slide_error = slide_end - slide(reference.last(), &reference.frames[0]);
            let relative_jitter = max_of(t.since(scene.seconds - 2.0).iter().map(|f| {
                f.iter().enumerate().map(|(i, s)| (s.linvel - f[carrier.hull].linvel).y.abs() * if i == carrier.hull { 0.0 } else { 1.0 }).fold(0.0, f64::max)
            })) * 1e3;
            let peak_g = {
                let dt = t.dt;
                let mut peak = 0.0f64;
                for pair in t.frames.windows(2) {
                    peak = peak.max(((pair[1][carried.hull].linvel.y - pair[0][carried.hull].linvel.y) / dt + 9.81) / 9.81);
                }
                peak
            };
            let joints = &scene.joints;
            let least_ext = |ship: &ShipIds| min_of(t.frames.iter().flat_map(|f| ship.struts.iter().map(move |&j| joint_coordinate(&joints[j], f))));
            rows.push((scheme.name(), vec![carrier_error, carried_error, slide_end, slide_error, relative_jitter, peak_g, least_ext(&carrier), least_ext(&carried), t.worst_balance]));
        }
        print_table(
            &format!("{}: a 20 t carrier on four sprung legs with a 3 t ship on four sprung legs on its deck. height error of each hull vs converged (mm), carried ship's slide on the deck (mm) and its error, jitter (mm/s, last 2 s), peak g of the carried hull, least strut extension of each", scene.title),
            &["carrier err", "carried err", "slide mm", "slide err", "jitter", "peak g", "ext carrier", "ext carried", "balance"],
            &rows,
            &[1, 1, 1, 1, 1, 2, 3, 3, 1],
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------------------
// Friction: a block sliding to a stop.

fn friction() {
    let mut scene = Scene::new("slide", 3.0);
    scene.gravity = v(0.0, -9.81, 0.0);
    scene.ground.push(ground_floor(0.5));
    scene.add_car(CarSpec::new("block", 0, Shape::Cuboid(Vector::splat(0.5)), 1000.0, v(0.0, 0.5, 0.0)).moving(v(4.0, 0.0, 0.0)).friction(0.5));
    let mut rows = Vec::new();
    for scheme in SCHEMES {
        let t = run(&scene, scheme, Settings::default());
        let end = t.last()[0];
        let stop = t.frames.iter().position(|f| f[0].linvel.x < 0.01).map_or(f64::NAN, |k| k as f64 * t.dt);
        rows.push((scheme.name(), vec![end.pose.translation.x, stop, end.linvel.x * 1e3, t.worst_balance]));
    }
    print_table(
        "a 1 t block slides at 4 m/s on a floor with friction 0.5 (it stops after 1.63 m, in 0.82 s): where it stopped (m), when its speed fell under 0.01 (s), speed at 3 s (mm/s)",
        &["x end", "stopped s", "v mm/s", "balance"],
        &rows,
        &[3, 3, 2, 1],
    );
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------------------
// Long chains and a chain squeezed from both ends.

fn long() {
    for cars in [8usize, 16] {
        for hit in [Hit::Axial, Hit::Transverse] {
            let scene = chain_scene(JointKind::Hinge, hit, 1000.0, cars);
            let reference = run(&scene, Scheme::Hi, Settings::default());
            let mut rows = Vec::new();
            for scheme in SCHEMES {
                let t = run(&scene, scheme, Settings::default());
                let end = t.last();
                let stranger = end[cars];
                let off = (end[cars - 1].pose.translation - reference.last()[cars - 1].pose.translation).length() * 1e3;
                let off_first = (end[0].pose.translation - reference.last()[0].pose.translation).length() * 1e3;
                let overlap = max_of(t.frames.iter().map(|f| (1.0 - (f[cars - 1].pose.translation - f[cars].pose.translation).length()) * 1e3));
                let gap = max_of(t.frames.iter().map(|f| scene.joints.iter().map(|j| joint_gap(j, f)).fold(0.0, f64::max))) * 1e3;
                rows.push((scheme.name(), vec![stranger.linvel.x, stranger.linvel.y, off, off_first, overlap, gap, t.micros_per_tick]));
            }
            print_table(
                &format!("hinged chain of {cars} cars (1000 kg), hit {} by 1000 kg at 5 m/s, 1 s: stranger's exit velocity, last and first car off the converged chain (mm), deepest overlap (mm), joint gap (mm)", if hit == Hit::Axial { "along it" } else { "across it" }),
                &["exit vx", "exit vy", "last off", "first off", "overlap", "gap mm", "us/tick"],
                &rows,
                &[3, 3, 1, 1, 1, 2, 0],
            );
        }
    }
}

fn squeeze() {
    for kind in [JointKind::Fixed, JointKind::Hinge] {
        let mut scene = chain_scene(kind, Hit::Axial, 1000.0, 4);
        scene.add_car(CarSpec::new("stranger 2", 2, Shape::Ball(0.5), 1000.0, v(-1.5, 0.0, 0.0)).moving(v(5.0, 0.0, 0.0)).friction(0.0));
        let reference = run(&scene, Scheme::Hi, Settings::default());
        let mut rows = Vec::new();
        for scheme in SCHEMES {
            let t = run(&scene, scheme, Settings::default());
            let end = t.last();
            let squeeze = max_of(t.frames.iter().map(|f| 3.0 - (f[3].pose.translation.x - f[0].pose.translation.x))) * 1e3;
            let gap = max_of(t.frames.iter().map(|f| scene.joints.iter().map(|j| joint_gap(j, f)).fold(0.0, f64::max))) * 1e3;
            let off = (end[3].pose.translation - reference.last()[3].pose.translation).length() * 1e3;
            rows.push((scheme.name(), vec![end[4].linvel.x, end[5].linvel.x, squeeze, gap, off, kinetic_energy(&scene, end) / kinetic_energy(&scene, &t.frames[0])]));
        }
        print_table(
            &format!("{:?} chain of 4 squeezed from both ends by 1000 kg at 5 m/s each, 1 s: exit speeds of the two strangers, the chain's greatest shortening (mm), joint gap (mm), last car off the converged one (mm), energy kept", kind),
            &["exit 1", "exit 2", "shortened", "gap mm", "off mm", "energy"],
            &rows,
            &[3, 3, 1, 2, 1, 3],
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------------------
// A stranger that comes from a distance, fast: when is its hit delivered?

fn approach() {
    for speed in [5.0, 20.0, 60.0] {
        let mut scene = chain_scene(JointKind::Fixed, Hit::Axial, 1000.0, 4);
        let last = scene.cars.len() - 1;
        scene.cars[last].pose = Pose::from_translation(v(3.0 + 0.5 + 5.0 + 0.5, 0.0, 0.0));
        scene.cars[last].linvel = v(-speed, 0.0, 0.0);
        scene.seconds = (5.0 / speed + 1.0).max(1.0);
        let reference = run(&scene, Scheme::Hi, Settings::default());
        let mut rows = Vec::new();
        for (scheme, calls) in [(Scheme::Hi, 1), (Scheme::Game, 1), (Scheme::Ghost, 1), (Scheme::GhostDifference, 1), (WHOLE_IMPULSE, 1), (PER_CAR, 1), (WHOLE_RIGID, 1)] {
            let t = run_with(&scene, scheme, Settings::default(), calls);
            let end = t.last();
            let overlap = max_of(t.frames.iter().map(|f| (1.0 - (f[3].pose.translation - f[last].pose.translation).length()) * 1e3));
            let off = (end[3].pose.translation - reference.last()[3].pose.translation).length() * 1e3;
            let momentum = total_momentum(&scene, end) - total_momentum(&scene, &t.frames[0]);
            rows.push((format!("{} x{calls}", scheme.name()), vec![end[last].linvel.x, end[3].linvel.x, overlap, off, momentum.length(), t.worst_balance]));
        }
        print_table(
            &format!("a 1000 kg stranger comes at {speed} m/s from 5 m away onto a stiff chain of 4 (fixed joints), 1 s after: the stranger's and the chain end's speed, deepest overlap (mm), last car off the converged one (mm), momentum change, 'x4' is the contact world stepped in 4 pieces"),
            &["stranger vx", "chain vx", "overlap mm", "off mm", "dP", "balance"],
            &rows,
            &[3, 3, 1, 1, 1, 1],
        );
    }
}

fn cost() {
    println!("\ncost: hinged chains of N cars hit across, microseconds per tick (3 steps) for the whole step (the one world, or the joint worlds and the contact world)");
    println!("{:<30} {:>9} {:>9} {:>9} {:>9}", "scheme", "N=8", "N=32", "N=128", "N=256");
    let mut table: Vec<(String, Vec<f64>)> = Vec::new();
    for scheme in [Scheme::Game, Scheme::Ghost, Scheme::GhostDifference, Scheme::GhostWelded, Scheme::GhostWeldedDifference, WHOLE_RIGID, WHOLE_IMPULSE, CLUSTER_IMPULSE, PER_CAR, PER_CAR_ROUNDS_2, Scheme::Proxy { partition: Partition::Clusters, handback: Handback::Impulse, rounds: 2, feedback: Feedback::Last }] {
        let mut row = Vec::new();
        for cars in [8usize, 32, 128, 256] {
            let mut scene = chain_scene(JointKind::Hinge, Hit::Transverse, 1000.0, cars);
            scene.seconds = 1.0;
            let t = run(&scene, scheme, Settings::default());
            row.push(t.micros_per_tick);
        }
        table.push((scheme.name(), row));
    }
    for (name, row) in table {
        println!("{name:<30} {:>9.0} {:>9.0} {:>9.0} {:>9.0}", row[0], row[1], row[2], row[3]);
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------------------
// Does a stranger that is near and does not touch change a mechanism? (promise (a)). And bit-for-bit fingerprints, for another machine to compare.

fn nearness() {
    let (alone, ids) = gear_scene(2.0, 3.0);
    let mut with = alone.clone();
    with.add_car(CarSpec::new("bystander", 1, Shape::Cuboid(Vector::splat(0.5)), 1000.0, v(3.0, 0.5, 0.0)).friction(0.5));
    let cars = alone.cars.len();
    println!("
the landing-gear ship with and without a 1 t cube standing on the floor 1.5 m from its hull (touching nothing), 3 s: the greatest difference in any car's place (m) and velocity (m/s), and whether every bit agrees");
    println!("{:<34} {:>12} {:>12} {:>10}", "scheme", "place", "velocity", "same bits");
    for (scheme, ghost_calls) in [(Scheme::Hi, 1), (Scheme::Game, 1), (Scheme::Ghost, 1), (Scheme::Ghost, 2), (Scheme::GhostDifference, 1), (Scheme::GhostDifference, 2), (WHOLE_IMPULSE, 1), (PER_CAR, 1), (WHOLE_RIGID, 1)] {
        let options = Options { ghost_calls, ..Options::default() };
        let a = run_options(&alone, scheme, Settings::default(), options);
        let b = run_options(&with, scheme, Settings::default(), options);
        let (mut place, mut velocity) = (0.0f64, 0.0f64);
        for (fa, fb) in a.frames.iter().zip(&b.frames) {
            for i in 0..cars {
                place = place.max((fa[i].pose.translation - fb[i].pose.translation).length());
                velocity = velocity.max((fa[i].linvel - fb[i].linvel).length());
            }
        }
        let name = if ghost_calls > 1 { format!("{} (ghost steps x{ghost_calls})", scheme.name()) } else { scheme.name() };
        println!("{name:<34} {place:>12.3e} {velocity:>12.3e} {:>10}", if place == 0.0 && velocity == 0.0 { "yes" } else { "no" });
    }
    let _ = ids;
}

fn fingerprints() {
    let mut scenes: Vec<(String, Scene)> = Vec::new();
    let mut tower = tower_scene(4, 4.0);
    tower.seconds = 4.0;
    scenes.push(("tower 4 s".to_string(), tower));
    scenes.push(("gear 2 m/s, 4 s".to_string(), gear_scene(2.0, 4.0).0));
    let mut spring = door_scene(false, 1000.0, 4.0);
    spring.seconds = 2.0;
    scenes.push(("door on a spring, 2 s".to_string(), spring));
    let mut rest = carrier_scene(CarrierRun::Rest).0;
    rest.seconds = 3.0;
    scenes.push(("carrier at rest, 3 s".to_string(), rest));
    let mut coasting = rover_scene(false, true).0;
    coasting.seconds = 3.0;
    scenes.push(("rover coasting, 3 s".to_string(), coasting));
    let mut chain = chain_scene(JointKind::Hinge, Hit::Transverse, 1000.0, 4);
    chain.seconds = 1.0;
    scenes.push(("hinged chain hit across, 1 s".to_string(), chain));
    let mut schemes = SCHEMES.to_vec();
    schemes.extend([
        Scheme::GhostWelded,
        Scheme::GhostWeldedDifference,
        Scheme::Proxy { partition: Partition::PerCar, handback: Handback::Impulse, rounds: 0, feedback: Feedback::Last },
        Scheme::Proxy { partition: Partition::Clusters, handback: Handback::Impulse, rounds: 2, feedback: Feedback::Persistent },
    ]);
    for (name, scene) in &scenes {
        for &scheme in &schemes {
            let t = run_options(scene, scheme, Settings::default(), Options { ghost_calls: 2, ..Options::default() });
            println!("fingerprint {name:<30} {:<30} {:016x}", scheme.name(), t.fingerprint());
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------------------
// Sensitivity: the ghost worlds' own step count, the coupling interval, and the rounds of the per-car scheme.

fn variants() {
    let ghost_variants: Vec<(String, Scheme, usize)> = vec![
        ("one world, game (today)".to_string(), Scheme::Game, 1),
        ("ghost impulses, ghost steps x1".to_string(), Scheme::Ghost, 1),
        ("ghost impulses, ghost steps x2".to_string(), Scheme::Ghost, 2),
        ("ghost impulses, ghost steps x4".to_string(), Scheme::Ghost, 4),
        ("ghost difference, ghost steps x1".to_string(), Scheme::GhostDifference, 1),
        ("ghost difference, ghost steps x2".to_string(), Scheme::GhostDifference, 2),
        ("ghost difference, ghost steps x4".to_string(), Scheme::GhostDifference, 4),
    ];
    // Landing gear.
    {
        let (scene, ids) = gear_scene(2.0, 8.0);
        let reference = run(&scene, Scheme::Hi, Settings::default());
        let mut rows = Vec::new();
        for (name, scheme, calls) in &ghost_variants {
            let t = run_options(&scene, *scheme, Settings::default(), Options { ghost_calls: *calls, ..Options::default() });
            let r = gear_report(&scene, &ids, &t, &reference);
            rows.push((name.clone(), vec![r.peak_g, r.least_extension, r.final_height_error_mm, r.jitter_mm_s, r.foot_below_mm, r.worst_gap_mm]));
        }
        print_table("landing gear at 2 m/s, the ghost worlds with their own number of steps (the truth keeps 3 a tick)", &["peak g", "least ext", "err mm", "jitter", "foot below", "gap mm"], &rows, &[2, 3, 1, 2, 1, 2]);
    }
    // The door on a spring.
    {
        let scene = door_scene(false, 1000.0, 4.0);
        let mut rows = Vec::new();
        for (name, scheme, calls) in &ghost_variants {
            let t = run_options(&scene, *scheme, Settings::default(), Options { ghost_calls: *calls, ..Options::default() });
            let peak_angle = max_of(t.frames.iter().map(|f| joint_coordinate(&scene.joints[0], f).abs()));
            rows.push((name.clone(), vec![t.last()[2].linvel.z, t.last()[0].linvel.length(), peak_angle]));
        }
        print_table("door on a spring, 1000 kg stranger (converged: exit -1.507, hull 0.403, peak angle 0.288)", &["exit vz", "hull m/s", "peak ang"], &rows, &[3, 3, 3]);
    }
    // A fast stranger onto a stiff chain.
    {
        let mut scene = chain_scene(JointKind::Fixed, Hit::Axial, 1000.0, 4);
        let last = scene.cars.len() - 1;
        scene.cars[last].pose = Pose::from_translation(v(3.0 + 0.5 + 5.0 + 0.5, 0.0, 0.0));
        scene.cars[last].linvel = v(-20.0, 0.0, 0.0);
        scene.seconds = 1.25;
        let mut rows = Vec::new();
        for (name, scheme, calls) in &ghost_variants {
            let t = run_options(&scene, *scheme, Settings::default(), Options { ghost_calls: *calls, ..Options::default() });
            let overlap = max_of(t.frames.iter().map(|f| (1.0 - (f[3].pose.translation - f[last].pose.translation).length()) * 1e3));
            rows.push((name.clone(), vec![t.last()[last].linvel.x, t.last()[3].linvel.x, overlap]));
        }
        print_table("a 1000 kg stranger at 20 m/s onto a stiff chain (converged: both -4.000)", &["stranger vx", "chain vx", "overlap mm"], &rows, &[3, 3, 1]);
    }
    // The rounds of the per-car scheme, on the landing gear.
    {
        let (scene, ids) = gear_scene(2.0, 8.0);
        let reference = run(&scene, Scheme::Hi, Settings::default());
        let mut rows = Vec::new();
        for rounds in [0usize, 1, 2, 4, 8, 16] {
            let scheme = Scheme::Proxy { partition: Partition::PerCar, handback: Handback::Impulse, rounds, feedback: Feedback::None };
            let t = run(&scene, scheme, Settings::default());
            let r = gear_report(&scene, &ids, &t, &reference);
            rows.push((format!("per car, {rounds} extra rounds"), vec![r.peak_g, r.least_extension, r.final_height_error_mm, r.jitter_mm_s, r.foot_below_mm, t.micros_per_tick]));
        }
        print_table("landing gear at 2 m/s, per-car proxies with extra rounds of block Gauss-Seidel (converged: 2.49 g, 0.362, 0 mm)", &["peak g", "least ext", "err mm", "jitter", "foot below", "us/tick"], &rows, &[2, 3, 1, 2, 1, 0]);
    }
    // The coupling interval: one step of 1/20 s a tick (12 substeps) instead of three of 1/60 s.
    {
        let coarse = Settings { steps_per_tick: 1, substeps: 12, ..Settings::default() };
        let (scene, ids) = gear_scene(2.0, 8.0);
        let reference = run(&scene, Scheme::Hi, Settings::default());
        let mut rows = Vec::new();
        for scheme in [Scheme::Game, Scheme::Ghost, Scheme::GhostDifference, WHOLE_RIGID, WHOLE_IMPULSE, PER_CAR] {
            let t = run(&scene, scheme, coarse);
            let r = gear_report(&scene, &ids, &t, &reference);
            rows.push((scheme.name(), vec![r.peak_g, r.least_extension, r.final_height_error_mm, r.jitter_mm_s, r.foot_below_mm, r.worst_gap_mm]));
        }
        print_table("landing gear at 2 m/s with one coupling step of 1/20 s a tick (12 substeps) instead of three of 1/60 s", &["peak g", "least ext", "err mm", "jitter", "foot below", "gap mm"], &rows, &[2, 3, 1, 2, 1, 2]);
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------------------
// The scoreboard: one row of the key numbers of ten scenes for a scheme, to compare fixes and variants at a glance.

/// How a column is printed: as it is, or as the difference from the converged row in millimetres.
#[derive(Clone, Copy)]
enum Show {
    Raw,
    DifferenceMillimetres,
}

const SCORE_COLUMNS: [(&str, Show, usize); 15] = [
    ("tower off", Show::Raw, 0),
    ("gear g", Show::Raw, 2),
    ("gear mm", Show::DifferenceMillimetres, 1),
    ("door rad", Show::Raw, 3),
    ("door exit", Show::Raw, 3),
    ("drive v", Show::Raw, 2),
    ("coast v", Show::Raw, 2),
    ("carrier mm", Show::DifferenceMillimetres, 0),
    ("carried mm", Show::DifferenceMillimetres, 0),
    ("land g", Show::Raw, 2),
    ("across 16", Show::Raw, 3),
    ("far mm", Show::DifferenceMillimetres, 0),
    ("along 8", Show::Raw, 3),
    ("stiff vx", Show::Raw, 3),
    ("stiff lap", Show::Raw, 0),
];

fn score(scheme: Scheme, options: Options) -> Vec<f64> {
    let settings = Settings::default();
    let mut row = Vec::new();
    // 0: a tower of cubes at rest, 8 s: how far it walks off its axis (mm).
    {
        let scene = tower_scene(4, 8.0);
        let t = run_options(&scene, scheme, settings, options);
        row.push(max_of(t.frames.iter().map(|f| f.iter().map(|s| (s.pose.translation.x.powi(2) + s.pose.translation.z.powi(2)).sqrt()).fold(0.0, f64::max))) * 1e3);
    }
    // 1, 2: the landing gear at 2 m/s: peak g, settled hull height.
    {
        let (scene, ids) = gear_scene(2.0, 6.0);
        let t = run_options(&scene, scheme, settings, options);
        let dt = t.dt;
        let mut peak = 0.0f64;
        for pair in t.frames.windows(2) {
            peak = peak.max(((pair[1][ids.hull].linvel.y - pair[0][ids.hull].linvel.y) / dt + 9.81) / 9.81);
        }
        row.push(peak);
        row.push(mean_of(t.since(scene.seconds - 2.0).iter().map(|f| f[ids.hull].pose.translation.y)));
    }
    // 3, 4: the spring door hit by 1000 kg: peak angle, the stranger's exit speed.
    {
        let scene = door_scene(false, 1000.0, 4.0);
        let t = run_options(&scene, scheme, settings, options);
        row.push(max_of(t.frames.iter().map(|f| joint_coordinate(&scene.joints[0], f).abs())));
        row.push(t.last()[2].linvel.z);
    }
    // 5, 6: the rover driving, and coasting: speed at 8 s.
    for (drive, coasting) in [(true, false), (false, true)] {
        let (scene, ids) = rover_scene(drive, coasting);
        let t = run_options(&scene, scheme, settings, options);
        row.push(t.last()[ids.chassis].linvel.x);
    }
    // 7, 8, 9: the carrier at rest (heights of the two hulls), and the ship dropped on it (peak g of the carried hull).
    {
        let (scene, carrier, carried) = carrier_scene(CarrierRun::Rest);
        let t = run_options(&scene, scheme, settings, options);
        row.push(t.last()[carrier.hull].pose.translation.y);
        row.push(t.last()[carried.hull].pose.translation.y);
        let (scene, _carrier, carried) = carrier_scene(CarrierRun::Land);
        let t = run_options(&scene, scheme, settings, options);
        let mut peak = 0.0f64;
        for pair in t.frames.windows(2) {
            peak = peak.max(((pair[1][carried.hull].linvel.y - pair[0][carried.hull].linvel.y) / t.dt + 9.81) / 9.81);
        }
        row.push(peak);
    }
    // 10, 11: a hinged chain of 16 hit across: the stranger's exit speed, the far car's height.
    {
        let scene = chain_scene(JointKind::Hinge, Hit::Transverse, 1000.0, 16);
        let t = run_options(&scene, scheme, settings, options);
        row.push(t.last()[16].linvel.y);
        row.push(t.last()[15].pose.translation.y);
    }
    // 12: a hinged chain of 8 hit along: the stranger's exit speed.
    {
        let scene = chain_scene(JointKind::Hinge, Hit::Axial, 1000.0, 8);
        let t = run_options(&scene, scheme, settings, options);
        row.push(t.last()[8].linvel.x);
    }
    // 13, 14: a stranger at 20 m/s onto a stiff chain of 4: its exit speed, the deepest overlap (mm).
    {
        let mut scene = chain_scene(JointKind::Fixed, Hit::Axial, 1000.0, 4);
        let last = scene.cars.len() - 1;
        scene.cars[last].pose = Pose::from_translation(v(9.0, 0.0, 0.0));
        scene.cars[last].linvel = v(-20.0, 0.0, 0.0);
        scene.seconds = 1.25;
        let t = run_options(&scene, scheme, settings, options);
        row.push(t.last()[last].linvel.x);
        row.push(max_of(t.frames.iter().map(|f| (1.0 - (f[3].pose.translation - f[last].pose.translation).length()) * 1e3)));
    }
    row
}

fn print_scoreboard(title: &str, rows: &[(String, Scheme, Options)]) {
    println!("\n{title}");
    print!("{:<34}", "scheme");
    for (name, _, _) in SCORE_COLUMNS {
        print!(" {name:>10}");
    }
    println!();
    let truth = score(Scheme::Hi, Options::default());
    print!("{:<34}", "converged (truth; hull heights in m)");
    for (k, (_, show, precision)) in SCORE_COLUMNS.iter().enumerate() {
        let places = if matches!(show, Show::DifferenceMillimetres) { 3 } else { *precision };
        print!(" {:>10.places$}", truth[k]);
    }
    println!();
    for (name, scheme, options) in rows {
        let values = score(*scheme, *options);
        print!("{name:<34}");
        for (k, (_, show, precision)) in SCORE_COLUMNS.iter().enumerate() {
            let value = match show {
                Show::Raw => values[k],
                Show::DifferenceMillimetres => (values[k] - truth[k]) * 1e3,
            };
            print!(" {value:>10.precision$}", precision = *precision);
        }
        println!();
    }
}

fn fixes() {
    let plain = Options::default();
    let with = |partition: Partition, rounds: usize, feedback: Feedback| Scheme::Proxy { partition, handback: Handback::Impulse, rounds, feedback };
    let rows: Vec<(String, Scheme, Options)> = vec![
        ("one world, game (today)".to_string(), Scheme::Game, plain),
        ("ghost, impulses".to_string(), Scheme::Ghost, plain),
        ("ghost, difference".to_string(), Scheme::GhostDifference, plain),
        ("welded ghost, impulses".to_string(), Scheme::GhostWelded, plain),
        ("welded ghosts, difference".to_string(), Scheme::GhostWeldedDifference, plain),
        ("whole body, impulse (owner)".to_string(), WHOLE_IMPULSE, plain),
        ("per car".to_string(), PER_CAR, plain),
        ("per car + load".to_string(), with(Partition::PerCar, 0, Feedback::Last), plain),
        ("per car + load (persistent)".to_string(), with(Partition::PerCar, 0, Feedback::Persistent), plain),
        ("cluster + load (persistent)".to_string(), with(Partition::Clusters, 0, Feedback::Persistent), plain),
        ("cluster + load + 2 rounds".to_string(), with(Partition::Clusters, 2, Feedback::Last), plain),
        ("cluster + load (persistent) + 2 rounds".to_string(), with(Partition::Clusters, 2, Feedback::Persistent), plain),
    ];
    print_scoreboard(
        "scoreboard (columns: tower off axis mm; gear peak g, hull height error mm; door peak angle, stranger exit; rover speed driving, coasting; carrier and carried hull height error mm, landing g; hinged chain of 16 across: stranger exit vy, far car error mm; chain of 8 along: exit vx; stiff chain at 20 m/s: exit vx, overlap mm)",
        &rows,
    );
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------------------
// Part priorities, in three forms: solver work (per island), welding the rigid parts into one body, and who receives the impulse of a contact.

fn stiff_approach_scene(speed: f64) -> Scene {
    let mut scene = chain_scene(JointKind::Fixed, Hit::Axial, 1000.0, 4);
    let last = scene.cars.len() - 1;
    scene.cars[last].pose = Pose::from_translation(v(3.0 + 0.5 + 5.0 + 0.5, 0.0, 0.0));
    scene.cars[last].linvel = v(-speed, 0.0, 0.0);
    scene.seconds = (5.0 / speed + 1.0).max(1.0);
    scene
}

fn effort() {
    println!("\nsolver work as a priority: the ghost worlds given extra substeps and passes (Rapier applies them to a whole island, the largest request of its bodies), always or only after a step in which something was touched");
    println!("{:<44} {:>10} {:>10} {:>10} {:>10} {:>10}", "ghost, difference form; extra (substeps, passes)", "stiff 5", "stiff 20", "along 8", "gear g", "us/tick");
    let stiff_5 = stiff_approach_scene(5.0);
    let stiff_20 = stiff_approach_scene(20.0);
    let along_8 = chain_scene(JointKind::Hinge, Hit::Axial, 1000.0, 8);
    let (gear, ids) = gear_scene(2.0, 6.0);
    println!("{:<44} {:>10.3} {:>10.3} {:>10.3} {:>10.2}   (converged: stranger exit -1.000, -4.000, -0.553; gear 2.49 g)", "converged", -1.0, -4.0, -0.553, 2.49);
    for (label, effort, touched) in [
        ("none (today)", (0usize, 0usize), false),
        ("(0, 3)", (0, 3), false),
        ("(0, 7)", (0, 7), false),
        ("(0, 15)", (0, 15), false),
        ("(4, 0)", (4, 0), false),
        ("(4, 7)", (4, 7), false),
        ("(12, 31) = the converged settings' work", (12, 31), false),
        ("(4, 7), only after a touch", (4, 7), true),
        ("(12, 31), only after a touch", (12, 31), true),
    ] {
        let options = Options { ghost_effort: effort, effort_when_touched: touched, ..Options::default() };
        let exit_of = |scene: &Scene, index: usize| run_options(scene, Scheme::GhostDifference, Settings::default(), options).last()[index].linvel.x;
        let a = exit_of(&stiff_5, 4);
        let b = exit_of(&stiff_20, 4);
        let c = exit_of(&along_8, 8);
        let t = run_options(&gear, Scheme::GhostDifference, Settings::default(), options);
        let mut peak = 0.0f64;
        for pair in t.frames.windows(2) {
            peak = peak.max(((pair[1][ids.hull].linvel.y - pair[0][ids.hull].linvel.y) / t.dt + 9.81) / 9.81);
        }
        println!("{label:<44} {a:>10.3} {b:>10.3} {c:>10.3} {peak:>10.2} {:>10.0}", t.micros_per_tick);
    }
}

fn welds() {
    // The locked door: the hinge is held by a very stiff motor; the scene says it is to be taken as rigid where a scheme welds.
    let mut locked = door_scene(true, 1000.0, 4.0);
    locked.welds.push(0);
    let mut spring = door_scene(false, 1000.0, 4.0);
    spring.welds.push(0);
    let stiff_5 = stiff_approach_scene(5.0);
    let stiff_20 = stiff_approach_scene(20.0);
    let stiff_60 = stiff_approach_scene(60.0);
    let tower = tower_scene(4, 8.0);
    let (gear, gear_ids) = gear_scene(2.0, 6.0);
    println!("\nwelding the rigid parts of a mechanism into one body in the ghost (fixed joints, and a joint the scene marks as immobilized): the stranger's exit speed after a hit on");
    println!("{:<34} {:>10} {:>10} {:>10} {:>10} {:>12} {:>10} {:>10} {:>9}", "scheme", "stiff 5", "stiff 20", "stiff 60", "door lock", "door spring*", "tower off", "gear g", "us/tick 4");
    println!("{:<34} {:>10.3} {:>10.3} {:>10.3} {:>10.3} {:>12.3} {:>10.1} {:>10.2}   (converged; * welded although it is a spring: wrong on purpose)", "converged", -1.0, -4.0, -11.983, -1.985, -1.507, 0.0, 2.49);
    for scheme in [Scheme::Game, Scheme::Ghost, Scheme::GhostWelded, Scheme::GhostDifference, Scheme::GhostWeldedDifference, WHOLE_IMPULSE] {
        let stranger_exit = |scene: &Scene, index: usize| run(scene, scheme, Settings::default()).last()[index].linvel;
        let a = stranger_exit(&stiff_5, 4).x;
        let b = stranger_exit(&stiff_20, 4).x;
        let c = stranger_exit(&stiff_60, 4).x;
        let d = stranger_exit(&locked, 2).z;
        let e = stranger_exit(&spring, 2).z;
        let tw = run(&tower, scheme, Settings::default());
        let off = max_of(tw.frames.iter().map(|f| f.iter().map(|s| (s.pose.translation.x.powi(2) + s.pose.translation.z.powi(2)).sqrt()).fold(0.0, f64::max))) * 1e3;
        let t = run(&gear, scheme, Settings::default());
        let mut peak = 0.0f64;
        for pair in t.frames.windows(2) {
            peak = peak.max(((pair[1][gear_ids.hull].linvel.y - pair[0][gear_ids.hull].linvel.y) / t.dt + 9.81) / 9.81);
        }
        let cost = run(&stiff_20, scheme, Settings::default()).micros_per_tick;
        println!("{:<34} {a:>10.3} {b:>10.3} {c:>10.3} {d:>10.3} {e:>12.3} {off:>10.1} {peak:>10.2} {cost:>9.0}", scheme.name());
    }
}

fn priority() {
    // A 100 kg crate thrown at the wall of a 8 t ship (a different group), as a thing inside a ship would be: the ship must not be moved by it.
    let mut scene = Scene::new("crate against a hull", 1.0);
    let hull = scene.add_car(CarSpec::new("hull", 0, Shape::Cuboid(v(1.5, 1.0, 1.5)), 8000.0, v(0.0, 0.0, 0.0)).priority(1));
    let crate_car = scene.add_car(CarSpec::new("crate", 1, Shape::Cuboid(Vector::splat(0.25)), 100.0, v(1.5 + 0.25 + 0.5, 0.0, 0.0)).moving(v(-5.0, 0.0, 0.0)));
    println!("\npriority as who is given the impulse: a 100 kg crate at 5 m/s into the wall of an 8 t ship (the ship has the higher priority); speeds after 1 s (m/s)");
    println!("{:<44} {:>12} {:>12} {:>12}", "scheme", "ship vx", "crate vx", "momentum");
    for (name, scheme, rule) in [
        ("one world, game", Scheme::Game, false),
        ("ghost, impulses", Scheme::Ghost, false),
        ("ghost, impulses, priority rule", Scheme::Ghost, true),
        ("welded ghost, impulses, priority rule", Scheme::GhostWelded, true),
        ("per car, priority rule", PER_CAR, true),
        ("whole body, impulse, priority rule", WHOLE_IMPULSE, true),
    ] {
        let t = run_options(&scene, scheme, Settings::default(), Options { priority_rule: rule, ..Options::default() });
        let end = t.last();
        let momentum = total_momentum(&scene, end).x - total_momentum(&scene, &t.frames[0]).x;
        println!("{name:<44} {:>12.5} {:>12.4} {:>12.2}", end[hull].linvel.x, end[crate_car].linvel.x, momentum);
    }
}

fn lod() {
    // Far parts as one body (a level of detail by distance from the hit): a hinged chain of 16 cars hit at its last car, the first 8 or 12 welded in the ghost.
    println!("\nlevel of detail as a priority: a hinged chain of 16 cars hit at the last car by 1000 kg at 5 m/s; the ghost (difference form) with the first cars welded into one body, 1 s");
    println!("{:<44} {:>10} {:>10} {:>10} {:>10} {:>10}", "welded in the ghost", "across vy", "far off mm", "along vx", "far off mm", "us/tick");
    for welded_cars in [0usize, 4, 8, 12] {
        let mut row = Vec::new();
        let mut micros = 0.0;
        for hit in [Hit::Transverse, Hit::Axial] {
            let mut scene = chain_scene(JointKind::Hinge, hit, 1000.0, 16);
            scene.welds = (0..welded_cars.saturating_sub(1)).collect();
            let reference = run(&chain_scene(JointKind::Hinge, hit, 1000.0, 16), Scheme::Hi, Settings::default());
            let t = run(&scene, Scheme::GhostWeldedDifference, Settings::default());
            let end = t.last();
            row.push(if hit == Hit::Transverse { end[16].linvel.y } else { end[16].linvel.x });
            row.push((end[15].pose.translation - reference.last()[15].pose.translation).length() * 1e3);
            micros = t.micros_per_tick;
        }
        let label = if welded_cars == 0 { "none".to_string() } else { format!("the first {welded_cars} cars") };
        println!("{label:<44} {:>10.3} {:>10.1} {:>10.3} {:>10.1} {micros:>10.0}", row[0], row[1], row[2], row[3]);
    }
    println!("{:<44} {:>10.3} {:>10} {:>10.3}   (converged)", "", -2.174, "", -0.232);
}

fn main() {
    let which = std::env::args().nth(1).unwrap_or_else(|| "all".to_string());
    let all = which == "all";
    if which == "regress" || all {
        regress();
    }
    if which == "rest" || all {
        tower();
    }
    if which == "gear" || all {
        gear();
    }
    if which == "door" || all {
        door();
    }
    if which == "nearness" || all {
        nearness();
    }
    if which == "variants" || all {
        variants();
    }
    if which == "rounds" {
        // Do extra rounds converge on the hinged chain hit across (the old probe's rounds got worse, believed to be repeated steps of one world)?
        for hit in [Hit::Transverse, Hit::Axial] {
            let scene = chain_scene(JointKind::Hinge, hit, 1000.0, 4);
            let reference = run(&scene, Scheme::Hi, Settings::default());
            let mut rows = Vec::new();
            for rounds in [0usize, 1, 2, 4, 8, 32] {
                let scheme = Scheme::Proxy { partition: Partition::PerCar, handback: Handback::Impulse, rounds, feedback: Feedback::None };
                let t = run(&scene, scheme, Settings::default());
                let end = t.last();
                let off = (end[3].pose.translation - reference.last()[3].pose.translation).length() * 1e3;
                let overlap = max_of(t.frames.iter().map(|f| (1.0 - (f[3].pose.translation - f[4].pose.translation).length()) * 1e3));
                rows.push((format!("per car, {rounds} extra rounds"), vec![end[4].linvel.x, end[4].linvel.y, off, overlap]));
            }
            print_table(&format!("hinged chain of 4 hit {} (converged stranger exit: {:+.3}, {:+.3})", if hit == Hit::Axial { "along" } else { "across" }, reference.last()[4].linvel.x, reference.last()[4].linvel.y), &["exit vx", "exit vy", "end off mm", "overlap"], &rows, &[3, 3, 1, 1]);
        }
    }
    if which == "fixes" {
        fixes();
    }
    for (name, function) in [("effort", effort as fn()), ("welds", welds), ("priority", priority), ("lod", lod)] {
        if which == name || which == "priorities" {
            function();
        }
    }
    if which == "rovertrace" {
        // The coasting rover under a feedback scheme, second by second: where does its extra speed come from?
        let name = std::env::args().nth(2).unwrap_or_else(|| "load".to_string());
        let scheme = match name.as_str() {
            "game" => Scheme::Game,
            "normal" => Scheme::Proxy { partition: Partition::PerCar, handback: Handback::Impulse, rounds: 0, feedback: Feedback::LastNormal },
            "none" => PER_CAR,
            _ => PER_CAR_LOAD,
        };
        let (scene, ids) = rover_scene(false, true);
        let t = run(&scene, scheme, Settings::default());
        for k in (0..t.frames.len()).step_by(12) {
            let f = &t.frames[k];
            let rims: Vec<String> = ids.wheels.iter().map(|&w| format!("{:+.2}", -f[w].angvel.z * 0.4)).collect();
            let wheel_v: Vec<String> = ids.wheels.iter().map(|&w| format!("{:+.2}", f[w].linvel.x)).collect();
            println!("t {:.2} chassis vx {:+.3} y {:.3}  wheel vx [{}] rim [{}]  chassis tilt z {:+.4}", k as f64 * t.dt, f[ids.chassis].linvel.x, f[ids.chassis].pose.translation.y, wheel_v.join(" "), rims.join(" "), f[ids.chassis].angvel.z);
        }
    }
    if which == "fingerprints" {
        fingerprints();
    }
    for (name, function) in [("rover", rover as fn()), ("carrier", carrier), ("friction", friction), ("long", long), ("squeeze", squeeze), ("approach", approach), ("cost", cost)] {
        if which == name || all {
            function();
        }
    }
    if which == "trace" {
        // Per-leg extension and foot height over time for the gear at rest: `trace whole|ghost|game|car`.
        let name = std::env::args().nth(2).unwrap_or_else(|| "whole".to_string());
        let scheme = match name.as_str() {
            "ghost" => Scheme::Ghost,
            "game" => Scheme::Game,
            "car" => PER_CAR,
            "rigid" => WHOLE_RIGID,
            _ => WHOLE_IMPULSE,
        };
        let (scene, ids) = gear_scene(2.0, 5.0);
        let t = run(&scene, scheme, Settings::default());
        for k in (0..t.frames.len()).step_by(6) {
            let f = &t.frames[k];
            let ext: Vec<String> = ids.struts.iter().map(|&j| format!("{:.3}", joint_coordinate(&scene.joints[j], f))).collect();
            let foot: Vec<String> = ids.feet.iter().map(|&c| format!("{:+.4}", f[c].pose.translation.y - 0.1)).collect();
            let speed: Vec<String> = ids.feet.iter().map(|&c| format!("{:+.2}", f[c].linvel.y)).collect();
            println!("t {:.2}  hull y {:.3} vy {:+.3}  ext [{}]  foot bottom [{}]  foot vy [{}]", k as f64 * t.dt, f[ids.hull].pose.translation.y, f[ids.hull].linvel.y, ext.join(" "), foot.join(" "), speed.join(" "));
        }
    }
    if which == "pushout" {
        // A box that starts 5 cm inside the floor, no gravity: the solver pushes it out. Are the impulses of its contact pairs its change of momentum?
        for depth in [0.0, 0.005, 0.05] {
            let mut scene = Scene::new("pushout", 0.3);
            scene.ground.push(ground_floor(0.0));
            scene.add_car(CarSpec::new("box", 0, Shape::Cuboid(Vector::splat(0.5)), 1000.0, v(0.0, 0.5 - depth, 0.0)).friction(0.0));
            let t = run(&scene, PER_CAR, Settings::default());
            let end = t.last()[0];
            println!("depth {depth} m: after 0.3 s the box is at y {:.4} speed {:.4}; worst balance {:.3e}", end.pose.translation.y, end.linvel.y, t.worst_balance);
        }
    }
    if which == "debug" {
        for friction in [0.8, 0.0] {
            let (mut scene, _) = gear_scene(2.0, 1.2);
            for car in &mut scene.cars {
                car.friction = friction;
            }
            for ground in &mut scene.ground {
                ground.friction = friction;
            }
            let t = run(&scene, WHOLE_IMPULSE, Settings::default());
            println!("gear, whole impulse, friction {friction}: worst balance {:.2e}", t.worst_balance);
        }
    }
}
