//! Voxunami Eels as the shell runs it: what the player's keys do, and what the title says.

use game_shell::session::Session;
use game_shell::{CameraStart, GameDefinition, KeyCode};
use namespaced_id::NamespacedId;
use player_presence::PlayersPlugin;
use plugin_host::PluginHost;
use protocol::ClientId;
use server::GameLogic;
use transport::Connection;
use world_storage::WorldData;

use crate::content::{ContentEels, SLOT_NAMES, game_id};
use crate::eel::Steer;
use crate::logic::PluginEels;
use crate::messages::{Standings, decode_standings, encode_steer, standings_kind, steer_kind};
use crate::round::{MAXIMUM_EELS, Phase, TICKS_PER_SECOND};

/// What the Eels client remembers: how the rounds stand, as the server last said.
#[derive(Debug, Default)]
pub struct ClientStateEels {
    pub standings: Standings,
}

#[derive(Debug)]
pub struct Eels;

pub type SessionEels<C> = Session<C, Eels>;

/// What a key asks of the eel, if anything.
pub fn steer_for(key: KeyCode) -> Option<Steer> {
    match key {
        KeyCode::KeyA | KeyCode::ArrowLeft => Some(Steer::Left),
        KeyCode::KeyD | KeyCode::ArrowRight => Some(Steer::Right),
        KeyCode::KeyW | KeyCode::ArrowUp => Some(Steer::Up),
        KeyCode::KeyS | KeyCode::ArrowDown => Some(Steer::Down),
        KeyCode::Space | KeyCode::KeyQ | KeyCode::KeyE => Some(Steer::Roll),
        _ => None,
    }
}

/// Asks the server to steer the player's eel.
pub fn steer<C: Connection>(session: &mut SessionEels<C>, steer: Steer) {
    session.send_game_message(steer_kind(), encode_steer(steer));
}

/// The colour's name for a slot.
fn colour_name(slot: u8) -> &'static str {
    SLOT_NAMES[usize::from(slot) % MAXIMUM_EELS]
}

/// What the title says about the round, from the standings and which client this is.
pub fn describe(standings: &Standings, own: ClientId) -> String {
    let me = standings.players.iter().find(|player| player.client == own);
    let seconds = |ticks_left: u32| ticks_left.div_ceil(TICKS_PER_SECOND);
    let round = match standings.phase {
        Phase::Idle => "waiting for players".to_owned(),
        Phase::Countdown { ticks_left } => format!("starting in {}", seconds(ticks_left)),
        Phase::Running => {
            let alive = standings
                .players
                .iter()
                .filter(|player| player.alive)
                .count();
            format!("swimming, {alive} of {} alive", standings.players.len())
        }
        Phase::Over { winner, .. } => match winner {
            Some(slot) => format!("{} eel wins the round", colour_name(slot)),
            None => "nobody wins the round".to_owned(),
        },
    };
    match me {
        Some(player) => format!(
            "{} eel{} | wins {} | {round}",
            colour_name(player.slot),
            if player.alive { "" } else { " (out)" },
            player.wins
        ),
        None => format!("watching | {round}"),
    }
}

impl GameDefinition for Eels {
    type Content = ContentEels;
    type ClientState = ClientStateEels;

    fn id() -> NamespacedId {
        game_id()
    }

    fn title() -> &'static str {
        "Voxunami Eels"
    }

    fn command() -> &'static str {
        "eels"
    }

    fn summary() -> &'static str {
        "3D Tron: flat eels race through an arena, each leaving a wall behind it. Last eel swimming wins."
    }

    fn controls() -> &'static str {
        "A and D (or the left and right arrows) turn left and right; W and S (or up and down) turn up and down; Space (or Q or E) rolls the eel a quarter turn.\nYour eel is a ribbon three cells wide and one thick. Rolling turns it from lying flat to standing on its edge, which changes what gaps it fits through and the wall it leaves.\nAn eel dies on the arena's wall, on any eel's body, or head-on with another eel. The last one swimming wins the round; a lone player plays until they die. Rounds follow one another.\nThe title shows your colour, your wins and the round."
    }

    fn default_seed() -> u64 {
        1
    }

    fn default_camera() -> CameraStart {
        CameraStart {
            position: [3.5, 60.0, 3.5],
            yaw_degrees: -135.0,
            pitch_degrees: -26.0,
        }
    }

    fn reach() -> f32 {
        0.0
    }

    fn ticks_per_second() -> u32 {
        TICKS_PER_SECOND
    }

    fn server_moves_players() -> bool {
        true
    }

    fn server_sets_view() -> bool {
        true
    }

    fn shows_other_players() -> bool {
        false
    }

    fn shows_crosshair() -> bool {
        false
    }

    fn new_content() -> ContentEels {
        ContentEels::new()
    }

    fn new_logic(content: &ContentEels, world: &WorldData) -> Box<dyn GameLogic> {
        // The server moves every player, so what they say about where they are is not believed.
        let players = PlayersPlugin::new(false);
        let rules = PluginEels::new(content, players.book());
        let host = PluginHost::new(vec![Box::new(players), Box::new(rules)], world)
            .expect("the plugins of Eels are consistent");
        Box::new(host)
    }

    fn handle_game_message(state: &mut ClientStateEels, kind: &NamespacedId, payload: &[u8]) {
        if *kind == standings_kind()
            && let Ok(standings) = decode_standings(payload)
        {
            state.standings = standings;
        }
    }

    fn key_pressed<C: Connection>(session: &mut SessionEels<C>, key: KeyCode) {
        if let Some(asked) = steer_for(key) {
            steer(session, asked);
        }
    }

    fn status_text<C: Connection>(session: &SessionEels<C>) -> String {
        let own = session.client().world().server_info().client;
        describe(&session.state.standings, own)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::Player;

    fn player(client: u32, slot: u8, alive: bool, wins: u32) -> Player {
        Player {
            client: ClientId::new(client),
            slot,
            alive,
            wins,
        }
    }

    #[test]
    fn keys_steer_and_other_keys_do_not() {
        for (key, expected) in [
            (KeyCode::KeyA, Steer::Left),
            (KeyCode::ArrowLeft, Steer::Left),
            (KeyCode::KeyD, Steer::Right),
            (KeyCode::ArrowRight, Steer::Right),
            (KeyCode::KeyW, Steer::Up),
            (KeyCode::ArrowUp, Steer::Up),
            (KeyCode::KeyS, Steer::Down),
            (KeyCode::ArrowDown, Steer::Down),
            (KeyCode::Space, Steer::Roll),
            (KeyCode::KeyQ, Steer::Roll),
            (KeyCode::KeyE, Steer::Roll),
        ] {
            assert_eq!(steer_for(key), Some(expected), "{key:?}");
        }
        for key in [
            KeyCode::KeyX,
            KeyCode::Digit1,
            KeyCode::Enter,
            KeyCode::Escape,
        ] {
            assert_eq!(steer_for(key), None, "{key:?}");
        }
    }

    #[test]
    fn the_title_says_who_you_are_and_how_the_round_stands() {
        let standings = Standings {
            phase: Phase::Running,
            players: vec![
                player(7, 3, true, 2),
                player(9, 0, false, 0),
                player(11, 1, true, 1),
            ],
        };
        assert_eq!(
            describe(&standings, ClientId::new(7)),
            "green eel | wins 2 | swimming, 2 of 3 alive"
        );
        assert_eq!(
            describe(&standings, ClientId::new(9)),
            "red eel (out) | wins 0 | swimming, 2 of 3 alive"
        );
        assert_eq!(
            describe(&standings, ClientId::new(100)),
            "watching | swimming, 2 of 3 alive"
        );
        let countdown = Standings {
            phase: Phase::Countdown { ticks_left: 61 },
            ..standings.clone()
        };
        assert!(describe(&countdown, ClientId::new(7)).ends_with("starting in 3"));
        let over = Standings {
            phase: Phase::Over {
                ticks_left: 30,
                winner: Some(1),
            },
            ..standings.clone()
        };
        assert!(describe(&over, ClientId::new(7)).ends_with("orange eel wins the round"));
        let drawn = Standings {
            phase: Phase::Over {
                ticks_left: 30,
                winner: None,
            },
            ..standings
        };
        assert!(describe(&drawn, ClientId::new(7)).ends_with("nobody wins the round"));
        assert_eq!(
            describe(&Standings::default(), ClientId::new(1)),
            "watching | waiting for players"
        );
    }
}
