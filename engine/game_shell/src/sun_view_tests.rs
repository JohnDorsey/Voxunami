//! The sun in a player's session: it follows the clock of the world that the server sends, the same for everyone and in a recording; a player can pin the time of day
//! with `.time`; and a game that has no sun in a level has none there.

use identity::Identity;
use player_presence::PlayersPlugin;
use plugin_host::Plugin;
use server::Server;
use transport::Connection;
use transport::loopback::LoopbackConnector;

use crate::definition::{GameContent, GameDefinition};
use crate::files::{open_recording, record_connection};
use crate::host::build_server_with_plugins;
use crate::session::{Mode, Session};
use crate::test_support::{ExampleContent, ExampleGame, ExampleSunGame};
use crate::view::camera_at;

type Player = Session<Box<dyn Connection>, ExampleSunGame>;

struct Rig {
    server: Server,
    connector: LoopbackConnector,
}

impl Rig {
    /// A world whose clock was at `clock` before this run.
    fn new(clock: u64) -> Self {
        let content: ExampleContent = ExampleSunGame::new_content();
        let mut world = content.new_world(7);
        world.set_clock(clock);
        let plugins: Vec<Box<dyn Plugin>> = vec![Box::new(PlayersPlugin::new(true))];
        let (server, connector) =
            build_server_with_plugins::<ExampleSunGame>(&content, world, 2, plugins).unwrap();
        Self { server, connector }
    }

    fn join(&mut self, who: u8) -> Player {
        let connection: Box<dyn Connection> = Box::new(self.connector.connect().unwrap());
        self.join_over(connection, who)
    }

    fn join_over(&mut self, connection: Box<dyn Connection>, who: u8) -> Player {
        let server = &mut self.server;
        Session::<_, ExampleSunGame>::connect(
            connection,
            &Identity::from_secret([who; 32]),
            Mode::Playing,
            2,
            camera_at([8.0, 40.0, 8.0], 0.0, 0.0),
            || {
                server.step();
            },
        )
        .unwrap()
    }

    /// Runs the server `ticks` ticks and brings the player up to date with them.
    fn run(&mut self, player: &mut Player, ticks: u32) {
        for _ in 0..ticks {
            self.server.step();
        }
        while player.advance_one_tick().unwrap() {}
    }
}

#[test]
fn the_sun_is_where_the_world_s_clock_puts_it_and_goes_on_with_each_tick() {
    let mut rig = Rig::new(0);
    let mut player = rig.join(1);
    rig.run(&mut player, 0);
    let sun = ExampleSunGame::the_sun();
    let clock = player.client().world().world_clock();
    let view = player.sun_view().expect("the level has a sun");
    assert_eq!(view.hours, sun.hours_at(clock, 20));
    assert_eq!(view.direction, sun.direction_at(view.hours));
    assert!(clock < 10, "a new world: {clock}");
    assert!((6.0..6.01).contains(&view.hours), "{}", view.hours);
    // A day is 24,000 ticks, so 600 ticks are 0.6 of an hour.
    let before = view.hours;
    rig.run(&mut player, 600);
    let after = player.sun_view().unwrap().hours;
    assert!((after - before - 0.6).abs() < 1e-9, "{before} to {after}");
}

#[test]
fn the_clock_the_client_counts_is_the_servers_however_old_the_world_is() {
    // A world that was 12,000 ticks old (half a day) when it was started again: noon of the second half of the day begins at 18.
    let mut rig = Rig::new(12_000);
    let mut player = rig.join(1);
    rig.run(&mut player, 40);
    let world = player.client().world();
    assert_eq!(
        world.world_clock() - world.tick().get(),
        rig.server.world().clock() - rig.server.tick().get(),
        "the client's clock is the server's, less the ticks it has not been told of"
    );
    assert!(world.world_clock() >= 12_040);
    let hours = player.sun_view().unwrap().hours;
    assert!((18.0..18.1).contains(&hours), "{hours}");
}

#[test]
fn two_players_see_the_same_sun_and_one_who_comes_later_sees_where_it_has_got_to() {
    let mut rig = Rig::new(0);
    let mut first = rig.join(1);
    rig.run(&mut first, 2_000);
    let mut second = rig.join(2);
    rig.run(&mut first, 1);
    rig.run(&mut second, 0);
    let (a, b) = (first.sun_view().unwrap(), second.sun_view().unwrap());
    assert_eq!(
        first.client().world().tick(),
        second.client().world().tick()
    );
    assert_eq!(a.hours, b.hours);
    assert_eq!(a.direction, b.direction);
    assert!(a.hours > 7.99, "{}", a.hours);
}

#[test]
fn a_recording_has_the_same_day_as_the_player_it_recorded() {
    let directory = std::env::temp_dir().join(format!("voxunami_sun_view_{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("session.voxreplay");
    let mut rig = Rig::new(9_000);
    let recorded: Box<dyn Connection> =
        record_connection(rig.connector.connect().unwrap(), &path).unwrap();
    let mut player = rig.join_over(recorded, 1);
    rig.run(&mut player, 300);
    let live = player.sun_view().unwrap();
    let live_tick = player.client().world().tick();
    drop(player);

    let connection: Box<dyn Connection> = Box::new(open_recording(&path).unwrap());
    let mut replay = Session::<_, ExampleSunGame>::connect(
        connection,
        &Identity::generate(),
        Mode::Replaying,
        2,
        camera_at([0.0; 3], 0.0, 0.0),
        || {},
    )
    .unwrap();
    while replay.advance_one_tick().unwrap_or(false) {}
    assert_eq!(replay.client().world().tick(), live_tick);
    let replayed = replay.sun_view().unwrap();
    assert_eq!(replayed.hours, live.hours);
    assert_eq!(replayed.direction, live.direction);
    assert_eq!(replayed.sky_color, live.sky_color);
    assert!(
        live.hours > 15.0,
        "the recording is of a world that was 9,000 ticks old: {}",
        live.hours
    );
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn the_atmosphere_of_a_picture_is_the_suns_and_the_sides_are_shaded_from_one_place() {
    let mut rig = Rig::new(0);
    let mut player = rig.join(1);
    player.client_command(".time 15:00");
    let view = player.sun_view().unwrap();
    let (lit, occluders) = crate::view::lit_atmosphere(&player);
    assert_eq!(lit.sun_direction, view.direction);
    assert_eq!((lit.sun_light, lit.sky_light), (255, 100));
    assert_eq!(lit.sky_color, view.sky_color);
    assert_eq!(occluders, view.occluders);
    assert_eq!(occluders, [renderer::SunOccluder::FlatGround]);
    // The sides of faces are shaded from the renderer's own direction, not the moving sun's.
    assert_eq!(
        lit.side_direction,
        Some(renderer::Atmosphere::default().sun_direction)
    );
    assert_ne!(lit.side_direction, Some(lit.sun_direction));
    // The terrain casts shadows, with the share of the sky's light the game says reaches a place in one; the player can turn them off, and on.
    assert!(lit.shadows);
    assert_eq!(lit.sky_light_in_shadow, 64);
    // And the share that reaches a place with the terrain over it.
    assert_eq!(lit.sky_light_under_cover, 32);
    player.client_command(".shadows off");
    assert!(!crate::view::lit_atmosphere(&player).0.shadows);
    player.client_command(".shadows on");
    assert!(crate::view::lit_atmosphere(&player).0.shadows);
    // The fog is the shell's, as it was: its range, and the ambient light the player set.
    player.client_command(".ambient 33");
    let (lit, _) = crate::view::lit_atmosphere(&player);
    assert_eq!(lit.ambient_light, 33);
    let plain = crate::view::atmosphere(player.fog_radius_bricks(), 33);
    assert_eq!(
        (lit.fog_start, lit.fog_end),
        (plain.fog_start, plain.fog_end)
    );
}

#[test]
fn a_game_with_no_sun_in_the_level_is_lit_as_it_was() {
    let content: ExampleContent = ExampleGame::new_content();
    let world = content.new_world(7);
    let plugins: Vec<Box<dyn Plugin>> = vec![Box::new(PlayersPlugin::new(true))];
    let (mut server, connector) =
        build_server_with_plugins::<ExampleGame>(&content, world, 2, plugins).unwrap();
    let connection: Box<dyn Connection> = Box::new(connector.connect().unwrap());
    let player = Session::<_, ExampleGame>::connect(
        connection,
        &Identity::from_secret([1; 32]),
        Mode::Playing,
        2,
        camera_at([8.0, 40.0, 8.0], 0.0, 0.0),
        || {
            server.step();
        },
    )
    .unwrap();
    let (lit, occluders) = crate::view::lit_atmosphere(&player);
    assert_eq!(lit, crate::view::atmosphere(player.fog_radius_bricks(), 0));
    assert!(occluders.is_empty());
    assert_eq!(
        (lit.sun_light, lit.sky_light, lit.side_direction),
        (0, 0, None)
    );
    // (A world with no sun has no shadows, whatever the player's setting.)
    assert!(!lit.shadows);
}

#[test]
fn a_game_with_no_sun_in_the_level_has_no_view_of_one() {
    // The example game says nothing about a sun at all.
    let content: ExampleContent = ExampleGame::new_content();
    let world = content.new_world(7);
    let plugins: Vec<Box<dyn Plugin>> = vec![Box::new(PlayersPlugin::new(true))];
    let (mut server, connector) =
        build_server_with_plugins::<ExampleGame>(&content, world, 2, plugins).unwrap();
    let connection: Box<dyn Connection> = Box::new(connector.connect().unwrap());
    let player = Session::<_, ExampleGame>::connect(
        connection,
        &Identity::from_secret([1; 32]),
        Mode::Playing,
        2,
        camera_at([8.0, 40.0, 8.0], 0.0, 0.0),
        || {
            server.step();
        },
    )
    .unwrap();
    assert_eq!(player.sun_view(), None);
    assert_eq!(player.settings().time_of_day, None);
}

#[test]
fn the_time_command_pins_the_sun_and_gives_it_back_to_the_clock() {
    let mut rig = Rig::new(0);
    let mut player = rig.join(1);
    let by_the_clock = player.sun_view().unwrap().hours;
    // It says what time it is, by the clock.
    let reply = player.client_command(".time");
    assert_eq!(reply, "it is 06:00 by the world's clock", "{reply}");

    assert_eq!(
        player.client_command(".time 12"),
        "the sun stays at 12:00; .time auto goes back to the clock of the world"
    );
    let pinned = player.sun_view().unwrap();
    assert_eq!(pinned.hours, 12.0);
    assert!(pinned.direction[1] > 0.999, "{:?}", pinned.direction);
    assert_eq!(pinned.daylight, 1.0);
    assert_eq!(
        player.client_command(".time"),
        "the sun is pinned at 12:00 (by the world's clock it is 06:00)"
    );

    // The pin does not stop the clock, and other forms of the time are taken.
    rig.run(&mut player, 1_000);
    assert_eq!(player.sun_view().unwrap().hours, 12.0);
    assert_eq!(
        player.client_command(".time 18:30"),
        "the sun stays at 18:30; .time auto goes back to the clock of the world"
    );
    assert_eq!(player.sun_view().unwrap().hours, 18.5);
    assert_eq!(
        player.client_command(".time midnight"),
        "the sun stays at 00:00; .time auto goes back to the clock of the world"
    );
    let midnight = player.sun_view().unwrap();
    assert_eq!(midnight.hours, 0.0);
    assert_eq!(midnight.daylight, 0.0, "flat ground: the sun is under it");
    assert_eq!(midnight.sky_color, [0.0; 3]);
    assert_eq!(
        player.client_command(".time noon"),
        "the sun stays at 12:00; .time auto goes back to the clock of the world"
    );

    assert_eq!(
        player.client_command(".time auto"),
        "the sun follows the clock of the world again; it is 07:00"
    );
    let after = player.sun_view().unwrap().hours;
    assert!(
        after > by_the_clock + 0.9 && after < by_the_clock + 1.1,
        "{by_the_clock} to {after}"
    );
    assert_eq!(player.settings().pinned_time, None);
}

#[test]
fn what_is_not_a_time_changes_nothing() {
    let mut rig = Rig::new(0);
    let mut player = rig.join(1);
    player.client_command(".time 9");
    for bad in [
        ".time 24",
        ".time 12:60",
        ".time -1",
        ".time lots",
        ".time 12:",
        ".time 7:5:3",
        ".time 1.5",
    ] {
        assert_eq!(
            player.client_command(bad),
            "usage: .time [HOURS[:MINUTES]|noon|midnight|auto]",
            "{bad}"
        );
        assert_eq!(player.settings().pinned_time, Some(9 * 60), "{bad}");
    }
}
