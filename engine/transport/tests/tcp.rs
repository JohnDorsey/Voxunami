//! TCP connections on this machine's loopback interface.

use std::io::Write;
use std::net::TcpStream;
use std::time::{Duration, Instant};

use transport::loopback::loopback_listener;
use transport::tcp::{
    MAXIMUM_FRAME_LENGTH, MAXIMUM_QUEUED_BYTES, TcpConnection, TcpServerListener,
};
use transport::{Connection, ConnectionClosed, Listener, ListenerSet};

const PATIENCE: Duration = Duration::from_secs(10);

fn listener() -> (TcpServerListener, std::net::SocketAddr) {
    let listener = TcpServerListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_address().unwrap();
    (listener, address)
}

fn accept(listener: &mut TcpServerListener) -> Box<dyn Connection> {
    let deadline = Instant::now() + PATIENCE;
    loop {
        if let Some(connection) = listener.accept() {
            return connection;
        }
        assert!(Instant::now() < deadline, "nobody connected");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// A server-side connection and the client-side connection it is connected to.
fn pair() -> (Box<dyn Connection>, TcpConnection) {
    let (mut listener, address) = listener();
    let client = TcpConnection::connect(address, PATIENCE).unwrap();
    (accept(&mut listener), client)
}

/// Polls `connection` until a frame arrives, the connection closes, or time runs out.
fn next(connection: &mut dyn Connection) -> Result<Option<Vec<u8>>, ConnectionClosed> {
    let deadline = Instant::now() + PATIENCE;
    loop {
        match connection.receive()? {
            Some(frame) => return Ok(Some(frame)),
            None if Instant::now() > deadline => return Ok(None),
            None => std::thread::sleep(Duration::from_millis(1)),
        }
    }
}

#[test]
fn frames_arrive_whole_in_order_in_both_directions() {
    let (mut server, mut client) = pair();
    let frames: Vec<Vec<u8>> = vec![
        vec![],
        vec![1],
        vec![2; 1000],
        (0..=255u8).cycle().take(100_000).collect(),
    ];
    for frame in &frames {
        client.send(frame.clone()).unwrap();
    }
    server.send(vec![9, 9]).unwrap();
    for frame in &frames {
        assert_eq!(next(&mut *server), Ok(Some(frame.clone())));
    }
    assert_eq!(next(&mut client), Ok(Some(vec![9, 9])));
    assert_eq!(server.receive(), Ok(None));
}

#[test]
fn a_frame_far_larger_than_the_network_buffers_is_delivered_by_polling_both_ends() {
    let (mut server, mut client) = pair();
    let big: Vec<u8> = (0..6_000_000u32).map(|n| (n % 251) as u8).collect();
    client.send(big.clone()).unwrap();
    client.send(vec![7]).unwrap();
    let deadline = Instant::now() + PATIENCE;
    let mut received = None;
    while received.is_none() {
        // The sender is polled too: that is what keeps its queue moving.
        let _ = client.receive();
        received = server.receive().unwrap();
        assert!(Instant::now() < deadline, "the frame never arrived");
    }
    assert!(received.unwrap() == big);
    assert_eq!(next(&mut *server), Ok(Some(vec![7])));
}

#[test]
fn closing_is_noticed_only_after_what_was_sent_is_delivered() {
    let (mut server, mut client) = pair();
    for number in 0..3u8 {
        client.send(vec![number]).unwrap();
    }
    drop(client);
    for number in 0..3u8 {
        assert_eq!(next(&mut *server), Ok(Some(vec![number])));
    }
    assert_eq!(next(&mut *server), Err(ConnectionClosed));
    assert_eq!(server.receive(), Err(ConnectionClosed));
    // Sending to a closed peer fails soon: the first writes may be accepted by the network stack.
    let failed = (0..200).any(|_| {
        std::thread::sleep(Duration::from_millis(2));
        server.send(vec![0; 1000]).is_err() || server.receive().is_err()
    });
    assert!(failed);
}

#[test]
fn a_last_message_sent_just_before_closing_still_arrives() {
    let (mut server, client) = pair();
    server.send(b"goodbye".to_vec()).unwrap();
    drop(server);
    let mut client = client;
    assert_eq!(next(&mut client), Ok(Some(b"goodbye".to_vec())));
    assert_eq!(next(&mut client), Err(ConnectionClosed));
}

#[test]
fn a_frame_that_arrives_in_pieces_is_delivered_once_it_is_whole() {
    let (mut listener, address) = listener();
    let mut raw = TcpStream::connect(address).unwrap();
    raw.set_nodelay(true).unwrap();
    let mut server = accept(&mut listener);
    let mut wire = 5u32.to_le_bytes().to_vec();
    wire.extend([10, 20, 30, 40, 50]);
    for byte in &wire[..wire.len() - 1] {
        raw.write_all(&[*byte]).unwrap();
        std::thread::sleep(Duration::from_millis(2));
        assert_eq!(server.receive(), Ok(None));
    }
    raw.write_all(&wire[wire.len() - 1..]).unwrap();
    assert_eq!(next(&mut *server), Ok(Some(vec![10, 20, 30, 40, 50])));
}

#[test]
fn two_frames_in_one_write_come_out_as_two_frames() {
    let (mut listener, address) = listener();
    let mut raw = TcpStream::connect(address).unwrap();
    let mut server = accept(&mut listener);
    let mut wire = Vec::new();
    for frame in [vec![1u8, 2], vec![3u8]] {
        wire.extend((frame.len() as u32).to_le_bytes());
        wire.extend(frame);
    }
    raw.write_all(&wire).unwrap();
    assert_eq!(next(&mut *server), Ok(Some(vec![1, 2])));
    assert_eq!(next(&mut *server), Ok(Some(vec![3])));
}

#[test]
fn a_frame_longer_than_allowed_closes_the_connection_without_reading_it() {
    let (mut listener, address) = listener();
    let mut raw = TcpStream::connect(address).unwrap();
    let mut server = accept(&mut listener);
    raw.write_all(&(MAXIMUM_FRAME_LENGTH as u32 + 1).to_le_bytes())
        .unwrap();
    assert_eq!(next(&mut *server), Err(ConnectionClosed));
    // The longest allowed frame is not refused on its own account.
    let mut raw = TcpStream::connect(address).unwrap();
    let mut server = accept(&mut listener);
    raw.write_all(&(MAXIMUM_FRAME_LENGTH as u32).to_le_bytes())
        .unwrap();
    // Give the header time to arrive: it must be accepted, the frame simply not complete yet.
    for _ in 0..30 {
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(server.receive(), Ok(None));
    }
}

#[test]
fn sending_a_frame_longer_than_allowed_fails() {
    let (_server, mut client) = pair();
    assert_eq!(
        client.send(vec![0; MAXIMUM_FRAME_LENGTH + 1]),
        Err(ConnectionClosed)
    );
    assert_eq!(
        client.send(vec![1]),
        Err(ConnectionClosed),
        "and it stays failed"
    );
}

#[test]
fn a_frame_of_exactly_the_longest_allowed_length_is_sent_and_delivered() {
    let (mut server, mut client) = pair();
    let frame: Vec<u8> = (0..MAXIMUM_FRAME_LENGTH).map(|n| (n % 253) as u8).collect();
    client.send(frame.clone()).unwrap();
    let deadline = Instant::now() + PATIENCE;
    let received = loop {
        let _ = client.receive();
        if let Some(frame) = server.receive().unwrap() {
            break frame;
        }
        assert!(Instant::now() < deadline, "the frame never arrived");
    };
    assert!(received == frame);
}

#[test]
fn bytes_still_queued_when_a_connection_is_dropped_are_delivered() {
    let (mut server, mut client) = pair();
    // Three frames, far more than the network will take before the reader starts reading.
    let frame: Vec<u8> = (0..MAXIMUM_FRAME_LENGTH as u32)
        .map(|n| (n % 241) as u8)
        .collect();
    let reader = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(30));
        let deadline = Instant::now() + PATIENCE;
        let mut frames = Vec::new();
        while frames.len() < 3 && Instant::now() < deadline {
            match server.receive() {
                Ok(Some(frame)) => frames.push(frame),
                Ok(None) => std::thread::sleep(Duration::from_millis(1)),
                Err(_) => break,
            }
        }
        frames
    });
    for _ in 0..3 {
        client.send(frame.clone()).unwrap();
    }
    drop(client); // most of the frames is still queued here, not yet sent
    let received = reader.join().unwrap();
    assert_eq!(received.len(), 3, "all three frames arrive");
    assert!(received.iter().all(|received| *received == frame));
}

#[test]
fn a_peer_that_never_reads_is_dropped_instead_of_queued_for_forever() {
    let (_server_that_never_reads, mut client) = pair();
    let frame = vec![0u8; 1 << 20];
    let mut sent = 0usize;
    let outcome = loop {
        match client.send(frame.clone()) {
            Ok(()) => sent += frame.len(),
            Err(error) => break error,
        }
        assert!(sent < 4 * MAXIMUM_QUEUED_BYTES, "the queue never filled");
    };
    assert_eq!(outcome, ConnectionClosed);
    assert!(
        sent >= MAXIMUM_QUEUED_BYTES / 2,
        "{sent} bytes were accepted first"
    );
}

#[test]
fn a_listener_has_nobody_until_somebody_connects_and_then_hands_them_out_in_order() {
    let (mut listener, address) = listener();
    assert!(listener.accept().is_none());
    let mut first = TcpConnection::connect(address, PATIENCE).unwrap();
    assert!(first.peer_address().is_some());
    first.send(vec![1]).unwrap();
    let mut second = TcpConnection::connect(address, PATIENCE).unwrap();
    second.send(vec![2]).unwrap();
    let mut on_server_first = accept(&mut listener);
    let mut on_server_second = accept(&mut listener);
    assert_eq!(next(&mut *on_server_first), Ok(Some(vec![1])));
    assert_eq!(next(&mut *on_server_second), Ok(Some(vec![2])));
}

#[test]
fn connecting_to_nothing_is_an_error() {
    let (listener, address) = listener();
    drop(listener);
    assert!(TcpConnection::connect(address, Duration::from_secs(2)).is_err());
    assert!(
        TcpConnection::connect("not a host name at all:99999", Duration::from_secs(1)).is_err()
    );
}

#[test]
fn many_frames_both_ways_from_two_threads_all_arrive() {
    let (mut server, mut client) = pair();
    let echo = std::thread::spawn(move || {
        let mut count = 0u32;
        while count < 2000 {
            match next(&mut *server).unwrap() {
                Some(frame) => {
                    assert_eq!(frame, count.to_le_bytes());
                    count += 1;
                    server.send(frame).unwrap();
                }
                None => panic!("the client stopped sending"),
            }
        }
        server
    });
    let mut echoed = 0u32;
    for number in 0..2000u32 {
        client.send(number.to_le_bytes().to_vec()).unwrap();
        while let Some(frame) = client.receive().unwrap() {
            assert_eq!(frame, echoed.to_le_bytes());
            echoed += 1;
        }
    }
    while echoed < 2000 {
        let frame = next(&mut client).unwrap().expect("the echoes arrive");
        assert_eq!(frame, echoed.to_le_bytes());
        echoed += 1;
    }
    echo.join().unwrap();
}

#[test]
fn a_listener_set_asks_its_listeners_in_order() {
    let (loopback, connector) = loopback_listener();
    let (tcp, address) = listener();
    let mut set = ListenerSet::new().with(loopback).with(tcp);
    assert!(set.accept().is_none());
    let mut over_tcp = TcpConnection::connect(address, PATIENCE).unwrap();
    let mut in_process = connector.connect().unwrap();
    over_tcp.send(vec![1]).unwrap();
    in_process.send(vec![2]).unwrap();
    // Both are waiting by now, so which comes first is down to the order of the listeners.
    std::thread::sleep(Duration::from_millis(100));
    // The in-process listener comes first, then the network one, whatever the order of arrival.
    let mut first = loop {
        if let Some(connection) = set.accept() {
            break connection;
        }
    };
    assert_eq!(next(&mut *first), Ok(Some(vec![2])));
    let mut second = accept_from(&mut set);
    assert_eq!(next(&mut *second), Ok(Some(vec![1])));
}

fn accept_from(listener: &mut dyn Listener) -> Box<dyn Connection> {
    let deadline = Instant::now() + PATIENCE;
    loop {
        if let Some(connection) = listener.accept() {
            return connection;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn what_a_peer_sent_just_before_closing_arrives_even_if_it_closed_with_unread_data() {
    // Closing a TCP socket that still has unread incoming data can make the network stack reset the
    // connection, throwing away what the peer had not yet read. The peer here keeps sending to us
    // right up to the moment we leave, so we always have unread data.
    // Many rounds, side by side: each close waits a moment for its peer, and the loss this guards
    // against happens only now and then.
    let workers: Vec<_> = (0..10u8)
        .map(|worker| {
            std::thread::spawn(move || {
                for round in 0..15u8 {
                    let marker = worker * 15 + round;
                    let (mut server, mut client) = pair();
                    for _ in 0..100 {
                        server.send(vec![0; 10_000]).unwrap();
                    }
                    // Let some of it arrive in the client's buffer, unread.
                    std::thread::sleep(Duration::from_millis(3));
                    client.send(vec![marker; 100]).unwrap();
                    drop(client);
                    let mut got = None;
                    let deadline = Instant::now() + PATIENCE;
                    while got.is_none() && Instant::now() < deadline {
                        match server.receive() {
                            Ok(Some(frame)) => got = Some(frame),
                            Ok(None) => std::thread::sleep(Duration::from_millis(1)),
                            Err(_) => break,
                        }
                    }
                    assert_eq!(
                        got,
                        Some(vec![marker; 100]),
                        "worker {worker} round {round}"
                    );
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
}
