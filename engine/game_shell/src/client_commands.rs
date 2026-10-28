//! Commands the window runs itself. They start with a period, as in other voxel games' chat, and
//! are never sent to the server: a line that starts with `/` is the server's, one that starts with
//! `.` is the window's.
//!
//! They change how this player's window shows the world, not the world, so each player has their own
//! and a recording does not hold them. The shell registers `.ambient` and `.help` here the way a client
//! plugin registers its own commands (see [`crate::client_plugin`]).

/// What the client commands change, and what a command can say about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
}

impl ClientSettings {
    /// Settings for a game whose own ambient light is `ambient_light`.
    pub fn new(ambient_light: u8) -> Self {
        Self {
            ambient_light,
            default_ambient_light: ambient_light,
            pinned_time: None,
            time_of_day: None,
        }
    }
}

/// Whether a line typed in the window is a client command rather than the server's.
pub fn is_client_command(line: &str) -> bool {
    line.starts_with('.')
}

use plugin_host::{Argument, CommandSpec};

use crate::client_plugin::ClientRegistrar;

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
            "unknown client command .dance; client commands: .ambient [LEVEL], .choose OPTION, .help [COMMAND], .keys, .time [TIME]"
        );
        assert_eq!(
            run(".", &mut settings),
            "client commands: .ambient [LEVEL], .choose OPTION, .help [COMMAND], .keys, .time [TIME]"
        );
        assert!(
            run(".help ambient", &mut settings).starts_with(".ambient [LEVEL]: sets how bright")
        );
        assert_eq!(
            run(".help nothing", &mut settings),
            "there is no client command .nothing; client commands: .ambient [LEVEL], .choose OPTION, .help [COMMAND], .keys, .time [TIME]"
        );
        // Spaces round the words do not matter.
        assert_eq!(
            run(".  ambient   12 ", &mut settings),
            "ambient light is now 12 of 255"
        );
    }
}
