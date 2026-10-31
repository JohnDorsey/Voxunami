use rapier3d::prelude::*;

pub struct World {
    pub pipeline: PhysicsPipeline,
    pub parameters: IntegrationParameters,
    pub islands: IslandManager,
    pub broad_phase: BroadPhaseBvh,
    pub narrow_phase: NarrowPhase,
    pub bodies: RigidBodySet,
    pub colliders: ColliderSet,
    pub impulse_joints: ImpulseJointSet,
    pub multibody_joints: MultibodyJointSet,
    pub soft_bodies: SoftBodySet,
    pub ccd: CCDSolver,
    pub gravity: Vector,
    pub voxel_size: f32,
    pub never_sleep: bool,
}

impl World {
    /// Physics units are metres: gravity is 9.81 and `length_unit` is 1.
    pub fn new(voxel_size: f32) -> Self {
        Self::with_units(voxel_size, 1.0)
    }

    /// `units_per_metre` rescales gravity and the solver's tolerances (Rapier's `length_unit`).
    pub fn with_units(voxel_size: f32, units_per_metre: f32) -> Self {
        let mut parameters = IntegrationParameters::default();
        parameters.dt = 1.0 / 60.0;
        parameters.length_unit = units_per_metre;
        Self {
            pipeline: PhysicsPipeline::new(),
            parameters,
            islands: IslandManager::new(),
            broad_phase: BroadPhaseBvh::new(),
            narrow_phase: NarrowPhase::new(),
            bodies: RigidBodySet::new(),
            colliders: ColliderSet::new(),
            impulse_joints: ImpulseJointSet::new(),
            multibody_joints: MultibodyJointSet::new(),
            soft_bodies: SoftBodySet::new(),
            ccd: CCDSolver::new(),
            gravity: Vector::new(0.0, -9.81 * units_per_metre, 0.0),
            voxel_size,
            never_sleep: false,
        }
    }

    pub fn step(&mut self) {
        self.pipeline.step(
            self.gravity,
            &self.parameters,
            &mut self.islands,
            &mut self.broad_phase,
            &mut self.narrow_phase,
            &mut self.bodies,
            &mut self.colliders,
            &mut self.impulse_joints,
            &mut self.multibody_joints,
            &mut self.soft_bodies,
            &mut self.ccd,
            &(),
            &(),
        );
    }

    pub fn add_static_voxels(&mut self, voxel: f32, voxels: &[IVector], at: Vector) -> RigidBodyHandle {
        let body = self.bodies.insert(RigidBodyBuilder::fixed().translation(at));
        let collider = ColliderBuilder::voxels(Vector::splat(voxel), voxels).friction(0.8);
        self.colliders.insert_with_parent(collider, body, &mut self.bodies);
        body
    }

    pub fn add_dynamic_voxels(&mut self, voxel: f32, voxels: &[IVector], at: Vector, density: f32) -> RigidBodyHandle {
        let body = self.bodies.insert(RigidBodyBuilder::dynamic().translation(at).can_sleep(!self.never_sleep));
        let collider = ColliderBuilder::voxels(Vector::splat(voxel), voxels).density(density).friction(0.6);
        self.colliders.insert_with_parent(collider, body, &mut self.bodies);
        body
    }

    /// FNV-1a over the bit patterns of every body's position and rotation, in insertion order.
    pub fn state_hash(&self) -> u64 {
        let mut state: u64 = 0xcbf2_9ce4_8422_2325;
        let mut feed = |value: f32| {
            for byte in value.to_bits().to_le_bytes() {
                state ^= u64::from(byte);
                state = state.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        for (_, body) in self.bodies.iter() {
            let t = body.translation();
            let r = body.rotation();
            for v in [t.x, t.y, t.z, r.x, r.y, r.z, r.w] {
                feed(v);
            }
        }
        state
    }
}

/// Hilly ground: columns of solid voxels with a pseudo-random height around `base`.
pub fn terrain_voxels(width: i32, depth: i32, base: i32) -> Vec<IVector> {
    let mut voxels = Vec::new();
    for x in 0..width {
        for z in 0..depth {
            let hash = (x.wrapping_mul(7919) ^ z.wrapping_mul(104_729)).rem_euclid(4);
            for y in 0..=(base + hash) {
                voxels.push(IVector::new(x, y, z));
            }
        }
    }
    voxels
}

pub fn box_voxels(x: i32, y: i32, z: i32) -> Vec<IVector> {
    let mut voxels = Vec::new();
    for i in 0..x {
        for j in 0..y {
            for k in 0..z {
                voxels.push(IVector::new(i, j, k));
            }
        }
    }
    voxels
}
