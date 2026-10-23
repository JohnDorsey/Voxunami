use std::collections::HashMap;
use std::marker::PhantomData;

use voxel_space::{FaceDirection, LatticeRotation};

use crate::definition::BlockDefinition;
use crate::property::engine::{self, face_from_code};
use crate::property::{
    Column, PropertyError, PropertyHandle, PropertyKey, PropertySchema, PropertyType, check_value,
    key_id,
};
use crate::shape::{FaceConnections, Shape, ShapeBox};

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
}

/// The face of a voxel that is drawn in the mark colour, in the orientation the voxel has, and (for a log) the face opposite it too.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaceMark {
    pub face: FaceDirection,
    pub both_ends: bool,
    /// The renderer's palette entry of the mark colour.
    pub material: u32,
}

impl FaceMark {
    /// Whether this face is drawn in the mark colour.
    pub fn marks(&self, face: FaceDirection) -> bool {
        face == self.face || (self.both_ends && face == self.face.opposite())
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

#[derive(Clone, Debug)]
struct Model {
    /// The shape, in the pool of the table.
    shape: u32,
    block: u32,
    see_through: bool,
    /// The sub-faces on the voxel's boundary that hide what is behind them: the shape's, unless the
    /// block is see-through.
    hides: [u64; 6],
    opaque_cube: bool,
    mark: Option<FaceMark>,
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
    /// The colours after the blocks' own: every block's state colours, in block order, and then the mark colours.
    state_palette: Vec<[f32; 3]>,
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
            see_through: bool,
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
            for shape in definition.shapes() {
                bases.push(Base {
                    shape,
                    block: block as u32,
                    see_through: definition.see_through,
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
            see_through: false,
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
        // The marked faces: each colour a block marks a face in takes one palette entry after all the state colours.
        let value_of = |columns: &[Column], column: Option<usize>, base: usize| {
            column.map_or(0, |column| columns[column].values[base])
        };
        let marked_face_column = position_of(&columns, engine::MARKED_FACE.id());
        let mark_color_column = position_of(&columns, engine::MARK_COLOR.id());
        let mark_both_ends_column = position_of(&columns, engine::MARK_BOTH_ENDS.id());
        let first_mark_material = (definitions.len() + state_palette.len()) as u32;
        let mut mark_colors: Vec<u32> = Vec::new();
        let mut base_marks: Vec<Option<FaceMark>> = vec![None; bases.len()];
        if marked_face_column.is_some() && mark_color_column.is_some() {
            for entry in entries
                .iter()
                .filter(|entry| entry.first_base != unknown_base as u32)
            {
                for state in 0..entry.state_count {
                    let base = (entry.first_base + state) as usize;
                    let Some(face) = u8::try_from(value_of(&columns, marked_face_column, base))
                        .ok()
                        .and_then(face_from_code)
                    else {
                        continue;
                    };
                    let color = value_of(&columns, mark_color_column, base) as u32;
                    let position = mark_colors
                        .iter()
                        .position(|known| *known == color)
                        .unwrap_or_else(|| {
                            mark_colors.push(color);
                            mark_colors.len() - 1
                        });
                    base_marks[base] = Some(FaceMark {
                        face,
                        both_ends: value_of(&columns, mark_both_ends_column, base) != 0,
                        material: first_mark_material + position as u32,
                    });
                }
            }
        }
        state_palette.extend(mark_colors.into_iter().map(|packed| {
            [16, 8, 0].map(|shift| f32::from((packed >> shift & 0xFF) as u8) / 255.0)
        }));
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
        for (base, details) in bases.iter().enumerate() {
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
                let mark = base_marks[base].map(|mark| FaceMark {
                    face: rotation.apply_face(mark.face),
                    ..mark
                });
                models.push(make_model(
                    &shapes[shape as usize],
                    shape,
                    details.see_through,
                    details.block,
                    mark,
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
        if column.kind != T::KIND {
            return Err(PropertyError::WrongKind {
                key: id,
                declared: column.kind,
                asked: T::KIND,
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
        match self.entries.get(block as usize) {
            Some(entry) if entry.state_color_count > 0 => {
                entry.first_state_material + state % entry.state_color_count
            }
            _ => block,
        }
    }

    /// What a face of a voxel is drawn as: the material of the voxel (see [`BlockTable::material_of`]), unless the face is one the block marks, which
    /// is drawn in the mark colour.
    pub fn material_of_face(
        &self,
        block: u32,
        state: u32,
        orientation: u32,
        face: FaceDirection,
    ) -> u32 {
        match self.face_mark(block, state, orientation) {
            Some(mark) if mark.marks(face) => mark.material,
            _ => self.material_of(block, state),
        }
    }

    /// The face a block marks in a state, standing in an orientation, if the block marks one.
    pub fn face_mark(&self, block: u32, state: u32, orientation: u32) -> Option<FaceMark> {
        self.models[self.model_of(block, state, orientation).0 as usize].mark
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

    /// How a block is turned when a player places it: by its front, by its axis, or not at all.
    pub fn placement(&self, block: u32, state: u32) -> Placement {
        if let Some(front) = self.front_face(block, state, 0) {
            Placement::FrontToPlayer(front)
        } else if let Some(axis) = self.axis_face(block, state, 0) {
            Placement::AlongPlacedFace(axis)
        } else {
            Placement::Fixed
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

    pub fn is_see_through(&self, model: ModelId) -> bool {
        self.models[model.0 as usize].see_through
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
        if !other.see_through {
            other.hides[usize::from(face.opposite().index())]
        } else if own.see_through && own.block == other.block {
            self.shape(neighbour).face_cover(face.opposite())
        } else {
            0
        }
    }

    /// The faces light and other fills can pass between through the model: all of them for a
    /// see-through block, else the ones its empty part joins.
    pub fn connections(&self, model: ModelId) -> FaceConnections {
        if self.models[model.0 as usize].see_through {
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
    see_through: bool,
    block: u32,
    mark: Option<FaceMark>,
) -> Model {
    let hides = std::array::from_fn(|face| {
        if see_through {
            0
        } else {
            FaceDirection::from_index(face as u8).map_or(0, |face| shape.face_cover(face))
        }
    });
    Model {
        shape: index,
        block,
        see_through,
        hides,
        opaque_cube: shape.is_full() && !see_through,
        mark,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOWN: FaceDirection = FaceDirection::NegativeY;
    const UP: FaceDirection = FaceDirection::PositiveY;

    fn table() -> BlockTable {
        let air = BlockDefinition::air();
        let stone = BlockDefinition::cube([0.5, 0.5, 0.5]).with_shapes(vec![
            Shape::full(),
            Shape::half(DOWN),
            Shape::half(UP),
        ]);
        let glass = BlockDefinition::cube([0.7, 0.9, 1.0]).with_see_through(true);
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
        assert!(!table.is_solid(air) && table.is_see_through(air));
        let cube = table.model_of(1, 0, 0);
        assert!(table.is_solid(cube) && table.is_opaque_cube(cube));
        let bottom_slab = table.model_of(1, 1, 0);
        assert!(table.is_solid(bottom_slab) && !table.is_opaque_cube(bottom_slab));
        assert_eq!(table.shape(bottom_slab), &Shape::half(DOWN));
        assert_ne!(table.model_of(1, 1, 0), table.model_of(1, 2, 0));
    }

    #[test]
    fn what_hides_what_is_behind_it_follows_the_shape_and_see_through_blocks_hide_nothing() {
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
    fn light_passes_through_see_through_blocks_and_the_empty_parts_of_shapes() {
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
    fn an_unknown_block_is_drawn_as_its_own_id() {
        let table = table();
        for block in [3, 4, 9999] {
            assert_eq!(table.material_of(block, 5), block);
        }
    }

    #[test]
    fn a_neighbour_hides_what_it_covers_and_see_through_neighbours_only_hide_their_own_block() {
        let air = BlockDefinition::air();
        let stone =
            BlockDefinition::cube([0.5; 3]).with_shapes(vec![Shape::full(), Shape::half(DOWN)]);
        let glass = BlockDefinition::cube([0.7; 3])
            .with_see_through(true)
            .with_shapes(vec![Shape::full(), Shape::half(DOWN)]);
        let other_glass = BlockDefinition::cube([0.9; 3]).with_see_through(true);
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

    const YELLOW: [u8; 3] = [255, 216, 0];

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
            .with_marked_faces([255, 216, 0], &[Some(FaceDirection::PositiveX)])
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
            let turned = Some(rotation.apply_face(FaceDirection::PositiveX));
            let o = orientation as u32;
            assert_eq!(table.docking_face(0, 0, o), turned, "{orientation}");
            assert_eq!(table.front_face(0, 0, o), turned);
            assert_eq!(table.face_mark(0, 0, o).map(|mark| mark.face), turned);
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
    fn a_marked_face_is_drawn_in_its_own_colour_and_the_other_faces_in_the_blocks_wherever_the_voxel_faces()
     {
        let stone = BlockDefinition::cube([0.5; 3]);
        let tinted = BlockDefinition::cube([0.6; 3]).with_state_colors(vec![[0.9; 3], [0.8; 3]]);
        let port = port();
        let table = BlockTable::from_definitions(&[Some(&stone), Some(&tinted), Some(&port)]);
        // The palette is the three blocks, the two state colours of the tinted one, and the yellow once.
        let palette = table.palette();
        assert_eq!(palette.len(), 6);
        assert_eq!(palette[5], [1.0, 216.0 / 255.0, 0.0]);
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
        assert_eq!(table.face_mark(0, 0, 0), None);
        assert_eq!(table.color(2), [0.3, 0.3, 0.32]);
        let _ = YELLOW;
    }

    #[test]
    fn a_log_marks_both_its_ends_and_a_piston_only_its_front() {
        let log = BlockDefinition::cube([0.4; 3])
            .with_marked_faces([200, 180, 120], &[Some(FaceDirection::PositiveY)])
            .with_both_ends_marked()
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
