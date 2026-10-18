//! The bytes of the messages of Eels, pinned: converting them to declared messages must not move one.

use protocol::ClientId;
use voxunami_eels::eel::Steer;
use voxunami_eels::messages::{Player, Standings, encode_standings, encode_steer};
use voxunami_eels::round::Phase;

#[test]
fn the_messages_of_eels_have_their_pinned_bytes() {
    let players = vec![
        Player {
            client: ClientId::new(1),
            slot: 0,
            alive: true,
            wins: 3,
        },
        Player {
            client: ClientId::new(9),
            slot: 2,
            alive: false,
            wins: 300,
        },
    ];
    let over = Standings {
        phase: Phase::Over {
            ticks_left: 70,
            winner: Some(2),
        },
        players: players.clone(),
    };
    let countdown = Standings {
        phase: Phase::Countdown { ticks_left: 400 },
        players,
    };
    let steers = [
        (Steer::Left, "00"),
        (Steer::Right, "01"),
        (Steer::Up, "02"),
        (Steer::Down, "03"),
        (Steer::Roll, "04"),
    ];
    let mut pins: Vec<(String, Vec<u8>, &str)> = steers
        .iter()
        .map(|&(steer, pinned)| (format!("{steer:?}"), encode_steer(steer), pinned))
        .collect();
    pins.push((
        "a finished round".into(),
        encode_standings(&over),
        "0346030201000000000103090000000200ac02",
    ));
    pins.push((
        "a countdown".into(),
        encode_standings(&countdown),
        "0190030201000000000103090000000200ac02",
    ));
    pins.push((
        "no one".into(),
        encode_standings(&Standings::default()),
        "0000",
    ));
    pins.push((
        "a running round".into(),
        encode_standings(&Standings {
            phase: Phase::Running,
            players: Vec::new(),
        }),
        "0200",
    ));
    let wrong: Vec<String> = pins
        .iter()
        .filter_map(|(name, bytes, pinned)| {
            let actual: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
            (actual != *pinned).then(|| format!("{name}: {actual}"))
        })
        .collect();
    assert!(wrong.is_empty(), "{wrong:#?}");
}
