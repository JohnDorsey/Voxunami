//! The line a player types a command on: opened with `/`, edited with the keyboard, sent
//! with Enter, dropped with Escape. It only holds the text; the window decides when keys go to it.

use protocol::MAXIMUM_COMMAND_LENGTH;

/// Whether a key press opens the command line: the key that types a slash, and nothing else, so a
/// player who presses Enter (as one does when a game starts, or to confirm) does not find every
/// key typing into the title bar. Only a press the player makes counts: not one the system repeats
/// for a key held down, not one it makes up on gaining focus, and not one of a key the window
/// already knows is down.
pub fn opens_a_command(
    text: Option<&str>,
    repeated: bool,
    synthetic: bool,
    already_down: bool,
) -> bool {
    !repeated && !synthetic && !already_down && text == Some("/")
}

#[derive(Debug, Default)]
pub struct CommandEntry {
    /// The text typed so far, or `None` while no command is being typed.
    line: Option<String>,
}

impl CommandEntry {
    pub fn is_active(&self) -> bool {
        self.line.is_some()
    }

    /// Starts a command with `first` already typed (the slash that opened it, or nothing).
    pub fn begin(&mut self, first: &str) {
        self.line = Some(String::new());
        self.insert(first);
    }

    /// Types text. Control characters (the keys that edit rather than type) are ignored, and so is
    /// whatever would make the line longer than a command may be. Does nothing when no command is
    /// being typed.
    pub fn insert(&mut self, text: &str) {
        let Some(line) = &mut self.line else {
            return;
        };
        for character in text.chars().filter(|character| !character.is_control()) {
            if line.len() + character.len_utf8() > MAXIMUM_COMMAND_LENGTH {
                break;
            }
            line.push(character);
        }
    }

    /// Removes the last character typed.
    pub fn backspace(&mut self) {
        if let Some(line) = &mut self.line {
            line.pop();
        }
    }

    /// Ends the command and returns what was typed, unless it was nothing but spaces.
    pub fn submit(&mut self) -> Option<String> {
        let line = self.line.take()?;
        (!line.trim().is_empty()).then_some(line)
    }

    /// Throws the command away.
    pub fn cancel(&mut self) {
        self.line = None;
    }

    /// What to show while typing: the line, and a mark for where the next character goes.
    pub fn display(&self) -> Option<String> {
        self.line.as_ref().map(|line| format!("> {line}_"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_fresh_press_of_slash_opens_a_command() {
        assert!(opens_a_command(Some("/"), false, false, false));
        // Enter, and the keys that move the player or choose things, do not.
        assert!(!opens_a_command(None, false, false, false));
        assert!(!opens_a_command(Some("\r"), false, false, false));
        assert!(!opens_a_command(Some("w"), false, false, false));
        assert!(!opens_a_command(Some("1"), false, false, false));
        // A held key repeating, a press made up on gaining focus, and a key already down do not.
        assert!(!opens_a_command(Some("/"), true, false, false));
        assert!(!opens_a_command(Some("/"), false, true, false));
        assert!(!opens_a_command(Some("/"), false, false, true));
    }

    #[test]
    fn typing_editing_and_sending_a_command() {
        let mut entry = CommandEntry::default();
        assert!(!entry.is_active());
        assert_eq!(entry.display(), None);
        entry.begin("/");
        assert!(entry.is_active());
        entry.insert("level go cellr");
        entry.backspace();
        entry.insert("ar");
        assert_eq!(entry.display().as_deref(), Some("> /level go cellar_"));
        assert_eq!(entry.submit().as_deref(), Some("/level go cellar"));
        assert!(!entry.is_active());
        assert_eq!(entry.display(), None);
    }

    #[test]
    fn a_command_can_start_empty_be_cancelled_and_be_nothing() {
        let mut entry = CommandEntry::default();
        entry.begin("");
        assert_eq!(entry.display().as_deref(), Some("> _"));
        entry.insert("abc");
        entry.cancel();
        assert!(!entry.is_active());
        assert_eq!(entry.submit(), None);

        entry.begin("/");
        entry.backspace();
        entry.backspace(); // nothing left to remove
        entry.insert("   ");
        assert_eq!(entry.submit(), None, "spaces alone are not a command");
        assert!(!entry.is_active(), "but the line still ends");
    }

    #[test]
    fn nothing_is_typed_unless_a_command_is_open_and_control_characters_are_ignored() {
        let mut entry = CommandEntry::default();
        entry.insert("ignored");
        entry.backspace();
        assert!(!entry.is_active());
        entry.begin("");
        entry.insert("a\r\n\u{8}\u{1b}b\tc");
        assert_eq!(entry.submit().as_deref(), Some("abc"));
    }

    #[test]
    fn a_line_never_grows_past_the_longest_command_and_never_splits_a_character() {
        let mut entry = CommandEntry::default();
        entry.begin("");
        entry.insert(&"a".repeat(MAXIMUM_COMMAND_LENGTH - 1));
        entry.insert("é"); // two bytes: would be one too many
        entry.insert("b");
        let line = entry.submit().unwrap();
        assert_eq!(line.len(), MAXIMUM_COMMAND_LENGTH);
        assert!(line.ends_with('b'));
        entry.begin("");
        entry.insert(&"é".repeat(MAXIMUM_COMMAND_LENGTH));
        assert_eq!(entry.submit().unwrap().len(), MAXIMUM_COMMAND_LENGTH);
    }
}
