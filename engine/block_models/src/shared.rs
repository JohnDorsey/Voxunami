//! The one block table of a running server, shared by everything that reads it.
//!
//! A server's rules (what a block weighs, what it lights, what a body can pass, what burns) all read the same table, and a table can be replaced while the
//! server runs (a data pack edited, a block added: see `hot-reload-plan.md`). So the server holds it in one place, a [`SharedBlockTable`], and every
//! part that reads it holds a clone of that handle. A clone is another name for the same table, not a copy of it: replacing the table through any clone is
//! seen by every other.
//!
//! What a part reads is a [`SharedBlockTable::current`] snapshot, an `Arc<BlockTable>` that never changes under its holder, and what it works out from
//! the table (the column of a property, the shapes it has made colliders of) it works out again when [`SharedBlockTable::version`] is not the one it
//! worked from.

use std::sync::{Arc, PoisonError, RwLock};

use crate::BlockTable;

#[derive(Debug)]
struct Current {
    table: Arc<BlockTable>,
    /// How many times the table has been replaced: 0 for the one the handle was made with.
    version: u64,
}

/// A block table that can be replaced, held by everything that reads it. Cloning gives another handle to the same table.
#[derive(Clone, Debug)]
pub struct SharedBlockTable {
    current: Arc<RwLock<Current>>,
}

impl SharedBlockTable {
    /// A handle to `table`, at version 0.
    pub fn new(table: impl Into<Arc<BlockTable>>) -> Self {
        Self {
            current: Arc::new(RwLock::new(Current {
                table: table.into(),
                version: 0,
            })),
        }
    }

    /// The table now, which stays as it is for whoever holds it, whatever is replaced after.
    pub fn current(&self) -> Arc<BlockTable> {
        Arc::clone(&self.read().table)
    }

    /// How many times the table has been replaced. A part that worked something out from the table keeps the version it worked from and works it out
    /// again when this is another.
    pub fn version(&self) -> u64 {
        self.read().version
    }

    /// The table and its version as they are together: a replacement cannot come between the two.
    pub fn snapshot(&self) -> (Arc<BlockTable>, u64) {
        let current = self.read();
        (Arc::clone(&current.table), current.version)
    }

    /// Makes `table` the table of every holder of this handle, and returns the new version.
    pub fn replace(&self, table: impl Into<Arc<BlockTable>>) -> u64 {
        let mut current = self.current.write().unwrap_or_else(PoisonError::into_inner);
        current.table = table.into();
        current.version += 1;
        current.version
    }

    /// Whether `other` is a handle to the same table as this one (a clone of it), not merely one that holds an equal table.
    pub fn is_the_same_as(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.current, &other.current)
    }

    /// How many handles there are to this table, this one included. For a test of what a server's parts share.
    pub fn handle_count(&self) -> usize {
        Arc::strong_count(&self.current)
    }

    /// How many places hold the table that is current now: this handle's own hold, and every snapshot taken of it that is still kept. For a test of what a
    /// server's parts share: a part that made a table of its own holds none of this one.
    pub fn table_holders(&self) -> usize {
        Arc::strong_count(&self.read().table)
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, Current> {
        self.current.read().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests;
