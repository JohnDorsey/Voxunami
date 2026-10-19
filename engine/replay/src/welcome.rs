use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use protocol::{ServerMessage, Welcome};

use crate::file::{ReplayError, ReplayReader};

/// The server's welcome, which every recording starts with (after the challenge, in one made at protocol version 4): it names the game that made the
/// recording and describes the world, so a launcher can pick the program that can play it.
pub fn read_welcome(path: &Path) -> Result<Welcome, ReplayError> {
    let mut reader = ReplayReader::new(BufReader::new(File::open(path)?))?;
    loop {
        match reader
            .next_frame()?
            .map(|frame| ServerMessage::decode(&frame))
        {
            Some(Ok(ServerMessage::Welcome(welcome))) => return Ok(welcome),
            // A recording of a server that challenges its players starts with the challenge.
            Some(Ok(ServerMessage::Challenge { .. })) => {}
            _ => return Err(ReplayError::NoWelcome),
        }
    }
}
