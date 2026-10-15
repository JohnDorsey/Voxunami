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
  --view-radius BRICKS            how far to see, in 32-voxel bricks (default 12)
  --adapter discrete|integrated   which GPU to use (default: best available)
  --size WIDTHxHEIGHT             window or image size (default 1280x720)
  --camera X,Y,Z                  start position in voxels (default {x},{y},{z})
  --yaw DEGREES --pitch DEGREES   start orientation (default {yaw}, {pitch})
  --screenshot FILE.png           render one image without a window and exit
  --benchmark                     time frames without a window and exit
  --no-vsync                      do not wait for the display refresh
  --exit-after SECONDS            close the window after this long
  --help                          show this text

Controls: click the window to capture the mouse (Esc releases it). W A S D move, Space and Shift
go up and down, Ctrl moves five times faster.
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
    pub view_radius: i64,
    pub width: u32,
    pub height: u32,
    pub vsync: bool,
    pub exit_after_seconds: Option<f32>,
    pub screenshot: Option<PathBuf>,
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
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlayOptions {
    pub view: ViewOptions,
    pub seed: u64,
    pub world_file: Option<PathBuf>,
    pub record: Option<PathBuf>,
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
    Ok(Parsed::Run(options))
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
             --world w.voxunami --record r.voxreplay",
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
                },
                seed: 7,
                world_file: Some(PathBuf::from("w.voxunami")),
                record: Some(PathBuf::from("r.voxreplay")),
                camera: [1.5, 2.0, -3.0],
                yaw_degrees: 90.0,
                pitch_degrees: -45.0,
                benchmark: true,
            }
        );
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
