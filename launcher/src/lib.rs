//! The Voxunami launcher: one program that starts any of the games, and opens a recording with the
//! game that made it.
//!
//! ```text
//! voxunami                      lists the games
//! voxunami cave [options]       plays Voxunami Cave
//! voxunami replay FILE [options]  watches a recording, in whichever game made it
//! ```
//!
//! Games are compile-time crates listed in [`GAMES`]; adding a game is adding a line there.

use std::path::Path;

use namespaced_id::NamespacedId;

type Command = fn(Vec<String>) -> Result<(), String>;

/// A game the launcher can start.
#[derive(Debug)]
pub struct Game {
    /// What to type after `voxunami`.
    pub command: &'static str,
    pub title: &'static str,
    /// The id recordings of the game carry in their welcome.
    pub id: fn() -> NamespacedId,
    pub play: Command,
    pub serve: Command,
    pub replay: Command,
}

pub const GAMES: &[Game] = &[
    Game {
        command: "cave",
        title: "Voxunami Cave: the Minecraft-like game",
        id: voxunami_cave::game_id,
        play: |arguments| voxunami_cave::run_play(arguments),
        serve: |arguments| voxunami_cave::run_serve(arguments),
        replay: |arguments| voxunami_cave::run_replay(arguments),
    },
    Game {
        command: "2048",
        title: "Voxunami 2048: 3D 2048",
        id: voxunami_2048::game_id,
        play: |arguments| voxunami_2048::run_play(arguments),
        serve: |arguments| voxunami_2048::run_serve(arguments),
        replay: |arguments| voxunami_2048::run_replay(arguments),
    },
];

fn usage() -> String {
    let mut text = String::from(
        "Voxunami launcher\n\nUsage:\n  voxunami GAME [options]         play a game (GAME --help for its options)\n  voxunami GAME --join HOST       play on someone else's server\n  voxunami serve GAME [options]   run a dedicated server for a game\n  voxunami replay FILE [options]  watch a recording, in the game that made it\n\nGames:\n",
    );
    for game in GAMES {
        text.push_str(&format!("  {:<8} {}\n", game.command, game.title));
    }
    text
}

/// Runs the launcher with the command line after the program name.
pub fn run(arguments: impl IntoIterator<Item = String>) -> Result<(), String> {
    let mut arguments: Vec<String> = arguments.into_iter().collect();
    if arguments.is_empty() {
        print!("{}", usage());
        return Ok(());
    }
    let command = arguments.remove(0);
    match command.as_str() {
        "help" | "--help" | "-h" => {
            print!("{}", usage());
            Ok(())
        }
        "replay" => replay(arguments),
        "serve" => serve(arguments),
        other => match GAMES.iter().find(|game| game.command == other) {
            Some(game) => (game.play)(arguments),
            None => Err(format!("unknown command {other:?}\n\n{}", usage())),
        },
    }
}

/// Runs a dedicated server for the game named first.
fn serve(mut arguments: Vec<String>) -> Result<(), String> {
    if arguments.is_empty() {
        return Err(format!(
            "which game to serve? usage: voxunami serve GAME [options]\n\n{}",
            usage()
        ));
    }
    let command = arguments.remove(0);
    match GAMES.iter().find(|game| game.command == command) {
        Some(game) => (game.serve)(arguments),
        None => Err(format!("unknown game {command:?}\n\n{}", usage())),
    }
}

/// Opens a recording with the game that made it, found from the recording itself.
fn replay(arguments: Vec<String>) -> Result<(), String> {
    let Some(file) = arguments
        .first()
        .filter(|argument| !argument.starts_with("--"))
    else {
        return Err("which recording? usage: voxunami replay FILE [options]".to_owned());
    };
    let welcome =
        replay::read_welcome(Path::new(file)).map_err(|error| format!("{file}: {error}"))?;
    match GAMES.iter().find(|game| (game.id)() == welcome.game) {
        Some(game) => (game.replay)(arguments),
        None => Err(format!(
            "{file} was recorded by {}, which this launcher does not have",
            welcome.game
        )),
    }
}

#[cfg(test)]
mod tests {
    use protocol::{ClientId, PROTOCOL_VERSION, ServerMessage, Tick, Welcome};
    use replay::ReplayWriter;

    use super::*;

    fn arguments(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_owned).collect()
    }

    fn recording_of(game: &str, name: &str) -> String {
        let welcome = ServerMessage::Welcome(Welcome {
            protocol_version: PROTOCOL_VERSION,
            game: game.parse().unwrap(),
            client: ClientId::new(1),
            ticks_per_second: 20,
            tick: Tick::ZERO,
            world_description: Vec::new(),
        });
        let path =
            std::env::temp_dir().join(format!("launcher_test_{}_{name}", std::process::id()));
        let mut writer = ReplayWriter::new(Vec::new()).unwrap();
        writer.append(&welcome.encode()).unwrap();
        std::fs::write(&path, writer.into_inner()).unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn no_command_and_help_list_the_games() {
        assert!(run(arguments("")).is_ok());
        assert!(run(arguments("help")).is_ok());
        let text = usage();
        for game in GAMES {
            assert!(text.contains(game.command) && text.contains(game.title));
        }
    }

    #[test]
    fn an_unknown_command_is_an_error_that_lists_the_games() {
        let error = run(arguments("pong")).unwrap_err();
        assert!(
            error.contains("\"pong\"") && error.contains("cave"),
            "{error}"
        );
    }

    #[test]
    fn a_game_command_reaches_the_game() {
        // The game's own help is printed and succeeds; a bad option reaches the game's parser.
        assert!(run(arguments("cave --help")).is_ok());
        let error = run(arguments("cave --bogus")).unwrap_err();
        assert!(
            error.contains("--bogus") && error.contains("Usage: voxunami cave"),
            "{error}"
        );
    }

    #[test]
    fn every_game_has_a_distinct_command_and_id_and_answers_help() {
        for game in GAMES {
            assert!(run(vec![game.command.to_owned(), "--help".to_owned()]).is_ok());
            let error = run(vec![game.command.to_owned(), "--bogus".to_owned()]).unwrap_err();
            assert!(error.contains("--bogus"), "{}: {error}", game.command);
        }
        let commands: std::collections::BTreeSet<_> = GAMES.iter().map(|g| g.command).collect();
        let ids: std::collections::BTreeSet<_> = GAMES.iter().map(|g| (g.id)()).collect();
        assert_eq!(commands.len(), GAMES.len());
        assert_eq!(ids.len(), GAMES.len());
    }

    #[test]
    fn serve_needs_a_game_that_exists_and_reaches_its_options() {
        assert!(run(arguments("serve")).unwrap_err().contains("which game"));
        assert!(run(arguments("serve pong")).unwrap_err().contains("pong"));
        for game in GAMES {
            let serve = |option: &str| {
                run(vec![
                    "serve".to_owned(),
                    game.command.to_owned(),
                    option.to_owned(),
                ])
            };
            assert!(serve("--help").is_ok());
            let error = serve("--bogus").unwrap_err();
            assert!(error.contains("--bogus"), "{error}");
        }
    }

    #[test]
    fn replay_needs_a_file_that_exists_and_a_game_the_launcher_has() {
        assert!(
            run(arguments("replay"))
                .unwrap_err()
                .contains("which recording")
        );
        assert!(
            run(arguments("replay --help"))
                .unwrap_err()
                .contains("which recording")
        );
        assert!(run(arguments("replay /no/such/file.voxreplay")).is_err());

        let foreign = recording_of("some_other_game:game", "foreign");
        let error = run(vec!["replay".to_owned(), foreign.clone()]).unwrap_err();
        assert!(error.contains("some_other_game:game"), "{error}");
        std::fs::remove_file(foreign).unwrap();
    }

    #[test]
    fn replay_hands_a_recording_to_the_game_that_made_it() {
        let ours = recording_of("voxunami_cave:game", "ours");
        // The game reads the file and gets as far as loading the world described in the welcome,
        // which this stub recording lacks: proof it was dispatched to the cave game.
        let error = run(vec![
            "replay".to_owned(),
            ours.clone(),
            "--screenshot".to_owned(),
            format!("{ours}.png"),
        ])
        .unwrap_err();
        assert!(error.contains("world description"), "{error}");
        std::fs::remove_file(ours).unwrap();
    }
}
