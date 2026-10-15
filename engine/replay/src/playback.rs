use std::io::Read;

use transport::{Connection, ConnectionClosed};

use crate::file::{ReplayError, ReplayReader};

/// Why a replay stopped delivering frames.
#[derive(Debug)]
pub enum ReplayEnd {
    /// Frames are still being delivered.
    Playing,
    /// Every frame was delivered and the file ended cleanly.
    Complete,
    /// The file ended in the middle of a frame, as a crashed recording does. Everything before it
    /// was delivered.
    Interrupted,
    /// A frame was damaged or the file could not be read. Everything before it was delivered.
    Failed(ReplayError),
}

/// A server that plays a recording: it answers a client's requests with nothing and hands out the
/// recorded frames in order, so the very same client code that follows a live server follows a
/// replay.
///
/// It delivers a frame whenever asked, so pacing playback is up to the caller: apply one tick per
/// `1 / ticks_per_second` seconds for real time, or as fast as possible for processing a recording.
pub struct ReplayConnection<R: Read + Send> {
    reader: ReplayReader<R>,
    end: ReplayEnd,
}

impl<R: Read + Send> std::fmt::Debug for ReplayConnection<R> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReplayConnection")
            .field("frames_delivered", &self.frames_delivered())
            .field("end", &self.end)
            .finish()
    }
}

impl<R: Read + Send> ReplayConnection<R> {
    pub fn new(input: R) -> Result<Self, ReplayError> {
        Ok(Self {
            reader: ReplayReader::new(input)?,
            end: ReplayEnd::Playing,
        })
    }

    /// Whether the replay is still playing, and if not, why it stopped.
    pub fn end(&self) -> &ReplayEnd {
        &self.end
    }

    pub fn frames_delivered(&self) -> u64 {
        self.reader.frames_read()
    }
}

impl<R: Read + Send> Connection for ReplayConnection<R> {
    /// A replay has no one to listen: requests are dropped.
    fn send(&mut self, _frame: Vec<u8>) -> Result<(), ConnectionClosed> {
        Ok(())
    }

    fn receive(&mut self) -> Result<Option<Vec<u8>>, ConnectionClosed> {
        if !matches!(self.end, ReplayEnd::Playing) {
            return Err(ConnectionClosed);
        }
        match self.reader.next_frame() {
            Ok(Some(frame)) => Ok(Some(frame)),
            Ok(None) => {
                self.end = ReplayEnd::Complete;
                Err(ConnectionClosed)
            }
            Err(ReplayError::Truncated { .. }) => {
                self.end = ReplayEnd::Interrupted;
                Err(ConnectionClosed)
            }
            Err(error) => {
                self.end = ReplayEnd::Failed(error);
                Err(ConnectionClosed)
            }
        }
    }
}
