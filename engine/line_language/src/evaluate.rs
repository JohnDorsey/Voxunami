//! Running the holes of a scanned line, innermost first and left to right, and building the call of the command they are in
//! (scripting-spec.md 4.4 and 4.5).
//!
//! This crate does not know what a command is. A [`Runner`] does: it is given a [`Call`] (a name and the words after it, each
//! word with the value of the hole if it is a hole) and runs it. The server's runner runs the server's commands, the window's
//! the window's.

use std::fmt;

use protocol::MAXIMUM_COMMAND_LENGTH;

use crate::scan::{Hole, Layer, Line, Segment, Sigil};
use crate::value::CommandValue;

/// The most bytes of text one hole may give.
pub const MAXIMUM_RESULT_LENGTH: usize = MAXIMUM_COMMAND_LENGTH;

/// The most bytes a line may be once its holes are filled in: no more than a command may be on the wire.
pub const MAXIMUM_LINE_LENGTH: usize = MAXIMUM_COMMAND_LENGTH;

/// Which hole: the number it has in the line and the character it opens at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HoleName {
    pub number: usize,
    pub column: usize,
}

impl fmt::Display for HoleName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "hole {} (at character {})",
            self.number, self.column
        )
    }
}

/// One word after the name of a command, once its holes are filled in.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// The word as text: typed text, and each hole's value written as Python's `str` writes it.
    pub text: String,
    /// Whether any part of the word is a hole.
    pub has_hole: bool,
    /// The value of the hole, when the word is that hole and nothing else. A command's argument of a kind takes this value as it
    /// is and refuses one of the wrong kind; nothing is converted by guessing.
    pub whole: Option<CommandValue>,
    /// The first hole in the word, for a message that has to say which hole gave a value that does not fit.
    pub first_hole: Option<HoleName>,
}

/// A command and its words, ready to run.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    /// Empty when the line holds no word at all.
    pub name: String,
    pub items: Vec<Item>,
}

/// Runs what the evaluation reaches. An error is the message to show; the evaluation names the hole it came from.
pub trait Runner {
    /// Runs a command of the language of `sigil` (`Server` or `Client`).
    fn command(&mut self, sigil: Sigil, call: &Call) -> Result<CommandValue, String>;

    /// Runs code, whose client holes are already filled in as Python literals.
    fn code(&mut self, code: &str) -> Result<CommandValue, String>;
}

/// Why a line did not run to its end.
#[derive(Clone, Debug, PartialEq)]
pub struct Failure {
    /// The hole that failed, or `None` when it was the line itself.
    pub hole: Option<HoleName>,
    pub message: String,
    /// How many holes had finished before this failure. What they did is not undone, and the message says so.
    pub holes_already_run: usize,
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.hole {
            Some(hole) => write!(formatter, "{hole}: {}", self.message)?,
            None => formatter.write_str(&self.message)?,
        }
        match self.holes_already_run {
            0 => Ok(()),
            1 => formatter.write_str(" (1 hole before it already ran and is not undone)"),
            count => write!(
                formatter,
                " ({count} holes before it already ran and are not undone)"
            ),
        }
    }
}

impl std::error::Error for Failure {}

/// What stops an evaluation on its way up: a failure already named, or the message of a runner that the nearest hole above names.
pub(crate) enum Stop {
    Failed(Failure),
    Refused(String),
}

/// Runs a scanned line. The value is that of the line's own command or code.
pub fn evaluate(line: &Line, runner: &mut dyn Runner) -> Result<CommandValue, Failure> {
    let mut evaluation = Evaluation::new(runner);
    match evaluation.run_layer(line.sigil, &line.layer) {
        Ok(value) => Ok(value),
        Err(stop) => Err(evaluation.settle(stop)),
    }
}

pub(crate) struct Evaluation<'runner> {
    runner: &'runner mut dyn Runner,
    holes_run: usize,
}

/// The word being made.
#[derive(Default)]
struct ItemBuilder {
    text: String,
    typed_text: bool,
    holes: usize,
    first_value: Option<CommandValue>,
    first_hole: Option<HoleName>,
}

impl ItemBuilder {
    fn finish(self) -> Item {
        Item {
            has_hole: self.holes > 0,
            whole: if self.holes == 1 && !self.typed_text {
                self.first_value
            } else {
                None
            },
            first_hole: self.first_hole,
            text: self.text,
        }
    }
}

impl<'runner> Evaluation<'runner> {
    pub(crate) fn new(runner: &'runner mut dyn Runner) -> Self {
        Self {
            runner,
            holes_run: 0,
        }
    }

    pub(crate) fn failure(&self, hole: Option<HoleName>, message: String) -> Failure {
        Failure {
            hole,
            message,
            holes_already_run: self.holes_run,
        }
    }

    /// A failure that reaches the top of the line: a message nobody named belongs to the line itself.
    pub(crate) fn settle(&self, stop: Stop) -> Failure {
        match stop {
            Stop::Failed(failure) => failure,
            Stop::Refused(message) => self.failure(None, message),
        }
    }

    fn run_layer(&mut self, sigil: Sigil, layer: &Layer) -> Result<CommandValue, Stop> {
        match sigil {
            Sigil::Code => {
                let code = self.code_text(layer)?;
                self.runner.code(&code).map_err(Stop::Refused)
            }
            Sigil::Server | Sigil::Client => {
                let call = self.call(layer)?;
                self.runner.command(sigil, &call).map_err(Stop::Refused)
            }
        }
    }

    /// Runs a hole, and gives what it gives.
    pub(crate) fn hole(&mut self, hole: &Hole) -> Result<CommandValue, Stop> {
        let name = HoleName {
            number: hole.number,
            column: hole.column,
        };
        let value = match self.run_layer(hole.sigil, &hole.body) {
            Ok(value) => value,
            Err(Stop::Refused(message)) => {
                return Err(Stop::Failed(self.failure(Some(name), message)));
            }
            Err(failed) => return Err(failed),
        };
        self.holes_run += 1;
        let length = value.to_text().len();
        if length > MAXIMUM_RESULT_LENGTH {
            return Err(Stop::Failed(self.failure(
                Some(name),
                format!(
                    "its result is {length} bytes long and a result may be at most {MAXIMUM_RESULT_LENGTH} (the hole has run, and what it did is not undone)"
                ),
            )));
        }
        Ok(value)
    }

    /// The text of code with its client holes written in as Python.
    fn code_text(&mut self, layer: &Layer) -> Result<String, Stop> {
        let mut code = String::new();
        for segment in &layer.segments {
            match segment {
                Segment::Text(text) => code.push_str(text),
                Segment::Hole(hole) => code.push_str(&self.hole(hole)?.to_python()),
            }
        }
        Ok(code)
    }

    /// Splits the text of a layer into words and fills in the holes: a word is what lies between blanks, and a hole is a word of
    /// its own when blanks (or the ends) lie on both sides of it.
    fn call(&mut self, layer: &Layer) -> Result<Call, Stop> {
        let mut items: Vec<Item> = Vec::new();
        let mut open: Option<ItemBuilder> = None;
        let mut length = 0usize;
        for segment in &layer.segments {
            match segment {
                Segment::Text(text) => {
                    for character in text.chars() {
                        if character.is_whitespace() {
                            if let Some(builder) = open.take() {
                                items.push(builder.finish());
                            }
                        } else {
                            let builder = open.get_or_insert_with(ItemBuilder::default);
                            builder.text.push(character);
                            builder.typed_text = true;
                            length += character.len_utf8();
                        }
                    }
                }
                Segment::Hole(hole) => {
                    let value = self.hole(hole)?;
                    let text = value.to_text();
                    let name = HoleName {
                        number: hole.number,
                        column: hole.column,
                    };
                    length += text.len();
                    let builder = open.get_or_insert_with(ItemBuilder::default);
                    builder.text.push_str(&text);
                    builder.holes += 1;
                    if builder.holes == 1 {
                        builder.first_value = Some(value);
                        builder.first_hole = Some(name);
                    }
                    let words = items.len() + 1;
                    if length + words > MAXIMUM_LINE_LENGTH {
                        return Err(Stop::Failed(self.failure(
                            Some(name),
                            format!(
                                "the line would be longer than {MAXIMUM_LINE_LENGTH} bytes once this hole is filled in"
                            ),
                        )));
                    }
                }
            }
        }
        items.extend(open.map(ItemBuilder::finish));
        let mut items = items.into_iter();
        let Some(first) = items.next() else {
            return Ok(Call {
                name: String::new(),
                items: Vec::new(),
            });
        };
        if first.has_hole {
            return Err(Stop::Refused(
                "a hole cannot be the name of a command: holes are for the words after it"
                    .to_owned(),
            ));
        }
        Ok(Call {
            name: first.text,
            items: items.collect(),
        })
    }
}
