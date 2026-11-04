//! What the host of a server decides about the paths of ships on rails (the owner's settings of 2026-11-03, built one with each piece of code that obeys it).
//!
//! **They limit predictions and widths, never travel**: the server never limits how many spheres of influence a ship goes through and never takes a ship off its rail because of one of them.

use wide_float::Precision;

/// Why a number of bits is not a setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RailSettingsError {
    #[error("a rail is from {least} to {most} bits wide")]
    NotAWidth { least: u64, most: u64 },
    #[error("a prediction shows the rail the ship is on, so at least one rail")]
    NoRailShown,
}

/// How the host of a server wants the paths of its ships kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RailSettings {
    /// The widest number, in bits, that an orbit made by a hand-over from one sphere of influence to the next may have. Each hand-over adds
    /// `world_storage::rail::BITS_ADDED_BY_A_HAND_OVER` bits to the width of the orbit it makes, so that a path through many spheres is held as exactly as the spheres before it need,
    /// up to this: the cost of a ship's path grows with its width, and this is what a host pays at most. At least the 128 bits a rail has.
    pub widest_rail_bits: u64,
    /// The most consecutive sphere-of-influence changes a player may ask a prediction to show at once: they press "continue prediction" until this many are shown, and then it stops
    /// calculating. It limits what is shown and calculated, never where a ship goes: a ship goes through as many spheres as its path has.
    pub most_predicted_changes: u32,
    /// The fewest rails a prediction shows, counting the one the ship is on (so one less than that many changes ahead): when the ship's motion brings the end of what was asked for down to
    /// this, more of the path is added to what is shown, by itself. Never beyond `most_predicted_changes`.
    pub fewest_rails_shown: u32,
}

impl RailSettings {
    /// 65,536 bits: a path through some thousands of spheres is held to all the bits it has gained.
    pub const DEFAULT_WIDEST_RAIL_BITS: u64 = 65_536;

    /// 100 changes of sphere may be shown at once.
    pub const DEFAULT_MOST_PREDICTED_CHANGES: u32 = 100;

    /// Three rails are always shown: the one the ship is on and the next two.
    pub const DEFAULT_FEWEST_RAILS_SHOWN: u32 = 3;

    /// These settings, if they are possible: a width from the 128 bits a rail has up to the widest number there is, and at least one rail shown (the one the ship is on).
    pub fn new(
        widest_rail_bits: u64,
        most_predicted_changes: u32,
        fewest_rails_shown: u32,
    ) -> Result<Self, RailSettingsError> {
        let least = Precision::MINIMUM.bits();
        let most = Precision::MAXIMUM.bits();
        if !(least..=most).contains(&widest_rail_bits) {
            return Err(RailSettingsError::NotAWidth { least, most });
        }
        if fewest_rails_shown == 0 {
            return Err(RailSettingsError::NoRailShown);
        }
        Ok(Self {
            widest_rail_bits,
            most_predicted_changes,
            fewest_rails_shown,
        })
    }

    /// The same settings with this widest rail.
    pub fn with_widest_rail_bits(self, bits: u64) -> Result<Self, RailSettingsError> {
        Self::new(bits, self.most_predicted_changes, self.fewest_rails_shown)
    }

    /// The same settings with this most changes shown at once.
    #[must_use]
    pub fn with_most_predicted_changes(self, changes: u32) -> Self {
        Self {
            most_predicted_changes: changes,
            ..self
        }
    }

    /// The same settings with this fewest rails shown (at least one).
    pub fn with_fewest_rails_shown(self, rails: u32) -> Result<Self, RailSettingsError> {
        Self::new(self.widest_rail_bits, self.most_predicted_changes, rails)
    }
}

impl Default for RailSettings {
    fn default() -> Self {
        Self {
            widest_rail_bits: Self::DEFAULT_WIDEST_RAIL_BITS,
            most_predicted_changes: Self::DEFAULT_MOST_PREDICTED_CHANGES,
            fewest_rails_shown: Self::DEFAULT_FEWEST_RAILS_SHOWN,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_65536_bits_100_changes_and_3_rails_and_a_width_is_from_128_to_the_widest_number()
     {
        let defaults = RailSettings::default();
        assert_eq!(defaults.widest_rail_bits, 65_536);
        assert_eq!(defaults.most_predicted_changes, 100);
        assert_eq!(defaults.fewest_rails_shown, 3);
        assert_eq!(RailSettings::new(65_536, 100, 3), Ok(defaults));
        assert_eq!(RailSettings::new(128, 0, 1).unwrap().widest_rail_bits, 128);
        let most = Precision::MAXIMUM.bits();
        assert!(RailSettings::new(most, 100, 3).is_ok());
        for bits in [0, 1, 127, most + 1, u64::MAX] {
            assert_eq!(
                RailSettings::new(bits, 100, 3),
                Err(RailSettingsError::NotAWidth { least: 128, most }),
                "{bits}"
            );
        }
    }

    #[test]
    fn at_least_one_rail_is_shown_and_the_most_changes_may_be_none() {
        assert_eq!(
            RailSettings::new(128, 100, 0),
            Err(RailSettingsError::NoRailShown)
        );
        assert_eq!(
            RailSettings::default().with_fewest_rails_shown(0),
            Err(RailSettingsError::NoRailShown)
        );
        assert_eq!(
            RailSettings::default()
                .with_fewest_rails_shown(5)
                .unwrap()
                .fewest_rails_shown,
            5
        );
        assert_eq!(
            RailSettings::default()
                .with_most_predicted_changes(0)
                .most_predicted_changes,
            0
        );
        assert_eq!(
            RailSettings::default()
                .with_widest_rail_bits(256)
                .unwrap()
                .widest_rail_bits,
            256
        );
        assert!(RailSettings::default().with_widest_rail_bits(64).is_err());
    }
}
