//! What blocks look like and how they fill their cell.
//!
//! A voxel holds a *block* (a type from a game's registry) and a *state* of it. A
//! [`BlockDefinition`] gives each state a [`Shape`]: the part of the voxel the block fills, which is
//! the whole cell for an ordinary block and less for a slab or a stair. Shapes are made of boxes on a
//! grid of eighths of a voxel, and everything the engine wants to know about one is worked out when it
//! is made: which sub-faces show, which faces it covers (so it hides what is behind them), and which
//! faces its empty part joins (so light and other fills can pass).
//!
//! A [`BlockTable`] holds all the definitions of a game and answers by block id and state. The mesher
//! uses it to decide what to draw, the renderer for colours, and lighting and physics (built later)
//! for what passes and what is solid. The grid stays cubic; only what a block fills of its cell varies.

mod definition;
mod property;
mod shape;
mod table;

pub use definition::BlockDefinition;
pub use property::{
    PropertyDeclaration, PropertyError, PropertyHandle, PropertyKey, PropertyKind, PropertySchema,
    PropertyType, engine,
};
pub use shape::{FaceConnections, SUBDIVISIONS, Shape, ShapeBox, SubRect, face_axes};
pub use table::{BlockTable, MISSING_BLOCK_COLOR, ModelId};
