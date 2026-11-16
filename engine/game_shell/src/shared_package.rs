//! The package of a running server, which can be replaced: what every part of the server that reads the blocks reads them from.
//!
//! A [`SharedPackage`] holds the [`PackageData`] a server describes its game with and the block table made of it (a [`SharedBlockTable`]). The parts that read
//! only how blocks behave (what they weigh, light, pass) hold the table's handle; the parts that need to know which block is which (a player says a block by its
//! identifier, an item is named for a block, a flag says which blocks can be placed) read the package, whose blocks are in the order of their numbers.
//!
//! A replacement (`hot-reload-plan.md`, step 5) is made between ticks. It is refused, and nothing changes, if the new package's blocks cannot make a table or the table
//! declares other properties than the one it replaces.

use std::sync::{Arc, PoisonError, RwLock};

use block_models::{PropertiesChanged, PropertyError, SharedBlockTable};

use crate::PackageData;

/// Why a package was not replaced.
#[derive(Debug, thiserror::Error)]
pub enum ReplaceError {
    #[error("the blocks of the package cannot make a table: {0}")]
    Table(#[from] PropertyError),
    #[error("{0}")]
    Properties(#[from] PropertiesChanged),
}

/// The package of a server and its table of blocks, held by every part that reads either. Cloning gives another handle to the same.
#[derive(Clone, Debug)]
pub struct SharedPackage {
    table: SharedBlockTable,
    package: Arc<RwLock<Arc<PackageData>>>,
}

impl SharedPackage {
    /// A handle to `package`, at version 0. An error if its blocks cannot make a table.
    pub fn new(package: PackageData) -> Result<Self, PropertyError> {
        let table = SharedBlockTable::new(package.block_table()?);
        Ok(Self {
            table,
            package: Arc::new(RwLock::new(Arc::new(package))),
        })
    }

    /// The handle of the block table, for the parts that read only how blocks behave.
    pub fn table(&self) -> &SharedBlockTable {
        &self.table
    }

    /// The package now, which stays as it is for whoever holds it. Its blocks are in the order of their numbers.
    pub fn package(&self) -> Arc<PackageData> {
        Arc::clone(&self.package.read().unwrap_or_else(PoisonError::into_inner))
    }

    /// How many times the package has been replaced (the table's version).
    pub fn version(&self) -> u64 {
        self.table.version()
    }

    /// Makes `package` the package of every holder of this handle, and its table the table, and returns the new version. Nothing changes if it is refused.
    pub fn replace(&self, package: PackageData) -> Result<u64, ReplaceError> {
        let table = package.block_table()?;
        // (The lock of the package is held for the whole of the replacement, so that a reader of the package waits for it, and the version moves last.)
        let mut held = self.package.write().unwrap_or_else(PoisonError::into_inner);
        let version = self.table.replace(table)?;
        *held = Arc::new(package);
        Ok(version)
    }

    /// Whether `other` is a handle to the same package as this one (a clone of it).
    pub fn is_the_same_as(&self, other: &Self) -> bool {
        self.table.is_the_same_as(&other.table)
    }
}

#[cfg(test)]
mod tests;
