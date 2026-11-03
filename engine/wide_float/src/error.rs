/// Why a number could not be made or an operation could not give one.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum NumberError {
    #[error("division by zero")]
    DivisionByZero,
    /// The operation is not defined for this argument (the square root of a negative number, the
    /// logarithm of zero); the text says which.
    #[error("{0}")]
    OutsideDomain(&'static str),
    /// The result is beyond the exponents a number can have (about 2^1,073,741,824).
    #[error("the result is beyond the range of exponents")]
    ExponentRange,
    /// A NaN or an infinity was offered as a number.
    #[error("not a finite number")]
    NotFinite,
    /// The digits given need more bits than the precision they are to be exact in.
    #[error("the number needs {needed} bits and the precision is {allowed}")]
    DoesNotFit { needed: u64, allowed: u64 },
    #[error("malformed number: {0}")]
    Malformed(&'static str),
}
