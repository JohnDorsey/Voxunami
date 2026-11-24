use std::fmt;

/// What a command gives back, and so what a hole is worth: nothing, a truth, a whole number, a number, text, or a list of these
/// (scripting-spec.md 4.5 and 4.6). A command that gives back nothing in particular gives [`CommandValue::None`].
#[derive(Clone, Debug, PartialEq)]
pub enum CommandValue {
    None,
    Bool(bool),
    Integer(i64),
    Float(f64),
    Text(String),
    List(Vec<CommandValue>),
}

impl CommandValue {
    /// What the value is, for a message that says what was given in place of what was wanted.
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::None => "nothing (None)",
            Self::Bool(_) => "a truth value (True or False)",
            Self::Integer(_) => "a whole number",
            Self::Float(_) => "a number with a fraction",
            Self::Text(_) => "text",
            Self::List(_) => "a list",
        }
    }

    /// The value as text where a hole is joined to words: what Python's `str` gives (`None`, `True`, `2.0`, `[1, 'a']`).
    /// Text is itself.
    pub fn to_text(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            other => other.render(false),
        }
    }

    /// The value as Python source that makes it: what Python's `repr` gives, except that a float that is not a number is written
    /// the way a program must (`float('nan')`, where `repr` says `nan`). Text gets quotes and the escapes it needs.
    ///
    /// This differs from Python in one place: Python escapes the characters it cannot print (a zero-width space, say) and this
    /// leaves every character above the controls as it is.
    pub fn to_python(&self) -> String {
        self.render(true)
    }

    fn render(&self, as_source: bool) -> String {
        match self {
            Self::None => "None".to_owned(),
            Self::Bool(true) => "True".to_owned(),
            Self::Bool(false) => "False".to_owned(),
            Self::Integer(number) => number.to_string(),
            Self::Float(number) => float_written(*number, as_source),
            Self::Text(text) => text_literal(text),
            Self::List(items) => {
                let items: Vec<String> = items.iter().map(|item| item.render(as_source)).collect();
                format!("[{}]", items.join(", "))
            }
        }
    }
}

impl fmt::Display for CommandValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_text())
    }
}

fn float_written(number: f64, as_source: bool) -> String {
    let sign = if number < 0.0 { "-" } else { "" };
    match (number.is_nan(), number.is_infinite(), as_source) {
        (true, _, true) => "float('nan')".to_owned(),
        (true, _, false) => "nan".to_owned(),
        (_, true, true) => format!("{sign}float('inf')"),
        (_, true, false) => format!("{sign}inf"),
        _ => float_text(number),
    }
}

/// Python's `repr` of a finite float: the shortest digits that read back as the same number, in positional notation for numbers
/// from 1e-4 up to (not including) 1e16 and in scientific notation outside. Rust's own shortest-digits formatter gives the digits
/// (it uses no `libm`, so they are the same on every machine); only the layout is Python's.
fn float_text(number: f64) -> String {
    if number == 0.0 {
        return if number.is_sign_negative() {
            "-0.0"
        } else {
            "0.0"
        }
        .to_owned();
    }
    let scientific = format!("{:e}", number.abs());
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("`{:e}` of a finite float has an exponent");
    let exponent: i32 = exponent.parse().expect("the exponent is a whole number");
    let digits: String = mantissa.chars().filter(|&digit| digit != '.').collect();
    let sign = if number < 0.0 { "-" } else { "" };
    if (-4..16).contains(&exponent) {
        if exponent >= 0 {
            let before_the_point = exponent as usize + 1;
            if digits.len() <= before_the_point {
                let zeros = "0".repeat(before_the_point - digits.len());
                format!("{sign}{digits}{zeros}.0")
            } else {
                let (whole, fraction) = digits.split_at(before_the_point);
                format!("{sign}{whole}.{fraction}")
            }
        } else {
            let zeros = "0".repeat((-exponent - 1) as usize);
            format!("{sign}0.{zeros}{digits}")
        }
    } else {
        let (first, rest) = digits.split_at(1);
        let point = if rest.is_empty() { "" } else { "." };
        let exponent_sign = if exponent < 0 { '-' } else { '+' };
        format!(
            "{sign}{first}{point}{rest}e{exponent_sign}{:02}",
            exponent.abs()
        )
    }
}

/// A Python string literal for text: single quotes unless the text has a single quote and no double quote.
fn text_literal(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut literal = String::with_capacity(text.len() + 2);
    literal.push(quote);
    for character in text.chars() {
        match character {
            '\\' => literal.push_str("\\\\"),
            '\n' => literal.push_str("\\n"),
            '\r' => literal.push_str("\\r"),
            '\t' => literal.push_str("\\t"),
            character if character == quote => {
                literal.push('\\');
                literal.push(character);
            }
            character if character.is_control() && (character as u32) < 0x100 => {
                literal.push_str(&format!("\\x{:02x}", character as u32));
            }
            character if character.is_control() => {
                literal.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => literal.push(character),
        }
    }
    literal.push(quote);
    literal
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each pair is a number and what `repr` of it gave when Python 3 was run on 2026-11-24 (not worked out from memory).
    #[test]
    fn a_float_is_written_as_python_writes_it() {
        let cases: &[(f64, &str)] = &[
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (1.0, "1.0"),
            (-1.0, "-1.0"),
            (0.5, "0.5"),
            (0.1, "0.1"),
            (1.5, "1.5"),
            (100.0, "100.0"),
            (123456789.0, "123456789.0"),
            (1e15, "1000000000000000.0"),
            (9999999999999998.0, "9999999999999998.0"),
            (1e16, "1e+16"),
            (1.5e16, "1.5e+16"),
            (123456789012345678.0, "1.2345678901234568e+17"),
            (0.0001, "0.0001"),
            (0.00012345, "0.00012345"),
            (0.00001, "1e-05"),
            (1.5e-5, "1.5e-05"),
            (1e-7, "1e-07"),
            (1.234e-10, "1.234e-10"),
            (5e-324, "5e-324"),
            (f64::MAX, "1.7976931348623157e+308"),
            (2.5, "2.5"),
            (12.75, "12.75"),
            (1e22, "1e+22"),
            (1e100, "1e+100"),
            (0.30000000000000004, "0.30000000000000004"),
            (1234.5678, "1234.5678"),
            (65.0, "65.0"),
            (-2.25, "-2.25"),
        ];
        for &(number, expected) in cases {
            assert_eq!(
                CommandValue::Float(number).to_text(),
                expected,
                "{number:e}"
            );
        }
    }

    #[test]
    fn a_float_that_is_not_a_number_is_written_as_code_must() {
        assert_eq!(CommandValue::Float(f64::NAN).to_python(), "float('nan')");
        assert_eq!(
            CommandValue::Float(f64::INFINITY).to_python(),
            "float('inf')"
        );
        assert_eq!(
            CommandValue::Float(f64::NEG_INFINITY).to_python(),
            "-float('inf')"
        );
        // as text, which is what `str` makes of them
        assert_eq!(CommandValue::Float(f64::NAN).to_text(), "nan");
        assert_eq!(CommandValue::Float(f64::INFINITY).to_text(), "inf");
        assert_eq!(CommandValue::Float(f64::NEG_INFINITY).to_text(), "-inf");
    }

    #[test]
    fn text_is_quoted_as_python_quotes_it() {
        let cases: &[(&str, &str)] = &[
            ("plain", "'plain'"),
            ("it's", r#""it's""#),
            (r#"say "hi""#, r#"'say "hi"'"#),
            (r#"both ' and ""#, r#"'both \' and "'"#),
            (r"back\slash", r"'back\\slash'"),
            ("tab\there", r"'tab\there'"),
            ("line\nbreak", r"'line\nbreak'"),
            ("bell\u{7}", r"'bell\x07'"),
            ("del\u{7f}", r"'del\x7f'"),
            ("é", "'é'"),
        ];
        for &(text, expected) in cases {
            assert_eq!(
                CommandValue::Text(text.to_owned()).to_python(),
                expected,
                "{text:?}"
            );
        }
    }

    #[test]
    fn a_value_is_text_the_way_str_makes_it_and_a_literal_the_way_repr_does() {
        let list = CommandValue::List(vec![
            CommandValue::Integer(1),
            CommandValue::Text("a".to_owned()),
            CommandValue::None,
            CommandValue::Bool(true),
            CommandValue::Float(2.5),
            CommandValue::List(vec![
                CommandValue::Integer(3),
                CommandValue::Text("b".to_owned()),
            ]),
        ]);
        assert_eq!(list.to_text(), "[1, 'a', None, True, 2.5, [3, 'b']]");
        assert_eq!(list.to_python(), list.to_text());
        let text = CommandValue::Text("a b".to_owned());
        assert_eq!(text.to_text(), "a b");
        assert_eq!(text.to_python(), "'a b'");
        assert_eq!(CommandValue::None.to_text(), "None");
        assert_eq!(CommandValue::Bool(false).to_text(), "False");
        assert_eq!(CommandValue::Integer(-7).to_text(), "-7");
        assert_eq!(text.to_string(), "a b");
    }
}
