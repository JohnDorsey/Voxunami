use rapier3d::prelude::*;

fn run(iterations: usize, pgs: usize, mismatch: f32, steps: usize) -> (Vec<f32>, f32) {
    let mut parameters = IntegrationParameters::default();
    parameters.dt = 0.05 / 4.0;
    parameters.num_solver_iterations = iterations;
    parameters.num_internal_pgs_iterations = pgs;
    let (mut pipeline, mut islands, mut broad, mut narrow) = (PhysicsPipeline::new(), IslandManager::new(), BroadPhaseBvh::new(), NarrowPhase::new());
    let (mut bodies, mut colliders, mut impulse, mut multibody, mut soft, mut ccd) =
        (RigidBodySet::new(), ColliderSet::new(), ImpulseJointSet::new(), MultibodyJointSet::new(), SoftBodySet::new(), CCDSolver::new());
    let a = bodies.insert(RigidBodyBuilder::fixed());
    colliders.insert_with_parent(ColliderBuilder::cuboid(1.0, 1.0, 1.0).density(100.0).sensor(true), a, &mut bodies);
    let b = bodies.insert(RigidBodyBuilder::dynamic().translation(Vector::new(3.0, 0.0, 0.0)).can_sleep(false));
    colliders.insert_with_parent(ColliderBuilder::cuboid(1.0, 1.0, 1.0).density(100.0).sensor(true), b, &mut bodies);
    for (pa, pb) in [(Vector::new(1.5, -2.0, 0.0), Vector::new(-1.5, -2.0, 0.0)), (Vector::new(1.5, 2.0, 0.0), Vector::new(-1.5, 2.0 + mismatch, 0.0))] {
        impulse.insert(a, b, SphericalJointBuilder::new().local_anchor1(pa).local_anchor2(pb).build(), true);
    }
    bodies[b].set_angvel(Vector::new(0.0, 2.0, 0.0), true);
    let mut series = Vec::new();
    let mut worst = 0.0f32;
    for _ in 0..steps {
        pipeline.step(Vector::ZERO, &parameters, &mut islands, &mut broad, &mut narrow, &mut bodies, &mut colliders, &mut impulse, &mut multibody, &mut soft, &mut ccd, &(), &());
        series.push(bodies[b].linvel().length() + bodies[b].angvel().length());
        for (pa, pb) in [(Vector::new(1.5, -2.0, 0.0), Vector::new(-1.5, -2.0, 0.0)), (Vector::new(1.5, 2.0, 0.0), Vector::new(-1.5, 2.0 + mismatch, 0.0))] {
            worst = worst.max((bodies[a].position() * pa - bodies[b].position() * pb).length());
        }
    }
    (series, worst)
}

fn main() {
    println!("speed = |linear velocity| + |angular velocity| of the swinging car; true value 1.14286 (angular momentum conserved)");
    println!("substeps = Rapier num_solver_iterations (divides dt); pgs = num_internal_pgs_iterations (solver passes per substep)");
    for (mismatch, label) in [(0.0f32, "consistent"), (0.001, "1 mm mismatch")] {
        println!("-- {label} --");
        for (sub, pgs) in [(4usize, 1usize), (16, 1), (64, 1), (256, 1), (4, 4), (4, 16), (4, 64), (16, 4), (16, 16)] {
            let (s, worst) = run(sub, pgs, mismatch, 2400);
            let tail = &s[2000..];
            let mean = tail.iter().sum::<f32>() / tail.len() as f32;
            let step_rms = (tail.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum::<f32>() / (tail.len() - 1) as f32).sqrt();
            println!("  substeps={sub:<4} pgs={pgs:<3} speed error {:+.5} ({:+.2} %)  step-to-step rms {step_rms:.7}  worst gap {worst:.6} m", mean - 1.14286, 100.0 * (mean - 1.14286) / 1.14286);
        }
    }
}
