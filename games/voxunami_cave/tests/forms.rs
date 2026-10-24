//! A form the server shows a player: it reaches their session, changes and closes with the stream, what the player does with it
//! reaches the plugin that showed it, and a replay shows it and closes it at the same tick.

mod common;

use std::sync::{Arc, Mutex};

use common::{Player, Rig, SEED, VIEW_RADIUS};
use game_shell::Identity;
use game_shell::files::{open_recording, record_connection};
use game_shell::session::{Mode, Session};
use game_shell::view::camera_at;
use namespaced_id::NamespacedId;
use plugin_host::{CommandSpec, FormId, HandlerSpec, Plugin, Registrar, UiAction, UiResponded};
use transport::Connection;
use ui::{Anchor, Color, Direction, Node, Panel, Ui};
use voxunami_cave::content::CaveContent;
use voxunami_cave::game::Cave;
use voxunami_cave::plugins::cave_plugins;

type Heard = Arc<Mutex<Vec<UiResponded>>>;
type Shown = Arc<Mutex<Vec<FormId>>>;

fn menu(buttons: &[u32]) -> Ui {
    Ui {
        modal: true,
        anchor: Anchor::Center,
        root: Panel {
            title: Some("Board the ship?".to_owned()),
            color: Color([20, 20, 40]),
            direction: Direction::Column,
            children: buttons
                .iter()
                .map(|id| Node::Button {
                    id: *id,
                    text: format!("choice {id}"),
                })
                .collect(),
        },
    }
}

/// Shows the caller a form with `/menu`, changes the first form to one with other buttons with `/change`, closes it with `/shut`.
#[derive(Debug)]
struct Shower {
    heard: Heard,
    shown: Shown,
}

impl Plugin for Shower {
    fn id(&self) -> NamespacedId {
        "test:shower".parse().unwrap()
    }

    fn register(&self, registrar: &mut Registrar<'_>) {
        let shown = Arc::clone(&self.shown);
        registrar.command(
            CommandSpec::new("menu", "shows a form"),
            move |context, client, _| {
                if let Ok(id) = context.show_ui(client, &menu(&[1, 2, 3])) {
                    shown.lock().unwrap().push(id);
                }
            },
        );
        registrar.command(
            CommandSpec::new("change", "changes a form"),
            |context, client, _| {
                let _ = context.update_ui(client, FormId::new(1), &menu(&[7, 8]));
            },
        );
        let shown = Arc::clone(&self.shown);
        registrar.command(
            CommandSpec::new("shut", "closes the last form shown"),
            move |context, client, _| {
                if let Some(id) = shown.lock().unwrap().last().copied() {
                    context.close_ui(client, id);
                }
            },
        );
        let heard = Arc::clone(&self.heard);
        registrar.on::<UiResponded>(HandlerSpec::new("test:hear_forms"), move |event, _| {
            heard.lock().unwrap().push(*event);
        });
    }
}

fn rig_with_shower() -> (Rig, Heard) {
    let content = CaveContent::new();
    let world = content.new_overworld_world(SEED);
    let heard = Heard::default();
    let mut plugins = cave_plugins(&content);
    plugins.push(Box::new(Shower {
        heard: Arc::clone(&heard),
        shown: Shown::default(),
    }));
    (Rig::with_plugins(content, world, plugins), heard)
}

fn tick(rig: &mut Rig, player: &mut Player, count: usize) {
    for _ in 0..count {
        rig.step();
        player.advance().unwrap();
    }
}

fn forms(player: &Player) -> Vec<(u64, Ui)> {
    player
        .forms()
        .iter()
        .map(|(id, ui)| (id.get(), ui.clone()))
        .collect()
}

#[test]
fn a_form_is_on_the_players_session_until_the_server_changes_or_closes_it_and_what_they_press_reaches_the_plugin()
 {
    let (mut rig, heard) = rig_with_shower();
    let mut player = rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    assert_eq!(forms(&player), []);

    player.send_command("/menu");
    tick(&mut rig, &mut player, 2);
    assert_eq!(forms(&player), [(1, menu(&[1, 2, 3]))]);

    // A press goes to the plugin; the form stays.
    player.respond_to_form(FormId::new(1), UiAction::Pressed(2));
    tick(&mut rig, &mut player, 2);
    let heard_so_far = heard.lock().unwrap().clone();
    assert_eq!(heard_so_far.len(), 1);
    assert_eq!(heard_so_far[0].form, FormId::new(1));
    assert_eq!(heard_so_far[0].action, UiAction::Pressed(2));
    assert_eq!(forms(&player).len(), 1);

    // A button the form has not is not heard.
    player.respond_to_form(FormId::new(1), UiAction::Pressed(9));
    tick(&mut rig, &mut player, 2);
    assert_eq!(heard.lock().unwrap().len(), 1);

    // The server changes it: same number, new contents, in the same place in the list.
    player.send_command("/change");
    tick(&mut rig, &mut player, 2);
    assert_eq!(forms(&player), [(1, menu(&[7, 8]))]);

    // The player puts it away; the server closes it and the session forgets it.
    player.respond_to_form(FormId::new(1), UiAction::Dismissed);
    tick(&mut rig, &mut player, 2);
    assert_eq!(forms(&player), []);
    assert_eq!(
        heard.lock().unwrap().last().unwrap().action,
        UiAction::Dismissed
    );

    // And a form the plugin closes goes too.
    player.send_command("/menu");
    tick(&mut rig, &mut player, 2);
    assert_eq!(forms(&player).len(), 1);
    player.send_command("/shut");
    tick(&mut rig, &mut player, 2);
    assert_eq!(forms(&player), []);
}

#[test]
fn a_replay_shows_a_form_while_it_was_shown_and_not_after_and_answers_nothing() {
    let directory =
        std::env::temp_dir().join(format!("voxunami_cave_forms_{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("session.voxreplay");
    let (mut rig, heard) = rig_with_shower();
    let recorded: Box<dyn Connection> =
        record_connection(rig.connector.connect().unwrap(), &path).unwrap();
    let mut player = rig.join_with(recorded, [16.5, 110.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    player.send_command("/menu");
    tick(&mut rig, &mut player, 3);
    player.respond_to_form(FormId::new(1), UiAction::Dismissed);
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
    let mut shown = Vec::new();
    while replay.advance_one_tick().is_ok() {
        shown.push(!replay.forms().is_empty());
        // A replay answers nothing.
        replay.respond_to_form(FormId::new(1), UiAction::Pressed(1));
    }
    let first = shown.iter().position(|shown| *shown).expect("it was shown");
    let last = shown.iter().rposition(|shown| *shown).unwrap();
    assert!(shown[first..=last].iter().all(|shown| *shown));
    assert!(last < shown.len() - 1, "and gone before the end");
    assert_eq!(
        heard.lock().unwrap().len(),
        1,
        "only the dismissal of the live player, none from the replay"
    );
    std::fs::remove_dir_all(&directory).unwrap();
}
