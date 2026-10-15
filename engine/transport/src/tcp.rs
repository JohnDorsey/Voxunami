//! Connections over TCP, so a server and its players can be on different machines.
//!
//! On the wire each frame is its length (`u32`, little-endian) followed by its bytes. Nothing else is
//! added: no handshake, no encryption, no authentication. Use it on networks you trust; the protocol
//! layer above refuses what is malformed, but it cannot tell who is connecting.
//!
//! Like every [`Connection`], these never block. Writing queues bytes and sends what the network
//! takes; reading collects what has arrived. Both happen inside `send` and `receive`, so a program
//! that polls its connections once per tick keeps them moving. A peer that falls so far behind that
//! the queue for it fills up is treated as gone, rather than letting one slow client use unbounded
//! memory or stall everyone else.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use crate::{Connection, ConnectionClosed, Listener};

/// A frame longer than this is a protocol violation. Larger than any message the protocol sends.
pub const MAXIMUM_FRAME_LENGTH: usize = 1 << 24;

/// Bytes waiting to be sent to a peer beyond which the peer is dropped.
pub const MAXIMUM_QUEUED_BYTES: usize = 1 << 26;

/// How much one call to `receive` reads from the network at most.
const READ_BUDGET: usize = 1 << 20;

/// How long a connection being closed waits to hand its last bytes to a slow network, and then for
/// the peer to finish too.
const FAREWELL_TIMEOUT: Duration = Duration::from_millis(100);

const HEADER_LENGTH: usize = 4;

/// One end of a TCP connection.
#[derive(Debug)]
pub struct TcpConnection {
    stream: TcpStream,
    /// Bytes received that do not yet make a whole frame.
    incoming: Vec<u8>,
    /// Bytes queued to send; the front is what goes next.
    outgoing: VecDeque<u8>,
    /// The peer has closed its sending side: no more bytes will arrive.
    peer_finished: bool,
    /// Something went wrong that cannot be recovered from.
    failed: bool,
}

impl TcpConnection {
    /// Connects to a server, waiting up to `timeout` for each address it resolves to.
    pub fn connect(address: impl ToSocketAddrs, timeout: Duration) -> io::Result<Self> {
        let mut last_error = None;
        for candidate in address.to_socket_addrs()? {
            match TcpStream::connect_timeout(&candidate, timeout) {
                Ok(stream) => return Self::from_stream(stream),
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "the address names no host")
        }))
    }

    fn from_stream(stream: TcpStream) -> io::Result<Self> {
        stream.set_nodelay(true)?; // frames are small and latency matters more than packing
        stream.set_nonblocking(true)?;
        Ok(Self {
            stream,
            incoming: Vec::new(),
            outgoing: VecDeque::new(),
            peer_finished: false,
            failed: false,
        })
    }

    /// The address of the other end, if it is still known.
    pub fn peer_address(&self) -> Option<SocketAddr> {
        self.stream.peer_addr().ok()
    }

    /// Sends as much of the queue as the network takes right now.
    fn flush(&mut self) {
        while !self.failed && !self.outgoing.is_empty() {
            let (front, _) = self.outgoing.as_slices();
            match self.stream.write(front) {
                Ok(0) => self.failed = true,
                Ok(written) => {
                    self.outgoing.drain(..written);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => self.failed = true,
            }
        }
    }

    /// Reads what has arrived, up to the budget. Everything that touches the network reads before
    /// it writes: writing to a peer that has already gone makes the network stack reset the
    /// connection, which throws away what the peer sent that we have not yet read.
    fn fill(&mut self) {
        if self.incoming.len() >= 2 * (MAXIMUM_FRAME_LENGTH + HEADER_LENGTH) {
            return; // plenty is waiting to be taken; let the peer wait
        }
        let mut budget = READ_BUDGET;
        let mut chunk = [0u8; 16 * 1024];
        while !self.failed && !self.peer_finished && budget > 0 {
            match self.stream.read(&mut chunk) {
                Ok(0) => self.peer_finished = true,
                Ok(count) => {
                    self.incoming.extend_from_slice(&chunk[..count]);
                    budget = budget.saturating_sub(count);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => self.failed = true,
            }
        }
    }

    /// Removes the first whole frame from what has been received, if there is one. A length that is
    /// too large fails the connection.
    fn take_frame(&mut self) -> Option<Vec<u8>> {
        let header: [u8; HEADER_LENGTH] = self.incoming.get(..HEADER_LENGTH)?.try_into().ok()?;
        let length = u32::from_le_bytes(header) as usize;
        if length > MAXIMUM_FRAME_LENGTH {
            self.failed = true;
            return None;
        }
        let end = HEADER_LENGTH + length;
        if self.incoming.len() < end {
            return None;
        }
        let frame = self.incoming[HEADER_LENGTH..end].to_vec();
        self.incoming.drain(..end);
        Some(frame)
    }
}

impl Connection for TcpConnection {
    fn send(&mut self, frame: Vec<u8>) -> Result<(), ConnectionClosed> {
        if self.failed || frame.len() > MAXIMUM_FRAME_LENGTH {
            self.failed = true;
            return Err(ConnectionClosed);
        }
        if self.outgoing.len() + HEADER_LENGTH + frame.len() > MAXIMUM_QUEUED_BYTES {
            self.failed = true; // the peer is not keeping up
            return Err(ConnectionClosed);
        }
        self.fill();
        self.outgoing.extend((frame.len() as u32).to_le_bytes());
        self.outgoing.extend(frame);
        self.flush();
        if self.failed {
            Err(ConnectionClosed)
        } else {
            Ok(())
        }
    }

    fn receive(&mut self) -> Result<Option<Vec<u8>>, ConnectionClosed> {
        if let Some(frame) = self.take_frame() {
            return Ok(Some(frame));
        }
        self.fill();
        self.flush();
        if let Some(frame) = self.take_frame() {
            return Ok(Some(frame));
        }
        if self.failed || self.peer_finished {
            return Err(ConnectionClosed);
        }
        Ok(None)
    }
}

impl Drop for TcpConnection {
    /// Closes politely. The last queued bytes (a farewell message, say) get a short while to reach
    /// the peer; then the sending side is shut down so the peer sees the end of the stream; and
    /// what the peer has already sent is read and thrown away. That last step matters: closing a
    /// socket that still holds unread data makes the network stack reset the connection, which can
    /// discard what we just sent before the peer has read it.
    fn drop(&mut self) {
        if self.failed {
            return;
        }
        if !self.outgoing.is_empty()
            && self.stream.set_nonblocking(false).is_ok()
            && self
                .stream
                .set_write_timeout(Some(FAREWELL_TIMEOUT))
                .is_ok()
        {
            let pending: Vec<u8> = self.outgoing.drain(..).collect();
            let _ = self.stream.write_all(&pending);
        }
        let _ = self.stream.shutdown(Shutdown::Write);
        if self.peer_finished || self.stream.set_nonblocking(true).is_err() {
            return;
        }
        // Wait, for a moment at most, for the peer to finish too, discarding what it still sends.
        let deadline = Instant::now() + FAREWELL_TIMEOUT;
        let mut scratch = [0u8; 16 * 1024];
        loop {
            match self.stream.read(&mut scratch) {
                Ok(0) => return,
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => return,
            }
        }
    }
}

/// Where a server accepts TCP connections.
#[derive(Debug)]
pub struct TcpServerListener {
    listener: TcpListener,
}

impl TcpServerListener {
    /// Starts listening. Port 0 picks a free port; [`TcpServerListener::local_address`] says which.
    pub fn bind(address: impl ToSocketAddrs) -> io::Result<Self> {
        let listener = TcpListener::bind(address)?;
        listener.set_nonblocking(true)?;
        Ok(Self { listener })
    }

    pub fn local_address(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }
}

impl Listener for TcpServerListener {
    fn accept(&mut self) -> Option<Box<dyn Connection>> {
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    // A connection that cannot be set up is dropped; the next one may be fine.
                    if let Ok(connection) = TcpConnection::from_stream(stream) {
                        return Some(Box::new(connection));
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => return None,
            }
        }
    }
}
