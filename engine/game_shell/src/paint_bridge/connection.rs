//! The game's side of the socket: one non-blocking connection to a tool on this machine, that is looked at once a frame.

use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use super::protocol::{FromTool, LineBuffer, ToTool};

const READ_BYTES: usize = 64 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// A connection to a tool. Nothing here waits: `send` queues, `poll` takes what has arrived and sends what is queued.
#[derive(Debug)]
pub struct ToolConnection {
    stream: TcpStream,
    lines: LineBuffer,
    waiting: Vec<u8>,
    open: bool,
}

impl ToolConnection {
    /// Connects to a tool that is listening at `address` (this machine, normally).
    pub fn connect(address: SocketAddr) -> std::io::Result<Self> {
        let stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT)?;
        stream.set_nonblocking(true)?;
        stream.set_nodelay(true)?;
        Ok(Self {
            stream,
            lines: LineBuffer::default(),
            waiting: Vec::new(),
            open: true,
        })
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn send(&mut self, message: &ToTool) {
        if self.open {
            self.waiting.extend(message.to_line());
        }
    }

    /// What the tool has said since the last call (a line that is not a message is an `Err` with what is wrong). Sends what is queued. Once the tool has gone, `is_open`
    /// is false.
    pub fn poll(&mut self) -> Vec<Result<FromTool, String>> {
        let mut found = Vec::new();
        if !self.open {
            return found;
        }
        let mut buffer = vec![0u8; READ_BYTES];
        loop {
            match self.stream.read(&mut buffer) {
                Ok(0) => {
                    self.open = false;
                    break;
                }
                Ok(count) => {
                    for line in self.lines.feed(&buffer[..count]) {
                        found.push(FromTool::parse(&line));
                    }
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(_) => {
                    self.open = false;
                    break;
                }
            }
        }
        while self.open && !self.waiting.is_empty() {
            match self.stream.write(&self.waiting) {
                Ok(0) => self.open = false,
                Ok(count) => {
                    self.waiting.drain(..count);
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(_) => self.open = false,
            }
        }
        found
    }
}
