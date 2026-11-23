//! Packs and recordings (`docs/specs/pack-archive-spec.md`, step 4): a recording holds every pack its client had, whether the server sent it or the client took it
//! from its own cache, so a replay needs nothing installed, and a replay of a session rebuilds the packs and the views the live client had.

use client::{ClientError, MemoryPackStore, PackStore};
use end_to_end_tests::{Fixture, replay_player};
use pack_archive::{Archive, Hash};
use protocol::{ServerMessage, pack_chunks};
use replay::{ReplayConnection, ReplayEnd, ReplayReader};
use server::ServedPack;
use world_storage::test_support::ALL_PLUGINS;

/// A pack with one large file (so that it is several pieces) and one withheld file.
fn a_pack(pack_id: &str, filler: usize) -> ServedPack {
    let big: Vec<u8> = (0..filler)
        .map(|index| (index.wrapping_mul(40_503) >> 7) as u8)
        .collect();
    let archive = Archive::from_files([
        (
            "pack.json".to_owned(),
            format!("{{\"id\":\"{pack_id}\"}}").into_bytes(),
        ),
        ("code/rules.py".to_owned(), b"withheld".to_vec()),
        ("textures/big.bin".to_owned(), big),
    ])
    .unwrap();
    ServedPack::new(pack_id, &archive, |path| !path.starts_with("code/")).unwrap()
}

/// What a client holds of packs, in the order of the list: id, root, and the archive's bytes.
fn held<C: transport::Connection>(client: &client::Client<C>) -> Vec<(String, Hash, Vec<u8>)> {
    client
        .packs()
        .held()
        .map(|pack| (pack.id.clone(), pack.root(), pack.archive.to_bytes()))
        .collect()
}

fn play_to_the_end<R: std::io::Read + Send>(
    player: &mut end_to_end_tests::Player<ReplayConnection<R>>,
) {
    match player.catch_up() {
        Err(ClientError::ConnectionClosed) => {}
        other => panic!("a replay ends by closing the connection: {other:?}"),
    }
}

/// How many pieces of the pack with this root a recording holds.
fn pieces_in_recording(recording: Vec<u8>, root: Hash) -> usize {
    let mut reader = ReplayReader::new(std::io::Cursor::new(recording)).unwrap();
    let mut pieces = 0;
    while let Some(frame) = reader.next_frame().unwrap() {
        if let Ok(ServerMessage::PackChunk { root: found, .. }) = ServerMessage::decode(&frame)
            && found == root
        {
            pieces += 1;
        }
    }
    pieces
}

#[test]
fn a_replay_of_a_session_that_was_sent_packs_holds_the_same_packs_and_sees_the_same_world() {
    let big = a_pack("big", protocol::MAXIMUM_PACK_CHUNK * 2 + 500);
    let small = a_pack("small", 300);
    let mut fixture = Fixture::with_packs(vec![big.clone(), small.clone()]);
    let (mut alice, recording) =
        fixture.join_recorded_with_store(&ALL_PLUGINS, Box::new(MemoryPackStore::new()));
    for _ in 0..12 {
        fixture.step();
        alice.catch_up().unwrap();
    }
    assert!(alice.client.packs().is_complete(), "the packs arrived");
    assert_eq!(held(&alice.client).len(), 2);

    let mut replayed = replay_player(recording.contents(), &ALL_PLUGINS).unwrap();
    play_to_the_end(&mut replayed);
    assert!(matches!(
        replayed.client.connection().end(),
        ReplayEnd::Complete
    ));
    assert_eq!(held(&replayed.client), held(&alice.client));
    assert_eq!(
        replayed.client.packs().listed(),
        alice.client.packs().listed()
    );
    assert_eq!(replayed.updates, alice.updates, "events differ");
    assert_eq!(replayed.summaries, alice.summaries, "views differ");
    // what was held is what the server sent: the files marked sent, as the canonical archive
    assert_eq!(held(&alice.client)[0].2, big.archive_bytes());
}

#[test]
fn a_pack_a_client_took_from_its_own_cache_is_in_its_recording_all_the_same() {
    let cached = a_pack("cached", protocol::MAXIMUM_PACK_CHUNK * 2 + 500);
    let fresh = a_pack("fresh", 300);
    let store = MemoryPackStore::new();
    store.put("cached", cached.root(), cached.archive_bytes().to_vec());
    let mut fixture = Fixture::with_packs(vec![cached.clone(), fresh.clone()]);
    let (mut alice, recording) = fixture.join_recorded_with_store(&ALL_PLUGINS, Box::new(store));
    for _ in 0..12 {
        fixture.step();
        alice.catch_up().unwrap();
    }
    assert!(alice.client.packs().is_complete());

    // The server was never asked for the cached pack, and the recording has its pieces: the client noted them.
    let expected_pieces = pack_chunks(cached.archive_bytes()).count();
    assert!(expected_pieces >= 3);
    assert_eq!(
        pieces_in_recording(recording.contents(), cached.root()),
        expected_pieces
    );
    assert_eq!(
        pieces_in_recording(recording.contents(), fresh.root()),
        pack_chunks(fresh.archive_bytes()).count()
    );

    let mut replayed = replay_player(recording.contents(), &ALL_PLUGINS).unwrap();
    play_to_the_end(&mut replayed);
    assert_eq!(held(&replayed.client), held(&alice.client));
    assert_eq!(held(&replayed.client)[0].2, cached.archive_bytes());
    assert_eq!(replayed.summaries, alice.summaries);
}

#[test]
fn a_replaced_list_in_the_middle_of_a_session_replays_to_the_packs_the_client_ended_with() {
    let first = a_pack("first", 4_000);
    let second = a_pack("second", 5_000);
    let third = a_pack("third", 6_000);
    let mut fixture = Fixture::with_packs(vec![first.clone(), second.clone()]);
    let store = MemoryPackStore::new();
    let (mut alice, recording) =
        fixture.join_recorded_with_store(&ALL_PLUGINS, Box::new(store.clone()));
    for step in 0..30 {
        if step == 10 {
            // "first" goes, "third" comes, "second" stays
            fixture
                .server
                .replace_packs(vec![second.clone(), third.clone()]);
        }
        fixture.step();
        alice.catch_up().unwrap();
    }
    let ids: Vec<String> = held(&alice.client).into_iter().map(|pack| pack.0).collect();
    assert_eq!(ids, ["second", "third"]);
    // the client kept in its store what it was sent, the pack that was taken out of the list too
    for pack in [&first, &second, &third] {
        assert_eq!(
            store.get(&pack.listing().id, &pack.root()),
            Some(pack.archive_bytes().to_vec())
        );
    }

    let mut replayed = replay_player(recording.contents(), &ALL_PLUGINS).unwrap();
    play_to_the_end(&mut replayed);
    assert_eq!(held(&replayed.client), held(&alice.client));
    assert_eq!(replayed.summaries, alice.summaries);
}

#[test]
fn a_store_that_is_not_there_changes_nothing_but_what_is_kept() {
    // The same session with a client that has no store holds the same packs as one that has: a store only saves a download.
    let pack = a_pack("only", 100_000);
    let run = |store: Option<Box<dyn PackStore>>| {
        let mut fixture = Fixture::with_packs(vec![pack.clone()]);
        let (mut player, _) = match store {
            Some(store) => fixture.join_recorded_with_store(&ALL_PLUGINS, store),
            None => fixture.join_recorded_with_store(&ALL_PLUGINS, Box::new(client::NoPackStore)),
        };
        for _ in 0..8 {
            fixture.step();
            player.catch_up().unwrap();
        }
        held(&player.client)
    };
    assert_eq!(
        run(None),
        run(Some(Box::new(MemoryPackStore::new()))),
        "the same packs, store or none"
    );
}
