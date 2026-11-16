//! The one block table of a running server, shared by everything that reads it.
//!
//! A server's rules (what a block weighs, what it lights, what a body can pass, what burns) all read the same table, and a table can be replaced while the
//! server runs (a data pack edited, a block added: see `hot-reload-plan.md`). So the server holds it in one place, a [`SharedBlockTable`], and every
//! part that reads it holds a clone of that handle. A clone is another name for the same table, not a copy of it: replacing the table through any clone is
//! seen by every other.
//!
//! What a part reads is a [`SharedBlockTable::current`] snapshot, an `Arc<BlockTable>` that never changes under its holder, and what it works out from
//! the table (the shapes it has made colliders of, the light it has spread) it works out again when [`SharedBlockTable::version`] is not the one it worked
//! from. What it works out *about the table's layout* it does not: a replacement must declare the same properties as the table it replaces, so that a
//! [`PropertyHandle`](crate::PropertyHandle) taken once stays right (a replacement that does not is refused).
//!
//! A replacement is made between the ticks of the server, never while a rule is running.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard};

use crate::BlockTable;

/// Why a table was not replaced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PropertiesChanged;

impl std::fmt::Display for PropertiesChanged {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(
            "the new table declares other properties than the one it would replace (the properties of a game are declared when it starts)",
        )
    }
}

impl std::error::Error for PropertiesChanged {}

#[derive(Debug)]
struct Inner {
    table: RwLock<Arc<BlockTable>>,
    /// How many times the table has been replaced: 0 for the one the handle was made with. Changed only with the lock of the table held for writing, so
    /// a reader that holds it for reading sees the table of this version.
    version: AtomicU64,
}

/// A block table that can be replaced, held by everything that reads it. Cloning gives another handle to the same table.
#[derive(Clone, Debug)]
pub struct SharedBlockTable {
    inner: Arc<Inner>,
}

impl SharedBlockTable {
    /// A handle to `table`, at version 0.
    pub fn new(table: impl Into<Arc<BlockTable>>) -> Self {
        Self {
            inner: Arc::new(Inner {
                table: RwLock::new(table.into()),
                version: AtomicU64::new(0),
            }),
        }
    }

    /// The table now, which stays as it is for whoever holds it, whatever is replaced after.
    pub fn current(&self) -> Arc<BlockTable> {
        Arc::clone(&self.read())
    }

    /// How many times the table has been replaced. A part that worked something out from the table keeps the version it worked from and works it out
    /// again when this is another. Cheap (one atomic read, no lock): it may be asked of every voxel.
    pub fn version(&self) -> u64 {
        self.inner.version.load(Ordering::Acquire)
    }

    /// The table and its version as they are together: a replacement cannot come between the two.
    pub fn snapshot(&self) -> (Arc<BlockTable>, u64) {
        let table = self.read();
        (Arc::clone(&table), self.version())
    }

    /// Makes `table` the table of every holder of this handle, and returns the new version. Refused, and nothing changes, if `table` does not declare the same
    /// properties as the one it replaces (see the module).
    pub fn replace(&self, table: impl Into<Arc<BlockTable>>) -> Result<u64, PropertiesChanged> {
        let table = table.into();
        let mut current = self
            .inner
            .table
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        if !current.has_the_properties_of(&table) {
            return Err(PropertiesChanged);
        }
        *current = table;
        // (Still holding the lock: a reader that sees this version sees this table.)
        Ok(self.inner.version.fetch_add(1, Ordering::AcqRel) + 1)
    }

    /// Whether `other` is a handle to the same table as this one (a clone of it), not merely one that holds an equal table.
    pub fn is_the_same_as(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// How many handles there are to this table, this one included. For a test of what a server's parts share.
    pub fn handle_count(&self) -> usize {
        Arc::strong_count(&self.inner)
    }

    /// How many places hold the table that is current now: this handle's own hold, and every snapshot taken of it that is still kept. For a test of what a
    /// server's parts share: a part that made a table of its own holds none of this one.
    pub fn table_holders(&self) -> usize {
        Arc::strong_count(&self.read())
    }

    fn read(&self) -> RwLockReadGuard<'_, Arc<BlockTable>> {
        self.inner
            .table
            .read()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests;
