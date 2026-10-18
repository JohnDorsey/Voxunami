//! The columns of the block table that Cave's rules read: the properties the base blocks declare.

use block_models::{BlockTable, PropertyHandle};

/// How much light a block gives off, in a state. The game's package declares the property, so a table
/// built from it has it; a table without it is a mistake in the game.
pub(crate) fn emission(table: &BlockTable) -> PropertyHandle<u8> {
    table
        .handle(&base_blocks::EMISSION)
        .expect("the game declares base_blocks:emission")
}

/// How readily a block catches fire next to a fire.
pub(crate) fn flammability(table: &BlockTable) -> PropertyHandle<u8> {
    table
        .handle(&base_blocks::FLAMMABILITY)
        .expect("the game declares base_blocks:flammability")
}

/// Whether bodies move through a block as if it were not there.
pub(crate) fn passable(table: &BlockTable) -> PropertyHandle<bool> {
    table
        .handle(&base_blocks::PASSABLE)
        .expect("the game declares base_blocks:passable")
}

/// A table of these definitions that has the base blocks' properties declared, for tests of the rules.
#[cfg(test)]
pub(crate) fn test_table(definitions: &[Option<&block_models::BlockDefinition>]) -> BlockTable {
    BlockTable::try_from_definitions(&base_blocks::property_schema(), definitions)
        .expect("the test blocks use declared properties")
}
