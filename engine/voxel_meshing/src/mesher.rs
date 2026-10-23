use block_models::{BlockTable, FaceMark, ModelId, SUBDIVISIONS};
use voxel_grid::ChannelStorage;
use voxel_space::{BRICK_EDGE_LENGTH, FaceDirection, LocalVoxelIndex};

use crate::quad::{BrickMesh, DetailedQuad, FULL_LIGHT, Quad, quad_axes};

const EDGE: usize = BRICK_EDGE_LENGTH;

/// What a brick holds in each voxel that meshing looks at: the block, which state of it, and how
/// bright the voxel is. A game without block states or lighting leaves those out.
#[derive(Clone, Copy, Debug)]
pub struct BrickChannels<'a> {
    pub blocks: &'a ChannelStorage,
    /// The state of each voxel's block; state 0 everywhere when absent.
    pub states: Option<&'a ChannelStorage>,
    /// The orientation of each voxel (the number of a turn of the lattice, see `block_models::ORIENTATION_COUNT`); none everywhere when absent.
    pub orientations: Option<&'a ChannelStorage>,
    /// The light in each voxel, 0 to 255; [`FULL_LIGHT`] everywhere when absent.
    pub light: Option<&'a ChannelStorage>,
}

impl<'a> BrickChannels<'a> {
    /// A brick of blocks with no states and no light.
    pub fn blocks_only(blocks: &'a ChannelStorage) -> Self {
        Self {
            blocks,
            states: None,
            orientations: None,
            light: None,
        }
    }

    pub(crate) fn state_at(&self, voxel: LocalVoxelIndex) -> u32 {
        self.states.map_or(0, |states| states.get(voxel))
    }

    pub(crate) fn orientation_at(&self, voxel: LocalVoxelIndex) -> u32 {
        self.orientations
            .map_or(0, |orientations| orientations.get(voxel))
    }

    pub(crate) fn light_at(&self, voxel: LocalVoxelIndex) -> u8 {
        self.light
            .map_or(FULL_LIGHT, |light| light.get(voxel).min(255) as u8)
    }
}

/// The voxels a brick's mesh depends on: its own, and the six bricks that touch its faces (only
/// the layer next to the brick matters).
///
/// `neighbours` is indexed by [`FaceDirection::index`]. A missing neighbour is treated as empty,
/// so faces next to it are drawn; callers that stream bricks should mesh only once the
/// neighbours exist, or remesh when they arrive.
#[derive(Clone, Copy, Debug)]
pub struct BrickNeighbourhood<'a> {
    pub center: BrickChannels<'a>,
    pub neighbours: [Option<BrickChannels<'a>>; 6],
}

/// The mesh of a brick: a greedy-meshed rectangle for every maximal run of identical, exposed faces
/// of whole cubes, and detailed quads for the blocks that do not fill their cell. A face of a whole
/// cube is exposed when what it looks at does not hide all of it; a quad is as bright as the voxel it
/// looks at, so quads of different brightness do not merge.
pub fn mesh_brick(input: &BrickNeighbourhood<'_>, table: &BlockTable) -> BrickMesh {
    // Fast paths for the commonest bricks: sky, and solid ground with solid ground all around.
    if let Some(center) = uniform_model(&input.center, table) {
        if !table.is_solid(center) {
            return BrickMesh::default();
        }
        let fully_buried = table.is_opaque_cube(center)
            && FaceDirection::ALL.iter().all(|&face| {
                input.neighbours[usize::from(face.index())]
                    .as_ref()
                    .and_then(|neighbour| uniform_model(neighbour, table))
                    .is_some_and(|model| table.hides_fully(model, face.opposite()))
            });
        if fully_buried {
            return BrickMesh::default();
        }
    }

    let brick = MeshedBrick::new(input, table);
    let mut mesh = BrickMesh::default();
    for face in FaceDirection::ALL {
        brick.mesh_whole_faces(face, &mut mesh.quads);
    }
    brick.mesh_partial_blocks(&mut mesh.detailed);
    mesh
}

/// The model every voxel of the brick has, if they all have the same.
pub(crate) fn uniform_model(channels: &BrickChannels<'_>, table: &BlockTable) -> Option<ModelId> {
    let block = channels.blocks.uniform_value()?;
    let state = match channels.states {
        Some(states) => states.uniform_value()?,
        None => 0,
    };
    let orientation = match channels.orientations {
        Some(orientations) => orientations.uniform_value()?,
        None => 0,
    };
    Some(table.model_of(block, state, orientation))
}

fn local(p: [usize; 3]) -> LocalVoxelIndex {
    LocalVoxelIndex::new(p[0] as u8, p[1] as u8, p[2] as u8)
}

/// What the touching layer of a neighbouring brick holds, indexed `v * 32 + u` in the quad axes of
/// the face it touches. `None` is no brick at all.
struct Layer {
    models: Vec<Option<ModelId>>,
    light: Vec<u8>,
}

impl Layer {
    fn of(neighbour: Option<&BrickChannels<'_>>, face: FaceDirection, table: &BlockTable) -> Self {
        let Some(neighbour) = neighbour else {
            return Self {
                models: vec![None; EDGE * EDGE],
                light: vec![FULL_LIGHT; EDGE * EDGE],
            };
        };
        let (u_axis, v_axis) = quad_axes(face);
        let touching_slice = if face.is_positive() { 0 } else { EDGE - 1 };
        let mut models = Vec::with_capacity(EDGE * EDGE);
        let mut light = Vec::with_capacity(EDGE * EDGE);
        for v in 0..EDGE {
            for u in 0..EDGE {
                let mut p = [0usize; 3];
                p[face.axis()] = touching_slice;
                p[u_axis] = u;
                p[v_axis] = v;
                let voxel = local(p);
                models.push(Some(table.model_of(
                    neighbour.blocks.get(voxel),
                    neighbour.state_at(voxel),
                    neighbour.orientation_at(voxel),
                )));
                light.push(neighbour.light_at(voxel));
            }
        }
        Self { models, light }
    }
}

/// A brick's voxels, looked at the way meshing needs.
struct MeshedBrick<'a> {
    table: &'a BlockTable,
    /// What each voxel is drawn as: its block, or for a block with colours for its states, the
    /// colour of its state (see [`BlockTable::material_of`]).
    materials: Vec<u32>,
    /// For a voxel whose block marks a face in its state, the face and the material it is drawn in.
    marks: Vec<Option<FaceMark>>,
    models: Vec<ModelId>,
    light: Vec<u8>,
    /// Indexed by [`FaceDirection::index`].
    layers: [Layer; 6],
}

impl<'a> MeshedBrick<'a> {
    fn new(input: &BrickNeighbourhood<'_>, table: &'a BlockTable) -> Self {
        let blocks: Vec<u32> = input.center.blocks.iter().collect();
        let states: Option<Vec<u32>> = input.center.states.map(|states| states.iter().collect());
        let state_of = |index: usize| states.as_ref().map_or(0, |states| states[index]);
        let orientations: Option<Vec<u32>> = input
            .center
            .orientations
            .map(|orientations| orientations.iter().collect());
        let orientation_of = |index: usize| {
            orientations
                .as_ref()
                .map_or(0, |orientations| orientations[index])
        };
        let models = blocks
            .iter()
            .enumerate()
            .map(|(index, &block)| table.model_of(block, state_of(index), orientation_of(index)))
            .collect();
        let materials = blocks
            .iter()
            .enumerate()
            .map(|(index, &block)| table.material_of(block, state_of(index)))
            .collect();
        let marks = blocks
            .iter()
            .enumerate()
            .map(|(index, &block)| table.face_mark(block, state_of(index), orientation_of(index)))
            .collect();
        let light = match input.center.light {
            Some(light) => light.iter().map(|value| value.min(255) as u8).collect(),
            None => vec![FULL_LIGHT; blocks.len()],
        };
        Self {
            table,
            materials,
            marks,
            models,
            light,
            layers: std::array::from_fn(|index| {
                let face = FaceDirection::from_index(index as u8).expect("six faces");
                Layer::of(input.neighbours[index].as_ref(), face, table)
            }),
        }
    }

    /// What the `face` of the voxel at this index is drawn as: the marked face in the mark colour, the others in the voxel's.
    fn material_of_face(&self, index: usize, face: FaceDirection) -> u32 {
        match self.marks[index] {
            Some(mark) if mark.marks(face) => mark.material,
            _ => self.materials[index],
        }
    }

    /// What is in the voxel `face` of the one at `p`, and how bright it is. `None` where there is no
    /// brick to ask.
    fn in_front(&self, p: [usize; 3], face: FaceDirection) -> (Option<ModelId>, u8) {
        let axis = face.axis();
        let inside = if face.is_positive() {
            p[axis] + 1 < EDGE
        } else {
            p[axis] > 0
        };
        if inside {
            let mut next = p;
            next[axis] = if face.is_positive() {
                p[axis] + 1
            } else {
                p[axis] - 1
            };
            let index = local(next).index();
            (Some(self.models[index]), self.light[index])
        } else {
            let (u_axis, v_axis) = quad_axes(face);
            let layer = &self.layers[usize::from(face.index())];
            let cell = p[v_axis] * EDGE + p[u_axis];
            (layer.models[cell], layer.light[cell])
        }
    }

    /// Which sub-faces of `face` of a voxel with `model` the voxel in front of it hides.
    fn hidden_by(&self, model: ModelId, front: Option<ModelId>, face: FaceDirection) -> u64 {
        front.map_or(0, |neighbour| {
            self.table.hidden_by_neighbour(model, face, neighbour)
        })
    }

    /// Greedy quads for the faces of whole cubes that look `face`.
    fn mesh_whole_faces(&self, face: FaceDirection, quads: &mut Vec<Quad>) {
        let axis = face.axis();
        let (u_axis, v_axis) = quad_axes(face);
        let mut mask: Vec<Option<(u32, u8)>> = vec![None; EDGE * EDGE];
        for slice in 0..EDGE {
            let mut any = false;
            for v in 0..EDGE {
                for u in 0..EDGE {
                    let mut p = [0usize; 3];
                    p[axis] = slice;
                    p[u_axis] = u;
                    p[v_axis] = v;
                    let index = local(p).index();
                    let mut key = None;
                    let model = self.models[index];
                    if self.table.is_opaque_cube(model) {
                        let (front, light) = self.in_front(p, face);
                        if self.hidden_by(model, front, face) != u64::MAX {
                            key = Some((self.material_of_face(index, face), light));
                        }
                    }
                    any |= key.is_some();
                    mask[v * EDGE + u] = key;
                }
            }
            if any {
                merge_slice(face, slice, &mut mask, quads);
            }
        }
    }

    /// Detailed quads for the blocks that are not whole cubes: a slab, a stair, glass.
    fn mesh_partial_blocks(&self, detailed: &mut Vec<DetailedQuad>) {
        let eighths = SUBDIVISIONS as u16;
        for z in 0..EDGE {
            for y in 0..EDGE {
                for x in 0..EDGE {
                    let p = [x, y, z];
                    let index = local(p).index();
                    let model = self.models[index];
                    if !self.table.is_solid(model) || self.table.is_opaque_cube(model) {
                        continue;
                    }
                    let shape = self.table.shape(model);
                    for face in FaceDirection::ALL {
                        let axis = face.axis();
                        let (u_axis, v_axis) = quad_axes(face);
                        let (front, front_light) = self.in_front(p, face);
                        for rectangle in shape.exposed(face) {
                            // On the voxel's boundary a rectangle takes the light of the voxel it
                            // looks out at and can be hidden by it; inside, it is lit by its own voxel.
                            let light = if rectangle.on_boundary(face) {
                                let wanted = rectangle.mask();
                                if self.hidden_by(model, front, face) & wanted == wanted {
                                    continue;
                                }
                                front_light
                            } else {
                                self.light[index]
                            };
                            let mut origin = [0u16; 3];
                            origin[axis] = p[axis] as u16 * eighths
                                + u16::from(rectangle.layer)
                                + u16::from(face.is_positive());
                            origin[u_axis] = p[u_axis] as u16 * eighths + u16::from(rectangle.u);
                            origin[v_axis] = p[v_axis] as u16 * eighths + u16::from(rectangle.v);
                            detailed.push(DetailedQuad {
                                origin,
                                width: u16::from(rectangle.width),
                                height: u16::from(rectangle.height),
                                face,
                                material: self.material_of_face(index, face),
                                light,
                            });
                        }
                    }
                }
            }
        }
    }
}

/// Covers the set cells of `mask` with maximal rectangles of equal material and brightness, clearing
/// it as it goes.
fn merge_slice(
    face: FaceDirection,
    slice: usize,
    mask: &mut [Option<(u32, u8)>],
    quads: &mut Vec<Quad>,
) {
    let axis = face.axis();
    let (u_axis, v_axis) = quad_axes(face);
    for v0 in 0..EDGE {
        let mut u0 = 0;
        while u0 < EDGE {
            let Some(key) = mask[v0 * EDGE + u0] else {
                u0 += 1;
                continue;
            };
            let mut width = 1;
            while u0 + width < EDGE && mask[v0 * EDGE + u0 + width] == Some(key) {
                width += 1;
            }
            let mut height = 1;
            'grow: while v0 + height < EDGE {
                for u in u0..u0 + width {
                    if mask[(v0 + height) * EDGE + u] != Some(key) {
                        break 'grow;
                    }
                }
                height += 1;
            }
            for v in v0..v0 + height {
                for u in u0..u0 + width {
                    mask[v * EDGE + u] = None;
                }
            }
            let mut origin = [0u8; 3];
            origin[axis] = slice as u8;
            origin[u_axis] = u0 as u8;
            origin[v_axis] = v0 as u8;
            quads.push(Quad {
                origin,
                width: width as u8,
                height: height as u8,
                face,
                material: key.0,
                light: key.1,
            });
            u0 += width;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use block_models::{BlockDefinition, Shape};
    use proptest::prelude::*;
    use voxel_space::BRICK_VOXEL_COUNT;

    use super::*;

    const AIR: u32 = 0;
    const STONE: u32 = 1;
    const DIRT: u32 = 2;
    /// A block with states: 0 whole, 1 bottom slab, 2 top slab, 3 a stair rising toward +x.
    const ROCK: u32 = 4;
    /// A see-through whole cube.
    const GLASS: u32 = 5;
    /// A whole cube drawn in one of three colours by its state, which any number can be.
    const NUMBERED: u32 = 6;

    fn table() -> BlockTable {
        let air = BlockDefinition::air();
        let cube = BlockDefinition::cube([0.5; 3]);
        let rock = BlockDefinition::cube([0.4; 3]).with_shapes(vec![
            Shape::full(),
            Shape::half(FaceDirection::NegativeY),
            Shape::half(FaceDirection::PositiveY),
            Shape::stair(FaceDirection::PositiveX),
        ]);
        let glass = BlockDefinition::cube([0.7; 3]).with_see_through(true);
        let numbered =
            BlockDefinition::cube([0.3; 3]).with_state_colors(vec![[0.1; 3], [0.2; 3], [0.3; 3]]);
        BlockTable::from_definitions(&[
            Some(&air),
            Some(&cube),
            Some(&cube),
            Some(&cube),
            Some(&rock),
            Some(&glass),
            Some(&numbered),
        ])
    }

    fn brick_from(fill: impl Fn(usize, usize, usize) -> u32) -> ChannelStorage {
        let mut values = vec![AIR; BRICK_VOXEL_COUNT];
        for z in 0..EDGE {
            for y in 0..EDGE {
                for x in 0..EDGE {
                    values[local([x, y, z]).index()] = fill(x, y, z);
                }
            }
        }
        ChannelStorage::from_values(&values)
    }

    fn only(blocks: &ChannelStorage) -> BrickChannels<'_> {
        BrickChannels::blocks_only(blocks)
    }

    fn isolated_mesh(center: &ChannelStorage) -> BrickMesh {
        mesh_brick(
            &BrickNeighbourhood {
                center: only(center),
                neighbours: [None; 6],
            },
            &table(),
        )
    }

    /// The quads of a brick of whole cubes alone in the world, which has no detailed ones.
    fn isolated(center: &ChannelStorage) -> Vec<Quad> {
        let mesh = isolated_mesh(center);
        assert!(mesh.detailed.is_empty());
        mesh.quads
    }

    fn with_neighbours(
        center: &ChannelStorage,
        neighbours: [Option<&ChannelStorage>; 6],
    ) -> BrickMesh {
        mesh_brick(
            &BrickNeighbourhood {
                center: only(center),
                neighbours: neighbours.map(|neighbour| neighbour.map(only)),
            },
            &table(),
        )
    }

    fn quad(origin: [u8; 3], width: u8, height: u8, face: FaceDirection, material: u32) -> Quad {
        Quad {
            origin,
            width,
            height,
            face,
            material,
            light: FULL_LIGHT,
        }
    }

    #[test]
    fn a_lone_voxel_gets_one_unit_quad_per_face() {
        let center = brick_from(|x, y, z| if (x, y, z) == (5, 6, 7) { STONE } else { AIR });
        let quads = isolated(&center);
        assert_eq!(quads.len(), 6);
        for face in FaceDirection::ALL {
            assert!(
                quads.contains(&quad([5, 6, 7], 1, 1, face, STONE)),
                "{face:?}"
            );
        }
    }

    #[test]
    fn a_full_brick_in_the_open_is_six_big_quads() {
        let quads = isolated(&ChannelStorage::uniform(STONE));
        assert_eq!(quads.len(), 6);
        for face in FaceDirection::ALL {
            // The voxel that owns a positive face is the last one along its axis.
            let mut origin = [0, 0, 0];
            if face.is_positive() {
                origin[face.axis()] = 31;
            }
            assert!(
                quads.contains(&quad(origin, 32, 32, face, STONE)),
                "{face:?}"
            );
        }
    }

    #[test]
    fn a_solid_neighbour_hides_the_faces_that_touch_it() {
        let center = ChannelStorage::uniform(STONE);
        let wall = ChannelStorage::uniform(DIRT);
        let mut neighbours = [None; 6];
        neighbours[usize::from(FaceDirection::PositiveX.index())] = Some(&wall);
        let mesh = with_neighbours(&center, neighbours);
        assert_eq!(mesh.quads.len(), 5);
        assert!(
            mesh.quads
                .iter()
                .all(|quad| quad.face != FaceDirection::PositiveX)
        );
    }

    #[test]
    fn only_the_neighbours_touching_layer_matters() {
        // A neighbour that is solid everywhere except the layer touching the brick.
        let center = ChannelStorage::uniform(STONE);
        let hollow_touching_layer = brick_from(|x, _, _| if x == 0 { AIR } else { STONE });
        let mut neighbours = [None; 6];
        neighbours[usize::from(FaceDirection::PositiveX.index())] = Some(&hollow_touching_layer);
        let mesh = with_neighbours(&center, neighbours);
        assert!(
            mesh.quads
                .iter()
                .any(|quad| quad.face == FaceDirection::PositiveX)
        );

        // Same neighbour seen from the other side: its x = 31 layer is solid, so no face.
        let mut neighbours = [None; 6];
        neighbours[usize::from(FaceDirection::NegativeX.index())] = Some(&hollow_touching_layer);
        let mesh = with_neighbours(&center, neighbours);
        assert!(
            mesh.quads
                .iter()
                .all(|quad| quad.face != FaceDirection::NegativeX)
        );
    }

    #[test]
    fn a_flat_floor_is_one_quad_on_top_and_one_on_each_open_side() {
        let center = brick_from(|_, y, _| if y < 16 { STONE } else { AIR });
        let quads = isolated(&center);
        assert_eq!(quads.len(), 6);
        assert!(quads.contains(&quad([0, 15, 0], 32, 32, FaceDirection::PositiveY, STONE)));
        assert!(quads.contains(&quad([0, 0, 0], 32, 32, FaceDirection::NegativeY, STONE)));
        // Side walls are 16 voxels tall: for x faces, u = y (height 16) and v = z (32).
        assert!(quads.contains(&quad([0, 0, 0], 16, 32, FaceDirection::NegativeX, STONE)));
    }

    #[test]
    fn different_materials_never_merge() {
        let center = brick_from(|x, y, _| {
            if y == 0 {
                if x < 10 { STONE } else { DIRT }
            } else {
                AIR
            }
        });
        let tops: Vec<Quad> = isolated(&center)
            .into_iter()
            .filter(|quad| quad.face == FaceDirection::PositiveY)
            .collect();
        assert_eq!(tops.len(), 2);
        // Y faces span z (width) then x (height): stone covers x 0..10, dirt x 10..32.
        assert!(tops.contains(&quad([0, 0, 0], 32, 10, FaceDirection::PositiveY, STONE)));
        assert!(tops.contains(&quad([10, 0, 0], 32, 22, FaceDirection::PositiveY, DIRT)));
    }

    #[test]
    fn empty_and_buried_bricks_take_the_fast_path_and_produce_nothing() {
        assert!(isolated_mesh(&ChannelStorage::uniform(AIR)).is_empty());
        let stone = ChannelStorage::uniform(STONE);
        let dirt = ChannelStorage::uniform(DIRT);
        assert!(with_neighbours(&stone, [Some(&dirt); 6]).is_empty());
        // One missing neighbour exposes a face.
        let mut neighbours = [Some(&dirt); 6];
        neighbours[3] = None;
        assert_eq!(with_neighbours(&stone, neighbours).quads.len(), 1);
    }

    #[test]
    fn a_brick_is_buried_only_when_what_touches_it_hides_the_side_that_faces_it() {
        // Solid sides, a top slab above and a bottom slab below. A top slab covers its top face, but
        // it is the face towards the brick that matters, and that one is open: the brick's top shows
        // through the gap under the slab. Likewise the bottom slab leaves a gap above itself.
        let stone = ChannelStorage::uniform(STONE);
        let wall = ChannelStorage::uniform(DIRT);
        let rock = ChannelStorage::uniform(ROCK);
        let bottom_slab = ChannelStorage::uniform(1);
        let top_slab = ChannelStorage::uniform(2);
        let mut neighbours = [BrickChannels::blocks_only(&wall); 6];
        let slab = |states| BrickChannels {
            blocks: &rock,
            states: Some(states),
            orientations: None,
            light: None,
        };
        neighbours[usize::from(FaceDirection::PositiveY.index())] = slab(&top_slab);
        neighbours[usize::from(FaceDirection::NegativeY.index())] = slab(&bottom_slab);
        let mesh = mesh_brick(
            &BrickNeighbourhood {
                center: only(&stone),
                neighbours: neighbours.map(Some),
            },
            &table(),
        );
        assert!(mesh.detailed.is_empty());
        assert_eq!(mesh.quads.len(), 2);
        assert!(
            mesh.quads
                .contains(&quad([0, 31, 0], 32, 32, FaceDirection::PositiveY, STONE))
        );
        assert!(
            mesh.quads
                .contains(&quad([0, 0, 0], 32, 32, FaceDirection::NegativeY, STONE))
        );
    }

    #[test]
    fn a_face_looking_into_a_brick_with_no_light_channel_is_fully_lit() {
        let stone = ChannelStorage::uniform(STONE);
        let dark = ChannelStorage::uniform(10);
        let air = ChannelStorage::uniform(AIR);
        let mesh = mesh_brick(
            &BrickNeighbourhood {
                center: BrickChannels {
                    blocks: &stone,
                    states: None,
                    orientations: None,
                    light: Some(&dark),
                },
                neighbours: [Some(only(&air)); 6],
            },
            &table(),
        );
        assert_eq!(mesh.quads.len(), 6);
        assert!(mesh.quads.iter().all(|quad| quad.light == FULL_LIGHT));
    }

    #[test]
    fn a_block_with_state_colours_merges_by_the_colour_of_its_state() {
        // The palette is the seven blocks, then the three colours of the numbered block, so its
        // states 1 and 4 are material 8 (both the second colour), and state 2 is material 9.
        let blocks = brick_from(|_, y, _| if y == 0 { NUMBERED } else { AIR });
        let states = brick_from(|x, _, _| match x {
            0..10 => 1,
            10..20 => 4,
            _ => 2,
        });
        let mesh = with_states(&blocks, &states);
        assert!(mesh.detailed.is_empty());
        let tops: Vec<Quad> = mesh
            .quads
            .iter()
            .copied()
            .filter(|quad| quad.face == FaceDirection::PositiveY)
            .collect();
        assert_eq!(tops.len(), 2, "{tops:?}");
        // Y faces span z (width) then x (height).
        assert!(tops.contains(&quad([0, 0, 0], 32, 20, FaceDirection::PositiveY, 8)));
        assert!(tops.contains(&quad([20, 0, 0], 32, 12, FaceDirection::PositiveY, 9)));
    }

    #[test]
    fn a_block_without_a_state_channel_is_drawn_in_the_colour_of_state_zero() {
        let blocks = brick_from(|_, y, _| if y == 0 { NUMBERED } else { AIR });
        let tops: Vec<Quad> = isolated(&blocks)
            .into_iter()
            .filter(|quad| quad.face == FaceDirection::PositiveY)
            .collect();
        assert_eq!(tops, [quad([0, 0, 0], 32, 32, FaceDirection::PositiveY, 7)]);
    }

    // ----- Marked faces -----

    const PORT: u32 = 2;
    const SLAB_PORT: u32 = 3;
    /// The first material after the blocks': the yellow of the marked faces.
    const YELLOW: u32 = 4;

    /// Blocks 0 air, 1 gunmetal cube, 2 a cube with six states that mark a different face each, and 3 a slab (bottom half) that marks its top.
    fn marked_table() -> BlockTable {
        let faces = FaceDirection::ALL.map(Some);
        let port = BlockDefinition::cube([0.3; 3])
            .with_shapes(vec![Shape::full(); 6])
            .with_marked_faces([255, 216, 0], &faces);
        let slab = BlockDefinition::cube([0.35; 3])
            .with_shapes(vec![Shape::half(FaceDirection::NegativeY)])
            .with_marked_faces([255, 216, 0], &[Some(FaceDirection::PositiveY)]);
        let air = BlockDefinition::air();
        let gunmetal = BlockDefinition::cube([0.3; 3]);
        BlockTable::from_definitions(&[Some(&air), Some(&gunmetal), Some(&port), Some(&slab)])
    }

    fn mesh_marked(blocks: &ChannelStorage, states: &ChannelStorage) -> BrickMesh {
        mesh_brick(
            &BrickNeighbourhood {
                center: BrickChannels {
                    blocks,
                    states: Some(states),
                    orientations: None,
                    light: None,
                },
                neighbours: [None; 6],
            },
            &marked_table(),
        )
    }

    #[test]
    fn only_the_marked_face_of_a_cube_is_drawn_in_the_mark_colour() {
        let at = (5, 6, 7);
        let blocks = brick_from(|x, y, z| if (x, y, z) == at { PORT } else { AIR });
        for marked in FaceDirection::ALL {
            let states = brick_from(|x, y, z| {
                if (x, y, z) == at {
                    u32::from(marked.index())
                } else {
                    0
                }
            });
            let mesh = mesh_marked(&blocks, &states);
            assert!(mesh.detailed.is_empty());
            assert_eq!(mesh.quads.len(), 6);
            for face in FaceDirection::ALL {
                let wanted = if face == marked { YELLOW } else { PORT };
                assert!(
                    mesh.quads.contains(&quad([5, 6, 7], 1, 1, face, wanted)),
                    "marked {marked:?}, face {face:?}"
                );
            }
        }
    }

    #[test]
    fn faces_merge_only_with_faces_of_the_same_material_so_marks_that_meet_merge_and_a_mark_beside_plain_does_not()
     {
        // Two ports side by side along z, both marked on +x: their +x faces are one yellow quad. A third beside them marked on +z has a plain +x face.
        let blocks = brick_from(|x, y, z| {
            if (x, y) == (5, 6) && (7..10).contains(&z) {
                PORT
            } else {
                AIR
            }
        });
        let states = brick_from(|x, y, z| {
            if (x, y) == (5, 6) && (7..10).contains(&z) {
                if z == 9 { 5 } else { 1 }
            } else {
                0
            }
        });
        let mesh = mesh_marked(&blocks, &states);
        let positive_x: Vec<Quad> = mesh
            .quads
            .iter()
            .copied()
            .filter(|quad| quad.face == FaceDirection::PositiveX)
            .collect();
        assert_eq!(positive_x.len(), 2, "{positive_x:?}");
        // The +x face spans y (width) then z (height).
        assert!(positive_x.contains(&quad([5, 6, 7], 1, 2, FaceDirection::PositiveX, YELLOW)));
        assert!(positive_x.contains(&quad([5, 6, 9], 1, 1, FaceDirection::PositiveX, PORT)));
    }

    #[test]
    fn the_marked_face_of_a_block_that_is_not_a_cube_is_drawn_in_the_mark_colour_too() {
        let blocks = brick_from(|x, y, z| {
            if (x, y, z) == (5, 6, 7) {
                SLAB_PORT
            } else {
                AIR
            }
        });
        let states = brick_from(|_, _, _| 0);
        let mesh = mesh_marked(&blocks, &states);
        assert!(mesh.quads.is_empty());
        assert_eq!(mesh.detailed.len(), 6);
        for quad in &mesh.detailed {
            let wanted = if quad.face == FaceDirection::PositiveY {
                YELLOW
            } else {
                SLAB_PORT
            };
            assert_eq!(quad.material, wanted, "{:?}", quad.face);
        }
    }

    // ----- Blocks that do not fill their cell -----

    fn rock_in_a_voxel(state: u32) -> (ChannelStorage, ChannelStorage) {
        let blocks = brick_from(|x, y, z| if (x, y, z) == (5, 6, 7) { ROCK } else { AIR });
        let states = brick_from(|x, y, z| if (x, y, z) == (5, 6, 7) { state } else { 0 });
        (blocks, states)
    }

    fn with_states(blocks: &ChannelStorage, states: &ChannelStorage) -> BrickMesh {
        mesh_brick(
            &BrickNeighbourhood {
                center: BrickChannels {
                    blocks,
                    states: Some(states),
                    orientations: None,
                    light: None,
                },
                neighbours: [None; 6],
            },
            &table(),
        )
    }

    fn detailed(
        origin: [u16; 3],
        width: u16,
        height: u16,
        face: FaceDirection,
        material: u32,
    ) -> DetailedQuad {
        DetailedQuad {
            origin,
            width,
            height,
            face,
            material,
            light: FULL_LIGHT,
        }
    }

    #[test]
    fn a_block_in_state_zero_is_a_whole_cube_and_meshes_like_one() {
        let (blocks, states) = rock_in_a_voxel(0);
        let mesh = with_states(&blocks, &states);
        assert!(mesh.detailed.is_empty());
        assert_eq!(mesh.quads.len(), 6);
        assert!(
            mesh.quads
                .contains(&quad([5, 6, 7], 1, 1, FaceDirection::PositiveY, ROCK))
        );
    }

    #[test]
    fn a_bottom_slab_alone_is_six_detailed_quads_the_top_one_half_way_up() {
        let (blocks, states) = rock_in_a_voxel(1);
        let mesh = with_states(&blocks, &states);
        assert!(mesh.quads.is_empty(), "nothing in it fills its cell");
        assert_eq!(mesh.detailed.len(), 6);
        // The voxel (5, 6, 7) starts at 40, 48, 56 eighths. The top is 4 eighths above its base.
        let top = detailed([40, 48 + 4, 56], 8, 8, FaceDirection::PositiveY, ROCK);
        let bottom = detailed([40, 48, 56], 8, 8, FaceDirection::NegativeY, ROCK);
        // An x face spans y then z: the slab's side is 4 eighths tall and 8 deep; the positive
        // face sits on the voxel's far side.
        let east = detailed([48, 48, 56], 4, 8, FaceDirection::PositiveX, ROCK);
        let west = detailed([40, 48, 56], 4, 8, FaceDirection::NegativeX, ROCK);
        for expected in [top, bottom, east, west] {
            assert!(mesh.detailed.contains(&expected), "{expected:?}");
        }
    }

    #[test]
    fn a_slab_beside_a_whole_cube_loses_the_face_that_touches_it_and_the_cube_keeps_its_own() {
        // A stone cube at (5, 6, 7) and a bottom slab of rock at (6, 6, 7) against its +x side.
        let blocks = brick_from(|x, y, z| match (x, y, z) {
            (5, 6, 7) => STONE,
            (6, 6, 7) => ROCK,
            _ => AIR,
        });
        let states = brick_from(|x, y, z| if (x, y, z) == (6, 6, 7) { 1 } else { 0 });
        let mesh = with_states(&blocks, &states);
        // The cube hides all of the slab's -x face, so the slab has five quads.
        assert_eq!(mesh.detailed.len(), 5);
        assert!(
            mesh.detailed
                .iter()
                .all(|quad| quad.face != FaceDirection::NegativeX)
        );
        // The slab only half hides the cube's +x face, so the cube still draws all of it.
        assert!(
            mesh.quads
                .contains(&quad([5, 6, 7], 1, 1, FaceDirection::PositiveX, STONE))
        );
        assert_eq!(mesh.quads.len(), 6);
    }

    #[test]
    fn two_slabs_side_by_side_hide_each_others_touching_faces_and_a_top_slab_does_not_hide_a_bottom_one()
     {
        let blocks = brick_from(|x, y, z| {
            if (y, z) == (6, 7) && (x == 5 || x == 6) {
                ROCK
            } else {
                AIR
            }
        });
        let bottoms = brick_from(|_, _, _| 1);
        let mesh = with_states(&blocks, &bottoms);
        // Each has five quads: the faces that touch are the same half and so hide one another.
        assert_eq!(mesh.detailed.len(), 10);
        // A bottom slab and a top slab side by side cover different halves of their shared plane.
        let states = brick_from(|x, y, z| match (x, y, z) {
            (5, 6, 7) => 1,
            (6, 6, 7) => 2,
            _ => 0,
        });
        let mesh = with_states(&blocks, &states);
        assert_eq!(mesh.detailed.len(), 12, "nothing is hidden");
    }

    #[test]
    fn a_stair_has_its_steps_and_is_hidden_where_its_back_meets_a_cube() {
        let (blocks, states) = rock_in_a_voxel(3);
        let mesh = with_states(&blocks, &states);
        assert!(mesh.quads.is_empty());
        // Bottom, two tops (tread and step), two risers (the low one and the step's), the back
        // wall, and two sides each made of two boxes' faces.
        let count = |face| {
            mesh.detailed
                .iter()
                .filter(|quad| quad.face == face)
                .count()
        };
        assert_eq!(count(FaceDirection::PositiveY), 2);
        assert_eq!(count(FaceDirection::NegativeY), 1);
        assert_eq!(count(FaceDirection::PositiveX), 1, "the back is one wall");
        assert_eq!(count(FaceDirection::NegativeX), 2);
        // A cube against the back hides it.
        let blocks = brick_from(|x, y, z| match (x, y, z) {
            (5, 6, 7) => ROCK,
            (6, 6, 7) => STONE,
            _ => AIR,
        });
        let states = brick_from(|x, y, z| if (x, y, z) == (5, 6, 7) { 3 } else { 0 });
        let mesh = with_states(&blocks, &states);
        assert!(
            mesh.detailed
                .iter()
                .all(|quad| quad.face != FaceDirection::PositiveX)
        );
    }

    #[test]
    fn glass_is_drawn_as_detailed_quads_hides_nothing_and_is_hidden_by_opaque_cubes() {
        let blocks = brick_from(|x, y, z| match (x, y, z) {
            (5, 6, 7) => GLASS,
            (6, 6, 7) => STONE,
            _ => AIR,
        });
        let mesh = isolated_mesh(&blocks);
        // The glass has five faces (the one against stone is hidden), the stone six: glass hides
        // nothing.
        assert_eq!(mesh.detailed.len(), 5);
        assert!(
            mesh.detailed
                .iter()
                .all(|quad| quad.face != FaceDirection::PositiveX)
        );
        assert_eq!(mesh.quads.len(), 6);
        // A body of glass shows its outside only: the faces between its voxels hide each other.
        let glass = isolated_mesh(&ChannelStorage::uniform(GLASS));
        assert!(glass.quads.is_empty());
        assert_eq!(glass.detailed.len(), 6 * EDGE * EDGE);
        assert!(
            glass
                .detailed
                .iter()
                .all(|quad| quad.width == 8 && quad.height == 8)
        );
    }

    #[test]
    fn a_brick_of_one_slab_state_is_not_taken_for_a_buried_cube() {
        let blocks = ChannelStorage::uniform(ROCK);
        let states = ChannelStorage::uniform(1);
        let stone = ChannelStorage::uniform(STONE);
        let mesh = mesh_brick(
            &BrickNeighbourhood {
                center: BrickChannels {
                    blocks: &blocks,
                    states: Some(&states),
                    orientations: None,
                    light: None,
                },
                neighbours: [Some(only(&stone)); 6],
            },
            &table(),
        );
        // Every slab's top is exposed, half way up its voxel, even where the slab above closes it off
        // (the price of not working out which hollows are sealed).
        assert_eq!(
            mesh.detailed
                .iter()
                .filter(|quad| quad.face == FaceDirection::PositiveY)
                .count(),
            EDGE * EDGE * EDGE
        );
        // A stone brick next to a brick of slabs is not hidden either: the slabs only half hide it.
        let mesh = mesh_brick(
            &BrickNeighbourhood {
                center: only(&stone),
                neighbours: [
                    None,
                    Some(BrickChannels {
                        blocks: &blocks,
                        states: Some(&states),
                        orientations: None,
                        light: None,
                    }),
                    None,
                    None,
                    None,
                    None,
                ],
            },
            &table(),
        );
        assert!(
            mesh.quads
                .iter()
                .any(|quad| quad.face == FaceDirection::PositiveX)
        );
    }

    // ----- Light -----

    fn lit(blocks: &ChannelStorage, light: &ChannelStorage) -> BrickMesh {
        mesh_brick(
            &BrickNeighbourhood {
                center: BrickChannels {
                    blocks,
                    states: None,
                    orientations: None,
                    light: Some(light),
                },
                neighbours: [None; 6],
            },
            &table(),
        )
    }

    #[test]
    fn a_face_is_as_bright_as_the_voxel_it_looks_at() {
        // A floor with a light in the air above half of it.
        let blocks = brick_from(|_, y, _| if y == 0 { STONE } else { AIR });
        let light = brick_from(|x, y, _| {
            if y == 1 {
                if x < 16 { 40 } else { 200 }
            } else {
                0
            }
        });
        let mesh = lit(&blocks, &light);
        let tops: Vec<Quad> = mesh
            .quads
            .iter()
            .copied()
            .filter(|quad| quad.face == FaceDirection::PositiveY)
            .collect();
        // Two quads, split where the light changes (x is the second axis of a y face).
        assert_eq!(tops.len(), 2);
        assert!(tops.contains(&Quad {
            light: 40,
            ..quad([0, 0, 0], 32, 16, FaceDirection::PositiveY, STONE)
        }));
        assert!(tops.contains(&Quad {
            light: 200,
            ..quad([16, 0, 0], 32, 16, FaceDirection::PositiveY, STONE)
        }));
        // The other faces look out at the edge of the brick, where no neighbour means full light.
        assert!(
            mesh.quads
                .iter()
                .filter(|quad| quad.face == FaceDirection::NegativeY)
                .all(|quad| quad.light == FULL_LIGHT)
        );
    }

    #[test]
    fn the_light_of_a_neighbouring_brick_lights_the_faces_that_look_into_it() {
        let blocks = ChannelStorage::uniform(STONE);
        let neighbour_blocks = ChannelStorage::uniform(AIR);
        let neighbour_light = brick_from(|x, _, _| if x == 0 { 90 } else { 10 });
        let mesh = mesh_brick(
            &BrickNeighbourhood {
                center: only(&blocks),
                neighbours: [
                    None,
                    Some(BrickChannels {
                        blocks: &neighbour_blocks,
                        states: None,
                        orientations: None,
                        light: Some(&neighbour_light),
                    }),
                    None,
                    None,
                    None,
                    None,
                ],
            },
            &table(),
        );
        let east: Vec<&Quad> = mesh
            .quads
            .iter()
            .filter(|quad| quad.face == FaceDirection::PositiveX)
            .collect();
        assert_eq!(east.len(), 1);
        assert_eq!(
            east[0].light, 90,
            "the layer touching the brick, not the one behind it"
        );
    }

    #[test]
    fn a_slab_is_lit_by_what_it_looks_at_on_its_boundary_and_by_its_own_voxel_inside() {
        let (blocks, states) = rock_in_a_voxel(1);
        let light = brick_from(|x, y, z| match (x, y, z) {
            (5, 6, 7) => 70,
            (5, 7, 7) => 150,
            _ => 20,
        });
        let mesh = mesh_brick(
            &BrickNeighbourhood {
                center: BrickChannels {
                    blocks: &blocks,
                    states: Some(&states),
                    orientations: None,
                    light: Some(&light),
                },
                neighbours: [None; 6],
            },
            &table(),
        );
        let light_of = |face| {
            mesh.detailed
                .iter()
                .find(|quad| quad.face == face)
                .unwrap()
                .light
        };
        assert_eq!(
            light_of(FaceDirection::PositiveY),
            70,
            "the top is inside the voxel"
        );
        assert_eq!(
            light_of(FaceDirection::NegativeY),
            20,
            "the bottom looks down at the voxel below"
        );
        assert_eq!(light_of(FaceDirection::PositiveX), 20);
    }

    // ----- Whole-mesh properties -----

    /// A deterministic pseudo-random brick: the proptest shrinker only needs to shrink the seed.
    fn random_brick(seed: u64, solid_per_mille: u64, materials: u64) -> Vec<u32> {
        let mut state = seed | 1;
        (0..BRICK_VOXEL_COUNT)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                if state % 1000 < solid_per_mille {
                    1 + (state >> 20) as u32 % materials as u32
                } else {
                    AIR
                }
            })
            .collect()
    }

    fn brick_model() -> impl Strategy<Value = Vec<u32>> {
        prop_oneof![
            1 => prop_oneof![Just(AIR), Just(STONE), Just(DIRT)].prop_map(|value| vec![value; BRICK_VOXEL_COUNT]),
            4 => (any::<u64>(), prop_oneof![Just(20u64), Just(500), Just(980)], 1u64..4)
                .prop_map(|(seed, density, materials)| random_brick(seed, density, materials)),
        ]
    }

    /// A rectangle of sub-faces of one face of one voxel: its layer of sub-cells, where it starts and
    /// how big it is.
    type Piece = (u8, u8, u8, u8, u8);

    /// What is exposed, found by looking at the world the slow obvious way: for every voxel of the
    /// centre brick and every face, the rectangles of that face that show, with their block and
    /// brightness.
    type Pieces = HashMap<([usize; 3], FaceDirection, Piece), (u32, u8)>;
    /// The blocks, states and light of a neighbouring brick, as flat arrays.
    type BrickArrays = (Vec<u32>, Vec<u32>, Vec<u8>);

    struct World<'a> {
        table: &'a BlockTable,
        center: &'a [u32],
        states: &'a [u32],
        light: &'a [u8],
        neighbours: &'a [Option<BrickArrays>; 6],
    }

    impl World<'_> {
        /// The model and light at a position at most one voxel outside the centre brick.
        fn at(&self, p: [i64; 3]) -> Option<(ModelId, u8)> {
            let outside: Vec<usize> = (0..3)
                .filter(|&a| !(0..EDGE as i64).contains(&p[a]))
                .collect();
            let (blocks, states, light, wrapped) = match outside[..] {
                [] => (self.center, self.states, self.light, p),
                [axis] => {
                    let face =
                        FaceDirection::from_index((2 * axis + usize::from(p[axis] >= 0)) as u8)
                            .unwrap();
                    let (blocks, states, light) =
                        self.neighbours[usize::from(face.index())].as_ref()?;
                    let mut wrapped = p;
                    wrapped[axis] = p[axis].rem_euclid(EDGE as i64);
                    (
                        blocks.as_slice(),
                        states.as_slice(),
                        light.as_slice(),
                        wrapped,
                    )
                }
                _ => unreachable!("faces only look one voxel along one axis"),
            };
            let index = local([
                wrapped[0] as usize,
                wrapped[1] as usize,
                wrapped[2] as usize,
            ])
            .index();
            Some((
                self.table.model_of(blocks[index], states[index], 0),
                light[index],
            ))
        }
    }

    fn expected_pieces(world: &World<'_>) -> Pieces {
        let mut pieces = Pieces::new();
        for z in 0..EDGE {
            for y in 0..EDGE {
                for x in 0..EDGE {
                    let index = local([x, y, z]).index();
                    let model = world
                        .table
                        .model_of(world.center[index], world.states[index], 0);
                    if !world.table.is_solid(model) {
                        continue;
                    }
                    let shape = world.table.shape(model);
                    for face in FaceDirection::ALL {
                        let [dx, dy, dz] = face.offset();
                        let front = world.at([x as i64 + dx, y as i64 + dy, z as i64 + dz]);
                        let hides = front.map_or(0, |(neighbour, _)| {
                            world.table.hidden_by_neighbour(model, face, neighbour)
                        });
                        let front_light = front.map_or(FULL_LIGHT, |(_, light)| light);
                        let boundary = if face.is_positive() {
                            SUBDIVISIONS - 1
                        } else {
                            0
                        };
                        let whole = [block_models::SubRect {
                            layer: boundary as u8,
                            u: 0,
                            v: 0,
                            width: 8,
                            height: 8,
                        }];
                        let rectangles: &[block_models::SubRect] =
                            if world.table.is_opaque_cube(model) {
                                &whole
                            } else {
                                shape.exposed(face)
                            };
                        for rectangle in rectangles {
                            let on_boundary = rectangle.on_boundary(face);
                            let wanted = rectangle.mask();
                            if on_boundary && hides & wanted == wanted {
                                continue;
                            }
                            let light = if on_boundary {
                                front_light
                            } else {
                                world.light[index]
                            };
                            pieces.insert(
                                (
                                    [x, y, z],
                                    face,
                                    (
                                        rectangle.layer,
                                        rectangle.u,
                                        rectangle.v,
                                        rectangle.width,
                                        rectangle.height,
                                    ),
                                ),
                                (world.center[index], light),
                            );
                        }
                    }
                }
            }
        }
        pieces
    }

    /// The same pieces, read back out of a mesh; also how many were covered, counting repeats.
    fn mesh_pieces(mesh: &BrickMesh) -> (Pieces, usize) {
        let mut pieces = Pieces::new();
        let mut total = 0;
        for quad in &mesh.quads {
            let (u_axis, v_axis) = quad_axes(quad.face);
            let boundary = if quad.face.is_positive() {
                SUBDIVISIONS - 1
            } else {
                0
            };
            for j in 0..usize::from(quad.height) {
                for i in 0..usize::from(quad.width) {
                    let mut voxel = [
                        usize::from(quad.origin[0]),
                        usize::from(quad.origin[1]),
                        usize::from(quad.origin[2]),
                    ];
                    voxel[u_axis] += i;
                    voxel[v_axis] += j;
                    pieces.insert(
                        (voxel, quad.face, (boundary as u8, 0, 0, 8, 8)),
                        (quad.material, quad.light),
                    );
                    total += 1;
                }
            }
        }
        for quad in &mesh.detailed {
            let (u_axis, v_axis) = quad_axes(quad.face);
            let axis = quad.face.axis();
            // The plane of a positive face is the high side of the sub-cell layer it belongs to.
            let layer_position =
                usize::from(quad.origin[axis]) - usize::from(quad.face.is_positive());
            let (u_position, v_position) = (
                usize::from(quad.origin[u_axis]),
                usize::from(quad.origin[v_axis]),
            );
            let mut voxel = [0usize; 3];
            voxel[axis] = layer_position / SUBDIVISIONS;
            voxel[u_axis] = u_position / SUBDIVISIONS;
            voxel[v_axis] = v_position / SUBDIVISIONS;
            pieces.insert(
                (
                    voxel,
                    quad.face,
                    (
                        (layer_position % SUBDIVISIONS) as u8,
                        (u_position % SUBDIVISIONS) as u8,
                        (v_position % SUBDIVISIONS) as u8,
                        quad.width as u8,
                        quad.height as u8,
                    ),
                ),
                (quad.material, quad.light),
            );
            total += 1;
        }
        (pieces, total)
    }

    /// A random brick of every kind of block, with states and light.
    fn mixed_brick() -> impl Strategy<Value = (Vec<u32>, Vec<u32>, Vec<u8>)> {
        prop_oneof![
            1 => prop_oneof![Just(AIR), Just(STONE), Just(ROCK), Just(GLASS)]
                .prop_map(|block| (vec![block; BRICK_VOXEL_COUNT], vec![1; BRICK_VOXEL_COUNT], vec![255; BRICK_VOXEL_COUNT])),
            5 => (any::<u64>(), prop_oneof![Just(100u64), Just(600), Just(950)], 1u64..4).prop_map(|(seed, density, light_levels)| {
                let mut state = seed | 1;
                let mut next = || {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    state
                };
                let mut blocks = Vec::new();
                let mut states = Vec::new();
                let mut light = Vec::new();
                for _ in 0..BRICK_VOXEL_COUNT {
                    let roll = next();
                    blocks.push(if roll % 1000 < density { [STONE, ROCK, ROCK, GLASS, DIRT][(roll >> 10) as usize % 5] } else { AIR });
                    states.push(((roll >> 20) % 5) as u32);
                    light.push(((roll >> 30) % light_levels * 60) as u8);
                }
                (blocks, states, light)
            }),
        ]
    }

    type Storages = (ChannelStorage, ChannelStorage, ChannelStorage);

    /// The channels of a brick that has block states and light.
    fn channels_of(storages: &Storages) -> BrickChannels<'_> {
        BrickChannels {
            blocks: &storages.0,
            states: Some(&storages.1),
            orientations: None,
            light: Some(&storages.2),
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(24))]

        /// The mesh covers exactly what shows: every sub-face of every rectangle of a block that is
        /// not hidden by its neighbour, none twice, none invented, each with its block's colour and the
        /// brightness of the voxel it looks at (or, inside a voxel, the voxel's own).
        #[test]
        fn the_mesh_covers_exactly_what_shows_with_the_right_block_and_light(
            center in mixed_brick(),
            neighbours in proptest::array::uniform6(prop_oneof![1 => Just(None), 3 => mixed_brick().prop_map(Some)]),
        ) {
            let table = table();
            let storages = |blocks: &[u32], states: &[u32], light: &[u8]| {
                (
                    ChannelStorage::from_values(blocks),
                    ChannelStorage::from_values(states),
                    ChannelStorage::from_values(&light.iter().map(|&value| u32::from(value)).collect::<Vec<_>>()),
                )
            };
            let center_storages = storages(&center.0, &center.1, &center.2);
            let neighbour_storages: Vec<Option<_>> = neighbours
                .iter()
                .map(|brick| brick.as_ref().map(|(blocks, states, light)| storages(blocks, states, light)))
                .collect();
            let input = BrickNeighbourhood {
                center: channels_of(&center_storages),
                neighbours: std::array::from_fn(|index| neighbour_storages[index].as_ref().map(channels_of)),
            };
            let mesh = mesh_brick(&input, &table);
            let (covered, total) = mesh_pieces(&mesh);
            let world = World {
                table: &table,
                center: &center.0,
                states: &center.1,
                light: &center.2,
                neighbours: &neighbours,
            };
            let expected = expected_pieces(&world);
            prop_assert_eq!(total, covered.len(), "something is covered twice");
            prop_assert_eq!(covered.len(), expected.len());
            prop_assert!(covered == expected, "what the mesh covers is not what shows");
        }

        #[test]
        fn greedy_merging_never_uses_more_quads_than_exposed_faces(center in brick_model()) {
            let storage = ChannelStorage::from_values(&center);
            let mesh = isolated_mesh(&storage);
            let exposed = {
                let neighbours: [Option<BrickArrays>; 6] = Default::default();
                let table = table();
                let states = vec![0; BRICK_VOXEL_COUNT];
                let light = vec![FULL_LIGHT; BRICK_VOXEL_COUNT];
                let world = World { table: &table, center: &center, states: &states, light: &light, neighbours: &neighbours };
                expected_pieces(&world).len()
            };
            prop_assert!(mesh.quads.len() <= exposed);
            prop_assert!(mesh.detailed.is_empty());
        }
    }
}
