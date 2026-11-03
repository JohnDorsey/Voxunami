//! The number layer: real numbers of a chosen number of bits, correctly rounded, the same on
//! every machine. What the orbits of ships, the flybys of a chain of planets and their previews
//! are computed in, from 128 bits for a ship round one planet to thousands of bits for a flight
//! with dozens of gravity assists (`docs/orbits-and-thrust-plan.md`, E4 and E6).
//!
//! **Why this exists.** An `f64` has 53 bits. The far end of an orbit a light-year across is
//! fixed by it to about two metres, a chain of twelve assists needs 336 bits to be right to a
//! tenth of a millimetre at its end, and one of three hundred needs 8,400. The rules of the
//! project still hold: the result must be bit for bit the same on Windows and Linux, on the
//! server and on a client that works out a preview, and it must not depend on how the computation
//! was scheduled.
//!
//! **How that is kept.** Every operation of [`WideFloat`] gives the correctly rounded value of
//! the exact result (nearest, ties to even). That is a property of the mathematics and not of a
//! library, so any library that is correctly rounded gives the same bits; this crate uses
//! `malachite-float` (LGPL-3.0, pure Rust, the fastest of the four tried), names it nowhere else,
//! and its tests (`tests/`) would find a library or a version that is not. Nothing uses an
//! instruction of the machine's floating-point unit, so there is no `libm` and no fast-math to
//! worry about, and `f64` appears only at the edges ([`WideFloat::from_f64`] and
//! [`WideFloat::to_f64`]).
//!
//! **Precision** is a width, 128 bits or more ([`Precision`]); numbers carry theirs, operations
//! give the wider of their arguments', and [`WideFloat::rounded_to`] sets it. A number is saved
//! with its precision and comes back equal to the bit (see [`codec`](WideFloat::write_to)).
//!
//! **A computation never has to finish.** One operation at 8 million bits takes seconds, and
//! there is no way to stop it; a long computation (a preview a client works out in a thread)
//! checks for being cancelled between operations, which is its own business.
//!
//! ```
//! use wide_float::{Precision, WideFloat};
//!
//! let precision = Precision::of(256);
//! let two = WideFloat::from_i64(2, precision);
//! let root = two.sqrt().unwrap();
//! // The square of the correctly rounded root of two is within a few units of the last place of two.
//! let error = &(&root * &root) - &two;
//! assert!(error.abs().compare(&two.times_power_of_two(-250).unwrap()).is_lt());
//! ```

mod codec;
mod error;
mod precision;
mod wide_float;

#[cfg(feature = "test-support")]
pub mod test_support;

pub use error::NumberError;
pub use precision::{Precision, PrecisionOutOfRange};
pub use wide_float::{Parts, WideFloat};
