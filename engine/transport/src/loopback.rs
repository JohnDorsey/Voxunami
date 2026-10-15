//! Connections inside one process, for singleplayer and tests. Frames are still encoded bytes, so
//! the integrated server and its client exercise exactly the code a networked game does.

use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};

use crate::{Connection, ConnectionClosed, Listener};

/// One end of a loopback connection.
#[derive(Debug)]
pub struct LoopbackConnection {
    outgoing: Sender<Vec<u8>>,
    incoming: Receiver<Vec<u8>>,
}

/// Two connected ends: whatever one sends, the other receives.
pub fn loopback_pair() -> (LoopbackConnection, LoopbackConnection) {
    let (first_sender, second_receiver) = channel();
    let (second_sender, first_receiver) = channel();
    (
        LoopbackConnection {
            outgoing: first_sender,
            incoming: first_receiver,
        },
        LoopbackConnection {
            outgoing: second_sender,
            incoming: second_receiver,
        },
    )
}

impl Connection for LoopbackConnection {
    fn send(&mut self, frame: Vec<u8>) -> Result<(), ConnectionClosed> {
        self.outgoing.send(frame).map_err(|_| ConnectionClosed)
    }

    fn receive(&mut self) -> Result<Option<Vec<u8>>, ConnectionClosed> {
        match self.incoming.try_recv() {
            Ok(frame) => Ok(Some(frame)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(ConnectionClosed),
        }
    }
}

/// The server's end of a loopback "network": hands out the connections clients make.
#[derive(Debug)]
pub struct LoopbackListener {
    pending: Receiver<LoopbackConnection>,
}

/// The clients' end: connects to the matching [`LoopbackListener`]. Clone it for each client.
#[derive(Clone, Debug)]
pub struct LoopbackConnector {
    to_server: Sender<LoopbackConnection>,
}

pub fn loopback_listener() -> (LoopbackListener, LoopbackConnector) {
    let (sender, receiver) = channel();
    (
        LoopbackListener { pending: receiver },
        LoopbackConnector { to_server: sender },
    )
}

impl LoopbackConnector {
    /// Connects to the server. Fails if the server's listener is gone.
    pub fn connect(&self) -> Result<LoopbackConnection, ConnectionClosed> {
        let (client_end, server_end) = loopback_pair();
        self.to_server
            .send(server_end)
            .map_err(|_| ConnectionClosed)?;
        Ok(client_end)
    }
}

impl Listener for LoopbackListener {
    fn accept(&mut self) -> Option<Box<dyn Connection>> {
        self.pending
            .try_recv()
            .ok()
            .map(|connection| Box::new(connection) as Box<dyn Connection>)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_arrive_in_order_in_both_directions() {
        let (mut first, mut second) = loopback_pair();
        assert_eq!(second.receive(), Ok(None));
        for number in 0..5u8 {
            first.send(vec![number]).unwrap();
        }
        second.send(vec![9, 9]).unwrap();
        for number in 0..5u8 {
            assert_eq!(second.receive(), Ok(Some(vec![number])));
        }
        assert_eq!(second.receive(), Ok(None));
        assert_eq!(first.receive(), Ok(Some(vec![9, 9])));
    }

    #[test]
    fn empty_frames_are_frames() {
        let (mut first, mut second) = loopback_pair();
        first.send(Vec::new()).unwrap();
        assert_eq!(second.receive(), Ok(Some(Vec::new())));
    }

    #[test]
    fn a_closed_end_is_noticed_only_after_its_frames_are_delivered() {
        let (mut first, mut second) = loopback_pair();
        first.send(vec![1]).unwrap();
        first.send(vec![2]).unwrap();
        drop(first);
        assert_eq!(second.receive(), Ok(Some(vec![1])));
        assert_eq!(second.receive(), Ok(Some(vec![2])));
        assert_eq!(second.receive(), Err(ConnectionClosed));
        assert_eq!(second.send(vec![3]), Err(ConnectionClosed));
        // And it stays closed.
        assert_eq!(second.receive(), Err(ConnectionClosed));
    }

    #[test]
    fn a_listener_accepts_clients_in_the_order_they_connected() {
        let (mut listener, connector) = loopback_listener();
        assert!(listener.accept().is_none());
        let mut first = connector.connect().unwrap();
        let mut second = connector.clone().connect().unwrap();
        first.send(vec![1]).unwrap();
        second.send(vec![2]).unwrap();

        let mut first_on_server = listener.accept().expect("first client");
        let mut second_on_server = listener.accept().expect("second client");
        assert!(listener.accept().is_none());
        assert_eq!(first_on_server.receive(), Ok(Some(vec![1])));
        assert_eq!(second_on_server.receive(), Ok(Some(vec![2])));
        first_on_server.send(vec![7]).unwrap();
        assert_eq!(first.receive(), Ok(Some(vec![7])));
        assert_eq!(second.receive(), Ok(None));
    }

    #[test]
    fn connecting_to_a_server_that_is_gone_fails() {
        let (listener, connector) = loopback_listener();
        drop(listener);
        assert!(connector.connect().is_err());
    }

    #[test]
    fn connections_can_be_moved_to_another_thread() {
        let (mut first, mut second) = loopback_pair();
        let worker = std::thread::spawn(move || {
            first.send(vec![42]).unwrap();
        });
        worker.join().unwrap();
        assert_eq!(second.receive(), Ok(Some(vec![42])));
    }
}
