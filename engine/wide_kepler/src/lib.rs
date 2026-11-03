//! Where a body is on an ellipse or a hyperbola at a time, worked out in numbers of any width from 128 bits (`wide_float`), for the rails of ships that fly for a long time
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
//!
//! **Hyperbolas.** A `ratio` below nothing (e above 1; `radius` is the size of the semi-major axis, `period` is 2 pi over the mean motion, `phase` is the mean anomaly at time 0 as it
//! is, not taken modulo a turn) is a hyperbola: Kepler's equation is `M = e sinh H - H`, solved the same way, and [`Elements::from_state`] finds one from a body that is going faster
//! than it would to escape. Reference rows made another way (`tests/reference/generate_hyperbola.py`) hold the answer to its last bit. A hyperbola goes round no turn, so there is no
//! mean anomaly in turns for one; a parabola (a ratio of nothing) is not made.
//!
//! **One orbit at many times.** A rail is asked for the same orbit at one tick after another, so what a place needs of an orbit whatever the time is (the directions of
//! its plane, the sine and cosine of the periapsis, the minor axis, the rate) is worked out once, [`Elements::prepare`], and [`PreparedElements::place_at`] is the rest
//! (19.7 of the 27 microseconds of a place of an ellipse at 128 bits). [`Elements::place_at`] is the two in a row, so the two ways give the same bits.

mod elements;
mod error;
mod fit;
mod kepler;

pub use elements::{Elements, Place, PreparedElements, Time};
pub use error::KeplerError;
