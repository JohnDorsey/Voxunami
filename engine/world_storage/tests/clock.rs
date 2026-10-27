//! The clock of a world: whole ticks since it was made, kept in its file so that a world that is saved and loaded goes on from the time it was at (the planets on
//! rails are a function of it). A world that has not ticked saves as it always did.

use world_storage::container::read_container;
use world_storage::engine_sections::{clock_id, decode_clock, encode_clock};
use world_storage::test_support::{ALL_PLUGINS, Game, sample_world};
use world_storage::{LoadError, Section};

fn sections_of(bytes: &[u8]) -> Vec<Section> {
    read_container(bytes).unwrap()
}

#[test]
fn a_new_world_is_at_tick_zero_and_its_clock_advances_by_whole_ticks() {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = sample_world(&game);
    assert_eq!(world.clock(), 0);
    world.advance_clock(1);
    world.advance_clock(1);
    assert_eq!(world.clock(), 2);
    // (A time warp is a clock that advances by more than a tick at a time.)
    world.advance_clock(1_000);
    assert_eq!(world.clock(), 1_002);
    world.set_clock(7);
    assert_eq!(world.clock(), 7);
    // It does not overflow: a world that has run for longer than the age of the universe stays where it is.
    world.set_clock(u64::MAX - 1);
    world.advance_clock(10);
    assert_eq!(world.clock(), u64::MAX);
}

#[test]
fn a_world_that_has_not_ticked_writes_no_clock_and_one_that_has_gets_it_back() {
    let mut game = Game::new(&ALL_PLUGINS);
    let mut world = sample_world(&game);
    assert!(
        !sections_of(&game.save(&world))
            .iter()
            .any(|section| section.identifier == clock_id()),
        "a world at tick 0 saves as it always did"
    );
    world.set_clock(123_456_789_012);
    let bytes = game.save(&world);
    assert!(
        sections_of(&bytes)
            .iter()
            .any(|section| section.identifier == clock_id())
    );
    let (loaded, _) = game.load(&bytes).unwrap();
    assert_eq!(loaded.clock(), 123_456_789_012);
    // And saved again, the same bytes.
    assert_eq!(game.save(&loaded), bytes);
}

#[test]
fn the_clock_of_a_description_is_not_sent_for_a_client_is_told_the_time_another_way() {
    let game = Game::new(&ALL_PLUGINS);
    let mut world = sample_world(&game);
    world.set_clock(99);
    let description = world.describe(&game.registries()).unwrap();
    assert!(
        !sections_of(&description)
            .iter()
            .any(|section| section.identifier == clock_id())
    );
}

#[test]
fn the_section_is_one_number_and_a_damaged_one_is_refused() {
    for ticks in [0, 1, 127, 128, 1 << 40, u64::MAX] {
        assert_eq!(decode_clock(&encode_clock(ticks)).unwrap(), ticks);
    }
    let bytes = encode_clock(300);
    assert!(
        decode_clock(&bytes[..bytes.len() - 1]).is_err(),
        "truncated"
    );
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(decode_clock(&longer).is_err(), "trailing bytes");
    assert!(decode_clock(&[]).is_err(), "empty");
}

#[test]
fn a_clock_section_of_a_newer_version_is_refused_not_dropped() {
    let mut game = Game::new(&ALL_PLUGINS);
    let mut world = sample_world(&game);
    world.set_clock(5);
    let bytes = game.save(&world);
    let mut sections = sections_of(&bytes);
    for section in &mut sections {
        if section.identifier == clock_id() {
            section.version = world_storage::engine_sections::SUPPORTED_VERSION + 1;
        }
    }
    let rewritten = world_storage::container::write_container(&sections);
    assert!(matches!(
        game.load(&rewritten),
        Err(LoadError::UnsupportedEngineSection { .. })
    ));
}
