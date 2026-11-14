//! The third round of the ghost study (`docs/history/loop-probe.md`, section 18, 2026-11-13): how good is the ghost really, and what makes a change of frame or of method
//! invisible to a player? The kit is `loop_probe::contraption`, the scenes `loop_probe::scenes`, the measures `loop_probe::study`.
//!
//! Run: `cargo run --release --bin ghosts -- hicheck|pop|sweep|fast|birth|adaptive|ownground|conserve|rest|cost|all`.

use loop_probe::contraption::Shape;
use loop_probe::contraption::*;
use loop_probe::scenes::*;
use loop_probe::study::*;
use loop_probe::*;

const TICK: f64 = 1.0 / 20.0;

fn settings() -> Settings {
    Settings::default()
}

fn all_cars(scene: &Scene) -> Vec<usize> {
    (0..scene.cars.len()).collect()
}

fn mm(metres: f64) -> f64 {
    metres * 1e3
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------------------
// How good is the reference? The converged world against one twice as fine in every count.

fn hicheck() {
    println!("\nthe reference against itself: `Hi` (12 steps a tick, 16 substeps, 32 passes) against a world twice as fine in each (24, 32, 64): the farthest any car is, over the whole run");
    println!("{:<44} {:>10} {:>12} {:>12} {:>10}", "scene", "place mm", "speed mm/s", "turn mrad", "Hi ms/tick");
    let mut scenes: Vec<(String, Scene)> = Vec::new();
    let mut chain = chain_scene(JointKind::Hinge, Hit::Transverse, 1000.0, 4);
    chain.seconds = 1.0;
    scenes.push(("hinged chain of 4 hit across at 5 m/s".to_string(), chain));
    scenes.push(("hinged chain of 8 hit along at 5 m/s".to_string(), chain_scene(JointKind::Hinge, Hit::Axial, 1000.0, 8)));
    scenes.push(("stiff chain, stranger at 20 m/s".to_string(), stiff_approach_scene(20.0)));
    let mut door = door_scene(false, 1000.0, 4.0);
    door.seconds = 3.0;
    scenes.push(("door on a spring hit by 1 t at 4 m/s".to_string(), door));
    scenes.push(("landing gear lands at 2 m/s (6 s)".to_string(), gear_scene(2.0, 6.0).0));
    scenes.push(("ship lands on a carrier (7 s)".to_string(), carrier_scene(CarrierRun::Land).0));
    scenes.push(("rover coasting (8 s)".to_string(), rover_scene(false, true).0));
    for (name, scene) in &scenes {
        let hi = run(scene, Scheme::Hi, settings());
        let finer = run(scene, Scheme::HiFiner, settings());
        let d = deviation(scene, &hi, &finer, TICK, &all_cars(scene), 0.0, scene.seconds);
        println!("{name:<44} {:>10.4} {:>12.4} {:>12.4} {:>10.1}", mm(d.position), mm(d.velocity), d.angle * 1e3, hi.micros_per_tick / 1e3);
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------------------
// A change of frame as the game has it: the number of steps a tick is cut into follows the fastest body (3 to 24). What does a mechanism do when it changes?

fn whip_scene() -> Scene {
    // Four hinged balls in no gravity, set going so that the chain bends and whips: pure joint work, no contact.
    let mut scene = Scene::new("whip", 4.0);
    let inertia = Vector::splat(1000.0 / 6.0);
    for k in 0..4 {
        scene.add_car(CarSpec::new(&format!("car{k}"), 0, Shape::Ball(0.5), 1000.0, v(k as f64, 0.0, 0.0)).moving(v(0.0, 0.8 * k as f64, 0.0)).friction(0.0).with_inertia(inertia));
    }
    for k in 0..3 {
        scene.add_joint(JointSpec::new(JointKind::Hinge, k, v(0.5, 0.0, 0.0), k + 1, v(-0.5, 0.0, 0.0)).axis(Vector::Z));
    }
    scene
}

fn pop() {
    println!("\nwhat a change of the number of steps a tick is cut into does to a mechanism (today's combined world: a frame cuts its tick by its fastest body, 3 to 24). The same scene always cut in 3, and cut in n for 40 ticks (2 s) and back to 3.");
    println!("pop = how far the first tick in the new cut ends from the always-3 run; inside = the most during the 40 ticks; back = the first tick after returning to 3; wake = the most after that. mm and mm/s.");
    println!("{:<34} {:>3} | {:>8} {:>8} | {:>8} {:>8} | {:>8} {:>8} | {:>8} {:>8}", "scene (the window starts at)", "n", "pop mm", "mm/s", "inside", "mm/s", "back mm", "mm/s", "wake mm", "mm/s");
    let (gear, _) = gear_scene(2.0, 8.0);
    let (landing, _) = gear_scene(2.0, 3.0);
    let mut door = door_scene(false, 1000.0, 4.0);
    door.seconds = 5.0;
    let cases: Vec<(String, Scene, usize)> = vec![
        ("landing gear at rest (5 s)".to_string(), gear, 100),
        ("tower of 4 at rest (5 s)".to_string(), tower_scene(4, 10.0), 100),
        ("carrier with a ship at rest (5 s)".to_string(), carrier_scene(CarrierRun::Rest).0, 100),
        ("rover coasting (3 s)".to_string(), rover_scene(false, true).0, 60),
        ("chain whipping in no gravity (1 s)".to_string(), whip_scene(), 20),
        ("door struck and swinging (0.5 s)".to_string(), door, 10),
        ("landing gear landing (0.05 s)".to_string(), landing, 1),
    ];
    for (name, scene, start) in &cases {
        let ticks = (scene.seconds / TICK).round() as usize;
        let baseline = run_scheduled(scene, Scheme::Game, settings(), Options::default(), &|_| 3);
        for n in [6usize, 12, 24] {
            let (first, last) = (*start, start + 40);
            let steps = move |k: usize| if k >= first && k < last { n } else { 3 };
            let changed = run_scheduled(scene, Scheme::Game, settings(), Options::default(), &steps);
            let off = |j: usize| -> (f64, f64) {
                let mut place = 0.0f64;
                let mut speed = 0.0f64;
                for i in 0..scene.cars.len() {
                    place = place.max((changed[j][i].pose.translation - baseline[j][i].pose.translation).length());
                    speed = speed.max((changed[j][i].linvel - baseline[j][i].linvel).length());
                }
                (mm(place), mm(speed))
            };
            let worst = |from: usize, to: usize| -> (f64, f64) { (from..=to.min(ticks)).map(&off).fold((0.0, 0.0), |a, b| (a.0.max(b.0), a.1.max(b.1))) };
            let pop = off(first + 1);
            let inside = worst(first + 1, last);
            let back = off((last + 1).min(ticks));
            let wake = worst(last + 1, ticks);
            println!("{name:<34} {n:>3} | {:>8.3} {:>8.2} | {:>8.3} {:>8.2} | {:>8.3} {:>8.2} | {:>8.3} {:>8.2}", pop.0, pop.1, inside.0, inside.1, back.0, back.1, wake.0, wake.1);
        }
    }
}

fn doortrace() {
    let mut door = door_scene(false, 1000.0, 4.0);
    door.seconds = 3.0;
    let hi = run(&door, Scheme::Hi, settings());
    let finer = run(&door, Scheme::HiFiner, settings());
    let game = run(&door, Scheme::Game, settings());
    for car in 0..door.cars.len() {
        let d = deviation(&door, &hi, &finer, TICK, &[car], 0.0, door.seconds);
        println!("car {car}: place {:.3} mm (at {:.2} s), speed {:.3} mm/s, turn {:.3} mrad", mm(d.position), d.at, mm(d.velocity), d.angle * 1e3);
    }
    println!("door on a spring hit by 1 t at 4 m/s: angle (rad), stranger vz (m/s) and z (m) at times, converged / finer / game");
    for k in [2usize, 4, 5, 6, 7, 8, 10, 14, 20, 30, 40, 60] {
        let t = k as f64 * TICK;
        let row = |trace: &Trace| {
            let f = trace.at(t);
            format!("{:+.4} {:+.4} {:+.4}", joint_coordinate(&door.joints[0], f), f[2].linvel.z, f[2].pose.translation.z)
        };
        println!("t {t:.2}  {} | {} | {}", row(&hi), row(&finer), row(&game));
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------------------
// Does the outcome of a hit depend on how the world was stepped? A player sees the same hit in a frame of 3 steps a tick and in one of 24 (a frame follows its fastest body),
// with the ghost at one effort or another. The spread of an outcome across such settings is what could be seen when frames or methods change.

struct Outcome {
    name: &'static str,
    scene: Scene,
    read: Box<dyn Fn(&Scene, &Trace) -> f64>,
    precision: usize,
}

fn peak_g(trace: &Trace, car: usize) -> f64 {
    let mut peak = 0.0f64;
    for pair in trace.frames.windows(2) {
        peak = peak.max(((pair[1][car].linvel.y - pair[0][car].linvel.y) / trace.dt + 9.81) / 9.81);
    }
    peak
}

fn outcomes() -> Vec<Outcome> {
    let (gear, gear_ids) = gear_scene(2.0, 6.0);
    let (carrier_scene_land, _, carried) = carrier_scene(CarrierRun::Land);
    let (rover, rover_ids) = rover_scene(false, true);
    let hull = gear_ids.hull;
    let carried_hull = carried.hull;
    let chassis = rover_ids.chassis;
    vec![
        Outcome { name: "stiff 20 exit", scene: stiff_approach_scene(20.0), read: Box::new(|_, t| t.last()[4].linvel.x), precision: 3 },
        Outcome { name: "along 8 exit", scene: chain_scene(JointKind::Hinge, Hit::Axial, 1000.0, 8), read: Box::new(|_, t| t.last()[8].linvel.x), precision: 3 },
        Outcome { name: "across 16 exit", scene: chain_scene(JointKind::Hinge, Hit::Transverse, 1000.0, 16), read: Box::new(|_, t| t.last()[16].linvel.y), precision: 3 },
        Outcome { name: "door exit", scene: door_scene(false, 1000.0, 4.0), read: Box::new(|_, t| t.last()[2].linvel.z), precision: 3 },
        Outcome { name: "door angle", scene: door_scene(false, 1000.0, 4.0), read: Box::new(|scene, t| t.frames.iter().map(|f| joint_coordinate(&scene.joints[0], f).abs()).fold(0.0, f64::max)), precision: 3 },
        Outcome { name: "gear g", scene: gear.clone(), read: Box::new(move |_, t| peak_g(t, hull)), precision: 2 },
        Outcome { name: "gear height", scene: gear, read: Box::new(move |scene, t| t.since(scene.seconds - 2.0).iter().map(|f| f[hull].pose.translation.y).sum::<f64>() / t.since(scene.seconds - 2.0).len() as f64), precision: 4 },
        Outcome { name: "landing g", scene: carrier_scene_land, read: Box::new(move |_, t| peak_g(t, carried_hull)), precision: 2 },
        Outcome { name: "rover coast", scene: rover, read: Box::new(move |_, t| t.last()[chassis].linvel.x), precision: 3 },
    ]
}

fn measure(outcomes: &[Outcome], scheme: Scheme, settings: Settings, options: Options) -> Vec<f64> {
    outcomes.iter().map(|o| (o.read)(&o.scene, &run_options(&o.scene, scheme, settings, options))).collect()
}

fn print_outcome_header(outcomes: &[Outcome], label: &str) {
    print!("{label:<46}");
    for o in outcomes {
        print!(" {:>13}", o.name);
    }
    println!();
}

fn print_outcome_row(outcomes: &[Outcome], label: &str, values: &[f64]) {
    print!("{label:<46}");
    for (o, value) in outcomes.iter().zip(values) {
        print!(" {value:>13.p$}", p = o.precision);
    }
    println!();
}

/// A group of configurations of one scheme: the rows, and below them the spread of each outcome (the greatest minus the least) and the worst distance from the converged value.
fn invariance_group(outcomes: &[Outcome], truth: &[f64], title: &str, rows: &[(String, Scheme, Settings, Options)]) {
    println!("
{title}");
    print_outcome_header(outcomes, "configuration");
    let mut table: Vec<Vec<f64>> = Vec::new();
    for (label, scheme, settings, options) in rows {
        let values = measure(outcomes, *scheme, *settings, *options);
        print_outcome_row(outcomes, label, &values);
        table.push(values);
    }
    let spread: Vec<f64> = (0..outcomes.len())
        .map(|k| table.iter().map(|r| r[k]).fold(f64::NEG_INFINITY, f64::max) - table.iter().map(|r| r[k]).fold(f64::INFINITY, f64::min))
        .collect();
    let worst: Vec<f64> = (0..outcomes.len()).map(|k| table.iter().map(|r| (r[k] - truth[k]).abs()).fold(0.0, f64::max)).collect();
    print_outcome_row(outcomes, "  spread over the configurations", &spread);
    print_outcome_row(outcomes, "  worst distance from converged", &worst);
}

fn invariance() {
    let outcomes = outcomes();
    println!("
how much of what a player sees depends on how the world was stepped. Outcomes of nine hits and landings (stranger's exit speed after a stiff chain at 20 m/s (true -4), a hinged chain of 8 hit along and of 16 across, a spring door: exit and peak angle; landing gear: peak g and settled height; a ship dropped on a carrier: peak g; a coasting rover's speed).");
    let truth = measure(&outcomes, Scheme::Hi, settings(), Options::default());
    print_outcome_header(&outcomes, "reference");
    print_outcome_row(&outcomes, "one world, converged", &truth);
    let finer = measure(&outcomes, Scheme::HiFiner, settings(), Options::default());
    print_outcome_row(&outcomes, "one world, finer than converged", &finer);
    let steps = |n: usize| Settings { steps_per_tick: n, ..Settings::default() };
    let plain = Options::default();
    let rows: Vec<(String, Scheme, Settings, Options)> = [3usize, 4, 6, 8, 12, 24].iter().map(|&n| (format!("one world, {n} steps a tick"), Scheme::Game, steps(n), plain)).collect();
    invariance_group(&outcomes, &truth, "today: one combined world in a frame that cuts its tick into n steps (3 is a group alone; more when a body is fast)", &rows);
    let rows: Vec<(String, Scheme, Settings, Options)> = [1usize, 2, 4, 8]
        .iter()
        .map(|&calls| (format!("ghost difference, truth 3 steps, ghost x{calls}"), Scheme::GhostDifference, settings(), Options { ghost_calls: calls, ..plain }))
        .collect();
    invariance_group(&outcomes, &truth, "the ghost (difference form): the groups' own worlds always 3 steps; the ghost's steps follow the need", &rows);
    let rows: Vec<(String, Scheme, Settings, Options)> = [(1usize, (0usize, 0usize)), (1, (0, 7)), (1, (0, 15)), (4, (0, 15)), (4, (4, 15)), (4, (12, 31))]
        .iter()
        .map(|&(calls, effort)| (format!("ghost difference, x{calls}, extra {effort:?}"), Scheme::GhostDifference, settings(), Options { ghost_calls: calls, ghost_effort: effort, ..plain }))
        .collect();
    invariance_group(&outcomes, &truth, "the ghost (difference form) with more solver work in the ghost", &rows);
    let rows: Vec<(String, Scheme, Settings, Options)> = [(1usize, (0usize, 0usize)), (1, (0, 15)), (4, (0, 15)), (4, (4, 15))]
        .iter()
        .map(|&(calls, effort)| (format!("welded ghosts, difference, x{calls}, extra {effort:?}"), Scheme::GhostWeldedDifference, settings(), Options { ghost_calls: calls, ghost_effort: effort, ..plain }))
        .collect();
    invariance_group(&outcomes, &truth, "the welded ghosts (rigid parts one body), difference form", &rows);
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------------------
// The sweep: random hits on what players build, at random speeds, masses and moments of the tick, every design against the converged world.

struct Case {
    label: String,
    scene: Scene,
    /// When the first hit happens (seconds).
    hit_at: f64,
}

/// One random case. `family` 0 chain, 1 door, 2 landing gear hit from the side, 3 a ship dropped on a carrier. The moment of the tick the hit falls at is random.
fn random_case(rng: &mut Rng, family: usize) -> Case {
    let step = TICK / 3.0;
    match family {
        0 => {
            let kind = rng.pick(&[JointKind::Fixed, JointKind::Hinge]);
            let hit = rng.pick(&[Hit::Axial, Hit::Transverse]);
            let cars = rng.pick(&[2usize, 3, 4, 6, 8]);
            let mass = rng.log_range(200.0, 8000.0);
            let speed = rng.log_range(1.0, 40.0);
            let mut scene = chain_scene(kind, hit, mass, cars);
            let direction = scene.cars[cars].linvel.normalize();
            scene.cars[cars].linvel = direction * speed;
            let extra = rng.range(0.0, speed * step);
            scene.cars[cars].pose.translation -= direction * extra;
            let hit_at = (0.5 + extra) / speed;
            scene.seconds = ((hit_at + 0.8) / TICK).ceil() * TICK;
            Case { label: format!("{:?} chain of {cars}, {} by {mass:.0} kg at {speed:.1} m/s", kind, if hit == Hit::Axial { "along" } else { "across" }), scene, hit_at }
        }
        1 => {
            let stiff = rng.unit() < 0.3;
            let mass = rng.log_range(100.0, 3000.0);
            let speed = rng.log_range(1.0, 12.0);
            let mut scene = door_scene(stiff, mass, speed);
            let extra = rng.range(0.0, speed * step);
            scene.cars[2].pose.translation.z += extra;
            if stiff {
                scene.welds.push(0);
            }
            let hit_at = (0.5 + extra) / speed;
            scene.seconds = ((hit_at + 1.5) / TICK).ceil() * TICK;
            Case { label: format!("{} door, hit by {mass:.0} kg at {speed:.1} m/s", if stiff { "locked" } else { "spring" }), scene, hit_at }
        }
        2 => {
            let mass = rng.log_range(200.0, 6000.0);
            let speed = rng.log_range(1.0, 12.0);
            let arrival = 1.0 + rng.range(0.0, step);
            let (scene, _, _) = gear_hit_scene(mass, speed, arrival, arrival + 1.5);
            let scene = Scene { seconds: ((arrival + 1.5) / TICK).ceil() * TICK, ..scene };
            Case { label: format!("landed ship hit on the hull by {mass:.0} kg at {speed:.1} m/s"), scene, hit_at: arrival }
        }
        _ => {
            let speed = rng.log_range(0.3, 3.0);
            let gap = rng.range(0.1, 0.3);
            let (mut scene, _, _) = carrier_scene_with(CarrierRun::Land, gap, speed);
            scene.seconds = (((gap / speed) + 3.0) / TICK).ceil() * TICK;
            Case { label: format!("ship dropped on a carrier at {speed:.2} m/s from {gap:.2} m"), scene, hit_at: gap / speed }
        }
    }
}

/// A design to be tried: how it is stepped (the settings), what it is, and what extra it is given.
#[derive(Clone)]
struct Design {
    name: String,
    scheme: Scheme,
    settings: Settings,
    options: Options,
}

fn design(name: &str, scheme: Scheme, steps: usize, options: Options) -> Design {
    Design { name: name.to_string(), scheme, settings: Settings { steps_per_tick: steps, ..Settings::default() }, options }
}

const ADAPTIVE_LEVELS: [(usize, usize); 5] = [(0, 0), (0, 3), (0, 7), (0, 15), (4, 15)];

/// What the designs are measured against: the world twice as fine as the converged one (see `reference`).
const REFERENCE: Scheme = Scheme::HiFiner;

fn sweep_designs(set: &str) -> Vec<Design> {
    let plain = Options::default();
    let effort = |e: (usize, usize)| Options { world_effort: e, ..plain };
    let cut = |e: (usize, usize)| Options { world_effort: e, toi: true, ..plain };
    let ghost = |calls: usize, e: (usize, usize), truth: (usize, usize), toi: bool| Options { ghost_calls: calls, ghost_effort: e, world_effort: truth, toi, ..plain };
    if set == "knee" {
        let mut rows = vec![design("converged (12 steps, 16, 32)", Scheme::Hi, 3, plain), design("today: one world, 3 steps", Scheme::Game, 3, plain)];
        for passes in [3usize, 7, 15, 31] {
            rows.push(design(&format!("one world, 3 steps, passes {passes}"), Scheme::Game, 3, effort((0, passes))));
        }
        for passes in [3usize, 7, 15, 31] {
            rows.push(design(&format!("one world, 3 steps, passes {passes}, cut"), Scheme::Game, 3, cut((0, passes))));
        }
        rows.push(design("one world, 3 steps, substeps 4, passes 15, cut", Scheme::Game, 3, cut((4, 15))));
        rows.push(design("one world, 6 steps, passes 15, cut", Scheme::Game, 6, cut((0, 15))));
        return rows;
    }
    vec![
        design("converged (12 steps, 16, 32)", Scheme::Hi, 3, plain),
        design("today: one world, 3 steps", Scheme::Game, 3, plain),
        design("one world, 3 steps, (0, 15)", Scheme::Game, 3, effort((0, 15))),
        design("one world, 3 steps, (0, 15), cut at touch", Scheme::Game, 3, cut((0, 15))),
        design("one world, 6 steps, (0, 15)", Scheme::Game, 6, effort((0, 15))),
        design("one world, 12 steps, (0, 15)", Scheme::Game, 12, effort((0, 15))),
        design("one world, 12 steps, (4, 15)", Scheme::Game, 12, effort((4, 15))),
        design("ghost diff, x4, (0, 15)", Scheme::GhostDifference, 3, ghost(4, (0, 15), (0, 0), false)),
        design("ghost diff, x4, (0, 15), truth (0, 15)", Scheme::GhostDifference, 3, ghost(4, (0, 15), (0, 15), false)),
        design("ghost diff, cut, (0, 15), truth (0, 15)", Scheme::GhostDifference, 3, ghost(1, (0, 15), (0, 15), true)),
        design("welded ghost diff, cut, (0,15), truth (0,15)", Scheme::GhostWeldedDifference, 3, ghost(1, (0, 15), (0, 15), true)),
        design("ghost impulses, cut, (0, 15), truth (0, 15)", Scheme::Ghost, 3, ghost(1, (0, 15), (0, 15), true)),
        design("ghost diff, adaptive 2%, cut, truth (0, 15)", Scheme::GhostDifference, 3, Options { adaptive: Some(Adaptive { levels: &ADAPTIVE_LEVELS, position: 0.0002, velocity: 0.005, relative: 0.02 }), toi: true, world_effort: (0, 15), ..plain }),
    ]
}

#[derive(Clone, Copy, Default)]
struct Result {
    place: f64,
    velocity: f64,
    angle: f64,
    overlap: f64,
    micros: f64,
}

fn evaluate(case: &Case, designs: &[Design], reference: &Trace) -> Vec<Result> {
    let all = all_cars(&case.scene);
    let from = (case.hit_at + 0.25).min(case.scene.seconds - TICK);
    designs
        .iter()
        .map(|d| {
            let trace = run_options(&case.scene, d.scheme, d.settings, d.options);
            let dev = deviation(&case.scene, &trace, reference, TICK, &all, from, case.scene.seconds);
            Result { place: dev.position, velocity: dev.velocity, angle: dev.angle, overlap: deepest_overlap(&case.scene, &trace), micros: trace.micros_per_tick }
        })
        .collect()
}

fn run_cases_in_parallel(cases: &[Case], designs: &[Design]) -> Vec<Vec<Result>> {
    let threads = 10usize;
    let mut results: Vec<Option<Vec<Result>>> = vec![None; cases.len()];
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|t| {
                scope.spawn(move || {
                    let mut mine = Vec::new();
                    for index in (t..cases.len()).step_by(threads) {
                        let reference = run(&cases[index].scene, REFERENCE, settings());
                        mine.push((index, evaluate(&cases[index], designs, &reference)));
                    }
                    mine
                })
            })
            .collect();
        for handle in handles {
            for (index, row) in handle.join().expect("a worker") {
                results[index] = Some(row);
            }
        }
    });
    results.into_iter().map(|r| r.expect("every case ran")).collect()
}

fn sweep() {
    let count: usize = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(60);
    let mut rng = Rng::new(20261113);
    let cases: Vec<Case> = (0..count).map(|k| random_case(&mut rng, k % 4)).collect();
    let set = std::env::args().nth(3).unwrap_or_else(|| "main".to_string());
    let designs = sweep_designs(&set);
    println!("\nthe sweep: {count} random hits (chains, doors, a landed ship hit from the side, a ship dropped on a carrier; random masses, speeds, and moments of the tick), each design against the world twice as fine as the converged one.");
    println!("The distance is the greatest of any car's place, turn and speed from the reference's, at the end of every tick from 0.25 s after the first hit to the end of the run. Visible: place over 10 mm, turn over 5 mrad (a ball's turn is not counted) or speed over 50 mm/s.");
    let results = run_cases_in_parallel(&cases, &designs);
    println!("{:<50} | {:>8} {:>8} {:>8} | {:>8} {:>8} {:>8} | {:>7} {:>7} | {:>8} {:>7}", "design", "place50", "place90", "placeMax", "speed50", "speed90", "speedMax", "visible", "within", "overlap90", "us/tick");
    println!("{:<50} | {:>8} {:>8} {:>8} | {:>8} {:>8} {:>8} | {:>7} {:>7} | {:>8} {:>7}", "", "mm", "mm", "mm", "mm/s", "mm/s", "mm/s", "of all", "of all", "mm", "median");
    for (k, d) in designs.iter().enumerate() {
        let column = |f: &dyn Fn(&Result) -> f64| -> Vec<f64> { results.iter().map(|row| f(&row[k])).collect() };
        let place = column(&|r| mm(r.place));
        let speed = column(&|r| mm(r.velocity));
        let overlap = column(&|r| mm(r.overlap));
        let micros = column(&|r| r.micros);
        let visible = results.iter().filter(|row| row[k].place > 0.010 || row[k].angle > 0.005 || row[k].velocity > 0.050).count();
        // Within the reference's own uncertainty: not farther than twice what the converged world is from the reference (or the visible limits, if that is less).
        let within = results.iter().filter(|row| row[k].place <= 0.010f64.max(2.0 * row[0].place) && row[k].velocity <= 0.050f64.max(2.0 * row[0].velocity) && row[k].angle <= 0.005f64.max(2.0 * row[0].angle)).count();
        println!(
            "{:<50} | {:>8.2} {:>8.2} {:>8.1} | {:>8.1} {:>8.1} {:>8.1} | {:>6.0}% {:>6.0}% | {:>8.1} {:>7.0}",
            d.name,
            percentile(&place, 50.0),
            percentile(&place, 90.0),
            percentile(&place, 100.0),
            percentile(&speed, 50.0),
            percentile(&speed, 90.0),
            percentile(&speed, 100.0),
            100.0 * visible as f64 / count as f64,
            100.0 * within as f64 / count as f64,
            percentile(&overlap, 90.0),
            percentile(&micros, 50.0)
        );
    }
    // The families apart, for the designs worth reading closely.
    println!("\nvisible, by family (chains, doors, a landed ship hit, a ship on a carrier):");
    for (k, d) in designs.iter().enumerate() {
        let mut line = String::new();
        for family in 0..4 {
            let rows: Vec<&Vec<Result>> = results.iter().enumerate().filter(|(i, _)| i % 4 == family).map(|(_, r)| r).collect();
            let visible = rows.iter().filter(|row| row[k].place > 0.010 || row[k].angle > 0.005 || row[k].velocity > 0.050).count();
            line.push_str(&format!(" {:>3}/{:<3}", visible, rows.len()));
        }
        println!("{:<50} {line}", d.name);
    }
    // The cases that today's design gets most wrong, and what each design did there.
    let today = 1;
    let mut order: Vec<usize> = (0..count).collect();
    order.sort_by(|&a, &b| results[b][today].place.partial_cmp(&results[a][today].place).unwrap());
    println!("
the 6 hits where today's design is farthest from the reference (place mm / speed mm/s), the reference's own uncertainty (the converged world's distance), and the best two designs there:");
    for &i in order.iter().take(6) {
        let row = &results[i];
        let mut best: Vec<usize> = (2..designs.len()).collect();
        best.sort_by(|&a, &b| row[a].place.partial_cmp(&row[b].place).unwrap());
        println!("  {:<62} today {:>7.1} / {:>7.0}   floor {:>6.1}   best: {} {:.1}, {} {:.1}", cases[i].label, mm(row[today].place), mm(row[today].velocity), mm(row[0].place), designs[best[0]].name, mm(row[best[0]].place), designs[best[1]].name, mm(row[best[1]].place));
    }
    // What is left: the hits where the chosen design (the last, or the one named by the fourth argument) is farthest beyond twice the reference's uncertainty.
    let chosen: usize = std::env::args().nth(4).and_then(|x| x.parse().ok()).unwrap_or(designs.len() - 1);
    let mut left: Vec<usize> = (0..count).filter(|&i| results[i][chosen].place > 0.010f64.max(2.0 * results[i][0].place)).collect();
    left.sort_by(|&a, &b| (results[b][chosen].place - 2.0 * results[b][0].place).partial_cmp(&(results[a][chosen].place - 2.0 * results[a][0].place)).unwrap());
    println!("
what is left for '{}': {} hits are beyond twice the reference's uncertainty (place mm; the floor; speed mm/s):", designs[chosen].name, left.len());
    for &i in left.iter().take(10) {
        println!("  {:<62} {:>7.1}  floor {:>6.1}  speed {:>6.0}  (today {:>7.1})", cases[i].label, mm(results[i][chosen].place), mm(results[i][0].place), mm(results[i][chosen].velocity), mm(results[i][today].place));
    }
}

fn reference() {
    let count: usize = std::env::args().nth(2).and_then(|x| x.parse().ok()).unwrap_or(24);
    let mut rng = Rng::new(20261113);
    let cases: Vec<Case> = (0..count).map(|k| random_case(&mut rng, k % 4)).collect();
    println!("
how settled is the reference? {count} random hits (the sweep's), the converged world (12 steps, 16 substeps, 32 passes) and one twice as fine (24, 32, 64) each against one twice as fine again (48, 64, 128): greatest distance over the cars, from 0.25 s after the hit");
    let threads = 10usize;
    let mut rows: Vec<Option<(Deviation, Deviation)>> = vec![None; count];
    std::thread::scope(|scope| {
        let cases = &cases;
        let handles: Vec<_> = (0..threads)
            .map(|t| {
                scope.spawn(move || {
                    let mut mine = Vec::new();
                    for index in (t..cases.len()).step_by(threads) {
                        let case = &cases[index];
                        let all = all_cars(&case.scene);
                        let from = (case.hit_at + 0.25).min(case.scene.seconds - TICK);
                        let finest = run(&case.scene, Scheme::HiFinest, settings());
                        let finer = run(&case.scene, Scheme::HiFiner, settings());
                        let hi = run(&case.scene, Scheme::Hi, settings());
                        mine.push((index, (deviation(&case.scene, &hi, &finest, TICK, &all, from, case.scene.seconds), deviation(&case.scene, &finer, &finest, TICK, &all, from, case.scene.seconds))));
                    }
                    mine
                })
            })
            .collect();
        for handle in handles {
            for (index, row) in handle.join().expect("a worker") {
                rows[index] = Some(row);
            }
        }
    });
    let rows: Vec<(Deviation, Deviation)> = rows.into_iter().map(|r| r.expect("ran")).collect();
    println!("{:<34} | {:>8} {:>8} {:>8} | {:>8} {:>8} {:>8} | {:>7}", "against the finest", "place50", "place90", "placeMax", "speed50", "speed90", "speedMax", "visible");
    for (name, pick) in [("converged (12, 16, 32)", 0usize), ("finer (24, 32, 64)", 1)] {
        let get = |f: &dyn Fn(&Deviation) -> f64| -> Vec<f64> { rows.iter().map(|r| f(if pick == 0 { &r.0 } else { &r.1 })).collect() };
        let place = get(&|d| mm(d.position));
        let speed = get(&|d| mm(d.velocity));
        let visible = rows.iter().filter(|r| (if pick == 0 { r.0 } else { r.1 }).visible()).count();
        println!("{name:<34} | {:>8.2} {:>8.2} {:>8.1} | {:>8.1} {:>8.1} {:>8.1} | {:>6.0}%", percentile(&place, 50.0), percentile(&place, 90.0), percentile(&place, 100.0), percentile(&speed, 50.0), percentile(&speed, 90.0), percentile(&speed, 100.0), 100.0 * visible as f64 / count as f64);
    }
}

fn effort_pop() {
    println!("
what a change of the solver's work does to a mechanism: the same scene always at today's work, and with extra (substeps, passes) for 40 ticks (2 s) and back. The same columns as `pop` (mm, mm/s).");
    println!("{:<34} {:>10} | {:>8} {:>8} | {:>8} {:>8} | {:>8} {:>8} | {:>8} {:>8}", "scene (the window starts at)", "extra", "pop mm", "mm/s", "inside", "mm/s", "back mm", "mm/s", "wake mm", "mm/s");
    let (gear, _) = gear_scene(2.0, 8.0);
    let (landing, _) = gear_scene(2.0, 3.0);
    let mut door = door_scene(false, 1000.0, 4.0);
    door.seconds = 5.0;
    let cases: Vec<(String, Scene, usize)> = vec![
        ("landing gear at rest (5 s)".to_string(), gear, 100),
        ("tower of 4 at rest (5 s)".to_string(), tower_scene(4, 10.0), 100),
        ("carrier with a ship at rest (5 s)".to_string(), carrier_scene(CarrierRun::Rest).0, 100),
        ("rover coasting (3 s)".to_string(), rover_scene(false, true).0, 60),
        ("chain whipping in no gravity (1 s)".to_string(), whip_scene(), 20),
        ("door struck and swinging (0.5 s)".to_string(), door, 10),
        ("landing gear landing (0.05 s)".to_string(), landing, 1),
    ];
    for (name, scene, start) in &cases {
        let ticks = (scene.seconds / TICK).round() as usize;
        let baseline = run_scheduled(scene, Scheme::Game, settings(), Options::default(), &|_| 3);
        for effort in [(0usize, 3usize), (0, 15), (4, 15)] {
            let (first, last) = (*start, start + 40);
            let schedule = move |k: usize| if k >= first && k < last { effort } else { (0, 0) };
            let changed = run_scheduled_with(scene, Scheme::Game, settings(), Options::default(), &|_| 3, &schedule);
            let off = |j: usize| -> (f64, f64) {
                let mut place = 0.0f64;
                let mut speed = 0.0f64;
                for i in 0..scene.cars.len() {
                    place = place.max((changed[j][i].pose.translation - baseline[j][i].pose.translation).length());
                    speed = speed.max((changed[j][i].linvel - baseline[j][i].linvel).length());
                }
                (mm(place), mm(speed))
            };
            let worst = |from: usize, to: usize| -> (f64, f64) { (from..=to.min(ticks)).map(&off).fold((0.0, 0.0), |a, b| (a.0.max(b.0), a.1.max(b.1))) };
            let pop = off(first + 1);
            let inside = worst(first + 1, last);
            let back = off((last + 1).min(ticks));
            let wake = worst(last + 1, ticks);
            println!("{name:<34} {:>10} | {:>8.3} {:>8.2} | {:>8.3} {:>8.2} | {:>8.3} {:>8.2} | {:>8.3} {:>8.2}", format!("{effort:?}"), pop.0, pop.1, inside.0, inside.1, back.0, back.1, wake.0, wake.1);
        }
    }
}

/// The designs of the resting and fast tables: a name, a scheme, the steps a tick and the options.
fn rest_designs() -> Vec<Design> {
    let plain = Options::default();
    let ghost = |calls: usize, e: (usize, usize), truth: (usize, usize), toi: bool| Options { ghost_calls: calls, ghost_effort: e, world_effort: truth, toi, ..plain };
    vec![
        design("converged (12 steps, 16, 32)", Scheme::Hi, 3, plain),
        design("today: one world, 3 steps", Scheme::Game, 3, plain),
        design("one world, 3 steps, (0, 15)", Scheme::Game, 3, Options { world_effort: (0, 15), ..plain }),
        design("one world, 3 steps, (0, 15), cut", Scheme::Game, 3, Options { world_effort: (0, 15), toi: true, ..plain }),
        design("one world, 12 steps, (0, 15)", Scheme::Game, 12, Options { world_effort: (0, 15), ..plain }),
        design("ghost diff, x1", Scheme::GhostDifference, 3, plain),
        design("ghost diff, x1, (0, 15), truth (0, 15), cut", Scheme::GhostDifference, 3, ghost(1, (0, 15), (0, 15), true)),
        design("ghost diff, x4, (0, 15), truth (0, 15)", Scheme::GhostDifference, 3, ghost(4, (0, 15), (0, 15), false)),
        design("ghost diff, x4, (0, 15), truth today's", Scheme::GhostDifference, 3, ghost(4, (0, 15), (0, 0), false)),
        design("ghost impulses, x1, (0, 15), truth (0, 15), cut", Scheme::Ghost, 3, ghost(1, (0, 15), (0, 15), true)),
        design("own-ground ghost diff, x1, (0, 15), truth (0, 15), cut", Scheme::GhostDifference, 3, Options { own_ground: true, ..ghost(1, (0, 15), (0, 15), true) }),
        design("own-ground ghost impulses, (0, 15), truth (0, 15), cut", Scheme::Ghost, 3, Options { own_ground: true, ..ghost(1, (0, 15), (0, 15), true) }),
    ]
}

fn rest() {
    println!("
resting for 20 s: a tower of 4 cubes, the landing gear (it lands at 2 m/s first), a carrier with a ship on its deck. Per scene: speed noise in the last 5 s (mm/s), the drift of the hull between 10 s and 20 s (mm), and how far the settled height is from the converged world's (mm).");
    let designs = rest_designs();
    let (gear, gear_ids) = gear_scene(2.0, 20.0);
    let (carrier, carrier_ids, carried_ids) = carrier_scene(CarrierRun::Rest);
    let carrier = Scene { seconds: 20.0, ..carrier };
    let scenes: Vec<(&str, Scene, Vec<usize>)> = vec![("tower", tower_scene(4, 20.0), vec![0]), ("gear", gear, vec![gear_ids.hull]), ("carrier+ship", carrier, vec![carrier_ids.hull, carried_ids.hull])];
    print!("{:<56}", "design");
    for (name, _, _) in &scenes {
        print!(" | {:>8} {:>7} {:>7}", format!("{name} v"), "drift", "height");
    }
    println!(" | {:>8}", "us/tick");
    let mut references: Vec<Trace> = Vec::new();
    for (_, scene, _) in &scenes {
        references.push(run(scene, REFERENCE, settings()));
    }
    for d in &designs {
        print!("{:<56}", d.name);
        let mut micros = 0.0;
        for ((_, scene, hulls), reference) in scenes.iter().zip(&references) {
            let t = run_options(scene, d.scheme, d.settings, d.options);
            let last = t.since(scene.seconds - 5.0);
            let noise = last.iter().map(|f| f.iter().map(|s| s.linvel.length()).fold(0.0, f64::max)).fold(0.0, f64::max);
            let height = |trace: &Trace, from: f64, to: f64| -> f64 {
                let frames: Vec<&Vec<CarState>> = trace.frames.iter().skip((from / trace.dt) as usize).take(((to - from) / trace.dt) as usize).collect();
                frames.iter().map(|f| hulls.iter().map(|&h| f[h].pose.translation.y).sum::<f64>() / hulls.len() as f64).sum::<f64>() / frames.len() as f64
            };
            let drift = height(&t, 15.0, 20.0) - height(&t, 10.0, 15.0);
            let off = height(&t, 15.0, 20.0) - height(reference, 15.0, 20.0);
            print!(" | {:>8.1} {:>7.2} {:>7.2}", mm(noise), mm(drift), mm(off));
            micros += t.micros_per_tick / scenes.len() as f64;
        }
        println!(" | {micros:>8.0}");
    }
}

fn fast() {
    println!("
fast hits: a 1000 kg stranger comes at a stiff chain of 4 (fixed joints) from 5 m away; the stranger's exit speed (true: 0.2 of the closing speed, kept with the chain), the deepest overlap (mm), and the chain's end off the converged one (mm). Impact moment random (the median of 5 moments).");
    let designs: Vec<Design> = {
        let plain = Options::default();
        let ghost = |calls: usize, e: (usize, usize), truth: (usize, usize), toi: bool| Options { ghost_calls: calls, ghost_effort: e, world_effort: truth, toi, ..plain };
        vec![
            design("today: one world, 3 steps", Scheme::Game, 3, plain),
            design("one world, 3 steps, (0, 15), cut", Scheme::Game, 3, Options { world_effort: (0, 15), toi: true, ..plain }),
            design("ghost impulses, x1", Scheme::Ghost, 3, plain),
            design("ghost impulses, x1, (0, 15), cut", Scheme::Ghost, 3, ghost(1, (0, 15), (0, 15), true)),
            design("ghost diff, x1, (0, 15), cut", Scheme::GhostDifference, 3, ghost(1, (0, 15), (0, 15), true)),
            design("welded ghost diff, x1, cut", Scheme::GhostWeldedDifference, 3, ghost(1, (0, 0), (0, 0), true)),
            design("ghost diff, rule 0.8 m a piece, (0, 15)", Scheme::GhostDifference, 3, Options { calls_rule: Some(0.8), ..ghost(1, (0, 15), (0, 15), false) }),
            design("ghost diff, rule 0.8 m, (0, 15), cut", Scheme::GhostDifference, 3, Options { calls_rule: Some(0.8), ..ghost(1, (0, 15), (0, 15), true) }),
        ]
    };
    print!("{:<44}", "design");
    for speed in [20.0, 60.0, 150.0, 300.0] {
        print!(" | {:>7} {:>7} {:>7}", format!("{speed} m/s"), "overlap", "end off");
    }
    println!();
    for d in &designs {
        print!("{:<44}", d.name);
        for speed in [20.0f64, 60.0, 150.0, 300.0] {
            let mut exits = Vec::new();
            let mut overlaps = Vec::new();
            let mut offs = Vec::new();
            for k in 0..5 {
                let mut scene = chain_scene(JointKind::Fixed, Hit::Axial, 1000.0, 4);
                let last = scene.cars.len() - 1;
                let gap = 5.0 + speed * (TICK / 3.0) * (k as f64 / 5.0);
                scene.cars[last].pose = Pose::from_translation(v(3.0 + 0.5 + gap + 0.5, 0.0, 0.0));
                scene.cars[last].linvel = v(-speed, 0.0, 0.0);
                scene.seconds = ((gap / speed + 1.0) / TICK).ceil() * TICK;
                let reference = run(&scene, REFERENCE, settings());
                let t = run_options(&scene, d.scheme, d.settings, d.options);
                exits.push(t.last()[last].linvel.x / -speed);
                overlaps.push(mm((0..3).map(|c| overlap(&scene, &t.frames[t.frames.len() - 1], c, last)).fold(0.0, f64::max)).max(mm(t.frames.iter().map(|f| (1.0 - (f[3].pose.translation - f[last].pose.translation).length()).max(0.0)).fold(0.0, f64::max))));
                offs.push(mm((t.last()[3].pose.translation - reference.last()[3].pose.translation).length()));
            }
            print!(" | {:>7.3} {:>7.0} {:>7.0}", percentile(&exits, 50.0), percentile(&overlaps, 50.0), percentile(&offs, 50.0));
        }
        println!();
    }
}

fn invariance_cut() {
    let outcomes = outcomes();
    println!("
the same nine outcomes with the measures that make a hit settled: extra passes in the solver, and a step cut where the first touch is. The spread over the frame's step count is what a change of frame would show.");
    let truth = measure(&outcomes, REFERENCE, settings(), Options::default());
    print_outcome_header(&outcomes, "reference");
    print_outcome_row(&outcomes, "the world twice as fine as converged", &truth);
    let steps = |n: usize| Settings { steps_per_tick: n, ..Settings::default() };
    let plain = Options::default();
    let rows: Vec<(String, Scheme, Settings, Options)> = [3usize, 4, 6, 8, 12, 24].iter().map(|&n| (format!("one world, {n} steps, passes 15"), Scheme::Game, steps(n), Options { world_effort: (0, 15), ..plain })).collect();
    invariance_group(&outcomes, &truth, "one world, extra passes 15, the frame cut into n steps", &rows);
    let rows: Vec<(String, Scheme, Settings, Options)> = [3usize, 4, 6, 8, 12, 24].iter().map(|&n| (format!("one world, {n} steps, passes 15, cut"), Scheme::Game, steps(n), Options { world_effort: (0, 15), toi: true, ..plain })).collect();
    invariance_group(&outcomes, &truth, "the same, with the step cut where the first touch is", &rows);
    let rows: Vec<(String, Scheme, Settings, Options)> = [3usize, 4, 6, 8, 12, 24]
        .iter()
        .map(|&n| (format!("welded ghosts, {n} steps, passes 15, cut"), Scheme::GhostWeldedDifference, steps(n), Options { world_effort: (0, 15), ghost_effort: (0, 15), toi: true, ..plain }))
        .collect();
    invariance_group(&outcomes, &truth, "welded ghosts (difference), the groups' worlds at n steps, passes 15 everywhere, cut", &rows);
}

fn birth() {
    println!("
a ghost that is born late (nothing warm in it: no contact has been solved before, no joint has a warm start) against one that was there from the start: how far the run is, from the first touch on, when the ghost is born k steps before the touch (k < 0: after it). Both ghosts are the welded difference form at passes 15 with the cut.");
    let options = Options { ghost_effort: (0, 15), world_effort: (0, 15), toi: true, ..Options::default() };
    let own = Options { own_ground: true, ..options };
    let cases: Vec<(&str, Scene, f64, Scheme, Options)> = {
        let (gear, ids) = gear_scene(2.0, 3.0);
        let touch_gear = run(&gear, Scheme::Hi, settings()).frames.iter().enumerate().find(|(_, f)| ids.feet.iter().any(|&c| f[c].pose.translation.y - 0.1 < 0.005)).map(|(k, _)| k as f64 * (TICK / 12.0)).expect("lands");
        let (carrier, carrier_ids, carried_ids) = carrier_scene(CarrierRun::Land);
        let deck = carrier.cars[carrier_ids.hull].pose.translation.y + 0.5;
        let touch_deck = run(&carrier, Scheme::Hi, settings()).frames.iter().enumerate().find(|(_, f)| carried_ids.feet.iter().any(|&c| f[c].pose.translation.y - 0.1 < deck + 0.005)).map(|(k, _)| k as f64 * (TICK / 12.0)).expect("lands");
        let carrier = Scene { seconds: 3.0, ..carrier };
        vec![
            ("landing gear lands on the ground, ground in the ghost", gear.clone(), touch_gear, Scheme::GhostDifference, options),
            ("a ship lands on a carrier, ground in the ghost", carrier.clone(), touch_deck, Scheme::GhostDifference, options),
            ("a ship lands on a carrier, ground in the groups' own worlds", carrier, touch_deck, Scheme::GhostDifference, own),
        ]
    };
    println!("{:<60} {:>8} | {:>9} {:>9} | {:>9} {:>9} | {:>9} {:>9} | {:>9} {:>9}", "scene", "touch s", "k = 12", "", "k = 3", "", "k = 1", "", "k = -1", "");
    println!("{:<60} {:>8} | {:>9} {:>9} | {:>9} {:>9} | {:>9} {:>9} | {:>9} {:>9}", "", "", "mm", "mm/s", "mm", "mm/s", "mm", "mm/s", "mm", "mm/s");
    for (name, scene, touch, scheme, options) in &cases {
        let all = all_cars(scene);
        let early = run_options(scene, *scheme, settings(), *options);
        print!("{name:<60} {touch:>8.3} |");
        for k in [12.0f64, 3.0, 1.0, -1.0] {
            let born = (touch - k * (TICK / 3.0)).max(0.0);
            let late = run_options(scene, *scheme, settings(), Options { ghost_born_at: born, ..*options });
            let d = deviation(scene, &late, &early, TICK, &all, 0.0, scene.seconds);
            print!(" {:>9.2} {:>9.1} |", mm(d.position), mm(d.velocity));
        }
        println!();
    }
}

fn conserve() {
    println!("
what a hit keeps, in no gravity: the change of the whole system's momentum over the hit (kg m/s; it must be 0) and the energy kept (the converged world's in brackets). Hinged and stiff chains hit at 20 m/s by 1000 kg.");
    println!("{:<50} | {:>12} {:>10} | {:>12} {:>10}", "design", "stiff dP", "energy", "hinged dP", "energy");
    let stiff = {
        let mut s = stiff_approach_scene(20.0);
        s.seconds = 1.25;
        s
    };
    let hinged = {
        let mut s = chain_scene(JointKind::Hinge, Hit::Axial, 1000.0, 8);
        s.cars[8].linvel = v(-20.0, 0.0, 0.0);
        s
    };
    let references: Vec<f64> = [&stiff, &hinged].iter().map(|scene| { let t = run(scene, REFERENCE, settings()); kinetic_energy(scene, t.last()) / kinetic_energy(scene, &t.frames[0]) }).collect();
    for d in rest_designs() {
        print!("{:<50} |", d.name);
        for scene in [&stiff, &hinged] {
            let t = run_options(scene, d.scheme, d.settings, d.options);
            let dp = (total_momentum(scene, t.last()) - total_momentum(scene, &t.frames[0])).length();
            let energy = kinetic_energy(scene, t.last()) / kinetic_energy(scene, &t.frames[0]);
            print!(" {dp:>12.3e} {energy:>10.4} |");
        }
        println!();
    }
    println!("{:<50} | {:>12} {:>10.4} | {:>12} {:>10.4}", "(the world twice as fine as converged)", "", references[0], "", references[1]);
}

fn fingerprints() {
    // Bit for bit: every number of every car in every frame of a run, hashed; the lines to compare between this machine and another.
    let mut scenes: Vec<(String, Scene)> = Vec::new();
    scenes.push(("stiff chain, stranger at 20 m/s".to_string(), stiff_approach_scene(20.0)));
    let mut across = chain_scene(JointKind::Hinge, Hit::Transverse, 1000.0, 6);
    across.seconds = 1.0;
    scenes.push(("hinged chain of 6 across".to_string(), across));
    let mut door = door_scene(false, 1000.0, 4.0);
    door.seconds = 2.0;
    scenes.push(("spring door".to_string(), door));
    scenes.push(("gear lands at 2 m/s, 3 s".to_string(), gear_scene(2.0, 3.0).0));
    let mut carrier = carrier_scene(CarrierRun::Land).0;
    carrier.seconds = 3.0;
    scenes.push(("ship lands on a carrier, 3 s".to_string(), carrier));
    let (hit, _, _) = gear_hit_scene(2000.0, 6.0, 1.0, 2.5);
    scenes.push(("landed ship hit from the side".to_string(), hit));
    let mut designs = rest_designs();
    designs.extend(sweep_designs("main"));
    designs.extend(sweep_designs("knee"));
    let mut seen = std::collections::HashSet::new();
    for (name, scene) in &scenes {
        for d in &designs {
            if !seen.insert((name.clone(), d.name.clone())) {
                continue;
            }
            let mut scene = scene.clone();
            if d.scheme == Scheme::HiFiner || d.scheme == Scheme::Hi {
                scene.seconds = scene.seconds.min(1.0);
            }
            let t = run_options(&scene, d.scheme, d.settings, d.options);
            println!("fingerprint {name:<34} {:<56} {:016x}", d.name, t.fingerprint());
        }
    }
}

fn bystander() {
    println!("
does something happening elsewhere change a mechanism that has nothing to do with it? A rover coasts at 5 m/s (group 0) and a chain whips in no gravity far away... no: the rover, and 10 m to its side another ship comes down at 2 m/s and lands at 2 s (group 1), under its thrust (the pushes cancel its weight until 0.05 s before the landing). The rover's greatest difference in place (mm) and speed (mm/s) between the run without the other ship and with it, from 2 s on.");
    let (alone, _) = rover_scene(false, true);
    let legs = small_legs();
    let mut with = alone.clone();
    let hull_height = legs.drop_below_hull() + 4.0123;
    let other = add_ship(&mut with, 1, "other", &legs, v(8.0, hull_height, 10.0), v(0.0, -2.0, 0.0));
    for &car in std::iter::once(&other.hull).chain(&other.feet) {
        let mass = with.cars[car].mass;
        with.pushes.push(Push { car, force: v(0.0, mass * 9.81, 0.0), from: 0.0, until: 1.95 });
    }
    let first: Vec<usize> = (0..alone.cars.len()).collect();
    println!("{:<54} {:>10} {:>10} {:>8}", "design", "place mm", "speed mm/s", "cuts");
    let plain = Options::default();
    let ghost = |toi: bool, own: bool| Options { ghost_effort: (0, 15), world_effort: (0, 15), toi, own_ground: own, ..plain };
    let designs = vec![
        design("today: one world, 3 steps", Scheme::Game, 3, plain),
        design("one world, 3 steps, (0, 15)", Scheme::Game, 3, Options { world_effort: (0, 15), ..plain }),
        design("one world, 3 steps, (0, 15), cut", Scheme::Game, 3, Options { world_effort: (0, 15), toi: true, ..plain }),
        design("ghost diff, (0, 15), cut", Scheme::GhostDifference, 3, ghost(true, false)),
        design("ghost impulses, (0, 15), cut", Scheme::Ghost, 3, ghost(true, false)),
        design("own-ground ghost diff, (0, 15), cut", Scheme::GhostDifference, 3, ghost(true, true)),
        design("own-ground ghost impulses, (0, 15), cut", Scheme::Ghost, 3, ghost(true, true)),
    ];
    for d in designs {
        let a = run_options(&alone, d.scheme, d.settings, d.options);
        let b = run_options(&with, d.scheme, d.settings, d.options);
        let dev = deviation(&alone, &b, &a, TICK, &first, 2.0, 8.0);
        println!("{:<54} {:>10.4} {:>10.4} {:>8}", d.name, mm(dev.position), mm(dev.velocity), b.cuts);
    }
}

fn debugcut() {
    {
        use rapier3d::parry::query::{ShapeCastOptions, cast_shapes};
        use rapier3d::prelude::SharedShape;
        let foot = SharedShape::cuboid(0.3, 0.1, 0.3);
        let ground = SharedShape::cuboid(200.0, 0.5, 200.0);
        let options = ShapeCastOptions { max_time_of_impact: 0.3, target_distance: 0.0, stop_at_penetration: true, compute_impact_geometry_on_penetration: false };
        let result = cast_shapes(&Pose::from_translation(v(0.0, 0.5, 0.0)), v(0.0, -2.0, 0.0), &*foot, &Pose::from_translation(v(0.0, -0.5, 0.0)), Vector::ZERO, &*ground, options);
        println!("cuboid foot at 0.5 above a ground cuboid, going down at 2 m/s: {:?} (true 0.2 s)", result.map(|r| r.map(|h| h.time_of_impact)));
        let ball = SharedShape::ball(0.5);
        let result = cast_shapes(&Pose::from_translation(v(0.0, 1.0, 0.0)), v(0.0, -2.0, 0.0), &*ball, &Pose::from_translation(v(0.0, -0.5, 0.0)), Vector::ZERO, &*ground, options);
        println!("ball at 1.0 above the ground cuboid: {:?} (true 0.25 s)", result.map(|r| r.map(|h| h.time_of_impact)));
    }
    let plain = Options::default();
    let (gear, ids) = gear_scene(2.0, 1.0);
    let t = run_options(&gear, Scheme::Game, settings(), Options { world_effort: (0, 15), toi: true, ..plain });
    println!("gear landing: cuts {}, hull y at the end {:.4}", t.cuts, t.last()[ids.hull].pose.translation.y);
    let legs = small_legs();
    let (alone, _) = rover_scene(false, true);
    let mut with = alone.clone();
    let hull_height = legs.drop_below_hull() + 4.0123;
    let other = add_ship(&mut with, 1, "other", &legs, v(8.0, hull_height, 10.0), v(0.0, -2.0, 0.0));
    for &car in std::iter::once(&other.hull).chain(&other.feet) {
        let mass = with.cars[car].mass;
        with.pushes.push(Push { car, force: v(0.0, mass * 9.81, 0.0), from: 0.0, until: 1.95 });
    }
    let t = run_options(&with, Scheme::Game, settings(), Options { world_effort: (0, 15), toi: true, ..plain });
    for k in [0usize, 20, 40, 41, 42, 43, 44, 60, 100] {
        let f = t.at(k as f64 * TICK);
        println!("t {:.2}: other hull y {:.3} vy {:+.3}; foot y {:.3}", k as f64 * TICK, f[other.hull].pose.translation.y, f[other.hull].linvel.y, f[other.feet[0]].pose.translation.y);
    }
    println!("cuts {}", t.cuts);
}

fn main() {
    let which = std::env::args().nth(1).unwrap_or_else(|| "all".to_string());
    let all = which == "all";
    for (name, function) in [("hicheck", hicheck as fn()), ("pop", pop), ("doortrace", doortrace), ("invariance", invariance), ("sweep", sweep), ("reference", reference), ("effortpop", effort_pop), ("rest", rest), ("fast", fast), ("invariancecut", invariance_cut), ("birth", birth), ("conserve", conserve), ("fingerprints", fingerprints), ("bystander", bystander), ("debugcut", debugcut)] {
        if which == name || all {
            function();
        }
    }
}
