//! Asking a player a question, through a real server and clients.

use std::sync::{Arc, Mutex};

use plugin_host::{
    Argument, CommandSpec, HandlerSpec, Plugin, PluginHost, Prompt, PromptAnswer, PromptAnswered,
    PromptClosed, PromptError, PromptId, Registrar,
};
use protocol::{ClientId, ClientMessage, Message, ServerMessage};
use server::test_support::log_in;
use server::{Server, ServerConfig};
use transport::Connection;
use transport::loopback::{LoopbackConnection, LoopbackConnector, loopback_listener};
use world_storage::test_support::{Game, Plugin as FixturePlugin, id, sample_world};

type Asked = Arc<Mutex<Vec<Result<PromptId, PromptError>>>>;
type Answers = Arc<Mutex<Vec<PromptAnswered>>>;
type Cancelled = Arc<Mutex<Vec<bool>>>;

/// A plugin that asks on request and records what comes back: `/ask` asks the caller whether to delete a
/// level, `/askbad` asks with no options, `/askabsent` asks someone who is not there, and `/cancel N` withdraws
/// the caller's question number N.
#[derive(Debug)]
struct Asker {
    asked: Asked,
    answers: Answers,
    cancelled: Cancelled,
}

impl Plugin for Asker {
    fn id(&self) -> namespaced_id::NamespacedId {
        id("test:asker")
    }

    fn register(&self, registrar: &mut Registrar<'_>) {
        let asked = Arc::clone(&self.asked);
        registrar.command(
            CommandSpec::new("ask", "asks"),
            move |context, client, _| {
                let result = context.prompt(client, "Delete the level?", &["Keep it", "Delete it"]);
                asked.lock().unwrap().push(result);
            },
        );
        let asked = Arc::clone(&self.asked);
        registrar.command(
            CommandSpec::new("askbad", "asks badly"),
            move |context, client, _| {
                let result = context.prompt(client, "Nothing to choose?", &[]);
                asked.lock().unwrap().push(result);
            },
        );
        let asked = Arc::clone(&self.asked);
        registrar.command(
            CommandSpec::new("askabsent", "asks someone not there"),
            move |context, _, _| {
                let result = context.prompt(ClientId::new(99), "Hello?", &["Yes"]);
                asked.lock().unwrap().push(result);
            },
        );
        let cancelled = Arc::clone(&self.cancelled);
        registrar.command(
            CommandSpec::new("cancel", "withdraws a question").argument(Argument::number("number")),
            move |context, client, arguments| {
                let id = PromptId::new(arguments.number(0) as u64);
                cancelled
                    .lock()
                    .unwrap()
                    .push(context.cancel_prompt(client, id));
            },
        );
        let answers = Arc::clone(&self.answers);
        registrar.on::<PromptAnswered>(HandlerSpec::new("test:hear_answers"), move |event, _| {
            answers.lock().unwrap().push(*event)
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

    fn answer(&mut self, id: PromptId, choice: u32) {
        self.send(&ClientMessage::GameMessage {
            kind: PromptAnswer::kind(),
            payload: PromptAnswer { id, choice }.encode(),
        });
    }

    fn received(&mut self) -> Vec<ServerMessage> {
        let mut messages = Vec::new();
        while let Some(frame) = self.connection.receive().unwrap_or(None) {
            messages.push(ServerMessage::decode(&frame).unwrap());
        }
        messages
    }

    /// The questions and the endings of questions in what was received, in order.
    fn prompts(&mut self) -> (Vec<Prompt>, Vec<PromptClosed>) {
        let (mut asked, mut closed) = (Vec::new(), Vec::new());
        for message in self.received() {
            let ServerMessage::GameMessage { kind, payload } = message else {
                continue;
            };
            if kind == Prompt::kind() {
                asked.push(Prompt::decode(&payload).unwrap());
            } else if kind == PromptClosed::kind() {
                closed.push(PromptClosed::decode(&payload).unwrap());
            }
        }
        (asked, closed)
    }
}

struct Rig {
    server: Server,
    connector: LoopbackConnector,
    asked: Asked,
    answers: Answers,
    cancelled: Cancelled,
    /// How many messages the host could not read.
    malformed: Arc<std::sync::atomic::AtomicU64>,
    joined: u8,
}

impl Rig {
    fn new() -> Self {
        let game = Game::new(&[FixturePlugin::BaseBlocks]);
        let world = sample_world(&game);
        let (asked, answers, cancelled) =
            (Asked::default(), Answers::default(), Cancelled::default());
        let asker = Asker {
            asked: Arc::clone(&asked),
            answers: Arc::clone(&answers),
            cancelled: Arc::clone(&cancelled),
        };
        let host = PluginHost::new(vec![Box::new(asker)], &world).unwrap();
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
            asked,
            answers,
            cancelled,
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
        client.received();
        client
    }

    fn step(&mut self) {
        self.server.step();
    }

    fn answers(&self) -> Vec<PromptAnswered> {
        self.answers.lock().unwrap().clone()
    }
}

/// Asks the client a question with `/ask` and returns it as the client was told it.
fn ask(rig: &mut Rig, client: &mut Client) -> Prompt {
    client.command("/ask");
    rig.step();
    let (mut asked, _) = client.prompts();
    assert_eq!(asked.len(), 1);
    asked.remove(0)
}

#[test]
fn a_question_goes_to_the_player_it_is_asked_of_and_only_to_them() {
    let mut rig = Rig::new();
    let (mut first, mut second) = (rig.join(), rig.join());
    let question = ask(&mut rig, &mut first);
    assert_eq!(question.id, PromptId::new(1));
    assert_eq!(question.title, "Delete the level?");
    assert_eq!(question.options, ["Keep it", "Delete it"]);
    assert_eq!(second.prompts(), (vec![], vec![]));
    assert_eq!(rig.asked.lock().unwrap().clone(), [Ok(PromptId::new(1))]);
}

#[test]
fn an_answer_reaches_the_plugin_once_and_the_player_is_told_the_question_is_over() {
    let mut rig = Rig::new();
    let mut player = rig.join();
    let question = ask(&mut rig, &mut player);
    player.answer(question.id, 1);
    rig.step();
    assert_eq!(
        rig.answers(),
        [PromptAnswered {
            client: ClientId::new(1),
            prompt: question.id,
            choice: 1,
        }]
    );
    assert_eq!(player.prompts(), (vec![], vec![PromptClosed(question.id)]));
    // A second answer to it, and a late one for the other option, are nothing.
    player.answer(question.id, 1);
    player.answer(question.id, 0);
    rig.step();
    assert_eq!(rig.answers().len(), 1);
    assert_eq!(player.prompts(), (vec![], vec![]));
}

#[test]
fn an_answer_that_is_not_one_that_the_question_can_have_is_ignored_and_the_question_stays() {
    let mut rig = Rig::new();
    let (mut asked, mut other) = (rig.join(), rig.join());
    let question = ask(&mut rig, &mut asked);
    // An option it does not have, a question that was never asked, one asked of someone else, and a message
    // that is not an answer at all.
    asked.answer(question.id, 2);
    asked.answer(PromptId::new(77), 0);
    other.answer(question.id, 0);
    asked.send(&ClientMessage::GameMessage {
        kind: PromptAnswer::kind(),
        payload: vec![0xFF],
    });
    rig.step();
    assert_eq!(rig.answers(), []);
    assert_eq!(
        rig.malformed.load(std::sync::atomic::Ordering::Relaxed),
        1,
        "only the one that could not be read is counted"
    );
    assert_eq!(asked.prompts(), (vec![], vec![]));
    assert_eq!(other.prompts(), (vec![], vec![]));
    // It can still be answered by the one it was asked of.
    asked.answer(question.id, 0);
    rig.step();
    assert_eq!(rig.answers().len(), 1);
}

#[test]
fn questions_are_numbered_apart_and_answered_in_any_order() {
    let mut rig = Rig::new();
    let mut player = rig.join();
    let first = ask(&mut rig, &mut player);
    let second = ask(&mut rig, &mut player);
    assert_eq!([first.id, second.id], [PromptId::new(1), PromptId::new(2)]);
    player.answer(second.id, 0);
    player.answer(first.id, 1);
    rig.step();
    let heard: Vec<(u64, usize)> = rig
        .answers()
        .iter()
        .map(|answer| (answer.prompt.get(), answer.choice))
        .collect();
    assert_eq!(heard, [(2, 0), (1, 1)]);
}

#[test]
fn a_question_that_cannot_be_asked_is_an_error_to_the_plugin_and_nothing_to_the_player() {
    let mut rig = Rig::new();
    let mut player = rig.join();
    player.command("/askbad");
    player.command("/askabsent");
    rig.step();
    assert_eq!(
        rig.asked.lock().unwrap().clone(),
        [Err(PromptError::NoOptions), Err(PromptError::NotConnected)]
    );
    assert_eq!(player.prompts(), (vec![], vec![]));
}

#[test]
fn a_question_withdrawn_is_closed_on_the_clients_screen_and_cannot_be_answered() {
    let mut rig = Rig::new();
    let (mut player, mut other) = (rig.join(), rig.join());
    let question = ask(&mut rig, &mut player);
    // Not the question of another player, and not one that was never asked.
    other.command(&format!("/cancel {}", question.id));
    player.command("/cancel 55");
    rig.step();
    assert_eq!(*rig.cancelled.lock().unwrap(), [false, false]);
    assert_eq!(player.prompts(), (vec![], vec![]));
    // Its own.
    player.command(&format!("/cancel {}", question.id));
    rig.step();
    assert_eq!(rig.cancelled.lock().unwrap().last(), Some(&true));
    assert_eq!(player.prompts(), (vec![], vec![PromptClosed(question.id)]));
    player.answer(question.id, 0);
    rig.step();
    assert_eq!(rig.answers(), []);
}

#[test]
fn a_player_who_leaves_takes_their_questions_with_them() {
    let mut rig = Rig::new();
    let mut leaver = rig.join();
    let question = ask(&mut rig, &mut leaver);
    leaver.send(&ClientMessage::Goodbye);
    rig.step();
    // Nobody else can answer the leaver's question, and the next question is numbered on from it. (That the
    // leaver's entry is dropped from the host is memory hygiene: client ids are never reused, so it cannot be
    // seen from outside, and no test bites on it.)
    let mut next = rig.join();
    next.answer(question.id, 0);
    rig.step();
    assert_eq!(rig.answers(), []);
    let theirs = ask(&mut rig, &mut next);
    assert_eq!(theirs.id, PromptId::new(2), "numbers are not used again");
}
