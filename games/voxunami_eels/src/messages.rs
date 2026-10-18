//! The game's own messages, carried inside the engine's generic game messages.

use binary_codec::{ByteReader, ByteWriter, DecodeError};
use namespaced_id::NamespacedId;
use protocol::{ClientId, Message};

use crate::eel::Steer;
use crate::round::{MAXIMUM_EELS, Phase};

fn kind(text: &str) -> NamespacedId {
    text.parse().expect("constant identifier is valid")
}

/// Client to server: steer the player's eel.
impl Message for Steer {
    fn kind() -> NamespacedId {
        kind("voxunami_eels:steer")
    }

    fn encode(&self) -> Vec<u8> {
        vec![self.index()]
    }

    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = ByteReader::new(payload);
        let steer =
            Steer::from_index(reader.read_u8()?).ok_or(DecodeError::Invalid("no such steer"))?;
        reader.finish()?;
        Ok(steer)
    }

    fn samples() -> Vec<Self> {
        Steer::ALL.to_vec()
    }
}

/// One player in the standings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Player {
    pub client: ClientId,
    /// The player's colour, from 0.
    pub slot: u8,
    /// Whether their eel is swimming.
    pub alive: bool,
    /// Rounds won.
    pub wins: u32,
}

/// Where the rounds stand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Standings {
    pub phase: Phase,
    pub players: Vec<Player>,
}

impl Default for Standings {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            players: Vec::new(),
        }
    }
}

fn write_phase(writer: &mut ByteWriter, phase: Phase) {
    match phase {
        Phase::Idle => writer.write_u8(0),
        Phase::Countdown { ticks_left } => {
            writer.write_u8(1);
            writer.write_varint(u64::from(ticks_left));
        }
        Phase::Running => writer.write_u8(2),
        Phase::Over { ticks_left, winner } => {
            writer.write_u8(3);
            writer.write_varint(u64::from(ticks_left));
            // 0 for no winner, else the winner's slot plus one.
            writer.write_u8(winner.map_or(0, |slot| slot + 1));
        }
    }
}

fn read_ticks(reader: &mut ByteReader<'_>) -> Result<u32, DecodeError> {
    u32::try_from(reader.read_varint()?).map_err(|_| DecodeError::Invalid("too many ticks"))
}

fn read_phase(reader: &mut ByteReader<'_>) -> Result<Phase, DecodeError> {
    Ok(match reader.read_u8()? {
        0 => Phase::Idle,
        1 => Phase::Countdown {
            ticks_left: read_ticks(reader)?,
        },
        2 => Phase::Running,
        3 => {
            let ticks_left = read_ticks(reader)?;
            let winner = match reader.read_u8()? {
                0 => None,
                slot if usize::from(slot) <= MAXIMUM_EELS => Some(slot - 1),
                _ => return Err(DecodeError::Invalid("no such slot")),
            };
            Phase::Over { ticks_left, winner }
        }
        _ => return Err(DecodeError::Invalid("no such phase")),
    })
}

impl Message for Standings {
    fn kind() -> NamespacedId {
        kind("voxunami_eels:standings")
    }

    fn encode(&self) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        write_phase(&mut writer, self.phase);
        writer.write_varint(self.players.len() as u64);
        for player in &self.players {
            writer.write_u32(player.client.get());
            writer.write_u8(player.slot);
            writer.write_u8(u8::from(player.alive));
            writer.write_varint(u64::from(player.wins));
        }
        writer.into_bytes()
    }

    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = ByteReader::new(payload);
        let phase = read_phase(&mut reader)?;
        let count = reader.read_count(7)?;
        let mut players = Vec::with_capacity(count.min(MAXIMUM_EELS));
        for _ in 0..count {
            let client = ClientId::new(reader.read_u32()?);
            let slot = reader.read_u8()?;
            if usize::from(slot) >= MAXIMUM_EELS {
                return Err(DecodeError::Invalid("no such slot"));
            }
            let alive = match reader.read_u8()? {
                0 => false,
                1 => true,
                _ => return Err(DecodeError::Invalid("not a yes or a no")),
            };
            let wins = u32::try_from(reader.read_varint()?)
                .map_err(|_| DecodeError::Invalid("too many wins"))?;
            players.push(Player {
                client,
                slot,
                alive,
                wins,
            });
        }
        reader.finish()?;
        Ok(Self { phase, players })
    }

    fn samples() -> Vec<Self> {
        let players = vec![
            Player {
                client: ClientId::new(1),
                slot: 0,
                alive: true,
                wins: 3,
            },
            Player {
                client: ClientId::new(900),
                slot: 7,
                alive: false,
                wins: 100_000,
            },
        ];
        let mut samples = vec![Self::default()];
        for phase in [
            Phase::Idle,
            Phase::Countdown { ticks_left: 400 },
            Phase::Running,
            Phase::Over {
                ticks_left: 0,
                winner: None,
            },
            Phase::Over {
                ticks_left: 70,
                winner: Some(7),
            },
        ] {
            samples.push(Self {
                phase,
                players: players.clone(),
            });
        }
        samples
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_declared_messages_pass_the_generated_check() {
        assert_eq!(protocol::check_message::<Steer>(), Ok(()));
        assert_eq!(protocol::check_message::<Standings>(), Ok(()));
    }

    #[test]
    fn every_steer_survives_encoding_and_nothing_else_is_accepted() {
        for steer in Steer::ALL {
            assert_eq!(Steer::decode(&steer.encode()).unwrap(), steer);
        }
        assert!(Steer::decode(&[]).is_err());
        assert!(Steer::decode(&[9]).is_err());
        assert!(Steer::decode(&[0, 0]).is_err());
    }

    fn sample() -> Standings {
        Standings {
            phase: Phase::Over {
                ticks_left: 77,
                winner: Some(3),
            },
            players: vec![
                Player {
                    client: ClientId::new(4),
                    slot: 0,
                    alive: false,
                    wins: 2,
                },
                Player {
                    client: ClientId::new(900),
                    slot: 7,
                    alive: true,
                    wins: 100_000,
                },
            ],
        }
    }

    #[test]
    fn standings_survive_encoding_in_every_phase() {
        for phase in [
            Phase::Idle,
            Phase::Countdown { ticks_left: 90 },
            Phase::Running,
            Phase::Over {
                ticks_left: 0,
                winner: None,
            },
            Phase::Over {
                ticks_left: 90,
                winner: Some(7),
            },
        ] {
            let standings = Standings { phase, ..sample() };
            assert_eq!(Standings::decode(&standings.encode()).unwrap(), standings);
        }
        let empty = Standings::default();
        assert_eq!(Standings::decode(&empty.encode()).unwrap(), empty);
    }

    #[test]
    fn damaged_standings_are_errors_and_never_panics() {
        let good = sample().encode();
        for length in 0..good.len() {
            assert!(
                Standings::decode(&good[..length]).is_err(),
                "cut at {length}"
            );
        }
        let mut extra = good.clone();
        extra.push(0);
        assert!(Standings::decode(&extra).is_err());
        // A slot that does not exist, a phase that does not exist, a yes-or-no that is neither.
        let mut bad_slot = Standings {
            phase: Phase::Running,
            players: vec![Player {
                client: ClientId::new(1),
                slot: 0,
                alive: true,
                wins: 0,
            }],
        }
        .encode();
        let slot_position = bad_slot.len() - 3;
        bad_slot[slot_position] = 8;
        assert!(Standings::decode(&bad_slot).is_err());
        assert!(Standings::decode(&[9, 0]).is_err());
        let mut bad_flag = Standings {
            phase: Phase::Running,
            players: vec![Player {
                client: ClientId::new(1),
                slot: 0,
                alive: true,
                wins: 0,
            }],
        }
        .encode();
        let flag_position = bad_flag.len() - 2;
        bad_flag[flag_position] = 2;
        assert!(Standings::decode(&bad_flag).is_err());
        // A winner past the last slot.
        assert!(Standings::decode(&[3, 0, 9, 0]).is_err());
    }

    #[test]
    fn arbitrary_bytes_never_panic() {
        for seed in 0..2000u32 {
            let bytes: Vec<u8> = (0..(seed % 24))
                .map(|index| {
                    (seed
                        .wrapping_mul(2_654_435_761)
                        .wrapping_add(index * 40_503)
                        >> 7) as u8
                })
                .collect();
            let _ = Standings::decode(&bytes);
            let _ = Steer::decode(&bytes);
        }
    }
}
