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
}

impl ClientSettings {
    /// Settings for a game whose own ambient light is `ambient_light`.
    pub fn new(ambient_light: u8) -> Self {
        Self {
            ambient_light,
            default_ambient_light: ambient_light,
        }
    }
}

/// Whether a line typed in the window is a client command rather than the server's.
pub fn is_client_command(line: &str) -> bool {
    line.starts_with('.')
}

use plugin_host::{Argument, CommandSpec};

use crate::client_plugin::ClientRegistrar;

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
        let camera = camera_at([0.0; 3], 0.0, 0.0, 3);
        let table = BlockTable::of_cubes(&[false]);
        let players = BTreeMap::new();
        let mut host = ClientHost::new(&[]).expect("the shell's commands register");
        let mut context = ClientContext::new(SessionView {
            camera: &camera,
            block_table: &table,
            players: &players,
            settings,
            target: None,
            target_content: None,
            tick: protocol::Tick::ZERO,
            mode: Mode::Playing,
            own_client: protocol::ClientId::new(1),
        });
        host.command(line, &mut context)
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
            "unknown client command .dance; client commands: .ambient [LEVEL], .help [COMMAND], .keys"
        );
        assert_eq!(
            run(".", &mut settings),
            "client commands: .ambient [LEVEL], .help [COMMAND], .keys"
        );
        assert!(
            run(".help ambient", &mut settings).starts_with(".ambient [LEVEL]: sets how bright")
        );
        assert_eq!(
            run(".help nothing", &mut settings),
            "there is no client command .nothing; client commands: .ambient [LEVEL], .help [COMMAND], .keys"
        );
        // Spaces round the words do not matter.
        assert_eq!(
            run(".  ambient   12 ", &mut settings),
            "ambient light is now 12 of 255"
        );
    }
}
