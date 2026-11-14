//! Commands the window runs itself. They start with a period, as in other voxel games' chat, and
//! are never sent to the server: a line that starts with `/` is the server's, one that starts with
//! `.` is the window's.
//!
//! They change how this player's window shows the world, not the world, so each player has their own
//! and a recording does not hold them. The shell registers `.ambient` and `.help` here the way a client
//! plugin registers its own commands (see [`crate::client_plugin`]).

/// What the client commands change, and what a command can say about it.
#[derive(Clone, Debug, PartialEq)]
pub struct ClientSettings {
    /// How bright, from 0 to 255, a face is drawn that no light reaches (see
    /// `renderer::Atmosphere::ambient_light`).
    pub ambient_light: u8,
    /// What it is when the game starts, and what `.ambient default` returns to.
    pub default_ambient_light: u8,
    /// The time of day the player has pinned the sun to, in minutes from midnight (`.time`), instead of the world's clock.
    pub pinned_time: Option<u16>,
    /// The time of day by the world's clock in the level the player is in, in minutes from midnight, if the game has a sun there. The session keeps it, for `.time` to say.
    pub time_of_day: Option<u16>,
    /// Whether the terrain casts shadows in the pictures (`.shadows`). On by default; it costs a second drawing of what is near the sun's way, so a slow GPU may want it off.
    pub shadows: bool,
    /// Whether each pixel of a block's picture is wholly in the light or wholly in a shadow (`.pixelshadows`, `renderer::VoxelRenderer::set_pixelated_shadows`). Off by default.
    pub pixelated_shadows: bool,
    /// How far from the camera, in voxels, a block's picture is drawn on it (`.textures`); beyond a quarter more the block is its flat colour. 0 draws no pictures.
    pub texture_distance: u32,
    /// The shader packs the player has (`.shader`), by id, and the id of the one chosen (`None`: the default). The session is told of the first when it starts; a pack the
    /// graphics card refuses is taken off the second by the window.
    pub shader_packs: Vec<ShaderPackName>,
    pub shader_pack: Option<String>,
    /// The values the player has chosen for the engine's shader settings (`.shadersetting`, `renderer::engine_settings`), by name; a setting that is not here is its default.
    pub shader_settings: renderer::SettingChoices,
}

impl ClientSettings {
    /// Settings for a game whose own ambient light is `ambient_light`.
    pub fn new(ambient_light: u8) -> Self {
        Self {
            ambient_light,
            default_ambient_light: ambient_light,
            pinned_time: None,
            time_of_day: None,
            shadows: true,
            pixelated_shadows: false,
            texture_distance: renderer::DEFAULT_PICTURE_DISTANCE as u32,
            shader_packs: Vec::new(),
            shader_pack: None,
            shader_settings: renderer::SettingChoices::new(),
        }
    }
}

/// The setting of these a player's text names: its name, or the beginning of one name only.
fn pick_setting<'a>(
    declarations: &'a [renderer::SettingDeclaration],
    text: &str,
) -> Result<&'a renderer::SettingDeclaration, String> {
    let lower = text.to_lowercase();
    let found = declarations
        .iter()
        .find(|declaration| declaration.name() == lower)
        .map(Ok)
        .unwrap_or_else(|| {
            let beginnings: Vec<_> = declarations
                .iter()
                .filter(|declaration| declaration.name().starts_with(&lower))
                .collect();
            match beginnings.as_slice() {
                [one] => Ok(*one),
                _ => Err(()),
            }
        });
    found.map_err(|()| {
        let names: Vec<&str> = declarations
            .iter()
            .map(renderer::SettingDeclaration::name)
            .collect();
        format!(
            "there is no shader setting called {text:?}; there are {}",
            names.join(", ")
        )
    })
}

/// What `.shadersetting` says with no name: each setting with its value, and its default where the player has changed it.
fn settings_listed(settings: &ClientSettings) -> String {
    let listed: Vec<String> = renderer::engine_settings()
        .iter()
        .map(|declaration| {
            let value = settings
                .shader_settings
                .get(declaration.name())
                .copied()
                .unwrap_or_else(|| declaration.default_value());
            if value == declaration.default_value() {
                format!("{} {value}", declaration.name())
            } else {
                format!(
                    "{} {value} (the default is {})",
                    declaration.name(),
                    declaration.default_value()
                )
            }
        })
        .collect();
    format!(
        "shader settings: {}; .shadersetting NAME says what one is for, .shadersetting NAME VALUE changes it",
        listed.join(", ")
    )
}

/// Whether a line typed in the window is a client command rather than the server's.
pub fn is_client_command(line: &str) -> bool {
    line.starts_with('.')
}

use plugin_host::{Argument, CommandSpec};

use crate::client_plugin::ClientRegistrar;
use crate::shader_packs::{ShaderPackName, pick};

/// A time of day in minutes from midnight as `07:05`.
fn clock_text(minutes: u16) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

/// The minutes from midnight of what a player typed: `noon`, `midnight`, hours (`7`) or hours and minutes (`7:05`), from 0:00 to 23:59.
pub(crate) fn minutes_of_day(text: &str) -> Option<u16> {
    match text {
        "noon" => return Some(12 * 60),
        "midnight" => return Some(0),
        _ => {}
    }
    let digits = |part: &str| {
        (!part.is_empty() && part.len() <= 2 && part.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| part.parse::<u16>().ok())
            .flatten()
    };
    let (hours, minutes) = match text.split_once(':') {
        Some((hours, minutes)) => (digits(hours)?, digits(minutes)?),
        None => (digits(text)?, 0),
    };
    (hours < 24 && minutes < 60).then_some(hours * 60 + minutes)
}

/// The shell's own client commands.
pub(crate) fn register(registrar: &mut ClientRegistrar<'_>) {
    registrar.command(
        CommandSpec::new(
            "help",
            "lists the client commands; .help NAME says what one does",
        )
        .with_prefix('.')
        .argument(Argument::word("command").optional()),
        // Answered by the host, which knows every command (see `ClientHost::command`).
        |_, _| String::new(),
    );
    registrar.command(
        CommandSpec::new("keys", "lists what the keys and mouse buttons do").with_prefix('.'),
        // Answered by the host, which knows every action.
        |_, _| String::new(),
    );
    registrar.command(
        CommandSpec::new(
            "ambient",
            "sets how bright a face is that no light reaches: 0 is black, 255 is as bright as the light of a lamp, and about 20 to 40 is dark like a cave in a game that is not; .ambient default goes back to the game's own",
        )
        .with_prefix('.')
        .argument(Argument::word("level").optional()),
        |context, arguments| {
            let settings = context.settings_mut();
            if !arguments.present(0) {
                return format!(
                    "ambient light is {} of 255 (the game's own is {})",
                    settings.ambient_light, settings.default_ambient_light
                );
            }
            match arguments.word(0) {
                "default" => {
                    settings.ambient_light = settings.default_ambient_light;
                    format!("ambient light is {}, the game's own", settings.ambient_light)
                }
                value => match value.parse::<u8>() {
                    Ok(value) => {
                        settings.ambient_light = value;
                        format!("ambient light is now {value} of 255")
                    }
                    Err(_) => "usage: .ambient [0-255|default]".to_owned(),
                },
            }
        },
    );
    registrar.command(
        CommandSpec::new(
            "time",
            "shows the time of day of the sun, which is the world's clock; .time 15:30 (or noon, or midnight) pins the sun at that time for you, and .time auto lets the clock move it again",
        )
        .with_prefix('.')
        .argument(Argument::word("time").optional()),
        |context, arguments| {
            let settings = context.settings_mut();
            if !arguments.present(0) {
                return match (settings.pinned_time, settings.time_of_day) {
                    (_, None) => "there is no sun in this level".to_owned(),
                    (None, Some(now)) => {
                        format!("it is {} by the world's clock", clock_text(now))
                    }
                    (Some(pinned), Some(now)) => format!(
                        "the sun is pinned at {} (by the world's clock it is {})",
                        clock_text(pinned),
                        clock_text(now)
                    ),
                };
            }
            if arguments.word(0) == "auto" {
                settings.pinned_time = None;
                return match settings.time_of_day {
                    Some(now) => format!(
                        "the sun follows the clock of the world again; it is {}",
                        clock_text(now)
                    ),
                    None => "the sun follows the clock of the world again".to_owned(),
                };
            }
            match minutes_of_day(arguments.word(0)) {
                Some(minutes) => {
                    settings.pinned_time = Some(minutes);
                    format!(
                        "the sun stays at {}; .time auto goes back to the clock of the world",
                        clock_text(minutes)
                    )
                }
                None => "usage: .time [HOURS[:MINUTES]|noon|midnight|auto]".to_owned(),
            }
        },
    );
    registrar.command(
        CommandSpec::new(
            "shadows",
            "turns the shadows of the terrain on or off for you (they are on): they cost a second drawing of what is near the sun's way, and a slow GPU may do better without; .shadows alone says which",
        )
        .with_prefix('.')
        .argument(Argument::word("on|off").optional()),
        |context, arguments| {
            let settings = context.settings_mut();
            if !arguments.present(0) {
                return format!("shadows are {}", if settings.shadows { "on" } else { "off" });
            }
            match arguments.word(0) {
                "on" => {
                    settings.shadows = true;
                    "shadows are now on".to_owned()
                }
                "off" => {
                    settings.shadows = false;
                    "shadows are now off".to_owned()
                }
                _ => "usage: .shadows [on|off]".to_owned(),
            }
        },
    );
    registrar.command(
        CommandSpec::new(
            "pixelshadows",
            "makes every pixel of a block's picture wholly in the light or wholly in a shadow, so that the edge of a shadow steps along the picture's pixels (it is off); exact up to about 28 voxels from you, and in the maps' own steps beyond; .pixelshadows alone says which",
        )
        .with_prefix('.')
        .argument(Argument::word("on|off").optional()),
        |context, arguments| {
            let settings = context.settings_mut();
            if !arguments.present(0) {
                return format!(
                    "pixelated shadows are {}",
                    if settings.pixelated_shadows { "on" } else { "off" }
                );
            }
            match arguments.word(0) {
                "on" => {
                    settings.pixelated_shadows = true;
                    "pixelated shadows are now on".to_owned()
                }
                "off" => {
                    settings.pixelated_shadows = false;
                    "pixelated shadows are now off".to_owned()
                }
                _ => "usage: .pixelshadows [on|off]".to_owned(),
            }
        },
    );
    registrar.command(
        CommandSpec::new(
            "textures",
            "sets how far away, in voxels, you see the pictures on blocks (the blocks of a farther place are their flat colour, with a fade between); .textures off draws no pictures, .textures default goes back to 64, and .textures alone says what it is",
        )
        .with_prefix('.')
        .argument(Argument::word("distance").optional()),
        |context, arguments| {
            let settings = context.settings_mut();
            if !arguments.present(0) {
                return match settings.texture_distance {
                    0 => "pictures on blocks are off".to_owned(),
                    distance => format!("pictures on blocks are drawn up to {distance} voxels away"),
                };
            }
            let distance = match arguments.word(0) {
                "off" => Some(0),
                "default" => Some(renderer::DEFAULT_PICTURE_DISTANCE as u32),
                value => value.parse::<u32>().ok(),
            };
            match distance {
                Some(0) => {
                    settings.texture_distance = 0;
                    "pictures on blocks are now off".to_owned()
                }
                Some(distance) => {
                    settings.texture_distance = distance;
                    format!("pictures on blocks are now drawn up to {distance} voxels away")
                }
                None => "usage: .textures [VOXELS|off|default]".to_owned(),
            }
        },
    );
    registrar.command(
        CommandSpec::new(
            "shader",
            "chooses the shader pack that makes the colour of every face (a folder of `shaderpacks` in the game data folder, or of --shader-packs): .shader NAME (its id or name, or the beginning of it), .shader default for the game's own, .shader alone says which is in use and what there is",
        )
        .with_prefix('.')
        .argument(Argument::word("pack").optional()),
        |context, arguments| {
            let settings = context.settings_mut();
            let listed = |settings: &ClientSettings| {
                if settings.shader_packs.is_empty() {
                    "there are no shader packs".to_owned()
                } else {
                    let names: Vec<String> = settings
                        .shader_packs
                        .iter()
                        .map(|pack| format!("{} ({})", pack.name, pack.id))
                        .collect();
                    format!("there are {}", names.join(", "))
                }
            };
            if !arguments.present(0) {
                let in_use = match &settings.shader_pack {
                    Some(id) => settings
                        .shader_packs
                        .iter()
                        .find(|pack| &pack.id == id)
                        .map_or_else(|| format!("the pack {id}"), |pack| format!("the pack {} ({})", pack.name, pack.id)),
                    None => "the game's own".to_owned(),
                };
                return format!("the shader is {in_use}; {}", listed(settings));
            }
            if arguments.word(0) == "default" {
                settings.shader_pack = None;
                return "the shader is the game's own".to_owned();
            }
            match pick(&settings.shader_packs, arguments.word(0)) {
                Ok(index) => {
                    let pack = settings.shader_packs[index].clone();
                    settings.shader_pack = Some(pack.id.clone());
                    format!("the shader is now the pack {} ({}); .shader default goes back", pack.name, pack.id)
                }
                Err(error) => error,
            }
        },
    );
    registrar.command(
        CommandSpec::new(
            "shadersetting",
            "changes a number of how the engine draws, by eye (how deep the relief of block pictures is, how dark the foot of a prism's wall is, how much brighter the darks of the whole scene are made): .shadersetting NAME VALUE (the beginning of a name will do), .shadersetting NAME default goes back, .shadersetting NAME says what one is for, .shadersetting alone lists them; a change makes the shaders again, a pause of a moment",
        )
        .with_prefix('.')
        .argument(Argument::word("setting").optional())
        .argument(Argument::word("value").optional()),
        |context, arguments| {
            let settings = context.settings_mut();
            if !arguments.present(0) {
                return settings_listed(settings);
            }
            let declarations = renderer::engine_settings();
            let declaration = match pick_setting(&declarations, arguments.word(0)) {
                Ok(declaration) => declaration,
                Err(error) => return error,
            };
            let name = declaration.name();
            let default = declaration.default_value();
            let current = settings
                .shader_settings
                .get(name)
                .copied()
                .unwrap_or(default);
            if !arguments.present(1) {
                return format!(
                    "{name} is {current}{}: {}; it takes {}",
                    if current == default { " (the default)" } else { "" },
                    declaration.description(),
                    declaration.kind().describe()
                );
            }
            let wanted = match arguments.word(1) {
                "default" => default,
                text => match declaration.kind().parse(text) {
                    Ok(value) => value,
                    Err(error) => return format!("{name}: {error}"),
                },
            };
            if wanted == default {
                settings.shader_settings.remove(name);
                return format!("{name} is {default}, the default");
            }
            settings.shader_settings.insert(name.to_owned(), wanted);
            format!(
                "{name} is now {wanted} (the default is {default}); .shadersetting {name} default goes back"
            )
        },
    );
    registrar.command(
        CommandSpec::new(
            "choose",
            "answers the question the game is asking: .choose 1 is the first option",
        )
        .with_prefix('.')
        .argument(Argument::word("option")),
        |context, arguments| {
            let Some(prompt) = context.prompt().cloned() else {
                return "there is no question to answer".to_owned();
            };
            match arguments.word(0).parse::<usize>() {
                Ok(number) if (1..=prompt.options.len()).contains(&number) => {
                    context.send_message(&plugin_host::PromptAnswer {
                        id: prompt.id,
                        choice: (number - 1) as u32,
                    });
                    format!("you chose {}", prompt.options[number - 1])
                }
                _ => format!("choose a number from 1 to {}", prompt.options.len()),
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use block_models::BlockTable;

    use super::*;
    use crate::client_plugin::{ClientContext, ClientHost, SessionView};
    use crate::session::Mode;
    use crate::view::camera_at;

    /// Runs a typed line the way the window does, against these settings.
    fn run(line: &str, settings: &mut ClientSettings) -> String {
        run_with_questions(line, settings, &[]).0
    }

    /// The same, with these questions waiting, and what the line sent to the server.
    fn run_with_questions(
        line: &str,
        settings: &mut ClientSettings,
        prompts: &[plugin_host::Prompt],
    ) -> (String, Vec<crate::client_plugin::Outgoing>) {
        let camera = camera_at([0.0; 3], 0.0, 0.0);
        let table = BlockTable::of_cubes(&[false]);
        let package = crate::package_data::PackageData::for_tests();
        let players = BTreeMap::new();
        let entities = client::ClientEntities::default();
        let mut host = ClientHost::new(&[]).expect("the shell's commands register");
        let mut context = ClientContext::new(SessionView {
            camera: &camera,
            block_table: &table,
            package: &package,
            players: &players,
            entities: &entities,
            prompts,
            settings,
            target: None,
            target_content: None,
            tick: protocol::Tick::ZERO,
            mode: Mode::Playing,
            own_client: protocol::ClientId::new(1),
        });
        let reply = host.command(line, &mut context);
        (reply, context.into_outgoing())
    }

    fn asked() -> Vec<plugin_host::Prompt> {
        vec![
            plugin_host::Prompt {
                id: plugin_host::PromptId::new(7),
                title: "Delete the level?".to_owned(),
                options: vec!["Keep it".to_owned(), "Delete it".to_owned()],
            },
            plugin_host::Prompt {
                id: plugin_host::PromptId::new(8),
                title: String::new(),
                options: vec!["Yes".to_owned()],
            },
        ]
    }

    fn settings_with_packs() -> ClientSettings {
        let mut settings = ClientSettings::new(0);
        settings.shader_packs = [("tester:cel", "Cel shading"), ("tester:flat", "Flat")]
            .map(|(id, name)| ShaderPackName {
                id: id.to_owned(),
                name: name.to_owned(),
            })
            .to_vec();
        settings
    }

    #[test]
    fn shader_says_which_pack_is_in_use_and_what_there_is_chooses_by_name_and_goes_back_to_the_default()
     {
        let mut settings = settings_with_packs();
        assert_eq!(
            run(".shader", &mut settings),
            "the shader is the game's own; there are Cel shading (tester:cel), Flat (tester:flat)"
        );
        assert_eq!(
            run(".shader cel", &mut settings),
            "the shader is now the pack Cel shading (tester:cel); .shader default goes back"
        );
        assert_eq!(settings.shader_pack.as_deref(), Some("tester:cel"));
        assert_eq!(
            run(".shader", &mut settings),
            "the shader is the pack Cel shading (tester:cel); there are Cel shading (tester:cel), Flat (tester:flat)"
        );
        run(".shader FLAT", &mut settings);
        assert_eq!(settings.shader_pack.as_deref(), Some("tester:flat"));
        // What a player types is often the part of the id after the colon.
        run(".shader cel", &mut settings);
        assert_eq!(settings.shader_pack.as_deref(), Some("tester:cel"));
        run(".shader flat", &mut settings);
        assert_eq!(
            run(".shader default", &mut settings),
            "the shader is the game's own"
        );
        assert_eq!(settings.shader_pack, None);
    }

    #[test]
    fn shader_with_a_name_that_is_no_pack_or_is_two_changes_nothing_and_says_what_there_is() {
        let mut settings = settings_with_packs();
        run(".shader flat", &mut settings);
        let reply = run(".shader nothing", &mut settings);
        assert!(
            reply.contains("there is no shader pack called")
                && reply.contains("Cel shading (tester:cel)"),
            "{reply}"
        );
        let reply = run(".shader t", &mut settings);
        assert!(reply.contains("could be any of"), "{reply}");
        assert_eq!(
            settings.shader_pack.as_deref(),
            Some("tester:flat"),
            "nothing changed"
        );
        let mut none = ClientSettings::new(0);
        assert_eq!(
            run(".shader", &mut none),
            "the shader is the game's own; there are no shader packs"
        );
        assert!(run(".shader any", &mut none).contains("there are none"));
    }

    #[test]
    fn shadersetting_lists_the_settings_with_what_they_are_and_says_what_one_is_for() {
        let mut settings = ClientSettings::new(0);
        assert_eq!(
            run(".shadersetting", &mut settings),
            "shader settings: relief_depth 0.1, wall_darkness 0.2, scene_lift 1.2; \
             .shadersetting NAME says what one is for, .shadersetting NAME VALUE changes it"
        );
        let reply = run(".shadersetting scene_lift", &mut settings);
        assert!(
            reply.starts_with("scene_lift is 1.2 (the default)")
                && reply.contains("a number from 0.5 to 3")
                && reply.contains("black stays black"),
            "{reply}"
        );
        assert!(
            settings.shader_settings.is_empty(),
            "asking changes nothing"
        );
    }

    #[test]
    fn shadersetting_sets_a_value_by_name_or_the_beginning_of_it_and_goes_back_to_the_default() {
        use renderer::SettingValue;
        let mut settings = ClientSettings::new(0);
        assert_eq!(
            run(".shadersetting scene_lift 1.5", &mut settings),
            "scene_lift is now 1.5 (the default is 1.2); .shadersetting scene_lift default goes back"
        );
        assert_eq!(
            settings.shader_settings.get("scene_lift"),
            Some(&SettingValue::Float(1.5))
        );
        assert_eq!(
            run(".shadersetting", &mut settings),
            "shader settings: relief_depth 0.1, wall_darkness 0.2, scene_lift 1.5 (the default is 1.2); \
             .shadersetting NAME says what one is for, .shadersetting NAME VALUE changes it"
        );
        // The beginning of a name is a name if it is the beginning of one only.
        run(".shadersetting wall 0.75", &mut settings);
        assert_eq!(
            settings.shader_settings.get("wall_darkness"),
            Some(&SettingValue::Float(0.75))
        );
        // The default, as a number or as a word, is no choice at all.
        assert_eq!(
            run(".shadersetting scene_lift 1.2", &mut settings),
            "scene_lift is 1.2, the default"
        );
        assert!(!settings.shader_settings.contains_key("scene_lift"));
        run(".shadersetting wall_darkness default", &mut settings);
        assert!(settings.shader_settings.is_empty());
    }

    #[test]
    fn shadersetting_with_a_bad_name_or_value_changes_nothing_and_says_what_there_is() {
        let mut settings = ClientSettings::new(0);
        run(".shadersetting relief_depth 0.15", &mut settings);
        let before = settings.shader_settings.clone();
        let reply = run(".shadersetting brightness 1", &mut settings);
        assert!(
            reply.contains("there is no shader setting called \"brightness\"")
                && reply.contains("relief_depth, wall_darkness, scene_lift"),
            "{reply}"
        );
        for text in [
            ".shadersetting relief_depth 9",
            ".shadersetting relief_depth deep",
            ".shadersetting relief_depth -1",
        ] {
            let reply = run(text, &mut settings);
            assert!(
                reply.contains("relief_depth") && reply.contains("a number from 0 to 0.2"),
                "{text}: {reply}"
            );
        }
        assert_eq!(settings.shader_settings, before, "nothing changed");
    }

    #[test]
    fn choose_answers_the_oldest_question_with_the_option_numbered_from_one() {
        use plugin_host::{PromptAnswer, PromptId};
        let mut settings = ClientSettings::new(0);
        let (reply, sent) = run_with_questions(".choose 2", &mut settings, &asked());
        assert_eq!(reply, "you chose Delete it");
        assert_eq!(
            sent,
            [crate::client_plugin::Outgoing::Message(
                <PromptAnswer as protocol::Message>::kind(),
                protocol::Message::encode(&PromptAnswer {
                    id: PromptId::new(7),
                    choice: 1
                })
            )]
        );
        let (reply, sent) = run_with_questions(".choose 1", &mut settings, &asked());
        assert_eq!(reply, "you chose Keep it");
        assert_eq!(sent.len(), 1);
    }

    #[test]
    fn choose_says_what_is_wrong_and_sends_nothing_when_there_is_no_question_or_no_such_option() {
        let mut settings = ClientSettings::new(0);
        let (reply, sent) = run_with_questions(".choose 1", &mut settings, &[]);
        assert_eq!(reply, "there is no question to answer");
        assert!(sent.is_empty());
        for wrong in [
            ".choose 0",
            ".choose 3",
            ".choose -1",
            ".choose two",
            ".choose 1.5",
        ] {
            let (reply, sent) = run_with_questions(wrong, &mut settings, &asked());
            assert_eq!(reply, "choose a number from 1 to 2", "{wrong}");
            assert!(sent.is_empty(), "{wrong}");
        }
        let (reply, sent) = run_with_questions(".choose", &mut settings, &asked());
        assert_eq!(reply, "usage: .choose OPTION");
        assert!(sent.is_empty());
    }

    #[test]
    fn a_line_is_the_clients_if_it_starts_with_a_period_and_the_servers_otherwise() {
        assert!(is_client_command(".ambient 20"));
        assert!(is_client_command("."));
        assert!(!is_client_command("/ambient 20"));
        assert!(!is_client_command("ambient"));
        assert!(!is_client_command(" .ambient"));
        assert!(!is_client_command(""));
    }

    #[test]
    fn shadows_are_on_to_begin_with_and_can_be_turned_off_and_on_and_nothing_else() {
        let mut settings = ClientSettings::new(0);
        assert!(settings.shadows);
        assert_eq!(run(".shadows", &mut settings), "shadows are on");
        assert_eq!(run(".shadows off", &mut settings), "shadows are now off");
        assert!(!settings.shadows);
        assert_eq!(run(".shadows", &mut settings), "shadows are off");
        assert_eq!(run(".shadows on", &mut settings), "shadows are now on");
        assert!(settings.shadows);
        // What is neither changes nothing.
        for bad in [".shadows maybe", ".shadows 1", ".shadows ON"] {
            settings.shadows = false;
            assert_eq!(run(bad, &mut settings), "usage: .shadows [on|off]", "{bad}");
            assert!(!settings.shadows, "{bad}");
        }
        // (More than one word is the usage the shell itself gives.)
        settings.shadows = false;
        assert_eq!(
            run(".shadows off on", &mut settings),
            "usage: .shadows [ON|OFF]"
        );
        assert!(!settings.shadows);
    }

    #[test]
    fn pixel_shadows_are_off_to_begin_with_and_can_be_turned_on_and_off_and_nothing_else() {
        let mut settings = ClientSettings::new(0);
        assert!(!settings.pixelated_shadows);
        assert_eq!(
            run(".pixelshadows", &mut settings),
            "pixelated shadows are off"
        );
        assert_eq!(
            run(".pixelshadows on", &mut settings),
            "pixelated shadows are now on"
        );
        assert!(settings.pixelated_shadows);
        assert_eq!(
            run(".pixelshadows", &mut settings),
            "pixelated shadows are on"
        );
        assert_eq!(
            run(".pixelshadows off", &mut settings),
            "pixelated shadows are now off"
        );
        assert!(!settings.pixelated_shadows);
        // What is neither changes nothing.
        for bad in [".pixelshadows maybe", ".pixelshadows 1", ".pixelshadows ON"] {
            settings.pixelated_shadows = true;
            assert_eq!(
                run(bad, &mut settings),
                "usage: .pixelshadows [on|off]",
                "{bad}"
            );
            assert!(settings.pixelated_shadows, "{bad}");
        }
    }

    #[test]
    fn textures_say_set_turn_off_and_reset_how_far_the_pictures_on_blocks_are_drawn() {
        let mut settings = ClientSettings::new(0);
        assert_eq!(settings.texture_distance, 64);
        assert_eq!(
            run(".textures", &mut settings),
            "pictures on blocks are drawn up to 64 voxels away"
        );
        assert_eq!(
            run(".textures 20", &mut settings),
            "pictures on blocks are now drawn up to 20 voxels away"
        );
        assert_eq!(settings.texture_distance, 20);
        assert_eq!(
            run(".textures", &mut settings),
            "pictures on blocks are drawn up to 20 voxels away"
        );
        assert_eq!(
            run(".textures off", &mut settings),
            "pictures on blocks are now off"
        );
        assert_eq!(settings.texture_distance, 0);
        assert_eq!(
            run(".textures", &mut settings),
            "pictures on blocks are off"
        );
        run(".textures 300", &mut settings);
        assert_eq!(settings.texture_distance, 300);
        // 0 is the same as off.
        assert_eq!(
            run(".textures 0", &mut settings),
            "pictures on blocks are now off"
        );
        assert_eq!(settings.texture_distance, 0);
        assert_eq!(
            run(".textures default", &mut settings),
            "pictures on blocks are now drawn up to 64 voxels away"
        );
        assert_eq!(settings.texture_distance, 64);
        // What is not a distance changes nothing.
        for bad in [
            ".textures -1",
            ".textures far",
            ".textures 1.5",
            ".textures 99999999999",
        ] {
            assert_eq!(
                run(bad, &mut settings),
                "usage: .textures [VOXELS|off|default]",
                "{bad}"
            );
            assert_eq!(settings.texture_distance, 64, "{bad}");
        }
    }

    #[test]
    fn ambient_shows_sets_and_resets_how_bright_unlit_faces_are() {
        let mut settings = ClientSettings::new(0);
        assert_eq!(
            run(".ambient", &mut settings),
            "ambient light is 0 of 255 (the game's own is 0)"
        );
        assert_eq!(
            run(".ambient 30", &mut settings),
            "ambient light is now 30 of 255"
        );
        assert_eq!(settings.ambient_light, 30);
        assert_eq!(
            run(".ambient", &mut settings),
            "ambient light is 30 of 255 (the game's own is 0)"
        );
        assert_eq!(
            run(".ambient 255", &mut settings),
            "ambient light is now 255 of 255"
        );
        assert_eq!(settings.ambient_light, 255);
        assert_eq!(
            run(".ambient 0", &mut settings),
            "ambient light is now 0 of 255"
        );
        assert_eq!(settings.ambient_light, 0);
        run(".ambient 77", &mut settings);
        assert_eq!(
            run(".ambient default", &mut settings),
            "ambient light is 0, the game's own"
        );
        assert_eq!(settings.ambient_light, 0);
        // A game with a lit day has another default to go back to.
        let mut lit = ClientSettings::new(160);
        run(".ambient 5", &mut lit);
        run(".ambient default", &mut lit);
        assert_eq!(lit.ambient_light, 160);
    }

    #[test]
    fn what_is_not_a_level_changes_nothing() {
        let mut settings = ClientSettings::new(40);
        for bad in [
            ".ambient 256",
            ".ambient -1",
            ".ambient 1.5",
            ".ambient lots",
        ] {
            assert_eq!(
                run(bad, &mut settings),
                "usage: .ambient [0-255|default]",
                "{bad}"
            );
            assert_eq!(settings.ambient_light, 40, "{bad}");
        }
    }

    #[test]
    fn more_words_than_a_command_takes_is_answered_with_its_usage() {
        let mut settings = ClientSettings::new(40);
        for bad in [".ambient 10 20", ".ambient default 5"] {
            assert_eq!(run(bad, &mut settings), "usage: .ambient [LEVEL]", "{bad}");
            assert_eq!(settings.ambient_light, 40, "{bad}");
        }
    }

    #[test]
    fn help_and_unknown_commands_say_what_there_is() {
        let mut settings = ClientSettings::new(0);
        assert!(run(".help", &mut settings).starts_with("client commands: .ambient"));
        assert_eq!(
            run(".dance", &mut settings),
            "unknown client command .dance; client commands: .ambient [LEVEL], .choose OPTION, .help [COMMAND], .keys, .pixelshadows [ON|OFF], .shader [PACK], .shadersetting [SETTING] [VALUE], .shadows [ON|OFF], .textures [DISTANCE], .time [TIME]"
        );
        assert_eq!(
            run(".", &mut settings),
            "client commands: .ambient [LEVEL], .choose OPTION, .help [COMMAND], .keys, .pixelshadows [ON|OFF], .shader [PACK], .shadersetting [SETTING] [VALUE], .shadows [ON|OFF], .textures [DISTANCE], .time [TIME]"
        );
        assert!(
            run(".help ambient", &mut settings).starts_with(".ambient [LEVEL]: sets how bright")
        );
        assert_eq!(
            run(".help nothing", &mut settings),
            "there is no client command .nothing; client commands: .ambient [LEVEL], .choose OPTION, .help [COMMAND], .keys, .pixelshadows [ON|OFF], .shader [PACK], .shadersetting [SETTING] [VALUE], .shadows [ON|OFF], .textures [DISTANCE], .time [TIME]"
        );
        // Spaces round the words do not matter.
        assert_eq!(
            run(".  ambient   12 ", &mut settings),
            "ambient light is now 12 of 255"
        );
    }
}
