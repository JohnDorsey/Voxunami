//! Scanners for where a `{>...}` hole ends (scripting-spec.md section 4.2), as scratch spikes.
//!
//! Each takes the text after `{>` and returns the byte index of the `}` that closes the hole.
//! Three are here so that they can be compared:
//!
//! * [`hole_end_ruff`]: drives the lexer that Monty itself uses (ruff's) over the text and
//!   takes the first `}` token at bracket depth 0. The tokenizer is the reference.
//! * [`hole_end_lexical`]: a small hand-written scanner that knows strings, f-strings and
//!   brackets and nothing else, so the game would not need to link ruff.
//! * [`hole_end_naive`]: counts braces and nothing else (a mutant that the tests must reject).

use ruff_python_ast::token::TokenKind;
use ruff_python_parser::{Mode, ParseOptions, parse_unchecked};
use ruff_text_size::Ranged;

/// The reference: ruff's own token stream.
pub fn hole_end_ruff(text: &str) -> Option<usize> {
    let parsed = parse_unchecked(text, ParseOptions::from(Mode::Module));
    let mut depth = 0usize;
    for token in parsed.tokens() {
        match token.kind() {
            TokenKind::Lpar | TokenKind::Lsqb | TokenKind::Lbrace => depth += 1,
            TokenKind::Rpar | TokenKind::Rsqb => depth = depth.saturating_sub(1),
            TokenKind::Rbrace => {
                if depth == 0 {
                    return Some(token.start().to_usize());
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

/// The mutant: braces only.
pub fn hole_end_naive(text: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (index, byte) in text.bytes().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' if depth == 0 => return Some(index),
            b'}' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// The hand-written scanner. `#` is not a comment here: a hole ends at its `}` whatever comes before.
pub fn hole_end_lexical(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    scan_code(bytes, 0, false).map(|(index, _)| index)
}

/// Whether the quote at `at` has a string prefix, and if so whether it is raw and whether it is an f-string.
fn prefix_of(bytes: &[u8], at: usize) -> (bool, bool) {
    let mut start = at;
    while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
        start -= 1;
    }
    let run: String = bytes[start..at].iter().map(|b| b.to_ascii_lowercase() as char).collect();
    match run.as_str() {
        "r" | "br" | "rb" | "ur" => (true, false),
        "f" | "t" => (false, true),
        "fr" | "rf" | "tr" | "rt" => (true, true),
        _ => (false, false),
    }
}

/// Code until a `}` at depth 0 (or, inside an f-string field, a `:` or a `!` that is not `!=`).
fn scan_code(bytes: &[u8], mut index: usize, in_field: bool) -> Option<(usize, u8)> {
    let mut depth = 0usize;
    while index < bytes.len() {
        let byte = bytes[index];
        match byte {
            b'(' | b'[' | b'{' => {
                depth += 1;
                index += 1;
            }
            b')' | b']' => {
                depth = depth.saturating_sub(1);
                index += 1;
            }
            b'}' => {
                if depth == 0 {
                    return Some((index, byte));
                }
                depth -= 1;
                index += 1;
            }
            b':' if in_field && depth == 0 => return Some((index, byte)),
            b'!' if in_field && depth == 0 && bytes.get(index + 1) != Some(&b'=') => return Some((index, byte)),
            b'"' | b'\'' => {
                let (raw, format) = prefix_of(bytes, index);
                index = scan_string(bytes, index, raw, format)?;
            }
            _ => index += 1,
        }
    }
    None
}

/// A string literal whose opening quote is at `index`; the index after its closing quote.
fn scan_string(bytes: &[u8], mut index: usize, raw: bool, format: bool) -> Option<usize> {
    let quote = bytes[index];
    let triple = bytes.get(index + 1) == Some(&quote) && bytes.get(index + 2) == Some(&quote);
    index += if triple { 3 } else { 1 };
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == quote {
            if !triple {
                return Some(index + 1);
            }
            if bytes.get(index + 1) == Some(&quote) && bytes.get(index + 2) == Some(&quote) {
                return Some(index + 3);
            }
            index += 1;
            continue;
        }
        match byte {
            b'\\' => {
                let next = bytes.get(index + 1).copied();
                if format && matches!(next, Some(b'{') | Some(b'}')) {
                    index += 1; // the backslash stands alone; the brace is a brace
                } else if format && !raw && next == Some(b'N') && bytes.get(index + 2) == Some(&b'{') {
                    let close = bytes[index + 2..].iter().position(|b| *b == b'}')?;
                    index += 2 + close + 1; // \N{NAME}
                } else {
                    index += 2;
                }
            }
            b'{' if format => {
                if bytes.get(index + 1) == Some(&b'{') {
                    index += 2;
                } else {
                    index = scan_field(bytes, index + 1)?;
                }
            }
            b'}' if format && bytes.get(index + 1) == Some(&b'}') => index += 2,
            _ => index += 1,
        }
    }
    None
}

/// An f-string field, from the character after `{`; the index after its closing `}`.
fn scan_field(bytes: &[u8], index: usize) -> Option<usize> {
    let (at, byte) = scan_code(bytes, index, true)?;
    match byte {
        b'}' => Some(at + 1),
        b'!' => {
            let mut next = at + 1;
            while next < bytes.len() && bytes[next] != b':' && bytes[next] != b'}' {
                next += 1;
            }
            match bytes.get(next)? {
                b'}' => Some(next + 1),
                _ => scan_spec(bytes, next + 1),
            }
        }
        _ => scan_spec(bytes, at + 1),
    }
}

/// A format spec: text, with nested fields, up to the `}` that closes the field.
fn scan_spec(bytes: &[u8], mut index: usize) -> Option<usize> {
    while index < bytes.len() {
        match bytes[index] {
            b'{' => index = scan_field(bytes, index + 1)?,
            b'}' => return Some(index + 1),
            _ => index += 1,
        }
    }
    None
}
