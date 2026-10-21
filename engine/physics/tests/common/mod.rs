//! What the tests of characters share: a scene of terrain made of plain maps, and the calls that keep a character's terrain.

#![allow(dead_code)] // Each test file uses some of it.

use std::collections::HashMap;
use std::sync::Arc;

use block_models::{Shape, ShapeBox};
use physics::{BrickHint, CharacterBody, CharacterSettings, Collision, CollisionSource, MoveInput};
use voxel_space::{BrickCoordinate, VoxelCoordinate, WorldPosition};

/// A world of air above a floor, with whatever else is put in it. Looking at what the character asks
/// about is counted, to see that the work stays small.
#[derive(Default)]
pub struct Scene {
    /// Everything at or below this height is a whole cube. `None` for no floor.
    pub floor: Option<i64>,
    pub cubes: HashMap<VoxelCoordinate, Collision>,
    /// Voxels beyond this distance along +x are not available: whole cubes, as the contract says.
    pub unavailable_beyond_x: Option<i64>,
    /// Voxels below this height are not available either.
    pub unavailable_below_y: Option<i64>,
    /// Which voxels are whole cubes, for a scene that is not a floor with things on it (a wall, a floor turned over). Where it says no,
    /// the voxel is air, whatever else the scene has.
    pub solid: Option<Box<dyn Fn(VoxelCoordinate) -> bool>>,
    pub asked: usize,
    /// Edited since the character was told.
    pub dirty: bool,
}

impl Scene {
    pub fn with_floor(top_voxel: i64) -> Self {
        Self {
            floor: Some(top_voxel),
            ..Self::default()
        }
    }

    /// A scene whose whole cubes are the voxels `solid` says.
    pub fn of(solid: impl Fn(VoxelCoordinate) -> bool + 'static) -> Self {
        Self {
            solid: Some(Box::new(solid)),
            ..Self::default()
        }
    }

    pub fn cube(&mut self, x: i64, y: i64, z: i64) {
        self.dirty = true;
        self.cubes.insert(at(x, y, z), Collision::Full);
    }

    pub fn remove(&mut self, x: i64, y: i64, z: i64) {
        self.dirty = true;
        self.cubes.insert(at(x, y, z), Collision::Empty);
    }

    pub fn shape(&mut self, x: i64, y: i64, z: i64, shape: &Shape) {
        let boxes: Arc<[ShapeBox]> = shape.boxes().to_vec().into();
        self.dirty = true;
        self.cubes.insert(at(x, y, z), Collision::Partial(boxes));
    }
}

impl CollisionSource for Scene {
    fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
        self.asked += 1;
        if self
            .unavailable_beyond_x
            .is_some_and(|limit| voxel.x() > limit)
            || self
                .unavailable_below_y
                .is_some_and(|limit| voxel.y() < limit)
        {
            return Collision::Full;
        }
        if let Some(found) = self.cubes.get(&voxel) {
            return found.clone();
        }
        if let Some(solid) = &self.solid {
            return if solid(voxel) {
                Collision::Full
            } else {
                Collision::Empty
            };
        }
        if self.floor.is_some_and(|floor| voxel.y() <= floor) {
            Collision::Full
        } else {
            Collision::Empty
        }
    }

    fn brick_hint(&mut self, brick: BrickCoordinate) -> BrickHint {
        // High above everything is known to be empty without asking about its voxels.
        let low = brick.y() * 32;
        if self.solid.is_some() {
            BrickHint::Unknown
        } else if self.cubes.is_empty() && self.floor.is_some_and(|floor| low > floor) {
            BrickHint::Empty
        } else if self.cubes.is_empty()
            && self.floor.is_some_and(|floor| low + 31 <= floor)
            && self.unavailable_beyond_x.is_none()
        {
            // Everything in it is under the floor.
            BrickHint::Solid
        } else {
            BrickHint::Unknown
        }
    }
}

pub fn at(x: i64, y: i64, z: i64) -> VoxelCoordinate {
    VoxelCoordinate::new(x, y, z)
}

pub fn position(x: f32, y: f32, z: f32) -> WorldPosition {
    WorldPosition::new(BrickCoordinate::new(0, 0, 0), [x, y, z])
}

/// The eye position as plain numbers, in voxels.
pub fn eye(body: &CharacterBody) -> [f64; 3] {
    let eye = body.eye_position();
    let brick = eye.brick();
    let offset = eye.offset();
    [
        brick.x() as f64 * 32.0 + f64::from(offset[0]),
        brick.y() as f64 * 32.0 + f64::from(offset[1]),
        brick.z() as f64 * 32.0 + f64::from(offset[2]),
    ]
}

pub fn spawn(x: f32, y: f32, z: f32) -> CharacterBody {
    CharacterBody::new(position(x, y, z), CharacterSettings::default())
}

/// Steps `count` times with `input`, keeping the terrain around the character, and keeping it clear of what is built into it, as a game
/// does each tick.
pub fn run(body: &mut CharacterBody, scene: &mut Scene, input: MoveInput, count: usize) {
    if scene.dirty {
        edited(body, scene);
    }
    for _ in 0..count {
        body.update_terrain(scene);
        body.keep_clear(scene);
        body.step(input);
    }
}

/// Tells the character that the scene was edited, as the game does when a brick changes: every
/// brick around the origin, where these scenes are, has its colliders made again.
pub fn edited(body: &mut CharacterBody, scene: &mut Scene) {
    scene.dirty = false;
    for x in -1..=1 {
        for y in -1..=1 {
            for z in -1..=1 {
                body.brick_changed(scene, BrickCoordinate::new(x, y, z));
            }
        }
    }
}

pub const STILL: MoveInput = MoveInput {
    forward: 0,
    strafe: 0,
    vertical: 0,
    jump: false,
    sprint: false,
    yaw: 0.0,
};

pub const FORWARD: MoveInput = MoveInput {
    forward: 1,
    ..STILL
};
