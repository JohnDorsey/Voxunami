//! How a client takes in the packs a server replicates: asks for what it lacks, assembles the pieces, checks them against the index it was told, keeps what is
//! good in its store, and tells its connection of what it took from the store, so that a recording holds every pack.

use std::io;
use std::sync::{Arc, Mutex};

use client::{Client, ClientError, Handshake, HandshakeProgress, MemoryPackStore, PackStore};
use identity::Identity;
use pack_archive::{Archive, FileIndex, Hash};
use protocol::{
    ClientId, ClientMessage, PROTOCOL_VERSION, PackListing, ServerMessage, Tick, Welcome,
    pack_chunks,
};
use transport::loopback::{LoopbackConnection, loopback_pair};
use transport::{Connection, ConnectionClosed};
use world_storage::test_support::{ALL_PLUGINS, Game, id};

fn game_id() -> namespaced_id::NamespacedId {
    id("test_game:game")
}

/// A pack of three files, one of them withheld, as a server would make it: the listing clients are told, and the archive of what they are sent.
struct SamplePack {
    listing: PackListing,
    archive: Vec<u8>,
}

fn sample_pack(pack_id: &str, filler: usize) -> SamplePack {
    sample_pack_with(pack_id, filler, b"A")
}

/// The same, with another content for `blocks/a.json`: another pack of the same size.
fn sample_pack_with(pack_id: &str, filler: usize, block: &[u8]) -> SamplePack {
    let big: Vec<u8> = (0..filler).map(|index| (index * 7 % 251) as u8).collect();
    let all = Archive::from_files([
        (
            "pack.json".to_owned(),
            format!("{{\"id\":\"{pack_id}\"}}").into_bytes(),
        ),
        ("blocks/a.json".to_owned(), block.to_vec()),
        ("code/secret.py".to_owned(), b"withheld".to_vec()),
        ("textures/big.bin".to_owned(), big),
    ])
    .unwrap();
    let sent = |path: &str| !path.starts_with("code/");
    let archive = all.only(sent).to_bytes();
    SamplePack {
        listing: PackListing {
            id: pack_id.to_owned(),
            index: FileIndex::of(&all, sent),
            archive_size: archive.len() as u64,
        },
        archive,
    }
}

/// A store that cannot keep anything.
#[derive(Debug)]
struct FullStore;

impl PackStore for FullStore {
    fn load(&mut self, _pack_id: &str, _root: &Hash) -> Option<Vec<u8>> {
        None
    }

    fn save(&mut self, _pack_id: &str, _root: &Hash, _archive: &[u8]) -> io::Result<()> {
        Err(io::Error::other("the disk is full"))
    }
}

fn store_with(pack: &SamplePack) -> MemoryPackStore {
    let store = MemoryPackStore::new();
    store.put(&pack.listing.id, pack.listing.root(), pack.archive.clone());
    store
}

/// A connection that remembers the frames it was told of, which did not come over it.
#[derive(Debug)]
struct Noting {
    inner: LoopbackConnection,
    noted: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl Connection for Noting {
    fn send(&mut self, frame: Vec<u8>) -> Result<(), ConnectionClosed> {
        self.inner.send(frame)
    }

    fn receive(&mut self) -> Result<Option<Vec<u8>>, ConnectionClosed> {
        self.inner.receive()
    }

    fn note_frame(&mut self, frame: &[u8]) {
        self.noted.lock().unwrap().push(frame.to_vec());
    }
}

struct Rig {
    server: LoopbackConnection,
    client: Client<Noting>,
    noted: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl Rig {
    fn new(store: Option<Box<dyn PackStore>>) -> Self {
        let game = Game::new(&ALL_PLUGINS);
        let identity = Identity::from_secret([3; 32]);
        let (client_end, mut server) = loopback_pair();
        let noted = Arc::new(Mutex::new(Vec::new()));
        let connection = Noting {
            inner: client_end,
            noted: Arc::clone(&noted),
        };
        let handshake = Handshake::start(connection, game_id(), &identity).unwrap();
        server.receive().unwrap().expect("the hello");
        server
            .send(
                ServerMessage::Welcome(Welcome {
                    protocol_version: PROTOCOL_VERSION,
                    game: game_id(),
                    client: ClientId::new(7),
                    ticks_per_second: 20,
                    tick: Tick::new(4),
                    world_clock: 1_004,
                    world_description: game.new_world().describe(&game.registries()).unwrap(),
                    package: Vec::new(),
                })
                .encode(),
            )
            .unwrap();
        let HandshakeProgress::Welcomed(welcomed) = handshake.poll().unwrap() else {
            panic!("the welcome was sent");
        };
        let mut client_game = Game::new(&ALL_PLUGINS);
        let (client, _) = match store {
            Some(store) => welcomed
                .finish_with_packs(client_game.load_context(), store)
                .unwrap(),
            None => welcomed.finish(client_game.load_context()).unwrap(),
        };
        Self {
            server,
            client,
            noted,
        }
    }

    fn send(&mut self, message: ServerMessage) {
        self.server.send(message.encode()).unwrap();
    }

    /// What the client has sent the server since the last call.
    fn requests(&mut self) -> Vec<ClientMessage> {
        let mut requests = Vec::new();
        while let Some(frame) = self.server.receive().unwrap() {
            requests.push(ClientMessage::decode(&frame).unwrap());
        }
        requests
    }

    fn send_pack(&mut self, pack: &SamplePack) {
        for (offset, bytes) in pack_chunks(&pack.archive) {
            self.send(ServerMessage::PackChunk {
                root: pack.listing.root(),
                offset,
                bytes: bytes.to_vec(),
            });
        }
    }

    /// Ends a tick, so that the client has read everything before it.
    fn end_tick(&mut self, number: u64) -> Result<(), ClientError> {
        self.send(ServerMessage::TickCompleted {
            tick: Tick::new(number),
        });
        self.client.poll_tick().map(|update| {
            update.expect("the tick completed");
        })
    }
}

#[test]
fn a_client_with_no_store_asks_for_every_pack_in_the_list_and_holds_them_when_the_pieces_are_in() {
    let big = sample_pack("big", 70_000);
    let small = sample_pack("small", 10);
    let mut rig = Rig::new(None);
    assert!(
        rig.requests().is_empty(),
        "nothing is asked before it is told"
    );
    rig.send(packs_message(vec![
        big.listing.clone(),
        small.listing.clone(),
    ]));
    rig.end_tick(5).unwrap();
    assert_eq!(
        rig.requests(),
        [ClientMessage::RequestPacks {
            roots: vec![big.listing.root(), small.listing.root()]
        }]
    );
    assert!(!rig.client.packs().is_complete());
    assert_eq!(
        rig.client.packs().listed(),
        [big.listing.clone(), small.listing.clone()]
    );
    rig.send_pack(&small);
    rig.end_tick(6).unwrap();
    assert!(!rig.client.packs().is_complete(), "one of two");
    assert!(rig.client.packs().get(&small.listing.root()).is_some());
    rig.send_pack(&big);
    rig.end_tick(7).unwrap();
    assert!(rig.client.packs().is_complete());
    let held = rig.client.packs().get(&big.listing.root()).unwrap();
    assert_eq!(held.id, "big");
    assert_eq!(held.index, big.listing.index);
    assert_eq!(held.archive.to_bytes(), big.archive);
    assert!(held.archive.get("code/secret.py").is_none());
    assert!(
        rig.noted.lock().unwrap().is_empty(),
        "what the server sent is in a recording already: nothing is noted"
    );
}

#[test]
fn a_pack_in_the_store_is_not_asked_for_and_its_pieces_are_noted_to_the_connection_for_a_recording()
{
    let kept = sample_pack("kept", 150_000);
    let fresh = sample_pack("fresh", 20);
    let store = store_with(&kept);
    let mut rig = Rig::new(Some(Box::new(store.clone())));
    rig.send(packs_message(vec![
        kept.listing.clone(),
        fresh.listing.clone(),
    ]));
    rig.end_tick(5).unwrap();
    assert_eq!(
        rig.requests(),
        [ClientMessage::RequestPacks {
            roots: vec![fresh.listing.root()]
        }],
        "only the one it lacks"
    );
    assert!(rig.client.packs().get(&kept.listing.root()).is_some());
    // The noted frames are the pieces the server would have sent, in order: a replay of the recording reads them like a server's.
    let noted: Vec<ServerMessage> = rig
        .noted
        .lock()
        .unwrap()
        .iter()
        .map(|frame| ServerMessage::decode(frame).unwrap())
        .collect();
    let mut archive = Vec::new();
    for message in &noted {
        let ServerMessage::PackChunk {
            root,
            offset,
            bytes,
        } = message
        else {
            panic!("only pieces are noted: {message:?}");
        };
        assert_eq!(*root, kept.listing.root());
        assert_eq!(*offset as usize, archive.len());
        archive.extend_from_slice(bytes);
    }
    assert_eq!(archive, kept.archive);
    assert!(noted.len() >= 3);
    // A pack that arrives from the server is kept in the store for the next time.
    assert_eq!(store.get("fresh", &fresh.listing.root()), None);
    rig.send_pack(&fresh);
    rig.end_tick(6).unwrap();
    assert_eq!(
        store.get("fresh", &fresh.listing.root()),
        Some(fresh.archive.clone())
    );
    assert!(rig.client.packs().is_complete());
}

#[test]
fn a_file_in_the_store_that_does_not_match_its_pack_is_not_trusted_and_the_pack_is_asked_for() {
    let pack = sample_pack("kept", 1_000);
    let store = MemoryPackStore::new();
    let mut damaged = pack.archive.clone();
    let middle = damaged.len() / 2;
    damaged[middle] ^= 0xff;
    store.put("kept", pack.listing.root(), damaged);
    let mut rig = Rig::new(Some(Box::new(store.clone())));
    rig.send(packs_message(vec![pack.listing.clone()]));
    rig.end_tick(5).unwrap();
    assert_eq!(
        rig.requests(),
        [ClientMessage::RequestPacks {
            roots: vec![pack.listing.root()]
        }]
    );
    assert!(rig.client.packs().get(&pack.listing.root()).is_none());
    assert!(rig.noted.lock().unwrap().is_empty());
    // what the server sends then replaces the bad file
    rig.send_pack(&pack);
    rig.end_tick(6).unwrap();
    assert_eq!(store.get("kept", &pack.listing.root()), Some(pack.archive));
}

#[test]
fn a_store_that_cannot_save_does_not_stop_the_client_holding_the_pack() {
    let pack = sample_pack("kept", 100);
    let mut rig = Rig::new(Some(Box::new(FullStore)));
    rig.send(packs_message(vec![pack.listing.clone()]));
    rig.send_pack(&pack);
    rig.end_tick(5).unwrap();
    assert!(rig.client.packs().is_complete());
    assert_eq!(rig.client.packs().store_problems().len(), 1);
    assert!(rig.client.packs().store_problems()[0].contains("the disk is full"));
}

/// What the client says when it is sent a pack that is wrong.
fn error_of_a_bad_pack(
    pack: &SamplePack,
    pieces: impl FnOnce(&SamplePack) -> Vec<ServerMessage>,
) -> ClientError {
    let mut rig = Rig::new(None);
    rig.send(packs_message(vec![pack.listing.clone()]));
    for piece in pieces(pack) {
        rig.send(piece);
    }
    rig.end_tick(5).expect_err("the pack is refused")
}

/// The server's list, which is the whole of what it runs.
fn packs_message(listings: Vec<PackListing>) -> ServerMessage {
    ServerMessage::Packs {
        listings,
        whole: true,
    }
}

fn pieces_of(pack: &SamplePack) -> Vec<ServerMessage> {
    pack_chunks(&pack.archive)
        .map(|(offset, bytes)| ServerMessage::PackChunk {
            root: pack.listing.root(),
            offset,
            bytes: bytes.to_vec(),
        })
        .collect()
}

#[test]
fn a_pack_that_is_sent_wrong_is_refused_naming_the_pack_and_what_is_wrong() {
    let pack = sample_pack("odd", 70_000);
    // a piece that is not the next
    let error = error_of_a_bad_pack(&pack, |pack| {
        let mut pieces = pieces_of(pack);
        pieces.swap(0, 1);
        pieces
    });
    assert!(
        matches!(&error, ClientError::BadPack { pack, reason } if pack == "odd" && reason.contains("offset")),
        "{error}"
    );
    // more bytes than the list said
    let error = error_of_a_bad_pack(&pack, |pack| {
        let mut pieces = pieces_of(pack);
        let Some(ServerMessage::PackChunk { bytes, .. }) = pieces.last_mut() else {
            panic!("a piece");
        };
        bytes.push(1);
        pieces
    });
    assert!(
        matches!(&error, ClientError::BadPack { pack, .. } if pack == "odd"),
        "{error}"
    );
    // the right size and the wrong bytes: a file that is not the one the index says
    let error = error_of_a_bad_pack(&pack, |pack| {
        let mut changed = pack.archive.clone();
        let middle = changed.len() / 2;
        changed[middle] ^= 1;
        let altered = SamplePack {
            listing: pack.listing.clone(),
            archive: changed,
        };
        pieces_of(&altered)
    });
    assert!(
        matches!(&error, ClientError::BadPack { pack, .. } if pack == "odd"),
        "{error}"
    );
    // a canonical archive of the right size that is of other files (one byte of one file is another): the index refuses it
    let other = sample_pack_with("odd", 70_000, b"B");
    assert_eq!(other.archive.len(), pack.archive.len());
    let error = error_of_a_bad_pack(&pack, |pack| {
        pieces_of(&SamplePack {
            listing: pack.listing.clone(),
            archive: other.archive.clone(),
        })
    });
    assert!(
        matches!(&error, ClientError::BadPack { pack, reason } if pack == "odd" && reason.contains("blocks/a.json")),
        "{error}"
    );
}

#[test]
fn a_piece_of_a_pack_that_is_not_listed_is_ignored_since_a_replaced_list_may_have_taken_it_out() {
    let listed = sample_pack("listed", 10);
    let stranger = sample_pack("stranger", 10);
    let mut rig = Rig::new(None);
    rig.send(packs_message(vec![listed.listing.clone()]));
    rig.send_pack(&stranger);
    rig.end_tick(5).unwrap();
    assert!(!rig.client.packs().is_complete());
    assert!(rig.client.packs().get(&stranger.listing.root()).is_none());
}

#[test]
fn a_new_list_keeps_what_the_client_holds_asks_for_what_is_new_and_forgets_what_is_gone() {
    let kept = sample_pack("kept", 10);
    let going = sample_pack("going", 20);
    let coming = sample_pack("coming", 30);
    let mut rig = Rig::new(None);
    rig.send(packs_message(vec![
        kept.listing.clone(),
        going.listing.clone(),
    ]));
    rig.send_pack(&kept);
    rig.send_pack(&going);
    rig.end_tick(5).unwrap();
    rig.requests();
    assert!(rig.client.packs().is_complete());

    rig.send(packs_message(vec![
        coming.listing.clone(),
        kept.listing.clone(),
    ]));
    rig.end_tick(6).unwrap();
    assert_eq!(
        rig.requests(),
        [ClientMessage::RequestPacks {
            roots: vec![coming.listing.root()]
        }],
        "only the new one"
    );
    assert!(rig.client.packs().get(&kept.listing.root()).is_some());
    assert!(rig.client.packs().get(&going.listing.root()).is_none());
    assert!(!rig.client.packs().is_complete());
    rig.send_pack(&coming);
    rig.end_tick(7).unwrap();
    assert!(rig.client.packs().is_complete());

    // an empty list forgets them all and asks for nothing
    rig.send(packs_message(Vec::new()));
    rig.end_tick(8).unwrap();
    assert!(rig.requests().is_empty());
    assert!(rig.client.packs().listed().is_empty());
    assert_eq!(rig.client.packs().held().count(), 0);
    assert!(rig.client.packs().is_complete());
}

#[test]
fn a_pack_is_taken_in_between_the_messages_of_a_tick_and_the_view_of_the_tick_is_unchanged() {
    let pack = sample_pack("mid", 100_000);
    let mut rig = Rig::new(None);
    rig.send(packs_message(vec![pack.listing.clone()]));
    rig.end_tick(5).unwrap();
    let before = rig.client.world().tick();
    // the pieces arrive, with a tick's messages between them
    let pieces = pieces_of(&pack);
    let (first, rest) = pieces.split_first().unwrap();
    rig.send(first.clone());
    rig.send(ServerMessage::GameMessage {
        kind: id("test_game:thing"),
        payload: vec![1],
    });
    for piece in rest {
        rig.send(piece.clone());
    }
    rig.end_tick(6).unwrap();
    assert_eq!(rig.client.world().tick(), Tick::new(6));
    assert!(before < rig.client.world().tick());
    assert!(rig.client.packs().is_complete());
}

#[test]
fn a_client_knows_whether_the_list_is_the_whole_of_what_the_server_runs_and_a_server_that_says_nothing_has_only_the_games_own()
 {
    let pack = sample_pack("only", 10);
    let mut rig = Rig::new(None);
    // nothing said: the server runs only the game's own, which is a whole list of none
    assert!(rig.client.packs().whole());
    rig.send(ServerMessage::Packs {
        listings: vec![pack.listing.clone()],
        whole: false,
    });
    rig.end_tick(5).unwrap();
    assert!(!rig.client.packs().whole());
    // the pack is asked for and held all the same: it is the server's, and not all of it
    assert_eq!(rig.requests().len(), 1);
    rig.send_pack(&pack);
    rig.end_tick(6).unwrap();
    assert!(rig.client.packs().is_complete());
    assert!(!rig.client.packs().whole());
    // a list that says it is the whole, with the same packs, changes only that
    rig.send(packs_message(vec![pack.listing.clone()]));
    rig.end_tick(7).unwrap();
    assert!(rig.client.packs().whole());
    assert!(rig.client.packs().is_complete());
    assert!(
        rig.requests().is_empty(),
        "what it holds is not asked for again"
    );
    // a list of none that is not the whole is told too
    rig.send(ServerMessage::Packs {
        listings: Vec::new(),
        whole: false,
    });
    rig.end_tick(8).unwrap();
    assert!(!rig.client.packs().whole() && rig.client.packs().listed().is_empty());
}
