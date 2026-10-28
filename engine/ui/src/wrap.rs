//! Breaking a line of text into lines that fit a width.

/// A space that a line is not broken at, and that is drawn as a space: it keeps words together (a coordinate and the letter that names it).
pub const NO_BREAK_SPACE: char = '\u{a0}';

/// `text` as lines of at most `columns` letters (at least 1), broken at spaces where it can be and in the middle of a word only where the word alone is longer than a line.
/// A run of spaces inside a line is kept; the space a line is broken at is not drawn, and nor are spaces at the end of a line that is followed by another or at the start of one that goes on from another. Every
/// [`NO_BREAK_SPACE`] comes out as a plain space. The text of no letters is one empty line.
pub fn wrap_text(text: &str, columns: usize) -> Vec<String> {
    let columns = columns.max(1);
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    let mut length = 0usize;
    // Whether the line being made has a word in it (the very first word of the text may be empty, for a text that starts with a space).
    let mut started = false;
    for word in text.split(' ') {
        let word_length = word.chars().count();
        if started {
            if length + 1 + word_length <= columns {
                line.push(' ');
                line.push_str(word);
                length += 1 + word_length;
                continue;
            }
            // It does not fit after what is there: that line is done.
            // (And its spaces at the end are not drawn.)
            let done = std::mem::take(&mut line);
            lines.push(done.trim_end_matches(' ').to_owned());
            started = false;
        }
        if !lines.is_empty() && word.is_empty() {
            // (Spaces where a line goes on from another are not drawn.)
            continue;
        }
        // The first word of a line: a word that is longer than a line is cut into lines, and what follows goes on the last piece.
        let mut rest = word;
        while rest.chars().count() > columns {
            let cut = rest
                .char_indices()
                .nth(columns)
                .map_or(rest.len(), |(at, _)| at);
            lines.push(rest[..cut].to_owned());
            rest = &rest[cut..];
        }
        line.push_str(rest);
        length = rest.chars().count();
        started = true;
    }
    if started || lines.is_empty() {
        lines.push(line);
    }
    lines
        .into_iter()
        .map(|line| line.replace(NO_BREAK_SPACE, " "))
        .collect()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn wrapped(text: &str, columns: usize) -> Vec<String> {
        wrap_text(text, columns)
    }

    #[test]
    fn text_that_fits_is_one_line_exactly_as_it_was() {
        assert_eq!(wrapped("hello world", 11), ["hello world"]);
        assert_eq!(wrapped("hello world", 80), ["hello world"]);
        assert_eq!(wrapped("", 10), [""]);
        // (Spaces inside a line are kept, and so are the ones at its ends when there is no break.)
        assert_eq!(wrapped("a  b", 10), ["a  b"]);
        assert_eq!(wrapped("> typed ", 10), ["> typed "]);
    }

    #[test]
    fn a_line_is_broken_at_the_last_space_that_fits_and_the_space_is_not_drawn() {
        assert_eq!(wrapped("hello world", 10), ["hello", "world"]);
        assert_eq!(
            wrapped("one two three four", 9),
            ["one two", "three", "four"]
        );
        assert_eq!(wrapped("one two three four", 13), ["one two three", "four"]);
        assert_eq!(wrapped("aaa bbb ccc", 7), ["aaa bbb", "ccc"]);
        assert_eq!(wrapped("aaa bbb ccc", 8), ["aaa bbb", "ccc"]);
    }

    #[test]
    fn a_word_longer_than_a_line_is_cut_and_what_follows_it_goes_on_the_last_piece() {
        assert_eq!(wrapped("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(wrapped("abcdefghij klm", 4), ["abcd", "efgh", "ij", "klm"]);
        assert_eq!(wrapped("abcdefgh a", 4), ["abcd", "efgh", "a"]);
        assert_eq!(wrapped("x abcdefghij", 4), ["x", "abcd", "efgh", "ij"]);
        // A column of nothing is a column of one.
        assert_eq!(wrapped("abc", 0), ["a", "b", "c"]);
    }

    #[test]
    fn spaces_are_not_drawn_where_a_line_goes_on() {
        assert_eq!(wrapped("aaaa   bbbb", 4), ["aaaa", "bbbb"]);
        assert_eq!(wrapped("aaaa   bbbb", 5), ["aaaa", "bbbb"]);
    }

    #[test]
    fn a_no_break_space_keeps_two_words_together_and_comes_out_as_a_space() {
        let joined = format!("x{NO_BREAK_SPACE}0x1f y{NO_BREAK_SPACE}0x2e");
        assert_eq!(wrapped(&joined, 20), ["x 0x1f y 0x2e"]);
        assert_eq!(wrapped(&joined, 7), ["x 0x1f", "y 0x2e"]);
        // Not even where that makes a line too long: it is cut inside the word as any other.
        assert_eq!(wrapped(&joined, 4), ["x 0x", "1f", "y 0x", "2e"]);
    }

    #[test]
    fn letters_are_counted_not_bytes() {
        assert_eq!(wrapped("éé éé", 2), ["éé", "éé"]);
        assert_eq!(wrapped("ééééé", 2), ["éé", "éé", "é"]);
    }

    proptest! {
        /// Whatever the text and the width: no line is longer than a line, no word is lost or changed or moved, and nothing is made up.
        #[test]
        fn no_line_is_too_long_and_the_words_are_the_same_in_the_same_order(
            text in "[a-z ]{0,120}",
            columns in 1usize..30,
        ) {
            let lines = wrap_text(&text, columns);
            prop_assert!(!lines.is_empty());
            for line in &lines {
                prop_assert!(line.chars().count() <= columns, "{line:?} in {columns}");
            }
            // The letters, with every space taken out, are the same letters.
            let before: String = text.chars().filter(|c| *c != ' ').collect();
            let after: String = lines.iter().flat_map(|line| line.chars()).filter(|c| *c != ' ').collect();
            prop_assert_eq!(before, after);
        }

        /// A word that fits a line is never cut.
        #[test]
        fn a_word_that_fits_is_whole_on_a_line(
            words in proptest::collection::vec("[a-z]{1,8}", 1..12),
            columns in 8usize..30,
        ) {
            let text = words.join(" ");
            let lines = wrap_text(&text, columns);
            let found: Vec<&str> = lines.iter().flat_map(|line| line.split(' ')).collect();
            let wanted: Vec<&str> = words.iter().map(String::as_str).collect();
            prop_assert_eq!(found, wanted);
        }
    }
}
