//! Asking a player a question: a title and a few numbered options, to one client, and the answer back.
//!
//! A plugin asks with [`crate::Context::prompt`] and hears the answer as the event [`PromptAnswered`]. The
//! engine keeps which questions are waiting for whom, so that only the player asked can answer, only once,
//! and only with one of the options; anything else is ignored. What the plugin does meanwhile (holding the
//! thing the question is about, going on without it) is the plugin's business: a question has no time limit,
//! and ends when it is answered, cancelled ([`crate::Context::cancel_prompt`]), or the player leaves.
//!
//! The three messages are the engine's own, so the clients' shells show the question and send the answer for
//! every game, and a recording shows the question.

use std::collections::BTreeMap;

use binary_codec::{ByteReader, ByteWriter, DecodeError};
use namespaced_id::NamespacedId;
use protocol::{ClientId, Message};

/// How many options a question has, at most (the player chooses one by number, 1 to 9).
pub const MAXIMUM_PROMPT_OPTIONS: usize = 9;

/// How long, in bytes, a question's title and each of its options may be.
pub const MAXIMUM_PROMPT_TEXT: usize = 200;

fn kind(text: &str) -> NamespacedId {
    text.parse().expect("constant identifier is valid")
}

/// Which question. Numbered from 1 in the order they are asked, and not used again while the server runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PromptId(u64);

impl PromptId {
    pub const fn new(number: u64) -> Self {
        Self(number)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for PromptId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// Why a question could not be asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PromptError {
    #[error("a question has at least one option")]
    NoOptions,
    #[error("a question has at most {MAXIMUM_PROMPT_OPTIONS} options")]
    TooManyOptions,
    #[error("the title and the options are at most {MAXIMUM_PROMPT_TEXT} bytes each")]
    TextTooLong,
    #[error("there is no such player to ask")]
    NotConnected,
}

/// Whether a title and options can be asked.
pub(crate) fn check(title: &str, options: &[&str]) -> Result<(), PromptError> {
    if options.is_empty() {
        return Err(PromptError::NoOptions);
    }
    if options.len() > MAXIMUM_PROMPT_OPTIONS {
        return Err(PromptError::TooManyOptions);
    }
    if title.len() > MAXIMUM_PROMPT_TEXT || options.iter().any(|o| o.len() > MAXIMUM_PROMPT_TEXT) {
        return Err(PromptError::TextTooLong);
    }
    Ok(())
}

/// Server to client: a question.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prompt {
    pub id: PromptId,
    pub title: String,
    pub options: Vec<String>,
}

impl Message for Prompt {
    fn kind() -> NamespacedId {
        kind("engine:prompt")
    }

    fn encode(&self) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        writer.write_varint(self.id.get());
        writer.write_string(&self.title);
        writer.write_varint(self.options.len() as u64);
        for option in &self.options {
            writer.write_string(option);
        }
        writer.into_bytes()
    }

    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = ByteReader::new(payload);
        let id = PromptId::new(reader.read_varint()?);
        let title = reader.read_string()?.to_owned();
        let count = reader.read_count(1)?;
        let options: Vec<String> = (0..count)
            .map(|_| reader.read_string().map(str::to_owned))
            .collect::<Result<_, _>>()?;
        reader.finish()?;
        let borrowed: Vec<&str> = options.iter().map(String::as_str).collect();
        check(&title, &borrowed)
            .map_err(|_| DecodeError::Invalid("a question is not one that can be asked"))?;
        Ok(Self { id, title, options })
    }

    fn samples() -> Vec<Self> {
        vec![
            Self {
                id: PromptId::new(1),
                title: "Delete this level?".to_owned(),
                options: vec!["Keep it".to_owned(), "Delete it".to_owned()],
            },
            Self {
                id: PromptId::new(300),
                title: String::new(),
                options: vec!["Only one".to_owned()],
            },
        ]
    }
}

/// Client to server: the player's answer, the number of the option chosen counted from 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PromptAnswer {
    pub id: PromptId,
    pub choice: u32,
}

impl Message for PromptAnswer {
    fn kind() -> NamespacedId {
        kind("engine:prompt_answer")
    }

    fn encode(&self) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        writer.write_varint(self.id.get());
        writer.write_varint(u64::from(self.choice));
        writer.into_bytes()
    }

    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = ByteReader::new(payload);
        let id = PromptId::new(reader.read_varint()?);
        let choice = u32::try_from(reader.read_varint()?)
            .map_err(|_| DecodeError::Invalid("an option number is too large"))?;
        reader.finish()?;
        Ok(Self { id, choice })
    }

    fn samples() -> Vec<Self> {
        vec![
            Self {
                id: PromptId::new(1),
                choice: 0,
            },
            Self {
                id: PromptId::new(70_000),
                choice: 8,
            },
        ]
    }
}

/// Server to client: a question is over (answered or withdrawn), so the client stops showing it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PromptClosed(pub PromptId);

impl Message for PromptClosed {
    fn kind() -> NamespacedId {
        kind("engine:prompt_closed")
    }

    fn encode(&self) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        writer.write_varint(self.0.get());
        writer.into_bytes()
    }

    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = ByteReader::new(payload);
        let id = PromptId::new(reader.read_varint()?);
        reader.finish()?;
        Ok(Self(id))
    }

    fn samples() -> Vec<Self> {
        vec![Self(PromptId::new(1)), Self(PromptId::new(1 << 40))]
    }
}

/// The questions that are waiting for an answer, and the numbering of them.
#[derive(Debug, Default)]
pub(crate) struct Prompts {
    last: u64,
    /// For each question waiting, how many options it has.
    waiting: BTreeMap<(ClientId, PromptId), usize>,
}

impl Prompts {
    /// Records a question to `client` with this many options, and numbers it.
    pub(crate) fn ask(&mut self, client: ClientId, options: usize) -> PromptId {
        self.last += 1;
        let id = PromptId::new(self.last);
        self.waiting.insert((client, id), options);
        id
    }

    /// Ends a question that was answered with `choice`: whether it was waiting for that client and has that
    /// option. A question that is not, or an option it does not have, changes nothing.
    pub(crate) fn answer(&mut self, client: ClientId, id: PromptId, choice: u32) -> bool {
        let valid = self
            .waiting
            .get(&(client, id))
            .is_some_and(|options| (choice as usize) < *options);
        if valid {
            self.waiting.remove(&(client, id));
        }
        valid
    }

    /// Ends a question without an answer: whether it was waiting.
    pub(crate) fn cancel(&mut self, client: ClientId, id: PromptId) -> bool {
        self.waiting.remove(&(client, id)).is_some()
    }

    /// Ends every question waiting for a client, who has gone.
    pub(crate) fn forget(&mut self, client: ClientId) {
        self.waiting
            .retain(|(waiting_for, _), _| *waiting_for != client);
    }

    /// How many questions are waiting.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.waiting.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_messages_round_trip_and_pass_the_generated_check() {
        assert_eq!(protocol::check_message::<Prompt>(), Ok(()));
        assert_eq!(protocol::check_message::<PromptAnswer>(), Ok(()));
        assert_eq!(protocol::check_message::<PromptClosed>(), Ok(()));
    }

    #[test]
    fn a_question_that_could_not_be_asked_does_not_decode_either() {
        let good = Prompt::samples().remove(0);
        let with = |options: usize, title: usize, option: usize| {
            Prompt {
                id: good.id,
                title: "t".repeat(title),
                options: vec!["o".repeat(option); options],
            }
            .encode()
        };
        assert!(Prompt::decode(&with(1, 0, 0)).is_ok());
        assert!(Prompt::decode(&with(9, MAXIMUM_PROMPT_TEXT, MAXIMUM_PROMPT_TEXT)).is_ok());
        assert!(Prompt::decode(&with(0, 1, 1)).is_err(), "no options");
        assert!(Prompt::decode(&with(10, 1, 1)).is_err(), "too many options");
        assert!(Prompt::decode(&with(1, MAXIMUM_PROMPT_TEXT + 1, 1)).is_err());
        assert!(Prompt::decode(&with(1, 1, MAXIMUM_PROMPT_TEXT + 1)).is_err());
    }

    /// Every cut-short form of every sample, and every sample with a byte too many, is refused.
    fn refuses_damaged<M: Message>() {
        for sample in M::samples() {
            let good = sample.encode();
            for length in 0..good.len() {
                assert!(
                    M::decode(&good[..length]).is_err(),
                    "{} cut to {length} bytes",
                    M::kind()
                );
            }
            let mut trailing = good;
            trailing.push(0);
            assert!(
                M::decode(&trailing).is_err(),
                "{} with a byte too many",
                M::kind()
            );
        }
    }

    #[test]
    fn damaged_messages_are_errors_never_panics() {
        refuses_damaged::<Prompt>();
        refuses_damaged::<PromptAnswer>();
        refuses_damaged::<PromptClosed>();
        // An option number does not fit in a u32.
        let mut writer = ByteWriter::new();
        writer.write_varint(1);
        writer.write_varint(u64::from(u32::MAX) + 1);
        assert!(PromptAnswer::decode(&writer.into_bytes()).is_err());
    }

    #[test]
    fn only_the_player_asked_answers_once_and_with_an_option_that_there_is() {
        let (first, second) = (ClientId::new(1), ClientId::new(2));
        let mut prompts = Prompts::default();
        let id = prompts.ask(first, 3);
        assert_eq!(id, PromptId::new(1));
        assert!(!prompts.answer(second, id, 0), "not asked of them");
        assert!(!prompts.answer(first, id, 3), "there is no fourth option");
        assert!(
            !prompts.answer(first, PromptId::new(9), 0),
            "no such question"
        );
        assert_eq!(prompts.len(), 1, "nothing was used up by those");
        assert!(prompts.answer(first, id, 2));
        assert!(!prompts.answer(first, id, 2), "only once");
        assert_eq!(prompts.len(), 0);
    }

    #[test]
    fn questions_are_numbered_in_order_and_not_numbered_again() {
        let client = ClientId::new(1);
        let mut prompts = Prompts::default();
        let ids: Vec<u64> = (0..3).map(|_| prompts.ask(client, 2).get()).collect();
        assert_eq!(ids, [1, 2, 3]);
        assert!(prompts.cancel(client, PromptId::new(3)));
        assert!(!prompts.cancel(client, PromptId::new(3)));
        assert_eq!(prompts.ask(client, 2).get(), 4, "3 is not asked again");
    }

    #[test]
    fn a_player_who_leaves_takes_their_questions_and_nobody_elses_with_them() {
        let (gone, stays) = (ClientId::new(1), ClientId::new(2));
        let mut prompts = Prompts::default();
        prompts.ask(gone, 2);
        prompts.ask(gone, 2);
        let kept = prompts.ask(stays, 2);
        prompts.forget(gone);
        assert_eq!(prompts.len(), 1);
        assert!(prompts.answer(stays, kept, 1));
    }

    #[test]
    fn what_can_be_asked_is_checked() {
        assert_eq!(check("t", &[]), Err(PromptError::NoOptions));
        assert_eq!(check("t", &["a"; 10]), Err(PromptError::TooManyOptions));
        assert_eq!(
            check(&"t".repeat(201), &["a"]),
            Err(PromptError::TextTooLong)
        );
        assert_eq!(
            check("t", &[&"o".repeat(201)]),
            Err(PromptError::TextTooLong)
        );
        assert_eq!(check("", &["a"]), Ok(()));
        assert_eq!(check("t", &["a"; 9]), Ok(()));
    }
}
