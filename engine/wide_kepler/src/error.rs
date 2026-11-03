use wide_float::NumberError;

/// Why a place cannot be worked out.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum KeplerError {
    /// The elements are not those of an orbit (the text says what is wrong).
    #[error("{0}")]
    Invalid(&'static str),
    /// Newton's method did not settle, which it does for every orbit that [`crate::Elements::validate`] accepts, so this is a bug if it is ever seen.
    #[error("Kepler's equation did not settle")]
    DidNotSettle,
    #[error(transparent)]
    Number(#[from] NumberError),
}
