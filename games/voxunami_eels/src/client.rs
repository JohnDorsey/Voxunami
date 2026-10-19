//! The client half of the Eels plugin: the standings the server last said, what the keys ask of the eel,
//! and what the title says.

use std::sync::{Arc, Mutex, MutexGuard};

use game_shell::client_plugin::{ActionSpec, ClientPlugin, ClientRegistrar};
use namespaced_id::NamespacedId;

use crate::eel::Steer;
use crate::game::{STEER_KEYS, describe};
use crate::messages::Standings;

/// The client half of `voxunami_eels:rules`.
#[derive(Clone, Debug, Default)]
pub struct ClientEels {
    standings: Arc<Mutex<Standings>>,
}

fn locked(standings: &Mutex<Standings>) -> MutexGuard<'_, Standings> {
    standings
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl ClientEels {
    /// How the rounds stand, as the server last said.
    pub fn standings(&self) -> Standings {
        locked(&self.standings).clone()
    }
}

/// What a steer is called as an action, and what it does.
fn action_for(steer: Steer) -> ActionSpec {
    match steer {
        Steer::Left => ActionSpec::new("voxunami_eels:steer_left", "turns the eel left"),
        Steer::Right => ActionSpec::new("voxunami_eels:steer_right", "turns the eel right"),
        Steer::Up => ActionSpec::new("voxunami_eels:steer_up", "turns the eel up"),
        Steer::Down => ActionSpec::new("voxunami_eels:steer_down", "turns the eel down"),
        Steer::Roll => ActionSpec::new("voxunami_eels:roll", "rolls the eel a quarter turn"),
    }
}

impl ClientPlugin for ClientEels {
    fn id(&self) -> NamespacedId {
        "voxunami_eels:rules"
            .parse()
            .expect("constant identifier is valid")
    }

    fn register(&self, registrar: &mut ClientRegistrar<'_>) {
        let standings = Arc::clone(&self.standings);
        registrar.on_message(move |latest: Standings| *locked(&standings) = latest);

        for (steer, keys) in STEER_KEYS {
            registrar.keys(keys, action_for(steer), move |context| {
                context.send_message(&steer);
            });
        }

        let standings = Arc::clone(&self.standings);
        registrar
            .hud_line(move |context| Some(describe(&locked(&standings), context.own_client())));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_steer_has_keys_and_no_key_steers_two_ways() {
        let mut seen = Vec::new();
        for (steer, keys) in STEER_KEYS {
            assert!(!keys.is_empty(), "{steer:?} has no key");
            for key in keys {
                assert!(!seen.contains(key), "{key:?} is bound twice");
                seen.push(*key);
            }
        }
        let steers: Vec<Steer> = STEER_KEYS.iter().map(|(steer, _)| *steer).collect();
        assert_eq!(steers, Steer::ALL);
    }
}
