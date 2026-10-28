//! The command lines of a game: playing, and watching a recording.

use std::path::PathBuf;

use renderer::AdapterPreference;

use crate::definition::GameDefinition;

/// The help for playing `G`.
pub fn play_usage<G: GameDefinition>() -> String {
    let camera = G::default_camera();
    format!(
        "{title}: {summary}

Usage: voxunami {command} [options]
  --seed NUMBER                   terrain seed of a new world (default {seed})
  --world FILE                    load this world if it exists, and save to it on exit
  --record FILE                   record what you see, to watch later with `voxunami replay`
  --host [ADDRESS]                also let others join over the network (default port {port})
  --join ADDRESS                  join a game hosted elsewhere instead of starting one
  --packs DIRECTORY               load the data packs in this folder into the server this starts
  --identity FILE                 the key that says who you are to servers (default: made once, in the
                                  game data folder; keep it to stay the same player)
  --autosave SECONDS              save the world to --world this often while playing
  --view-radius BRICKS            how far to see, in 32-voxel bricks (default 12)
  --adapter discrete|integrated   which GPU to use (default: best available)
  --size WIDTHxHEIGHT             window or image size (default 1280x720)
  --camera X,Y,Z                  start position in voxels (default {x},{y},{z})
  --yaw DEGREES --pitch DEGREES   start orientation (default {yaw}, {pitch})
  --time TIME                     pin the sun at a time of day for you (noon, midnight, 18:30);
                                  by default it follows the world's clock
  --shadows on|off                shadows of the terrain (default on); .shadows changes it while playing
  --screenshot FILE.png           render one image without a window and exit
  --benchmark                     time frames without a window and exit
  --no-vsync                      do not wait for the display refresh
  --exit-after SECONDS            close the window after this long
  --help                          show this text

Controls: click the window to capture the mouse (Esc releases it). / starts typing a command for
the server and . one for the window, which the server never sees (Enter sends it, Esc drops it):
.ambient 0-255 sets how bright a face is that no light reaches (0 is black), .help lists them.
{controls}",
        title = G::title(),
        summary = G::summary(),
        command = G::command(),
        seed = G::default_seed(),
        x = camera.position[0],
        y = camera.position[1],
        z = camera.position[2],
        yaw = camera.yaw_degrees,
        pitch = camera.pitch_degrees,
        controls = G::controls(),
        port = crate::host::DEFAULT_PORT,
    )
}

/// The help for running a server with no window.
pub fn serve_usage<G: GameDefinition>() -> String {
    format!(
        "{title}: run a dedicated server.

Usage: voxunami serve {command} [options]
  --listen ADDRESS                where to accept players (default 0.0.0.0:{port})
  --seed NUMBER                   terrain seed of a new world (default {seed})
  --world FILE                    load this world if it exists, and save to it
  --packs DIRECTORY               load the data packs in this folder
  --autosave SECONDS              save the world this often (default 60 with --world)
  --view-radius BRICKS            the view radius players are expected to use (default 12)
  --ship-simulation-distance N    ships move while a player is within N voxels of them (default 256)
  --ship-unload-distance N        ships farther than N voxels from every player are put in files (default 1024;
                                  not less than the simulation distance)
  --ship-logout-policy POLICY     chair, always-stay or always-vanish: what a pilot's logout does to their ship
                                  (default chair: each ship does what it says)
  --help                          show this text

Players join with `voxunami {command} --join HOST[:PORT]`. The server saves on Ctrl-C.
There are no passwords or encryption: run it on networks you trust.",
        title = G::title(),
        command = G::command(),
        port = crate::host::DEFAULT_PORT,
        seed = G::default_seed(),
    )
}

/// The help for watching a recording.
pub fn replay_usage() -> String {
    "Watch a recording made with `voxunami GAME --record`.

Usage: voxunami replay FILE [options]
  --view-radius BRICKS            how far to see, in bricks (default 12; at most what was recorded)
  --adapter discrete|integrated   which GPU to use (default: best available)
  --size WIDTHxHEIGHT             window or image size (default 1280x720)
  --screenshot FILE.png           render the view at --tick (default: the end) and exit
  --tick NUMBER                   the tick to render with --screenshot
  --time TIME                     pin the sun at a time of day (noon, midnight, 18:30), not the recorded clock
  --shadows on|off                shadows of the terrain (default on)
  --no-vsync                      do not wait for the display refresh
  --exit-after SECONDS            close the window after this long
  --help                          show this text

The recording plays in real time, from the recorded player's point of view."
        .to_owned()
}

/// What both commands share.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewOptions {
    pub adapter: AdapterPreference,
    pub view_radius: i128,
    pub width: u32,
    pub height: u32,
    pub vsync: bool,
    pub exit_after_seconds: Option<f32>,
    pub screenshot: Option<PathBuf>,
    /// The time of day to pin the sun at, in minutes from midnight (`--time`), instead of following the world's clock.
    pub time: Option<u16>,
    /// Whether the terrain casts shadows (`--shadows`); on by default.
    pub shadows: bool,
}

impl Default for ViewOptions {
    fn default() -> Self {
        Self {
            adapter: AdapterPreference::HighPerformance,
            view_radius: 12,
            width: 1280,
            height: 720,
            vsync: true,
            exit_after_seconds: None,
            screenshot: None,
            time: None,
            shadows: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlayOptions {
    pub view: ViewOptions,
    pub seed: u64,
    pub world_file: Option<PathBuf>,
    pub record: Option<PathBuf>,
    /// Accept players over the network here, as well as the one at this computer.
    pub host: Option<String>,
    /// Join a server here instead of running one.
    pub join: Option<String>,
    /// Where the player's private key is kept: `None` for the file in the game data folder.
    pub identity: Option<PathBuf>,
    /// A directory of data packs for the server this runs.
    pub packs: Option<PathBuf>,
    pub autosave_seconds: Option<f32>,
    pub camera: [f64; 3],
    pub yaw_degrees: f32,
    pub pitch_degrees: f32,
    pub benchmark: bool,
}

impl PlayOptions {
    /// The options a command line without arguments gives for game `G`.
    pub fn defaults_for<G: GameDefinition>() -> Self {
        let camera = G::default_camera();
        Self {
            view: ViewOptions::default(),
            seed: G::default_seed(),
            world_file: None,
            record: None,
            host: None,
            join: None,
            identity: None,
            packs: None,
            autosave_seconds: None,
            camera: camera.position,
            yaw_degrees: camera.yaw_degrees,
            pitch_degrees: camera.pitch_degrees,
            benchmark: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReplayOptions {
    pub view: ViewOptions,
    pub file: PathBuf,
    pub tick: Option<u64>,
}

/// What a command line asked for.
#[derive(Clone, Debug, PartialEq)]
pub enum Parsed<T> {
    Run(T),
    ShowHelp,
}

fn parse_number<T: std::str::FromStr>(flag: &str, text: &str) -> Result<T, String> {
    text.parse()
        .map_err(|_| format!("{flag}: {text:?} is not a valid number"))
}

/// Reads the options both commands share. Returns false if `flag` is not one of them.
fn parse_view_flag(
    view: &mut ViewOptions,
    flag: &str,
    arguments: &mut impl Iterator<Item = String>,
) -> Result<bool, String> {
    let mut value = |flag: &str| {
        arguments
            .next()
            .ok_or_else(|| format!("{flag} needs a value"))
    };
    match flag {
        "--adapter" => {
            view.adapter = match value(flag)?.as_str() {
                "discrete" | "best" => AdapterPreference::HighPerformance,
                "integrated" => AdapterPreference::Integrated,
                other => return Err(format!("--adapter: unknown kind {other:?}")),
            };
        }
        "--view-radius" => {
            view.view_radius = parse_number(flag, &value(flag)?)?;
            if !(1..=16).contains(&view.view_radius) {
                return Err("--view-radius must be between 1 and 16".to_owned());
            }
        }
        "--size" => {
            let text = value(flag)?;
            let (width, height) = text
                .split_once('x')
                .ok_or_else(|| format!("--size: {text:?} should look like 1280x720"))?;
            view.width = parse_number(flag, width)?;
            view.height = parse_number(flag, height)?;
            if view.width == 0 || view.height == 0 {
                return Err("--size must be positive".to_owned());
            }
        }
        "--screenshot" => view.screenshot = Some(PathBuf::from(value(flag)?)),
        "--no-vsync" => view.vsync = false,
        "--exit-after" => view.exit_after_seconds = Some(parse_number(flag, &value(flag)?)?),
        "--time" => {
            let text = value(flag)?;
            view.time = Some(
                crate::client_commands::minutes_of_day(&text).ok_or_else(|| {
                    format!("--time: {text:?} should be noon, midnight, or a time like 7 or 18:30")
                })?,
            );
        }
        "--shadows" => {
            view.shadows = match value(flag)?.as_str() {
                "on" => true,
                "off" => false,
                other => return Err(format!("--shadows: {other:?} should be on or off")),
            };
        }
        _ => return Ok(false),
    }
    Ok(true)
}

pub fn parse_play<G: GameDefinition>(
    arguments: impl IntoIterator<Item = String>,
) -> Result<Parsed<PlayOptions>, String> {
    let mut options = PlayOptions::defaults_for::<G>();
    let mut arguments = arguments.into_iter();
    while let Some(flag) = arguments.next() {
        if flag == "--help" || flag == "-h" {
            return Ok(Parsed::ShowHelp);
        }
        if parse_view_flag(&mut options.view, &flag, &mut arguments)? {
            continue;
        }
        let mut value = |flag: &str| {
            arguments
                .next()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--seed" => options.seed = parse_number(&flag, &value(&flag)?)?,
            "--world" => options.world_file = Some(PathBuf::from(value(&flag)?)),
            "--record" => options.record = Some(PathBuf::from(value(&flag)?)),
            "--host" => options.host = Some(complete_address(&value(&flag)?, "0.0.0.0")),
            "--join" => options.join = Some(complete_address(&value(&flag)?, "127.0.0.1")),
            "--identity" => options.identity = Some(PathBuf::from(value(&flag)?)),
            "--packs" => options.packs = Some(PathBuf::from(value(&flag)?)),
            "--autosave" => options.autosave_seconds = Some(parse_number(&flag, &value(&flag)?)?),
            "--camera" => {
                let text = value(&flag)?;
                let parts: Vec<&str> = text.split(',').collect();
                let [x, y, z] = parts[..] else {
                    return Err(format!("--camera: {text:?} should look like 16,110,16"));
                };
                options.camera = [
                    parse_number(&flag, x)?,
                    parse_number(&flag, y)?,
                    parse_number(&flag, z)?,
                ];
            }
            "--yaw" => options.yaw_degrees = parse_number(&flag, &value(&flag)?)?,
            "--pitch" => options.pitch_degrees = parse_number(&flag, &value(&flag)?)?,
            "--benchmark" => options.benchmark = true,
            other => return Err(format!("unknown option {other:?}; try --help")),
        }
    }
    if options.join.is_some()
        && (options.host.is_some()
            || options.world_file.is_some()
            || options.packs.is_some()
            || options.autosave_seconds.is_some())
    {
        return Err("--join plays on someone else's server: it cannot be combined with --host, --world, --packs or --autosave".to_owned());
    }
    if options.autosave_seconds.is_some() && options.world_file.is_none() {
        return Err("--autosave needs --world to say where to save".to_owned());
    }
    if options
        .autosave_seconds
        .is_some_and(|seconds| seconds <= 0.0)
    {
        return Err("--autosave must be a positive number of seconds".to_owned());
    }
    Ok(Parsed::Run(options))
}

/// What a dedicated server was asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct ServeOptions {
    pub listen: String,
    pub seed: u64,
    pub world_file: Option<PathBuf>,
    /// A directory of data packs.
    pub packs: Option<PathBuf>,
    pub autosave_seconds: f32,
    pub view_radius: i128,
    /// How ships are kept.
    pub ships: server::ShipSettings,
}

pub fn parse_serve<G: GameDefinition>(
    arguments: impl IntoIterator<Item = String>,
) -> Result<Parsed<ServeOptions>, String> {
    let mut options = ServeOptions {
        listen: format!("0.0.0.0:{}", crate::host::DEFAULT_PORT),
        seed: G::default_seed(),
        world_file: None,
        packs: None,
        autosave_seconds: 60.0,
        view_radius: ViewOptions::default().view_radius,
        ships: server::ShipSettings::default(),
    };
    let mut arguments = arguments.into_iter();
    while let Some(flag) = arguments.next() {
        let mut value = |flag: &str| {
            arguments
                .next()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--help" | "-h" => return Ok(Parsed::ShowHelp),
            "--listen" => options.listen = complete_address(&value(&flag)?, "0.0.0.0"),
            "--seed" => options.seed = parse_number(&flag, &value(&flag)?)?,
            "--world" => options.world_file = Some(PathBuf::from(value(&flag)?)),
            "--packs" => options.packs = Some(PathBuf::from(value(&flag)?)),
            "--autosave" => {
                options.autosave_seconds = parse_number(&flag, &value(&flag)?)?;
                if options.autosave_seconds <= 0.0 {
                    return Err("--autosave must be a positive number of seconds".to_owned());
                }
            }
            "--view-radius" => {
                options.view_radius = parse_number(&flag, &value(&flag)?)?;
                if !(1..=16).contains(&options.view_radius) {
                    return Err("--view-radius must be between 1 and 16".to_owned());
                }
            }
            "--ship-simulation-distance" => {
                options.ships.simulation_distance = parse_number(&flag, &value(&flag)?)?;
            }
            "--ship-unload-distance" => {
                options.ships.unload_distance = parse_number(&flag, &value(&flag)?)?;
            }
            "--ship-logout-policy" => {
                let name = value(&flag)?;
                options.ships.logout_policy =
                    server::LogoutPolicy::from_name(&name).ok_or_else(|| {
                        format!(
                            "--ship-logout-policy must be chair, always-stay or always-vanish, not {name:?}"
                        )
                    })?;
            }
            other => return Err(format!("unknown option {other:?}; try --help")),
        }
    }
    // The two distances are a pair: one that does not agree with the other (or with the one it was not given) is refused.
    server::ShipSettings::new(
        options.ships.simulation_distance,
        options.ships.unload_distance,
    )
    .map_err(|error| {
        format!(
            "--ship-simulation-distance {} and --ship-unload-distance {}: {error}",
            options.ships.simulation_distance, options.ships.unload_distance
        )
    })?;
    Ok(Parsed::Run(options))
}

/// An address as typed, made complete: with the default port added if it has none
/// (`example.org` becomes `example.org:24816`), and a bare port number such as `1234` meaning port
/// 1234 on `host_for_bare_port`.
pub fn complete_address(text: &str, host_for_bare_port: &str) -> String {
    if let Ok(port) = text.parse::<u16>() {
        return format!("{host_for_bare_port}:{port}");
    }
    let has_port = match text.rsplit_once(':') {
        Some((host, port)) => {
            port.parse::<u16>().is_ok() && (!host.contains(':') || host.ends_with(']'))
        }
        None => false,
    };
    if has_port {
        text.to_owned()
    } else if text.contains(':') && !text.starts_with('[') {
        format!("[{text}]:{}", crate::host::DEFAULT_PORT) // a bare IPv6 address
    } else {
        format!("{text}:{}", crate::host::DEFAULT_PORT)
    }
}

pub fn parse_replay(
    arguments: impl IntoIterator<Item = String>,
) -> Result<Parsed<ReplayOptions>, String> {
    let mut view = ViewOptions::default();
    let mut file = None;
    let mut tick = None;
    let mut arguments = arguments.into_iter();
    while let Some(flag) = arguments.next() {
        if flag == "--help" || flag == "-h" {
            return Ok(Parsed::ShowHelp);
        }
        if parse_view_flag(&mut view, &flag, &mut arguments)? {
            continue;
        }
        match flag.as_str() {
            "--tick" => {
                let text = arguments
                    .next()
                    .ok_or_else(|| "--tick needs a value".to_owned())?;
                tick = Some(parse_number("--tick", &text)?);
            }
            other if other.starts_with("--") => {
                return Err(format!("unknown option {other:?}; try --help"));
            }
            path => {
                if file.replace(PathBuf::from(path)).is_some() {
                    return Err("only one recording can be watched at a time".to_owned());
                }
            }
        }
    }
    let file = file.ok_or_else(|| "which recording? give its file name".to_owned())?;
    Ok(Parsed::Run(ReplayOptions { view, file, tick }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ExampleGame;

    fn arguments(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_owned).collect()
    }

    fn play(text: &str) -> PlayOptions {
        match parse_play::<ExampleGame>(arguments(text)).unwrap() {
            Parsed::Run(options) => options,
            Parsed::ShowHelp => panic!("expected options"),
        }
    }

    #[test]
    fn no_arguments_gives_the_defaults() {
        assert_eq!(play(""), PlayOptions::defaults_for::<ExampleGame>());
    }

    #[test]
    fn every_play_option_is_read() {
        let parsed = play(
            "--adapter integrated --view-radius 8 --seed 7 --size 640x360 --camera 1.5,2,-3 \
             --yaw 90 --pitch -45 --screenshot out.png --benchmark --no-vsync --exit-after 2.5 \
             --world w.voxunami --record r.voxreplay --host 4000 --autosave 30 \n             --identity me.key --packs my_packs --time 18:30 --shadows off",
        );
        assert_eq!(
            parsed,
            PlayOptions {
                view: ViewOptions {
                    adapter: AdapterPreference::Integrated,
                    view_radius: 8,
                    width: 640,
                    height: 360,
                    vsync: false,
                    exit_after_seconds: Some(2.5),
                    screenshot: Some(PathBuf::from("out.png")),
                    time: Some(18 * 60 + 30),
                    shadows: false,
                },
                seed: 7,
                world_file: Some(PathBuf::from("w.voxunami")),
                record: Some(PathBuf::from("r.voxreplay")),
                host: Some("0.0.0.0:4000".to_owned()),
                join: None,
                identity: Some(PathBuf::from("me.key")),
                packs: Some(PathBuf::from("my_packs")),
                autosave_seconds: Some(30.0),
                camera: [1.5, 2.0, -3.0],
                yaw_degrees: 90.0,
                pitch_degrees: -45.0,
                benchmark: true,
            }
        );
    }

    #[test]
    fn shadows_are_on_unless_the_player_says_off_and_nothing_else_is_a_state() {
        assert!(play("").view.shadows);
        assert!(play("--shadows on").view.shadows);
        assert!(!play("--shadows off").view.shadows);
        for bad in ["maybe", "1", "ON", ""] {
            let error =
                parse_play::<ExampleGame>(arguments(&format!("--shadows {bad}"))).unwrap_err();
            assert!(error.contains("--shadows"), "{bad:?}: {error}");
        }
        assert!(
            parse_play::<ExampleGame>(arguments("--shadows"))
                .unwrap_err()
                .contains("--shadows")
        );
        // A recording can be watched without them too.
        let Parsed::Run(options) = parse_replay(arguments("r.voxreplay --shadows off")).unwrap()
        else {
            panic!("a replay");
        };
        assert!(!options.view.shadows);
    }

    #[test]
    fn the_time_of_day_is_noon_midnight_hours_or_hours_and_minutes_and_nothing_else() {
        assert_eq!(play("").view.time, None);
        assert_eq!(play("--time noon").view.time, Some(12 * 60));
        assert_eq!(play("--time midnight").view.time, Some(0));
        assert_eq!(play("--time 7").view.time, Some(7 * 60));
        assert_eq!(play("--time 07:05").view.time, Some(7 * 60 + 5));
        assert_eq!(play("--time 23:59").view.time, Some(23 * 60 + 59));
        for bad in ["24", "12:60", "lots", "-1", "12:", "auto"] {
            let error = parse_play::<ExampleGame>(arguments(&format!("--time {bad}"))).unwrap_err();
            assert!(error.contains("--time"), "{bad}: {error}");
        }
        assert!(
            parse_play::<ExampleGame>(arguments("--time"))
                .unwrap_err()
                .contains("needs a value")
        );
        // A recording can be watched at a time of day too.
        let Parsed::Run(options) = parse_replay(arguments("r.voxreplay --time 18:00")).unwrap()
        else {
            panic!("expected options")
        };
        assert_eq!(options.view.time, Some(18 * 60));
    }

    #[test]
    fn replay_needs_exactly_one_file_and_reads_its_options() {
        let Parsed::Run(options) = parse_replay(arguments(
            "session.voxreplay --tick 40 --screenshot a.png --size 320x200",
        ))
        .unwrap() else {
            panic!("expected options")
        };
        assert_eq!(options.file, PathBuf::from("session.voxreplay"));
        assert_eq!(options.tick, Some(40));
        assert_eq!(options.view.screenshot, Some(PathBuf::from("a.png")));
        assert_eq!((options.view.width, options.view.height), (320, 200));
        assert!(
            parse_replay(arguments("--tick 4"))
                .unwrap_err()
                .contains("which recording")
        );
        assert!(
            parse_replay(arguments("a b"))
                .unwrap_err()
                .contains("only one")
        );
    }

    #[test]
    fn help_wins() {
        assert_eq!(
            parse_play::<ExampleGame>(arguments("--help")).unwrap(),
            Parsed::ShowHelp
        );
        assert_eq!(
            parse_play::<ExampleGame>(arguments("--seed 3 -h")).unwrap(),
            Parsed::ShowHelp
        );
        assert_eq!(
            parse_replay(arguments("x --help")).unwrap(),
            Parsed::ShowHelp
        );
    }

    #[test]
    fn bad_input_is_reported_with_the_offending_flag() {
        let error = |text: &str| parse_play::<ExampleGame>(arguments(text)).unwrap_err();
        assert!(error("--bogus").contains("--bogus"));
        assert!(error("--seed").contains("--seed needs a value"));
        assert!(error("--seed abc").contains("--seed"));
        assert!(error("--adapter turbo").contains("turbo"));
        assert!(error("--size 100").contains("1280x720"));
        assert!(error("--size 0x100").contains("positive"));
        assert!(error("--camera 1,2").contains("--camera"));
        assert!(error("--view-radius 0").contains("between"));
        assert!(error("--view-radius 17").contains("between"));
        assert!(
            parse_replay(arguments("x --bogus"))
                .unwrap_err()
                .contains("--bogus")
        );
    }

    #[test]
    fn addresses_are_completed_the_way_people_type_them() {
        let local = |text: &str| complete_address(text, "127.0.0.1");
        assert_eq!(local("example.org"), "example.org:24816");
        assert_eq!(local("example.org:99"), "example.org:99");
        assert_eq!(local("192.168.1.5"), "192.168.1.5:24816");
        assert_eq!(local("192.168.1.5:7000"), "192.168.1.5:7000");
        assert_eq!(local("7000"), "127.0.0.1:7000");
        assert_eq!(complete_address("7000", "0.0.0.0"), "0.0.0.0:7000");
        assert_eq!(local("[::1]:7000"), "[::1]:7000");
        assert_eq!(local("::1"), "[::1]:24816");
        assert_eq!(local("localhost"), "localhost:24816");
    }

    #[test]
    fn joining_and_hosting_are_read_and_do_not_mix_with_what_only_a_host_does() {
        let joined = play("--join example.org");
        assert_eq!(joined.join.as_deref(), Some("example.org:24816"));
        assert_eq!(joined.host, None);
        let hosted = play("--host 0.0.0.0:5000 --world w --autosave 5");
        assert_eq!(hosted.host.as_deref(), Some("0.0.0.0:5000"));
        let error = |text: &str| parse_play::<ExampleGame>(arguments(text)).unwrap_err();
        for combined in [
            "--join a --host b",
            "--join a --world w",
            "--join a --autosave 1",
            "--join a --packs p",
        ] {
            assert!(error(combined).contains("--join"), "{combined}");
        }
        assert!(error("--autosave 5").contains("--world"));
        assert!(error("--world w --autosave 0").contains("positive"));
        assert!(error("--join").contains("needs a value"));
    }

    #[test]
    fn a_dedicated_server_reads_its_options() {
        let parse = |text: &str| parse_serve::<ExampleGame>(arguments(text));
        let Parsed::Run(defaults) = parse("").unwrap() else {
            panic!("expected options")
        };
        assert_eq!(defaults.listen, "0.0.0.0:24816");
        assert_eq!(defaults.seed, 2024);
        assert_eq!(defaults.autosave_seconds, 60.0);
        let Parsed::Run(options) = parse(
            "--listen 9000 --seed 5 --world w.voxunami --packs mods --autosave 10 --view-radius 6",
        )
        .unwrap() else {
            panic!("expected options")
        };
        assert_eq!(
            options,
            ServeOptions {
                listen: "0.0.0.0:9000".to_owned(),
                seed: 5,
                world_file: Some(PathBuf::from("w.voxunami")),
                packs: Some(PathBuf::from("mods")),
                autosave_seconds: 10.0,
                view_radius: 6,
                ships: server::ShipSettings::default(),
            }
        );
        assert_eq!(parse("--help").unwrap(), Parsed::ShowHelp);
        assert!(parse("--bogus").unwrap_err().contains("--bogus"));
        assert!(parse("--autosave -1").unwrap_err().contains("positive"));
        assert!(parse("--view-radius 99").unwrap_err().contains("between"));
        assert!(serve_usage::<ExampleGame>().contains("voxunami serve example"));
    }

    #[test]
    fn a_dedicated_server_is_told_how_to_keep_ships_and_refuses_distances_that_do_not_fit() {
        let parse =
            |text: &str| parse_serve::<ExampleGame>(text.split_whitespace().map(str::to_owned));
        let ships = |text: &str| match parse(text).unwrap() {
            Parsed::Run(options) => options.ships,
            Parsed::ShowHelp => panic!("expected options"),
        };
        assert_eq!(ships(""), server::ShipSettings::default());
        let set = ships(
            "--ship-simulation-distance 100 --ship-unload-distance 400 --ship-logout-policy always-vanish",
        );
        assert_eq!(set.simulation_distance, 100.0);
        assert_eq!(set.unload_distance, 400.0);
        assert_eq!(set.logout_policy, server::LogoutPolicy::AlwaysVanish);
        // One of the two, with the other what it was: each must still fit the other.
        assert_eq!(
            ships("--ship-simulation-distance 500").simulation_distance,
            500.0
        );
        let error = parse("--ship-simulation-distance 2000").unwrap_err();
        assert!(error.contains("2000") && error.contains("1024"), "{error}");
        assert!(
            parse("--ship-unload-distance 10")
                .unwrap_err()
                .contains("--ship-unload-distance 10")
        );
        assert!(parse("--ship-simulation-distance -5").is_err());
        assert!(parse("--ship-simulation-distance far").is_err());
        assert!(
            parse("--ship-logout-policy sometimes")
                .unwrap_err()
                .contains("always-stay")
        );
        let usage = serve_usage::<ExampleGame>();
        for flag in [
            "--ship-simulation-distance",
            "--ship-unload-distance",
            "--ship-logout-policy",
        ] {
            assert!(usage.contains(flag), "{flag}");
        }
    }

    #[test]
    fn the_usage_text_names_the_game_and_its_defaults() {
        let usage = play_usage::<ExampleGame>();
        assert!(usage.contains("Example: a game for tests"));
        assert!(usage.contains("voxunami example [options]"));
        assert!(usage.contains("(default 2024)"));
        assert!(usage.contains("(default 16,110,16)"));
        assert!(usage.contains("Nothing else."));
        assert!(replay_usage().contains("voxunami replay FILE"));
    }
}
