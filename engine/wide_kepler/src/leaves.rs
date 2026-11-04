//! When a body leaves a sphere round its centre: the instant a ship hands over from the sphere of influence of a planet to that of what the planet goes round
//! (E16 phase B of `docs/orbits-and-thrust-plan.md`).
//!
//! The distance of a body from its centre is `a (1 - e cos E)` on an ellipse and `|a| (e cosh H - 1)` on a hyperbola, so the anomaly at which it is a given distance out is an arc
//! cosine away, the mean anomaly there follows from Kepler's equation, and the time from the mean motion: no search, and no step of time to be coarser than another. The instant is then
//! **rounded up to a grid of 2^-31 of a second** (0.47 nanoseconds, [`INSTANTS_PER_SECOND`]), so that an instant is a [`Time`] and the places, the fit and the rails that already
//! work in [`Time`] need nothing new, and so that it is the same whatever the tick rate of the server is: a tick only asks whether it is at or past it.

use std::cmp::Ordering;

use wide_float::{Precision, WideFloat};

use crate::KeplerError;
use crate::elements::{Elements, GUARD, MOST_UNITS, Time, check_time, fraction_of};

/// The units of a second of the instants that [`Elements::leaves_at`] gives: 2^31, so that an instant of the grid is exact in any binary number.
pub const INSTANTS_PER_SECOND: u32 = 1 << 31;

impl Elements {
    /// The first instant of the grid of 2^-31 of a second (a [`Time`] of [`INSTANTS_PER_SECOND`] units a second), at or after `after`, at which the body is at least `distance`
    /// from its centre, or `None` if there is none: an ellipse that goes out no farther than `distance` (touching it is not leaving it), one that does so only for a stretch of time
    /// too short for an instant of the grid to be in it, or an instant too far off for the clock (2^96 units of the grid, 2^65 seconds).
    ///
    /// A body that is out already at `after` is out at the first instant of the grid at or after it (whichever way it is going: this is for a body that is inside, and says only
    /// that it is not inside). The working is in `precision` bits and 96 more, and the instant is the first of the grid after the crossing as worked out there: for any
    /// orbit that is not a hair from touching the distance it is the first instant at which the body is out, which the tests check against the place the orbit gives at it and at
    /// the instants before it.
    ///
    /// Refused: a distance that is not above nothing, a time the clock cannot hold, and elements that [`Elements::validate`] refuses.
    pub fn leaves_at(
        &self,
        distance: &WideFloat,
        after: Time,
        precision: Precision,
    ) -> Result<Option<Time>, KeplerError> {
        self.validate()?;
        check_time(after)?;
        if !distance.is_positive() {
            return Err(KeplerError::Invalid(
                "a body leaves a sphere that has a radius",
            ));
        }
        let width = precision.widened_by(GUARD);
        let at = |number: &WideFloat| number.rounded_to(width);
        let seconds_after = WideFloat::from_i128(after.units, width).divide(
            &WideFloat::from_i64(i64::from(after.units_per_second), width),
        )?;
        let crossing = if self.is_a_hyperbola() {
            self.hyperbola_crossing(&at(distance), after, &seconds_after, width)?
        } else {
            match self.ellipse_crossing(&at(distance), after, &seconds_after, width)? {
                Some(crossing) => crossing,
                None => return Ok(None),
            }
        };
        let Some(instant) = on_the_grid_after(&crossing.seconds)? else {
            return Ok(None);
        };
        if !crossing.already_out && !self.is_out_at(distance, instant, width)? {
            // (The crossing was worked out as in a stretch too short for the grid to have an instant in it.)
            return Ok(None);
        }
        Ok(Some(instant))
    }

    /// Whether the body is at least `distance` from its centre at `time`.
    fn is_out_at(
        &self,
        distance: &WideFloat,
        time: Time,
        width: Precision,
    ) -> Result<bool, KeplerError> {
        let place = self.place_at(time, width)?.position;
        let squared = &(&place[0].square() + &place[1].square()) + &place[2].square();
        Ok(squared.compare(&distance.rounded_to(width).square()) != Ordering::Less)
    }

    /// The time of the crossing of an ellipse, in seconds, or `None` if the ellipse does not reach `distance`. The distance is `a (1 - e cos E)`, so `cos E = (1 - distance / a) / e`,
    /// and `E` is the arc tangent of its sine and cosine, the sine being `sqrt((1 - cos E)(1 + cos E))` with both factors worked out from the ratio and not from the cosine
    /// so that nothing cancels near the ends. The mean anomaly is `M = ratio E + e (E - sin E)`. The distance grows from the nearest point (M = 0) to the far end (M = pi), so the body
    /// is out from `M` to `2 pi - M`, going the way the period says (a retrograde body is mirrored, the distance being the same at `M` and `-M`).
    fn ellipse_crossing(
        &self,
        distance: &WideFloat,
        after: Time,
        seconds_after: &WideFloat,
        width: Precision,
    ) -> Result<Option<Crossing>, KeplerError> {
        let at = |number: &WideFloat| number.rounded_to(width);
        let (radius, ratio) = (at(&self.radius), at(&self.ratio));
        let one = WideFloat::from_i64(1, width);
        let two = WideFloat::from_i64(2, width);
        let already_out = || Crossing {
            seconds: seconds_after.clone(),
            already_out: true,
        };
        if distance.compare(&(&radius * &ratio)) != Ordering::Greater {
            return Ok(Some(already_out()));
        }
        let eccentricity = &one - &ratio;
        let in_radii = distance.divide(&radius)?;
        let down = (&in_radii - &ratio).divide(&eccentricity)?;
        let up = (&(&two - &ratio) - &in_radii).divide(&eccentricity)?;
        // (The far end is `a (2 - ratio)`: where `up` is not above nothing the distance is at it or past it, and touching is not leaving.)
        if !up.is_positive() {
            return Ok(None);
        }
        if !down.is_positive() {
            return Ok(Some(already_out()));
        }
        let sine = (&down * &up).sqrt()?;
        let cosine = (&one - &in_radii).divide(&eccentricity)?;
        let eccentric = sine.atan2(&cosine)?;
        let mean_at_crossing = &(&ratio * &eccentric) + &(&eccentricity * &(&eccentric - &sine));

        // Where the body is now, as a part of a turn the way it goes (a body going the other way round is mirrored), and the stretch of turn to the crossing.
        let turns = self.mean_anomaly_in_turns(after, width)?;
        let turns = if self.period.is_negative() {
            fraction_of(&-&turns)?
        } else {
            turns
        };
        let two_pi = WideFloat::pi(width).times_power_of_two(1)?;
        let now = &two_pi * &turns;
        let gap = if now.compare(&mean_at_crossing) == Ordering::Less {
            &mean_at_crossing - &now
        } else if now.compare(&(&two_pi - &mean_at_crossing)) == Ordering::Greater {
            &(&two_pi - &now) + &mean_at_crossing
        } else {
            return Ok(Some(already_out()));
        };
        // A radian takes |period| / 2 pi seconds.
        let seconds = seconds_after + &(&gap * &at(&self.period).abs()).divide(&two_pi)?;
        Ok(Some(Crossing {
            seconds,
            already_out: false,
        }))
    }

    /// The time of the crossing of a hyperbola, in seconds. The distance is `|a| (e cosh H - 1)`, so `cosh H = (distance / |a| + 1) / e`, `M = e sinh H - H` written as
    /// `(e - 1) sinh H + (sinh H - H)` (its terms are not below nothing together), and the mean anomaly grows with the time at the rate the period says, so the time to the crossing is
    /// the difference of the mean anomalies over the rate. A body whose mean anomaly is already beyond that of the crossing, either side of the periapsis, is out.
    fn hyperbola_crossing(
        &self,
        distance: &WideFloat,
        after: Time,
        seconds_after: &WideFloat,
        width: Precision,
    ) -> Result<Crossing, KeplerError> {
        let at = |number: &WideFloat| number.rounded_to(width);
        let (radius, size) = (at(&self.radius), at(&self.ratio).abs());
        let one = WideFloat::from_i64(1, width);
        let cosh = (&distance.divide(&radius)? + &one).divide(&(&one + &size))?;
        let already_out = || Crossing {
            seconds: seconds_after.clone(),
            already_out: true,
        };
        if cosh.compare(&one) != Ordering::Greater {
            return Ok(already_out());
        }
        let anomaly = cosh.acosh()?;
        let sinh = anomaly.sinh()?;
        let mean_at_crossing = &(&size * &sinh) + &(&sinh - &anomaly);
        let mean = self.hyperbolic_mean_anomaly(after, width)?;
        if mean.abs().compare(&mean_at_crossing) != Ordering::Less {
            return Ok(already_out());
        }
        let rate = WideFloat::pi(width)
            .times_power_of_two(1)?
            .divide(&at(&self.period))?;
        let target = if self.period.is_negative() {
            -&mean_at_crossing
        } else {
            mean_at_crossing
        };
        Ok(Crossing {
            seconds: seconds_after + &(&target - &mean).divide(&rate)?,
            already_out: false,
        })
    }
}

/// When a body crosses, in seconds, and whether it was out already at the time asked from (so that the crossing is that time and is not one to check).
struct Crossing {
    seconds: WideFloat,
    already_out: bool,
}

/// The first instant of the grid at or after a time in seconds, or `None` if the clock cannot hold it.
fn on_the_grid_after(seconds: &WideFloat) -> Result<Option<Time>, KeplerError> {
    let units_of_the_grid = seconds.times_power_of_two(31)?;
    let Some(floor) = units_of_the_grid.floor_to_i128() else {
        return Ok(None);
    };
    let whole = units_of_the_grid
        .compare(&WideFloat::from_i128(floor, units_of_the_grid.precision()))
        == Ordering::Equal;
    let units = if whole {
        Some(floor)
    } else {
        floor.checked_add(1)
    };
    Ok(units
        .filter(|units| units.unsigned_abs() < MOST_UNITS)
        .map(|units| Time {
            units,
            units_per_second: INSTANTS_PER_SECOND,
        }))
}
