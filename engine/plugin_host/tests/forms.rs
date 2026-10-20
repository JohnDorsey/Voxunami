//! Showing players interfaces, through a real server and clients.

use std::sync::{Arc, Mutex};

use plugin_host::{
    Argument, CommandSpec, FormError, FormId, HandlerSpec, MAXIMUM_FORMS_PER_PLAYER, Plugin,
    PluginHost, Registrar, UiAction, UiClose, UiResponded, UiResponse, UiShow,
};
use protocol::{ClientId, ClientMessage, Message, ServerMessage};
use server::test_support::log_in;
use server::{Server, ServerConfig};
use transport::Connection;
use transport::loopback::{LoopbackConnection, LoopbackConnector, loopback_listener};
use ui::{Anchor, Color, Direction, Node, Panel, Ui};
use world_storage::test_support::{Game, Plugin as FixturePlugin, id, sample_world};

type Shown = Arc<Mutex<Vec<Result<FormId, FormError>>>>;
type Heard = Arc<Mutex<Vec<UiResponded>>>;
type Closed = Arc<Mutex<Vec<bool>>>;

fn modal(buttons: &[u32]) -> Ui {
    Ui {
        modal: true,
        anchor: Anchor::Center,
        root: Panel {
            title: Some("Delete the level?".to_owned()),
            color: Color([20, 20, 40]),
            direction: Direction::Column,
            children: buttons
                .iter()
                .map(|button| Node::Button {
                    id: *button,
                    text: format!("button {button}"),
                })
                .collect(),
        },
    }
}

fn bar() -> Ui {
    Ui {
        modal: false,
        anchor: Anchor::TopRight,
        root: Panel {
            title: None,
            color: Color([0, 0, 0]),
            direction: Direction::Row,
            children: vec![Node::Bar {
                permille: 500,
                cells: 10,
                color: Color([200, 30, 30]),
            }],
        },
    }
}

/// A plugin that shows forms on request and records what comes back: `/form` shows the caller a modal form with buttons 1
/// and 2, `/bar` a panel that is not modal, `/unreachable` a panel with a button, `/absent` a form to someone who is not there,
/// `/update N` changes the caller's form N to one with buttons 5 and 6, and `/close N` takes it away.
#[derive(Debug)]
struct Shower {
    shown: Shown,
    heard: Heard,
    closed: Closed,
    updates: Arc<Mutex<Vec<Result<(), FormError>>>>,
}

impl Plugin for Shower {
    fn id(&self) -> namespaced_id::NamespacedId {
        id("test:shower")
    }

    fn register(&self, registrar: &mut Registrar<'_>) {
        let command =
            |registrar: &mut Registrar<'_>,
             name: &'static str,
             show: fn(&mut plugin_host::Context<'_>, ClientId) -> Result<FormId, FormError>,
             shown: &Shown| {
                let shown = Arc::clone(shown);
                registrar.command(
                    CommandSpec::new(name, "shows"),
                    move |context, client, _| {
                        let result = show(context, client);
                        shown.lock().unwrap().push(result);
                    },
                );
            };
        command(
            registrar,
            "form",
            |context, client| context.show_ui(client, &modal(&[1, 2])),
            &self.shown,
        );
        command(
            registrar,
            "bar",
            |context, client| context.show_ui(client, &bar()),
            &self.shown,
        );
        command(
            registrar,
            "unreachable",
            |context, client| {
                let mut panel = modal(&[1]);
                panel.modal = false;
                context.show_ui(client, &panel)
            },
            &self.shown,
        );
        command(
            registrar,
            "absent",
            |context, _| context.show_ui(ClientId::new(99), &bar()),
            &self.shown,
        );
        let updates = Arc::clone(&self.updates);
        registrar.command(
            CommandSpec::new("update", "changes a form").argument(Argument::number("number")),
            move |context, client, arguments| {
                let form = FormId::new(arguments.number(0) as u64);
                updates
                    .lock()
                    .unwrap()
                    .push(context.update_ui(client, form, &modal(&[5, 6])));
            },
        );
        let closed = Arc::clone(&self.closed);
        registrar.command(
            CommandSpec::new("close", "takes a form away").argument(Argument::number("number")),
            move |context, client, arguments| {
                let form = FormId::new(arguments.number(0) as u64);
                closed.lock().unwrap().push(context.close_ui(client, form));
            },
        );
        let heard = Arc::clone(&self.heard);
        registrar.on::<UiResponded>(HandlerSpec::new("test:hear_forms"), move |event, _| {
            heard.lock().unwrap().push(*event)
        });
    }
}

struct Client {
    connection: LoopbackConnection,
}

impl Client {
    fn send(&mut self, message: &ClientMessage) {
        self.connection.send(message.encode()).unwrap();
    }

    fn command(&mut self, text: &str) {
        self.send(&ClientMessage::Command {
            text: text.to_owned(),
        });
    }

    fn respond(&mut self, id: FormId, action: UiAction) {
        self.send(&ClientMessage::GameMessage {
            kind: UiResponse::kind(),
            payload: UiResponse { id, action }.encode(),
        });
    }

    /// The forms shown and closed in what was received, in order.
    fn screen(&mut self) -> (Vec<UiShow>, Vec<UiClose>) {
        let (mut shown, mut closed) = (Vec::new(), Vec::new());
        while let Some(frame) = self.connection.receive().unwrap_or(None) {
            let ServerMessage::GameMessage { kind, payload } =
                ServerMessage::decode(&frame).unwrap()
            else {
                continue;
            };
            if kind == UiShow::kind() {
                shown.push(UiShow::decode(&payload).unwrap());
            } else if kind == UiClose::kind() {
                closed.push(UiClose::decode(&payload).unwrap());
            }
        }
        (shown, closed)
    }
}

struct Rig {
    server: Server,
    connector: LoopbackConnector,
    shown: Shown,
    heard: Heard,
    closed: Closed,
    updates: Arc<Mutex<Vec<Result<(), FormError>>>>,
    malformed: Arc<std::sync::atomic::AtomicU64>,
    joined: u8,
}

impl Rig {
    fn new() -> Self {
        let game = Game::new(&[FixturePlugin::BaseBlocks]);
        let world = sample_world(&game);
        let (shown, heard, closed) = (Shown::default(), Heard::default(), Closed::default());
        let updates = Arc::default();
        let shower = Shower {
            shown: Arc::clone(&shown),
            heard: Arc::clone(&heard),
            closed: Arc::clone(&closed),
            updates: Arc::clone(&updates),
        };
        let host = PluginHost::new(vec![Box::new(shower)], &world).unwrap();
        let malformed = host.malformed_messages_counter();
        let (listener, connector) = loopback_listener();
        let server = Server::new(
            ServerConfig::new(id("test_game:game")).with_counted_challenges(1),
            world,
            &game.registries(),
            Box::new(host),
            Box::new(listener),
        )
        .unwrap();
        Self {
            server,
            connector,
            shown,
            heard,
            closed,
            updates,
            malformed,
            joined: 0,
        }
    }

    fn join(&mut self) -> Client {
        let mut client = Client {
            connection: self.connector.connect().unwrap(),
        };
        self.joined += 1;
        log_in(
            &mut self.server,
            &mut client.connection,
            &id("test_game:game"),
            self.joined,
        );
        let _ = client.screen();
        client
    }

    fn step(&mut self) {
        self.server.step();
    }

    fn heard(&self) -> Vec<UiResponded> {
        self.heard.lock().unwrap().clone()
    }
}

/// Shows the client a modal form with `/form` and returns what the client was shown.
fn show_form(rig: &mut Rig, client: &mut Client) -> UiShow {
    client.command("/form");
    rig.step();
    let (mut shown, _) = client.screen();
    assert_eq!(shown.len(), 1);
    shown.remove(0)
}

#[test]
fn a_form_goes_to_the_player_it_is_shown_to_and_only_to_them_with_its_number() {
    let mut rig = Rig::new();
    let (mut first, mut second) = (rig.join(), rig.join());
    let form = show_form(&mut rig, &mut first);
    assert_eq!(form.id, FormId::new(1));
    assert_eq!(form.ui, modal(&[1, 2]));
    assert_eq!(second.screen(), (vec![], vec![]));
    assert_eq!(rig.shown.lock().unwrap().clone(), [Ok(FormId::new(1))]);
}

#[test]
fn a_press_reaches_the_plugin_each_time_and_the_form_stays_open() {
    let mut rig = Rig::new();
    let mut player = rig.join();
    let form = show_form(&mut rig, &mut player);
    player.respond(form.id, UiAction::Pressed(2));
    player.respond(form.id, UiAction::Pressed(2));
    player.respond(form.id, UiAction::Pressed(1));
    rig.step();
    let responded = |button| UiResponded {
        client: ClientId::new(1),
        form: form.id,
        action: UiAction::Pressed(button),
    };
    assert_eq!(rig.heard(), [responded(2), responded(2), responded(1)]);
    assert_eq!(player.screen(), (vec![], vec![]), "nothing closed");
}

#[test]
fn putting_a_modal_form_away_is_heard_once_closes_it_on_the_screen_and_ends_its_buttons() {
    let mut rig = Rig::new();
    let mut player = rig.join();
    let form = show_form(&mut rig, &mut player);
    player.respond(form.id, UiAction::Dismissed);
    rig.step();
    assert_eq!(
        rig.heard(),
        [UiResponded {
            client: ClientId::new(1),
            form: form.id,
            action: UiAction::Dismissed
        }]
    );
    assert_eq!(player.screen(), (vec![], vec![UiClose(form.id)]));
    // It is gone: nothing more can be done with it.
    player.respond(form.id, UiAction::Dismissed);
    player.respond(form.id, UiAction::Pressed(1));
    rig.step();
    assert_eq!(rig.heard().len(), 1);
    assert_eq!(player.screen(), (vec![], vec![]));
}

#[test]
fn responses_that_are_not_the_players_to_make_are_ignored_and_a_response_that_cannot_be_read_is_counted()
 {
    let mut rig = Rig::new();
    let (mut shown_to, mut other) = (rig.join(), rig.join());
    let form = show_form(&mut rig, &mut shown_to);
    // A button it does not have, a form that was never shown, one shown to someone else, and a message that is not a response.
    shown_to.respond(form.id, UiAction::Pressed(3));
    shown_to.respond(FormId::new(77), UiAction::Pressed(1));
    other.respond(form.id, UiAction::Pressed(1));
    other.respond(form.id, UiAction::Dismissed);
    shown_to.send(&ClientMessage::GameMessage {
        kind: UiResponse::kind(),
        payload: vec![0xFF],
    });
    // A panel cannot be put away.
    shown_to.command("/bar");
    rig.step();
    let (bars, _) = shown_to.screen();
    shown_to.respond(bars[0].id, UiAction::Dismissed);
    rig.step();
    assert_eq!(rig.heard(), []);
    assert_eq!(
        rig.malformed.load(std::sync::atomic::Ordering::Relaxed),
        1,
        "only the unreadable one is counted"
    );
    assert_eq!(other.screen(), (vec![], vec![]));
    // The form is still there for the one it was shown to.
    shown_to.respond(form.id, UiAction::Pressed(1));
    rig.step();
    assert_eq!(rig.heard().len(), 1);
}

#[test]
fn updating_a_form_keeps_its_number_sends_the_new_one_and_ends_the_old_buttons() {
    let mut rig = Rig::new();
    let mut player = rig.join();
    let form = show_form(&mut rig, &mut player);
    player.command(&format!("/update {}", form.id));
    rig.step();
    let (shown, closed) = player.screen();
    assert_eq!((shown.len(), closed.len()), (1, 0));
    assert_eq!(shown[0].id, form.id);
    assert_eq!(shown[0].ui, modal(&[5, 6]));
    player.respond(form.id, UiAction::Pressed(1));
    player.respond(form.id, UiAction::Pressed(5));
    rig.step();
    assert_eq!(rig.heard().len(), 1);
    assert_eq!(rig.heard()[0].action, UiAction::Pressed(5));
    // A form that is not open cannot be updated, and nothing is sent for it.
    player.command("/update 9");
    rig.step();
    assert_eq!(
        rig.updates.lock().unwrap().clone(),
        [Ok(()), Err(FormError::NotOpen)]
    );
    assert_eq!(player.screen(), (vec![], vec![]));
}

#[test]
fn a_form_that_cannot_be_shown_is_an_error_to_the_plugin_and_nothing_to_the_player() {
    let mut rig = Rig::new();
    let mut player = rig.join();
    player.command("/unreachable");
    player.command("/absent");
    rig.step();
    assert_eq!(
        rig.shown.lock().unwrap().clone(),
        [
            Err(FormError::ButtonsNeedAModal),
            Err(FormError::NotConnected)
        ]
    );
    assert_eq!(player.screen(), (vec![], vec![]));
    // The ninth panel at once is too many.
    for _ in 0..MAXIMUM_FORMS_PER_PLAYER + 1 {
        player.command("/bar");
    }
    rig.step();
    let results = rig.shown.lock().unwrap().clone();
    assert_eq!(
        results.iter().filter(|result| result.is_ok()).count(),
        MAXIMUM_FORMS_PER_PLAYER
    );
    assert_eq!(results.last(), Some(&Err(FormError::TooManyForms)));
    assert_eq!(player.screen().0.len(), MAXIMUM_FORMS_PER_PLAYER);
}

#[test]
fn a_form_closed_by_the_plugin_is_closed_on_the_screen_and_cannot_be_responded_to() {
    let mut rig = Rig::new();
    let (mut player, mut other) = (rig.join(), rig.join());
    let form = show_form(&mut rig, &mut player);
    // Not another player's form, and not one that was never shown.
    other.command(&format!("/close {}", form.id));
    player.command("/close 55");
    rig.step();
    assert_eq!(*rig.closed.lock().unwrap(), [false, false]);
    assert_eq!(player.screen(), (vec![], vec![]));
    player.command(&format!("/close {}", form.id));
    rig.step();
    assert_eq!(rig.closed.lock().unwrap().last(), Some(&true));
    assert_eq!(player.screen(), (vec![], vec![UiClose(form.id)]));
    player.respond(form.id, UiAction::Pressed(1));
    rig.step();
    assert_eq!(rig.heard(), []);
}

#[test]
fn a_player_who_leaves_takes_their_forms_with_them() {
    let mut rig = Rig::new();
    let mut leaver = rig.join();
    let form = show_form(&mut rig, &mut leaver);
    leaver.send(&ClientMessage::Goodbye);
    rig.step();
    let mut next = rig.join();
    next.respond(form.id, UiAction::Pressed(1));
    rig.step();
    assert_eq!(rig.heard(), []);
    assert_eq!(
        show_form(&mut rig, &mut next).id,
        FormId::new(2),
        "numbers are not used again"
    );
}
