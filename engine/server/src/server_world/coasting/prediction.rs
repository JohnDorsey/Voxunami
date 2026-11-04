//! The prediction of the path of a ship through the spheres of influence it will cross (E16 phase B of `docs/orbits-and-thrust-plan.md`).
//!
//! A ship's path from the last time it was put on an orbit from a state (a manoeuvre, a push, a change of the mass of what it goes round) is a function of that orbit alone: the orbit round
//! the planet, the instant it leaves the planet's sphere, the orbit round what the planet goes round that its state is on there, and so on. A prediction **walks** that function ahead without
//! touching the world, with the same two functions the ship's hand-over uses, so that what is shown is the rails the ship will be on, to the bit.
//!
//! **How much is shown** (the owner's answers of 2026-11-03). The ship keeps [`Orbit::predicted`], the number of changes of sphere after the state that a player asked for, and
//! [`Orbit::changes`], the number it has been through. The motion of the ship through the spheres does not advance what was asked for: if the last rail shown goes through a planet's sphere
//! and the player waits until the ship has been through all those before it, the last rail shown is still that one. What is shown is that number of changes ahead, never more
//! than [`RailSettings::most_predicted_changes`] and never fewer than [`RailSettings::fewest_rails_shown`] rails (the one the ship is on and the ones after it): when the ship's motion brings the
//! end down to the fewest, the end moves on by itself. A path that is regenerated from the ship's saved orbit is what it was, however long the ship was away and however many spheres it went
//! through, since the orbit it is saved on is the one on its path where it is, with the number asked for kept.

use super::*;

/// A rail of a predicted path: the orbit, and the stretch of time the ship is on it.
#[derive(Clone, Debug, PartialEq)]
pub struct PredictedRail {
    pub orbit: Orbit,
    /// The time the ship goes on it: the instant of the crossing that put it there (or, for the rail it is on now, the time it was put on it).
    pub from: RailTime,
    /// The instant it leaves the sphere of what it goes round, on the grid of 2^-31 of a second, or `None` if it never does.
    pub until: Option<RailTime>,
}

/// Why a prediction stops where it does.
#[derive(Clone, Debug, PartialEq)]
pub enum PredictionEnd {
    /// The last rail is the one the ship stays on for ever: it never leaves the sphere of what it goes round.
    ForEver,
    /// The last rail leaves its sphere (at its `until`) and the path goes on, but no further than the changes asked for are shown.
    AsFarAsShown,
    /// The orbit the ship would be on after the last rail is not one a rail may have (it comes lower than the ground it goes round, or what it goes round pulls nothing): the ship comes
    /// off its rail there, and goes on as it was going.
    Lost(NotCoasting),
}

/// The path of a ship through the spheres it will cross, as far as is shown.
#[derive(Clone, Debug, PartialEq)]
pub struct Prediction {
    /// The rail the ship is on, and the ones after it: at least one.
    pub rails: Vec<PredictedRail>,
    pub end: PredictionEnd,
}

impl ServerWorld {
    /// How many changes of sphere ahead of an orbit a prediction shows: what was asked for after the ones gone through, but at least what the fewest rails need (one less, the rail the
    /// ship is on being one of them) and at most what the host allows.
    fn changes_to_show(&self, orbit: &Orbit) -> u32 {
        let asked = orbit.predicted.saturating_sub(orbit.changes);
        let floor = self.rail_settings.fewest_rails_shown.saturating_sub(1);
        asked
            .max(floor)
            .min(self.rail_settings.most_predicted_changes)
    }

    /// The path of a ship that is on a rail round a planet, as far as the changes of sphere shown go (see the module). Nothing in the world changes. `now` is the time of the world, for an
    /// orbit that was not made from a state and so does not say when it began. `None` for a ship that is not on a rail of this kind.
    pub fn predicted_path(&self, ship: GridId, now: RailTime) -> Option<Prediction> {
        let body = self.data.grids().get(ship)?;
        // (A planet on a rail goes round another and is not a ship that is predicted.)
        if body.terrain.is_some() {
            return None;
        }
        let first = body
            .rail
            .as_ref()
            .filter(|rail| is_a_coasting_rail(rail))?
            .orbit
            .clone()?;
        let to_show = self.changes_to_show(&first) as usize;
        let mut rails = Vec::new();
        let mut orbit = first;
        let end = loop {
            let until = match orbit.around {
                Centre::Grid(parent) => self
                    .sphere_of_influence(parent)
                    .and_then(|sphere| search_for_exit(&orbit, sphere, now).0),
                Centre::Point(_) => None,
            };
            rails.push(PredictedRail {
                from: orbit.since.unwrap_or(now),
                until,
                orbit: orbit.clone(),
            });
            let Some(instant) = until else {
                break PredictionEnd::ForEver;
            };
            if rails.len() > to_show {
                break PredictionEnd::AsFarAsShown;
            }
            match self.orbit_after(&orbit, instant) {
                Ok(next) => orbit = next,
                Err(error) => break PredictionEnd::Lost(error),
            }
        };
        Some(Prediction { rails, end })
    }

    /// The player's "continue prediction": one more change of sphere is shown for a ship, if there is one more on its path and the host's most is not reached. Returns how many changes are
    /// shown now (the same as before if it could not go on), or `None` for a ship that is not on a rail of this kind. The number asked for is kept with the ship's orbit, so it is kept
    /// as the ship goes through the spheres, and with the ship in its file.
    pub fn continue_prediction(&mut self, ship: GridId, now: RailTime) -> Option<usize> {
        let prediction = self.predicted_path(ship, now)?;
        let shown = prediction.rails.len() - 1;
        let mut orbit = prediction.rails.into_iter().next()?.orbit;
        let to_show = self.changes_to_show(&orbit);
        if prediction.end != PredictionEnd::AsFarAsShown
            || to_show >= self.rail_settings.most_predicted_changes
        {
            return Some(shown);
        }
        orbit.predicted = orbit.changes + to_show + 1;
        let _ = self.set_grid_rail(
            ship,
            Some(Rail {
                spin: None,
                orbit: Some(orbit),
            }),
        );
        Some(shown + 1)
    }
}
