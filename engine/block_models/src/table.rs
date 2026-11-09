use std::collections::HashMap;
use std::marker::PhantomData;

use voxel_space::{FaceDirection, LatticeRotation};

use crate::definition::{BlockDefinition, FacePictures, TileRef};
use crate::property::engine::{self, face_from_code};
use crate::property::{
    Column, PropertyError, PropertyHandle, PropertyKey, PropertySchema, PropertyType, check_value,
    key_id,
};
use crate::shape::{FaceConnections, Shape, ShapeBox};
use crate::transparency::Transparency;

/// The colour of a block this program has no definition for (content from a plugin that is not
/// loaded): magenta, so that unknown content is seen rather than vanishing.
pub const MISSING_BLOCK_COLOR: [f32; 3] = [1.0, 0.0, 1.0];

/// How many orientations a voxel can have: the 24 turns of the lattice, for every block, whether it looks different in all of them or in none (a
/// whole cube does not). An orientation is the number of a turn in [`LatticeRotation::all`], and 0 is none.
pub const ORIENTATION_COUNT: usize = LatticeRotation::COUNT;

/// One shape of one block as it stands, as the table holds it: what a voxel with a given block, state and orientation is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ModelId(u32);

impl ModelId {
    pub fn get(self) -> u32 {
        self.0
    }

    /// The number of the block-and-state this model is a standing of: the same for all the orientations of it, and what the columns of properties
    /// are indexed by.
    fn base(self) -> usize {
        self.0 as usize / ORIENTATION_COUNT
    }
}

/// How a block is turned when a player places it, from its definition (see [`BlockTable::placement`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// As it is made: in no turn.
    Fixed,
    /// A block with one front face, like a piston (`engine:front_face`): the front faces the player.
    FrontToPlayer(FaceDirection),
    /// A block with a line of radial symmetry, like a log (`engine:axis_face`): the line runs along the face it is placed against.
    AlongPlacedFace(FaceDirection),
    /// A block with both, like the docking port: its `axis` (its top) goes along the face it is placed against, away from it, and its `front` is turned to the side
    /// the player is on, as far as the axis allows (the front is one of the four sides of the axis).
    AlongPlacedFaceFacingPlayer {
        axis: FaceDirection,
        front: FaceDirection,
    },
}

/// A picture on one face of a voxel: which tile, and how the image runs across the face. `across` is the direction along the face in which the picture's
/// left-to-right goes, and `up` the direction its bottom-to-top goes; they are directions of the grid the voxel is in, and `across x up` is the outward normal of
/// the face, so a picture is never mirrored. The picture of a face is turned with the voxel (a log on its side has the bark running along it).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FacePicture {
    pub tile: TileRef,
    pub across: FaceDirection,
    pub up: FaceDirection,
    /// Whether the clear pixels of the picture are holes (the block's [`Transparency`] is not the opaque one); an opaque block draws its picture whole.
    pub cut_out: bool,
}

impl FacePicture {
    /// The picture as one number of 18 bits that a palette entry holds for the GPU: bit 0 is set (a palette entry with none has 0), bits 1 and 2 the number of the
    /// tile's size, 3 to 10 its place among those of that size, 11 to 13 the index of `across`, 14 to 16 the index of `up` (see [`FaceDirection::index`]), and bit 17
    /// is set if the clear pixels are holes.
    pub fn word(self) -> u32 {
        1 | (self.tile.class() as u32) << 1
            | (self.tile.layer() as u32) << 3
            | u32::from(self.across.index()) << 11
            | u32::from(self.up.index()) << 14
            | u32::from(self.cut_out) << 17
    }

    /// The picture a word names; `None` for 0 (no picture) and for a number that is not a word of this kind.
    pub fn from_word(word: u32) -> Option<Self> {
        if word & 1 == 0 || word >> 18 != 0 {
            return None;
        }
        Some(Self {
            tile: TileRef::new((word >> 1 & 3) as usize, (word >> 3 & 255) as usize)?,
            across: FaceDirection::from_index((word >> 11 & 7) as u8)?,
            up: FaceDirection::from_index((word >> 14 & 7) as u8)?,
            cut_out: word >> 17 & 1 == 1,
        })
    }
}

/// How the picture of a face of a block as it is made runs across it: `(across, up)`. The sides have the picture upright (up is +y) and the left-to-right of someone
/// looking at the face from outside; the top has north (-z) at the top of the picture, and the bottom the same, seen from below.
fn made_image_axes(face: FaceDirection) -> (FaceDirection, FaceDirection) {
    use FaceDirection::{NegativeX, NegativeY, NegativeZ, PositiveX, PositiveY, PositiveZ};
    match face {
        PositiveX => (NegativeZ, PositiveY),
        NegativeX => (PositiveZ, PositiveY),
        PositiveZ => (PositiveX, PositiveY),
        NegativeZ => (NegativeX, PositiveY),
        PositiveY => (PositiveX, NegativeZ),
        NegativeY => (NegativeX, NegativeZ),
    }
}

#[derive(Clone, Debug)]
struct Entry {
    color: [f32; 3],
    /// The number of the first state's base; a block's states are one after another.
    first_base: u32,
    state_count: u32,
    /// Where this block's state colours start in the palette, and how many there are (0 if the
    /// block is one colour).
    first_state_material: u32,
    state_color_count: u32,
}

impl Entry {
    /// What a voxel of this block in `state` is drawn as (see [`BlockTable::material_of`]).
    fn material_of(&self, block: u32, state: u32) -> u32 {
        if self.state_color_count > 0 {
            self.first_state_material + state % self.state_color_count
        } else {
            block
        }
    }
}

#[derive(Clone, Debug)]
struct Model {
    /// The shape, in the pool of the table.
    shape: u32,
    block: u32,
    sight_through: bool,
    light_through: bool,
    transparency: Transparency,
    /// The sub-faces on the voxel's boundary that hide what is behind them: the shape's, unless the
    /// block is sight-through.
    hides: [u64; 6],
    opaque_cube: bool,
    /// What each face of the voxel is drawn as (indexed by [`FaceDirection::index`]), for a block that colours faces; `None` if every face is the voxel's.
    face_materials: Option<[u32; 6]>,
}

/// Every block's definition, turned into what the mesher, the renderer, lighting and physics ask
/// for most: by block id, state and orientation, quickly. Ids are the runtime ids of a game's block registry; an
/// id the table has no definition for is an unknown block, drawn as an opaque magenta cube in every orientation.
#[derive(Clone, Debug)]
pub struct BlockTable {
    entries: Vec<Entry>,
    /// The distinct shapes of all the models.
    shapes: Vec<Shape>,
    /// A model for each block and state and each orientation of it: base `b` standing in orientation `o` is `b * ORIENTATION_COUNT + o`.
    models: Vec<Model>,
    unknown: ModelId,
    /// The colours after the blocks' own: every block's state colours, in block order, and then the colours of faces.
    state_palette: Vec<[f32; 3]>,
    /// The entries after those: a colour with a picture on it (a [`FacePicture`] word), one for each colour and picture a face of some model has.
    picture_palette: Vec<([f32; 3], u32)>,
    /// The properties, a column for each, with a value for every block-and-state (not for every orientation).
    columns: Vec<Column>,
    /// The columns of the face properties of the engine, if the schema declares them.
    docking_face_column: Option<usize>,
    front_face_column: Option<usize>,
    axis_face_column: Option<usize>,
    rotations: [LatticeRotation; ORIENTATION_COUNT],
}

impl BlockTable {
    /// A table of these blocks, in id order, with only the engine's properties declared. `None` is a block
    /// whose definition is not available. Panics if a definition sets a property that is not declared
    /// (see [`BlockTable::try_from_definitions`]).
    pub fn from_definitions(definitions: &[Option<&BlockDefinition>]) -> Self {
        Self::try_from_definitions(&PropertySchema::new(), definitions)
            .unwrap_or_else(|error| panic!("the block definitions cannot be used: {error}"))
    }

    /// A table of these blocks, in id order, with the properties `schema` declares. `None` is a block whose
    /// definition is not available. An error if a definition sets a property the schema does not declare, a
    /// value it does not take, or a state its block does not have.
    pub fn try_from_definitions(
        schema: &PropertySchema,
        definitions: &[Option<&BlockDefinition>],
    ) -> Result<Self, PropertyError> {
        // The block-and-states ("bases"): their shapes, and whether their blocks are see-through.
        struct Base<'a> {
            shape: &'a Shape,
            block: u32,
            state: u32,
            sight_through: bool,
            light_through: bool,
            transparency: Transparency,
        }
        let unknown_shape = Shape::full();
        let mut bases: Vec<Base<'_>> = Vec::new();
        let mut entries = Vec::with_capacity(definitions.len());
        let mut state_palette = Vec::new();
        for (block, definition) in definitions.iter().enumerate() {
            let Some(definition) = definition else {
                entries.push(Entry {
                    color: MISSING_BLOCK_COLOR,
                    first_base: u32::MAX,
                    state_count: 1,
                    first_state_material: 0,
                    state_color_count: 0,
                });
                continue;
            };
            let first_base = bases.len() as u32;
            for (state, shape) in definition.shapes().iter().enumerate() {
                bases.push(Base {
                    shape,
                    block: block as u32,
                    state: state as u32,
                    sight_through: definition.sight_through,
                    light_through: definition.light_through,
                    transparency: definition.transparency(),
                });
            }
            entries.push(Entry {
                color: definition.color,
                first_base,
                state_count: definition.state_count() as u32,
                first_state_material: (definitions.len() + state_palette.len()) as u32,
                state_color_count: definition.state_colors().len() as u32,
            });
            state_palette.extend_from_slice(definition.state_colors());
        }
        // The base of every unknown block is added last, once.
        let unknown_base = bases.len();
        bases.push(Base {
            shape: &unknown_shape,
            block: u32::MAX,
            state: 0,
            sight_through: false,
            light_through: false,
            transparency: Transparency::Opaque,
        });
        for entry in &mut entries {
            if entry.first_base == u32::MAX {
                entry.first_base = unknown_base as u32;
            }
        }
        // The properties: a column for each declared key, the default everywhere (so for unknown blocks),
        // then what each definition sets, for each of its states.
        let mut columns: Vec<Column> = schema
            .iter()
            .map(|declaration| Column {
                id: declaration.id.clone(),
                kind: declaration.kind,
                values: vec![declaration.default; bases.len()],
            })
            .collect();
        for (block, definition) in definitions.iter().enumerate() {
            let Some(definition) = definition else {
                continue;
            };
            let entry = &entries[block];
            for (key, set) in definition.properties() {
                let declaration = schema
                    .get(key)
                    .ok_or_else(|| PropertyError::Undeclared(key.clone()))?;
                let column = columns
                    .iter_mut()
                    .find(|column| column.id == *key)
                    .expect("every declared key has a column");
                for value in set.every_state.iter().chain(set.by_state.values()) {
                    check_value(declaration, *value)?;
                }
                if let Some(&state) = set
                    .by_state
                    .keys()
                    .find(|&&state| state >= entry.state_count)
                {
                    return Err(PropertyError::NoSuchState {
                        key: key.clone(),
                        block: block as u32,
                        state,
                        states: entry.state_count,
                    });
                }
                for state in 0..entry.state_count {
                    if let Some(&value) = set.by_state.get(&state).or(set.every_state.as_ref()) {
                        column.values[(entry.first_base + state) as usize] = value;
                    }
                }
            }
        }
        // The coloured faces: each distinct colour a block gives a face takes one palette entry after all the state colours, in the order of the
        // blocks and then of the faces.
        let first_face_material = (definitions.len() + state_palette.len()) as u32;
        let mut face_palette: Vec<[f32; 3]> = Vec::new();
        let mut block_face_materials: Vec<Option<[Option<u32>; 6]>> = vec![None; definitions.len()];
        for (block, definition) in definitions.iter().enumerate() {
            let Some(definition) = definition else {
                continue;
            };
            if definition.face_colors().iter().all(Option::is_none) {
                continue;
            }
            block_face_materials[block] = Some(definition.face_colors().map(|color| {
                let color = color?;
                let bits = color.map(f32::to_bits);
                let position = face_palette
                    .iter()
                    .position(|known| known.map(f32::to_bits) == bits)
                    .unwrap_or_else(|| {
                        face_palette.push(color);
                        face_palette.len() - 1
                    });
                Some(first_face_material + position as u32)
            }));
        }
        state_palette.extend(face_palette);
        // The pictures: a face with a picture is a palette entry of its own, the colour it would have flat and the picture, and the same colour and picture are one entry.
        let block_pictures: Vec<Option<FacePictures>> = definitions
            .iter()
            .map(|definition| definition.and_then(|definition| definition.pictures().copied()))
            .collect();
        let first_picture_material = (definitions.len() + state_palette.len()) as u32;
        let mut picture_palette: Vec<([f32; 3], u32)> = Vec::new();
        let mut picture_materials: HashMap<([u32; 3], u32), u32> = HashMap::new();
        let colour_of_material = |material: u32| -> [f32; 3] {
            let material = material as usize;
            entries.get(material).map_or_else(
                || state_palette[material - definitions.len()],
                |entry| entry.color,
            )
        };
        // The models: each base in each orientation. Shapes that come out the same are one shape of the pool (a whole cube is one however it is turned).
        let rotations = LatticeRotation::all();
        let mut shapes: Vec<Shape> = Vec::new();
        let mut pool: HashMap<([u64; 8], Vec<ShapeBox>), u32> = HashMap::new();
        let mut turned_shapes: HashMap<(u32, usize), u32> = HashMap::new();
        let mut pooled = |shape: &Shape, shapes: &mut Vec<Shape>| -> u32 {
            let key = (shape.cell_key(), shape.boxes().to_vec());
            *pool.entry(key).or_insert_with(|| {
                shapes.push(shape.clone());
                shapes.len() as u32 - 1
            })
        };
        let mut models = Vec::with_capacity(bases.len() * ORIENTATION_COUNT);
        for details in &bases {
            let own = pooled(details.shape, &mut shapes);
            for (orientation, rotation) in rotations.iter().enumerate() {
                let shape = if orientation == 0 {
                    own
                } else if let Some(known) = turned_shapes.get(&(own, orientation)) {
                    *known
                } else {
                    let turned = details.shape.turned(*rotation);
                    let found = pooled(&turned, &mut shapes);
                    turned_shapes.insert((own, orientation), found);
                    found
                };
                // The faces as the block is made: its own coloured ones, and for the rest the material of the voxel, and with its picture if it has one; each goes
                // where the turn takes it, and so does the way its picture runs.
                let coloured = block_face_materials
                    .get(details.block as usize)
                    .copied()
                    .flatten();
                let pictures = block_pictures
                    .get(details.block as usize)
                    .copied()
                    .flatten();
                let face_materials = (coloured.is_some() || pictures.is_some()).then(|| {
                    let voxel =
                        entries[details.block as usize].material_of(details.block, details.state);
                    let mut turned = [voxel; 6];
                    for face in FaceDirection::ALL {
                        let flat = coloured
                            .and_then(|made| made[usize::from(face.index())])
                            .unwrap_or(voxel);
                        let material = match pictures {
                            None => flat,
                            Some(pictures) => {
                                let tile = match face {
                                    FaceDirection::PositiveY => pictures.top,
                                    FaceDirection::NegativeY => pictures.bottom,
                                    _ => pictures.side,
                                };
                                let (across, up) = made_image_axes(face);
                                let word = FacePicture {
                                    tile,
                                    across: rotation.apply_face(across),
                                    up: rotation.apply_face(up),
                                    cut_out: details.transparency.cuts_holes(),
                                }
                                .word();
                                let colour = colour_of_material(flat);
                                *picture_materials
                                    .entry((colour.map(f32::to_bits), word))
                                    .or_insert_with(|| {
                                        picture_palette.push((colour, word));
                                        first_picture_material + picture_palette.len() as u32 - 1
                                    })
                            }
                        };
                        turned[usize::from(rotation.apply_face(face).index())] = material;
                    }
                    turned
                });
                models.push(make_model(
                    &shapes[shape as usize],
                    shape,
                    (details.sight_through, details.light_through),
                    details.block,
                    details.transparency,
                    face_materials,
                ));
            }
        }
        let find = |key: &'static str| position_of(&columns, key);
        let (docking_face_column, front_face_column, axis_face_column) = (
            find(engine::DOCKING_FACE.id()),
            find(engine::FRONT_FACE.id()),
            find(engine::AXIS_FACE.id()),
        );
        Ok(Self {
            entries,
            shapes,
            models,
            unknown: ModelId((unknown_base * ORIENTATION_COUNT) as u32),
            state_palette,
            picture_palette,
            columns,
            docking_face_column,
            front_face_column,
            axis_face_column,
            rotations,
        })
    }

    /// Finds a property's column, to read through [`BlockTable::get`]. An error if the table's schema does
    /// not declare the key, or declares it as another kind than `T`.
    pub fn handle<T: PropertyType>(
        &self,
        key: &PropertyKey<T>,
    ) -> Result<PropertyHandle<T>, PropertyError> {
        let id = key_id(key.id());
        let (position, column) = self
            .columns
            .iter()
            .enumerate()
            .find(|(_, column)| column.id == id)
            .ok_or_else(|| PropertyError::Undeclared(id.clone()))?;
        if column.kind != key.kind() {
            return Err(PropertyError::WrongKind {
                key: id,
                declared: column.kind,
                asked: key.kind(),
            });
        }
        Ok(PropertyHandle {
            column: position as u32,
            marker: PhantomData,
        })
    }

    /// A property of a block in a state, by its model (see [`BlockTable::model_of`]): an array index. The
    /// handle must be from this table. (A property does not depend on the orientation; the faces that the engine's face properties name do, and are
    /// read with [`BlockTable::docking_face`] and the like.)
    pub fn get<T: PropertyType>(&self, handle: PropertyHandle<T>, model: ModelId) -> T {
        T::from_value(self.columns[handle.column as usize].values[model.base()])
    }

    /// A property of a block in a state, for a caller that has the block and state and not the model: the key's
    /// default for an unknown block, and the state 0 value for a state the block does not have.
    pub fn get_of<T: PropertyType>(&self, handle: PropertyHandle<T>, block: u32, state: u32) -> T {
        self.get(handle, self.model_of(block, state, 0))
    }

    /// A table of whole cubes, one per flag: opaque where the flag is set, and air where it is not.
    /// For tests and tools that care about nothing else.
    pub fn of_cubes(opaque: &[bool]) -> Self {
        let cube = BlockDefinition::cube([0.5; 3]);
        let air = BlockDefinition::air();
        let definitions: Vec<Option<&BlockDefinition>> = opaque
            .iter()
            .map(|&opaque| Some(if opaque { &cube } else { &air }))
            .collect();
        Self::from_definitions(&definitions)
    }

    /// How many block ids the table has definitions for (or placeholders).
    pub fn block_count(&self) -> usize {
        self.entries.len()
    }

    /// How many states a block has. An unknown block has one.
    pub fn state_count(&self, block: u32) -> u32 {
        self.entries
            .get(block as usize)
            .map_or(1, |entry| entry.state_count)
    }

    /// The model of a block in a state, standing in an orientation (the number of a turn in [`LatticeRotation::all`]; one that is not a turn is
    /// none). A state the block does not have is its state 0, and an unknown block is an opaque cube.
    pub fn model_of(&self, block: u32, state: u32, orientation: u32) -> ModelId {
        let Some(entry) = self.entries.get(block as usize) else {
            return self.unknown;
        };
        let state = if state < entry.state_count { state } else { 0 };
        let orientation = if (orientation as usize) < ORIENTATION_COUNT {
            orientation
        } else {
            0
        };
        ModelId((entry.first_base + state) * ORIENTATION_COUNT as u32 + orientation)
    }

    /// What a voxel is drawn as: a value to give the renderer's palette. It is the block id, unless the
    /// block has colours for its states, and then one of the palette entries after the blocks'.
    /// Voxels are drawn in the same colour exactly when their materials are equal.
    pub fn material_of(&self, block: u32, state: u32) -> u32 {
        self.entries
            .get(block as usize)
            .map_or(block, |entry| entry.material_of(block, state))
    }

    /// What a face of a voxel is drawn as: the material of the voxel (see [`BlockTable::material_of`]), unless the block gives the face a colour of its own.
    pub fn material_of_face(
        &self,
        block: u32,
        state: u32,
        orientation: u32,
        face: FaceDirection,
    ) -> u32 {
        match self.face_materials(block, state, orientation) {
            Some(materials) => materials[usize::from(face.index())],
            None => self.material_of(block, state),
        }
    }

    /// What each face of a voxel is drawn as, indexed by [`FaceDirection::index`], for a block that gives faces colours of their own (turned with the
    /// voxel's orientation); `None` for a block whose faces are all the voxel's material.
    pub fn face_materials(&self, block: u32, state: u32, orientation: u32) -> Option<[u32; 6]> {
        self.models[self.model_of(block, state, orientation).0 as usize].face_materials
    }

    /// A face that a property of the engine names (it is in the orientation of the block as it is made, and is turned with the voxel).
    fn turned_face(
        &self,
        column: Option<usize>,
        block: u32,
        state: u32,
        orientation: u32,
    ) -> Option<FaceDirection> {
        let model = self.model_of(block, state, orientation);
        let value = self.columns[column?].values[model.base()];
        let face = u8::try_from(value).ok().and_then(face_from_code)?;
        Some(self.rotations[model.0 as usize % ORIENTATION_COUNT].apply_face(face))
    }

    /// The face of a block that docks, in a state and an orientation (`engine:docking_face`): `None` for a block that is no docking port.
    pub fn docking_face(&self, block: u32, state: u32, orientation: u32) -> Option<FaceDirection> {
        self.turned_face(self.docking_face_column, block, state, orientation)
    }

    /// The front of a block that has one (`engine:front_face`), in a state and an orientation.
    pub fn front_face(&self, block: u32, state: u32, orientation: u32) -> Option<FaceDirection> {
        self.turned_face(self.front_face_column, block, state, orientation)
    }

    /// One end of the line of symmetry of a block that has one (`engine:axis_face`), in a state and an orientation.
    pub fn axis_face(&self, block: u32, state: u32, orientation: u32) -> Option<FaceDirection> {
        self.turned_face(self.axis_face_column, block, state, orientation)
    }

    /// How a block is turned when a player places it: by its front, by its axis, by both, or not at all.
    pub fn placement(&self, block: u32, state: u32) -> Placement {
        match (
            self.front_face(block, state, 0),
            self.axis_face(block, state, 0),
        ) {
            (Some(front), Some(axis)) => Placement::AlongPlacedFaceFacingPlayer { axis, front },
            (Some(front), None) => Placement::FrontToPlayer(front),
            (None, Some(axis)) => Placement::AlongPlacedFace(axis),
            (None, None) => Placement::Fixed,
        }
    }

    pub fn shape(&self, model: ModelId) -> &Shape {
        &self.shapes[self.models[model.0 as usize].shape as usize]
    }

    /// Whether the model has any shape at all (it is not air).
    pub fn is_solid(&self, model: ModelId) -> bool {
        !self.shape(model).is_empty()
    }

    /// Whether the model is a whole cube that hides what is behind it.
    pub fn is_opaque_cube(&self, model: ModelId) -> bool {
        self.models[model.0 as usize].opaque_cube
    }

    /// Whether the block hides nothing behind it (glass, leaves, air).
    pub fn is_sight_through(&self, model: ModelId) -> bool {
        self.models[model.0 as usize].sight_through
    }

    /// Whether light passes through the block and the voxel holds light itself.
    pub fn is_light_through(&self, model: ModelId) -> bool {
        self.models[model.0 as usize].light_through
    }

    /// The sub-faces of `face` that the model hides what is behind (the shape's covered sub-faces,
    /// none if the block is see-through); see [`Shape::face_cover`].
    pub fn hides(&self, model: ModelId, face: FaceDirection) -> u64 {
        self.models[model.0 as usize].hides[usize::from(face.index())]
    }

    /// Whether the model hides everything behind its `face`.
    pub fn hides_fully(&self, model: ModelId, face: FaceDirection) -> bool {
        self.hides(model, face) == u64::MAX
    }

    /// How the model's block shows its faces and the clear pixels of its pictures.
    pub fn transparency(&self, model: ModelId) -> Transparency {
        self.models[model.0 as usize].transparency
    }

    /// Whether the faces between two voxels of the model's block are drawn, though the block is sight-through ([`Transparency::shows_inner_faces`]).
    pub fn shows_inner_faces(&self, model: ModelId) -> bool {
        self.transparency(model).shows_inner_faces()
    }

    /// Whether the model's faces are drawn also to someone behind them, inside the voxel ([`Transparency::shows_back_faces`]).
    pub fn shows_back_faces(&self, model: ModelId) -> bool {
        self.transparency(model).shows_back_faces()
    }

    /// Which sub-faces of `model`'s boundary `face` are hidden by `neighbour`, the model of the voxel
    /// in front of it (bit `v * 8 + u`, as for [`Shape::face_cover`]). An opaque neighbour hides what
    /// its shape covers; a see-through neighbour hides only faces of the same block, so that a body
    /// of glass shows its outside and not every face of every voxel inside it.
    pub fn hidden_by_neighbour(
        &self,
        model: ModelId,
        face: FaceDirection,
        neighbour: ModelId,
    ) -> u64 {
        let own = &self.models[model.0 as usize];
        let other = &self.models[neighbour.0 as usize];
        if !other.sight_through {
            other.hides[usize::from(face.opposite().index())]
        } else if own.sight_through && own.block == other.block {
            if own.transparency.shows_inner_faces() {
                0
            } else {
                self.shape(neighbour).face_cover(face.opposite())
            }
        } else {
            0
        }
    }

    /// The faces light and other fills can pass between through the model: all of them for a
    /// light-through block, else the ones its empty part joins.
    pub fn connections(&self, model: ModelId) -> FaceConnections {
        if self.models[model.0 as usize].light_through {
            FaceConnections::all()
        } else {
            self.shape(model).connections()
        }
    }

    pub fn color(&self, block: u32) -> [f32; 3] {
        self.entries
            .get(block as usize)
            .map_or(MISSING_BLOCK_COLOR, |entry| entry.color)
    }

    /// The renderer's palette: the colour of every block in id order, then the state colours of the
    /// blocks that have them. [`BlockTable::material_of`] gives positions in it.
    pub fn palette(&self) -> Vec<[f32; 3]> {
        self.entries
            .iter()
            .map(|entry| entry.color)
            .chain(self.state_palette.iter().copied())
            .chain(self.picture_palette.iter().map(|(color, _)| *color))
            .collect()
    }

    /// For each entry of [`BlockTable::palette`], the [`FacePicture`] word of the picture on it, or 0 for a flat colour: the entries of blocks and of states and of the
    /// colours of faces have none; those after them are a colour and a picture.
    pub fn palette_pictures(&self) -> Vec<u32> {
        let flat = self.entries.len() + self.state_palette.len();
        std::iter::repeat_n(0, flat)
            .chain(self.picture_palette.iter().map(|(_, word)| *word))
            .collect()
    }
}

/// Where the column of a property is, if the schema declares it.
fn position_of(columns: &[Column], key: &'static str) -> Option<usize> {
    let id = key_id(key);
    columns.iter().position(|column| column.id == id)
}

fn make_model(
    shape: &Shape,
    index: u32,
    (sight_through, light_through): (bool, bool),
    block: u32,
    transparency: Transparency,
    face_materials: Option<[u32; 6]>,
) -> Model {
    let hides = std::array::from_fn(|face| {
        if sight_through {
            0
        } else {
            FaceDirection::from_index(face as u8).map_or(0, |face| shape.face_cover(face))
        }
    });
    Model {
        shape: index,
        block,
        sight_through,
        light_through,
        transparency,
        hides,
        opaque_cube: shape.is_full() && !sight_through,
        face_materials,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::definition::color_from_bytes;
    use crate::property::PropertyKind;

    const DOWN: FaceDirection = FaceDirection::NegativeY;
    const UP: FaceDirection = FaceDirection::PositiveY;

    fn table() -> BlockTable {
        let air = BlockDefinition::air();
        let stone = BlockDefinition::cube([0.5, 0.5, 0.5]).with_shapes(vec![
            Shape::full(),
            Shape::half(DOWN),
            Shape::half(UP),
        ]);
        let glass = BlockDefinition::cube([0.7, 0.9, 1.0])
            .with_sight_through(true)
            .with_light_through(true);
        BlockTable::from_definitions(&[Some(&air), Some(&stone), Some(&glass), None])
    }

    #[test]
    fn every_block_and_state_has_a_model_with_its_shape() {
        let table = table();
        assert_eq!(table.block_count(), 4);
        assert_eq!(
            (
                table.state_count(0),
                table.state_count(1),
                table.state_count(2)
            ),
            (1, 3, 1)
        );
        let air = table.model_of(0, 0, 0);
        assert!(!table.is_solid(air) && table.is_sight_through(air));
        let cube = table.model_of(1, 0, 0);
        assert!(table.is_solid(cube) && table.is_opaque_cube(cube));
        let bottom_slab = table.model_of(1, 1, 0);
        assert!(table.is_solid(bottom_slab) && !table.is_opaque_cube(bottom_slab));
        assert_eq!(table.shape(bottom_slab), &Shape::half(DOWN));
        assert_ne!(table.model_of(1, 1, 0), table.model_of(1, 2, 0));
    }

    #[test]
    fn what_hides_what_is_behind_it_follows_the_shape_and_sight_through_blocks_hide_nothing() {
        let table = table();
        let cube = table.model_of(1, 0, 0);
        assert!(
            FaceDirection::ALL
                .iter()
                .all(|&face| table.hides_fully(cube, face))
        );
        let slab = table.model_of(1, 1, 0);
        assert!(table.hides_fully(slab, DOWN));
        assert!(!table.hides_fully(slab, UP));
        assert_eq!(
            table.hides(slab, FaceDirection::PositiveX),
            0x0F0F_0F0F_0F0F_0F0F
        );
        let glass = table.model_of(2, 0, 0);
        assert!(table.shape(glass).is_full(), "glass fills its cell");
        assert!(!table.is_opaque_cube(glass));
        assert!(
            FaceDirection::ALL
                .iter()
                .all(|&face| table.hides(glass, face) == 0)
        );
    }

    #[test]
    fn light_passes_through_sight_through_blocks_and_the_empty_parts_of_shapes() {
        let table = table();
        assert_eq!(
            table.connections(table.model_of(0, 0, 0)),
            FaceConnections::all()
        );
        assert_eq!(
            table.connections(table.model_of(2, 0, 0)),
            FaceConnections::all()
        );
        assert_eq!(
            table.connections(table.model_of(1, 0, 0)),
            FaceConnections::NONE
        );
        let slab = table.connections(table.model_of(1, 1, 0));
        assert!(slab.connects(UP, FaceDirection::PositiveZ) && !slab.is_open(DOWN));
    }

    #[test]
    fn a_state_the_block_lacks_is_its_first_and_an_unknown_block_is_an_opaque_magenta_cube() {
        let table = table();
        assert_eq!(table.model_of(1, 3, 0), table.model_of(1, 0, 0));
        assert_eq!(table.model_of(1, u32::MAX, 0), table.model_of(1, 0, 0));
        for block in [3, 4, 9999] {
            let model = table.model_of(block, 0, 0);
            assert!(table.is_opaque_cube(model), "block {block}");
            assert_eq!(table.color(block), MISSING_BLOCK_COLOR);
            assert_eq!(table.state_count(block), 1);
            assert_eq!(table.model_of(block, 7, 0), model);
        }
    }

    #[test]
    fn colours_are_per_block() {
        let table = table();
        assert_eq!(table.color(1), [0.5, 0.5, 0.5]);
        assert_eq!(
            table.palette(),
            [[0.0; 3], [0.5; 3], [0.7, 0.9, 1.0], MISSING_BLOCK_COLOR]
        );
    }

    #[test]
    fn a_block_with_state_colours_is_drawn_by_its_state_and_other_blocks_by_their_id() {
        let air = BlockDefinition::air();
        let numbers =
            BlockDefinition::cube([0.5; 3]).with_state_colors(vec![[0.1; 3], [0.2; 3], [0.3; 3]]);
        let stone = BlockDefinition::cube([0.4; 3]);
        let more = BlockDefinition::cube([0.6; 3]).with_state_colors(vec![[0.9; 3], [0.8; 3]]);
        let table =
            BlockTable::from_definitions(&[Some(&air), Some(&numbers), Some(&stone), Some(&more)]);
        // The palette is the four blocks, then each block's state colours in block order.
        assert_eq!(
            table.palette(),
            [
                [0.0; 3], [0.5; 3], [0.4; 3], [0.6; 3], [0.1; 3], [0.2; 3], [0.3; 3], [0.9; 3],
                [0.8; 3],
            ]
        );
        let materials: Vec<u32> = (0..5).map(|state| table.material_of(1, state)).collect();
        assert_eq!(
            materials,
            [4, 5, 6, 4, 5],
            "the colours repeat, however large the state"
        );
        // 2^32 - 1 is a multiple of 3, and one less than it leaves 2.
        assert_eq!(table.material_of(1, u32::MAX), 4);
        assert_eq!(table.material_of(1, u32::MAX - 1), 6);
        assert_eq!(table.palette()[table.material_of(1, 1) as usize], [0.2; 3]);
        assert_eq!(table.material_of(3, 1), 8);
        assert_eq!(table.palette()[table.material_of(3, 2) as usize], [0.9; 3]);
        // Blocks without state colours are drawn as their own id, in every state.
        assert_eq!(table.material_of(2, 0), 2);
        assert_eq!(table.material_of(2, 77), 2);
        assert_eq!(table.material_of(0, 0), 0);
        // Colours are not shapes: every state is still the whole cube.
        assert_eq!(table.state_count(1), 1);
        assert_eq!(table.model_of(1, 1_000_000, 0), table.model_of(1, 0, 0));
    }

    #[test]
    fn the_faces_a_block_shows_are_what_its_mode_says_and_only_a_sight_through_block_of_the_same_kind_is_changed_by_it()
     {
        let sight_through = || BlockDefinition::cube([0.7; 3]).with_sight_through(true);
        let leaves = sight_through().with_transparency(Transparency::Leaves);
        let glass = sight_through().with_transparency(Transparency::Glass);
        let plain_glass = sight_through();
        let stone = BlockDefinition::cube([0.5; 3]);
        let table = BlockTable::from_definitions(&[
            Some(&leaves),
            Some(&glass),
            Some(&plain_glass),
            Some(&stone),
            None,
        ]);
        let east = FaceDirection::PositiveX;
        let model = |block| table.model_of(block, 0, 0);
        let shown: Vec<(Transparency, bool, bool)> = (0..5)
            .map(|block| {
                (
                    table.transparency(model(block)),
                    table.shows_inner_faces(model(block)),
                    table.shows_back_faces(model(block)),
                )
            })
            .collect();
        assert_eq!(
            shown,
            [
                (Transparency::Leaves, true, true),
                (Transparency::Glass, false, true),
                (Transparency::Opaque, false, false),
                (Transparency::Opaque, false, false),
                (Transparency::Opaque, false, false),
            ]
        );
        // Leaves beside leaves hide nothing of each other; glass beside glass still does (its back faces do not change that), and so does a block with no mode.
        assert_eq!(table.hidden_by_neighbour(model(0), east, model(0)), 0);
        assert_eq!(
            table.hidden_by_neighbour(model(1), east, model(1)),
            u64::MAX
        );
        assert_eq!(
            table.hidden_by_neighbour(model(2), east, model(2)),
            u64::MAX
        );
        // Showing inner faces does not make an opaque neighbour sight-through, and does not let the leaves be seen where stone hides them.
        assert_eq!(
            table.hidden_by_neighbour(model(0), east, model(3)),
            u64::MAX
        );
        assert_eq!(table.hidden_by_neighbour(model(3), east, model(0)), 0);
        // The mode is the same in every orientation.
        for orientation in 0..24 {
            assert_eq!(
                table.transparency(table.model_of(0, 0, orientation)),
                Transparency::Leaves
            );
        }
    }

    #[test]
    fn light_passing_is_its_own_flag_and_does_not_follow_sight() {
        let air = BlockDefinition::air();
        let stone = BlockDefinition::cube([0.5; 3]);
        let tinted_glass = BlockDefinition::cube([0.5; 3]).with_sight_through(true);
        let dark_window = BlockDefinition::cube([0.5; 3]).with_light_through(true);
        let clear = BlockDefinition::cube([0.5; 3])
            .with_sight_through(true)
            .with_light_through(true);
        let table = BlockTable::from_definitions(&[
            Some(&air),
            Some(&stone),
            Some(&tinted_glass),
            Some(&dark_window),
            Some(&clear),
        ]);
        let model = |block| table.model_of(block, 0, 0);
        let flags: Vec<(bool, bool)> = (0..5)
            .map(|block| {
                (
                    table.is_sight_through(model(block)),
                    table.is_light_through(model(block)),
                )
            })
            .collect();
        assert_eq!(
            flags,
            [
                (true, true),
                (false, false),
                (true, false),
                (false, true),
                (true, true)
            ]
        );
        // Light passes between all the faces of a light-through block and none of a whole cube that is not.
        let passes: Vec<bool> = (0..5)
            .map(|block| table.connections(model(block)) == FaceConnections::all())
            .collect();
        assert_eq!(passes, [true, false, false, true, true]);
        // Sight decides what hides what, light does not.
        let east = FaceDirection::PositiveX;
        assert_eq!(
            table.hidden_by_neighbour(model(1), east, model(3)),
            u64::MAX
        );
        assert_eq!(table.hidden_by_neighbour(model(1), east, model(2)), 0);
    }

    #[test]
    fn an_unknown_block_is_drawn_as_its_own_id() {
        let table = table();
        for block in [3, 4, 9999] {
            assert_eq!(table.material_of(block, 5), block);
        }
    }

    #[test]
    fn a_neighbour_hides_what_it_covers_and_sight_through_neighbours_only_hide_their_own_block() {
        let air = BlockDefinition::air();
        let stone =
            BlockDefinition::cube([0.5; 3]).with_shapes(vec![Shape::full(), Shape::half(DOWN)]);
        let glass = BlockDefinition::cube([0.7; 3])
            .with_sight_through(true)
            .with_shapes(vec![Shape::full(), Shape::half(DOWN)]);
        let other_glass = BlockDefinition::cube([0.9; 3]).with_sight_through(true);
        let table = BlockTable::from_definitions(&[
            Some(&air),
            Some(&stone),
            Some(&glass),
            Some(&other_glass),
        ]);
        let (air, cube, slab) = (
            table.model_of(0, 0, 0),
            table.model_of(1, 0, 0),
            table.model_of(1, 1, 0),
        );
        let (glass_cube, glass_slab) = (table.model_of(2, 0, 0), table.model_of(2, 1, 0));
        let east = FaceDirection::PositiveX;
        // Air hides nothing; a cube hides everything; a slab hides the half it covers.
        assert_eq!(table.hidden_by_neighbour(cube, east, air), 0);
        assert_eq!(table.hidden_by_neighbour(cube, east, cube), u64::MAX);
        assert_eq!(
            table.hidden_by_neighbour(cube, east, slab),
            0x0F0F_0F0F_0F0F_0F0F
        );
        // Glass never hides stone; stone hides glass.
        assert_eq!(table.hidden_by_neighbour(cube, east, glass_cube), 0);
        assert_eq!(table.hidden_by_neighbour(glass_cube, east, cube), u64::MAX);
        // Glass hides glass of the same block by what its shape covers, and other glass not at all.
        assert_eq!(
            table.hidden_by_neighbour(glass_cube, east, glass_cube),
            u64::MAX
        );
        assert_eq!(
            table.hidden_by_neighbour(glass_cube, east, glass_slab),
            0x0F0F_0F0F_0F0F_0F0F
        );
        // It is the neighbour's face towards the viewer that counts: above, a bottom slab shows its
        // whole underside to the glass cube; below, it shows only the half-height top.
        assert_eq!(
            table.hidden_by_neighbour(glass_cube, FaceDirection::PositiveY, glass_slab),
            u64::MAX
        );
        assert_eq!(
            table.hidden_by_neighbour(glass_cube, FaceDirection::NegativeY, glass_slab),
            0
        );
        assert_eq!(
            table.hidden_by_neighbour(glass_cube, east, table.model_of(3, 0, 0)),
            0
        );
    }

    #[test]
    fn of_cubes_makes_whole_opaque_cubes_and_air() {
        let table = BlockTable::of_cubes(&[false, true, true]);
        assert!(!table.is_solid(table.model_of(0, 0, 0)));
        assert!(table.is_opaque_cube(table.model_of(1, 0, 0)));
        assert!(table.is_opaque_cube(table.model_of(2, 0, 0)));
        assert_eq!(table.block_count(), 3);
    }

    fn yellow() -> [f32; 3] {
        color_from_bytes([255, 216, 0])
    }

    /// The number of the turn that carries the face `from` to the face `to` (the first, if there are several).
    fn orientation_carrying(from: FaceDirection, to: FaceDirection) -> u32 {
        LatticeRotation::all()
            .into_iter()
            .find(|rotation| rotation.apply_face(from) == to)
            .unwrap()
            .index()
            .into()
    }

    /// A block of one state: a whole cube whose front, docking face and marked face are the east, as it is made.
    fn port() -> BlockDefinition {
        BlockDefinition::cube([0.3, 0.3, 0.32])
            .with_face_colors(&[(FaceDirection::PositiveX, yellow())])
            .with_docking_faces(&[Some(FaceDirection::PositiveX)])
            .with_front_face(FaceDirection::PositiveX)
    }

    #[test]
    fn a_voxel_has_an_orientation_whatever_its_block_and_the_shape_in_it_is_the_shape_turned() {
        let stone = BlockDefinition::cube([0.5; 3]).with_shapes(vec![
            Shape::full(),
            Shape::half(DOWN),
            Shape::stair(FaceDirection::PositiveX),
        ]);
        let table = BlockTable::from_definitions(&[Some(&stone)]);
        // Orientation 0 is the shape as it is made; another is the shape turned by the turn with that number, for every state and every number.
        for state in 0..3 {
            let made = table.shape(table.model_of(0, state, 0)).clone();
            for (orientation, rotation) in LatticeRotation::all().into_iter().enumerate() {
                let model = table.model_of(0, state, orientation as u32);
                assert!(
                    table.shape(model).same_cells(&made.turned(rotation)),
                    "state {state} orientation {orientation}"
                );
            }
        }
        // A slab turned over is a top slab; a whole cube is a whole cube however it is turned and is as cheap.
        let over = orientation_carrying(DOWN, UP);
        assert!(
            table
                .shape(table.model_of(0, 1, over))
                .same_cells(&Shape::half(UP))
        );
        for orientation in 0..24 {
            assert!(table.is_opaque_cube(table.model_of(0, 0, orientation)));
        }
        // A number that is not an orientation is none (a world is read as it is found).
        assert_eq!(table.model_of(0, 1, 24), table.model_of(0, 1, 0));
        assert_eq!(table.model_of(0, 1, u32::MAX), table.model_of(0, 1, 0));
        // Unknown blocks are an opaque cube in every orientation, and so is a block with no definition.
        assert_eq!(table.model_of(9, 0, 5), table.model_of(9, 0, 0));
        assert!(table.is_opaque_cube(table.model_of(9, 3, 17)));
    }

    #[test]
    fn what_a_property_says_does_not_depend_on_the_orientation_and_the_faces_it_names_turn_with_it()
    {
        let table = BlockTable::from_definitions(&[Some(&port())]);
        let key = engine::DENSITY;
        let handle = table.handle(&key).unwrap();
        for orientation in 0..24 {
            assert_eq!(
                table.get(handle, table.model_of(0, 0, orientation)),
                table.get(handle, table.model_of(0, 0, 0))
            );
        }
        for (orientation, rotation) in LatticeRotation::all().into_iter().enumerate() {
            let turned_face = rotation.apply_face(FaceDirection::PositiveX);
            let turned = Some(turned_face);
            let o = orientation as u32;
            assert_eq!(table.docking_face(0, 0, o), turned, "{orientation}");
            assert_eq!(table.front_face(0, 0, o), turned);
            let materials = table
                .face_materials(0, 0, o)
                .expect("the port colours a face");
            let yellow_faces: Vec<FaceDirection> = FaceDirection::ALL
                .into_iter()
                .filter(|face| materials[usize::from(face.index())] != 0)
                .collect();
            assert_eq!(yellow_faces, [turned_face], "{orientation}");
        }
        // Every face a port can face is an orientation, and only the roll about it differs between the four that share it.
        for face in FaceDirection::ALL {
            let o = orientation_carrying(FaceDirection::PositiveX, face);
            assert_eq!(table.docking_face(0, 0, o), Some(face));
        }
        // A block that has none says none, and a block that is unknown too.
        let stone = BlockDefinition::cube([0.5; 3]);
        let table = BlockTable::from_definitions(&[Some(&stone)]);
        assert_eq!(table.docking_face(0, 0, 3), None);
        assert_eq!(table.docking_face(5, 0, 3), None);
    }

    #[test]
    fn a_coloured_face_is_drawn_in_its_own_colour_and_the_other_faces_in_the_blocks_wherever_the_voxel_faces()
     {
        let stone = BlockDefinition::cube([0.5; 3]);
        let tinted = BlockDefinition::cube([0.6; 3]).with_state_colors(vec![[0.9; 3], [0.8; 3]]);
        let port = port();
        let table = BlockTable::from_definitions(&[Some(&stone), Some(&tinted), Some(&port)]);
        // The palette is the three blocks, the two state colours of the tinted one, and the yellow once.
        let palette = table.palette();
        assert_eq!(palette.len(), 6);
        assert_eq!(palette[5], yellow());
        for face in FaceDirection::ALL {
            let o = orientation_carrying(FaceDirection::PositiveX, face);
            for other in FaceDirection::ALL {
                let wanted = if other == face { 5 } else { 2 };
                assert_eq!(
                    table.material_of_face(2, 0, o, other),
                    wanted,
                    "marked {face:?}, looking at {other:?}"
                );
            }
        }
        // A block that marks nothing is drawn as it always was, whatever the face, and so is an unknown block.
        for face in FaceDirection::ALL {
            assert_eq!(table.material_of_face(0, 0, 7, face), 0);
            assert_eq!(
                table.material_of_face(1, 1, 7, face),
                table.material_of(1, 1)
            );
            assert_eq!(table.material_of_face(9999, 0, 7, face), 9999);
        }
        assert_eq!(table.face_materials(0, 0, 0), None);
        assert_eq!(table.color(2), [0.3, 0.3, 0.32]);
    }

    #[test]
    fn a_log_colours_both_its_ends_and_a_piston_only_its_front() {
        let ends = color_from_bytes([200, 180, 120]);
        let log = BlockDefinition::cube([0.4; 3])
            .with_face_colors(&[
                (FaceDirection::PositiveY, ends),
                (FaceDirection::NegativeY, ends),
            ])
            .with_axis_face(FaceDirection::PositiveY);
        let table = BlockTable::from_definitions(&[Some(&log), Some(&port())]);
        let sideways = orientation_carrying(FaceDirection::PositiveY, FaceDirection::PositiveX);
        let marked: Vec<FaceDirection> = FaceDirection::ALL
            .into_iter()
            .filter(|face| table.material_of_face(0, 0, sideways, *face) != 0)
            .collect();
        assert_eq!(marked, [FaceDirection::NegativeX, FaceDirection::PositiveX]);
        let piston: Vec<FaceDirection> = FaceDirection::ALL
            .into_iter()
            .filter(|face| table.material_of_face(1, 0, 0, *face) != 1)
            .collect();
        assert_eq!(piston, [FaceDirection::PositiveX]);
    }

    #[test]
    fn faces_of_a_block_with_state_colours_that_are_not_coloured_are_the_colour_of_the_state() {
        // A block that has both: its own coloured face is a palette entry after the state colours, and its other faces follow the state.
        let block = BlockDefinition::cube([0.5; 3])
            .with_state_colors(vec![[0.1; 3], [0.2; 3]])
            .with_shapes(vec![Shape::full(), Shape::full(), Shape::full()])
            .with_face_colors(&[(FaceDirection::PositiveY, yellow())]);
        let table = BlockTable::from_definitions(&[Some(&block)]);
        // The palette is the block, the two state colours, then the yellow.
        assert_eq!(table.palette(), [[0.5; 3], [0.1; 3], [0.2; 3], yellow()]);
        for (state, voxel) in [(0, 1), (1, 2), (2, 1)] {
            for face in FaceDirection::ALL {
                let wanted = if face == FaceDirection::PositiveY {
                    3
                } else {
                    voxel
                };
                assert_eq!(table.material_of_face(0, state, 0, face), wanted);
            }
        }
    }

    #[test]
    fn a_colour_given_to_several_faces_or_blocks_is_one_palette_entry_in_the_order_of_blocks_then_faces()
     {
        let red = [1.0, 0.0, 0.0];
        let blue = [0.0, 0.0, 1.0];
        let first = BlockDefinition::cube([0.5; 3]).with_face_colors(&[
            (FaceDirection::PositiveZ, blue),
            (FaceDirection::NegativeX, red),
            (FaceDirection::PositiveX, blue),
        ]);
        let second =
            BlockDefinition::cube([0.6; 3]).with_face_colors(&[(FaceDirection::NegativeY, red)]);
        let table = BlockTable::from_definitions(&[Some(&first), Some(&second)]);
        // Two blocks, then red (the first block's lowest face), then blue.
        assert_eq!(table.palette(), [[0.5; 3], [0.6; 3], red, blue]);
        assert_eq!(table.face_materials(0, 0, 0), Some([2, 3, 0, 0, 0, 3]));
        assert_eq!(table.face_materials(1, 0, 0), Some([1, 1, 2, 1, 1, 1]));
    }

    #[test]
    fn coloured_faces_turn_with_the_voxel_for_every_orientation() {
        let colours = [
            [0.1, 0.0, 0.0],
            [0.2, 0.0, 0.0],
            [0.3, 0.0, 0.0],
            [0.4, 0.0, 0.0],
            [0.5, 0.0, 0.0],
            [0.6, 0.0, 0.0],
        ];
        let all: Vec<(FaceDirection, [f32; 3])> =
            FaceDirection::ALL.into_iter().zip(colours).collect();
        let block = BlockDefinition::cube([0.9; 3]).with_face_colors(&all);
        let table = BlockTable::from_definitions(&[Some(&block)]);
        for (orientation, rotation) in LatticeRotation::all().into_iter().enumerate() {
            for (made, colour) in all.iter().copied() {
                let turned = rotation.apply_face(made);
                let material = table.material_of_face(0, 0, orientation as u32, turned);
                assert_eq!(
                    table.palette()[material as usize],
                    colour,
                    "orientation {orientation}, made {made:?}"
                );
            }
        }
    }

    #[test]
    fn a_face_property_is_read_through_a_handle_as_a_number_and_asking_for_it_as_another_kind_is_refused()
     {
        let table = BlockTable::from_definitions(&[Some(&port())]);
        let handle = table.handle(&engine::FRONT_FACE).unwrap();
        assert_eq!(
            table.get_of(handle, 0, 0),
            2,
            "the east, as the code of a face"
        );
        let as_a_plain_number: PropertyKey<u8> =
            PropertyKey::new("engine:front_face", 0, "asked for as a plain number");
        assert!(matches!(
            table.handle(&as_a_plain_number),
            Err(PropertyError::WrongKind {
                declared: PropertyKind::Face,
                ..
            })
        ));
    }

    // ----- Pictures -----

    fn place(class: usize, layer: usize) -> TileRef {
        TileRef::new(class, layer).expect("a place for a picture")
    }

    /// A block with pictures: the top is tile (0, 1), the sides (1, 0) and the bottom (0, 0).
    fn pictured() -> BlockDefinition {
        BlockDefinition::cube([0.5; 3])
            .with_texture("test:stone".parse().unwrap())
            .with_pictures(FacePictures {
                top: place(0, 1),
                side: place(1, 0),
                bottom: place(0, 0),
            })
    }

    fn picture_on(
        table: &BlockTable,
        block: u32,
        orientation: u32,
        face: FaceDirection,
    ) -> FacePicture {
        let material = table.material_of_face(block, 0, orientation, face);
        FacePicture::from_word(table.palette_pictures()[material as usize])
            .unwrap_or_else(|| panic!("no picture on {face:?}"))
    }

    #[test]
    fn a_face_word_has_its_documented_bits_and_only_a_word_of_that_kind_is_a_picture() {
        let picture = FacePicture {
            tile: place(2, 200),
            across: FaceDirection::NegativeZ,
            up: FaceDirection::PositiveY,
            cut_out: false,
        };
        // 1 | class 2 << 1 | layer 200 << 3 | across 4 << 11 | up 3 << 14.
        assert_eq!(picture.word(), 1 + 4 + 1_600 + 8_192 + 49_152);
        assert_eq!(FacePicture::from_word(picture.word()), Some(picture));
        // Holes are bit 17, and nothing else moves.
        let with_holes = FacePicture {
            cut_out: true,
            ..picture
        };
        assert_eq!(with_holes.word(), picture.word() + (1 << 17));
        assert_eq!(FacePicture::from_word(with_holes.word()), Some(with_holes));
        for word in [0, 2, 1 << 18 | 1, 7 << 11 | 1, 6 << 14 | 1] {
            assert_eq!(FacePicture::from_word(word), None, "{word:#x}");
        }
        // Every place and every pair of directions comes back.
        for class in 0..4 {
            for layer in [0, 1, 128, 255] {
                for across in FaceDirection::ALL {
                    let picture = FacePicture {
                        tile: place(class, layer),
                        across,
                        up: FaceDirection::PositiveY,
                        cut_out: layer == 128,
                    };
                    assert_eq!(FacePicture::from_word(picture.word()), Some(picture));
                    assert!(picture.word() < 1 << 18);
                }
            }
        }
    }

    #[test]
    fn a_block_with_pictures_shows_its_top_its_sides_and_its_bottom_the_way_up_as_made() {
        use FaceDirection::{NegativeX, NegativeY, NegativeZ, PositiveX, PositiveY, PositiveZ};
        let table = BlockTable::from_definitions(&[Some(&pictured())]);
        // (face, tile, across, up): the table of the spec, written out.
        let wanted = [
            (PositiveX, place(1, 0), NegativeZ, PositiveY),
            (NegativeX, place(1, 0), PositiveZ, PositiveY),
            (PositiveY, place(0, 1), PositiveX, NegativeZ),
            (NegativeY, place(0, 0), NegativeX, NegativeZ),
            (PositiveZ, place(1, 0), PositiveX, PositiveY),
            (NegativeZ, place(1, 0), NegativeX, PositiveY),
        ];
        for (face, tile, across, up) in wanted {
            assert_eq!(
                picture_on(&table, 0, 0, face),
                FacePicture {
                    tile,
                    across,
                    up,
                    cut_out: false
                },
                "{face:?}"
            );
        }
    }

    /// The direction that is `first` crossed with `second`, for two axes that are not the same.
    fn cross(first: FaceDirection, second: FaceDirection) -> FaceDirection {
        let vector = |face: FaceDirection| {
            let mut vector = [0i32; 3];
            vector[face.axis()] = if face.is_positive() { 1 } else { -1 };
            vector
        };
        let (a, b) = (vector(first), vector(second));
        let crossed = [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ];
        FaceDirection::ALL
            .into_iter()
            .find(|face| vector(*face) == crossed)
            .expect("two axes cross to an axis")
    }

    #[test]
    fn however_a_block_is_turned_its_pictures_are_not_mirrored_the_top_is_where_up_went_and_the_sides_stand_on_it()
     {
        let table = BlockTable::from_definitions(&[Some(&pictured())]);
        for (orientation, rotation) in LatticeRotation::all().into_iter().enumerate() {
            let orientation = orientation as u32;
            let top_went_to = rotation.apply_face(FaceDirection::PositiveY);
            for face in FaceDirection::ALL {
                let picture = picture_on(&table, 0, orientation, face);
                // Right-handed: across, then up, then out: the picture is seen as it is drawn, never mirrored.
                assert_eq!(
                    cross(picture.across, picture.up),
                    face,
                    "orientation {orientation}, {face:?}"
                );
                // The picture of the top is on the face the top went to, the bottom's on the opposite, the sides' on the other four.
                let wanted = if face == top_went_to {
                    place(0, 1)
                } else if face == top_went_to.opposite() {
                    place(0, 0)
                } else {
                    place(1, 0)
                };
                assert_eq!(picture.tile, wanted, "orientation {orientation}, {face:?}");
                // A side's picture stands on the block's own bottom: its up is where the block's up went (so a log on its side has its grain along it).
                if face != top_went_to && face != top_went_to.opposite() {
                    assert_eq!(
                        picture.up, top_went_to,
                        "orientation {orientation}, {face:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_cube_of_one_picture_has_one_palette_entry_for_each_of_the_24_ways_a_picture_sits_on_a_face()
     {
        let one = BlockDefinition::cube([0.5; 3])
            .with_texture("test:stone".parse().unwrap())
            .with_pictures(FacePictures {
                top: place(0, 0),
                side: place(0, 0),
                bottom: place(0, 0),
            });
        let table = BlockTable::from_definitions(&[Some(&one)]);
        let pictures = table.palette_pictures();
        let with_picture = pictures.iter().filter(|word| **word != 0).count();
        assert_eq!(with_picture, 24, "6 directions across, 4 up from each");
        // A palette entry is a colour and a picture, and the two lists are the same length; the first entries (the block) have no picture.
        assert_eq!(table.palette().len(), pictures.len());
        assert_eq!(pictures[0], 0);
        // Every orientation of every face is one of the 24, and the same colour all through.
        for orientation in 0..24 {
            for face in FaceDirection::ALL {
                let material = table.material_of_face(0, 0, orientation, face);
                assert!(pictures[material as usize] != 0);
                assert_eq!(table.palette()[material as usize], [0.5; 3]);
            }
        }
    }

    #[test]
    fn a_face_with_a_picture_has_the_colour_it_would_have_flat_and_the_same_colour_and_picture_are_one_entry()
     {
        let top_gold = [1.0, 0.8, 0.0];
        let gilded = pictured().with_face_colors(&[(FaceDirection::PositiveY, top_gold)]);
        let tinted = pictured()
            .with_state_colors(vec![[0.1; 3], [0.2; 3]])
            .with_shapes(vec![Shape::full(), Shape::full()]);
        let table =
            BlockTable::from_definitions(&[Some(&gilded), Some(&tinted), Some(&pictured())]);
        let palette = table.palette();
        let colour =
            |block, state, face| palette[table.material_of_face(block, state, 0, face) as usize];
        // The top is gold and the sides the block's own colour; each has its picture.
        assert_eq!(colour(0, 0, FaceDirection::PositiveY), top_gold);
        assert_eq!(colour(0, 0, FaceDirection::PositiveX), [0.5; 3]);
        // A block with state colours shows the colour of the state under its pictures.
        assert_eq!(colour(1, 0, FaceDirection::PositiveX), [0.1; 3]);
        assert_eq!(colour(1, 1, FaceDirection::PositiveX), [0.2; 3]);
        // The same colour with the same picture is one entry, from block to block.
        assert_eq!(
            table.material_of_face(0, 0, 0, FaceDirection::PositiveX),
            table.material_of_face(2, 0, 0, FaceDirection::PositiveX)
        );
        assert_ne!(
            table.material_of_face(0, 0, 0, FaceDirection::PositiveY),
            table.material_of_face(2, 0, 0, FaceDirection::PositiveY),
            "gold is not the block's colour"
        );
    }

    #[test]
    fn blocks_without_pictures_have_no_picture_entries_and_are_drawn_as_before() {
        let plain = BlockDefinition::cube([0.5; 3]);
        let table = BlockTable::from_definitions(&[Some(&plain)]);
        assert!(table.palette_pictures().iter().all(|word| *word == 0));
        assert_eq!(table.palette().len(), table.palette_pictures().len());
        assert_eq!(table.face_materials(0, 0, 3), None);
        for face in FaceDirection::ALL {
            assert_eq!(table.material_of_face(0, 0, 5, face), 0);
        }
    }

    #[test]
    fn how_a_block_is_placed_is_read_from_its_definition() {
        let plain = BlockDefinition::cube([0.5; 3]);
        let log = BlockDefinition::cube([0.4; 3]).with_axis_face(FaceDirection::PositiveY);
        let table = BlockTable::from_definitions(&[Some(&plain), Some(&log), Some(&port())]);
        assert_eq!(table.placement(0, 0), Placement::Fixed);
        assert_eq!(
            table.placement(1, 0),
            Placement::AlongPlacedFace(FaceDirection::PositiveY)
        );
        assert_eq!(
            table.placement(2, 0),
            Placement::FrontToPlayer(FaceDirection::PositiveX)
        );
        assert_eq!(table.placement(9, 0), Placement::Fixed, "an unknown block");
        // A block with both has its line along the face it is put against and its front to the player's side.
        let both = BlockDefinition::cube([0.5; 3])
            .with_front_face(FaceDirection::PositiveX)
            .with_axis_face(FaceDirection::PositiveY);
        let table = BlockTable::from_definitions(&[Some(&both)]);
        assert_eq!(
            table.placement(0, 0),
            Placement::AlongPlacedFaceFacingPlayer {
                axis: FaceDirection::PositiveY,
                front: FaceDirection::PositiveX
            }
        );
    }

    #[test]
    fn shapes_turned_to_the_same_are_one_shape_and_the_table_does_not_grow_with_them() {
        // A whole cube in 24 orientations is one shape: the pool holds the shape once.
        let cubes = BlockTable::of_cubes(&[true, true, true, true]);
        assert_eq!(cubes.shapes.len(), 1);
        // A slab has six distinct standings, and a stair twelve (counted by the cells they fill).
        let distinct = |table: &BlockTable| {
            table
                .shapes
                .iter()
                .map(Shape::cell_key)
                .collect::<std::collections::HashSet<_>>()
                .len()
        };
        let slab = BlockTable::from_definitions(&[Some(
            &BlockDefinition::cube([0.5; 3]).with_shapes(vec![Shape::half(DOWN)]),
        )]);
        assert_eq!(distinct(&slab), 6 + 1, "the six slabs and the unknown cube");
        let stair = BlockTable::from_definitions(&[Some(
            &BlockDefinition::cube([0.5; 3])
                .with_shapes(vec![Shape::stair(FaceDirection::PositiveX)]),
        )]);
        assert_eq!(
            distinct(&stair),
            12 + 1,
            "the twelve stairs and the unknown cube"
        );
    }
}
