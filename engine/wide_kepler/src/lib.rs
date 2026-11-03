//! Where a body is on an ellipse at a time, worked out in numbers of any width from 128 bits (`wide_float`), for the rails of ships that fly for a long time
//! (`docs/orbits-and-thrust-plan.md`, E4 to E7).
//!
//! The definition is the one the rails of `world_storage` already have (`engine/world_storage/tools/exact_ellipse_reference.py` states it): the elements are
//! the plane's `normal`, the semi-major axis `radius`, `ratio` = 1 - e, the angle `periapsis` of the nearest point, the `period` and the `phase` (the mean
//! anomaly at time 0); the mean anomaly at a time is the phase and the turns gone, `time / period` taken modulo one **exactly**; Kepler's equation
//! `E - e sin E = M` gives the eccentric anomaly, and the place and the speed follow. What is new is that every element is a [`wide_float::WideFloat`] and the answer is
//! as exact as the width asked for: in a world of rails of 53-bit elements two ships can be told apart by a millimetre at a light year, and in one of
//! 4,096-bit elements by a hair at the far side of a galaxy.
//!
//! **What the answer is worth.** Asked for at a [`wide_float::Precision`] of `p` bits, a place is within `radius x 2^-(p - K)` of the truth and a speed within
//! `speed x 2^-(p - K)`, `K` being a few bits (measured by the tests, which say how many). So the width to ask for is the one that makes `radius x 2^-p`
//! smaller than the step wanted in the place: 128 bits for a ship round one planet (a radius of 10^7 m to 10^-31 m), 192 to 256 for a light year to a hair.
//!
//! **How.** Everything is done at a working width a little over the one asked for: 64 bits more because the equation is solved with a slope as small as the ratio
//! (down to 2^-64), and some more for the roundings. The equation is solved by Newton's method from a first guess made in `f64` (with `libm`, the same on every
//! machine), in the form `ratio E + e (E - sin E) - M`, which has no cancellation that matters, from the left or the right alike because the function is convex
//! on the half turn it is solved in; the iteration ends when a step is under 2^-(width + 8) of the answer, so how many steps it takes is a property of the numbers
//! and not of the machine or the clock.

mod elements;
mod error;
mod kepler;

pub use elements::{Elements, Place, Time};
pub use error::KeplerError;
pub use kepler::equation_in_f64;
