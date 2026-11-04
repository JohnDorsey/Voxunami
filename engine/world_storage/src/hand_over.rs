//! The hand-over of a body from the sphere of influence of a planet to that of what the planet goes round (E16 phase B of `docs/orbits-and-thrust-plan.md`).
//!
//! A ship on an orbit round a planet that is itself on an orbit round something else (patched conics, as the owner chose) leaves the planet's sphere at an **instant**, and from then
//! on goes round what the planet goes round: the orbit that its state at that instant is on. The instant is found in closed form and rounded up to a grid of 2^-31 of a second
//! ([`Orbit::leaves_at`]), so that it does not depend on any tick rate: a server only asks whether a tick is at or past it, and the ship is on the right orbit at every tick however many
//! spheres it has crossed since the one before. The state at the instant is the ship's from the planet (its orbit) and the planet's from what it goes round (its orbit), added, in wide numbers; the
//! new orbit is [`Orbit::from_wide_state`] of that, so the path is continuous there to the width of the new elements.
//!
//! **The width.** Each hand-over adds [`BITS_ADDED_BY_A_HAND_OVER`] bits to the width of the orbit it makes, up to the widest rail a server allows (never below the 128 a rail has), because the
//! new orbit is a map of the old one that magnifies what is not exact in it (a flyby loses 22 to 28 bits to the nearness of its periapsis: the plan's E4), and a path that goes through
//! many spheres is held in as many more bits as the spheres that went before it. The path is a function of the first orbit and nothing else: the widths follow from the count of hand-overs.

use wide_float::{Precision, WideFloat};

use crate::rail::{Orbit, RailError, RailTime};

/// The bits a hand-over adds to the width of the elements: a little over the 22 to 28 that a close pass loses (E4 of the plan).
pub const BITS_ADDED_BY_A_HAND_OVER: u32 = 32;

/// Bits over the width of the new elements that the two states are worked out in: the fit of an orbit from a state needs that many (`wide_kepler`'s fit works in them).
const BITS_OF_THE_STATE_OVER_THE_ORBIT: u32 = 128;

impl Orbit {
    /// The first instant of the grid of 2^-31 of a second, at or after `after`, at which the body is at least `radius` voxels from the centre of its orbit, or `None` if it never is
    /// (an ellipse that does not go that far out; an orbit that does not leave before the clock ends: 2^65 seconds). A body that is out at `after` leaves at the first instant of the grid at
    /// or after `after`.
    pub fn leaves_at(&self, radius: f64, after: RailTime) -> Result<Option<RailTime>, RailError> {
        // (A radius that is not a number is refused here, and one that is not above nothing by `wide_kepler`.)
        let distance = WideFloat::from_f64(radius, Precision::MINIMUM)
            .map_err(|_| RailError::Invalid("a sphere has a radius above nothing"))?;
        let found = self
            .elements
            .leaves_at(&distance, after.wide(), self.width())?;
        Ok(found.map(RailTime::of_instant))
    }

    /// The orbit round what `centre` goes round that a body on this orbit is on at `at`: this orbit is round the body whose orbit is `centre`, so the state of the body from what that
    /// goes round is the two states added. `mu` is `G M` of what `centre` goes round. The orbit has the width of the wider of the two plus [`BITS_ADDED_BY_A_HAND_OVER`], at most
    /// `widest_bits` (a server setting) and at least the 128 bits a rail has. It starts at `at`, is one change further along the path than this orbit ([`Orbit::changes`]) and has the same
    /// prediction asked for ([`Orbit::predicted`]).
    ///
    /// Refused: a state that is on no orbit (a body going straight at the middle or away from it, at rest, at the speed of escape), a `mu` that is not above nothing, and an orbit outside the limits
    /// of a rail (see [`Orbit::from_elements`]).
    pub fn handed_over_to(
        &self,
        centre: &Orbit,
        mu: f64,
        at: RailTime,
        widest_bits: u64,
    ) -> Result<Orbit, RailError> {
        let mu = WideFloat::from_f64(mu, Precision::MINIMUM)
            .ok()
            .filter(WideFloat::is_positive)
            .ok_or(RailError::Invalid(
                "what a body goes round weighs something",
            ))?;
        let wanted =
            u64::from(BITS_ADDED_BY_A_HAND_OVER) + self.width().bits().max(centre.width().bits());
        let precision = Precision::new(wanted.min(widest_bits).max(Precision::MINIMUM.bits()))
            .map_err(|_| RailError::Invalid("the width of a rail is a number of bits"))?;
        let working = precision.widened_by(BITS_OF_THE_STATE_OVER_THE_ORBIT);
        let own = self.elements.place_at(at.wide(), working)?;
        let place_of_centre = centre.elements.place_at(at.wide(), working)?;
        let added = |first: &[WideFloat; 3], second: &[WideFloat; 3]| {
            [0, 1, 2].map(|axis| &first[axis] + &second[axis])
        };
        let mut handed_over = Orbit::from_wide_state(
            centre.around,
            &added(&own.position, &place_of_centre.position),
            &added(&own.velocity, &place_of_centre.velocity),
            &mu,
            at,
            precision,
        )?;
        // (One change further on the path of the same state, and the prediction that was asked for is the same.)
        handed_over.changes = self.changes.saturating_add(1);
        handed_over.predicted = self.predicted;
        Ok(handed_over)
    }

    /// How wide the orbit is: the bits of its elements (that of the radius; the phase has more).
    fn width(&self) -> Precision {
        self.elements.radius.precision()
    }
}
