//! What is wrong with a pack, said so that its author can find it: the file, the place in the file, and why.

use std::fmt;

/// One problem in one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackError {
    /// The file as the author knows it, such as `my_pack/blocks/marble.json`.
    pub file: String,
    /// Where in the file, as a JSON pointer (`/properties/base_blocks:emission`); empty for the file as a whole.
    pub path: String,
    pub message: String,
}

impl fmt::Display for PackError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            write!(formatter, "{}: {}", self.file, self.message)
        } else {
            write!(formatter, "{}: {}: {}", self.file, self.path, self.message)
        }
    }
}

impl std::error::Error for PackError {}

/// Every problem found, in the order they were found. Never empty when it is an error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackErrors(pub Vec<PackError>);

impl fmt::Display for PackErrors {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, error) in self.0.iter().enumerate() {
            if index > 0 {
                writeln!(formatter)?;
            }
            write!(formatter, "{error}")?;
        }
        Ok(())
    }
}

impl std::error::Error for PackErrors {}
