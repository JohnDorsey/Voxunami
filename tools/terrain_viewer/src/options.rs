use std::path::PathBuf;

use renderer::AdapterPreference;

pub const USAGE: &str = "\
terrain_viewer: fly through generated terrain, or render it headlessly.

Usage: terrain_viewer [options]
  --adapter discrete|integrated   which GPU to use (default: best available)
  --view-radius BRICKS            how far to load, in 32-voxel bricks (default 12)
  --seed NUMBER                   terrain seed (default 2024)
  --size WIDTHxHEIGHT             window or image size (default 1280x720)
  --camera X,Y,Z                  start position in voxels (default 16,110,16)
  --yaw DEGREES --pitch DEGREES   start orientation (default 0, -15)
  --screenshot FILE.png           render one image headlessly and exit
  --benchmark                     time offscreen frames headlessly and exit
  --no-vsync                      do not wait for the display refresh
  --exit-after SECONDS            close the window after this long (smoke test)
  --help                          show this text

Controls: click the window to capture the mouse (Esc releases it); W A S D move, Space and
Shift go up and down, Ctrl moves five times faster.";

#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub adapter: AdapterPreference,
    pub view_radius: i64,
    pub seed: u64,
    pub width: u32,
    pub height: u32,
    pub camera: [f64; 3],
    pub yaw_degrees: f32,
    pub pitch_degrees: f32,
    pub screenshot: Option<PathBuf>,
    pub benchmark: bool,
    pub vsync: bool,
    pub exit_after_seconds: Option<f32>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            adapter: AdapterPreference::HighPerformance,
            view_radius: 12,
            seed: 2024,
            width: 1280,
            height: 720,
            camera: [16.0, 110.0, 16.0],
            yaw_degrees: 0.0,
            pitch_degrees: -15.0,
            screenshot: None,
            benchmark: false,
            vsync: true,
            exit_after_seconds: None,
        }
    }
}

/// What the command line asked for.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Run(Options),
    ShowHelp,
}

fn parse_number<T: std::str::FromStr>(flag: &str, text: &str) -> Result<T, String> {
    text.parse()
        .map_err(|_| format!("{flag}: {text:?} is not a valid number"))
}

pub fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let mut options = Options::default();
    let mut arguments = arguments.into_iter();
    while let Some(flag) = arguments.next() {
        let mut value = |flag: &str| {
            arguments
                .next()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--help" | "-h" => return Ok(Command::ShowHelp),
            "--adapter" => {
                options.adapter = match value(&flag)?.as_str() {
                    "discrete" | "best" => AdapterPreference::HighPerformance,
                    "integrated" => AdapterPreference::Integrated,
                    other => return Err(format!("--adapter: unknown kind {other:?}")),
                };
            }
            "--view-radius" => {
                options.view_radius = parse_number(&flag, &value(&flag)?)?;
                if !(1..=64).contains(&options.view_radius) {
                    return Err("--view-radius must be between 1 and 64".to_owned());
                }
            }
            "--seed" => options.seed = parse_number(&flag, &value(&flag)?)?,
            "--size" => {
                let text = value(&flag)?;
                let (width, height) = text
                    .split_once('x')
                    .ok_or_else(|| format!("--size: {text:?} should look like 1280x720"))?;
                options.width = parse_number(&flag, width)?;
                options.height = parse_number(&flag, height)?;
                if options.width == 0 || options.height == 0 {
                    return Err("--size must be positive".to_owned());
                }
            }
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
            "--screenshot" => options.screenshot = Some(PathBuf::from(value(&flag)?)),
            "--benchmark" => options.benchmark = true,
            "--no-vsync" => options.vsync = false,
            "--exit-after" => {
                options.exit_after_seconds = Some(parse_number(&flag, &value(&flag)?)?)
            }
            other => return Err(format!("unknown option {other:?}; try --help")),
        }
    }
    Ok(Command::Run(options))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(text: &str) -> Result<Command, String> {
        parse(text.split_whitespace().map(str::to_owned))
    }

    fn options(text: &str) -> Options {
        match parse_args(text).unwrap() {
            Command::Run(options) => options,
            Command::ShowHelp => panic!("expected options"),
        }
    }

    #[test]
    fn no_arguments_gives_the_defaults() {
        assert_eq!(options(""), Options::default());
    }

    #[test]
    fn every_option_is_read() {
        let parsed = options(
            "--adapter integrated --view-radius 8 --seed 7 --size 640x360 --camera 1.5,2,-3 \
             --yaw 90 --pitch -45 --screenshot out.png --benchmark --no-vsync --exit-after 2.5",
        );
        assert_eq!(
            parsed,
            Options {
                adapter: AdapterPreference::Integrated,
                view_radius: 8,
                seed: 7,
                width: 640,
                height: 360,
                camera: [1.5, 2.0, -3.0],
                yaw_degrees: 90.0,
                pitch_degrees: -45.0,
                screenshot: Some(PathBuf::from("out.png")),
                benchmark: true,
                vsync: false,
                exit_after_seconds: Some(2.5),
            }
        );
    }

    #[test]
    fn help_wins() {
        assert_eq!(parse_args("--help").unwrap(), Command::ShowHelp);
        assert_eq!(parse_args("--seed 3 -h").unwrap(), Command::ShowHelp);
    }

    #[test]
    fn bad_input_is_reported_with_the_offending_flag() {
        let error = |text: &str| parse_args(text).unwrap_err();
        assert!(error("--bogus").contains("--bogus"));
        assert!(error("--seed").contains("--seed needs a value"));
        assert!(error("--seed abc").contains("--seed"));
        assert!(error("--adapter turbo").contains("turbo"));
        assert!(error("--size 100").contains("1280x720"));
        assert!(error("--size 0x100").contains("positive"));
        assert!(error("--camera 1,2").contains("--camera"));
        assert!(error("--view-radius 0").contains("between"));
        assert!(error("--view-radius 65").contains("between"));
    }
}
