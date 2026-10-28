use std::collections::{BTreeMap, HashMap};
use std::num::NonZeroU64;

use block_models::{BlockDefinition, BlockTable, Shape, ShapeBox};
use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use region_octree::BrickCube;
use voxel_grid::ChannelStorage;
use voxel_meshing::{
    BrickChannels, BrickNeighbourhood, PackedBrickMesh, PackedDetailedQuad, PackedQuad, mesh_brick,
};
use voxel_space::{
    BRICK_VOXEL_COUNT, BrickCoordinate, GridId, GridPose, LocalVoxelIndex, WorldPosition,
};
use wgpu::util::DeviceExt;

use crate::camera::{Camera, Frustum};
use crate::crosshair::Crosshair;
use crate::graphics_context::GraphicsContext;
use crate::overlay::{Overlay, OverlayRect};
use crate::shadow::{Cascade, LightBasis, MAXIMUM_CASCADES, SUN_DRIFT, half_widths};
use crate::shadow_maps::{ShadowPipelines, ShadowTarget, comparison_sampler, shadow_frame_layout};
use crate::sun::{
    MAXIMUM_SUN_BALLS, SUN_DISC_SOFTNESS, SunOccluder, TWILIGHT_SOFTNESS, balls_and_ground,
    length_and_direction,
};

pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Radius of a sphere around a brick: half the diagonal of a 32-voxel cube.
const BRICK_BOUNDING_RADIUS: f32 = 27.72;
const BRICK_CENTER_OFFSET: f32 = 16.0;
/// How many quads of shadow maps may be drawn again in a frame to keep them up to date (a map that is wrong is always drawn again, and one map is always allowed): a cascade
/// keeps its map while the camera stays near the middle of it, and drawing the terrain of a whole picture is most of what shadows cost.
const REFRESH_BUDGET_QUADS: usize = 1_500_000;
/// What the calls that draw one thing into a shadow map cost, in quads (see [`REFRESH_BUDGET_QUADS`]).
const SHADOW_CALL_COST_QUADS: usize = 300;
/// Things that move (other players, boxes, ships) are drawn into the maps whose texels are this wide, in voxels, or less: a figure of two voxels is no shadow in a cascade of
/// texels of several, and a map that held it would have to be drawn again whenever it moved.
const DYNAMIC_CASTER_TEXEL_LIMIT: f32 = 1.0;

/// How many frames a map of terrain that has changed (a brick came or went, or was edited) is kept before it is drawn again: none for the fine cascades, so that a block that
/// is placed has its shadow at once, and more for each coarser, which have more terrain to draw and shadows that a texel's change of would not be seen in.
fn terrain_refresh_age(index: usize) -> u64 {
    1 << index.saturating_sub(2)
}

/// A placement is the position of the brick's corner relative to the camera (a `vec4`) and the three columns of the turn of the grid it
/// is in (a `mat3x3`, each column padded to a `vec4`).
const PLACEMENT_SIZE: u64 = 64;

/// How many different avatar colours there are. Their material values are the last of all the values a quad can
/// carry ([`AVATAR_MATERIAL_BASE`] and the 15 after it), which no game material can reach, so games have every
/// value below [`MAXIMUM_GAME_MATERIALS`].
pub const AVATAR_COLOR_COUNT: usize = 16;
/// The material value of the first avatar colour. The shader knows it too (`AVATAR_BASE` in `voxel.wgsl`).
pub(crate) const AVATAR_MATERIAL_BASE: u32 =
    voxel_meshing::MAXIMUM_MATERIAL - AVATAR_COLOR_COUNT as u32 + 1;
/// How many materials a game may have: a block and a colour for each state of each block that has colours. They are in a
/// buffer of the GPU of 16 bytes each (16 MiB at most), so the number is a choice, not a limit of the picture's format
/// (which has room for 16 million).
pub const MAXIMUM_GAME_MATERIALS: usize = 1 << 20;
/// sRGB, chosen to be told apart from each other and from the usual terrain colours.
const AVATAR_COLORS: [[f32; 3]; AVATAR_COLOR_COUNT] = [
    [1.00, 0.20, 0.20],
    [0.20, 0.45, 1.00],
    [1.00, 0.85, 0.10],
    [0.75, 0.25, 0.95],
    [1.00, 0.55, 0.05],
    [0.10, 0.90, 0.90],
    [1.00, 0.45, 0.75],
    [0.95, 0.95, 0.95],
    [0.10, 0.10, 0.12],
    [0.55, 0.95, 0.15],
    [0.55, 0.30, 0.10],
    [0.30, 0.30, 0.95],
    [0.90, 0.10, 0.50],
    [0.20, 0.65, 0.55],
    [0.65, 0.65, 0.20],
    [0.50, 0.50, 0.60],
];
/// An avatar's mesh is a voxel model in a brick; this local voxel is where the player's eyes are.
const AVATAR_EYE_VOXEL: u8 = 16;
/// Radius of a sphere around an avatar, for culling.
const AVATAR_BOUNDING_RADIUS: f32 = 2.0;

/// Another player, drawn where their eyes are: a head over a body, in a colour of their own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Avatar {
    /// Where the player's eyes are.
    pub position: WorldPosition,
    /// Which of the [`AVATAR_COLOR_COUNT`] colours; larger numbers wrap around.
    pub color: u32,
}

/// Radius of a sphere around an entity's box, for culling.
const ENTITY_BOX_BOUNDING_RADIUS: f32 = 1.0;

/// An entity drawn as a box: a thing that is not a player, and has no model yet (an item on the ground).
/// Boxes are placed like avatars, and made like blocks that do not fill their cell, so their sizes are in
/// eighths of a voxel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntityBox {
    /// The middle of the box.
    pub position: WorldPosition,
    /// How big the box is along each axis, in eighths of a voxel. Each is made even and from 2 to 8, so that
    /// the box can be centred on `position`.
    pub size: [u8; 3],
    /// The palette value it is drawn in (the same values the blocks of the world are drawn in); the values
    /// the avatars use are not for boxes.
    pub material: u32,
}

impl EntityBox {
    /// What the box is made of, as the renderer remembers its mesh: the size made even and put from 2 to 8,
    /// and the material put among the game's.
    fn key(&self) -> (u32, [u8; 3]) {
        let material = self.material.clamp(1, MAXIMUM_GAME_MATERIALS as u32 - 1);
        (
            material,
            self.size.map(|size| size.clamp(1, 8).div_ceil(2) * 2),
        )
    }
}

/// The mesh of a box of one material and size: a voxel at the middle of a brick whose shape is a box.
fn box_mesh(material: u32, size: [u8; 3]) -> PackedBrickMesh {
    let mut values = vec![0u32; BRICK_VOXEL_COUNT];
    let at = LocalVoxelIndex::new(AVATAR_EYE_VOXEL, AVATAR_EYE_VOXEL, AVATAR_EYE_VOXEL);
    values[at.index()] = 1;
    let storage = ChannelStorage::from_values(&values);
    let shape = Shape::from_boxes(&[ShapeBox {
        min: size.map(|size| (8 - size) / 2),
        max: size.map(|size| (8 + size) / 2),
    }]);
    let solid = BlockDefinition::cube([0.5; 3]).with_shapes(vec![shape]);
    let air = BlockDefinition::air();
    let definitions: Vec<Option<&BlockDefinition>> = vec![Some(&air), Some(&solid)];
    with_material(
        mesh_brick(
            &BrickNeighbourhood {
                center: BrickChannels::blocks_only(&storage),
                neighbours: [None; 6],
            },
            &BlockTable::from_definitions(&definitions),
        )
        .pack(),
        material,
    )
}

/// A palette as the shader reads it: linear RGB (sRGB in), and one magenta entry if there are none, since a buffer is not empty.
fn linear_palette(colors: &[[f32; 3]]) -> Vec<[f32; 4]> {
    let mut linear: Vec<[f32; 4]> = colors
        .iter()
        .map(|color| {
            let [red, green, blue] = srgb_to_linear_color(*color);
            [red, green, blue, 1.0]
        })
        .collect();
    if linear.is_empty() {
        linear.push([1.0, 0.0, 1.0, 1.0]);
    }
    linear
}

/// The same mesh with every quad given this material value, whatever it had. (A mesh is made with a small block number, not with
/// a table that has a block for each of the million values there can be.)
fn with_material(mut mesh: PackedBrickMesh, material: u32) -> PackedBrickMesh {
    const MATERIAL_BITS: u32 = 0x00FF_FFFF;
    for quad in &mut mesh.quads {
        quad.material = quad.material & !MATERIAL_BITS | material;
    }
    for quad in &mut mesh.detailed {
        quad.words[3] = quad.words[3] & !MATERIAL_BITS | material;
    }
    mesh
}

/// The mesh of an avatar of one colour: two voxels, the head with the eyes in it over the body.
fn avatar_mesh(color: usize) -> PackedBrickMesh {
    let mut values = vec![0u32; BRICK_VOXEL_COUNT];
    for y in [AVATAR_EYE_VOXEL - 1, AVATAR_EYE_VOXEL] {
        let at = LocalVoxelIndex::new(AVATAR_EYE_VOXEL, y, AVATAR_EYE_VOXEL);
        values[at.index()] = 1;
    }
    let storage = ChannelStorage::from_values(&values);
    // The mesh is made of block 1 (an opaque cube, air being 0) and given the avatar's material value.
    with_material(
        mesh_brick(
            &BrickNeighbourhood {
                center: BrickChannels::blocks_only(&storage),
                neighbours: [None; 6],
            },
            &BlockTable::of_cubes(&[false, true]),
        )
        .pack(),
        AVATAR_MATERIAL_BASE + color as u32,
    )
}

/// Colours are given in sRGB, as in an image editor; the renderer converts them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Atmosphere {
    pub sky_color: [f32; 3],
    /// Distance in voxels where distant terrain starts to fade into the sky.
    pub fog_start: f32,
    /// Distance in voxels where it has fully faded.
    pub fog_end: f32,
    /// Direction towards the sun (need not be normalized).
    pub sun_direction: [f32; 3],
    /// The direction the sides of faces are shaded by, if it is not the sun's: a face is a little brighter the more it looks along it, whatever lights it (the lamps and
    /// the ambient light; the sun lights by its own direction). `None`, the default, is the sun's direction. A world whose sun moves sets it, so that a lamp lights a
    /// wall as it did at every time of the day.
    pub side_direction: Option<[f32; 3]>,
    /// How bright, from 0 to 255, a face is that no light reaches. A face's brightness is its light
    /// and this one combined like screen blending in an image editor (each as a fraction of full
    /// brightness, one minus the product of what is missing from each), so light adds to the
    /// ambient and never goes beyond full. 0, the default, is no ambient light: brightness is the
    /// face's light alone.
    pub ambient_light: u8,
    /// How bright, from 0 to 255, the sun makes a face that looks straight at it and that nothing is in the way of (see [`VoxelRenderer::set_sun_occluders`]): by the
    /// cosine of the angle between the face and `sun_direction`. It adds to the face's light and the ambient light like another screen blend. 0, the default, is a world
    /// with no sun: its pictures are as they were.
    pub sun_light: u8,
    /// How bright, from 0 to 255, the sky makes every face the sun can reach, from whichever side (a face on the side away from the sun is shaded as ever). The sky is
    /// the sun's light scattered, so it goes where the sun does: a place the sun does not reach has none. 0 is none.
    pub sky_light: u8,
    /// Whether the terrain and everything drawn casts shadows (see [`crate::shadow`]): the sun lights only what it reaches, and the sky's light is cut to
    /// [`Atmosphere::sky_light_in_shadow`] where it does not. The renderer draws the shadow maps for it (they cost a second drawing of what is near the sun's way,
    /// finest for what is near the camera), and only while there is a sun or a sky light to shade. `false`, the default, is no shadows, and pictures as they were.
    pub shadows: bool,
    /// The share, from 0 to 255, of the sky's light that reaches a place the sun does not (one in the shadow of the terrain, or a face turned from the sun), while
    /// [`Atmosphere::shadows`] are on: the sky is scattered light, so a shadow is not black, but it is not told how much of the sky is hidden, which only a cave knows. 255, the
    /// default, is no darkening at all.
    pub sky_light_in_shadow: u8,
}

impl Default for Atmosphere {
    fn default() -> Self {
        Self {
            sky_color: [0.55, 0.72, 0.92],
            fog_start: 400.0,
            fog_end: 900.0,
            sun_direction: [0.4, 0.8, 0.3],
            side_direction: None,
            ambient_light: 0,
            sun_light: 0,
            sky_light: 0,
            shadows: false,
            sky_light_in_shadow: 255,
        }
    }
}

/// What a frame drew.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderStatistics {
    /// Bricks and coarse nodes drawn.
    pub bricks_drawn: usize,
    /// Of those, the coarse nodes.
    pub coarse_nodes_drawn: usize,
    pub bricks_culled: usize,
    /// Whole-voxel quads and detailed quads together.
    pub quads_drawn: usize,
    /// Of those, the detailed ones.
    pub detailed_quads_drawn: usize,
    pub avatars_drawn: usize,
    pub entity_boxes_drawn: usize,
    /// How many cascades of the shadow map were drawn again this frame (0 while there are no shadows, or no sun to cast them, and while the maps are kept), and how many things
    /// were drawn into them in all.
    pub shadow_cascades_drawn: usize,
    pub shadow_casters_drawn: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FrameUniform {
    view_projection: [[f32; 4]; 4],
    fog_color: [f32; 4],
    fog_range: [f32; 4],
    sun_direction: [f32; 4],
    side_direction: [f32; 4],
    /// x: the sun's light, y: the sky's (0 to 1), z: 1 if the level's ground is flat, w: how many balls follow.
    sun_light: [f32; 4],
    /// The balls that can hide the sun: the middle relative to the camera and the radius.
    sun_balls: [[f32; 4]; MAXIMUM_SUN_BALLS],
    /// x: how many cascades there are (0: no shadows), y: the share of the sky's light that reaches a place in a shadow.
    shadow: [f32; 4],
    cascades: [CascadeUniform; MAXIMUM_CASCADES],
    /// Linear RGB of each avatar colour.
    avatars: [[f32; 4]; AVATAR_COLOR_COUNT],
}

/// A cascade of the shadow map as the shader has it: the matrix from a place relative to the camera to the map, how far from the camera the cascade is looked in,
/// the width of a texel in voxels and the depth range in voxels.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CascadeUniform {
    matrix: [[f32; 4]; 4],
    limits: [f32; 4],
}

/// The meshes of a brick on the GPU: whole-voxel quads, and detailed ones if it has any. The quads are in the order of the faces they are on (a shadow map is drawn from
/// the faces that look away from the sun, which are a few of the six).
struct GpuBrick {
    quads: Option<wgpu::Buffer>,
    quad_count: u32,
    /// Where the quads of each face begin: those of face `f` are `quad_faces[f]..quad_faces[f + 1]`.
    quad_faces: [u32; 7],
    detailed: Option<wgpu::Buffer>,
    detailed_count: u32,
    detailed_faces: [u32; 7],
}

impl GpuBrick {
    fn quad_total(&self) -> usize {
        (self.quad_count + self.detailed_count) as usize
    }

    /// The ranges of whole quads, and of detailed ones, on the faces that look away from the sun when the brick is turned by `rotation` (the sun's direction is a unit
    /// vector, in the world): what a shadow map is drawn from. Neighbouring faces' ranges are joined.
    fn back_faces(
        faces: &[u32; 7],
        rotation: &[[f32; 3]; 3],
        toward_sun: Vec3,
    ) -> Vec<std::ops::Range<u32>> {
        let mut ranges: Vec<std::ops::Range<u32>> = Vec::new();
        for face in 0..6 {
            let (axis, sign) = (face / 2, if face % 2 == 1 { 1.0 } else { -1.0 });
            let normal = Vec3::new(rotation[0][axis], rotation[1][axis], rotation[2][axis]) * sign;
            let range = faces[face]..faces[face + 1];
            if normal.dot(toward_sun) >= 0.0 || range.is_empty() {
                continue;
            }
            match ranges.last_mut() {
                Some(last) if last.end == range.start => last.end = range.end,
                _ => ranges.push(range),
            }
        }
        ranges
    }

    /// How many quads a shadow map from this sun draws of the brick placed with this turn.
    fn shadow_quads(&self, rotation: &[[f32; 3]; 3], toward_sun: Vec3) -> usize {
        [&self.quad_faces, &self.detailed_faces]
            .into_iter()
            .flat_map(|faces| Self::back_faces(faces, rotation, toward_sun))
            .map(|range| range.len())
            .sum()
    }
}

/// Where the things on each face begin, for things in the order of their faces (0 to 5): the number of things before the first of each face, and the total after the last.
fn face_starts(faces: impl Iterator<Item = u32>) -> [u32; 7] {
    let mut starts = [0u32; 7];
    for face in faces {
        starts[face as usize + 1] += 1;
    }
    for face in 0..6 {
        starts[face + 1] += starts[face];
    }
    starts
}

/// Where one brick, avatar or box is drawn: the position of its corner relative to the camera, and the turn of what it is part of.
#[derive(Clone, Copy)]
struct Placement {
    offset: Vec3,
    /// Voxels to a unit of the mesh: 1 for a brick, `2^n` for a coarse node of detail level `n`.
    scale: f32,
    /// `rotation[row][column]`; the identity for what is not part of a ship.
    rotation: [[f32; 3]; 3],
}

const NO_TURN: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// The width in voxels of a cell of a node of detail `level`, 2^level, as the exact `f32` it is (levels up to 116 are far inside the range of an `f32`, which
/// reaches 2^127; a shift of an integer would not fit from level 64).
fn cell_scale(level: u8) -> f32 {
    assert!(level <= 127, "2^{level} is not an f32");
    f32::from_bits((127 + u32::from(level)) << 23)
}

/// The sphere that holds a node whose lowest corner is at `corner` (relative to the camera) and whose mesh is in units of `scale` voxels: its middle
/// and its radius, which are those of a brick times the scale.
fn node_sphere(corner: Vec3, scale: f32) -> (Vec3, f32) {
    (
        corner + Vec3::splat(BRICK_CENTER_OFFSET * scale),
        BRICK_BOUNDING_RADIUS * scale,
    )
}

/// A brick the renderer holds: one of the terrain, or one of a ship.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum BrickKey {
    Terrain(BrickCoordinate),
    Ship(GridId, BrickCoordinate),
    /// A coarse node of the terrain or of a ship (detail level 1 or more): cells of `2^n` voxels.
    CoarseTerrain(BrickCube),
    CoarseShip(GridId, BrickCube),
}

/// The turn and place of a ship for one frame, as the camera sees it.
struct ShipFrame {
    rotation: [[f64; 3]; 3],
    /// The vector from the camera to the ship's origin.
    origin: [f64; 3],
}

/// What a placement draws: a brick or node the renderer holds, an avatar's mesh, or a box's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MeshKey {
    Held(BrickKey),
    Avatar(usize),
    Box((u32, [u8; 3])),
}

/// One thing that may be drawn this frame: what it is, where it is placed, and the sphere that holds it (relative to the camera), for deciding whether the camera sees it and
/// whether a shadow map wants it.
struct Drawable {
    mesh: MeshKey,
    placement: Placement,
    middle: Vec3,
    radius: f32,
    /// Whether it moves (a ship, an avatar, a box) and not the terrain: the maps that keep their picture for a while hold only the terrain.
    dynamic: bool,
}

/// Every thing that may be drawn this frame: the bricks and nodes first (`pieces` of them), then the avatars (up to `avatars_end`), then the boxes.
struct Gathered {
    drawables: Vec<Drawable>,
    pieces: usize,
    avatars_end: usize,
}

/// How a kept map stands: what has become of the camera, the sun and the world since it was drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Standing {
    /// Whether the map still holds everything within its reach of the camera.
    holds: bool,
    /// The sine of the angle the sun has turned since (infinite if it has turned over).
    turned: f32,
    /// Whether the terrain has changed since, and (for a cascade fine enough to hold them) the things that move.
    terrain_changed: bool,
    things_changed: bool,
    /// How many frames ago it was drawn.
    waited: u64,
}

impl Standing {
    /// Whether the map is wrong, not just out of date: it does not hold what it must, or the sun has turned too far (ten times what is allowed to be a little).
    fn is_wrong(&self) -> bool {
        !self.holds || self.turned > 10.0 * SUN_DRIFT
    }
}

/// Whether the map of cascade `index`, which something in the picture is looked up in, is to be drawn again, given how the map that is kept stands (if there is one), how many
/// quads of maps have been drawn again in this frame already, and (asked for only when it matters) how many quads it takes to draw this one: always if there is none or it
/// is wrong; if it is only out of date, when it has waited its time (see [`terrain_refresh_age`]), for as many frames as it has times the budget in quads to draw, and there is
/// room in the frame (the first map drawn again in a frame is always allowed).
fn draw_again(
    standing: Option<Standing>,
    index: usize,
    spent: usize,
    cost: impl FnOnce() -> usize,
) -> bool {
    let Some(standing) = standing else {
        return true;
    };
    if standing.is_wrong() {
        return true;
    }
    let changed = standing.terrain_changed || standing.things_changed;
    if !changed && standing.turned <= SUN_DRIFT {
        return false;
    }
    let wait = if changed {
        terrain_refresh_age(index)
    } else {
        1
    };
    if standing.waited < wait {
        return false;
    }
    let cost = cost();
    let affordable = spent == 0 || spent + cost <= REFRESH_BUDGET_QUADS;
    affordable && standing.waited >= cost.div_ceil(REFRESH_BUDGET_QUADS) as u64
}

/// A cascade's map as it was drawn last, kept while it still serves.
struct CascadeCache {
    cascade: Cascade,
    basis: LightBasis,
    /// Where the camera was.
    camera: WorldPosition,
    /// What the renderer held then: the terrain, and the things that move.
    terrain_version: u64,
    dynamic_version: u64,
    /// The frame it was drawn in.
    frame: u64,
}

/// A cascade of the shadow map for this frame, and what is drawn into it.
struct PlannedCascade {
    /// The cascade as its map was drawn (this frame, or before), and the sun's frame it was drawn in.
    cascade: Cascade,
    basis: LightBasis,
    /// The camera's place relative to where it was when the map was drawn (nothing if it is drawn this frame).
    moved: Vec3,
    /// Whether the pictures look places up in it this frame: there is a map to look in that is right for the camera.
    used: bool,
    /// Whether its map is drawn this frame; if not, what it holds is kept.
    drawn: bool,
    /// The things drawn into it, by index in the frame's drawables (none if it is not drawn).
    casters: Vec<usize>,
}

/// The shadow maps of this frame.
struct ShadowPlan {
    cascades: Vec<PlannedCascade>,
}

/// The places in the placements buffer of the things drawn this frame: each thing drawn has one, however many passes it is drawn in.
struct Slots {
    slot_of: Vec<Option<u32>>,
    placements: Vec<Placement>,
}

impl Slots {
    fn new(count: usize) -> Self {
        Self {
            slot_of: vec![None; count],
            placements: Vec::new(),
        }
    }

    /// The slot of a drawable, given one if it has none.
    fn slot(&mut self, drawables: &[Drawable], index: usize) -> u32 {
        *self.slot_of[index].get_or_insert_with(|| {
            self.placements.push(drawables[index].placement);
            self.placements.len() as u32 - 1
        })
    }

    fn get(&self, index: usize) -> u32 {
        self.slot_of[index].expect("a drawable that is drawn has a slot")
    }
}

struct PlacementBuffer {
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    capacity: usize,
}

struct SizedTarget {
    size: (u32, u32),
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

/// Draws bricks of meshed voxels. Positions are camera-relative: each brick's offset from the
/// camera is computed in exact integer arithmetic, so precision does not degrade far from the
/// origin.
pub struct VoxelRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    color_format: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    detailed_pipeline: wgpu::RenderPipeline,
    frame_layout: wgpu::BindGroupLayout,
    /// The frame's uniform for the picture, and after it one copy for each cascade of the shadow map, each at a multiple of `frame_stride`.
    frame_buffer: wgpu::Buffer,
    frame_stride: u64,
    /// Linear RGB of every game material, which the shader reads by material value.
    palette_buffer: wgpu::Buffer,
    frame_bind_group: wgpu::BindGroup,
    /// The shadow maps: drawn into when there are shadows, looked up in by the pictures.
    shadow_target: ShadowTarget,
    shadow_sampler: wgpu::Sampler,
    shadow_pipelines: ShadowPipelines,
    shadow_frame_group: wgpu::BindGroup,
    placement_layout: wgpu::BindGroupLayout,
    placements: PlacementBuffer,
    placement_stride: u64,
    bricks: HashMap<BrickCoordinate, GpuBrick>,
    /// The bricks of ships, in the ships' own coordinates, and where each ship is. A ship without a pose is not drawn.
    ship_bricks: HashMap<(GridId, BrickCoordinate), GpuBrick>,
    /// The coarse nodes (detail level 1 or more) of the terrain, and of ships in their own coordinates.
    coarse_nodes: HashMap<BrickCube, GpuBrick>,
    ship_coarse_nodes: HashMap<(GridId, BrickCube), GpuBrick>,
    ship_poses: BTreeMap<GridId, GridPose>,
    avatar_meshes: Vec<GpuBrick>,
    avatars: Vec<Avatar>,
    entity_boxes: Vec<EntityBox>,
    /// The meshes of the boxes there have been, by material and size, made when first asked for.
    box_meshes: HashMap<(u32, [u8; 3]), GpuBrick>,
    atmosphere: Atmosphere,
    sun_occluders: Vec<SunOccluder>,
    /// How many times the terrain held changed (a brick or node came, went or was replaced), and the things that move (ships, avatars, boxes): what a kept map is held
    /// against, to know that it is out of date.
    terrain_version: u64,
    dynamic_version: u64,
    /// How many pictures have been drawn with shadows, and the maps as they were last drawn, one for each cascade.
    shadow_frame: u64,
    shadow_cache: Vec<Option<CascadeCache>>,
    depth: Option<SizedTarget>,
    offscreen: Option<SizedTarget>,
    crosshair: Crosshair,
    crosshair_enabled: bool,
    overlay: Overlay,
}

impl std::fmt::Debug for VoxelRenderer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VoxelRenderer")
            .field("color_format", &self.color_format)
            .field("brick_count", &self.bricks.len())
            .field("quad_count", &self.total_quad_count())
            .finish()
    }
}

// sRGB to linear needs a power function; this is client-side cosmetics, not simulation.
#[allow(clippy::disallowed_methods)]
pub(crate) fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn srgb_to_linear_color(color: [f32; 3]) -> [f32; 3] {
    color.map(srgb_to_linear)
}

impl VoxelRenderer {
    /// `color_format` must be an sRGB format (the shader outputs linear colour).
    pub fn new(context: &GraphicsContext, color_format: wgpu::TextureFormat) -> Self {
        assert!(
            color_format.is_srgb(),
            "the renderer needs an sRGB colour format"
        );
        let device = context.device.clone();
        let queue = context.queue.clone();

        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(size_of::<FrameUniform>() as u64),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(16),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });
        let placement_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("placement layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: NonZeroU64::new(PLACEMENT_SIZE),
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("voxel pipeline layout"),
            bind_group_layouts: &[Some(&frame_layout), Some(&placement_layout)],
            immediate_size: 0,
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("voxel shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("voxel.wgsl").into()),
        });
        let make_pipeline =
            |label: &str, entry_point: &str, vertex_buffer: wgpu::VertexBufferLayout<'_>| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some(entry_point),
                        compilation_options: Default::default(),
                        buffers: &[Some(vertex_buffer)],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("fragment_main"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: color_format,
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        front_face: wgpu::FrontFace::Ccw,
                        cull_mode: Some(wgpu::Face::Back),
                        ..Default::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: DEPTH_FORMAT,
                        depth_write_enabled: Some(true),
                        depth_compare: Some(wgpu::CompareFunction::Greater),
                        stencil: wgpu::StencilState::default(),
                        bias: wgpu::DepthBiasState::default(),
                    }),
                    multisample: wgpu::MultisampleState::default(),
                    multiview_mask: None,
                    cache: None,
                })
            };
        let pipeline = make_pipeline(
            "voxel pipeline",
            "vertex_main",
            wgpu::VertexBufferLayout {
                array_stride: size_of::<PackedQuad>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![0 => Uint32, 1 => Uint32],
            },
        );
        let detailed_pipeline = make_pipeline(
            "detailed voxel pipeline",
            "vertex_detailed",
            wgpu::VertexBufferLayout {
                array_stride: size_of::<PackedDetailedQuad>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![0 => Uint32, 1 => Uint32, 2 => Uint32, 3 => Uint32],
            },
        );

        let alignment = u64::from(device.limits().min_uniform_buffer_offset_alignment);
        let frame_stride = (size_of::<FrameUniform>() as u64).next_multiple_of(alignment);
        let frame_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame uniform"),
            size: frame_stride * (1 + MAXIMUM_CASCADES as u64),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let palette_buffer = Self::palette_buffer(&device, &[]);
        let shadow_target = ShadowTarget::new(&device, false);
        let shadow_sampler = comparison_sampler(&device);
        let frame_bind_group = Self::frame_bind_group(
            &device,
            &frame_layout,
            &frame_buffer,
            &palette_buffer,
            &shadow_target,
            &shadow_sampler,
        );
        let shadow_frame_layout = shadow_frame_layout(&device, size_of::<FrameUniform>() as u64);
        let shadow_frame_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow frame bind group"),
            layout: &shadow_frame_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &frame_buffer,
                    offset: 0,
                    size: NonZeroU64::new(size_of::<FrameUniform>() as u64),
                }),
            }],
        });

        let placement_stride = PLACEMENT_SIZE.next_multiple_of(alignment);
        let placements =
            Self::create_placement_buffer(&device, &placement_layout, placement_stride, 64);

        let shadow_pipelines =
            ShadowPipelines::new(&device, &shader, &shadow_frame_layout, &placement_layout);
        let crosshair = Crosshair::new(&device, color_format);
        let device_for_overlay = device.clone();
        let mut renderer = Self {
            device,
            queue,
            color_format,
            pipeline,
            detailed_pipeline,
            frame_layout,
            frame_buffer,
            frame_stride,
            palette_buffer,
            frame_bind_group,
            shadow_target,
            shadow_sampler,
            shadow_pipelines,
            shadow_frame_group,
            placement_layout,
            placements,
            placement_stride,
            bricks: HashMap::new(),
            ship_bricks: HashMap::new(),
            coarse_nodes: HashMap::new(),
            ship_coarse_nodes: HashMap::new(),
            ship_poses: BTreeMap::new(),
            avatar_meshes: Vec::new(),
            avatars: Vec::new(),
            entity_boxes: Vec::new(),
            box_meshes: HashMap::new(),
            atmosphere: Atmosphere::default(),
            sun_occluders: Vec::new(),
            terrain_version: 0,
            dynamic_version: 0,
            shadow_frame: 0,
            shadow_cache: (0..MAXIMUM_CASCADES).map(|_| None).collect(),
            depth: None,
            offscreen: None,
            crosshair,
            crosshair_enabled: false,
            overlay: Overlay::new(&device_for_overlay, color_format),
        };
        renderer.avatar_meshes = (0..AVATAR_COLOR_COUNT)
            .map(|color| renderer.gpu_brick(&avatar_mesh(color)))
            .collect();
        renderer
    }

    /// The GPU's copy of a palette: linear RGB for each colour (sRGB in), and one magenta entry if there are none, since a buffer is
    /// not empty.
    fn palette_buffer(device: &wgpu::Device, colors: &[[f32; 3]]) -> wgpu::Buffer {
        let linear = linear_palette(colors);
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("palette"),
            contents: bytemuck::cast_slice(&linear),
            usage: wgpu::BufferUsages::STORAGE,
        })
    }

    fn frame_bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        frame_buffer: &wgpu::Buffer,
        palette_buffer: &wgpu::Buffer,
        shadow_target: &ShadowTarget,
        shadow_sampler: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame bind group"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: frame_buffer,
                        offset: 0,
                        size: NonZeroU64::new(size_of::<FrameUniform>() as u64),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: palette_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&shadow_target.array_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(shadow_sampler),
                },
            ],
        })
    }

    fn gpu_brick(&self, mesh: &PackedBrickMesh) -> GpuBrick {
        let buffer = |label: &str, words: Vec<u32>| {
            (!words.is_empty()).then(|| {
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some(label),
                        contents: bytemuck::cast_slice(&words),
                        usage: wgpu::BufferUsages::VERTEX,
                    })
            })
        };
        // In the order of their faces (the face is in bits 25 to 27 of the geometry of a quad, and in bits 16 to 18 of the third word of a detailed one), the order within a
        // face kept.
        let quad_face = |quad: &PackedQuad| (quad.geometry >> 25) & 7;
        let detailed_face = |quad: &PackedDetailedQuad| (quad.words[2] >> 16) & 7;
        let mut quads = mesh.quads.clone();
        quads.sort_by_key(quad_face);
        let mut detailed = mesh.detailed.clone();
        detailed.sort_by_key(detailed_face);
        GpuBrick {
            quad_faces: face_starts(quads.iter().map(quad_face)),
            quads: buffer(
                "quads",
                quads
                    .iter()
                    .flat_map(|quad| [quad.geometry, quad.material])
                    .collect(),
            ),
            quad_count: quads.len() as u32,
            detailed_faces: face_starts(detailed.iter().map(detailed_face)),
            detailed: buffer(
                "detailed quads",
                detailed.iter().flat_map(|quad| quad.words).collect(),
            ),
            detailed_count: detailed.len() as u32,
        }
    }

    fn create_placement_buffer(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        stride: u64,
        capacity: usize,
    ) -> PlacementBuffer {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("brick placements"),
            size: stride * capacity as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("placement bind group"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: 0,
                    size: NonZeroU64::new(PLACEMENT_SIZE),
                }),
            }],
        });
        PlacementBuffer {
            buffer,
            bind_group,
            capacity,
        }
    }

    /// Sets the rectangles drawn over every picture from now on, after the crosshair, in order, later over earlier (see
    /// [`OverlayRect`]); none draws nothing. The user interface is drawn with these.
    pub fn set_overlay(&mut self, rects: &[OverlayRect]) {
        self.overlay.set(&self.device, rects);
    }

    /// How many rectangles of the overlay are drawn (those with a size).
    pub fn overlay_rect_count(&self) -> usize {
        self.overlay.count()
    }

    /// Draws a crosshair at the centre of every picture, for aiming. Off by default.
    pub fn set_crosshair(&mut self, enabled: bool) {
        self.crosshair_enabled = enabled;
    }

    /// Sets the colour of material values `0..colors.len()` (sRGB). Larger values draw magenta, which is also how content from a
    /// missing plugin shows up. At most [`MAXIMUM_GAME_MATERIALS`] can be drawn.
    pub fn set_palette(&mut self, colors: &[[f32; 3]]) {
        assert!(
            colors.len() <= MAXIMUM_GAME_MATERIALS,
            "at most {MAXIMUM_GAME_MATERIALS} materials can be drawn"
        );
        self.palette_buffer = Self::palette_buffer(&self.device, colors);
        self.remake_frame_bind_group();
    }

    fn remake_frame_bind_group(&mut self) {
        self.frame_bind_group = Self::frame_bind_group(
            &self.device,
            &self.frame_layout,
            &self.frame_buffer,
            &self.palette_buffer,
            &self.shadow_target,
            &self.shadow_sampler,
        );
    }

    /// Sets which avatars are drawn from now on, replacing the previous ones.
    pub fn set_avatars(&mut self, avatars: &[Avatar]) {
        if self.avatars != avatars {
            self.dynamic_version += 1;
            self.avatars = avatars.to_vec();
        }
    }

    /// Sets which boxes are drawn from now on, replacing the previous ones.
    pub fn set_entity_boxes(&mut self, boxes: &[EntityBox]) {
        for entity_box in boxes {
            let (material, size) = entity_box.key();
            if !self.box_meshes.contains_key(&(material, size)) {
                let mesh = self.gpu_brick(&box_mesh(material, size));
                self.box_meshes.insert((material, size), mesh);
            }
        }
        if self.entity_boxes != boxes {
            self.dynamic_version += 1;
            self.entity_boxes = boxes.to_vec();
        }
    }

    pub fn set_atmosphere(&mut self, atmosphere: Atmosphere) {
        self.atmosphere = atmosphere;
    }

    /// Sets what can stand between a place and the sun, replacing what was set: balls (the planets and moons, as the balls inside them that are ground all through) and
    /// the ground of a level that is flat. The sun lights a place by [`Atmosphere::sun_light`] and [`Atmosphere::sky_light`] in the share that gets past them
    /// ([`crate::sun_visibility`] is the rule, with a soft edge); with none set the sun reaches everywhere. Only the nearest [`MAXIMUM_SUN_BALLS`] balls are used.
    pub fn set_sun_occluders(&mut self, occluders: &[SunOccluder]) {
        self.sun_occluders = occluders.to_vec();
    }

    /// Replaces the mesh of one brick. An empty mesh removes the brick.
    pub fn upload_brick(&mut self, coordinate: BrickCoordinate, mesh: &PackedBrickMesh) {
        self.terrain_version += 1;
        if mesh.is_empty() {
            self.bricks.remove(&coordinate);
            return;
        }
        let brick = self.gpu_brick(mesh);
        self.bricks.insert(coordinate, brick);
    }

    pub fn remove_brick(&mut self, coordinate: BrickCoordinate) {
        self.terrain_version += 1;
        self.bricks.remove(&coordinate);
    }

    pub fn brick_count(&self) -> usize {
        self.bricks.len()
    }

    pub fn has_brick(&self, coordinate: BrickCoordinate) -> bool {
        self.bricks.contains_key(&coordinate)
    }

    /// Whole-voxel quads and detailed quads of every brick held, of the terrain and of ships.
    pub fn total_quad_count(&self) -> usize {
        self.bricks
            .values()
            .chain(self.ship_bricks.values())
            .chain(self.coarse_nodes.values())
            .chain(self.ship_coarse_nodes.values())
            .map(GpuBrick::quad_total)
            .sum()
    }

    /// Replaces the mesh of one brick of a ship, in the ship's own coordinates. An empty mesh removes the brick. The ship is drawn
    /// once it has a pose ([`VoxelRenderer::set_grid_pose`]).
    pub fn upload_grid_brick(
        &mut self,
        grid: GridId,
        coordinate: BrickCoordinate,
        mesh: &PackedBrickMesh,
    ) {
        self.dynamic_version += 1;
        if mesh.is_empty() {
            self.ship_bricks.remove(&(grid, coordinate));
            return;
        }
        let brick = self.gpu_brick(mesh);
        self.ship_bricks.insert((grid, coordinate), brick);
    }

    pub fn remove_grid_brick(&mut self, grid: GridId, coordinate: BrickCoordinate) {
        self.dynamic_version += 1;
        self.ship_bricks.remove(&(grid, coordinate));
    }

    /// Replaces the mesh of a coarse node of the terrain: a cube of bricks of detail level 1 or more (`mesh_coarse_node` makes the mesh, in cells of
    /// `2^level` voxels). An empty mesh removes the node. Panics for level 0, which is a brick (see [`VoxelRenderer::upload_brick`]).
    ///
    /// The renderer draws what it holds and does not choose: a node and the bricks inside it that are both held are both drawn, so whoever selects the
    /// nodes (`coarse_cells::select_nodes`) removes what a node replaces.
    pub fn upload_coarse_node(&mut self, cube: BrickCube, mesh: &PackedBrickMesh) {
        assert!(cube.level() > 0, "a cube of level 0 is a brick");
        self.terrain_version += 1;
        if mesh.is_empty() {
            self.coarse_nodes.remove(&cube);
            return;
        }
        let node = self.gpu_brick(mesh);
        self.coarse_nodes.insert(cube, node);
    }

    pub fn remove_coarse_node(&mut self, cube: BrickCube) {
        self.terrain_version += 1;
        self.coarse_nodes.remove(&cube);
    }

    pub fn coarse_node_count(&self) -> usize {
        self.coarse_nodes.len()
    }

    pub fn has_coarse_node(&self, cube: BrickCube) -> bool {
        self.coarse_nodes.contains_key(&cube)
    }

    /// The same for a coarse node of a ship, in the ship's own coordinates.
    pub fn upload_grid_coarse_node(
        &mut self,
        grid: GridId,
        cube: BrickCube,
        mesh: &PackedBrickMesh,
    ) {
        assert!(cube.level() > 0, "a cube of level 0 is a brick");
        self.dynamic_version += 1;
        if mesh.is_empty() {
            self.ship_coarse_nodes.remove(&(grid, cube));
            return;
        }
        let node = self.gpu_brick(mesh);
        self.ship_coarse_nodes.insert((grid, cube), node);
    }

    pub fn remove_grid_coarse_node(&mut self, grid: GridId, cube: BrickCube) {
        self.dynamic_version += 1;
        self.ship_coarse_nodes.remove(&(grid, cube));
    }

    /// How many coarse nodes of ships are held.
    pub fn grid_coarse_node_count(&self) -> usize {
        self.ship_coarse_nodes.len()
    }

    /// Says where a ship is and how it is turned: its bricks are drawn there from the next picture on.
    pub fn set_grid_pose(&mut self, grid: GridId, pose: GridPose) {
        if self.ship_poses.insert(grid, pose) != Some(pose) {
            self.dynamic_version += 1;
        }
    }

    /// Forgets a ship, its pose and all its bricks.
    pub fn remove_grid(&mut self, grid: GridId) {
        self.dynamic_version += 1;
        self.ship_poses.remove(&grid);
        self.ship_bricks.retain(|(held, _), _| *held != grid);
        self.ship_coarse_nodes.retain(|(held, _), _| *held != grid);
    }

    /// How many bricks of ships are held.
    pub fn grid_brick_count(&self) -> usize {
        self.ship_bricks.len()
    }

    fn brick_of(&self, key: BrickKey) -> &GpuBrick {
        match key {
            BrickKey::Terrain(coordinate) => &self.bricks[&coordinate],
            BrickKey::Ship(grid, coordinate) => &self.ship_bricks[&(grid, coordinate)],
            BrickKey::CoarseTerrain(cube) => &self.coarse_nodes[&cube],
            BrickKey::CoarseShip(grid, cube) => &self.ship_coarse_nodes[&(grid, cube)],
        }
    }

    fn mesh(&self, key: MeshKey) -> &GpuBrick {
        match key {
            MeshKey::Held(held) => self.brick_of(held),
            MeshKey::Avatar(color) => &self.avatar_meshes[color],
            MeshKey::Box(key) => &self.box_meshes[&key],
        }
    }

    /// Every thing that may be drawn this frame, placed relative to the camera: the placement of a brick is computed exactly from integers, then it is only a matter of which
    /// of them the camera sees and which a shadow map wants.
    fn gather(&self, camera: &Camera) -> Gathered {
        let mut drawables: Vec<Drawable> = Vec::new();
        for &coordinate in self.bricks.keys() {
            let corner = WorldPosition::new(coordinate, [0.0; 3]).relative_to(camera.position);
            let corner = Vec3::from_array(corner);
            drawables.push(Drawable {
                mesh: MeshKey::Held(BrickKey::Terrain(coordinate)),
                placement: Placement {
                    offset: corner,
                    scale: 1.0,
                    rotation: NO_TURN,
                },
                middle: corner + Vec3::splat(BRICK_CENTER_OFFSET),
                radius: BRICK_BOUNDING_RADIUS,
                dynamic: false,
            });
        }
        // Coarse nodes of the terrain: a node of detail level `n` is `2^n` bricks along an edge, drawn from its mesh in cells of `2^n` voxels.
        for &cube in self.coarse_nodes.keys() {
            let scale = cell_scale(cube.level());
            let corner = Vec3::from_array(
                WorldPosition::new(cube.origin(), [0.0; 3]).relative_to(camera.position),
            );
            let (middle, radius) = node_sphere(corner, scale);
            drawables.push(Drawable {
                mesh: MeshKey::Held(BrickKey::CoarseTerrain(cube)),
                placement: Placement {
                    offset: corner,
                    scale,
                    rotation: NO_TURN,
                },
                middle,
                radius,
                dynamic: false,
            });
        }
        // A ship's brick is placed by turning its place in the ship and adding the ship's place, in `f64` so that a brick far along a
        // big ship is as exact as one near its origin.
        let frames: BTreeMap<GridId, ShipFrame> = self
            .ship_poses
            .iter()
            .map(|(&grid, pose)| {
                (
                    grid,
                    ShipFrame {
                        rotation: pose.rotation_matrix(),
                        origin: pose.origin_relative_to(camera.position),
                    },
                )
            })
            .collect();
        for &(grid, coordinate) in self.ship_bricks.keys() {
            let Some(frame) = frames.get(&grid) else {
                continue;
            };
            let place = |local: [f64; 3]| -> Vec3 {
                Vec3::from_array(std::array::from_fn(|row| {
                    (frame.origin[row]
                        + frame.rotation[row][0] * local[0]
                        + frame.rotation[row][1] * local[1]
                        + frame.rotation[row][2] * local[2]) as f32
                }))
            };
            let local = [
                coordinate.x() as f64 * 32.0,
                coordinate.y() as f64 * 32.0,
                coordinate.z() as f64 * 32.0,
            ];
            drawables.push(Drawable {
                mesh: MeshKey::Held(BrickKey::Ship(grid, coordinate)),
                placement: Placement {
                    offset: place(local),
                    scale: 1.0,
                    rotation: frame.rotation.map(|row| row.map(|entry| entry as f32)),
                },
                middle: place(local.map(|component| component + f64::from(BRICK_CENTER_OFFSET))),
                radius: BRICK_BOUNDING_RADIUS,
                dynamic: true,
            });
        }
        for &(grid, cube) in self.ship_coarse_nodes.keys() {
            let Some(frame) = frames.get(&grid) else {
                continue;
            };
            let scale = cell_scale(cube.level());
            let place = |local: [f64; 3]| -> Vec3 {
                Vec3::from_array(std::array::from_fn(|row| {
                    (frame.origin[row]
                        + frame.rotation[row][0] * local[0]
                        + frame.rotation[row][1] * local[1]
                        + frame.rotation[row][2] * local[2]) as f32
                }))
            };
            let origin = cube.origin();
            let local = [
                origin.x() as f64 * 32.0,
                origin.y() as f64 * 32.0,
                origin.z() as f64 * 32.0,
            ];
            let half = f64::from(BRICK_CENTER_OFFSET) * f64::from(scale);
            drawables.push(Drawable {
                mesh: MeshKey::Held(BrickKey::CoarseShip(grid, cube)),
                placement: Placement {
                    offset: place(local),
                    scale,
                    rotation: frame.rotation.map(|row| row.map(|entry| entry as f32)),
                },
                middle: place(local.map(|component| component + half)),
                radius: BRICK_BOUNDING_RADIUS * scale,
                dynamic: true,
            });
        }
        let pieces = drawables.len();
        // Avatars are placed like bricks: by where their model's corner is relative to the camera.
        let eye = Vec3::splat(f32::from(AVATAR_EYE_VOXEL) + 0.5);
        for avatar in &self.avatars {
            let relative = Vec3::from_array(avatar.position.relative_to(camera.position));
            drawables.push(Drawable {
                mesh: MeshKey::Avatar(avatar.color as usize % AVATAR_COLOR_COUNT),
                placement: Placement {
                    offset: relative - eye,
                    scale: 1.0,
                    rotation: NO_TURN,
                },
                middle: relative,
                radius: AVATAR_BOUNDING_RADIUS,
                dynamic: true,
            });
        }
        let avatars_end = drawables.len();
        for entity_box in &self.entity_boxes {
            let relative = Vec3::from_array(entity_box.position.relative_to(camera.position));
            drawables.push(Drawable {
                mesh: MeshKey::Box(entity_box.key()),
                placement: Placement {
                    offset: relative - eye,
                    scale: 1.0,
                    rotation: NO_TURN,
                },
                middle: relative,
                radius: ENTITY_BOX_BOUNDING_RADIUS,
                dynamic: true,
            });
        }
        Gathered {
            drawables,
            pieces,
            avatars_end,
        }
    }

    /// The things a cascade's map is drawn from, and how high the highest of them reaches: those the cascade wants (see [`Cascade::wants_caster`]), without the things that move
    /// if the cascade is one that keeps its map for a while.
    fn shadow_casters(
        drawables: &[Drawable],
        cascade: &Cascade,
        basis: &LightBasis,
    ) -> (Vec<usize>, f32) {
        let with_things_that_move = cascade.texel <= DYNAMIC_CASTER_TEXEL_LIMIT;
        let mut casters = Vec::new();
        let mut highest = f32::NEG_INFINITY;
        for (index, drawable) in drawables.iter().enumerate() {
            if (with_things_that_move || !drawable.dynamic)
                && cascade.wants_caster(basis, drawable.middle, drawable.radius)
            {
                casters.push(index);
                highest = highest.max(Cascade::top_of(basis, drawable.middle, drawable.radius));
            }
        }
        (casters, highest)
    }

    /// What drawing these casters into a shadow map for this sun takes, as a number of quads: the quads of the faces drawn, and for each caster something for the calls it
    /// takes to draw them (a brick is a few calls and a few hundred quads, and the calls cost about as much as the quads).
    fn shadow_cost(&self, drawables: &[Drawable], casters: &[usize], basis: &LightBasis) -> usize {
        casters
            .iter()
            .map(|&index| {
                SHADOW_CALL_COST_QUADS
                    + self
                        .mesh(drawables[index].mesh)
                        .shadow_quads(&drawables[index].placement.rotation, basis.toward_sun)
            })
            .sum()
    }

    /// The shadow maps for this frame, if there are to be any: for each cascade, whether its map is drawn again or kept, and what is drawn into it. `seen` are the drawables the
    /// camera sees. None while shadows are off, while there is no sun or sky to shade, while the ground of a flat level hides the sun from everything, and while nothing is seen.
    ///
    /// A map is drawn again when it is wrong: there is none, or the camera has moved too far from where it was (see [`Cascade::holds_after_moving`]) or the sun has turned too
    /// far ([`SUN_DRIFT`] times ten). A map that is a little out of date (the sun a little turned, the terrain or the things that move changed) is drawn again when it has waited
    /// long enough and there is room in the frame for it, finest first: the map of terrain that changed waits longer the coarser the cascade ([`terrain_refresh_age`]) and every
    /// map waits for as many frames as it has times the budget in quads to draw.
    fn plan_shadows(
        &mut self,
        camera: &Camera,
        drawables: &[Drawable],
        seen: &[usize],
    ) -> Option<ShadowPlan> {
        let atmosphere = &self.atmosphere;
        if !atmosphere.shadows
            || (atmosphere.sun_light == 0 && atmosphere.sky_light == 0)
            || seen.is_empty()
        {
            return None;
        }
        let sun = Vec3::from_array(atmosphere.sun_direction).normalize_or(Vec3::Y);
        let (_, flat_ground) = balls_and_ground(&self.sun_occluders, camera.position);
        if flat_ground && sun.y <= -(SUN_DISC_SOFTNESS + TWILIGHT_SOFTNESS) {
            return None;
        }
        let widths = half_widths(atmosphere.fog_end);
        self.shadow_frame += 1;
        let frame = self.shadow_frame;
        let basis = LightBasis::new(sun);
        let camera_across_the_light = basis.coordinates_of(camera.position);
        // How near the camera and how far from it the things it sees are: a cascade is used only if something seen is looked up in it.
        let mut nearest = f32::INFINITY;
        let mut farthest = 0.0f32;
        for &index in seen {
            let (distance, _) = length_and_direction(drawables[index].middle);
            nearest = nearest.min((distance - drawables[index].radius).max(0.0));
            farthest = farthest.max(distance + drawables[index].radius);
        }
        let mut previous_reach = 0.0;
        let mut spent = 0usize;
        let mut cascades = Vec::new();
        for (index, &half_width) in widths.iter().enumerate() {
            let mut cascade = Cascade::new(half_width, camera_across_the_light);
            let serves = cascade.serves(previous_reach, nearest, farthest);
            previous_reach = cascade.reach;
            let kept = self.shadow_cache[index]
                .as_ref()
                .filter(|kept| kept.cascade.half_width == half_width);
            // How the map that is kept stands: where the camera is from where it was, how far the sun has turned, what has changed.
            let moved = kept.map_or(Vec3::ZERO, |kept| {
                Vec3::from_array(camera.position.relative_to(kept.camera))
            });
            let standing = kept.map(|kept| Standing {
                holds: kept.cascade.holds_after_moving(&kept.basis, moved),
                turned: if kept.basis.toward_sun.dot(sun) > 0.0 {
                    kept.basis.toward_sun.cross(sun).length()
                } else {
                    f32::INFINITY
                },
                terrain_changed: kept.terrain_version != self.terrain_version,
                things_changed: cascade.texel <= DYNAMIC_CASTER_TEXEL_LIMIT
                    && kept.dynamic_version != self.dynamic_version,
                waited: frame - kept.frame,
            });
            let wrong = standing.is_none_or(|standing| standing.is_wrong());
            let mut found = None;
            let redraw = serves
                && draw_again(standing, index, spent, || {
                    let (casters, highest) = Self::shadow_casters(drawables, &cascade, &basis);
                    let cost = self.shadow_cost(drawables, &casters, &basis);
                    found = Some((casters, highest));
                    cost
                });
            let (casters, highest) = match found {
                Some(found) => found,
                None if redraw => Self::shadow_casters(drawables, &cascade, &basis),
                None => (Vec::new(), f32::NEG_INFINITY),
            };
            if redraw {
                cascade.fit(highest);
                spent += self.shadow_cost(drawables, &casters, &basis);
                self.shadow_cache[index] = Some(CascadeCache {
                    cascade,
                    basis,
                    camera: camera.position,
                    terrain_version: self.terrain_version,
                    dynamic_version: self.dynamic_version,
                    frame,
                });
                cascades.push(PlannedCascade {
                    cascade,
                    basis,
                    moved: Vec3::ZERO,
                    used: true,
                    drawn: true,
                    casters,
                });
            } else if let (false, Some(kept)) = (wrong, kept) {
                cascades.push(PlannedCascade {
                    cascade: kept.cascade,
                    basis: kept.basis,
                    moved,
                    used: true,
                    drawn: false,
                    casters,
                });
            } else {
                // Nothing in the picture is looked up in it, and what it held is wrong for where the camera is: not used.
                cascades.push(PlannedCascade {
                    cascade,
                    basis,
                    moved: Vec3::ZERO,
                    used: false,
                    drawn: false,
                    casters,
                });
            }
        }
        Some(ShadowPlan { cascades })
    }

    /// Draws every visible brick into `color_view`, which must be `size` pixels in the renderer's
    /// colour format.
    pub fn render(
        &mut self,
        color_view: &wgpu::TextureView,
        size: (u32, u32),
        camera: &Camera,
    ) -> RenderStatistics {
        self.ensure_depth_target(size);
        let aspect_ratio = size.0 as f32 / size.1 as f32;
        let view_projection = camera.view_projection(aspect_ratio);
        let frustum = Frustum::from_view_projection(view_projection);

        let Gathered {
            drawables,
            pieces,
            avatars_end,
        } = self.gather(camera);
        let sees = |index: usize| {
            frustum.intersects_sphere(drawables[index].middle, drawables[index].radius)
        };
        // What the camera sees, nearer bricks first so that they hide farther ones early.
        let mut seen_pieces: Vec<(f32, usize)> = (0..pieces)
            .filter(|&index| sees(index))
            .map(|index| (drawables[index].middle.length_squared(), index))
            .collect();
        seen_pieces.sort_by(|a, b| a.0.total_cmp(&b.0));
        let visible: Vec<usize> = seen_pieces.into_iter().map(|(_, index)| index).collect();
        let visible_avatars: Vec<usize> = (pieces..avatars_end).filter(|&i| sees(i)).collect();
        let visible_boxes: Vec<usize> = (avatars_end..drawables.len())
            .filter(|&i| sees(i))
            .collect();
        let held = |index: usize| match drawables[index].mesh {
            MeshKey::Held(key) => key,
            _ => unreachable!("only the first drawables are bricks and nodes"),
        };

        let seen: Vec<usize> = visible
            .iter()
            .chain(&visible_avatars)
            .chain(&visible_boxes)
            .copied()
            .collect();
        let shadows = self.plan_shadows(camera, &drawables, &seen);
        if shadows.is_some() && !self.shadow_target.full_size {
            self.shadow_target = ShadowTarget::new(&self.device, true);
            self.remake_frame_bind_group();
        }

        // Each thing drawn has a place in the buffer of placements: the pictures' first (bricks, then avatars, then boxes), then what only the shadow maps draw.
        let mut slots = Slots::new(drawables.len());
        for &index in seen.iter() {
            slots.slot(&drawables, index);
        }
        if let Some(plan) = &shadows {
            for planned in plan.cascades.iter().filter(|planned| planned.drawn) {
                for &index in &planned.casters {
                    slots.slot(&drawables, index);
                }
            }
        }

        let mut statistics = RenderStatistics {
            avatars_drawn: visible_avatars.len(),
            entity_boxes_drawn: visible_boxes.len(),
            bricks_drawn: visible.len(),
            coarse_nodes_drawn: visible
                .iter()
                .filter(|&&index| {
                    matches!(
                        held(index),
                        BrickKey::CoarseTerrain(_) | BrickKey::CoarseShip(..)
                    )
                })
                .count(),
            bricks_culled: pieces - visible.len(),
            quads_drawn: visible
                .iter()
                .map(|&index| self.mesh(drawables[index].mesh).quad_total())
                .sum(),
            detailed_quads_drawn: visible
                .iter()
                .map(|&index| self.mesh(drawables[index].mesh).detailed_count as usize)
                .sum(),
            ..RenderStatistics::default()
        };
        if let Some(plan) = &shadows {
            statistics.shadow_cascades_drawn =
                plan.cascades.iter().filter(|planned| planned.drawn).count();
            statistics.shadow_casters_drawn = plan
                .cascades
                .iter()
                .map(|planned| planned.casters.len())
                .sum();
        }

        self.write_placements(&slots.placements);
        self.write_frame_uniform(view_projection, camera.position, shadows.as_ref());
        self.crosshair.set_screen_size(&self.queue, size);
        self.overlay.set_screen_size(&self.queue, size);

        let depth_view = &self
            .depth
            .as_ref()
            .expect("depth target was just created")
            .view;
        let [red, green, blue] = srgb_to_linear_color(self.atmosphere.sky_color);
        let placement_offset =
            |index: usize| (u64::from(slots.get(index)) * self.placement_stride) as u32;
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("voxel frame"),
            });
        // The shadow maps first: the sun's view of what is near the camera, a map for each cascade that something is looked up in.
        if let Some(plan) = &shadows {
            for (layer, planned) in plan.cascades.iter().enumerate() {
                if !planned.drawn {
                    continue;
                }
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("shadow pass"),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &self.shadow_target.layer_views[layer],
                        depth_ops: Some(wgpu::Operations {
                            // (Depth grows toward the sun: 0 is as far from it as the map goes, where nothing is.)
                            load: wgpu::LoadOp::Clear(0.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                let frame_offset = ((layer as u64 + 1) * self.frame_stride) as u32;
                pass.set_bind_group(0, &self.shadow_frame_group, &[frame_offset]);
                // (Only the faces that look away from the sun: the others are not kept, and are not worth the vertices.)
                let toward_sun = planned.basis.toward_sun;
                pass.set_pipeline(&self.shadow_pipelines.quads);
                for &index in &planned.casters {
                    let mesh = self.mesh(drawables[index].mesh);
                    let Some(quads) = &mesh.quads else {
                        continue;
                    };
                    let rotation = &drawables[index].placement.rotation;
                    let ranges = GpuBrick::back_faces(&mesh.quad_faces, rotation, toward_sun);
                    if ranges.is_empty() {
                        continue;
                    }
                    pass.set_bind_group(1, &self.placements.bind_group, &[placement_offset(index)]);
                    pass.set_vertex_buffer(0, quads.slice(..));
                    for range in ranges {
                        pass.draw(0..6, range);
                    }
                }
                pass.set_pipeline(&self.shadow_pipelines.detailed);
                for &index in &planned.casters {
                    let mesh = self.mesh(drawables[index].mesh);
                    let Some(detailed) = &mesh.detailed else {
                        continue;
                    };
                    let rotation = &drawables[index].placement.rotation;
                    let ranges = GpuBrick::back_faces(&mesh.detailed_faces, rotation, toward_sun);
                    if ranges.is_empty() {
                        continue;
                    }
                    pass.set_bind_group(1, &self.placements.bind_group, &[placement_offset(index)]);
                    pass.set_vertex_buffer(0, detailed.slice(..));
                    for range in ranges {
                        pass.draw(0..6, range);
                    }
                }
            }
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("voxel pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(red),
                            g: f64::from(green),
                            b: f64::from(blue),
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations {
                        // (Depth is reversed: 0 is as far as can be.)
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.frame_bind_group, &[]);
            for &index in &visible {
                let brick = self.mesh(drawables[index].mesh);
                let Some(quads) = &brick.quads else {
                    continue;
                };
                pass.set_bind_group(1, &self.placements.bind_group, &[placement_offset(index)]);
                pass.set_vertex_buffer(0, quads.slice(..));
                pass.draw(0..6, 0..brick.quad_count);
            }
            // The bricks that have blocks that do not fill their cells, with the other pipeline.
            pass.set_pipeline(&self.detailed_pipeline);
            for &index in &visible {
                let brick = self.mesh(drawables[index].mesh);
                let Some(detailed) = &brick.detailed else {
                    continue;
                };
                pass.set_bind_group(1, &self.placements.bind_group, &[placement_offset(index)]);
                pass.set_vertex_buffer(0, detailed.slice(..));
                pass.draw(0..6, 0..brick.detailed_count);
            }
            pass.set_pipeline(&self.pipeline);
            for &index in &visible_avatars {
                let mesh = self.mesh(drawables[index].mesh);
                let Some(quads) = &mesh.quads else {
                    continue;
                };
                pass.set_bind_group(1, &self.placements.bind_group, &[placement_offset(index)]);
                pass.set_vertex_buffer(0, quads.slice(..));
                pass.draw(0..6, 0..mesh.quad_count);
            }
            // Boxes: the faces that fill a cell, then the ones that do not (a box is all of the second kind unless
            // it is a whole voxel).
            for &index in &visible_boxes {
                let mesh = self.mesh(drawables[index].mesh);
                let Some(quads) = &mesh.quads else {
                    continue;
                };
                pass.set_bind_group(1, &self.placements.bind_group, &[placement_offset(index)]);
                pass.set_vertex_buffer(0, quads.slice(..));
                pass.draw(0..6, 0..mesh.quad_count);
            }
            pass.set_pipeline(&self.detailed_pipeline);
            for &index in &visible_boxes {
                let mesh = self.mesh(drawables[index].mesh);
                let Some(detailed) = &mesh.detailed else {
                    continue;
                };
                pass.set_bind_group(1, &self.placements.bind_group, &[placement_offset(index)]);
                pass.set_vertex_buffer(0, detailed.slice(..));
                pass.draw(0..6, 0..mesh.detailed_count);
            }
            if self.crosshair_enabled {
                self.crosshair.draw(&mut pass);
            }
            self.overlay.draw(&mut pass);
        }
        self.queue.submit([encoder.finish()]);
        statistics
    }

    /// Renders to an internal texture without reading it back. For timing: call
    /// [`VoxelRenderer::wait_for_gpu`] afterwards to include the GPU's work.
    pub fn render_offscreen(
        &mut self,
        camera: &Camera,
        width: u32,
        height: u32,
    ) -> RenderStatistics {
        let size = (width, height);
        if self
            .offscreen
            .as_ref()
            .is_none_or(|target| target.size != size)
        {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("offscreen colour"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.color_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.offscreen = Some(SizedTarget {
                size,
                texture,
                view,
            });
        }
        let view = self.offscreen.as_ref().expect("just created").view.clone();
        self.render(&view, size, camera)
    }

    /// Renders to an internal texture and returns tightly packed RGBA bytes (sRGB encoded), the
    /// way an image file stores them.
    pub fn render_to_rgba(
        &mut self,
        camera: &Camera,
        width: u32,
        height: u32,
    ) -> (Vec<u8>, RenderStatistics) {
        let statistics = self.render_offscreen(camera, width, height);

        let padded_row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: u64::from(padded_row) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("readback"),
            });
        encoder.copy_texture_to_buffer(
            self.offscreen
                .as_ref()
                .expect("just created")
                .texture
                .as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                sender
                    .send(result)
                    .expect("receiver outlives the map callback");
            });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the device finishes the readback");
        receiver
            .recv()
            .expect("the map callback runs")
            .expect("mapping the readback buffer succeeds");

        let mapped = readback
            .slice(..)
            .get_mapped_range()
            .expect("the readback buffer is mapped");
        let swap_red_and_blue = matches!(self.color_format, wgpu::TextureFormat::Bgra8UnormSrgb);
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for row in 0..height as usize {
            let start = row * padded_row as usize;
            for texel in mapped[start..start + (width * 4) as usize]
                .as_chunks::<4>()
                .0
            {
                if swap_red_and_blue {
                    pixels.extend_from_slice(&[texel[2], texel[1], texel[0], texel[3]]);
                } else {
                    pixels.extend_from_slice(texel);
                }
            }
        }
        drop(mapped);
        readback.unmap();
        (pixels, statistics)
    }

    /// Blocks until everything submitted so far has finished on the GPU. For timing.
    pub fn wait_for_gpu(&self) {
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the device finishes its work");
    }

    fn ensure_depth_target(&mut self, size: (u32, u32)) {
        if self
            .depth
            .as_ref()
            .is_some_and(|target| target.size == size)
        {
            return;
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.depth = Some(SizedTarget {
            size,
            texture,
            view,
        });
    }

    fn write_placements(&mut self, placements: &[Placement]) {
        if placements.len() > self.placements.capacity {
            let capacity = placements.len().next_power_of_two();
            self.placements = Self::create_placement_buffer(
                &self.device,
                &self.placement_layout,
                self.placement_stride,
                capacity,
            );
        }
        let stride = self.placement_stride as usize;
        let mut bytes = vec![0u8; placements.len() * stride];
        for (index, placement) in placements.iter().enumerate() {
            let rotation = placement.rotation;
            // The matrix by columns, each padded to four floats.
            let words: [f32; 16] = [
                placement.offset.x,
                placement.offset.y,
                placement.offset.z,
                placement.scale,
                rotation[0][0],
                rotation[1][0],
                rotation[2][0],
                0.0,
                rotation[0][1],
                rotation[1][1],
                rotation[2][1],
                0.0,
                rotation[0][2],
                rotation[1][2],
                rotation[2][2],
                0.0,
            ];
            bytes[index * stride..index * stride + PLACEMENT_SIZE as usize]
                .copy_from_slice(bytemuck::cast_slice(&words));
        }
        if !bytes.is_empty() {
            self.queue.write_buffer(&self.placements.buffer, 0, &bytes);
        }
    }

    fn write_frame_uniform(
        &self,
        view_projection: glam::Mat4,
        camera: WorldPosition,
        shadows: Option<&ShadowPlan>,
    ) {
        let [fog_red, fog_green, fog_blue] = srgb_to_linear_color(self.atmosphere.sky_color);
        let sun = Vec3::from_array(self.atmosphere.sun_direction).normalize_or(Vec3::Y);
        let side = self
            .atmosphere
            .side_direction
            .map_or(sun, |side| Vec3::from_array(side).normalize_or(Vec3::Y));
        let (balls, flat_ground) = balls_and_ground(&self.sun_occluders, camera);
        let mut sun_balls = [[0.0; 4]; MAXIMUM_SUN_BALLS];
        for (slot, ball) in sun_balls.iter_mut().zip(&balls) {
            *slot = [ball.centre.x, ball.centre.y, ball.centre.z, ball.radius];
        }
        let uniform = FrameUniform {
            view_projection: view_projection.to_cols_array_2d(),
            fog_color: [fog_red, fog_green, fog_blue, 1.0],
            fog_range: [
                self.atmosphere.fog_start,
                self.atmosphere.fog_end,
                f32::from(self.atmosphere.ambient_light) / 255.0,
                0.0,
            ],
            sun_direction: [sun.x, sun.y, sun.z, 0.0],
            side_direction: [side.x, side.y, side.z, 0.0],
            sun_light: [
                f32::from(self.atmosphere.sun_light) / 255.0,
                f32::from(self.atmosphere.sky_light) / 255.0,
                if flat_ground { 1.0 } else { 0.0 },
                balls.len() as f32,
            ],
            sun_balls,
            shadow: [
                shadows.map_or(0.0, |plan| plan.cascades.len() as f32),
                f32::from(self.atmosphere.sky_light_in_shadow) / 255.0,
                0.0,
                0.0,
            ],
            cascades: std::array::from_fn(|index| {
                shadows
                    .and_then(|plan| plan.cascades.get(index))
                    .filter(|planned| planned.used)
                    .map_or_else(Zeroable::zeroed, |planned| CascadeUniform {
                        matrix: planned
                            .cascade
                            .matrix_after_moving(&planned.basis, planned.moved)
                            .to_cols_array_2d(),
                        limits: [
                            planned.cascade.reach,
                            planned.cascade.texel,
                            planned.cascade.range,
                            0.0,
                        ],
                    })
            }),
            avatars: AVATAR_COLORS.map(|color| {
                let [red, green, blue] = srgb_to_linear_color(color);
                [red, green, blue, 1.0]
            }),
        };
        self.queue
            .write_buffer(&self.frame_buffer, 0, bytemuck::bytes_of(&uniform));
        // The copies the shadow passes use: the same, with the cascade's matrix where the camera's is.
        if let Some(plan) = shadows {
            for (index, planned) in plan.cascades.iter().enumerate() {
                if !planned.drawn {
                    continue;
                }
                let copy = FrameUniform {
                    view_projection: planned.cascade.matrix(&planned.basis).to_cols_array_2d(),
                    ..uniform
                };
                self.queue.write_buffer(
                    &self.frame_buffer,
                    (index as u64 + 1) * self.frame_stride,
                    bytemuck::bytes_of(&copy),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_scale_of_a_cell_is_two_to_its_level_exactly_at_every_level_there_is() {
        let mut expected = 1.0f32;
        for level in 0..=116u8 {
            assert_eq!(cell_scale(level), expected, "level {level}");
            expected *= 2.0;
        }
        assert!(cell_scale(116).is_finite());
        // (The node is 32 cells wide, and a sphere round it a little more than that: still an `f32`.)
        assert!((cell_scale(116) * 32.0 * 2.0).is_finite());
    }

    #[test]
    fn the_sphere_of_a_node_is_that_of_a_brick_times_its_scale_from_its_corner() {
        let corner = Vec3::new(5.0, -7.0, 100.0);
        let (middle, radius) = node_sphere(corner, 1.0);
        assert_eq!((middle, radius), (corner + Vec3::splat(16.0), 27.72));
        let (middle, radius) = node_sphere(corner, 8.0);
        assert_eq!((middle, radius), (corner + Vec3::splat(128.0), 27.72 * 8.0));
        // The sphere holds the node: the far corner of the cube (32 * scale along each axis) is inside it.
        let far = corner + Vec3::splat(32.0 * 8.0);
        assert!((far - middle).length() <= radius);
    }

    use super::*;

    #[test]
    fn the_starts_of_the_faces_count_what_comes_before_each_and_the_total_after_the_last() {
        assert_eq!(face_starts([].into_iter()), [0; 7]);
        assert_eq!(
            face_starts([0, 0, 2, 5, 5, 5].into_iter()),
            [0, 2, 2, 3, 3, 3, 6]
        );
        assert_eq!(face_starts([1].into_iter()), [0, 0, 1, 1, 1, 1, 1]);
    }

    #[test]
    fn only_the_faces_that_look_away_from_the_sun_are_drawn_into_a_shadow_map_and_a_turn_turns_them()
     {
        // Faces in the order -x, +x, -y, +y, -z, +z, with 1, 2, 3, 4, 5 and 6 quads on them.
        let starts = [0, 1, 3, 6, 10, 15, 21];
        let drawn =
            |rotation: &[[f32; 3]; 3], sun: Vec3| GpuBrick::back_faces(&starts, rotation, sun);
        // A sun straight up: the faces that look down are the backs, and the sides are edge on and not drawn.
        assert_eq!(drawn(&NO_TURN, Vec3::Y), vec![3..6]);
        // A sun over +x and +y (and +z a little): the backs are -x, -y and -z.
        assert_eq!(
            drawn(&NO_TURN, Vec3::new(0.6, 0.8, 0.1)),
            vec![0..1, 3..6, 10..15]
        );
        // A sun over -x and +y: the backs are +x and -y, which are neighbours and make one range.
        assert_eq!(drawn(&NO_TURN, Vec3::new(-0.6, 0.8, 0.0)), vec![1..6]);
        // A brick turned half way round the y axis: its +x face looks to -x and its +z face to -z, so for the first sun the backs are the brick's +x, -y and +z.
        let half_turn = [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]];
        assert_eq!(
            drawn(&half_turn, Vec3::new(0.6, 0.8, 0.1)),
            vec![1..6, 15..21]
        );
        // No quads on a face: no range.
        assert_eq!(
            GpuBrick::back_faces(&[0, 0, 0, 0, 5, 5, 5], &NO_TURN, Vec3::Y),
            vec![]
        );
    }

    fn standing() -> Standing {
        Standing {
            holds: true,
            turned: 0.0,
            terrain_changed: false,
            things_changed: false,
            waited: 100,
        }
    }

    fn cost_of(quads: usize) -> impl FnOnce() -> usize {
        move || quads
    }

    #[test]
    fn a_map_that_is_wrong_or_missing_is_drawn_again_whatever_it_costs_and_one_that_is_right_is_not()
     {
        let nothing = || panic!("the cost is not asked for when it does not matter");
        assert!(draw_again(None, 6, 9_000_000, nothing));
        for wrong in [
            Standing {
                holds: false,
                ..standing()
            },
            Standing {
                turned: 10.0 * SUN_DRIFT * 1.01,
                ..standing()
            },
            Standing {
                turned: f32::INFINITY,
                ..standing()
            },
        ] {
            assert!(wrong.is_wrong());
            assert!(draw_again(Some(wrong), 6, 9_000_000, nothing), "{wrong:?}");
            assert!(draw_again(
                Some(Standing { waited: 0, ..wrong }),
                6,
                9_000_000,
                nothing
            ));
        }
        assert!(!standing().is_wrong());
        assert!(!draw_again(Some(standing()), 0, 0, nothing));
        // A sun that has turned a little, but not enough to matter, is no reason.
        assert!(!draw_again(
            Some(Standing {
                turned: SUN_DRIFT,
                ..standing()
            }),
            0,
            0,
            nothing
        ));
    }

    #[test]
    fn a_map_that_is_out_of_date_waits_for_its_time_and_for_room_and_the_first_in_a_frame_is_let_in()
     {
        let turned = Standing {
            turned: SUN_DRIFT * 1.5,
            ..standing()
        };
        // A sun that has turned: after a frame, if it is cheap.
        assert!(!draw_again(
            Some(Standing {
                waited: 0,
                ..turned
            }),
            0,
            0,
            cost_of(1)
        ));
        assert!(draw_again(
            Some(Standing {
                waited: 1,
                ..turned
            }),
            0,
            0,
            cost_of(1)
        ));
        // The cost is a number of budgets, and so of frames to wait: 4 million is three of 1.5 million.
        let costly = |waited| Standing { waited, ..turned };
        assert!(!draw_again(Some(costly(2)), 0, 0, cost_of(4_000_000)));
        assert!(draw_again(Some(costly(3)), 0, 0, cost_of(4_000_000)));
        // The first map of a frame is let in over the budget; one after it is not, unless it fits.
        assert!(draw_again(Some(costly(9)), 0, 0, cost_of(7_000_000)));
        assert!(!draw_again(Some(costly(9)), 0, 1, cost_of(7_000_000)));
        assert!(draw_again(Some(costly(9)), 0, 1_000_000, cost_of(400_000)));
        assert!(!draw_again(Some(costly(9)), 0, 1_000_000, cost_of(600_000)));
        // Terrain that has changed waits for its cascade's time: none for the finest three, then 2, 4, 8 frames.
        let changed = |waited| Standing {
            terrain_changed: true,
            waited,
            ..standing()
        };
        for (index, wait) in [(0, 1), (1, 1), (2, 1), (3, 2), (4, 4), (5, 8), (9, 128)] {
            assert!(
                !draw_again(Some(changed(wait - 1)), index, 0, cost_of(1)),
                "{index}"
            );
            assert!(
                draw_again(Some(changed(wait)), index, 0, cost_of(1)),
                "{index}"
            );
        }
        // Things that move are a reason in the same way, and so is each of the reasons alone.
        let things = Standing {
            things_changed: true,
            waited: 1,
            ..standing()
        };
        assert!(draw_again(Some(things), 0, 0, cost_of(1)));
        assert!(!draw_again(
            Some(Standing {
                waited: 0,
                ..things
            }),
            0,
            0,
            cost_of(1)
        ));
    }

    #[test]
    fn the_wait_of_terrain_that_changed_doubles_for_each_cascade_after_the_third() {
        let waits: Vec<u64> = (0..MAXIMUM_CASCADES).map(terrain_refresh_age).collect();
        assert_eq!(waits, [1, 1, 1, 2, 4, 8, 16, 32, 64, 128, 256]);
    }

    #[test]
    fn the_palette_the_shader_reads_is_linear_and_never_empty() {
        assert_eq!(linear_palette(&[]), [[1.0, 0.0, 1.0, 1.0]]);
        let [black, middle, white] = linear_palette(&[[0.0; 3], [0.5; 3], [1.0; 3]])[..] else {
            panic!("three colours");
        };
        assert_eq!(black, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(white, [1.0, 1.0, 1.0, 1.0]);
        // Half in sRGB is about a fifth in linear light.
        assert!((middle[0] - 0.2140).abs() < 0.001, "{middle:?}");
        assert_eq!(middle[0], middle[2]);
    }

    #[test]
    fn the_shader_and_the_rule_agree_on_how_soft_a_shadow_is_and_how_many_balls_there_are() {
        let shader = include_str!("voxel.wgsl");
        for declaration in [
            format!(
                "const SUN_DISC_SOFTNESS: f32 = {:?};",
                crate::sun::SUN_DISC_SOFTNESS
            ),
            format!(
                "const TWILIGHT_SOFTNESS: f32 = {:?};",
                crate::sun::TWILIGHT_SOFTNESS
            ),
            format!(
                "const MOST_OF_A_BALL: f32 = {:?};",
                crate::sun::MOST_OF_A_BALL
            ),
            format!("const TURNED_UP_TO: f32 = {:?};", crate::sun::TURNED_UP_TO),
            format!(
                "const TURNED_NO_MORE_FROM: f32 = {:?};",
                crate::sun::TURNED_NO_MORE_FROM
            ),
            format!("sun_balls: array<vec4<f32>, {MAXIMUM_SUN_BALLS}>,"),
            format!("const BLEND_FROM: f32 = {:?};", crate::shadow::BLEND_FROM),
            format!(
                "const DEPTH_BIAS_TEXELS: f32 = {:?};",
                crate::shadow::DEPTH_BIAS_TEXELS
            ),
            format!(
                "const SLOPE_BIAS_LIMIT: f32 = {:?};",
                crate::shadow::SLOPE_BIAS_LIMIT
            ),
            format!(
                "const FACING_FULL_FROM: f32 = {:?};",
                crate::shadow::FACING_FULL_FROM
            ),
            format!("cascades: array<Cascade, {MAXIMUM_CASCADES}>,"),
        ] {
            assert!(shader.contains(&declaration), "{declaration}");
        }
    }

    #[test]
    fn the_shader_and_the_renderer_agree_where_the_avatar_materials_start() {
        let shader = include_str!("voxel.wgsl");
        let declaration = format!("const AVATAR_BASE: u32 = 0x{AVATAR_MATERIAL_BASE:X}u;");
        assert!(shader.contains(&declaration), "{declaration}");
        assert_eq!(
            AVATAR_MATERIAL_BASE as usize + AVATAR_COLOR_COUNT - 1,
            voxel_meshing::MAXIMUM_MATERIAL as usize,
            "the avatar colours are the last values a quad can carry"
        );
        assert!(
            (MAXIMUM_GAME_MATERIALS as u32) <= AVATAR_MATERIAL_BASE,
            "no game material is an avatar colour"
        );
    }
}
