/// A command a player typed, split into its name and arguments: `/level go nether` is the name
/// `level` and the arguments `go` and `nether`. The leading slash is optional.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandLine<'text> {
    pub name: &'text str,
    pub arguments: Vec<&'text str>,
}

impl<'text> CommandLine<'text> {
    /// `None` for text with no command in it (empty, spaces, or only a slash).
    pub fn parse(text: &'text str) -> Option<Self> {
        let text = text.trim();
        let text = text.strip_prefix('/').unwrap_or(text);
        let mut words = text.split_whitespace();
        let name = words.next()?;
        Some(Self {
            name,
            arguments: words.collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_has_a_name_and_arguments_and_the_slash_is_optional() {
        let level = CommandLine::parse("/level go nether").unwrap();
        assert_eq!(level.name, "level");
        assert_eq!(level.arguments, ["go", "nether"]);
        assert_eq!(CommandLine::parse("level go nether"), Some(level));
        assert_eq!(
            CommandLine::parse("  /help  "),
            Some(CommandLine {
                name: "help",
                arguments: Vec::new()
            })
        );
        assert_eq!(
            CommandLine::parse("tp   1  2\t3").unwrap().arguments,
            ["1", "2", "3"]
        );
    }

    #[test]
    fn text_with_no_command_in_it_is_none() {
        for text in ["", "   ", "/", " / "] {
            assert_eq!(CommandLine::parse(text), None, "{text:?}");
        }
    }
}
