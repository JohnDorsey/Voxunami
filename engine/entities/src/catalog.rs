use crate::{EntityKinds, ItemTable};

/// The content entities are made of: the kinds a game has and the items. Built from what plugins
/// declare, and shared by whatever makes, replicates and saves entities.
#[derive(Clone, Debug, Default)]
pub struct EntityCatalog {
    pub kinds: EntityKinds,
    pub items: ItemTable,
}
