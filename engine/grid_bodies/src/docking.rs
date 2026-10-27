//! Docking: how two ships with docking ports (see `engine:docking_face`) are pulled together, turned into line, and joined.
//!
//! This module is the geometry, pure and tested alone: where a port's face is in the world, what a pair of ports is to each other (how far apart,
//! how much twist is left to give), the forces and torques that pull them in, and the exact pose a ship has when it has docked. The plugin
//! (`plugin.rs`) finds the ports of the ships it moves and puts these on their bodies.
//!
//! All of it is in metres, kilograms and seconds, the world's axes and `f64`; the one transcendental function (the angle left to turn) is `libm`'s,
//! so the result is the same on every machine.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

use block_models::{BlockTable, PropertyHandle};
use server::{AccessError, DetachError, GridView, ServerWorld, SplitError};
use voxel_grid::ChannelIndex;
use voxel_space::{
    FaceDirection, GridId, GridPose, LatticeRotation, LatticeTransform, VoxelCoordinate,
    WorldPosition,
};
use world_storage::MovingGrid;

use crate::plugin::Weigher;

/// How ships are drawn together and joined. The forces do not depend on the mass of a ship: each of the two ports gets the same force and each
/// ship the same torque, so a light ship is moved more by them than a heavy one (owner, 2026-10-22).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DockingSettings {
    /// How far apart, in metres, the middles of two ports' faces may be and still pull on each other.
    pub range: f64,
    /// The force, in newtons, on each of two ports that pull on each other, toward the other.
    pub attraction_force: f64,
    /// The torque, in newton metres, on each of the two ships, that turns them toward lining up: the second about the axis that brings it into
    /// line and the first about the opposite.
    pub twist_torque: f64,
    /// The angle left to turn, in degrees, at which the torque is the whole of `twist_torque`; with less left to turn it is in proportion.
    pub full_twist_degrees: f64,
    /// Ports whose faces are this near, in metres, dock if they are also within `snap_degrees` of lined up.
    pub snap_distance: f64,
    pub snap_degrees: f64,
    /// How fast apart, in metres a second, two ships that were undocked are set going, in all (shared out so that their momentum is as it was).
    pub separation_speed: f64,
}

impl Default for DockingSettings {
    /// A range of 6 metres, 10 kilonewtons and 10 kilonewton metres (a metric ton accelerates at 10 metres a second a second), full twist
    /// from 30 degrees out; snapping within 0.3 metres and 10 degrees (owner, 2026-10-22); ships set apart at half a metre a second. The
    /// range, the forces and the speed apart are my guesses, for the owner's feel.
    fn default() -> Self {
        Self {
            range: 6.0,
            attraction_force: 10_000.0,
            twist_torque: 10_000.0,
            full_twist_degrees: 30.0,
            snap_distance: 0.3,
            snap_degrees: 10.0,
            separation_speed: 0.5,
        }
    }
}

/// A docking port of a ship: the voxel its block is in and the face of the block that docks, in the ship's own axes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Port {
    pub voxel: VoxelCoordinate,
    pub face: FaceDirection,
}

impl Port {
    /// The middle of the port's face, in voxels, in the ship's own axes.
    pub fn local_centre(&self) -> [f64; 3] {
        let step = self.face.offset();
        [0, 1, 2].map(|axis| {
            [self.voxel.x(), self.voxel.y(), self.voxel.z()][axis] as f64
                + 0.5
                + 0.5 * step[axis] as f64
        })
    }
}

/// A port seen from the world: all of it in the world's axes, measured from a point that is the same for every port compared.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PortView {
    pub port: Port,
    /// The middle of the port's face.
    pub centre: [f64; 3],
    /// The way the face looks.
    pub normal: [f64; 3],
    /// The ship's turn: its axes in the world's.
    pub rotation: [[f64; 3]; 3],
}

impl PortView {
    /// The port of a ship with this pose, measured from `from`. `None` if the pose is not where `from` can measure to.
    pub fn of(port: Port, pose: &GridPose, from: WorldPosition) -> Self {
        let rotation = pose.rotation_matrix();
        let origin = pose.origin_relative_to(from);
        let at = turn(rotation, port.local_centre());
        let step = port.face.offset();
        Self {
            port,
            centre: [0, 1, 2].map(|axis| origin[axis] + at[axis]),
            normal: turn(rotation, step.map(|component| component as f64)),
            rotation,
        }
    }
}

/// What a pair of ports is to each other.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Approach {
    /// From the middle of the first port's face to the middle of the second's.
    pub gap: [f64; 3],
    pub distance: f64,
    /// The turn that carries the second ship's axes onto the first's when they are docked: of the 24 there are, the four that put the second's
    /// docking face against the first's are the ones that dock them, and this is the one nearest to how the ships are turned.
    pub turn: LatticeRotation,
    /// The axis, in the world's, that the second ship turns about to come into line by the shortest way, and the cosine of the angle it has
    /// to turn by (1 when it is in line).
    pub twist_axis: [f64; 3],
    pub twist_cosine: f64,
}

impl Approach {
    /// How far the second ship has to turn to be in line, in radians, 0 to π.
    pub fn twist_angle(&self) -> f64 {
        let sine = length(self.twist_axis_scaled());
        libm::atan2(sine, self.twist_cosine)
    }

    /// The axis scaled by the sine of the angle (the vector the turn leaves in the skew part of its matrix).
    fn twist_axis_scaled(&self) -> [f64; 3] {
        let sine = (1.0 - self.twist_cosine * self.twist_cosine)
            .max(0.0)
            .sqrt();
        self.twist_axis.map(|component| component * sine)
    }
}

/// How two ports are placed to each other: the gap between their faces, and what the second ship has to turn by to dock with the first.
pub fn approach(first: &PortView, second: &PortView) -> Approach {
    let gap = [0, 1, 2].map(|axis| second.centre[axis] - first.centre[axis]);
    // The second ship's axes in the first's.
    let relative = multiply(transpose(first.rotation), second.rotation);
    let wanted = first.port.face.opposite();
    let mut best: Option<(LatticeRotation, f64)> = None;
    for candidate in LatticeRotation::all() {
        if candidate.apply_face(second.port.face) != wanted {
            continue;
        }
        let matrix = candidate.matrix().map(|row| row.map(|entry| entry as f64));
        let score: f64 = (0..3)
            .flat_map(|row| (0..3).map(move |column| (row, column)))
            .map(|(row, column)| matrix[row][column] * relative[row][column])
            .sum();
        if best.is_none_or(|(_, top)| score > top) {
            best = Some((candidate, score));
        }
    }
    let (turn, _) = best.expect("four turns carry any face to any other face");
    // What the second has to turn by, in the world's axes: where it should be, then undo where it is.
    let matrix = turn.matrix().map(|row| row.map(|entry| entry as f64));
    let delta = multiply(multiply(first.rotation, matrix), transpose(second.rotation));
    let cosine = ((delta[0][0] + delta[1][1] + delta[2][2] - 1.0) / 2.0).clamp(-1.0, 1.0);
    let skew = [
        (delta[2][1] - delta[1][2]) / 2.0,
        (delta[0][2] - delta[2][0]) / 2.0,
        (delta[1][0] - delta[0][1]) / 2.0,
    ];
    let sine = length(skew);
    let twist_axis = if sine > 1e-12 {
        skew.map(|component| component / sine)
    } else {
        [0.0; 3]
    };
    Approach {
        gap,
        distance: length(gap),
        turn,
        twist_axis,
        twist_cosine: cosine,
    }
}

impl DockingSettings {
    /// Whether two ports can pull on each other: their faces are near enough, each looks toward the other (a little behind it is allowed, by the
    /// snap distance, so that ports that have come together still do), and they look against each other.
    pub fn pulls(&self, first: &PortView, second: &PortView, approach: &Approach) -> bool {
        approach.distance <= self.range
            && dot(first.normal, approach.gap) >= -self.snap_distance
            && dot(second.normal, approach.gap) <= self.snap_distance
            && dot(first.normal, second.normal) < 0.0
    }

    /// The force on the first port (the second has the same, the other way): `attraction_force` toward the other, none where the faces are at
    /// one place.
    pub fn attraction(&self, approach: &Approach) -> [f64; 3] {
        if approach.distance < 1e-9 {
            return [0.0; 3];
        }
        approach
            .gap
            .map(|component| self.attraction_force * component / approach.distance)
    }

    /// The torque on the second ship, about its centre of mass; the first ship has the opposite. Zero where they are in line.
    pub fn twist(&self, approach: &Approach) -> [f64; 3] {
        let share = (approach.twist_angle() / self.full_twist_angle()).min(1.0);
        approach
            .twist_axis
            .map(|component| self.twist_torque * share * component)
    }

    fn full_twist_angle(&self) -> f64 {
        self.full_twist_degrees * DEGREES
    }

    /// Whether the ports are near enough, and the ships in line enough, to dock.
    pub fn can_snap(&self, approach: &Approach) -> bool {
        approach.distance <= self.snap_distance
            && approach.twist_cosine >= libm::cos(self.snap_degrees * DEGREES)
    }
}

const DEGREES: f64 = std::f64::consts::PI / 180.0;

/// The torque about the centre of mass of a force put on a body away from it: the arm (from the centre of mass to where the force is, in the body's
/// own axes), turned into the world's by the body's rotation, crossed with the force.
pub fn torque_of_force(rotation: [[f64; 3]; 3], arm: [f64; 3], force: [f64; 3]) -> [f64; 3] {
    cross(turn(rotation, arm), force)
}

/// Where the second ship is when it has docked with the first, exactly: the pose it has, and the transform that carries its voxels onto the
/// first's (the one `voxel_space::lattice_alignment` finds from the two poses). The second's port voxel is the voxel against the first's
/// port, on the side its face looks. `None` if that is outside the world.
pub fn docked_pose(
    first_pose: &GridPose,
    first: Port,
    second: Port,
    turn: LatticeRotation,
) -> Option<(GridPose, LatticeTransform)> {
    let step = first.face.offset();
    let wanted = first.voxel.checked_offset(step[0], step[1], step[2])?;
    let carried = turn.apply([second.voxel.x(), second.voxel.y(), second.voxel.z()]);
    let offset = [
        wanted.x() - carried[0],
        wanted.y() - carried[1],
        wanted.z() - carried[2],
    ];
    // Where the second's origin is in the first's axes: the transform's offset, less what the turn does to the half voxel to a voxel's middle.
    let corner = turn.apply([1, 1, 1]);
    let origin: [i128; 3] = [0, 1, 2].map(|axis| offset[axis] - (corner[axis] - 1) / 2);
    let place = {
        let (brick, local) = VoxelCoordinate::new(origin[0], origin[1], origin[2]).split();
        WorldPosition::new(brick, [local.x(), local.y(), local.z()].map(f32::from))
    };
    let position = first_pose.local_to_world(place)?;
    let orientation = first_pose.orientation.compose(turn.orientation());
    Some((
        GridPose::new(position, orientation),
        LatticeTransform {
            rotation: turn,
            offset,
        },
    ))
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

fn turn(matrix: [[f64; 3]; 3], vector: [f64; 3]) -> [f64; 3] {
    matrix.map(|row| dot(row, vector))
}

fn transpose(m: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    [0, 1, 2].map(|row| [0, 1, 2].map(|column| m[column][row]))
}

fn multiply(a: [[f64; 3]; 3], b: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    [0, 1, 2].map(|row| [0, 1, 2].map(|column| (0..3).map(|k| a[row][k] * b[k][column]).sum()))
}

/// Two ports that are docked: the first's face looks at the second's block, and the second's looks back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DockedPair {
    pub first: Port,
    pub second: Port,
}

/// Why two docked ships could not be undocked.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum UndockError {
    #[error(transparent)]
    Access(#[from] AccessError),
    #[error("those ports are not docked to each other")]
    NotDocked,
    #[error(
        "the ships are joined by blocks besides the ports (a weld); breaking them lets the ships come apart"
    )]
    Welded,
    #[error(transparent)]
    Split(#[from] SplitError),
    #[error(
        "both sides of those ports stand on the ground of the planet: neither would leave (undock a ship that is above the ground, or dig the ground from under one side)"
    )]
    BothSidesOnTheGround,
    #[error("more than {PLANET_UNDOCK_BLOCKS} blocks are joined to those ports")]
    TooManyBlocks,
    #[error(transparent)]
    Detach(#[from] DetachError),
}

/// The most blocks of a planet that a side of a docked pair of ports may have to be told from the rest: it is looked at block by block.
const PLANET_UNDOCK_BLOCKS: usize = 100_000;

/// Docking, as a game sets it up: how ships are drawn together, the blocks that tell where the ports are, and the pairs of ships that were
/// undocked and have not yet come apart (which do not pull on each other until they have). Clones share that record, so the plugin that moves ships
/// and the command that undocks them agree.
#[derive(Clone, Debug)]
pub struct Docking {
    pub settings: DockingSettings,
    table: BlockTable,
    block_channel: ChannelIndex,
    state_channel: ChannelIndex,
    orientation_channel: Option<ChannelIndex>,
    air: u32,
    density: PropertyHandle<u32>,
    released: Arc<Mutex<BTreeSet<(GridId, GridId)>>>,
}

impl Docking {
    /// Docking in a game whose blocks are in `table` (which declares the engine's properties, as every game's does).
    pub fn new(
        settings: DockingSettings,
        table: BlockTable,
        block_channel: ChannelIndex,
        state_channel: ChannelIndex,
        orientation_channel: Option<ChannelIndex>,
        air: u32,
    ) -> Self {
        let density = table
            .handle(&block_models::engine::DENSITY)
            .expect("the engine declares engine:density in every game");
        Self {
            settings,
            table,
            block_channel,
            state_channel,
            orientation_channel,
            air,
            density,
            released: Arc::default(),
        }
    }

    pub(crate) fn table(&self) -> &BlockTable {
        &self.table
    }

    fn weigher(&self) -> Weigher<'_> {
        Weigher {
            table: &self.table,
            density: self.density,
            block_channel: self.block_channel,
            state_channel: self.state_channel,
            orientation_channel: self.orientation_channel,
            air: self.air,
        }
    }

    fn released_set(&self) -> MutexGuard<'_, BTreeSet<(GridId, GridId)>> {
        self.released
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Whether any pair of ships was undocked and has not yet come apart.
    pub(crate) fn has_released(&self) -> bool {
        !self.released_set().is_empty()
    }

    /// The pairs of ships (the lower id first) that were undocked and have not come apart.
    pub(crate) fn released(&self) -> BTreeSet<(GridId, GridId)> {
        self.released_set().clone()
    }

    /// Lets ships that were undocked pull on each other again once none of their ports is within range of the other's (or one of them is gone).
    pub(crate) fn forget_those_that_have_come_apart(
        &self,
        world: &ServerWorld,
        ports: &BTreeMap<GridId, Vec<Port>>,
    ) {
        let mut released = self.released_set();
        released.retain(|(first, second)| {
            let (Some(one), Some(other)) = (
                world.data().grids().get(*first),
                world.data().grids().get(*second),
            ) else {
                return false;
            };
            // (A ship the bodies have not yet taken up, made this tick, is not known to have come apart from anything.)
            let (Some(first_ports), Some(second_ports)) = (ports.get(first), ports.get(second))
            else {
                return true;
            };
            let from = one.pose.position;
            first_ports.iter().any(|first_port| {
                let first_view = PortView::of(*first_port, &one.pose, from);
                second_ports.iter().any(|second_port| {
                    let second_view = PortView::of(*second_port, &other.pose, from);
                    approach(&first_view, &second_view).distance <= self.settings.range
                })
            })
        });
    }

    /// Whether a ship has a docking port there, as it says.
    pub(crate) fn has_port(&self, ship: &MovingGrid, port: Port) -> bool {
        let orientation = self
            .orientation_channel
            .map_or(0, |channel| ship.voxels.get(channel, port.voxel));
        self.table
            .docking_face(
                ship.voxels.get(self.block_channel, port.voxel),
                ship.voxels.get(self.state_channel, port.voxel),
                orientation,
            )
            .is_some_and(|face| face == port.face)
            && ship.voxels.get(self.block_channel, port.voxel) != self.air
    }

    /// The docking ports of a ship, in order of brick and voxel.
    pub fn ports_of(&self, ship: &MovingGrid) -> Vec<Port> {
        let weigher = self.weigher();
        ship.voxels
            .sorted_brick_coordinates()
            .into_iter()
            .flat_map(|brick| weigher.scan_brick(ship, brick, Some(&self.table)).1)
            .collect()
    }

    /// The pairs of ports of a ship that are docked: a port whose face looks at the block of another port whose face looks back at it.
    pub fn docked_pairs(&self, ship: &MovingGrid) -> Vec<DockedPair> {
        let ports = self.ports_of(ship);
        let mut pairs = Vec::new();
        for first in &ports {
            let step = first.face.offset();
            let Some(next) = first.voxel.checked_offset(step[0], step[1], step[2]) else {
                continue;
            };
            let second = Port {
                voxel: next,
                face: first.face.opposite(),
            };
            if first.voxel < second.voxel && ports.contains(&second) {
                pairs.push(DockedPair {
                    first: *first,
                    second,
                });
            }
        }
        pairs
    }

    /// Cuts a ship in two at a docked pair of ports: the ports are not joined, and the ship comes apart if nothing else joins its two sides
    /// (it does not if a weld does: a solid block beside both, say), and then each side is a ship that goes on as it did and is let go from the other
    /// at `separation_speed` (shared out so that their momentum is as it was), and the two do not pull on each other until they are out of
    /// range. Returns the side of the first port and the side of the second.
    pub fn undock(
        &self,
        world: &mut ServerWorld,
        ship: GridId,
        pair: DockedPair,
    ) -> Result<[GridId; 2], UndockError> {
        let moving = world
            .data()
            .grids()
            .get(ship)
            .ok_or(AccessError::NoSuchGrid(ship))?;
        if !self.docked_pairs(moving).contains(&pair) {
            return Err(UndockError::NotDocked);
        }
        if moving.terrain.is_some() {
            return self.undock_from_planet(world, ship, pair);
        }
        let is_part =
            |voxel: VoxelCoordinate| moving.voxels.get(self.block_channel, voxel) != self.air;
        // The first side: everything joined to the first port without crossing between the two ports.
        let mut side: BTreeSet<VoxelCoordinate> = BTreeSet::new();
        let mut queue = VecDeque::from([pair.first.voxel]);
        side.insert(pair.first.voxel);
        while let Some(voxel) = queue.pop_front() {
            for face in FaceDirection::ALL {
                let step = face.offset();
                let Some(next) = voxel.checked_offset(step[0], step[1], step[2]) else {
                    continue;
                };
                let between_the_ports = (voxel == pair.first.voxel && next == pair.second.voxel)
                    || (voxel == pair.second.voxel && next == pair.first.voxel);
                if between_the_ports || side.contains(&next) || !is_part(next) {
                    continue;
                }
                side.insert(next);
                queue.push_back(next);
            }
        }
        if side.contains(&pair.second.voxel) {
            return Err(UndockError::Welded);
        }
        let parts = self.weigher().parts_of(moving, vec![side]);
        let masses: Vec<f64> = parts.iter().map(|(_, mass)| *mass).collect();
        let direction = moving
            .pose
            .direction_to_world(pair.first.face.offset().map(|component| component as f32));
        let split = world.split_grid(ship, parts.into_iter().map(|(part, _)| part).collect())?;
        let (first_side, second_side) = (split[0], split[1]);
        // Apart: the side of the first port goes back, the side of the second forward, sharing the speed out so that momentum stays.
        if let [first_mass, second_mass] = masses[..]
            && first_mass + second_mass > 0.0
        {
            let speed = self.settings.separation_speed;
            let shares = [
                (
                    first_side,
                    -speed * second_mass / (first_mass + second_mass),
                ),
                (second_side, speed * first_mass / (first_mass + second_mass)),
            ];
            for (part, change) in shares {
                if let Some(grid) = world.data().grids().get(part) {
                    let (linear, angular) = (grid.linear_velocity, grid.angular_velocity);
                    let faster =
                        [0, 1, 2].map(|axis| linear[axis] + direction[axis] * change as f32);
                    let _ = world.set_grid_velocity(part, faster, angular);
                }
            }
        }
        let released = if first_side < second_side {
            (first_side, second_side)
        } else {
            (second_side, first_side)
        };
        self.released_set().insert(released);
        Ok([first_side, second_side])
    }
}

impl Docking {
    /// Undocks what is docked to a planet. The blocks of a planet that someone built (a block that is not the planet's ground at that place) join into sides at a
    /// docked pair of ports, as the blocks of a ship do; the side that does not stand on the ground of the planet (none of its blocks is in, or beside, ground that
    /// has not been dug or built on) leaves, as a ship of its own that begins at rest in the planet's frame and is set going away from the other side at
    /// `separation_speed`; if neither stands on the ground the smaller leaves, and if both do, nothing does. The planet keeps its identifier. Returns the
    /// side of the first port and the side of the second, as for a ship: the planet is the one that stayed.
    fn undock_from_planet(
        &self,
        world: &mut ServerWorld,
        planet: GridId,
        pair: DockedPair,
    ) -> Result<[GridId; 2], UndockError> {
        let mut sides: Vec<(BTreeSet<VoxelCoordinate>, bool)> = Vec::new();
        for (start, other) in [(pair.first, pair.second), (pair.second, pair.first)] {
            let side = self.built_side(world, planet, start.voxel, other.voxel)?;
            if side.contains(&other.voxel) {
                return Err(UndockError::Welded);
            }
            let grounded = self.stands_on_the_ground(world, planet, &side)?;
            sides.push((side, grounded));
        }
        let leaving = match (sides[0].1, sides[1].1) {
            (true, true) => return Err(UndockError::BothSidesOnTheGround),
            (false, true) => 0,
            (true, false) => 1,
            (false, false) => usize::from(sides[1].0.len() < sides[0].0.len()),
        };
        let staying_port = if leaving == 0 {
            pair.second
        } else {
            pair.first
        };
        let voxels: Vec<VoxelCoordinate> = sides[leaving].0.iter().copied().collect();
        let pose = world
            .data()
            .grids()
            .get(planet)
            .ok_or(AccessError::NoSuchGrid(planet))?
            .pose;
        let ship = world.detach_blocks(planet, &voxels)?;
        // Away from the side that stays: the way the face of its port looks, which is toward the side that leaves.
        let direction =
            pose.direction_to_world(staying_port.face.offset().map(|component| component as f32));
        let speed = self.settings.separation_speed as f32;
        let _ =
            world.set_grid_velocity(ship, direction.map(|component| component * speed), [0.0; 3]);
        let released = if planet < ship {
            (planet, ship)
        } else {
            (ship, planet)
        };
        self.released_set().insert(released);
        Ok(if leaving == 0 {
            [ship, planet]
        } else {
            [planet, ship]
        })
    }

    /// The voxels with a built block (see above) joined to this one without crossing to the other voxel, which is where the other port is.
    fn built_side(
        &self,
        world: &mut ServerWorld,
        planet: GridId,
        start: VoxelCoordinate,
        other: VoxelCoordinate,
    ) -> Result<BTreeSet<VoxelCoordinate>, UndockError> {
        let mut view = world.grid(planet)?;
        let mut side: BTreeSet<VoxelCoordinate> = BTreeSet::from([start]);
        let mut queue = VecDeque::from([start]);
        while let Some(voxel) = queue.pop_front() {
            for face in FaceDirection::ALL {
                let step = face.offset();
                let Some(next) = voxel.checked_offset(step[0], step[1], step[2]) else {
                    continue;
                };
                let between_the_ports =
                    (voxel == start && next == other) || (voxel == other && next == start);
                if between_the_ports || side.contains(&next) {
                    continue;
                }
                let now = view.get(self.block_channel, next)?;
                if now == self.air || now == view.generated(self.block_channel, next)? {
                    continue;
                }
                if side.len() >= PLANET_UNDOCK_BLOCKS {
                    return Err(UndockError::TooManyBlocks);
                }
                side.insert(next);
                queue.push_back(next);
            }
        }
        Ok(side)
    }

    /// Whether a block of the side is in ground the planet's generator made, or beside ground that is as it made it.
    fn stands_on_the_ground(
        &self,
        world: &mut ServerWorld,
        planet: GridId,
        side: &BTreeSet<VoxelCoordinate>,
    ) -> Result<bool, UndockError> {
        let mut view = world.grid(planet)?;
        for &voxel in side {
            if view.generated(self.block_channel, voxel)? != self.air {
                return Ok(true);
            }
            for face in FaceDirection::ALL {
                let step = face.offset();
                let Some(next) = voxel.checked_offset(step[0], step[1], step[2]) else {
                    continue;
                };
                if side.contains(&next) {
                    continue;
                }
                let now = view.get(self.block_channel, next)?;
                if now != self.air && now == view.generated(self.block_channel, next)? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use voxel_space::{AlignmentTolerance, BrickCoordinate, Orientation, lattice_alignment};

    use super::*;

    const EAST: FaceDirection = FaceDirection::PositiveX;
    const WEST: FaceDirection = FaceDirection::NegativeX;

    fn port(x: i128, y: i128, z: i128, face: FaceDirection) -> Port {
        Port {
            voxel: VoxelCoordinate::new(x, y, z),
            face,
        }
    }

    fn pose(x: f32, y: f32, z: f32, orientation: Orientation) -> GridPose {
        GridPose::new(
            WorldPosition::new(BrickCoordinate::ORIGIN, [x, y, z]),
            orientation,
        )
    }

    /// The pair of views of two ports on two ships, measured from the first ship's origin.
    fn views(first: (Port, GridPose), second: (Port, GridPose)) -> (PortView, PortView) {
        let from = first.1.position;
        (
            PortView::of(first.0, &first.1, from),
            PortView::of(second.0, &second.1, from),
        )
    }

    fn close(a: [f64; 3], b: [f64; 3]) -> bool {
        (0..3).all(|axis| (a[axis] - b[axis]).abs() < 1e-4)
    }

    /// An orientation that does `orientation` and then turns by `angle` about a world axis.
    fn tilted(orientation: Orientation, axis: [f32; 3], angle: f32) -> Orientation {
        let half = f64::from(angle) / 2.0;
        let sine = libm::sin(half);
        Orientation::from_components([
            f64::from(axis[0]) * sine,
            f64::from(axis[1]) * sine,
            f64::from(axis[2]) * sine,
            libm::cos(half),
        ])
        .unwrap()
        .compose(orientation)
    }

    /// Two ports, the first east-facing at the origin of an unturned ship, the second west-facing, and the pose the second ship has when docked.
    fn a_docked_pair() -> (Port, Port, GridPose, GridPose) {
        let first_pose = pose(0.0, 0.0, 0.0, Orientation::IDENTITY);
        let (first, second) = (port(0, 0, 0, EAST), port(0, 0, 0, WEST));
        let (docked, _) =
            docked_pose(&first_pose, first, second, LatticeRotation::IDENTITY).unwrap();
        (first, second, first_pose, docked)
    }

    #[test]
    fn a_port_is_the_middle_of_a_face_looking_where_the_face_looks_in_the_worlds_axes() {
        let ship = pose(10.0, 20.0, 30.0, Orientation::IDENTITY);
        let view = PortView::of(
            port(2, 3, 4, FaceDirection::NegativeY),
            &ship,
            ship.position,
        );
        assert_eq!(view.centre, [2.5, 3.0, 4.5]);
        assert_eq!(view.normal, [0.0, -1.0, 0.0]);
        // A ship turned a quarter about the vertical that carries +x to -z: the east face of a port looks along -z.
        let quarter = LatticeRotation::all()
            .into_iter()
            .find(|rotation| {
                rotation.apply([1, 0, 0]) == [0, 0, -1] && rotation.apply([0, 1, 0]) == [0, 1, 0]
            })
            .unwrap();
        let turned = pose(0.0, 0.0, 0.0, quarter.orientation());
        let view = PortView::of(port(1, 0, 0, EAST), &turned, turned.position);
        assert!(close(view.normal, [0.0, 0.0, -1.0]), "{:?}", view.normal);
        assert!(close(view.centre, [0.5, 0.5, -2.0]), "{:?}", view.centre);
    }

    #[test]
    fn ships_docked_exactly_have_no_gap_and_nothing_to_turn_whatever_their_turns_and_whichever_way_each_port_faces()
     {
        for first_turn in LatticeRotation::all() {
            let first_pose = pose(5.0, 6.0, 7.0, first_turn.orientation());
            for second_face in FaceDirection::ALL {
                for turn in LatticeRotation::all() {
                    if turn.apply_face(second_face) != EAST.opposite() {
                        continue;
                    }
                    let (first, second) = (port(3, 1, 2, EAST), port(-4, 5, 0, second_face));
                    let (pose_of_second, transform) =
                        docked_pose(&first_pose, first, second, turn).unwrap();
                    let (one, two) = views((first, first_pose), (second, pose_of_second));
                    let how = approach(&one, &two);
                    assert!(how.distance < 1e-4, "{first_turn:?} {turn:?} {how:?}");
                    assert!(how.twist_cosine > 1.0 - 1e-6, "{how:?}");
                    assert_eq!(how.turn, turn);
                    // The transform is the one the poses give, and it carries the second's port voxel to the voxel against the first's.
                    assert_eq!(
                        lattice_alignment(
                            &pose_of_second,
                            &first_pose,
                            AlignmentTolerance::default()
                        ),
                        Some(transform)
                    );
                    assert_eq!(
                        transform.apply(second.voxel),
                        Some(VoxelCoordinate::new(4, 1, 2))
                    );
                }
            }
        }
    }

    #[test]
    fn any_quarter_turn_about_the_docking_axis_is_as_well_in_line_as_none() {
        let (first, second, first_pose, docked) = a_docked_pair();
        // The second ship turned about the docking axis (x) by each multiple of a quarter.
        for quarters in 0..4 {
            let roll = tilted(
                docked.orientation,
                [1.0, 0.0, 0.0],
                quarters as f32 * std::f32::consts::FRAC_PI_2,
            );
            let rolled = GridPose::new(docked.position, roll);
            let (one, two) = views((first, first_pose), (second, rolled));
            let how = approach(&one, &two);
            assert!(how.twist_cosine > 1.0 - 1e-5, "{quarters}: {how:?}");
        }
        // An eighth of a turn is as far from line as it can be about that axis: 45 degrees.
        let roll = tilted(
            docked.orientation,
            [1.0, 0.0, 0.0],
            std::f32::consts::FRAC_PI_4,
        );
        let (one, two) = views(
            (first, first_pose),
            (second, GridPose::new(docked.position, roll)),
        );
        let how = approach(&one, &two);
        assert!(
            (how.twist_angle() - std::f64::consts::FRAC_PI_4).abs() < 1e-4,
            "{how:?}"
        );
    }

    #[test]
    fn a_tilt_is_twisted_back_by_the_ship_that_is_tilted_and_the_other_the_opposite_way_with_the_same_strength()
     {
        let settings = DockingSettings::default();
        let (first, second, first_pose, docked) = a_docked_pair();
        // The second tilted 6 degrees about the vertical axis: it is to be turned back about the vertical, the other way.
        let angle = 6.0f32.to_radians();
        let tilt = tilted(docked.orientation, [0.0, 1.0, 0.0], angle);
        let (one, two) = views(
            (first, first_pose),
            (second, GridPose::new(docked.position, tilt)),
        );
        let how = approach(&one, &two);
        assert!(
            (how.twist_angle() - f64::from(angle)).abs() < 1e-4,
            "{how:?}"
        );
        let on_second = settings.twist(&how);
        // 6 of the 30 degrees to full strength: a fifth of the torque, about the vertical, against the tilt.
        assert!(
            close(
                on_second.map(|component| component / settings.twist_torque),
                [0.0, -0.2, 0.0]
            ),
            "{on_second:?}"
        );
        // Whatever the size of the tilt, the torque is no more than the whole.
        let big = tilted(docked.orientation, [0.0, 1.0, 0.0], 80.0f32.to_radians());
        let (one, two) = views(
            (first, first_pose),
            (second, GridPose::new(docked.position, big)),
        );
        let size = length(settings.twist(&approach(&one, &two)));
        assert!((size - settings.twist_torque).abs() < 1e-3, "{size}");
        // No tilt, no torque.
        let (one, two) = views((first, first_pose), (second, docked));
        assert!(length(settings.twist(&approach(&one, &two))) < 1e-6);
    }

    #[test]
    fn two_ports_are_pulled_together_by_the_same_force_whatever_the_distance_and_not_when_they_do_not_face_each_other()
     {
        let settings = DockingSettings::default();
        let first_pose = pose(0.0, 0.0, 0.0, Orientation::IDENTITY);
        let (first, second) = (port(0, 0, 0, EAST), port(0, 0, 0, WEST));
        for gap in [0.5f32, 1.0, 3.0, 5.9] {
            // The second ship's west face is `gap` metres along x from the first's east face (which is at x = 1).
            let second_pose = pose(1.0 + gap, 0.0, 0.0, Orientation::IDENTITY);
            let (one, two) = views((first, first_pose), (second, second_pose));
            let how = approach(&one, &two);
            assert!((how.distance - f64::from(gap)).abs() < 1e-5);
            assert!(settings.pulls(&one, &two, &how), "gap {gap}");
            let on_first = settings.attraction(&how);
            assert!(
                close(
                    on_first.map(|component| component / settings.attraction_force),
                    [1.0, 0.0, 0.0]
                ),
                "{on_first:?}"
            );
        }
        let pulls = |second_port: Port, second_pose: GridPose| {
            let (one, two) = views((first, first_pose), (second_port, second_pose));
            settings.pulls(&one, &two, &approach(&one, &two))
        };
        // Out of range; behind the first's face (on the side it does not look to); the faces looking the same way.
        assert!(!pulls(second, pose(8.0, 0.0, 0.0, Orientation::IDENTITY)));
        assert!(!pulls(second, pose(-3.0, 0.0, 0.0, Orientation::IDENTITY)));
        assert!(!pulls(
            port(0, 0, 0, EAST),
            pose(3.0, 0.0, 0.0, Orientation::IDENTITY)
        ));
        // Faces at one place pull not at all (there is no direction), and can snap.
        let (_, _, _, docked) = a_docked_pair();
        let (one, two) = views((first, first_pose), (second, docked));
        let how = approach(&one, &two);
        assert_eq!(settings.attraction(&how), [0.0; 3]);
        assert!(settings.can_snap(&how));
    }

    /// Two views that are only a place and a way to look (what `pulls` reads), and the approach between them.
    fn bare(
        first_normal: [f64; 3],
        second_normal: [f64; 3],
        gap: [f64; 3],
    ) -> (PortView, PortView, Approach) {
        let view = |normal: [f64; 3], centre: [f64; 3]| PortView {
            port: port(0, 0, 0, EAST),
            centre,
            normal,
            rotation: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        };
        let how = Approach {
            gap,
            distance: length(gap),
            turn: LatticeRotation::IDENTITY,
            twist_axis: [0.0; 3],
            twist_cosine: 1.0,
        };
        (view(first_normal, [0.0; 3]), view(second_normal, gap), how)
    }

    #[test]
    fn each_of_the_conditions_for_pulling_keeps_ports_apart_when_it_alone_is_not_met() {
        let settings = DockingSettings::default();
        let (east, steep) = ([1.0, 0.0, 0.0], [-0.5, 0.866_025_4, 0.0]);
        let pulls = |first: [f64; 3], second: [f64; 3], gap: [f64; 3]| {
            let (one, two, how) = bare(first, second, gap);
            settings.pulls(&one, &two, &how)
        };
        // All met: the first looks along +x at a second that is ahead of it and looks back (not exactly, but against it).
        assert!(pulls(east, steep, [1.0, -3.0, 0.0]));
        // Only the first looks away from the second (the second is behind the first's face and looks back at it from the side).
        assert!(!pulls(east, steep, [-1.0, -3.0, 0.0]));
        // Only the second looks away from the first (it is ahead of the first, and faces away).
        assert!(!pulls(east, steep, [1.0, 3.0, 0.0]));
        // Only the two do not look against each other (both look off to the side the other is on).
        assert!(!pulls(
            [1.0, 0.0, 0.0],
            [0.5, 0.866_025_4, 0.0],
            [1.0, -3.0, 0.0]
        ));
        // Only out of range.
        assert!(!pulls(east, steep, [1.0, -7.0, 0.0]));
    }

    #[test]
    fn a_force_away_from_the_centre_turns_a_body_about_the_axis_arm_cross_force_in_the_worlds_axes()
    {
        let upright = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        // One metre up from the centre, pushed along x: the torque is arm (0, 1, 0) crossed with force (5, 0, 0).
        assert_eq!(
            torque_of_force(upright, [0.0, 1.0, 0.0], [5.0, 0.0, 0.0]),
            [0.0, 0.0, -5.0]
        );
        // On the centre: none.
        assert_eq!(
            torque_of_force(upright, [0.0; 3], [5.0, 2.0, 1.0]),
            [0.0; 3]
        );
        // Turned a quarter about z (its y along the world's +x): the arm is along +x, and a push along x is on the axis.
        let quarter = [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
        assert_eq!(
            torque_of_force(quarter, [0.0, 1.0, 0.0], [5.0, 0.0, 0.0]),
            [0.0; 3]
        );
        // And pushed along y: arm (1, 0, 0) crossed with force (0, 5, 0).
        assert_eq!(
            torque_of_force(quarter, [0.0, 1.0, 0.0], [0.0, 5.0, 0.0]),
            [0.0, 0.0, 5.0]
        );
    }

    #[test]
    fn ports_snap_within_three_tenths_of_a_metre_and_ten_degrees_and_not_beyond_either() {
        let settings = DockingSettings::default();
        let (first, second, first_pose, docked) = a_docked_pair();
        let try_it = |away: f32, degrees: f32| {
            let moved = GridPose::new(
                WorldPosition::new(BrickCoordinate::ORIGIN, [1.0 + away, 0.0, 0.0]),
                tilted(docked.orientation, [0.0, 0.0, 1.0], degrees.to_radians()),
            );
            let (one, two) = views((first, first_pose), (second, moved));
            settings.can_snap(&approach(&one, &two))
        };
        assert!(try_it(0.29, 0.0) && try_it(0.0, 9.9) && try_it(0.29, 9.9));
        assert!(!try_it(0.31, 0.0) && !try_it(0.0, 10.1) && !try_it(0.31, 10.1));
    }
}
