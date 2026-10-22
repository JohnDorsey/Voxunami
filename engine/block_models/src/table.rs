use std::marker::PhantomData;

use voxel_space::{FaceDirection, LatticeRotation};

use crate::definition::BlockDefinition;
use crate::property::engine::{self, face_from_code};
use crate::property::{
    Column, PropertyError, PropertyHandle, PropertyKey, PropertySchema, PropertyType, check_value,
    key_id,
};
use crate::shape::{FaceConnections, Shape};

/// The colour of a block this program has no definition for (content from a plugin that is not
/// loaded): magenta, so that unknown content is seen rather than vanishing.
pub const MISSING_BLOCK_COLOR: [f32; 3] = [1.0, 0.0, 1.0];

/// One shape of one block, as the table holds it: what a voxel with a given block and state is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ModelId(u32);

impl ModelId {
    pub fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Debug)]
struct Entry {
    color: [f32; 3],
    first_model: u32,
    state_count: u32,
    /// Where this block's state colours start in the palette, and how many there are (0 if the
    /// block is one colour).
    first_state_material: u32,
    state_color_count: u32,
}

#[derive(Clone, Debug)]
struct Model {
    shape: Shape,
    block: u32,
    see_through: bool,
    /// The sub-faces on the voxel's boundary that hide what is behind them: the shape's, unless the
    /// block is see-through.
    hides: [u64; 6],
    opaque_cube: bool,
    /// The face drawn in the block's mark colour, and the palette entry of that colour.
    mark: Option<(FaceDirection, u32)>,
}

/// Every block's definition, turned into what the mesher, the renderer, lighting and physics ask
/// for most: by block id and state, quickly. Ids are the runtime ids of a game's block registry; an
/// id the table has no definition for is an unknown block, drawn as an opaque magenta cube.
#[derive(Clone, Debug)]
pub struct BlockTable {
    entries: Vec<Entry>,
    models: Vec<Model>,
    unknown: ModelId,
    /// The colours after the blocks' own: every block's state colours, in block order, and then the mark colours.
    state_palette: Vec<[f32; 3]>,
    /// The properties, a column for each, with a value for every model.
    columns: Vec<Column>,
    /// The columns of the face properties of the engine, if the schema declares them.
    marked_face_column: Option<usize>,
    docking_face_column: Option<usize>,
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
        let mut models = Vec::new();
        let mut entries = Vec::with_capacity(definitions.len());
        let mut state_palette = Vec::new();
        for (block, definition) in definitions.iter().enumerate() {
            let Some(definition) = definition else {
                entries.push(Entry {
                    color: MISSING_BLOCK_COLOR,
                    first_model: u32::MAX,
                    state_count: 1,
                    first_state_material: 0,
                    state_color_count: 0,
                });
                continue;
            };
            let first_model = models.len() as u32;
            for shape in definition.shapes() {
                models.push(make_model(shape, definition.see_through, block as u32));
            }
            entries.push(Entry {
                color: definition.color,
                first_model,
                state_count: definition.state_count() as u32,
                first_state_material: (definitions.len() + state_palette.len()) as u32,
                state_color_count: definition.state_colors().len() as u32,
            });
            state_palette.extend_from_slice(definition.state_colors());
        }
        // The model of every unknown block is added last, once.
        let unknown = ModelId(models.len() as u32);
        models.push(make_model(&Shape::full(), false, u32::MAX));
        for entry in &mut entries {
            if entry.first_model == u32::MAX {
                entry.first_model = unknown.0;
            }
        }
        // The properties: a column for each declared key, the default everywhere (so for unknown blocks),
        // then what each definition sets, for each of its states.
        let mut columns: Vec<Column> = schema
            .iter()
            .map(|declaration| Column {
                id: declaration.id.clone(),
                kind: declaration.kind,
                values: vec![declaration.default; models.len()],
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
                        column.values[(entry.first_model + state) as usize] = value;
                    }
                }
            }
        }
        // The marked faces: each colour a block marks a face in takes one palette entry after all the state colours.
        let marked_face_column = position_of(&columns, engine::MARKED_FACE.id());
        if let (Some(face_column), Some(color_column)) = (
            marked_face_column,
            position_of(&columns, engine::MARK_COLOR.id()),
        ) {
            let first_mark_material = (definitions.len() + state_palette.len()) as u32;
            let mut mark_colors: Vec<u32> = Vec::new();
            for entry in entries
                .iter()
                .filter(|entry| entry.first_model != unknown.0)
            {
                for state in 0..entry.state_count {
                    let model = (entry.first_model + state) as usize;
                    let Some(face) = u8::try_from(columns[face_column].values[model])
                        .ok()
                        .and_then(face_from_code)
                    else {
                        continue;
                    };
                    let color = columns[color_column].values[model] as u32;
                    let position = mark_colors
                        .iter()
                        .position(|known| *known == color)
                        .unwrap_or_else(|| {
                            mark_colors.push(color);
                            mark_colors.len() - 1
                        });
                    models[model].mark = Some((face, first_mark_material + position as u32));
                }
            }
            state_palette.extend(mark_colors.into_iter().map(|packed| {
                [16, 8, 0].map(|shift| f32::from((packed >> shift & 0xFF) as u8) / 255.0)
            }));
        }
        let docking_face_column = position_of(&columns, engine::DOCKING_FACE.id());
        Ok(Self {
            entries,
            models,
            unknown,
            state_palette,
            columns,
            marked_face_column,
            docking_face_column,
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
    /// handle must be from this table.
    pub fn get<T: PropertyType>(&self, handle: PropertyHandle<T>, model: ModelId) -> T {
        T::from_value(self.columns[handle.column as usize].values[model.0 as usize])
    }

    /// A property of a block in a state, for a caller that has the block and state and not the model: the key's
    /// default for an unknown block, and the state 0 value for a state the block does not have.
    pub fn get_of<T: PropertyType>(&self, handle: PropertyHandle<T>, block: u32, state: u32) -> T {
        self.get(handle, self.model_of(block, state))
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

    /// The model of a block in a state. A state the block does not have is its state 0, and an unknown
    /// block is an opaque cube.
    pub fn model_of(&self, block: u32, state: u32) -> ModelId {
        let Some(entry) = self.entries.get(block as usize) else {
            return self.unknown;
        };
        let state = if state < entry.state_count { state } else { 0 };
        ModelId(entry.first_model + state)
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

    /// What a face of a voxel is drawn as: the material of the voxel (see [`BlockTable::material_of`]), unless the face is the block's marked
    /// face in this state, which is drawn in the mark colour.
    pub fn material_of_face(&self, block: u32, state: u32, face: FaceDirection) -> u32 {
        match self.marked_face_material(block, state) {
            Some((marked, material)) if marked == face => material,
            _ => self.material_of(block, state),
        }
    }

    /// The face a block marks in a state and the material it is drawn in, if the block marks one.
    pub fn marked_face_material(&self, block: u32, state: u32) -> Option<(FaceDirection, u32)> {
        self.models[self.model_of(block, state).0 as usize].mark
    }

    /// The face of a block that docks, in a state (`engine:docking_face`): `None` for a block that is no docking port.
    pub fn docking_face(&self, block: u32, state: u32) -> Option<FaceDirection> {
        self.face_property(self.docking_face_column, block, state)
    }

    fn face_property(
        &self,
        column: Option<usize>,
        block: u32,
        state: u32,
    ) -> Option<FaceDirection> {
        let model = self.model_of(block, state);
        let value = self.columns[column?].values[model.0 as usize];
        u8::try_from(value).ok().and_then(face_from_code)
    }

    /// The state a block has when the grid it is in is turned by a rotation of the lattice: the state whose shape, marked face and docking
    /// face are those of this state turned, so that a stair rises the way it did, a slab stays on the side it was and a docking port faces
    /// the way the ship does. A state that is already right is kept; if the block has no state of the turned shape, or the state is not
    /// one of its shapes (a number the block's rules use, a fire's age), it is kept as it is.
    pub fn turned_state(&self, block: u32, state: u32, rotation: LatticeRotation) -> u32 {
        let Some(entry) = self.entries.get(block as usize) else {
            return state;
        };
        if rotation == LatticeRotation::IDENTITY || state >= entry.state_count {
            return state;
        }
        let turned_face = |face: Option<FaceDirection>| face.map(|face| rotation.apply_face(face));
        let shape = self.shape(self.model_of(block, state)).turned(rotation);
        let marked = turned_face(self.face_property(self.marked_face_column, block, state));
        let docking = turned_face(self.docking_face(block, state));
        let fits = |candidate: u32| {
            self.shape(self.model_of(block, candidate))
                .same_cells(&shape)
                && self.face_property(self.marked_face_column, block, candidate) == marked
                && self.docking_face(block, candidate) == docking
        };
        if fits(state) {
            return state;
        }
        (0..entry.state_count)
            .find(|candidate| fits(*candidate))
            .unwrap_or(state)
    }

    pub fn shape(&self, model: ModelId) -> &Shape {
        &self.models[model.0 as usize].shape
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
            other.shape.face_cover(face.opposite())
        } else {
            0
        }
    }

    /// The faces light and other fills can pass between through the model: all of them for a
    /// see-through block, else the ones its empty part joins.
    pub fn connections(&self, model: ModelId) -> FaceConnections {
        let model = &self.models[model.0 as usize];
        if model.see_through {
            FaceConnections::all()
        } else {
            model.shape.connections()
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

fn make_model(shape: &Shape, see_through: bool, block: u32) -> Model {
    let hides = std::array::from_fn(|index| {
        if see_through {
            0
        } else {
            FaceDirection::from_index(index as u8).map_or(0, |face| shape.face_cover(face))
        }
    });
    Model {
        shape: shape.clone(),
        block,
        see_through,
        hides,
        opaque_cube: shape.is_full() && !see_through,
        mark: None,
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
        let air = table.model_of(0, 0);
        assert!(!table.is_solid(air) && table.is_see_through(air));
        let cube = table.model_of(1, 0);
        assert!(table.is_solid(cube) && table.is_opaque_cube(cube));
        let bottom_slab = table.model_of(1, 1);
        assert!(table.is_solid(bottom_slab) && !table.is_opaque_cube(bottom_slab));
        assert_eq!(table.shape(bottom_slab), &Shape::half(DOWN));
        assert_ne!(table.model_of(1, 1), table.model_of(1, 2));
    }

    #[test]
    fn what_hides_what_is_behind_it_follows_the_shape_and_see_through_blocks_hide_nothing() {
        let table = table();
        let cube = table.model_of(1, 0);
        assert!(
            FaceDirection::ALL
                .iter()
                .all(|&face| table.hides_fully(cube, face))
        );
        let slab = table.model_of(1, 1);
        assert!(table.hides_fully(slab, DOWN));
        assert!(!table.hides_fully(slab, UP));
        assert_eq!(
            table.hides(slab, FaceDirection::PositiveX),
            0x0F0F_0F0F_0F0F_0F0F
        );
        let glass = table.model_of(2, 0);
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
            table.connections(table.model_of(0, 0)),
            FaceConnections::all()
        );
        assert_eq!(
            table.connections(table.model_of(2, 0)),
            FaceConnections::all()
        );
        assert_eq!(
            table.connections(table.model_of(1, 0)),
            FaceConnections::NONE
        );
        let slab = table.connections(table.model_of(1, 1));
        assert!(slab.connects(UP, FaceDirection::PositiveZ) && !slab.is_open(DOWN));
    }

    #[test]
    fn a_state_the_block_lacks_is_its_first_and_an_unknown_block_is_an_opaque_magenta_cube() {
        let table = table();
        assert_eq!(table.model_of(1, 3), table.model_of(1, 0));
        assert_eq!(table.model_of(1, u32::MAX), table.model_of(1, 0));
        for block in [3, 4, 9999] {
            let model = table.model_of(block, 0);
            assert!(table.is_opaque_cube(model), "block {block}");
            assert_eq!(table.color(block), MISSING_BLOCK_COLOR);
            assert_eq!(table.state_count(block), 1);
            assert_eq!(table.model_of(block, 7), model);
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
        assert_eq!(table.model_of(1, 1_000_000), table.model_of(1, 0));
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
            table.model_of(0, 0),
            table.model_of(1, 0),
            table.model_of(1, 1),
        );
        let (glass_cube, glass_slab) = (table.model_of(2, 0), table.model_of(2, 1));
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
            table.hidden_by_neighbour(glass_cube, east, table.model_of(3, 0)),
            0
        );
    }

    #[test]
    fn of_cubes_makes_whole_opaque_cubes_and_air() {
        let table = BlockTable::of_cubes(&[false, true, true]);
        assert!(!table.is_solid(table.model_of(0, 0)));
        assert!(table.is_opaque_cube(table.model_of(1, 0)));
        assert!(table.is_opaque_cube(table.model_of(2, 0)));
        assert_eq!(table.block_count(), 3);
    }

    const YELLOW: [u8; 3] = [255, 216, 0];

    /// A block of whole cubes with six states, the marked face and the docking face of each looking a different way.
    fn port() -> BlockDefinition {
        let faces = FaceDirection::ALL.map(Some);
        BlockDefinition::cube([0.3, 0.3, 0.32])
            .with_shapes(vec![Shape::full(); 6])
            .with_marked_faces(YELLOW, &faces)
            .with_docking_faces(&faces)
    }

    #[test]
    fn a_marked_face_is_drawn_in_its_own_colour_and_the_other_faces_in_the_blocks() {
        let stone = BlockDefinition::cube([0.5; 3]);
        let tinted = BlockDefinition::cube([0.6; 3]).with_state_colors(vec![[0.9; 3], [0.8; 3]]);
        let port = port();
        let table = BlockTable::from_definitions(&[Some(&stone), Some(&tinted), Some(&port)]);
        // The palette is the three blocks, the two state colours of the tinted one, and the yellow once, however many states mark it.
        let yellow = [255.0 / 255.0, 216.0 / 255.0, 0.0];
        let palette = table.palette();
        assert_eq!(palette.len(), 6);
        assert_eq!(palette[5], yellow);
        for state in 0..6 {
            for face in FaceDirection::ALL {
                let wanted = if face.index() as u32 == state { 5 } else { 2 };
                assert_eq!(
                    table.material_of_face(2, state, face),
                    wanted,
                    "state {state} {face:?}"
                );
            }
            assert_eq!(
                table.marked_face_material(2, state),
                Some((FaceDirection::ALL[state as usize], 5))
            );
        }
        // A block that marks nothing is drawn as it always was, whatever the face; so is a state its block does not have.
        for face in FaceDirection::ALL {
            assert_eq!(table.material_of_face(0, 0, face), table.material_of(0, 0));
            assert_eq!(table.material_of_face(1, 1, face), table.material_of(1, 1));
            assert_eq!(table.material_of_face(9999, 0, face), 9999);
        }
        assert_eq!(table.marked_face_material(0, 0), None);
        // Gunmetal is the block's own colour.
        assert_eq!(table.color(2), [0.3, 0.3, 0.32]);
    }

    #[test]
    fn the_docking_face_of_each_state_is_what_the_definition_says_and_a_block_that_is_no_port_has_none()
     {
        let stone = BlockDefinition::cube([0.5; 3]);
        let port = port();
        let table = BlockTable::from_definitions(&[Some(&stone), Some(&port)]);
        for (state, face) in FaceDirection::ALL.into_iter().enumerate() {
            assert_eq!(table.docking_face(1, state as u32), Some(face));
        }
        assert_eq!(table.docking_face(0, 0), None);
        assert_eq!(table.docking_face(9999, 0), None);
    }

    fn rotations() -> [LatticeRotation; 24] {
        LatticeRotation::all()
    }

    #[test]
    fn a_docking_port_turned_with_its_ship_faces_the_way_the_ship_turned_it() {
        let port = port();
        let table = BlockTable::from_definitions(&[Some(&port)]);
        for rotation in rotations() {
            for face in FaceDirection::ALL {
                let state = u32::from(face.index());
                let turned = table.turned_state(0, state, rotation);
                assert_eq!(
                    table.docking_face(0, turned),
                    Some(rotation.apply_face(face)),
                    "{rotation:?} {face:?}"
                );
                assert_eq!(
                    table.marked_face_material(0, turned).map(|mark| mark.0),
                    Some(rotation.apply_face(face)),
                    "the yellow face goes with the docking face"
                );
            }
        }
    }

    #[test]
    fn slabs_and_stairs_turned_with_their_ship_are_the_state_of_the_turned_shape_and_what_has_no_such_state_is_kept()
     {
        let stone = BlockDefinition::cube([0.5; 3]).with_shapes(vec![
            Shape::full(),
            Shape::half(DOWN),
            Shape::half(UP),
            Shape::stair(FaceDirection::PositiveX),
            Shape::stair(FaceDirection::NegativeX),
            Shape::stair(FaceDirection::NegativeZ),
            Shape::stair(FaceDirection::PositiveZ),
        ]);
        let table = BlockTable::from_definitions(&[Some(&stone)]);
        let find = |wanted: &dyn Fn([i64; 3]) -> [i64; 3]| {
            rotations()
                .into_iter()
                .find(|rotation| {
                    [[1, 0, 0], [0, 1, 0], [0, 0, 1]]
                        .into_iter()
                        .all(|vector| rotation.apply(vector) == wanted(vector))
                })
                .unwrap()
        };
        // Turned over about the x axis (up goes down, z goes to -z): a bottom slab is a top slab, a top slab a bottom slab.
        let over = find(&|v| [v[0], -v[1], -v[2]]);
        assert_eq!(table.turned_state(0, 1, over), 2);
        assert_eq!(table.turned_state(0, 2, over), 1);
        assert_eq!(
            table.turned_state(0, 0, over),
            0,
            "a whole block is as it was"
        );
        // A quarter turn about the vertical axis (+z goes to +x, +x to -z): the stair rising east rises north, and north becomes west.
        let quarter = find(&|v| [v[2], v[1], -v[0]]);
        assert_eq!(table.turned_state(0, 3, quarter), 5);
        assert_eq!(table.turned_state(0, 5, quarter), 4);
        assert_eq!(
            table.turned_state(0, 1, quarter),
            1,
            "a bottom slab stays on the bottom"
        );
        // A quarter turn about the x axis stands a slab on its side, which this block has no state for: it keeps its state.
        let on_its_side = find(&|v| [v[0], -v[2], v[1]]);
        assert_eq!(table.turned_state(0, 1, on_its_side), 1);
        // No turn is no change; a state that is not one of the block's shapes (a number) and an unknown block are kept, whatever the turn.
        for rotation in rotations() {
            assert_eq!(table.turned_state(0, 77, rotation), 77);
            assert_eq!(table.turned_state(5, 3, rotation), 3);
            assert_eq!(table.turned_state(0, 3, LatticeRotation::IDENTITY), 3);
        }
    }

    #[test]
    fn a_turn_and_its_inverse_give_the_state_back_for_every_state_that_has_a_turned_state() {
        let port = port();
        let table = BlockTable::from_definitions(&[Some(&port)]);
        for rotation in rotations() {
            for state in 0..6 {
                let there = table.turned_state(0, state, rotation);
                assert_eq!(table.turned_state(0, there, rotation.inverse()), state);
            }
        }
    }

    #[test]
    fn the_shape_the_marked_face_and_the_docking_face_each_decide_which_state_a_turn_leads_to() {
        use FaceDirection::{NegativeX, NegativeZ, PositiveX, PositiveZ};
        let east_to_south = LatticeRotation::all()
            .into_iter()
            .find(|rotation| {
                rotation.apply_face(PositiveX) == PositiveZ
                    && rotation.apply([0, 1, 0]) == [0, 1, 0]
            })
            .unwrap();
        // Four states of whole cubes: marked east or marked west, each docking east or docking west. Turned so that east is south, the
        // states that face south and north are wanted, and there are none: so use faces that do exist after the turn, by making the
        // block with every combination of south and north as well.
        let combos: Vec<(Option<FaceDirection>, Option<FaceDirection>)> =
            [PositiveX, NegativeX, PositiveZ, NegativeZ]
                .into_iter()
                .flat_map(|mark| {
                    [PositiveX, NegativeX, PositiveZ, NegativeZ]
                        .map(|dock| (Some(mark), Some(dock)))
                })
                .collect();
        let marks: Vec<Option<FaceDirection>> = combos.iter().map(|combo| combo.0).collect();
        let docks: Vec<Option<FaceDirection>> = combos.iter().map(|combo| combo.1).collect();
        let block = BlockDefinition::cube([0.3; 3])
            .with_shapes(vec![Shape::full(); combos.len()])
            .with_marked_faces(YELLOW, &marks)
            .with_docking_faces(&docks);
        let table = BlockTable::from_definitions(&[Some(&block)]);
        for (state, (mark, dock)) in combos.iter().enumerate() {
            let turned = table.turned_state(0, state as u32, east_to_south) as usize;
            assert_eq!(
                combos[turned],
                (
                    mark.map(|face| east_to_south.apply_face(face)),
                    dock.map(|face| east_to_south.apply_face(face))
                ),
                "state {state}"
            );
        }
    }

    #[test]
    fn a_state_that_is_right_already_is_kept_though_another_state_fits_as_well_and_a_number_is_kept_though_a_shape_fits()
     {
        // Two states that are the same whole cube: either is right for either.
        let twins = BlockDefinition::cube([0.5; 3]).with_shapes(vec![Shape::full(), Shape::full()]);
        // Two slabs, and a state number beyond them (a fire's age is the model of its first state).
        let slabs =
            BlockDefinition::cube([0.5; 3]).with_shapes(vec![Shape::half(DOWN), Shape::half(UP)]);
        let table = BlockTable::from_definitions(&[Some(&twins), Some(&slabs)]);
        let over = LatticeRotation::all()
            .into_iter()
            .find(|rotation| rotation.apply([0, 1, 0]) == [0, -1, 0])
            .unwrap();
        assert_eq!(table.turned_state(0, 1, over), 1);
        assert_eq!(table.turned_state(0, 0, over), 0);
        assert_eq!(table.turned_state(1, 0, over), 1);
        assert_eq!(table.turned_state(1, 77, over), 77);
    }
}
