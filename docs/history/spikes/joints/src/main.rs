use rapier3d::prelude::*;

struct W {
    pipeline: PhysicsPipeline, parameters: IntegrationParameters, islands: IslandManager,
    broad: BroadPhaseBvh, narrow: NarrowPhase, bodies: RigidBodySet, colliders: ColliderSet,
    impulse: ImpulseJointSet, multibody: MultibodyJointSet, soft: SoftBodySet, ccd: CCDSolver, gravity: Vector,
}
impl W {
    fn new(iterations: usize, gravity: f32) -> Self {
        let mut parameters = IntegrationParameters::default();
        parameters.dt = 0.05 / 4.0;
        parameters.num_solver_iterations = iterations;
        Self { pipeline: PhysicsPipeline::new(), parameters, islands: IslandManager::new(), broad: BroadPhaseBvh::new(),
            narrow: NarrowPhase::new(), bodies: RigidBodySet::new(), colliders: ColliderSet::new(), impulse: ImpulseJointSet::new(),
            multibody: MultibodyJointSet::new(), soft: SoftBodySet::new(), ccd: CCDSolver::new(), gravity: Vector::new(0.0, -gravity, 0.0) }
    }
    fn step(&mut self) {
        self.pipeline.step(self.gravity, &self.parameters, &mut self.islands, &mut self.broad, &mut self.narrow, &mut self.bodies,
            &mut self.colliders, &mut self.impulse, &mut self.multibody, &mut self.soft, &mut self.ccd, &(), &());
    }
    fn car(&mut self, at: Vector, half: Vector, density: f32, sensor: bool) -> RigidBodyHandle {
        let b = self.bodies.insert(RigidBodyBuilder::dynamic().translation(at).can_sleep(false));
        self.colliders.insert_with_parent(ColliderBuilder::cuboid(half.x, half.y, half.z).density(density).sensor(sensor).restitution(0.0).friction(0.5), b, &mut self.bodies);
        b
    }
    fn wp(&self, b: RigidBodyHandle, local: Vector) -> Vector { self.bodies[b].position() * local }
    fn fix(&mut self, b: RigidBodyHandle) { self.bodies[b].set_body_type(RigidBodyType::Fixed, true); }
}

#[derive(Clone, Copy)]
enum Kind { Impulse, Multibody }
fn couple(w: &mut W, kind: Kind, a: RigidBodyHandle, b: RigidBodyHandle, pa: Vector, pb: Vector) -> bool {
    let j = SphericalJointBuilder::new().local_anchor1(pa).local_anchor2(pb).build();
    match kind { Kind::Impulse => { w.impulse.insert(a, b, j, true); true } Kind::Multibody => w.multibody.insert(a, b, j, true).is_some() }
}
type Pivots = Vec<(RigidBodyHandle, Vector, RigidBodyHandle, Vector)>;
fn gap(w: &W, p: &Pivots) -> f32 { p.iter().map(|&(x, px, y, py)| (w.wp(x, px) - w.wp(y, py)).length()).fold(0.0, f32::max) }
fn mean_std(v: &[f32]) -> (f32, f32) { let m = v.iter().sum::<f32>() / v.len() as f32; (m, (v.iter().map(|x| (x - m).powi(2)).sum::<f32>() / v.len() as f32).sqrt()) }

fn main() {
    let mm = |x: f32| x * 0.001;
    println!("== 1. Mass ratio: a small slab (1x5x1 m) hinged on a huge cube (20 m side) by two joints 4 m apart; huge cube free; slab spun about the joint line ==");
    for ratio_density in [100.0f32, 800_000.0] {
        for it in [16usize, 64] {
            let mut w = W::new(it, 0.0);
            let big = w.car(Vector::ZERO, Vector::splat(10.0), ratio_density, true);
            let small = w.car(Vector::new(11.0, 0.0, 0.0), Vector::new(0.5, 2.5, 0.5), 100.0, true);
            let piv: Pivots = vec![(big, Vector::new(10.5, -2.0, 0.0), small, Vector::new(-0.5, -2.0, 0.0)), (big, Vector::new(10.5, 2.0, 0.0), small, Vector::new(-0.5, 2.0, 0.0))];
            for &(a, pa, b, pb) in &piv { couple(&mut w, Kind::Impulse, a, b, pa, pb); }
            w.bodies[small].set_angvel(Vector::new(0.3, 2.0, 0.7), true);
            let mut worst = 0.0f32;
            for _ in 0..2400 { w.step(); worst = worst.max(gap(&w, &piv)); }
            println!("  mass ratio {:>9.0}:1  impulse it={it:<3} worst gap {:.6} m  final gap {:.6} m  big car drifted {:.4} m", w.bodies[big].mass() / w.bodies[small].mass(), worst, gap(&w, &piv), w.bodies[big].translation().length());
        }
    }

    println!("== 2. Light bar between two huge blocks (one joint at each end), bar kicked ==");
    for it in [16usize, 64] {
        let mut w = W::new(it, 0.0);
        let l = w.car(Vector::new(-6.0, 0.0, 0.0), Vector::splat(5.0), 800_000.0, true);
        let r = w.car(Vector::new(6.0, 0.0, 0.0), Vector::splat(5.0), 800_000.0, true);
        let bar = w.car(Vector::ZERO, Vector::new(1.0, 0.2, 0.2), 100.0, true);
        let piv: Pivots = vec![(l, Vector::new(5.0, 0.0, 0.0), bar, Vector::new(-1.0, 0.0, 0.0)), (bar, Vector::new(1.0, 0.0, 0.0), r, Vector::new(-5.0, 0.0, 0.0))];
        for &(a, pa, b, pb) in &piv { couple(&mut w, Kind::Impulse, a, b, pa, pb); }
        w.bodies[bar].set_linvel(Vector::new(0.0, 3.0, 0.0), true); w.bodies[bar].set_angvel(Vector::new(5.0, 4.0, 3.0), true);
        let mut worst = 0.0f32;
        for _ in 0..2400 { w.step(); worst = worst.max(gap(&w, &piv)); }
        println!("  impulse it={it:<3} worst gap {worst:.6} m  final gap {:.6} m", gap(&w, &piv));
    }

    println!("== 3. Contact: the door (A fixed, 2 joints) swung into a fixed wall, solid colliders ==");
    for it in [4usize, 16, 64] {
        let mut w = W::new(it, 0.0);
        let a = w.car(Vector::ZERO, Vector::splat(1.0), 100.0, false); w.fix(a);
        let b = w.car(Vector::new(3.0, 0.0, 0.0), Vector::splat(1.0), 100.0, false);
        let wall = w.bodies.insert(RigidBodyBuilder::fixed().translation(Vector::new(3.0, 0.0, -2.5)));
        w.colliders.insert_with_parent(ColliderBuilder::cuboid(3.0, 3.0, 1.0).restitution(0.0).friction(0.5), wall, &mut w.bodies);
        let piv: Pivots = vec![(a, Vector::new(1.5, -2.0, 0.0), b, Vector::new(-1.5, -2.0, 0.0)), (a, Vector::new(1.5, 2.0, 0.0), b, Vector::new(-1.5, 2.0, 0.0))];
        for &(x, px, y, py) in &piv { couple(&mut w, Kind::Impulse, x, y, px, py); }
        w.bodies[b].set_angvel(Vector::new(0.0, 2.0, 0.0), true);
        let (mut worst, mut tail) = (0.0f32, Vec::new());
        for s in 0..2400 { w.step(); worst = worst.max(gap(&w, &piv)); if s >= 2000 { tail.push(w.bodies[b].linvel().length() + w.bodies[b].angvel().length()); } }
        let (m, sd) = mean_std(&tail);
        println!("  impulse it={it:<3} worst gap {worst:.6} m  resting speed mean {m:.6} (std {sd:.6})  centre ended at z={:.3}", w.bodies[b].translation().z);
    }

    println!("== 4. Slightly inconsistent pivots: second joint's pivot on B shifted along the axis, A fixed, B spun ==");
    for off in [mm(0.1), mm(1.0), mm(10.0)] {
        for it in [16usize, 64] {
            let mut w = W::new(it, 0.0);
            let a = w.car(Vector::ZERO, Vector::splat(1.0), 100.0, true); w.fix(a);
            let b = w.car(Vector::new(3.0, 0.0, 0.0), Vector::splat(1.0), 100.0, true);
            let piv: Pivots = vec![(a, Vector::new(1.5, -2.0, 0.0), b, Vector::new(-1.5, -2.0, 0.0)), (a, Vector::new(1.5, 2.0, 0.0), b, Vector::new(-1.5, 2.0 + off, 0.0))];
            for &(x, px, y, py) in &piv { couple(&mut w, Kind::Impulse, x, y, px, py); }
            w.bodies[b].set_angvel(Vector::new(0.0, 2.0, 0.0), true);
            let (mut worst, mut tail) = (0.0f32, Vec::new());
            for s in 0..2400 { w.step(); worst = worst.max(gap(&w, &piv)); if s >= 2000 { tail.push(w.bodies[b].linvel().length() + w.bodies[b].angvel().length()); } }
            let (m, sd) = mean_std(&tail);
            println!("  mismatch {:>5.1} mm  impulse it={it:<3} worst gap {worst:.6} m  final gap {:.6} m  late speed mean {m:.5} (std {sd:.5})", off * 1000.0, gap(&w, &piv));
        }
    }

    println!("== 5. Is the error floor f32 rounding? Door (consistent), everything moved x metres from the origin ==");
    for x in [0.0f32, 100.0, 1000.0, 10_000.0] {
        for it in [16usize, 64] {
            let mut w = W::new(it, 0.0);
            let a = w.car(Vector::new(x, 0.0, 0.0), Vector::splat(1.0), 100.0, true); w.fix(a);
            let b = w.car(Vector::new(x + 3.0, 0.0, 0.0), Vector::splat(1.0), 100.0, true);
            let piv: Pivots = vec![(a, Vector::new(1.5, -2.0, 0.0), b, Vector::new(-1.5, -2.0, 0.0)), (a, Vector::new(1.5, 2.0, 0.0), b, Vector::new(-1.5, 2.0, 0.0))];
            for &(p, pp, q, pq) in &piv { couple(&mut w, Kind::Impulse, p, q, pp, pq); }
            w.bodies[b].set_angvel(Vector::new(0.3, 2.0, 0.7), true);
            let mut worst = 0.0f32;
            for _ in 0..2400 { w.step(); worst = worst.max(gap(&w, &piv)); }
            println!("  at {x:>7.0} m  impulse it={it:<3} worst gap {worst:.6} m   (one f32 step at that distance: {:.6} m)", f32::EPSILON * x.max(1.0));
        }
    }

    println!("== 6. Tree under gravity: B hung by one ball joint from free A, both cubes; energy over 30 s ==");
    for (label, kind) in [("impulse", Kind::Impulse), ("multibody", Kind::Multibody)] {
        for density_a in [100.0f32, 800_000.0] {
            let mut w = W::new(16, 9.81);
            let a = w.car(Vector::ZERO, Vector::splat(1.0), density_a, true);
            let b = w.car(Vector::new(3.0, 0.0, 0.0), Vector::splat(1.0), 100.0, true);
            let piv: Pivots = vec![(a, Vector::new(1.0, 0.0, 0.0), b, Vector::new(-1.0, 0.0, 0.0))];
            let ok = couple(&mut w, kind, a, b, piv[0].1, piv[0].3);
            let energy = |w: &W| -> f32 { [a, b].iter().map(|&h| { let r = &w.bodies[h]; let m = r.mass(); let inertia = m * 4.0 / 6.0;
                0.5 * m * r.linvel().length_squared() + 0.5 * inertia * r.angvel().length_squared() + m * 9.81 * r.translation().y }).sum() };
            let e0 = energy(&w);
            let mut worst = 0.0f32;
            let mut e_max_dev = 0.0f32;
            for _ in 0..2400 { w.step(); worst = worst.max(gap(&w, &piv)); e_max_dev = e_max_dev.max((energy(&w) - e0).abs()); }
            let scale = w.bodies[b].mass() * 9.81 * 3.0;
            println!("  {label:<9} A/B mass {:>7.0}:1 accepted={ok} worst gap {worst:.6} m  max energy deviation {:.3} % of B's swing energy", w.bodies[a].mass() / w.bodies[b].mass(), 100.0 * e_max_dev / scale);
        }
    }
}
