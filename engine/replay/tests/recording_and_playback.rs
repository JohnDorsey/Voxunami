//! Recording a connection and playing the recording back, through the public interface only.

use std::io::{self, Cursor, Write};
use std::sync::{Arc, Mutex};

use protocol::{ServerMessage, Tick};
use replay::{
    RecordingConnection, ReplayConnection, ReplayEnd, ReplayError, ReplayReader, ReplayWriter,
};
use transport::loopback::loopback_pair;
use transport::{Connection, ConnectionClosed};

/// A writer the test can look into after handing it over, that counts flushes.
#[derive(Clone, Default)]
struct SharedBuffer {
    bytes: Arc<Mutex<Vec<u8>>>,
    flushes: Arc<Mutex<u32>>,
}

impl Write for SharedBuffer {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.bytes.lock().unwrap().extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        *self.flushes.lock().unwrap() += 1;
        Ok(())
    }
}

/// Accepts this many bytes, then fails.
struct FullDisk {
    remaining: usize,
}

impl Write for FullDisk {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if buffer.len() > self.remaining {
            return Err(io::Error::other("disk full"));
        }
        self.remaining -= buffer.len();
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn note(payload: u8) -> Vec<u8> {
    ServerMessage::GameMessage {
        kind: "test:note".parse().unwrap(),
        payload: vec![payload],
    }
    .encode()
}

fn tick(number: u64) -> Vec<u8> {
    ServerMessage::TickCompleted {
        tick: Tick::new(number),
    }
    .encode()
}

fn recorded_frames(buffer: &SharedBuffer) -> Vec<Vec<u8>> {
    let bytes = buffer.bytes.lock().unwrap().clone();
    let mut reader = ReplayReader::new(Cursor::new(bytes)).unwrap();
    let mut frames = Vec::new();
    while let Some(frame) = reader.next_frame().unwrap() {
        frames.push(frame);
    }
    frames
}

fn replay_bytes(frames: &[Vec<u8>]) -> Vec<u8> {
    let mut writer = ReplayWriter::new(Vec::new()).unwrap();
    for frame in frames {
        writer.append(frame).unwrap();
    }
    writer.into_inner()
}

fn drain(connection: &mut ReplayConnection<Cursor<Vec<u8>>>) -> Vec<Vec<u8>> {
    let mut delivered = Vec::new();
    while let Ok(Some(frame)) = connection.receive() {
        delivered.push(frame);
    }
    delivered
}

#[test]
fn what_the_owner_receives_is_recorded_and_passed_on_unchanged() {
    let (mut server, client_end) = loopback_pair();
    let buffer = SharedBuffer::default();
    let mut recording = RecordingConnection::new(client_end, buffer.clone()).unwrap();
    let frames = [note(1), note(2), tick(1), note(3), tick(2)];
    for frame in &frames {
        server.send(frame.clone()).unwrap();
    }
    let mut received = Vec::new();
    while let Some(frame) = recording.receive().unwrap() {
        received.push(frame);
    }
    assert_eq!(received, frames);
    assert_eq!(recorded_frames(&buffer), frames);
    assert_eq!(recording.frames_recorded(), 5);
    assert!(recording.recording_error().is_none());
}

#[test]
fn what_the_owner_sends_goes_through_and_is_not_recorded() {
    let (mut server, client_end) = loopback_pair();
    let buffer = SharedBuffer::default();
    let mut recording = RecordingConnection::new(client_end, buffer.clone()).unwrap();
    recording.send(vec![1, 2, 3]).unwrap();
    assert_eq!(server.receive(), Ok(Some(vec![1, 2, 3])));
    assert!(recorded_frames(&buffer).is_empty());
    drop(server);
    assert_eq!(recording.send(vec![4]), Err(ConnectionClosed));
    assert_eq!(recording.receive(), Err(ConnectionClosed));
}

#[test]
fn the_recording_is_flushed_at_the_end_of_each_tick_and_not_between() {
    let (mut server, client_end) = loopback_pair();
    let buffer = SharedBuffer::default();
    let mut recording = RecordingConnection::new(client_end, buffer.clone()).unwrap();
    let flushes = || *buffer.flushes.lock().unwrap();
    for frame in [note(1), note(2)] {
        server.send(frame).unwrap();
        recording.receive().unwrap();
    }
    assert_eq!(flushes(), 0);
    server.send(tick(1)).unwrap();
    recording.receive().unwrap();
    assert_eq!(flushes(), 1);
    server.send(note(3)).unwrap();
    recording.receive().unwrap();
    assert_eq!(flushes(), 1);
}

#[test]
fn when_writing_fails_the_connection_keeps_working_and_the_error_is_kept() {
    let (mut server, client_end) = loopback_pair();
    // Room for the header and the first frame only: 12 header bytes, then a one-byte length, a
    // four-byte checksum and the frame.
    let first = note(1);
    let room = 12 + 1 + 4 + first.len();
    let mut recording = RecordingConnection::new(client_end, FullDisk { remaining: room }).unwrap();
    for frame in [first.clone(), note(2), tick(1)] {
        server.send(frame).unwrap();
    }
    assert_eq!(recording.receive(), Ok(Some(first)));
    assert!(recording.recording_error().is_none());
    assert_eq!(recording.receive(), Ok(Some(note(2))));
    assert!(recording.recording_error().is_some());
    assert_eq!(recording.frames_recorded(), 1);
    // The game still receives everything.
    assert_eq!(recording.receive(), Ok(Some(tick(1))));
    assert_eq!(recording.frames_recorded(), 1);
}

fn sample_frames() -> Vec<Vec<u8>> {
    vec![vec![1], vec![2, 2], vec![3, 3, 3]]
}

#[test]
fn frames_are_played_in_order_and_then_the_connection_closes() {
    let mut replay = ReplayConnection::new(Cursor::new(replay_bytes(&sample_frames()))).unwrap();
    assert!(matches!(replay.end(), ReplayEnd::Playing));
    assert_eq!(replay.receive(), Ok(Some(vec![1])));
    assert_eq!(replay.frames_delivered(), 1);
    assert_eq!(replay.receive(), Ok(Some(vec![2, 2])));
    assert_eq!(replay.receive(), Ok(Some(vec![3, 3, 3])));
    assert_eq!(replay.receive(), Err(ConnectionClosed));
    assert!(matches!(replay.end(), ReplayEnd::Complete));
    assert_eq!(replay.receive(), Err(ConnectionClosed));
}

#[test]
fn requests_to_a_replay_are_accepted_and_ignored() {
    let mut replay = ReplayConnection::new(Cursor::new(replay_bytes(&sample_frames()))).unwrap();
    assert_eq!(replay.send(vec![9, 9]), Ok(()));
    assert_eq!(replay.receive(), Ok(Some(vec![1])));
}

#[test]
fn an_interrupted_recording_plays_to_its_last_complete_frame_and_says_so() {
    let bytes = replay_bytes(&sample_frames());
    let mut replay = ReplayConnection::new(Cursor::new(bytes[..bytes.len() - 1].to_vec())).unwrap();
    assert_eq!(drain(&mut replay), sample_frames()[..2]);
    assert!(matches!(replay.end(), ReplayEnd::Interrupted));
}

#[test]
fn a_damaged_recording_plays_to_the_damage_and_says_so() {
    let mut bytes = replay_bytes(&sample_frames());
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF;
    let mut replay = ReplayConnection::new(Cursor::new(bytes)).unwrap();
    assert_eq!(drain(&mut replay), sample_frames()[..2]);
    assert!(matches!(
        replay.end(),
        ReplayEnd::Failed(ReplayError::Damaged { record: 2 })
    ));
}

#[test]
fn something_that_is_not_a_replay_does_not_open() {
    assert!(matches!(
        ReplayConnection::new(Cursor::new(vec![0u8; 40])),
        Err(ReplayError::NotAReplay)
    ));
}

#[test]
fn a_recording_names_its_game_in_the_welcome_it_starts_with() {
    use protocol::{ClientId, PROTOCOL_VERSION, Welcome};

    let welcome = ServerMessage::Welcome(Welcome {
        protocol_version: PROTOCOL_VERSION,
        game: "some_game:game".parse().unwrap(),
        client: ClientId::new(1),
        ticks_per_second: 20,
        tick: Tick::ZERO,
        world_description: vec![1, 2, 3],
    });
    let directory =
        std::env::temp_dir().join(format!("voxunami_replay_test_{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let write = |name: &str, frames: &[Vec<u8>]| {
        let path = directory.join(name);
        std::fs::write(&path, replay_bytes(frames)).unwrap();
        path
    };
    let good = write("good.voxreplay", &[welcome.encode(), tick(1)]);
    let found = replay::read_welcome(&good).unwrap();
    assert_eq!(found.game.as_str(), "some_game:game");
    assert_eq!(found.world_description, [1, 2, 3]);

    for (name, frames) in [
        ("empty.voxreplay", vec![]),
        ("tick_first.voxreplay", vec![tick(1)]),
        ("garbage_first.voxreplay", vec![vec![250, 1]]),
    ] {
        let path = write(name, &frames);
        assert!(
            matches!(replay::read_welcome(&path), Err(ReplayError::NoWelcome)),
            "{name}"
        );
    }
    assert!(replay::read_welcome(&directory.join("missing.voxreplay")).is_err());
    std::fs::remove_dir_all(&directory).unwrap();
}
