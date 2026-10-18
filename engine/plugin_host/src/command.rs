//! Commands a player types: what each is called, what arguments it takes, and the parsing of a typed line
//! into those arguments, so that a handler never parses text and every command answers a malformed
//! call the same way.

use namespaced_id::NamespacedId;

/// What an argument is written as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArgumentKind {
    /// Any one word.
    Word,
    /// A finite decimal number, such as `16.5` or `-3`.
    Number,
    /// A whole number, such as `-3`.
    Integer,
    /// A namespaced identifier, such as `base_blocks:lamp`.
    Identifier,
    /// All the words that remain, kept as they were typed. Last in a command.
    Rest,
}

impl ArgumentKind {
    fn description(self) -> &'static str {
        match self {
            Self::Word | Self::Rest => "a word",
            Self::Number => "a number",
            Self::Integer => "a whole number",
            Self::Identifier => "an identifier like namespace:name",
        }
    }
}

/// One argument of a command: a name (shown in capitals in the usage), what it is written as, and
/// whether it may be left out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Argument {
    name: &'static str,
    kind: ArgumentKind,
    optional: bool,
}

impl Argument {
    const fn new(name: &'static str, kind: ArgumentKind) -> Self {
        Self {
            name,
            kind,
            optional: false,
        }
    }

    pub const fn word(name: &'static str) -> Self {
        Self::new(name, ArgumentKind::Word)
    }

    pub const fn number(name: &'static str) -> Self {
        Self::new(name, ArgumentKind::Number)
    }

    pub const fn integer(name: &'static str) -> Self {
        Self::new(name, ArgumentKind::Integer)
    }

    pub const fn identifier(name: &'static str) -> Self {
        Self::new(name, ArgumentKind::Identifier)
    }

    /// The words that remain; the handler splits them further itself.
    pub const fn rest(name: &'static str) -> Self {
        Self::new(name, ArgumentKind::Rest)
    }

    /// The argument may be left out (and so may every one after it).
    pub const fn optional(mut self) -> Self {
        self.optional = true;
        self
    }

    fn shown(&self) -> String {
        let name = self.name.to_uppercase();
        let name = if self.kind == ArgumentKind::Rest {
            format!("{name}...")
        } else {
            name
        };
        if self.optional {
            format!("[{name}]")
        } else {
            name
        }
    }
}

/// A server command: its name, what it does, and the arguments it takes. The usage line is made from
/// these, so it cannot disagree with what the command accepts.
#[derive(Clone, Debug)]
pub struct CommandSpec {
    pub(crate) name: &'static str,
    pub(crate) summary: &'static str,
    pub(crate) arguments: Vec<Argument>,
}

impl CommandSpec {
    pub fn new(name: &'static str, summary: &'static str) -> Self {
        Self {
            name,
            summary,
            arguments: Vec::new(),
        }
    }

    /// Adds the next argument. A command is written by its plugin, so a mistake in its shape is a
    /// mistake in the plugin and panics: a `Rest` argument must be last, and once an argument is optional
    /// every later one must be too.
    pub fn argument(mut self, argument: Argument) -> Self {
        if let Some(last) = self.arguments.last() {
            assert!(
                last.kind != ArgumentKind::Rest,
                "/{}: nothing can follow a rest argument",
                self.name
            );
            assert!(
                !last.optional || argument.optional,
                "/{}: a required argument cannot follow an optional one",
                self.name
            );
        }
        self.arguments.push(argument);
        self
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn summary(&self) -> &'static str {
        self.summary
    }

    /// How the command is typed: `/tp X Y Z`, with optional arguments in brackets.
    pub fn usage(&self) -> String {
        let mut usage = format!("/{}", self.name);
        for argument in &self.arguments {
            usage.push(' ');
            usage.push_str(&argument.shown());
        }
        usage
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Value<'text> {
    Absent,
    Word(&'text str),
    Number(f64),
    Integer(i64),
    Identifier(NamespacedId),
    Rest(Vec<&'text str>),
}

/// The arguments of a call, checked against the command's spec. The getters take the position of the
/// argument in the spec; asking for one the spec does not have, or as the wrong kind, is a mistake in
/// the handler and panics.
#[derive(Clone, Debug)]
pub struct Arguments<'text> {
    values: Vec<Value<'text>>,
}

impl<'text> Arguments<'text> {
    /// Checks the words typed after a command's name. The error is the reply to give the player:
    /// the usage, and for a word that is not what its argument is, which one.
    pub(crate) fn parse(spec: &CommandSpec, words: &[&'text str]) -> Result<Self, String> {
        let required = spec
            .arguments
            .iter()
            .filter(|argument| !argument.optional)
            .count();
        let takes_rest = spec
            .arguments
            .last()
            .is_some_and(|argument| argument.kind == ArgumentKind::Rest);
        if words.len() < required || (!takes_rest && words.len() > spec.arguments.len()) {
            return Err(format!("usage: {}", spec.usage()));
        }
        let mut values = Vec::with_capacity(spec.arguments.len());
        for (position, argument) in spec.arguments.iter().enumerate() {
            let Some(&word) = words.get(position) else {
                values.push(Value::Absent);
                continue;
            };
            let value = match argument.kind {
                ArgumentKind::Word => Some(Value::Word(word)),
                ArgumentKind::Number => word
                    .parse::<f64>()
                    .ok()
                    .filter(|number| number.is_finite())
                    .map(Value::Number),
                ArgumentKind::Integer => word.parse::<i64>().ok().map(Value::Integer),
                ArgumentKind::Identifier => {
                    word.parse::<NamespacedId>().ok().map(Value::Identifier)
                }
                ArgumentKind::Rest => Some(Value::Rest(words[position..].to_vec())),
            };
            let Some(value) = value else {
                return Err(format!(
                    "usage: {}; {} must be {}",
                    spec.usage(),
                    argument.name.to_uppercase(),
                    argument.kind.description()
                ));
            };
            values.push(value);
        }
        Ok(Self { values })
    }

    /// Whether the argument was typed (always true for a required one).
    pub fn present(&self, position: usize) -> bool {
        !matches!(self.values[position], Value::Absent)
    }

    pub fn word(&self, position: usize) -> &'text str {
        match self.values[position] {
            Value::Word(word) => word,
            ref other => panic!("argument {position} is not a word that was typed: {other:?}"),
        }
    }

    pub fn number(&self, position: usize) -> f64 {
        match self.values[position] {
            Value::Number(number) => number,
            ref other => panic!("argument {position} is not a number that was typed: {other:?}"),
        }
    }

    pub fn integer(&self, position: usize) -> i64 {
        match self.values[position] {
            Value::Integer(number) => number,
            ref other => {
                panic!("argument {position} is not a whole number that was typed: {other:?}")
            }
        }
    }

    pub fn identifier(&self, position: usize) -> &NamespacedId {
        match &self.values[position] {
            Value::Identifier(identifier) => identifier,
            other => panic!("argument {position} is not an identifier that was typed: {other:?}"),
        }
    }

    /// The words of a `Rest` argument; none if it was left out.
    pub fn rest(&self, position: usize) -> &[&'text str] {
        match &self.values[position] {
            Value::Rest(words) => words,
            Value::Absent => &[],
            other => panic!("argument {position} is not a rest argument: {other:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> CommandSpec {
        CommandSpec::new("go", "goes somewhere")
            .argument(Argument::number("x"))
            .argument(Argument::integer("count"))
            .argument(Argument::identifier("block").optional())
            .argument(Argument::rest("notes").optional())
    }

    #[test]
    fn the_usage_is_made_from_the_arguments() {
        assert_eq!(spec().usage(), "/go X COUNT [BLOCK] [NOTES...]");
        assert_eq!(CommandSpec::new("fly", "flies").usage(), "/fly");
        let word = CommandSpec::new("say", "").argument(Argument::word("what"));
        assert_eq!(word.usage(), "/say WHAT");
    }

    #[test]
    fn typed_words_become_values() {
        let arguments = Arguments::parse(&spec(), &["-2.5", "7", "a:b", "and", "more"]).unwrap();
        assert_eq!(arguments.number(0), -2.5);
        assert_eq!(arguments.integer(1), 7);
        assert_eq!(arguments.identifier(2).to_string(), "a:b");
        assert_eq!(arguments.rest(3), ["and", "more"]);
        let fewer = Arguments::parse(&spec(), &["1", "2"]).unwrap();
        assert!(!fewer.present(2) && !fewer.present(3));
        assert!(fewer.rest(3).is_empty());
    }

    #[test]
    fn a_call_with_too_few_or_too_many_words_is_answered_with_the_usage() {
        let usage = "usage: /go X COUNT [BLOCK] [NOTES...]";
        assert_eq!(Arguments::parse(&spec(), &[]).unwrap_err(), usage);
        assert_eq!(Arguments::parse(&spec(), &["1"]).unwrap_err(), usage);
        let fixed = CommandSpec::new("tp", "").argument(Argument::number("x"));
        assert_eq!(
            Arguments::parse(&fixed, &["1", "2"]).unwrap_err(),
            "usage: /tp X"
        );
    }

    #[test]
    fn a_word_that_is_not_what_its_argument_is_says_which_argument() {
        let wrong = |words: &[&str]| Arguments::parse(&spec(), words).unwrap_err();
        assert_eq!(
            wrong(&["x", "1"]),
            "usage: /go X COUNT [BLOCK] [NOTES...]; X must be a number"
        );
        for not_finite in ["nan", "inf", "-inf"] {
            assert!(wrong(&[not_finite, "1"]).ends_with("X must be a number"));
        }
        assert!(wrong(&["1", "1.5"]).ends_with("COUNT must be a whole number"));
        assert!(
            wrong(&["1", "2", "lamp"]).ends_with("BLOCK must be an identifier like namespace:name")
        );
    }

    #[test]
    #[should_panic(expected = "nothing can follow a rest argument")]
    fn nothing_can_follow_a_rest_argument() {
        let _ = CommandSpec::new("a", "")
            .argument(Argument::rest("all"))
            .argument(Argument::word("more"));
    }

    #[test]
    #[should_panic(expected = "a required argument cannot follow an optional one")]
    fn a_required_argument_cannot_follow_an_optional_one() {
        let _ = CommandSpec::new("a", "")
            .argument(Argument::word("first").optional())
            .argument(Argument::word("second"));
    }
}
