//! Entities: the server's model of things in the world that are not voxels.
//!
//! An [`Entity`] has an identity (an [`EntityId`](protocol::EntityId) from a counter saved with the
//! world), a kind (content a plugin declares: [`EntityKindSpec`]), a pose (a grid and a place and
//! orientation in it) and typed components kept as the bytes their own codecs make ([`ComponentBag`]).
//! A component is a [`protocol::Message`] type, so every component has an encoding, samples and a
//! generated round-trip check.
//!
//! Items are content too ([`ItemSpec`]); a stack of them ([`ItemStack`]) carries components of its own,
//! and an [`Inventory`] of slots is a component of whatever carries items.
//!
//! Everything here is deterministic: iteration is by identifier, nothing reads a clock, and a pose is
//! compared by bits. Replication to clients and saving are built on top of this crate (see
//! `docs/entities-spec.md`).

mod catalog;
mod component;
mod error;
mod inventory;
mod item;
mod kind;
mod store;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use catalog::EntityCatalog;
pub use component::ComponentBag;
pub use error::EntityError;
pub use inventory::{Inventory, MAXIMUM_SLOTS};
pub use item::{ItemSpec, ItemStack, ItemTable};
pub use kind::{ComponentDeclaration, EntityKindSpec, EntityKinds};
pub use store::{Entity, EntityStore};
