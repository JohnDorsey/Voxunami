//! A question the server asks a player, shown on their screen, answered with `.choose`, and seen again in a
//! replay.

mod common;

use std::sync::{Arc, Mutex};

use common::{Player, Rig, SEED, VIEW_RADIUS};
use game_shell::GameContent;
use game_shell::Identity;
use game_shell::files::{open_recording, record_connection};
use game_shell::session::{Mode, Session};
use game_shell::view::camera_at;
use namespaced_id::NamespacedId;
use plugin_host::{CommandSpec, HandlerSpec, Plugin, PromptAnswered, Registrar};
use transport::Connection;
use voxunami_cave::content::CaveContent;
use voxunami_cave::game::Cave;
use voxunami_cave::plugins::cave_plugins;

type Answers = Arc<Mutex<Vec<PromptAnswered>>>;

/// A plugin that asks the caller whether to delete a level when they say `/ask`, and remembers the answers.
#[derive(Debug)]
struct Asker {
    answers: Answers,
}

impl Plugin for Asker {
    fn id(&self) -> NamespacedId {
        "test:asker".parse().unwrap()
    }

    fn register(&self, registrar: &mut Registrar<'_>) {
        registrar.command(
            CommandSpec::new("ask", "asks a question"),
            |context, client, _| {
                let _ = context.prompt(client, "Delete the level?", &["Keep it", "Delete it"]);
            },
        );
        let answers = Arc::clone(&self.answers);
        registrar.on::<PromptAnswered>(HandlerSpec::new("test:hear_answers"), move |event, _| {
            answers.lock().unwrap().push(*event)
        });
    }
}

fn rig_with_asker() -> (Rig, Answers) {
    let content = CaveContent::new();
    let world = content.new_world(SEED);
    let answers = Answers::default();
    let mut plugins = cave_plugins(&content);
    plugins.push(Box::new(Asker {
        answers: Arc::clone(&answers),
    }));
    (Rig::with_plugins(content, world, plugins), answers)
}

fn tick(rig: &mut Rig, player: &mut Player, count: usize) {
    for _ in 0..count {
        rig.step();
        player.advance().unwrap();
    }
}

const QUESTION: &str = "Delete the level? 1) Keep it  2) Delete it  (.choose N)";

#[test]
fn a_question_is_on_the_screen_until_it_is_answered_with_choose_and_the_plugin_hears_which() {
    let (mut rig, answers) = rig_with_asker();
    let mut player = rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    assert!(!player.hud_text().contains("Delete"), "no question yet");

    player.send_command("/ask");
    tick(&mut rig, &mut player, 2);
    assert!(
        player.hud_text().starts_with(QUESTION),
        "{}",
        player.hud_text()
    );

    // A wrong number says so, and the question stays.
    assert_eq!(
        player.client_command(".choose 3"),
        "choose a number from 1 to 2"
    );
    tick(&mut rig, &mut player, 2);
    assert!(player.hud_text().starts_with(QUESTION));
    assert_eq!(*answers.lock().unwrap(), []);

    // The right one is answered, the plugin hears it, and the screen forgets it.
    assert_eq!(player.client_command(".choose 2"), "you chose Delete it");
    tick(&mut rig, &mut player, 2);
    let heard = answers.lock().unwrap().clone();
    assert_eq!(heard.len(), 1, "{heard:?}");
    assert_eq!(heard[0].choice, 1);
    assert!(
        !player.hud_text().contains("Delete"),
        "{}",
        player.hud_text()
    );
    assert_eq!(
        player.client_command(".choose 1"),
        "there is no question to answer"
    );
}

#[test]
fn a_replay_shows_the_question_while_it_was_asked_and_not_after() {
    let directory =
        std::env::temp_dir().join(format!("voxunami_cave_prompt_{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("session.voxreplay");
    let (mut rig, _) = rig_with_asker();
    let recorded = record_connection(rig.connector.connect().unwrap(), &path).unwrap();
    let mut player = rig.join_with(recorded, [16.5, 110.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    player.send_command("/ask");
    tick(&mut rig, &mut player, 3);
    player.client_command(".choose 1");
    tick(&mut rig, &mut player, 3);
    drop(player);

    let connection: Box<dyn Connection> = Box::new(open_recording(&path).unwrap());
    let mut replay = Session::<_, Cave>::connect(
        connection,
        &Identity::generate(),
        Mode::Replaying,
        VIEW_RADIUS,
        camera_at([0.0; 3], 0.0, 0.0),
        || {},
    )
    .unwrap();
    // One tick at a time: the question appears, and then closes.
    let mut shown = Vec::new();
    while replay.advance_one_tick().is_ok() {
        shown.push(replay.hud_text().contains("Delete the level?"));
    }
    let first = shown.iter().position(|shown| *shown).expect("it was shown");
    let last = shown.iter().rposition(|shown| *shown).unwrap();
    assert!(
        shown[first..=last].iter().all(|shown| *shown),
        "shown for as long as it was asked"
    );
    assert!(last < shown.len() - 1, "and gone before the end");
    assert!(!shown[..first].iter().any(|shown| *shown));
    std::fs::remove_dir_all(&directory).unwrap();
}
