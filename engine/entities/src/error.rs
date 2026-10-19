use namespaced_id::NamespacedId;
use protocol::EntityId;
use voxel_space::GridId;

/// Why an entity, a kind, an item or an inventory could not be made or changed as asked.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EntityError {
    #[error("the entity kind {0} is declared twice")]
    DuplicateKind(NamespacedId),
    #[error("the item {0} is declared twice")]
    DuplicateItem(NamespacedId),
    #[error("there is no entity kind {0}")]
    UnknownKind(NamespacedId),
    #[error("there is no item {0}")]
    UnknownItem(NamespacedId),
    #[error("there is no {0}")]
    UnknownEntity(EntityId),
    #[error("the entity kind {kind} has no component {component}")]
    UndeclaredComponent {
        kind: NamespacedId,
        component: NamespacedId,
    },
    #[error("the component {component} of an entity could not be read: {reason}")]
    UndecodableComponent {
        component: NamespacedId,
        reason: String,
    },
    #[error("{0} is not a root grid, and until structures exist only root grids hold entities")]
    NotARootGrid(GridId),
    #[error("an item stack must hold at least one item")]
    EmptyStack,
    #[error("an inventory has no slot {slot}; it has {slots}")]
    NoSuchSlot { slot: usize, slots: usize },
}
