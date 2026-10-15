use std::fmt;
use std::io::{self, Write};

use protocol::is_tick_boundary;
use transport::{Connection, ConnectionClosed};

use crate::file::ReplayWriter;

/// A connection that records every frame its owner receives, then passes it on unchanged.
///
/// Wrap the connection before the handshake, so the recording starts with the server's welcome. What
/// the owner sends is not recorded: a replay is the server's side of the conversation, which is all
/// a client needs to rebuild what it saw.
///
/// Recording is flushed at the end of every tick, so a crash loses at most the tick in progress. If
/// writing fails (a full disk, say) recording stops and the error is kept in
/// [`RecordingConnection::recording_error`]; the game itself carries on.
pub struct RecordingConnection<C: Connection, W: Write + Send> {
    inner: C,
    recording: Recording<W>,
}

enum Recording<W: Write> {
    Active(ReplayWriter<W>),
    Failed {
        error: io::Error,
        frames_recorded: u64,
    },
}

impl<C: Connection, W: Write + Send> RecordingConnection<C, W> {
    pub fn new(inner: C, output: W) -> io::Result<Self> {
        Ok(Self {
            inner,
            recording: Recording::Active(ReplayWriter::new(output)?),
        })
    }

    pub fn frames_recorded(&self) -> u64 {
        match &self.recording {
            Recording::Active(writer) => writer.frames_written(),
            Recording::Failed {
                frames_recorded, ..
            } => *frames_recorded,
        }
    }

    /// The error that stopped the recording, if one did.
    pub fn recording_error(&self) -> Option<&io::Error> {
        match &self.recording {
            Recording::Active(_) => None,
            Recording::Failed { error, .. } => Some(error),
        }
    }

    fn record(&mut self, frame: &[u8]) {
        let Recording::Active(writer) = &mut self.recording else {
            return;
        };
        let outcome = writer.append(frame).and_then(|()| {
            if is_tick_boundary(frame) {
                writer.flush()
            } else {
                Ok(())
            }
        });
        if let Err(error) = outcome {
            self.recording = Recording::Failed {
                error,
                frames_recorded: writer.frames_written(),
            };
        }
    }
}

impl<C: Connection, W: Write + Send> fmt::Debug for RecordingConnection<C, W> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RecordingConnection")
            .field("inner", &self.inner)
            .field("frames_recorded", &self.frames_recorded())
            .field("recording_error", &self.recording_error())
            .finish()
    }
}

impl<C: Connection, W: Write + Send> Connection for RecordingConnection<C, W> {
    fn send(&mut self, frame: Vec<u8>) -> Result<(), ConnectionClosed> {
        self.inner.send(frame)
    }

    fn receive(&mut self) -> Result<Option<Vec<u8>>, ConnectionClosed> {
        let received = self.inner.receive()?;
        if let Some(frame) = &received {
            self.record(frame);
        }
        Ok(received)
    }
}

impl<W: Write> Drop for Recording<W> {
    fn drop(&mut self) {
        if let Self::Active(writer) = self {
            let _ = writer.flush();
        }
    }
}
