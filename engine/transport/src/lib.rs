//! Carrying frames between a server and its clients.
//!
//! A *frame* is one encoded protocol message, as bytes. A [`Connection`] moves frames in order and
//! without loss between two ends; it knows nothing about what they mean. Singleplayer and tests use
//! the in-process [`loopback`]; networked play will add a socket-based connection behind the same
//! interface, so the server and client code do not change.
//!
//! Connections never block. A program that handles many of them polls each in turn once per tick.

pub mod loopback;

use std::fmt::Debug;

/// The other end is gone, and every frame it sent before leaving has been received.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the connection is closed")]
pub struct ConnectionClosed;

/// One end of an ordered, lossless, frame-oriented connection.
pub trait Connection: Debug + Send {
    /// Queues a frame for the other end. Fails once the other end has gone.
    fn send(&mut self, frame: Vec<u8>) -> Result<(), ConnectionClosed>;

    /// The next frame the other end sent, or `None` if none has arrived yet. After the other end
    /// goes, frames it already sent are still delivered; then this fails.
    fn receive(&mut self) -> Result<Option<Vec<u8>>, ConnectionClosed>;
}

impl Connection for Box<dyn Connection> {
    fn send(&mut self, frame: Vec<u8>) -> Result<(), ConnectionClosed> {
        (**self).send(frame)
    }

    fn receive(&mut self) -> Result<Option<Vec<u8>>, ConnectionClosed> {
        (**self).receive()
    }
}

/// Where a server finds clients that want to connect.
pub trait Listener: Debug + Send {
    /// The next client that has connected since the last call, if any. Never blocks.
    fn accept(&mut self) -> Option<Box<dyn Connection>>;
}
