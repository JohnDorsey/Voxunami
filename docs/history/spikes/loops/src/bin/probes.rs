//! Further probes of what jointed ships do when nothing pushes them: free spin, the step a multibody needs, an exact-mass direct solve, overlapping
//! cars, a strained loop, chains of fixed joints, and what the solver dials buy.
//! Run: `cargo run --release --bin probes -- [spin|steps|exact|overlap|strain|fixed|dials|all]`.

use std::time::Instant;

use loop_probe::harness::*;
use loop_probe::*;

fn main() {
    let which = std::env::args().nth(1).unwrap_or_else(|| "all".to_string());
    let all = which == "all";
    if which == "spin" || all {
        spin();
    }
    if which == "steps" || all {
        steps();
    }
    if which == "exact" || all {
        exact();
    }
    if which == "overlap" || all {
        overlap();
    }
    if which == "strain" || all {
        strain();
    }
    if which == "fixed" || all {
        fixed();
    }
    if which == "dials" || all {
        dials();
    }
    if which == "plate" || all {
        plate();
    }
    if which == "map" || all {
        map();
    }
    if which == "floor" || all {
        floor();
    }
    if which == "boxes" || all {
        boxes();
    }
    if which == "trace" || all {
        trace();
    }
    if which == "offcentre" || all {
        offcentre();
    }
    if which == "tunnel" || all {
        tunnel();
    }
}

/// Tunnelling judged properly: at any tick, is the cube's centre beyond the plate's far face while inside the plate's footprint, in the plate's own frame?
fn tunnel() {
    println!("== TUNNEL. A cube of 2 (8 kg) hits a free hull plate of t x 8 x 8 voxels (voxel shapes) at speed v, aimed `offset` voxels off its centre. THROUGH = at some tick the cube's centre was beyond the plate's far face and inside its footprint. ==");
    println!("{:<10} {:<12} {:<8} {:<8} {}", "plate", "plate mass", "step", "offset", "v = 20 / 50 / 100 / 150 / 200 / 300 / 400 m/s");
    for (thickness, mass) in [(1i64, 64.0f64), (1, 6400.0), (2, 128.0), (4, 256.0)] {
        for (step_name, settings) in [("game", game()), ("12 x 4", Settings { steps_per_tick: 12, ..game() })] {
            for offset in [0.0f64, 3.0] {
                let mut cells = Vec::new();
                for v in [20.0f64, 50.0, 100.0, 150.0, 200.0, 300.0, 400.0] {
                    let mut world = World::new(settings, Vector::ZERO);
                    let cube = voxel_car(&mut world, [2, 2, 2], Vector::new(0.0, 13.0 + offset, 13.0 + offset), 8.0, Vector::new(v, 0.0, 0.0), false);
                    let plate = voxel_car(&mut world, [thickness, 8, 8], Vector::new(30.0, 10.0, 10.0), mass, Vector::ZERO, false);
                    let mut through = false;
                    let ticks = (60.0 / v / 0.05).ceil() as usize + 60;
                    for _ in 0..ticks {
                        world.tick();
                        let local = world.bodies[plate.handle].rotation().inverse() * (world.bodies[cube.handle].center_of_mass() - world.bodies[plate.handle].center_of_mass());
                        if local.x > thickness as f64 / 2.0 && local.y.abs() < 4.0 && local.z.abs() < 4.0 {
                            through = true;
                        }
                    }
                    cells.push(if through { "THROUGH" } else { "ok" });
                }
                println!("{:<10} {:<12} {step_name:<8} {offset:<8} {}", format!("{thickness} thick"), format!("{mass} kg"), cells.join(" / "));
            }
        }
    }
}

/// Off-centre hits on a free light plate. The plate spins afterwards, so 'through' is judged by which side of the plate's own plane the cube's centre of mass is on.
fn offcentre() {
    println!("== OFF-CENTRE. A cube of 2 (8 kg) hits a free plate (1 x 8 x 8, 64 kg) at speed v, aimed `offset` voxels off the plate's centre in y and z. After 20 s, which side of the plate's plane is the cube's centre on? ==");
    println!("{:<8} {:<10} {:<10} {}", "shapes", "step", "offset", "v = 5 / 20 / 100 / 300 m/s");
    for as_boxes in [false, true] {
        for (step_name, settings) in [("game", game()), ("12 x 4", Settings { steps_per_tick: 12, ..game() })] {
            for offset in [0.0f64, 1.0, 3.0] {
                let mut cells = Vec::new();
                for v in [5.0f64, 20.0, 100.0, 300.0] {
                    let mut world = World::new(settings, Vector::ZERO);
                    let (cube, plate) = if as_boxes {
                        let mut make = |half: Vector, position: Vector, mass: f64, velocity: Vector| {
                            let inertia = Vector::new(mass / 3.0 * (half.y * half.y + half.z * half.z), mass / 3.0 * (half.x * half.x + half.z * half.z), mass / 3.0 * (half.x * half.x + half.y * half.y));
                            let handle = world.bodies.insert(RigidBodyBuilder::dynamic().translation(position).linvel(velocity).additional_mass_properties(MassProperties::new(Vector::ZERO, mass, inertia)));
                            world.colliders.insert_with_parent(ColliderBuilder::cuboid(half.x, half.y, half.z).density(0.0).friction(0.6), handle, &mut world.bodies);
                            handle
                        };
                        (make(Vector::new(1.0, 1.0, 1.0), Vector::new(1.0, 14.0 + offset, 14.0 + offset), 8.0, Vector::new(v, 0.0, 0.0)), make(Vector::new(0.5, 4.0, 4.0), Vector::new(30.5, 14.0, 14.0), 64.0, Vector::ZERO))
                    } else {
                        let c = voxel_car(&mut world, [2, 2, 2], Vector::new(0.0, 13.0 + offset, 13.0 + offset), 8.0, Vector::new(v, 0.0, 0.0), false);
                        let p = voxel_car(&mut world, [1, 8, 8], Vector::new(30.0, 10.0, 10.0), 64.0, Vector::ZERO, false);
                        (c.handle, p.handle)
                    };
                    let ticks = (80.0 / v / 0.05).ceil() as usize + 400;
                    let mut deepest = 0.0f64;
                    for _ in 0..ticks {
                        world.tick();
                        let normal = *world.bodies[plate].rotation() * Vector::X;
                        let side = (world.bodies[cube].center_of_mass() - world.bodies[plate].center_of_mass()).dot(normal);
                        deepest = deepest.max(side);
                    }
                    let normal = *world.bodies[plate].rotation() * Vector::X;
                    let side = (world.bodies[cube].center_of_mass() - world.bodies[plate].center_of_mass()).dot(normal);
                    cells.push(if side > 0.0 { format!("THROUGH ({side:.1})") } else { format!("ok ({side:.1})") });
                    let _ = deepest;
                }
                println!("{:<8} {step_name:<10} {offset:<10} {}", if as_boxes { "boxes" } else { "voxels" }, cells.join(" / "));
            }
        }
    }
}

/// Box version of the cube hitting the light plate at 5 m/s, tick by tick around the impact: velocities, overlap, contact points, sleeping.
fn trace() {
    let mut world = World::new(game(), Vector::ZERO);
    let mut make = |half: Vector, position: Vector, mass: f64, velocity: Vector| {
        let inertia = Vector::new(mass / 3.0 * (half.y * half.y + half.z * half.z), mass / 3.0 * (half.x * half.x + half.z * half.z), mass / 3.0 * (half.x * half.x + half.y * half.y));
        let handle = world.bodies.insert(RigidBodyBuilder::dynamic().translation(position).linvel(velocity).additional_mass_properties(MassProperties::new(Vector::ZERO, mass, inertia)));
        world.colliders.insert_with_parent(ColliderBuilder::cuboid(half.x, half.y, half.z).density(0.0).friction(0.6), handle, &mut world.bodies);
        handle
    };
    let cube = make(Vector::new(1.0, 1.0, 1.0), Vector::new(1.0, 14.0, 14.0), 8.0, Vector::new(5.0, 0.0, 0.0));
    let plate = make(Vector::new(0.5, 4.0, 4.0), Vector::new(30.5, 14.0, 14.0), 64.0, Vector::ZERO);
    let (c1, c2) = (world.bodies[cube].colliders()[0], world.bodies[plate].colliders()[0]);
    println!("boxes, 5 m/s: tick, overlap (cube front - plate back), cube v, plate v, contact points, asleep(cube,plate)");
    for tick in 0..260 {
        world.tick();
        if (108..130).contains(&tick) || tick % 20 == 0 {
            let overlap = world.bodies[cube].translation().x + 1.0 - (world.bodies[plate].translation().x - 0.5);
            let points = world.narrow_phase.contact_pair(c1, c2).map_or(0, |p| p.manifolds().iter().map(|m| m.points.len()).sum::<usize>());
            println!("  {tick:>3} {overlap:>8.3} {:>8.4} {:>8.4} {points:>3} {} {}", world.bodies[cube].linvel().x, world.bodies[plate].linvel().x, world.bodies[cube].is_sleeping(), world.bodies[plate].is_sleeping());
        }
    }
}

/// The same cube-and-plate meetings with the shapes as boxes instead of voxels: is the pass-through a property of the voxel-to-voxel contact?
fn boxes() {
    println!("== BOXES. A cube of 2 (8 kg) hits a free plate (1 x 8 x 8, 64 kg) at speed v; shapes as voxels or as boxes. Where does the cube end, 10 s later? (through = beyond the plate's far face) ==");
    println!("{:<14} {:<10} {}", "shapes", "step", "v = 1 / 5 / 20 / 50 / 100 m/s");
    for as_boxes in [false, true] {
        for (step_name, settings) in [("game", game()), ("12 x 4", Settings { steps_per_tick: 12, ..game() })] {
            let mut cells = Vec::new();
            for v in [1.0f64, 5.0, 20.0, 50.0, 100.0] {
                let mut world = World::new(settings, Vector::ZERO);
                let (cube, plate) = if as_boxes {
                    let mut make = |half: Vector, position: Vector, mass: f64, velocity: Vector| {
                        let inertia = Vector::new(mass / 3.0 * (half.y * half.y + half.z * half.z), mass / 3.0 * (half.x * half.x + half.z * half.z), mass / 3.0 * (half.x * half.x + half.y * half.y));
                        let handle = world.bodies.insert(RigidBodyBuilder::dynamic().translation(position).linvel(velocity).additional_mass_properties(MassProperties::new(Vector::ZERO, mass, inertia)));
                        world.colliders.insert_with_parent(ColliderBuilder::cuboid(half.x, half.y, half.z).density(0.0).friction(0.6), handle, &mut world.bodies);
                        handle
                    };
                    (make(Vector::new(1.0, 1.0, 1.0), Vector::new(1.0, 14.0, 14.0), 8.0, Vector::new(v, 0.0, 0.0)), make(Vector::new(0.5, 4.0, 4.0), Vector::new(30.5, 14.0, 14.0), 64.0, Vector::ZERO))
                } else {
                    let c = voxel_car(&mut world, [2, 2, 2], Vector::new(0.0, 13.0, 13.0), 8.0, Vector::new(v, 0.0, 0.0), false);
                    let p = voxel_car(&mut world, [1, 8, 8], Vector::new(30.0, 10.0, 10.0), 64.0, Vector::ZERO, false);
                    (c.handle, p.handle)
                };
                let ticks = (80.0 / v / 0.05).ceil() as usize + 200;
                for _ in 0..ticks {
                    world.tick();
                }
                // Positions of the back faces: voxel bodies have their origin at the back face, box bodies at the middle.
                let (cube_back, plate_front) = if as_boxes { (world.bodies[cube].translation().x - 1.0, world.bodies[plate].translation().x + 0.5) } else { (world.bodies[cube].translation().x, world.bodies[plate].translation().x + 1.0) };
                cells.push(if cube_back > plate_front - 0.05 { "THROUGH".to_string() } else { format!("held {:.2}", plate_front - cube_back) });
            }
            println!("{:<14} {step_name:<10} {}", if as_boxes { "boxes" } else { "voxels" }, cells.join(" / "));
        }
    }
}

/// A cube dropped on a horizontal hull plate (a ship standing on another ship's thin floor), under gravity 5 m/s^2. Does it stay on top?
fn floor() {
    println!("== FLOOR. A cube of 2 (8 kg) dropped from a height h onto a horizontal plate of 8 x 1 x 8 voxels, gravity 5 m/s^2, 30 s. 'on' = it ends resting on top, 'THROUGH' = it ends below the plate. ==");
    println!("{:<44} {:<12} {:>24} {:>24} {:>24}", "plate", "step", "h = 0.5 m", "h = 5 m", "h = 50 m");
    for (name, fixed, mass) in [("fixed", true, 64.0), ("free, 64 kg (the cube is lighter)", false, 64.0), ("free, 6400 kg", false, 6400.0)] {
        for (step_name, settings) in [("game", game()), ("12 x 4 (1 ms)", Settings { steps_per_tick: 12, ..game() })] {
            let mut cells = Vec::new();
            for h in [0.5f64, 5.0, 50.0] {
                // The free plates are held up by a field of their own: give them gravity-cancelling force each tick so that only the cube's weight is the question.
                let mut world = World::new(settings, Vector::new(0.0, -5.0, 0.0));
                let plate = voxel_car(&mut world, [8, 1, 8], Vector::new(10.0, 10.0, 10.0), mass, Vector::ZERO, fixed);
                let cube = voxel_car(&mut world, [2, 2, 2], Vector::new(13.0, 11.0 + h, 13.0), 8.0, Vector::ZERO, false);
                for _ in 0..600 {
                    if !fixed {
                        world.bodies[plate.handle].add_force(Vector::new(0.0, 5.0 * mass, 0.0), true);
                    }
                    world.tick();
                }
                let (cube_y, plate_y) = (world.bodies[cube.handle].translation().y, world.bodies[plate.handle].translation().y);
                let status = if cube_y > plate_y + 0.9 { "on" } else { "THROUGH" };
                cells.push(format!("{status} (cube {:.2} above plate)", cube_y - plate_y - 1.0));
            }
            println!("{name:<44} {step_name:<12} {:>24} {:>24} {:>24}", cells[0], cells[1], cells[2]);
        }
    }
}

/// Where does contact between two voxel bodies exist? Two fixed bodies are placed overlapping by d along x (for several shapes and offsets), one step is taken and
/// the contact pair is read: the deepest overlap that still has a contact point.
fn map() {
    println!("== MAP. A cube of 2 against another voxel body, both fixed, overlapping by d voxels along x. Contact points exist (+) or not (.) for d = 0.05 .. 1.0 step 0.05, then 1.25, 1.5, 1.75. ==");
    let depths: Vec<f64> = (1..=20).map(|k| k as f64 * 0.05).chain([1.25, 1.5, 1.75]).collect();
    let header: String = depths.iter().map(|d| if (d * 100.0).round() as i64 % 25 == 0 { format!("{d:<4.2}") } else { String::new() }).collect::<Vec<_>>().join("");
    println!("{:<46} (columns in order; a mark every 0.05, quarter marks: 0.25 0.50 0.75 1.00 1.25 1.50 1.75) {header}", "other body, offset in y and z");
    let shapes: [(&str, [i64; 3]); 5] = [("cube of 2", [2, 2, 2]), ("cube of 4", [4, 4, 4]), ("plate 1 x 8 x 8", [1, 8, 8]), ("plate 2 x 8 x 8", [2, 8, 8]), ("block 8 x 8 x 8", [8, 8, 8])];
    for (name, size) in shapes {
        for (offset_name, offset) in [("aligned", Vector::ZERO), ("half a voxel off", Vector::new(0.0, 0.5, 0.5)), ("0.3 off", Vector::new(0.0, 0.3, 0.3))] {
            let mut marks = String::new();
            for &d in &depths {
                let mut world = World::new(Settings::default(), Vector::ZERO);
                let cube = voxel_car(&mut world, [2, 2, 2], Vector::new(0.0, 14.0, 14.0), 8.0, Vector::ZERO, false);
                let other_x = 2.0 - d;
                let other_y = 14.0 + offset.y - (size[1] as f64 / 2.0 - 1.0);
                let other_z = 14.0 + offset.z - (size[2] as f64 / 2.0 - 1.0);
                let other = voxel_car(&mut world, size, Vector::new(other_x, other_y, other_z), 8.0, Vector::ZERO, true);
                world.step();
                let (c1, c2) = (world.bodies[cube.handle].colliders()[0], world.bodies[other.handle].colliders()[0]);
                let has = world.narrow_phase.contact_pair(c1, c2).is_some_and(|pair| pair.manifolds().iter().any(|m| m.points.iter().any(|p| p.dist < 0.0)));
                marks.push(if has { '+' } else { '.' });
            }
            println!("{:<46} {marks}", format!("{name}, {offset_name}"));
        }
    }
}

fn voxel_car(world: &mut World, size: [i64; 3], position: Vector, mass: f64, velocity: Vector, fixed: bool) -> Car {
    let mut keys = Vec::new();
    for x in 0..size[0] {
        for y in 0..size[1] {
            for z in 0..size[2] {
                keys.push(IVector::new(x, y, z));
            }
        }
    }
    let centre = Vector::new(size[0] as f64, size[1] as f64, size[2] as f64) * 0.5;
    let inertia = Vector::new(
        mass / 12.0 * (size[1].pow(2) + size[2].pow(2)) as f64,
        mass / 12.0 * (size[0].pow(2) + size[2].pow(2)) as f64,
        mass / 12.0 * (size[0].pow(2) + size[1].pow(2)) as f64,
    );
    let builder = if fixed { RigidBodyBuilder::fixed() } else { RigidBodyBuilder::dynamic() };
    let handle = world.bodies.insert(builder.translation(position).linvel(velocity).additional_mass_properties(MassProperties::new(centre, mass, inertia)));
    world.colliders.insert_with_parent(ColliderBuilder::voxels(Vector::ONE, &keys).density(0.0).friction(0.6), handle, &mut world.bodies);
    Car { handle, mass, inertia }
}

/// The cube-and-plate tunnelling, rebuilt directly in Rapier, with the settings that might be the cause switched one by one.
fn plate() {
    println!("== PLATE. A cube of 2 (8 kg) at 5 m/s meets a free plate of 1 x 8 x 8 (64 kg) in space; where is the cube after 10 s? (>= 31 means it is through) ==");
    println!("{:<40} {:>10} {:>12} {:>12} {:>12}", "setting", "cube x", "plate x", "cube v", "plate v");
    let game = Settings::default();
    type Tweak = fn(&mut IntegrationParameters);
    let tweaks: [(&str, Tweak); 8] = [
        ("Rapier defaults (as the game)", |_| {}),
        ("contact_recycling off", |p| p.contact_recycling = false),
        ("contact_clustering off", |p| p.contact_clustering = false),
        ("both off", |p| {
            p.contact_recycling = false;
            p.contact_clustering = false;
        }),
        ("friction model Coulomb", |p| p.friction_model = FrictionModel::Coulomb),
        ("prediction distance x10", |p| p.normalized_prediction_distance *= 10.0),
        ("contact softness 120 Hz", |p| p.contact_softness = SpringCoefficients::new(120.0, 10.0)),
        ("max corrective velocity x10", |p| p.normalized_max_corrective_velocity *= 10.0),
    ];
    {
        // Look at the contact pair itself while the cube creeps into the plate.
        let mut world = World::new(game, Vector::ZERO);
        let cube = voxel_car(&mut world, [2, 2, 2], Vector::new(0.0, 14.0, 14.0), 8.0, Vector::new(5.0, 0.0, 0.0), false);
        let plate = voxel_car(&mut world, [1, 8, 8], Vector::new(30.0, 10.0, 10.0), 64.0, Vector::ZERO, false);
        let (c1, c2) = (world.bodies[cube.handle].colliders()[0], world.bodies[plate.handle].colliders()[0]);
        for tick in 0..140 {
            world.tick();
            if tick >= 108 && tick % 3 == 0 {
                let overlap = world.bodies[cube.handle].translation().x + 2.0 - world.bodies[plate.handle].translation().x;
                match world.narrow_phase.contact_pair(c1, c2) {
                    Some(pair) => {
                        let points: Vec<String> = pair.manifolds().iter().flat_map(|m| m.points.iter().map(move |p| format!("d={:.3} n=({:.1},{:.1},{:.1}) i={:.2e}", p.dist, m.data.normal.x, m.data.normal.y, m.data.normal.z, p.data.impulse))).collect();
                        println!("  tick {tick}: overlap {overlap:.3} m, {} manifolds, {} points {}", pair.manifolds().len(), points.len(), points.iter().take(3).cloned().collect::<Vec<_>>().join(" | "));
                    }
                    None => println!("  tick {tick}: overlap {overlap:.3} m, no contact pair"),
                }
            }
        }
    }
    for (name, tweak) in tweaks {
        let mut world = World::new(game, Vector::ZERO);
        tweak(&mut world.parameters);
        let cube = voxel_car(&mut world, [2, 2, 2], Vector::new(0.0, 14.0, 14.0), 8.0, Vector::new(5.0, 0.0, 0.0), false);
        let plate = voxel_car(&mut world, [1, 8, 8], Vector::new(30.0, 10.0, 10.0), 64.0, Vector::ZERO, false);
        for _ in 0..200 {
            world.tick();
        }
        println!("{name:<40} {:>10.3} {:>12.3} {:>12.4} {:>12.4}", world.bodies[cube.handle].translation().x, world.bodies[plate.handle].translation().x, world.bodies[cube.handle].linvel().x, world.bodies[plate.handle].linvel().x);
    }
}

fn game() -> Settings {
    Settings::default()
}

/// A single free rigid body, torque-free: does its kinetic energy stay put at constant angular momentum?
fn spin() {
    println!("== SPIN. One free car, no torque, 1 hour. Kinetic energy and |L| relative to the start, at 1, 10, 30 and 60 minutes (Rapier's own gyroscopic step). ==");
    println!("{:<26} {:<18} {:>8} {:<14} {:>34} {:>34}", "inertia", "spin about", "rad/s", "step", "dE/E at 1/10/30/60 min", "d|L|/|L| at 1/10/30/60 min");
    let tensors: [(&str, [f64; 3]); 2] = [("I = (1, 2, 3)", [1.0, 2.0, 3.0]), ("ship hull 20x6x4 m", [52.0, 416.0, 436.0])];
    for (name, inertia) in tensors {
        let order = {
            let mut axes = [0usize, 1, 2];
            axes.sort_by(|&a, &b| inertia[a].total_cmp(&inertia[b]));
            axes
        };
        for (axis_name, axis) in [("max-inertia axis", order[2]), ("intermediate axis", order[1]), ("min-inertia axis", order[0])] {
            for rate in [0.5f64, 5.0] {
                for (step_name, settings) in [("game 4.2 ms", game()), ("1.0 ms", Settings { steps_per_tick: 12, ..game() })] {
                    let mut world = World::new(settings, Vector::ZERO);
                    let car = add_car(&mut world, Pose::IDENTITY, Vector::new(1.0, 1.0, 1.0), 1000.0);
                    let tensor = Vector::new(inertia[0], inertia[1], inertia[2]);
                    world.bodies[car.handle].set_additional_mass_properties(MassProperties::new(Vector::ZERO, 1000.0, tensor), true);
                    let car = Car { inertia: tensor, ..car };
                    let mut w = [0.0f64; 3];
                    w[axis] = rate;
                    w[order[1] % 3 + 0] += 0.0;
                    // A tilt of 0.4 % toward another axis.
                    let other = (axis + 1) % 3;
                    w[other] += 0.004 * rate;
                    world.bodies[car.handle].set_angvel(Vector::new(w[0], w[1], w[2]), true);
                    let start = totals(&world, &[car]);
                    let ticks_per_minute = (60.0 / settings.tick_length).round() as usize;
                    let mut e_cells = Vec::new();
                    let mut l_cells = Vec::new();
                    for minute in 1..=60usize {
                        for _ in 0..ticks_per_minute {
                            world.tick();
                        }
                        if [1usize, 10, 30, 60].contains(&minute) {
                            let t = totals(&world, &[car]);
                            e_cells.push(format!("{:+.1e}", (t.energy - start.energy) / start.energy));
                            l_cells.push(format!("{:+.1e}", (t.angular_momentum.length() - start.angular_momentum.length()) / start.angular_momentum.length()));
                        }
                    }
                    println!("{name:<26} {axis_name:<18} {rate:>8} {step_name:<14} {:>34} {:>34}", e_cells.join(" "), l_cells.join(" "));
                }
            }
        }
    }
}

/// How small must the step be for a multibody rig of 16 cars to behave? All-impulse and multibody + impulse closing, down from 16.7 ms.
fn steps() {
    println!("== STEPS. Ring of 4 + chain of 12 (16 cars), pushed 1 s, then 40 s free. ==");
    println!("{:<14} {:<12} {:>10} {:>11} {:>11} {:>10} {:>10} {:>10} {:>10}", "step", "joints", "max gap m", "dv centre", "centre off", "dL/L max", "dE/E max", "dE/E end", "us/tick");
    for substeps_total in [3usize, 6, 12, 24, 48] {
        for (mode, name) in [(Mode::AllImpulse, "impulse"), (Mode::MultibodyNative, "multibody")] {
            let mut case = Case::new(mode);
            case.tail = 12;
            case.seconds = 40.0;
            case.settings = Settings { steps_per_tick: substeps_total, substeps: 1, ..game() };
            let r = run(case);
            println!(
                "{:<14} {:<12} {:>10.1e} {:>11.1e} {:>11.1e} {:>10.1e} {:>+10.1e} {:>+10.1e} {:>10.0}",
                format!("{:.2} ms", 50.0 / substeps_total as f64),
                name,
                r.max_gap,
                r.max_velocity_drift,
                r.max_centre_off_line,
                r.max_angular_drift,
                r.max_energy_drift,
                r.final_energy_drift,
                r.micros_per_tick
            );
        }
    }
}

/// Does a closing solve with the true mass matrix remove the extra drift of the prototype that used Rapier's factorization?
fn exact() {
    println!("== EXACT. Ring of 4, pushed 1 s, then 40 s free: the closing joint as Rapier's own impulse joint, as the prototype direct solve, and as the direct solve with the true mass matrix. ==");
    println!("{:<14} {:<34} {:>10} {:>11} {:>11} {:>10} {:>10} {:>10}", "step", "closing", "max gap m", "dv centre", "centre off", "dL/L max", "dE/E max", "us/tick");
    for substeps_total in [12usize, 48] {
        let settings = Settings { steps_per_tick: substeps_total, substeps: 1, ..game() };
        for (mode, exact_mass, iterations, name) in [
            (Mode::AllImpulse, false, 2, "all impulse joints"),
            (Mode::TreeOnly, false, 2, "none (open chain)"),
            (Mode::MultibodyNative, false, 2, "Rapier's impulse joint"),
            (Mode::MultibodyDirect, false, 2, "direct, Rapier's factorization"),
            (Mode::MultibodyDirect, true, 2, "direct, true mass matrix"),
            (Mode::MultibodyDirect, true, 1, "direct, true mass matrix, 1 pass"),
        ] {
            let mut case = Case::new(mode);
            case.seconds = 40.0;
            case.settings = settings;
            case.exact_mass = exact_mass;
            case.iterations = iterations;
            let r = run(case);
            println!(
                "{:<14} {:<34} {:>10.1e} {:>11.1e} {:>11.1e} {:>10.1e} {:>+10.1e} {:>10.0}",
                format!("{:.2} ms", 50.0 / substeps_total as f64),
                name,
                r.max_gap,
                r.max_velocity_drift,
                r.max_centre_off_line,
                r.max_angular_drift,
                r.max_energy_drift,
                r.micros_per_tick
            );
        }
    }
}

fn car_with_collider(world: &mut World, pose: Pose, half: Vector, mass: f64) -> Car {
    let car = add_car(world, pose, half, mass);
    world.colliders.insert_with_parent(ColliderBuilder::cuboid(half.x, half.y, half.z).density(0.0).friction(0.6), car.handle, &mut world.bodies);
    car
}

/// Two cars whose boxes overlap, joined by a ball joint inside the overlap, alone in space and at rest. Anything that moves is free energy.
fn overlap() {
    println!("== OVERLAP. Two cars (3x1x1 m, 1000 kg) at rest in space, joined by a ball joint, their boxes overlapping. 60 s. Nothing pushes them. ==");
    println!("{:<22} {:<26} {:>12} {:>12} {:>12} {:>12}", "overlap", "joint", "max KE J", "KE at end J", "max speed", "max |P| kg m/s");
    for depth in [0.5f64, 0.05, 0.005] {
        for (name, contacts, multibody) in [
            ("impulse, contacts on", true, false),
            ("impulse, contacts off", false, false),
            ("multibody, contacts on", true, true),
            ("multibody, contacts off", false, true),
        ] {
            let mut world = World::new(game(), Vector::ZERO);
            let half = Vector::new(1.5, 0.5, 0.5);
            let a = car_with_collider(&mut world, Pose::from_translation(Vector::ZERO), half, 1000.0);
            let b = car_with_collider(&mut world, Pose::from_translation(Vector::new(3.0 - depth, 0.0, 0.0)), half, 1000.0);
            let anchor_a = Vector::new(1.5 - depth * 0.5, 0.0, 0.0);
            let anchor_b = Vector::new(-1.5 + depth * 0.5, 0.0, 0.0);
            if multibody {
                let frame1 = Pose::from_translation(anchor_a);
                let frame2 = Pose::from_translation(anchor_b);
                let joint = SphericalJointBuilder::new().local_frame1(frame1).local_frame2(frame2).contacts_enabled(contacts).build();
                world.multibody_joints.insert(a.handle, b.handle, joint, true);
            } else {
                let joint = SphericalJointBuilder::new().local_anchor1(anchor_a).local_anchor2(anchor_b).contacts_enabled(contacts).build();
                world.impulse_joints.insert(a.handle, b.handle, joint, true);
            }
            let cars = [a, b];
            let (mut max_ke, mut max_speed, mut max_momentum) = (0.0f64, 0.0f64, 0.0f64);
            for _ in 0..1200 {
                world.tick();
                let t = totals(&world, &cars);
                max_ke = max_ke.max(t.energy);
                max_momentum = max_momentum.max(t.momentum.length());
                max_speed = max_speed.max(cars.iter().map(|c| world.bodies[c.handle].linvel().length()).fold(0.0, f64::max));
            }
            let end = totals(&world, &cars);
            println!("{:<22} {name:<26} {max_ke:>12.3e} {:>12.3e} {max_speed:>12.3e} {max_momentum:>12.3e}", format!("{depth} m"), end.energy);
        }
    }
}

/// A loop built a little inconsistent, so that its joints cannot all be satisfied: does it hold strain, and does the strain turn into motion?
fn strain() {
    println!("== STRAIN. Ring of 4, the closing joint's anchor shifted out of the ring's plane by the mismatch; pushed 1 s, then 40 s free. ==");
    println!("{:<12} {:<28} {:>10} {:>10} {:>11} {:>11} {:>10} {:>10} {:>10}", "mismatch", "joints", "max gap m", "end gap m", "dv centre", "centre off", "dL/L max", "dE/E max", "dE/E end");
    for mismatch in [0.0f64, 0.001, 0.01, 0.1] {
        for (mode, name) in [(Mode::AllImpulse, "all impulse"), (Mode::MultibodyNative, "multibody + impulse closing")] {
            let mut case = Case::new(mode);
            case.seconds = 40.0;
            case.mismatch = Vector::new(0.0, 0.0, mismatch);
            let r = run(case);
            println!(
                "{:<12} {name:<28} {:>10.1e} {:>10.1e} {:>11.1e} {:>11.1e} {:>10.1e} {:>+10.1e} {:>+10.1e}",
                format!("{} mm", mismatch * 1000.0),
                r.max_gap,
                r.final_gap,
                r.max_velocity_drift,
                r.max_centre_off_line,
                r.max_angular_drift,
                r.max_energy_drift,
                r.final_energy_drift
            );
        }
    }
}

/// A straight chain of n cars all joined by fixed joints (what an immobilized or static joint is), spun and moved rigidly, 20 s.
fn fixed() {
    println!("== FIXED. A chain of n cars joined by fixed joints (one rigid thing), moving and spinning freely for 20 s. ==");
    println!("{:<6} {:<12} {:>11} {:>11} {:>10} {:>10} {:>11} {:>10}", "n", "joints", "dv centre", "centre off", "dL/L", "dE/E end", "max gap m", "us/tick");
    for n in [4usize, 16, 60] {
        for (multibody, name) in [(true, "multibody"), (false, "impulse")] {
            let mut world = World::new(game(), Vector::ZERO);
            let half = Vector::new(1.5, 0.5, 0.5);
            let mut cars = Vec::new();
            for k in 0..n {
                cars.push(add_car(&mut world, Pose::from_translation(Vector::new(3.0 * k as f64, 0.0, 0.0)), half, 1000.0));
            }
            let mut specs = Vec::new();
            for k in 0..n - 1 {
                let (end, start) = (Vector::new(1.5, 0.0, 0.0), Vector::new(-1.5, 0.0, 0.0));
                let joint = FixedJointBuilder::new().local_frame1(Pose::from_translation(end)).local_frame2(Pose::from_translation(start)).build();
                if multibody {
                    world.multibody_joints.insert(cars[k].handle, cars[k + 1].handle, joint, true).expect("a fixed joint");
                } else {
                    world.impulse_joints.insert(cars[k].handle, cars[k + 1].handle, joint, true);
                }
                specs.push(LoopSpec { a: cars[k].handle, anchor_a: end, b: cars[k + 1].handle, anchor_b: start });
            }
            start_moving(&mut world, &cars, Vector::new(0.4, 0.2, 0.1), Vector::new(0.0, 0.1, 0.3));
            world.tick();
            let start = totals(&world, &cars);
            world.step_time = Default::default();
            let ticks = 400;
            let (mut worst_dv, mut worst_off, mut worst_l, mut worst_gap) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
            let mut energy_end = 0.0;
            for tick in 0..ticks {
                world.tick();
                let t = totals(&world, &cars);
                let elapsed = (tick + 1) as f64 * world.tick_length;
                worst_dv = worst_dv.max((t.momentum - start.momentum).length() / t.mass);
                worst_off = worst_off.max((t.centre - (start.centre + start.momentum / start.mass * elapsed)).length());
                worst_l = worst_l.max((t.angular_momentum - start.angular_momentum).length() / start.angular_momentum.length());
                worst_gap = worst_gap.max(specs.iter().map(|s| gap(&world, s)).fold(0.0, f64::max));
                energy_end = (t.energy - start.energy) / start.energy;
            }
            println!("{n:<6} {name:<12} {worst_dv:>11.1e} {worst_off:>11.1e} {worst_l:>10.1e} {energy_end:>+10.1e} {worst_gap:>11.1e} {:>10.1}", world.step_time.as_secs_f64() * 1e6 / ticks as f64);
        }
    }
}

/// What do the solver dials buy? A chain of 12 cars on ball impulse joints pulled along its length, the far car light or very heavy. The worst gap at any joint.
fn dials() {
    println!("== DIALS. Chain of 12 cars (1000 kg; the last one as marked) on ball impulse joints, car 0 pulled along the chain by 20 kN for 10 s. Worst joint gap and cost. ==");
    println!("{:<20} {:<44} {:>12} {:>12} {:>10}", "last car", "dials", "worst gap m", "end gap m", "us/tick");
    for last_mass in [1000.0f64, 1.0e6] {
        for (name, settings, extra_iterations, extra_pgs) in [
            ("game: 3 x 4 substeps, pgs 1", game(), 0usize, 0usize),
            ("pgs 4", Settings { pgs_iterations: 4, ..game() }, 0, 0),
            ("pgs 16", Settings { pgs_iterations: 16, ..game() }, 0, 0),
            ("3 x 16 substeps", Settings { substeps: 16, ..game() }, 0, 0),
            ("3 x 4 + per-body 4 extra substeps", game(), 4, 0),
            ("3 x 4 + per-body 4 extra substeps, 4 pgs", game(), 4, 4),
            ("3 x 4 + per-body 16 extra substeps", game(), 16, 0),
        ] {
            let mut world = World::new(settings, Vector::ZERO);
            let half = Vector::new(1.5, 0.5, 0.5);
            let n = 12;
            let mut cars = Vec::new();
            for k in 0..n {
                let mass = if k == n - 1 { last_mass } else { 1000.0 };
                cars.push(add_car(&mut world, Pose::from_translation(Vector::new(3.0 * k as f64, 0.0, 0.0)), half, mass));
            }
            let mut specs = Vec::new();
            for k in 0..n - 1 {
                let (end, start) = (Vector::new(1.5, 0.0, 0.0), Vector::new(-1.5, 0.0, 0.0));
                world.impulse_joints.insert(cars[k].handle, cars[k + 1].handle, SphericalJointBuilder::new().local_anchor1(end).local_anchor2(start).build(), true);
                specs.push(LoopSpec { a: cars[k].handle, anchor_a: end, b: cars[k + 1].handle, anchor_b: start });
            }
            for car in &cars {
                world.bodies[car.handle].set_additional_solver_iterations(extra_iterations);
                world.bodies[car.handle].set_additional_pgs_iterations(extra_pgs);
            }
            let ticks = 200;
            let started = Instant::now();
            let mut worst = 0.0f64;
            for _ in 0..ticks {
                world.bodies[cars[0].handle].add_force(Vector::new(20000.0, 0.0, 0.0), true);
                world.tick();
                worst = worst.max(specs.iter().map(|s| gap(&world, s)).fold(0.0, f64::max));
            }
            let end_gap = specs.iter().map(|s| gap(&world, s)).fold(0.0, f64::max);
            println!("{:<20} {name:<44} {worst:>12.2e} {end_gap:>12.2e} {:>10.1}", if last_mass > 1.0e3 { "1,000,000 kg" } else { "1,000 kg" }, started.elapsed().as_secs_f64() * 1e6 / ticks as f64);
        }
    }
}
