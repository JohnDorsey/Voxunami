use rapier3d::prelude::*;
use std::time::Instant;

use crate::world::*;
use crate::{flat_ground, milliseconds};

fn world_point(world: &World, body: RigidBodyHandle, local: Vector) -> Vector {
    let body = &world.bodies[body];
    body.translation() + body.rotation() * local
}

pub fn joints() {
    println!("== joints (1 m voxels) ==");
    // A light vehicle on a turntable (or welded) on top of a heavy ship resting on the ground.
    for (label, fixed) in [("revolute turntable with motor", false), ("fixed joint", true)] {
        let mut world = World::new(1.0);
        world.add_static_voxels(1.0, &flat_ground(80, 80, 6), Vector::ZERO);
        let ship = world.add_dynamic_voxels(1.0, &box_voxels(24, 8, 12), Vector::new(20.0, 6.0, 30.0), 1000.0);
        let vehicle = world.add_dynamic_voxels(1.0, &box_voxels(2, 2, 2), Vector::new(30.0, 14.0, 34.0), 500.0);
        let (anchor_ship, anchor_vehicle) = (Vector::new(11.0, 8.0, 5.0), Vector::new(1.0, 0.0, 1.0));
        if fixed {
            let joint = FixedJointBuilder::new().local_anchor1(anchor_ship).local_anchor2(anchor_vehicle);
            world.impulse_joints.insert(ship, vehicle, joint, true);
        } else {
            let joint = RevoluteJointBuilder::new(Vector::Y)
                .local_anchor1(anchor_ship)
                .local_anchor2(anchor_vehicle)
                .motor_velocity(1.0, 1.0e5);
            world.impulse_joints.insert(ship, vehicle, joint, true);
        }
        let ratio = world.bodies[ship].mass() / world.bodies[vehicle].mass();
        let ship_start = world.bodies[ship].translation();
        let mut worst: f32 = 0.0;
        for _ in 0..600 {
            world.step();
            let error = (world_point(&world, ship, anchor_ship) - world_point(&world, vehicle, anchor_vehicle)).length();
            worst = worst.max(error);
        }
        let spin = world.bodies[vehicle].angvel().y;
        println!(
            "{label}: mass ratio {ratio:.0}:1, worst anchor separation {worst:.4} m, ship drifted {:.4} m, vehicle spin {spin:.2} rad/s",
            (world.bodies[ship].translation() - ship_start).length()
        );
    }

    // A hinge motor holding a heavy ship at a target angle against gravity.
    for stiffness in [1.0e6f32, 1.0e8, 1.0e10] {
        let mut world = World::new(1.0);
        let base = world.bodies.insert(RigidBodyBuilder::fixed().translation(Vector::new(0.0, 30.0, 0.0)));
        let ship = world.add_dynamic_voxels(1.0, &box_voxels(24, 4, 6), Vector::new(0.0, 28.0, 0.0), 1000.0);
        let joint = RevoluteJointBuilder::new(Vector::Z)
            .local_anchor1(Vector::ZERO)
            .local_anchor2(Vector::new(0.0, 2.0, 3.0))
            .motor_position(0.5, stiffness, stiffness * 0.1);
        world.impulse_joints.insert(base, ship, joint, true);
        for _ in 0..600 {
            world.step();
        }
        let q = world.bodies[ship].rotation();
        let angle = 2.0 * q.z.atan2(q.w);
        println!(
            "hinge holding {:.1e} kg to angle 0.5 rad, stiffness {stiffness:.0e}: reached {angle:.3} rad",
            world.bodies[ship].mass()
        );
    }

    // A swinging chain: impulse joints versus multibody (articulated) joints.
    for multibody in [false, true] {
        let mut world = World::new(1.0);
        let base = world.bodies.insert(RigidBodyBuilder::fixed().translation(Vector::new(10.0, 40.0, 10.0)));
        let mut previous = base;
        let mut previous_anchor = Vector::ZERO;
        let mut links = Vec::new();
        let mut top = Vector::new(10.0, 40.0, 10.0);
        for i in 0..6 {
            let top_anchor = Vector::new(0.5, 6.0, 0.5);
            let link = world.add_dynamic_voxels(1.0, &box_voxels(1, 6, 1), top - top_anchor, 1000.0);
            let joint = RevoluteJointBuilder::new(Vector::Z).local_anchor1(previous_anchor).local_anchor2(top_anchor);
            if multibody {
                world.multibody_joints.insert(previous, link, joint, true);
            } else {
                world.impulse_joints.insert(previous, link, joint, true);
            }
            links.push((previous, previous_anchor, link, top_anchor));
            previous = link;
            previous_anchor = Vector::new(0.5, 0.0, 0.5);
            top -= Vector::new(0.0, 6.0, 0.0);
            if i == 0 {
                world.bodies[link].set_angvel(Vector::new(0.0, 0.0, 2.0), true);
            }
        }
        let mut worst: f32 = 0.0;
        for _ in 0..600 {
            world.step();
            for &(a, anchor_a, b, anchor_b) in &links {
                let error = (world_point(&world, a, anchor_a) - world_point(&world, b, anchor_b)).length();
                worst = worst.max(error);
            }
        }
        println!(
            "6-link chain, {}: worst joint separation {worst:.4} m",
            if multibody { "multibody joints" } else { "impulse joints" }
        );
    }
}

pub fn buoyancy() {
    println!("== buoyancy: custom per-voxel force from submerged voxel centres (water surface at y = 0) ==");
    for smooth in [false, true] {
        for (voxel, label) in [(1.0f32, "1 m voxels"), (1.0 / 32.0, "1/32 m voxels")] {
            let mut world = World::new(voxel);
            let voxels = box_voxels(24, 8, 12);
            let body = world.bodies.insert(
                RigidBodyBuilder::dynamic().translation(Vector::new(0.0, 3.0 * voxel, 0.0)).linear_damping(1.5).angular_damping(2.0),
            );
            let collider = ColliderBuilder::voxels(Vector::splat(voxel), &voxels).density(500.0);
            world.colliders.insert_with_parent(collider, body, &mut world.bodies);
            let centres: Vec<Vector> = voxels
                .iter()
                .map(|k| Vector::new(k.x as f32 + 0.5, k.y as f32 + 0.5, k.z as f32 + 0.5) * voxel)
                .collect();
            let voxel_force = 1000.0 * 9.81 * voxel * voxel * voxel;
            let mut force_ms = 0.0;
            let mut worst_tilt = 0.0f32;
            for _ in 0..1800 {
                let start = Instant::now();
                world.bodies[body].reset_forces(false);
                world.bodies[body].reset_torques(false);
                let (translation, rotation) = (world.bodies[body].translation(), *world.bodies[body].rotation());
                for centre in &centres {
                    let point = translation + rotation * *centre;
                    // Fraction of the voxel below the surface: all-or-nothing, or ramped over one voxel of depth.
                    let submerged = if smooth { (0.5 - point.y / voxel).clamp(0.0, 1.0) } else if point.y < 0.0 { 1.0 } else { 0.0 };
                    if submerged > 0.0 {
                        world.bodies[body].add_force_at_point(Vector::new(0.0, voxel_force * submerged, 0.0), point, true);
                    }
                }
                force_ms += milliseconds(start);
                world.step();
                let up = world.bodies[body].rotation() * Vector::Y;
                worst_tilt = worst_tilt.max(up.dot(Vector::Y).clamp(-1.0, 1.0).acos().to_degrees());
            }
            let up = world.bodies[body].rotation() * Vector::Y;
            println!(
                "{} {label}: draft {:.2} voxels (expected 4.00), final tilt {:.2} deg, worst tilt {worst_tilt:.1} deg, {:.3} ms/step for {} voxels",
                if smooth { "smooth   " } else { "all-or-none" },
                -world.bodies[body].translation().y / voxel,
                up.dot(Vector::Y).clamp(-1.0, 1.0).acos().to_degrees(),
                force_ms / 1800.0,
                centres.len()
            );
        }
    }
}

pub fn misc() {
    println!("== mass properties, per-body gravity, splitting ==");
    let mut world = World::new(1.0);
    let uniform = world.add_dynamic_voxels(1.0, &box_voxels(10, 10, 10), Vector::new(0.0, 50.0, 0.0), 1.0);
    println!(
        "uniform density 1.0, 10x10x10 voxels: mass {:.0} (expected 1000), centre of mass {:?}",
        world.bodies[uniform].mass(),
        world.bodies[uniform].center_of_mass()
    );
    let custom = world.bodies.insert(
        RigidBodyBuilder::dynamic()
            .translation(Vector::new(30.0, 50.0, 0.0))
            .additional_mass_properties(MassProperties::new(Vector::new(2.0, 3.0, 4.0), 5000.0, Vector::splat(1000.0))),
    );
    world.colliders.insert_with_parent(
        ColliderBuilder::voxels(Vector::ONE, &box_voxels(10, 10, 10)).density(0.0),
        custom,
        &mut world.bodies,
    );
    println!(
        "additional_mass_properties, before a step: mass {:.0}, centre of mass {:?}",
        world.bodies[custom].mass(),
        world.bodies[custom].center_of_mass()
    );
    world.step();
    println!(
        "additional_mass_properties, after a step: mass {:.0}, centre of mass {:?}",
        world.bodies[custom].mass(),
        world.bodies[custom].center_of_mass()
    );
    world.bodies[custom].recompute_mass_properties_from_colliders(&world.colliders);
    println!(
        "additional_mass_properties, after recompute: mass {:.0}, centre of mass {:?}",
        world.bodies[custom].mass(),
        world.bodies[custom].center_of_mass()
    );
    let explicit = world.bodies.insert(RigidBodyBuilder::dynamic().translation(Vector::new(60.0, 50.0, 0.0)));
    world.colliders.insert_with_parent(
        ColliderBuilder::voxels(Vector::ONE, &box_voxels(10, 10, 10)).mass_properties(MassProperties::new(Vector::new(2.0, 3.0, 4.0), 5000.0, Vector::splat(1000.0))),
        explicit,
        &mut world.bodies,
    );
    println!(
        "collider mass_properties(...) with our own values: mass {:.0}, centre of mass {:?} (expected 5000 and the translation + (2,3,4))",
        world.bodies[explicit].mass(),
        world.bodies[explicit].center_of_mass()
    );

    // Gravity that points anywhere, applied as a force on each body.
    let mut world = World::new(1.0);
    world.gravity = Vector::ZERO;
    let body = world.add_dynamic_voxels(1.0, &box_voxels(4, 4, 4), Vector::new(0.0, 0.0, 0.0), 1000.0);
    let gravity = Vector::new(3.0, -4.0, 0.0) * 9.81 / 5.0;
    for _ in 0..60 {
        let mass = world.bodies[body].mass();
        world.bodies[body].reset_forces(false);
                world.bodies[body].reset_torques(false);
        world.bodies[body].add_force(gravity * mass, true);
        world.step();
    }
    println!(
        "per-body gravity (3,-4,0)/5 * g for 1 s: velocity {:?} (expected {:?})",
        world.bodies[body].linvel(),
        gravity
    );

    // Splitting: how long does it take to find disconnected pieces of a big structure?
    let (width, height, depth) = (100usize, 30usize, 40usize);
    let solid: Vec<bool> = (0..width * height * depth)
        .map(|i| {
            let (x, y) = (i % width, i / width % height);
            !(x == 50 && y > 5) // a slab removed down the middle splits the ship in two above y = 5
        })
        .collect();
    let start = Instant::now();
    let mut label = vec![0u32; solid.len()];
    let mut components = 0;
    let mut stack = Vec::new();
    for seed in 0..solid.len() {
        if !solid[seed] || label[seed] != 0 {
            continue;
        }
        components += 1;
        label[seed] = components;
        stack.push(seed);
        while let Some(i) = stack.pop() {
            let (x, y, z) = (i % width, i / width % height, i / (width * height));
            let mut visit = |nx: usize, ny: usize, nz: usize| {
                let j = nx + ny * width + nz * width * height;
                if solid[j] && label[j] == 0 {
                    label[j] = components;
                    stack.push(j);
                }
            };
            if x > 0 {
                visit(x - 1, y, z);
            }
            if x + 1 < width {
                visit(x + 1, y, z);
            }
            if y > 0 {
                visit(x, y - 1, z);
            }
            if y + 1 < height {
                visit(x, y + 1, z);
            }
            if z > 0 {
                visit(x, y, z - 1);
            }
            if z + 1 < depth {
                visit(x, y, z + 1);
            }
        }
    }
    println!(
        "connected components of a {}-voxel structure: {} found in {:.2} ms",
        solid.iter().filter(|&&s| s).count(),
        components,
        milliseconds(start)
    );
}
