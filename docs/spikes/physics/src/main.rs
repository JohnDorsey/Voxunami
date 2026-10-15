// Throwaway physics spike: can Rapier carry Voxunami's rigid voxel structures?
use rapier3d::prelude::*;
use std::time::Instant;

mod more;
mod world;
use more::*;
use world::*;

fn main() {
    let which = std::env::args().nth(1).unwrap_or_else(|| "all".to_owned());
    let run = |name: &str| which == "all" || which == name;
    if run("determinism") {
        determinism();
    }
    if run("scale") {
        scale();
    }
    if run("perf") {
        perf();
    }
    if run("joints") {
        joints();
    }
    if run("buoyancy") {
        buoyancy();
    }
    if run("origin") {
        origin();
    }
    if run("seams") {
        seams();
    }
    if run("collisions") {
        collisions();
    }
    if run("misc") {
        misc();
    }
}

fn flat_ground(width: i32, depth: i32, height: i32) -> Vec<IVector> {
    box_voxels(width, height, depth)
}

struct RestReport {
    sink_voxels: f32,
    tilt_degrees: f32,
    speed_voxels_per_second: f32,
    jitter_voxels: f32,
    sleeping: bool,
}

/// A 24x8x12 voxel ship dropped 4 voxels onto flat ground; measures how well it comes to rest.
fn rest_on_ground(voxel: f32, units_per_metre: f32, offset: f32, seconds: f32) -> RestReport {
    let mut world = World::with_units(voxel, units_per_metre);
    let ground_height = 6;
    let origin = Vector::new(offset, offset, offset);
    world.add_static_voxels(voxel, &flat_ground(60, 60, ground_height), origin);
    let top = ground_height as f32 * voxel;
    let ship = box_voxels(24, 8, 12);
    let body = world.add_dynamic_voxels(
        voxel,
        &ship,
        origin + Vector::new(15.0 * voxel, top + 4.0 * voxel, 24.0 * voxel),
        1000.0,
    );
    let steps = (seconds * 60.0) as usize;
    let mut late_heights = Vec::new();
    let mut speed = 0.0;
    for step in 0..steps {
        world.step();
        if step + 120 >= steps {
            late_heights.push(world.bodies[body].translation().y);
            speed = world.bodies[body].linvel().length();
        }
    }
    let body_ref = &world.bodies[body];
    let (min, max) = late_heights
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), &h| (lo.min(h), hi.max(h)));
    let up = body_ref.rotation() * Vector::Y;
    RestReport {
        sink_voxels: (origin.y + top - body_ref.translation().y) / voxel,
        tilt_degrees: up.dot(Vector::Y).clamp(-1.0, 1.0).acos().to_degrees(),
        speed_voxels_per_second: speed / voxel,
        jitter_voxels: (max - min) / voxel,
        sleeping: body_ref.is_sleeping(),
    }
}

fn scale() {
    println!("== scale: a 24x8x12 voxel ship dropped 4 voxels onto flat ground, 10 s ==");
    let cases = [
        ("1 m voxels, metres", 1.0, 1.0),
        ("1/32 m voxels, metres (length_unit 1)", 1.0 / 32.0, 1.0),
    ];
    for (label, voxel, units) in cases {
        let report = rest_on_ground(voxel, units, 0.0, 10.0);
        println!(
            "{label:<44} sink {:+.3} voxels, tilt {:.3} deg, speed {:.4} voxels/s, jitter {:.4} voxels, asleep {}",
            report.sink_voxels, report.tilt_degrees, report.speed_voxels_per_second, report.jitter_voxels, report.sleeping
        );
    }
    // The same world measured in voxels: one unit is one voxel, so a metre is 32 units.
    let mut world = World::with_units(1.0, 32.0);
    world.add_static_voxels(1.0, &flat_ground(60, 60, 6), Vector::ZERO);
    let body = world.add_dynamic_voxels(1.0, &box_voxels(24, 8, 12), Vector::new(15.0, 10.0, 24.0), 1000.0 / 32768.0);
    for _ in 0..600 {
        world.step();
    }
    println!(
        "{:<44} sink {:+.3} voxels, speed {:.4} voxels/s, asleep {} (units are voxels, g = {:.0} voxels/s^2)",
        "1/32 m voxels, units = voxels (length_unit 32)",
        6.0 - world.bodies[body].translation().y,
        world.bodies[body].linvel().length(),
        world.bodies[body].is_sleeping(),
        -world.gravity.y
    );
}

fn origin() {
    println!("== distance from the origin: same ship, whole scene translated (f32 positions) ==");
    for (voxel, offsets) in [(1.0f32, vec![0.0f32, 1.0e3, 1.0e4, 1.0e5, 1.0e6]), (1.0 / 32.0, vec![0.0, 1.0e2, 1.0e3, 1.0e4])] {
        for offset in offsets {
            let report = rest_on_ground(voxel, 1.0, offset, 10.0);
            println!(
                "voxel {voxel:.4} m, offset {offset:>9.0} m: sink {:+.3} voxels, jitter {:.4} voxels, asleep {}",
                report.sink_voxels, report.jitter_voxels, report.sleeping
            );
        }
    }
}

/// Mixed scene: voxel terrain, a voxel ship, a hinged leg with a motor, and some dropped cubes.
fn determinism_scene(world: &mut World, voxel: f32) {
    let ground = terrain_voxels(40, 40, 6);
    world.add_static_voxels(voxel, &ground, Vector::new(0.0, 0.0, 0.0));
    let ship = box_voxels(20, 6, 8);
    let ship_body = world.add_dynamic_voxels(voxel, &ship, Vector::new(10.0 * voxel, 20.0 * voxel, 12.0 * voxel), 1.0);
    // A leg hinged to the ship, driven by a motor.
    let leg = box_voxels(2, 8, 2);
    let leg_body = world.add_dynamic_voxels(voxel, &leg, Vector::new(10.0 * voxel, 20.0 * voxel, 8.0 * voxel), 1.0);
    let joint = RevoluteJointBuilder::new(Vector::X)
        .local_anchor1(Vector::new(0.0, -3.0 * voxel, -4.0 * voxel))
        .local_anchor2(Vector::new(0.0, 4.0 * voxel, 0.0))
        .motor_velocity(2.0, 1000.0 * voxel);
    world.impulse_joints.insert(ship_body, leg_body, joint, true);
    for i in 0..6 {
        let cube = box_voxels(3, 3, 3);
        world.add_dynamic_voxels(
            voxel,
            &cube,
            Vector::new((22.0 + i as f32 * 1.3) * voxel, (30.0 + i as f32 * 5.0) * voxel, (20.0 + i as f32 * 0.7) * voxel),
            1.0,
        );
    }
}

fn determinism() {
    println!("== determinism: platform {} ==", std::env::consts::OS);
    for (label, voxel) in [("1 m voxels", 1.0f32), ("1/32 m voxels", 1.0 / 32.0)] {
        let mut world = World::new(voxel);
        determinism_scene(&mut world, voxel);
        let mut hashes = Vec::new();
        for step in 1..=600 {
            world.step();
            if step % 100 == 0 {
                hashes.push(world.state_hash());
            }
        }
        println!("{label}: {}", hashes.iter().map(|h| format!("{h:016x}")).collect::<Vec<_>>().join(" "));
        let _ = Instant::now();
    }
}

fn hilly(width: i32, depth: i32) -> Vec<IVector> {
    let mut voxels = Vec::new();
    for x in 0..width {
        for z in 0..depth {
            let height = 8 + (x.wrapping_mul(7919) ^ z.wrapping_mul(104_729)).rem_euclid(3);
            for y in 0..=height {
                voxels.push(IVector::new(x, y, z));
            }
        }
    }
    voxels
}

fn milliseconds(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

fn perf() {
    println!("== performance (release, single thread, 1 m voxels, 60 Hz steps) ==");
    for (width, depth) in [(128, 128), (256, 256), (512, 512)] {
        let voxels = hilly(width, depth);
        let start = Instant::now();
        let collider = ColliderBuilder::voxels(Vector::ONE, &voxels).build();
        let build = milliseconds(start);
        let memory = collider.shape().as_voxels().map_or(0, |v| v.total_memory_size());
        println!(
            "terrain collider {width}x{depth}: {} voxels built in {build:.0} ms, {:.1} MiB ({:.2} bytes/voxel)",
            voxels.len(),
            memory as f64 / 1048576.0,
            memory as f64 / voxels.len() as f64
        );
    }

    let terrain = hilly(256, 256);
    for ships in [1usize, 10, 100] {
        let mut world = World::new(1.0);
        world.add_static_voxels(1.0, &terrain, Vector::ZERO);
        for i in 0..ships {
            let x = 10.0 + (i % 10) as f32 * 30.0;
            let z = 10.0 + (i / 10) as f32 * 20.0;
            world.add_dynamic_voxels(1.0, &box_voxels(24, 8, 12), Vector::new(x, 14.0, z), 1000.0);
        }
        let mut landing = 0.0;
        let mut resting = 0.0;
        for step in 0..300 {
            let start = Instant::now();
            world.step();
            let ms = milliseconds(start);
            if step < 60 {
                landing += ms / 60.0;
            } else if step >= 180 {
                resting += ms / 120.0;
            }
        }
        let asleep = world.bodies.iter().filter(|(_, b)| b.is_dynamic() && b.is_sleeping()).count();
        println!("{ships:>3} ships (2304 voxels each) on 256x256 terrain: landing {landing:.2} ms/step, settled {resting:.2} ms/step, {asleep} asleep");
    }

    println!("-- the same ships kept awake (as moving structures would be) --");
    for ships in [1usize, 10, 100] {
        let mut world = World::new(1.0);
        world.never_sleep = true;
        world.add_static_voxels(1.0, &terrain, Vector::ZERO);
        for i in 0..ships {
            let x = 10.0 + (i % 10) as f32 * 30.0;
            let z = 10.0 + (i / 10) as f32 * 20.0;
            world.add_dynamic_voxels(1.0, &box_voxels(24, 8, 12), Vector::new(x, 14.0, z), 1000.0);
        }
        let mut resting = 0.0;
        for step in 0..300 {
            let start = Instant::now();
            world.step();
            if step >= 180 {
                resting += milliseconds(start) / 120.0;
            }
        }
        println!("{ships:>3} awake ships resting on terrain: {resting:.3} ms/step");
    }
    let mut world = World::new(1.0);
    world.never_sleep = true;
    world.add_static_voxels(1.0, &terrain, Vector::ZERO);
    world.add_dynamic_voxels(1.0, &box_voxels(100, 30, 40), Vector::new(60.0, 14.0, 100.0), 1000.0);
    let mut resting = 0.0;
    for step in 0..300 {
        let start = Instant::now();
        world.step();
        if step >= 180 {
            resting += milliseconds(start) / 120.0;
        }
    }
    println!("one awake 100x30x40 ship (120000 voxels) resting on terrain: {resting:.3} ms/step");

    let mut world = World::new(1.0);
    world.add_static_voxels(1.0, &terrain, Vector::ZERO);
    world.add_dynamic_voxels(1.0, &box_voxels(100, 30, 40), Vector::new(60.0, 14.0, 100.0), 1000.0);
    let mut landing = 0.0;
    let mut resting = 0.0;
    for step in 0..300 {
        let start = Instant::now();
        world.step();
        let ms = milliseconds(start);
        if step < 60 {
            landing += ms / 60.0;
        } else if step >= 180 {
            resting += ms / 120.0;
        }
    }
    println!("one 100x30x40 ship (120000 voxels): landing {landing:.2} ms/step, settled {resting:.2} ms/step");

    // Edits: dig under a resting ship and watch it respond.
    let mut world = World::new(1.0);
    let terrain_body = world.add_static_voxels(1.0, &flat_ground(60, 60, 6), Vector::ZERO);
    let ship = world.add_dynamic_voxels(1.0, &box_voxels(12, 4, 12), Vector::new(20.0, 6.0, 20.0), 1000.0);
    for _ in 0..180 {
        world.step();
    }
    let before = world.bodies[ship].translation().y;
    let asleep_before = world.bodies[ship].is_sleeping();
    let terrain_collider = world.bodies[terrain_body].colliders()[0];
    let start = Instant::now();
    let mut edits = 0;
    for x in 18..34 {
        for z in 18..34 {
            for y in 0..6 {
                let voxels = world.colliders[terrain_collider].shape_mut().as_voxels_mut().expect("a voxels shape");
                voxels.set_voxel(IVector::new(x, y, z), false);
                edits += 1;
            }
        }
    }
    let edit_ms = milliseconds(start);
    for _ in 0..120 {
        world.step();
    }
    println!(
        "dug a 16x16x6 pit under a resting ship: {edits} voxel edits in {edit_ms:.2} ms; ship was asleep {asleep_before}, fell from y {before:.2} to {:.2}",
        world.bodies[ship].translation().y
    );

    // Editing a dynamic body's own voxels changes its mass only if we tell it to.
    let mut world = World::new(1.0);
    let body = world.add_dynamic_voxels(1.0, &box_voxels(10, 10, 10), Vector::new(0.0, 50.0, 0.0), 1.0);
    let before_mass = world.bodies[body].mass();
    let collider = world.bodies[body].colliders()[0];
    for x in 0..5 {
        for y in 0..10 {
            for z in 0..10 {
                world.colliders[collider].shape_mut().as_voxels_mut().unwrap().set_voxel(IVector::new(x, y, z), false);
            }
        }
    }
    world.step();
    let stale_mass = world.bodies[body].mass();
    world.bodies[body].recompute_mass_properties_from_colliders(&world.colliders);
    println!(
        "removing half a body's voxels: mass {before_mass:.0} -> {stale_mass:.0} after a step -> {:.0} after recompute_mass_properties_from_colliders",
        world.bodies[body].mass()
    );
}

fn collisions() {
    println!("== collisions between voxel structures, and arbitrary rotation (1 m voxels) ==");
    for (label, size) in [("24x8x12 ships", (24, 8, 12)), ("100x30x40 ships", (100, 30, 40))] {
        let mut world = World::new(1.0);
        world.gravity = Vector::ZERO;
        let (x, y, z) = size;
        let voxels = box_voxels(x, y, z);
        let left = world.add_dynamic_voxels(1.0, &voxels, Vector::new(0.0, 0.0, 0.0), 1000.0);
        let right = world.add_dynamic_voxels(1.0, &voxels, Vector::new(x as f32 + 6.0, 2.5, 3.0), 1000.0);
        world.bodies[left].set_linvel(Vector::new(8.0, 0.0, 0.0), true);
        world.bodies[right].set_linvel(Vector::new(-8.0, 0.0, 0.0), true);
        let mut worst = 0.0f64;
        let mut total = 0.0f64;
        for _ in 0..240 {
            let start = Instant::now();
            world.step();
            let ms = milliseconds(start);
            worst = worst.max(ms);
            total += ms;
        }
        let gap = world.bodies[right].translation().x - world.bodies[left].translation().x - x as f32;
        println!(
            "two {label} ramming at 16 m/s: worst step {worst:.2} ms, average {:.3} ms, final gap {gap:+.2} m (negative would mean interpenetration)",
            total / 240.0
        );
    }

    // Arbitrary orientation: a ship rotated off every axis lands on terrain.
    let mut world = World::new(1.0);
    world.add_static_voxels(1.0, &flat_ground(60, 60, 6), Vector::ZERO);
    let rotation = Rotation::from_axis_angle(Vector::new(1.0, 2.0, 3.0).normalize(), 0.9);
    let ship = world.bodies.insert(RigidBodyBuilder::dynamic().translation(Vector::new(20.0, 16.0, 20.0)).rotation(rotation.to_scaled_axis()));
    world.colliders.insert_with_parent(ColliderBuilder::voxels(Vector::ONE, &box_voxels(20, 6, 10)).density(1000.0).friction(0.8), ship, &mut world.bodies);
    let mut worst_penetration = 0.0f32;
    for _ in 0..600 {
        world.step();
        // The lowest corner of the hull can never be below the ground top (y = 6) by more than a skin.
        let (t, r) = (world.bodies[ship].translation(), *world.bodies[ship].rotation());
        for corner in [(0.0, 0.0, 0.0), (20.0, 0.0, 0.0), (0.0, 0.0, 10.0), (20.0, 0.0, 10.0), (0.0, 6.0, 0.0), (20.0, 6.0, 10.0), (0.0, 6.0, 10.0), (20.0, 6.0, 0.0)] {
            let p = t + r * Vector::new(corner.0, corner.1, corner.2);
            worst_penetration = worst_penetration.max(6.0 - p.y);
        }
    }
    let up = world.bodies[ship].rotation() * Vector::Y;
    println!(
        "tilted ship (0.9 rad about (1,2,3)) dropped on flat ground: ends with tilt {:.1} deg, asleep {}, deepest hull corner {:.3} m below the surface",
        up.dot(Vector::Y).clamp(-1.0, 1.0).acos().to_degrees(),
        world.bodies[ship].is_sleeping(),
        worst_penetration
    );
}

/// Slides a box across flat ground, once as one collider and once as tiles with seams between them.
fn seams() {
    println!("== seams: a box sliding at 40 m/s over flat ground, straddling tile seams (1 m voxels) ==");
    for (label, tile) in [("one collider", 0), ("tiles of 32 voxels", 32), ("tiles of 8 voxels", 8)] {
        let mut world = World::new(1.0);
        let extent = 128;
        let ground_height = 4;
        if tile == 0 {
            world.add_static_voxels(1.0, &flat_ground(extent, extent, ground_height), Vector::ZERO);
        } else {
            for tx in 0..extent / tile {
                for tz in 0..extent / tile {
                    world.add_static_voxels(
                        1.0,
                        &flat_ground(tile, tile, ground_height),
                        Vector::new((tx * tile) as f32, 0.0, (tz * tile) as f32),
                    );
                }
            }
        }
        let body = world.bodies.insert(RigidBodyBuilder::dynamic().translation(Vector::new(4.0, ground_height as f32, 61.0)));
        world.colliders.insert_with_parent(
            ColliderBuilder::voxels(Vector::ONE, &box_voxels(6, 3, 6)).density(1000.0).friction(0.2),
            body,
            &mut world.bodies,
        );
        for _ in 0..30 {
            world.step(); // settle
        }
        world.bodies[body].set_linvel(Vector::new(40.0, 0.0, 0.0), true);
        let mut slowest = f32::MAX;
        let mut crossed = 0.0f32;
        let mut highest_hop = 0.0f32;
        let mut sideways = 0.0f32;
        for _ in 0..600 {
            world.step();
            let v = world.bodies[body].linvel();
            if world.bodies[body].translation().x < 110.0 {
                slowest = slowest.min(v.x);
                crossed = crossed.max(world.bodies[body].translation().x);
            }
            highest_hop = highest_hop.max(world.bodies[body].translation().y - ground_height as f32);
            sideways = sideways.max(v.z.abs());
        }
        let t = world.bodies[body].translation();
        println!(
            "{label:<20} slowest speed {slowest:.3} m/s, highest hop {highest_hop:.4} m, max sideways speed {sideways:.4} m/s, travelled to x = {crossed:.1} (ended at {:.1})",
            t.x
        );
    }
}
