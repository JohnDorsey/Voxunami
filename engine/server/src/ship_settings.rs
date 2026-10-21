//! What the host of a server decides about its ships: how near a player has to be for a ship to move, how far off a ship has to be to be put
//! in a file, and what happens to a ship when its pilot logs out.

/// What happens to a ship when its pilot logs out, as the host of a server has it (a ship's own wish is `world_storage::PilotLogout`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LogoutPolicy {
    /// Each ship does what it says it does.
    #[default]
    Chair,
    /// Ships stay where they are, whatever they say.
    AlwaysStay,
    /// Ships go away with their pilots, whatever they say.
    AlwaysVanish,
}

impl LogoutPolicy {
    /// The name the host's flag gives it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Chair => "chair",
            Self::AlwaysStay => "always-stay",
            Self::AlwaysVanish => "always-vanish",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Chair, Self::AlwaysStay, Self::AlwaysVanish]
            .into_iter()
            .find(|policy| policy.name() == name)
    }
}

/// Why a pair of distances is not a setting.
#[derive(Clone, Copy, Debug, PartialEq, thiserror::Error)]
pub enum ShipSettingsError {
    #[error("a distance is a number of voxels that is not below zero")]
    NotADistance,
    #[error(
        "ships are put away from {unload} voxels, but they move up to {simulation}: they must be put away farther off than they stop moving"
    )]
    UnloadBelowSimulation { simulation: f64, unload: f64 },
}

/// How the host of a server wants its ships kept. Distances are in voxels (a metre, at the scale a game is played at), and are measured to the nearest
/// brick of a ship from a player in its level.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShipSettings {
    /// A ship moves, runs its rules and is edited while a player is this near. Farther off it is frozen: it is where it is, asleep, and
    /// its rules wait.
    pub simulation_distance: f64,
    /// A frozen ship that is this far from every player is put in a file and out of memory, until a player comes within the simulation
    /// distance. Never less than the simulation distance.
    pub unload_distance: f64,
    /// What a pilot's logout does to their ship.
    pub logout_policy: LogoutPolicy,
}

impl ShipSettings {
    pub const DEFAULT_SIMULATION_DISTANCE: f64 = 256.0;
    pub const DEFAULT_UNLOAD_DISTANCE: f64 = 1024.0;

    /// A ship that is within a tenth more than the simulation distance of a player that left it keeps moving, and one that is within a
    /// tenth more than the unload distance is kept: so a ship at the edge does not freeze and move again every tick.
    pub const HYSTERESIS: f64 = 1.1;

    pub fn new(simulation_distance: f64, unload_distance: f64) -> Result<Self, ShipSettingsError> {
        let distance = |value: f64| !value.is_nan() && value >= 0.0;
        if !distance(simulation_distance) || !distance(unload_distance) {
            return Err(ShipSettingsError::NotADistance);
        }
        if unload_distance < simulation_distance {
            return Err(ShipSettingsError::UnloadBelowSimulation {
                simulation: simulation_distance,
                unload: unload_distance,
            });
        }
        Ok(Self {
            simulation_distance,
            unload_distance,
            logout_policy: LogoutPolicy::default(),
        })
    }

    /// The same, with this policy for a pilot's logout.
    pub fn with_logout_policy(mut self, logout_policy: LogoutPolicy) -> Self {
        self.logout_policy = logout_policy;
        self
    }

    /// Ships move wherever they are, and none is ever put in a file by distance: for a game that has no use for either, and for tests of
    /// what ships do that have no players near them.
    pub fn everywhere() -> Self {
        Self {
            simulation_distance: f64::INFINITY,
            unload_distance: f64::INFINITY,
            logout_policy: LogoutPolicy::default(),
        }
    }
}

impl Default for ShipSettings {
    fn default() -> Self {
        Self {
            simulation_distance: Self::DEFAULT_SIMULATION_DISTANCE,
            unload_distance: Self::DEFAULT_UNLOAD_DISTANCE,
            logout_policy: LogoutPolicy::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_the_ones_the_owner_chose() {
        let defaults = ShipSettings::default();
        assert_eq!(defaults.simulation_distance, 256.0);
        assert_eq!(defaults.unload_distance, 1024.0);
        assert_eq!(defaults.logout_policy, LogoutPolicy::Chair);
        assert_eq!(ShipSettings::new(256.0, 1024.0), Ok(defaults));
    }

    #[test]
    fn a_server_refuses_to_put_ships_away_before_it_stops_moving_them() {
        assert!(ShipSettings::new(100.0, 100.0).is_ok());
        assert_eq!(
            ShipSettings::new(100.0, 99.0),
            Err(ShipSettingsError::UnloadBelowSimulation {
                simulation: 100.0,
                unload: 99.0
            })
        );
        for (simulation, unload) in [
            (-1.0, 10.0),
            (1.0, -10.0),
            (f64::NAN, 10.0),
            (1.0, f64::NAN),
        ] {
            assert_eq!(
                ShipSettings::new(simulation, unload),
                Err(ShipSettingsError::NotADistance),
                "{simulation} {unload}"
            );
        }
        // No distance at all is fine, and so is a distance that is no limit.
        assert!(ShipSettings::new(0.0, 0.0).is_ok());
        assert!(ShipSettings::new(10.0, f64::INFINITY).is_ok());
        assert!(ShipSettings::new(f64::INFINITY, f64::INFINITY).is_ok());
        assert_eq!(
            ShipSettings::new(f64::INFINITY, 10.0),
            Err(ShipSettingsError::UnloadBelowSimulation {
                simulation: f64::INFINITY,
                unload: 10.0
            })
        );
    }

    #[test]
    fn everywhere_has_no_limit_and_the_policies_have_names_that_read_back() {
        let everywhere = ShipSettings::everywhere();
        assert!(
            everywhere.simulation_distance.is_infinite()
                && everywhere.unload_distance.is_infinite()
        );
        for policy in [
            LogoutPolicy::Chair,
            LogoutPolicy::AlwaysStay,
            LogoutPolicy::AlwaysVanish,
        ] {
            assert_eq!(LogoutPolicy::from_name(policy.name()), Some(policy));
        }
        assert_eq!(LogoutPolicy::from_name("sometimes"), None);
        assert_eq!(
            ShipSettings::default()
                .with_logout_policy(LogoutPolicy::AlwaysVanish)
                .logout_policy,
            LogoutPolicy::AlwaysVanish
        );
    }
}
