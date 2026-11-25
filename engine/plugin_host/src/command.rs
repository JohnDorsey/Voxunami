//! Commands a player types: what each is called, what arguments it takes, and the parsing of a typed line
//! into those arguments, so that a handler never parses text and every command answers a malformed
//! call the same way.

use line_language::{CommandValue, Item};
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

/// What a command does to the world, from a short fixed list. Permissions are granted by class as well as by name, so every
/// command a player can run on the server says which it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CommandClass {
    /// Only tells the player something.
    Read,
    /// Changes the world, the player or the game's rules.
    Change,
    /// Changes who may do what.
    Permission,
    /// Makes something that acts later or by itself (code, a command block).
    Automation,
}

impl CommandClass {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Change => "change",
            Self::Permission => "permission",
            Self::Automation => "automation",
        }
    }
}

/// The id of a command or one of its subcommands, as a permission names it: `pack:command.subcommand`. The pack is the part
/// before the colon of the id of the plugin that registered the command. (The parts are not [`NamespacedId`]s: a dot divides them.)
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CommandId {
    pack: String,
    path: Vec<String>,
}

impl CommandId {
    pub fn new(pack: &str, path: &[&str]) -> Self {
        Self {
            pack: pack.to_owned(),
            path: path.iter().map(|part| (*part).to_owned()).collect(),
        }
    }

    pub fn pack(&self) -> &str {
        &self.pack
    }

    /// The command's name, then the names of the subcommands down to this one.
    pub fn path(&self) -> &[String] {
        &self.path
    }
}

impl std::fmt::Display for CommandId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}:{}", self.pack, self.path.join("."))
    }
}

/// One node of a command's tree: the command itself or a subcommand, with the class it has (its own, or the one it takes from
/// above) and whether typing it runs something.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandNode {
    pub id: CommandId,
    pub class: Option<CommandClass>,
    /// A node that has subcommands runs only when it was declared with [`CommandSpec::runs_alone`].
    pub runs: bool,
}

/// A command whose name is made of letters, digits and `_`, starting with a letter, so that it can be named in code.
fn is_clean_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase())
        && characters.all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        })
}

/// A command: its name, what it does, and either the arguments it takes or the subcommands it has. The usage line, `/help`
/// and the permission ids are made from this tree, so they cannot disagree with what the command accepts.
#[derive(Clone, Debug)]
pub struct CommandSpec {
    pub(crate) name: &'static str,
    pub(crate) summary: &'static str,
    pub(crate) arguments: Vec<Argument>,
    subcommands: Vec<CommandSpec>,
    runs_alone: bool,
    class: Option<CommandClass>,
    prefix: char,
}

impl CommandSpec {
    pub fn new(name: &'static str, summary: &'static str) -> Self {
        Self {
            name,
            summary,
            arguments: Vec::new(),
            subcommands: Vec::new(),
            runs_alone: false,
            class: None,
            prefix: '/',
        }
    }

    /// What the command does. A subcommand without a class takes its parent's, so one class can be said once for a tree. A
    /// command that a player can run on the server must end up with one: the host refuses to start without.
    pub fn class(mut self, class: CommandClass) -> Self {
        self.class = Some(class);
        self
    }

    /// Adds a subcommand: a word typed after this command's name, with its own summary, arguments or subcommands. A command has
    /// arguments or subcommands, never both, and a subcommand is named by a clean lowercase identifier.
    pub fn subcommand(mut self, child: CommandSpec) -> Self {
        assert!(
            self.arguments.is_empty(),
            "{}{}: a command has arguments or subcommands, not both",
            self.prefix,
            self.name
        );
        assert!(
            is_clean_name(child.name),
            "{}{}: the subcommand {:?} is not a lowercase identifier",
            self.prefix,
            self.name,
            child.name
        );
        assert!(
            !self
                .subcommands
                .iter()
                .any(|existing| existing.name == child.name),
            "{}{}: two subcommands are called {}",
            self.prefix,
            self.name,
            child.name
        );
        self.subcommands.push(child);
        self
    }

    /// A command with subcommands may also be typed with none (`/level` says which level you are in). Without this, a bare
    /// command that has subcommands is answered with its usage.
    pub fn runs_alone(mut self) -> Self {
        self.runs_alone = true;
        self
    }

    pub fn prefix(&self) -> char {
        self.prefix
    }

    /// Every node of the tree, the command first, then each subcommand below it in the order they were added; with the id each
    /// has in permissions.
    pub fn nodes(&self, pack: &str) -> Vec<CommandNode> {
        let mut nodes = Vec::new();
        self.collect_nodes(pack, &mut vec![self.name], None, &mut nodes);
        nodes
    }

    fn collect_nodes(
        &self,
        pack: &str,
        path: &mut Vec<&'static str>,
        inherited: Option<CommandClass>,
        nodes: &mut Vec<CommandNode>,
    ) {
        let class = self.class.or(inherited);
        nodes.push(CommandNode {
            id: CommandId::new(pack, path),
            class,
            runs: self.subcommands.is_empty() || self.runs_alone,
        });
        for child in &self.subcommands {
            path.push(child.name);
            child.collect_nodes(pack, path, class, nodes);
            path.pop();
        }
    }

    /// The ways the command can be typed, one after the other: `/level, /level list, /level go NAME`.
    fn forms(&self, shown: &str, forms: &mut Vec<String>) {
        if self.subcommands.is_empty() || self.runs_alone {
            let mut form = shown.to_owned();
            for argument in &self.arguments {
                form.push(' ');
                form.push_str(&argument.shown());
            }
            forms.push(form);
        }
        for child in &self.subcommands {
            child.forms(&format!("{shown} {}", child.name), forms);
        }
    }

    /// The command in one short piece, for a list of commands: `/level [list|go]` for one with subcommands that also runs
    /// alone, `/ship create|push` for one that does not, and the usage for any other.
    pub fn listing(&self) -> String {
        if self.subcommands.is_empty() {
            return self.usage();
        }
        let names: Vec<&str> = self.subcommands.iter().map(|child| child.name).collect();
        let names = names.join("|");
        if self.runs_alone {
            format!("{}{} [{names}]", self.prefix, self.name)
        } else {
            format!("{}{} {names}", self.prefix, self.name)
        }
    }

    /// What `/help` says of the command, or of the subcommand the words name: its usage, a colon, and its summary. None when the
    /// words name no subcommand.
    pub fn describe(&self, words: &[&str]) -> Option<String> {
        let mut node = self;
        let mut shown = format!("{}{}", self.prefix, self.name);
        for word in words {
            node = node.subcommands.iter().find(|child| child.name == *word)?;
            shown = format!("{shown} {}", node.name);
        }
        let mut forms = Vec::new();
        node.forms(&shown, &mut forms);
        Some(format!("{}: {}", forms.join(", "), node.summary))
    }

    /// The character a player types before the name: `/` for the server's commands (the default) and
    /// `.` for the window's.
    pub fn with_prefix(mut self, prefix: char) -> Self {
        self.prefix = prefix;
        self
    }

    /// Adds the next argument. A command is written by its plugin, so a mistake in its shape is a
    /// mistake in the plugin and panics: a `Rest` argument must be last, and once an argument is optional
    /// every later one must be too.
    pub fn argument(mut self, argument: Argument) -> Self {
        assert!(
            self.subcommands.is_empty(),
            "{}{}: a command has arguments or subcommands, not both",
            self.prefix,
            self.name
        );
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

    /// Checks the words typed after the command's name. The error is the reply to give the player: the
    /// usage, and for a word that is not what its argument is, which one.
    pub fn parse<'text>(&self, words: &[&'text str]) -> Result<Arguments<'text>, String> {
        Arguments::parse(self, words)
    }

    /// Checks the words of a line whose holes have been filled in (see [`line_language`]). A word that is a hole and nothing else
    /// fills an argument with the value the hole gave, if it is of the kind the argument is, and is refused if it is not: nothing
    /// is converted by guessing. A word made of text and a hole is text, and only the last argument, the one that takes the rest,
    /// may be made of that. The error is the reply to give the player.
    pub fn parse_items<'text>(&self, items: &'text [Item]) -> Result<Arguments<'text>, String> {
        Arguments::from_items(self, items)
    }

    /// How the command is typed: `/tp X Y Z`, with optional arguments in brackets; for a command with subcommands every way it can
    /// be typed, divided by commas: `/level, /level list, /level go NAME`.
    pub fn usage(&self) -> String {
        let mut forms = Vec::new();
        self.forms(&format!("{}{}", self.prefix, self.name), &mut forms);
        forms.join(", ")
    }
}

/// A typed word as the argument of that kind, if it is one. (`Rest` is not a kind of one word.)
fn typed_word(kind: ArgumentKind, word: &str) -> Option<Value<'_>> {
    match kind {
        ArgumentKind::Word => Some(Value::Word(word)),
        ArgumentKind::Number => word
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite())
            .map(Value::Number),
        ArgumentKind::Integer => word.parse::<i64>().ok().map(Value::Integer),
        ArgumentKind::Identifier => word.parse::<NamespacedId>().ok().map(Value::Identifier),
        ArgumentKind::Rest => None,
    }
}

/// A value that a hole gave, as the argument of that kind, if it is one: a number takes a whole number or a finite one with a
/// fraction (and never a truth value), a whole number only a whole number, a word and an identifier text. `text` is the text of
/// the value, which a word borrows.
fn typed_value<'text>(
    kind: ArgumentKind,
    given: &CommandValue,
    text: &'text str,
) -> Option<Value<'text>> {
    match (kind, given) {
        (ArgumentKind::Word, CommandValue::Text(word))
            if !word.is_empty()
                && !word
                    .chars()
                    .any(|character| character.is_whitespace() || character.is_control()) =>
        {
            Some(Value::Word(text))
        }
        (ArgumentKind::Number, CommandValue::Integer(number)) => {
            Some(Value::Number(*number as f64))
        }
        (ArgumentKind::Number, CommandValue::Float(number)) if number.is_finite() => {
            Some(Value::Number(*number))
        }
        (ArgumentKind::Integer, CommandValue::Integer(number)) => Some(Value::Integer(*number)),
        (ArgumentKind::Identifier, CommandValue::Text(word)) => {
            word.parse::<NamespacedId>().ok().map(Value::Identifier)
        }
        _ => None,
    }
}

/// What to say a hole gave, when it was not what the argument takes.
fn what_was_given(kind: ArgumentKind, given: &CommandValue) -> &'static str {
    match (kind, given) {
        (ArgumentKind::Word, CommandValue::Text(_)) => {
            "text that is empty or holds a blank or control character"
        }
        _ => given.kind_name(),
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
    subcommand: Vec<&'static str>,
}

/// Walks down the tree of a command by the words that name subcommands. Gives the node the words end at, the names passed on the
/// way, and how many words they were; or the usage of the whole command, for a word that is no subcommand of the node it follows
/// and for a node that has subcommands, is not meant to run alone and was given none.
fn descend<'spec, 'word>(
    root: &'spec CommandSpec,
    count: usize,
    word_at: impl Fn(usize) -> &'word str,
) -> Result<(&'spec CommandSpec, Vec<&'static str>, usize), String> {
    let mut node = root;
    let mut path = Vec::new();
    let mut used = 0;
    while !node.subcommands.is_empty() {
        if used == count {
            if node.runs_alone {
                break;
            }
            return Err(format!("usage: {}", root.usage()));
        }
        let word = word_at(used);
        let Some(child) = node.subcommands.iter().find(|child| child.name == word) else {
            return Err(format!("usage: {}", root.usage()));
        };
        path.push(child.name);
        node = child;
        used += 1;
    }
    Ok((node, path, used))
}

impl<'text> Arguments<'text> {
    /// Checks the words typed after a command's name. The error is the reply to give the player:
    /// the usage, and for a word that is not what its argument is, which one.
    pub(crate) fn parse(root: &CommandSpec, words: &[&'text str]) -> Result<Self, String> {
        let (spec, subcommand, used) = descend(root, words.len(), |index| words[index])?;
        let words = &words[used..];
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
            return Err(format!("usage: {}", root.usage()));
        }
        let mut values = Vec::with_capacity(spec.arguments.len());
        for (position, argument) in spec.arguments.iter().enumerate() {
            let Some(&word) = words.get(position) else {
                values.push(Value::Absent);
                continue;
            };
            let value = match argument.kind {
                ArgumentKind::Rest => Some(Value::Rest(words[position..].to_vec())),
                kind => typed_word(kind, word),
            };
            let Some(value) = value else {
                return Err(format!(
                    "usage: {}; {} must be {}",
                    root.usage(),
                    argument.name.to_uppercase(),
                    argument.kind.description()
                ));
            };
            values.push(value);
        }
        Ok(Self { values, subcommand })
    }

    /// As [`Arguments::parse`], for the words of a line with holes in it.
    pub(crate) fn from_items(root: &CommandSpec, items: &'text [Item]) -> Result<Self, String> {
        // (a word that names a subcommand is matched by its text, whether a hole gave it or it was typed)
        let (spec, subcommand, used) =
            descend(root, items.len(), |index| items[index].text.as_str())?;
        let items = &items[used..];
        let required = spec
            .arguments
            .iter()
            .filter(|argument| !argument.optional)
            .count();
        let takes_rest = spec
            .arguments
            .last()
            .is_some_and(|argument| argument.kind == ArgumentKind::Rest);
        if items.len() < required || (!takes_rest && items.len() > spec.arguments.len()) {
            return Err(format!("usage: {}", root.usage()));
        }
        let mut values = Vec::with_capacity(spec.arguments.len());
        for (position, argument) in spec.arguments.iter().enumerate() {
            let Some(item) = items.get(position) else {
                values.push(Value::Absent);
                continue;
            };
            let name = argument.name.to_uppercase();
            if argument.kind == ArgumentKind::Rest {
                // what the holes gave is pasted as text, and a result with blanks in it is more words
                let words: Vec<&str> = items[position..]
                    .iter()
                    .flat_map(|item| item.text.split_whitespace())
                    .collect();
                if words.is_empty() && !argument.optional {
                    return Err(format!("usage: {}", root.usage()));
                }
                values.push(Value::Rest(words));
                continue;
            }
            let value = if !item.has_hole {
                typed_word(argument.kind, &item.text).ok_or_else(|| {
                    format!(
                        "usage: {}; {name} must be {}",
                        root.usage(),
                        argument.kind.description()
                    )
                })?
            } else if let Some(given) = &item.whole {
                typed_value(argument.kind, given, &item.text).ok_or_else(|| {
                    let hole = item
                        .first_hole
                        .map_or_else(|| "a hole".to_owned(), |hole| hole.to_string());
                    format!(
                        "usage: {}; {name} must be {}, and {hole} gave {}",
                        root.usage(),
                        argument.kind.description(),
                        what_was_given(argument.kind, given)
                    )
                })?
            } else {
                return Err(format!(
                    "usage: {}; {name} is made of text and a hole, and only the argument that takes the rest can be",
                    root.usage()
                ));
            };
            values.push(value);
        }
        Ok(Self { values, subcommand })
    }

    /// The names of the subcommands the call went down, in order: empty for a command without subcommands and for one typed
    /// alone. The positions of the getters below count the arguments of the node it ended at.
    pub fn subcommand(&self) -> &[&'static str] {
        &self.subcommand
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

    fn whole(value: CommandValue) -> Item {
        Item {
            text: value.to_text(),
            has_hole: true,
            whole: Some(value),
            first_hole: None,
        }
    }

    fn text_item(text: &str) -> Item {
        Item {
            text: text.to_owned(),
            has_hole: false,
            whole: None,
            first_hole: None,
        }
    }

    #[test]
    fn a_value_fills_an_argument_of_its_kind_and_no_other() {
        let text = |text: &str| CommandValue::Text(text.to_owned());
        let spec = |argument: Argument| CommandSpec::new("c", "").argument(argument);
        let accepts = |argument: Argument, value: CommandValue| {
            spec(argument).parse_items(&[whole(value)]).is_ok()
        };
        // a number takes a whole number and a finite one, never a truth value or a text that reads as one
        assert!(accepts(Argument::number("n"), CommandValue::Integer(-3)));
        assert!(accepts(Argument::number("n"), CommandValue::Float(0.25)));
        assert!(!accepts(
            Argument::number("n"),
            CommandValue::Float(f64::NAN)
        ));
        assert!(!accepts(
            Argument::number("n"),
            CommandValue::Float(f64::INFINITY)
        ));
        assert!(!accepts(Argument::number("n"), CommandValue::Bool(true)));
        assert!(!accepts(Argument::number("n"), text("5")));
        assert!(!accepts(Argument::number("n"), CommandValue::None));
        // a whole number takes a whole number, and not 2.0
        assert!(accepts(Argument::integer("n"), CommandValue::Integer(7)));
        assert!(!accepts(Argument::integer("n"), CommandValue::Float(2.0)));
        assert!(!accepts(Argument::integer("n"), text("7")));
        // a word takes text with no blank and no control character, and something in it
        assert!(accepts(Argument::word("w"), text("a:b?")));
        for bad in ["", "a b", "a\tb", "a\u{7}b", " "] {
            assert!(!accepts(Argument::word("w"), text(bad)), "{bad:?}");
        }
        assert!(!accepts(Argument::word("w"), CommandValue::Integer(1)));
        // an identifier takes text that is one
        assert!(accepts(Argument::identifier("i"), text("a:b")));
        assert!(!accepts(Argument::identifier("i"), text("ab")));
        assert!(!accepts(
            Argument::identifier("i"),
            CommandValue::Integer(1)
        ));
        // typed text is read as it always was
        assert!(
            spec(Argument::number("n"))
                .parse_items(&[text_item("2.5")])
                .is_ok()
        );
        assert!(
            spec(Argument::number("n"))
                .parse_items(&[text_item("two")])
                .is_err()
        );
    }

    #[test]
    fn the_words_of_the_rest_are_the_words_of_all_that_remains_holes_included() {
        let spec = CommandSpec::new("c", "")
            .argument(Argument::word("first"))
            .argument(Argument::rest("rest").optional());
        let items = [
            text_item("a"),
            text_item("b"),
            whole(CommandValue::Text("c d".to_owned())),
            whole(CommandValue::Integer(5)),
        ];
        let arguments = spec.parse_items(&items).unwrap();
        assert_eq!(arguments.word(0), "a");
        assert_eq!(arguments.rest(1), ["b", "c", "d", "5"]);
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

    fn tree() -> CommandSpec {
        CommandSpec::new("level", "which level you are in")
            .class(CommandClass::Read)
            .runs_alone()
            .subcommand(CommandSpec::new("list", "lists them"))
            .subcommand(
                CommandSpec::new("go", "moves you")
                    .class(CommandClass::Change)
                    .argument(Argument::word("name")),
            )
    }

    #[test]
    fn the_usage_of_a_tree_is_every_way_to_type_it() {
        assert_eq!(tree().usage(), "/level, /level list, /level go NAME");
        let not_alone = CommandSpec::new("pen", "")
            .subcommand(CommandSpec::new("up", ""))
            .subcommand(CommandSpec::new("down", "").argument(Argument::number("by").optional()));
        assert_eq!(not_alone.usage(), "/pen up, /pen down [BY]");
        assert_eq!(not_alone.listing(), "/pen up|down");
        assert_eq!(tree().listing(), "/level [list|go]");
        // a command without subcommands lists as it is typed
        assert_eq!(spec().listing(), spec().usage());
    }

    #[test]
    fn describe_walks_down_the_words_and_says_none_for_a_word_that_is_no_subcommand() {
        assert_eq!(
            tree().describe(&[]).unwrap(),
            "/level, /level list, /level go NAME: which level you are in"
        );
        assert_eq!(
            tree().describe(&["go"]).unwrap(),
            "/level go NAME: moves you"
        );
        assert_eq!(
            tree().describe(&["list"]).unwrap(),
            "/level list: lists them"
        );
        assert_eq!(tree().describe(&["fly"]), None);
        assert_eq!(tree().describe(&["go", "x"]), None);
    }

    #[test]
    fn a_subcommand_takes_the_class_of_the_command_above_it_unless_it_has_its_own() {
        let ids: Vec<(String, Option<CommandClass>, bool)> = tree()
            .nodes("game")
            .into_iter()
            .map(|node| (node.id.to_string(), node.class, node.runs))
            .collect();
        assert_eq!(
            ids,
            [
                ("game:level".to_owned(), Some(CommandClass::Read), true),
                ("game:level.list".to_owned(), Some(CommandClass::Read), true),
                ("game:level.go".to_owned(), Some(CommandClass::Change), true),
            ]
        );
        // a command with no class anywhere says so, and one with subcommands that does not run alone does not run
        let loose = CommandSpec::new("pen", "").subcommand(CommandSpec::new("up", ""));
        let nodes = loose.nodes("game");
        assert_eq!(nodes[0].class, None);
        assert!(!nodes[0].runs);
        assert!(nodes[1].runs);
        let id = &nodes[1].id;
        assert_eq!(
            (id.pack(), id.path()),
            ("game", ["pen".to_owned(), "up".to_owned()].as_slice())
        );
    }

    #[test]
    fn the_names_of_the_classes_are_the_four_of_the_spec() {
        let names: Vec<&str> = [
            CommandClass::Read,
            CommandClass::Change,
            CommandClass::Permission,
            CommandClass::Automation,
        ]
        .into_iter()
        .map(CommandClass::name)
        .collect();
        assert_eq!(names, ["read", "change", "permission", "automation"]);
    }

    #[test]
    fn the_words_go_down_the_tree_and_the_arguments_are_those_of_the_node_reached() {
        let go = Arguments::parse(&tree(), &["go", "moon"]).unwrap();
        assert_eq!(go.subcommand(), ["go"]);
        assert_eq!(go.word(0), "moon");
        assert_eq!(
            Arguments::parse(&tree(), &[]).unwrap().subcommand(),
            [] as [&str; 0]
        );
        assert_eq!(
            Arguments::parse(&tree(), &["list"]).unwrap().subcommand(),
            ["list"]
        );
        let usage = "usage: /level, /level list, /level go NAME";
        for wrong in [&["fly"][..], &["go"], &["go", "a", "b"], &["list", "x"]] {
            assert_eq!(
                Arguments::parse(&tree(), wrong).unwrap_err(),
                usage,
                "{wrong:?}"
            );
        }
    }

    #[test]
    fn a_word_that_a_hole_gave_can_name_the_subcommand() {
        let items = [
            Item {
                text: "go".to_owned(),
                has_hole: true,
                whole: Some(CommandValue::Text("go".to_owned())),
                first_hole: None,
            },
            text_item("moon"),
        ];
        let arguments = tree().parse_items(&items).unwrap();
        assert_eq!(arguments.subcommand(), ["go"]);
        assert_eq!(arguments.word(0), "moon");
    }

    #[test]
    #[should_panic(expected = "a command has arguments or subcommands, not both")]
    fn a_command_with_arguments_cannot_get_subcommands() {
        let _ = CommandSpec::new("a", "")
            .argument(Argument::word("x"))
            .subcommand(CommandSpec::new("b", ""));
    }

    #[test]
    #[should_panic(expected = "a command has arguments or subcommands, not both")]
    fn a_command_with_subcommands_cannot_get_arguments() {
        let _ = CommandSpec::new("a", "")
            .subcommand(CommandSpec::new("b", ""))
            .argument(Argument::word("x"));
    }

    #[test]
    #[should_panic(expected = "is not a lowercase identifier")]
    fn a_subcommand_is_named_by_a_clean_lowercase_identifier() {
        let _ = CommandSpec::new("a", "").subcommand(CommandSpec::new("Go-Now", ""));
    }

    #[test]
    #[should_panic(expected = "two subcommands are called go")]
    fn two_subcommands_cannot_have_one_name() {
        let _ = CommandSpec::new("a", "")
            .subcommand(CommandSpec::new("go", ""))
            .subcommand(CommandSpec::new("go", ""));
    }

    #[test]
    fn a_word_that_is_not_what_its_argument_is_says_so_after_the_usage_of_the_whole_command() {
        let spec = CommandSpec::new("zone", "")
            .subcommand(CommandSpec::new("add", "").argument(Argument::integer("n")))
            .subcommand(CommandSpec::new("drop", ""));
        assert_eq!(
            Arguments::parse(&spec, &["add", "x"]).unwrap_err(),
            "usage: /zone add N, /zone drop; N must be a whole number"
        );
        // (and the same for a line with holes in it)
        assert_eq!(
            spec.parse_items(&[text_item("add"), text_item("x")])
                .unwrap_err(),
            "usage: /zone add N, /zone drop; N must be a whole number"
        );
    }
}
