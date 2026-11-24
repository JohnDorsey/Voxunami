//! Finding the holes in a line (scripting-spec.md 4.1 to 4.3).
//!
//! A line is text in one language, picked by its first character (its [`Sigil`]). In that text a hole starts at `{` and a sigil
//! and ends at the `}` that closes it; what is inside is text in the language the hole's own sigil picks. Each language says where
//! its holes end: commands (`/` and `.`) at the first `}` not escaped and not closing a hole inside, code (`>`) at the `}` that
//! closes it as Python would see it.

use std::fmt;

/// How many holes may stand inside one another (a number to measure, not a principle).
pub const MAXIMUM_DEPTH: usize = 8;

/// How many holes one line may hold: each runs a command or code, so a line must not hold a crowd of them.
pub const MAXIMUM_HOLES: usize = 64;

/// The character that says what language text is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sigil {
    /// `/`: a command for the server.
    Server,
    /// `.`: a command for the window.
    Client,
    /// `>`: code for the server's interpreter.
    Code,
}

impl Sigil {
    pub fn character(self) -> char {
        match self {
            Self::Server => '/',
            Self::Client => '.',
            Self::Code => '>',
        }
    }

    pub fn from_character(character: char) -> Option<Self> {
        match character {
            '/' => Some(Self::Server),
            '.' => Some(Self::Client),
            '>' => Some(Self::Code),
            _ => None,
        }
    }
}

/// Text in one language: pieces of text, and the holes between them. The text is what remains after the escapes are decoded
/// (commands) or, for code, the code the interpreter is to read.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub segments: Vec<Segment>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Segment {
    Text(String),
    Hole(Hole),
}

/// One hole: `{`, its sigil, its text, `}`.
#[derive(Clone, Debug, PartialEq)]
pub struct Hole {
    /// The language of what is inside.
    pub sigil: Sigil,
    /// Counted from 1 in the order the braces open in the line, so an outer hole has a lower number than the ones in it.
    pub number: usize,
    /// Where the `{` is in the text that was scanned, and the byte after the `}`.
    pub start: usize,
    pub end: usize,
    /// The place of the `{` as a player counts it: the sigil of the line is the first character.
    pub column: usize,
    pub body: Layer,
}

/// A scanned line.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub sigil: Sigil,
    pub layer: Layer,
    /// How many holes there are, nested ones included.
    pub holes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanErrorKind {
    /// A hole is opened and the text ends before it is closed.
    Unclosed,
    TooDeep,
    TooManyHoles,
    /// `{*` and `{**` are kept for unpacking, later.
    ReservedBrace,
    /// `{/` or `{>` inside code. (A client hole is allowed there.)
    ReservedInCode(Sigil),
}

/// Why a line could not be scanned, and at which character (counting the sigil as the first) the hole in question opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanError {
    pub kind: ScanErrorKind,
    pub column: usize,
}

impl fmt::Display for ScanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let column = self.column;
        match self.kind {
            ScanErrorKind::Unclosed => write!(
                formatter,
                "the hole that opens at character {column} is never closed: it needs a }}, or write \\{{ for a brace that is only text"
            ),
            ScanErrorKind::TooDeep => write!(
                formatter,
                "the hole at character {column} is nested more than {MAXIMUM_DEPTH} deep"
            ),
            ScanErrorKind::TooManyHoles => write!(
                formatter,
                "the hole at character {column} is more than the {MAXIMUM_HOLES} holes a line may hold"
            ),
            ScanErrorKind::ReservedBrace => write!(
                formatter,
                "{{* and {{** at character {column} are kept for a later use; write \\{{ for a brace that is only text"
            ),
            ScanErrorKind::ReservedInCode(sigil) => write!(
                formatter,
                "{{{} at character {column} cannot start a hole inside code; write \\{{ for a brace that is only text",
                sigil.character()
            ),
        }
    }
}

impl std::error::Error for ScanError {}

/// Finds the holes in `text`, the text of a line in the language of `sigil` without that sigil.
pub fn scan(sigil: Sigil, text: &str) -> Result<Line, ScanError> {
    let mut scanner = Scanner {
        text,
        bytes: text.as_bytes(),
        holes: 0,
    };
    let (layer, _) = match sigil {
        Sigil::Code => scanner.code(0, None, 0)?,
        Sigil::Server | Sigil::Client => scanner.commands(0, None, 0)?,
    };
    Ok(Line {
        sigil,
        layer,
        holes: scanner.holes,
    })
}

/// Puts a backslash before each backslash and brace, so that [`scan`] reads the result back as the text it was made from. This is
/// what puts a result made elsewhere into a line without it ever being read as a hole.
pub fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if matches!(character, '\\' | '{' | '}') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

struct Scanner<'text> {
    text: &'text str,
    bytes: &'text [u8],
    holes: usize,
}

/// Collects the pieces of one layer: the text between holes, written down only when a hole or the end comes.
struct Pieces {
    segments: Vec<Segment>,
    text: String,
}

impl Pieces {
    fn new() -> Self {
        Self {
            segments: Vec::new(),
            text: String::new(),
        }
    }

    fn flush(&mut self) {
        if !self.text.is_empty() {
            self.segments
                .push(Segment::Text(std::mem::take(&mut self.text)));
        }
    }

    fn hole(&mut self, hole: Hole) {
        self.flush();
        self.segments.push(Segment::Hole(hole));
    }

    fn finish(mut self) -> Layer {
        self.flush();
        Layer {
            segments: self.segments,
        }
    }
}

impl Scanner<'_> {
    /// The place of the byte at `index` as a player counts it.
    fn column(&self, index: usize) -> usize {
        self.text[..index].chars().count() + 2
    }

    fn error(&self, kind: ScanErrorKind, index: usize) -> ScanError {
        ScanError {
            kind,
            column: self.column(index),
        }
    }

    /// The hole whose `{` is at `start`. The text of a hole begins two bytes later.
    fn hole(&mut self, start: usize, depth: usize) -> Result<Hole, ScanError> {
        if depth > MAXIMUM_DEPTH {
            return Err(self.error(ScanErrorKind::TooDeep, start));
        }
        if self.holes >= MAXIMUM_HOLES {
            return Err(self.error(ScanErrorKind::TooManyHoles, start));
        }
        self.holes += 1;
        let number = self.holes;
        let sigil = Sigil::from_character(char::from(self.bytes[start + 1]))
            .expect("a hole starts at a brace and a sigil");
        let (body, end) = match sigil {
            Sigil::Code => self.code(start + 2, Some(start), depth)?,
            Sigil::Server | Sigil::Client => self.commands(start + 2, Some(start), depth)?,
        };
        Ok(Hole {
            sigil,
            number,
            start,
            end,
            column: self.column(start),
            body,
        })
    }

    /// Text in a command language from `at`: up to the end of the text when `opened_at` is `None`, or up to the `}` that closes
    /// the hole opened at `opened_at`. Returns the layer and the index after the closing brace (or the end).
    fn commands(
        &mut self,
        mut at: usize,
        opened_at: Option<usize>,
        depth: usize,
    ) -> Result<(Layer, usize), ScanError> {
        let mut pieces = Pieces::new();
        let mut run = at;
        loop {
            match self.bytes.get(at).copied() {
                None => {
                    if let Some(open) = opened_at {
                        return Err(self.error(ScanErrorKind::Unclosed, open));
                    }
                    pieces.text.push_str(&self.text[run..at]);
                    return Ok((pieces.finish(), at));
                }
                Some(b'\\') => match self.bytes.get(at + 1) {
                    Some(&escaped @ (b'\\' | b'{' | b'}')) => {
                        pieces.text.push_str(&self.text[run..at]);
                        pieces.text.push(char::from(escaped));
                        at += 2;
                        run = at;
                    }
                    _ => at += 1,
                },
                Some(b'}') if opened_at.is_some() => {
                    pieces.text.push_str(&self.text[run..at]);
                    return Ok((pieces.finish(), at + 1));
                }
                Some(b'{') => match self.bytes.get(at + 1) {
                    Some(b'/' | b'.' | b'>') => {
                        pieces.text.push_str(&self.text[run..at]);
                        let hole = self.hole(at, depth + 1)?;
                        at = hole.end;
                        run = at;
                        pieces.hole(hole);
                    }
                    Some(b'*') => return Err(self.error(ScanErrorKind::ReservedBrace, at)),
                    _ => at += 1,
                },
                Some(_) => at += 1,
            }
        }
    }

    /// Code from `at`, as for [`Scanner::commands`]. The blanks at its start are dropped (Python would read them as an
    /// indentation). Strings are copied as they are; outside them the three escapes are decoded, a `{.` and a letter starts a
    /// client hole, `{/` and `{>` are refused, and brackets nest so that the closing `}` is the one that closes the hole.
    fn code(
        &mut self,
        mut at: usize,
        opened_at: Option<usize>,
        depth: usize,
    ) -> Result<(Layer, usize), ScanError> {
        while let Some(character) = self.text[at..].chars().next()
            && character.is_whitespace()
        {
            at += character.len_utf8();
        }
        let mut pieces = Pieces::new();
        let mut run = at;
        let mut nesting = 0usize;
        loop {
            match self.bytes.get(at).copied() {
                None => {
                    if let Some(open) = opened_at {
                        return Err(self.error(ScanErrorKind::Unclosed, open));
                    }
                    pieces.text.push_str(&self.text[run..at]);
                    return Ok((pieces.finish(), at));
                }
                Some(b'\\') => match self.bytes.get(at + 1) {
                    Some(&escaped @ (b'\\' | b'{' | b'}')) => {
                        pieces.text.push_str(&self.text[run..at]);
                        pieces.text.push(char::from(escaped));
                        at += 2;
                        run = at;
                    }
                    _ => at += 1,
                },
                Some(b'"' | b'\'') => {
                    let (raw, format) = string_prefix(self.bytes, at);
                    match skip_string(self.bytes, at, raw, format) {
                        Some(end) => at = end,
                        None => match opened_at {
                            Some(open) => return Err(self.error(ScanErrorKind::Unclosed, open)),
                            // a string that never ends in a line of code is the interpreter's to report
                            None => at = self.bytes.len(),
                        },
                    }
                }
                Some(b'(' | b'[') => {
                    nesting += 1;
                    at += 1;
                }
                Some(b')' | b']') => {
                    nesting = nesting.saturating_sub(1);
                    at += 1;
                }
                Some(b'}') => {
                    if nesting == 0 && opened_at.is_some() {
                        pieces.text.push_str(&self.text[run..at]);
                        return Ok((pieces.finish(), at + 1));
                    }
                    nesting = nesting.saturating_sub(1);
                    at += 1;
                }
                Some(b'{') => match (self.bytes.get(at + 1), self.bytes.get(at + 2)) {
                    (Some(b'/'), _) => {
                        return Err(self.error(ScanErrorKind::ReservedInCode(Sigil::Server), at));
                    }
                    (Some(b'>'), _) => {
                        return Err(self.error(ScanErrorKind::ReservedInCode(Sigil::Code), at));
                    }
                    (Some(b'.'), Some(letter)) if letter.is_ascii_alphabetic() => {
                        pieces.text.push_str(&self.text[run..at]);
                        let hole = self.hole(at, depth + 1)?;
                        at = hole.end;
                        run = at;
                        pieces.hole(hole);
                    }
                    _ => {
                        nesting += 1;
                        at += 1;
                    }
                },
                Some(_) => at += 1,
            }
        }
    }
}

// What follows is the scanner of spike S2 (docs/history/spikes/scripting/monty_line_spikes, measured against Monty's own
// tokenizer on thousands of generated expressions), kept as it was: it knows strings, f-strings and brackets, and nothing else.

/// Whether the quote at `at` has a string prefix, and if so whether the string is raw and whether it is an f-string.
fn string_prefix(bytes: &[u8], at: usize) -> (bool, bool) {
    let mut start = at;
    while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
        start -= 1;
    }
    let run: String = bytes[start..at]
        .iter()
        .map(|byte| char::from(byte.to_ascii_lowercase()))
        .collect();
    match run.as_str() {
        "r" | "br" | "rb" | "ur" => (true, false),
        "f" | "t" => (false, true),
        "fr" | "rf" | "tr" | "rt" => (true, true),
        _ => (false, false),
    }
}

/// Code inside an f-string field, up to a `}` at depth 0 or a `:` or `!` (not `!=`) that starts a format spec or a conversion.
fn skip_field_code(bytes: &[u8], mut index: usize) -> Option<(usize, u8)> {
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
            b':' if depth == 0 => return Some((index, byte)),
            b'!' if depth == 0 && bytes.get(index + 1) != Some(&b'=') => {
                return Some((index, byte));
            }
            b'"' | b'\'' => {
                let (raw, format) = string_prefix(bytes, index);
                index = skip_string(bytes, index, raw, format)?;
            }
            _ => index += 1,
        }
    }
    None
}

/// A string literal whose opening quote is at `index`: the index after its closing quote.
fn skip_string(bytes: &[u8], mut index: usize, raw: bool, format: bool) -> Option<usize> {
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
                if format && matches!(next, Some(b'{' | b'}')) {
                    index += 1; // the backslash stands alone; the brace is a brace
                } else if format
                    && !raw
                    && next == Some(b'N')
                    && bytes.get(index + 2) == Some(&b'{')
                {
                    let close = bytes[index + 2..].iter().position(|byte| *byte == b'}')?;
                    index += 2 + close + 1; // \N{NAME}
                } else {
                    index += 2;
                }
            }
            b'{' if format => {
                if bytes.get(index + 1) == Some(&b'{') {
                    index += 2;
                } else {
                    index = skip_field(bytes, index + 1)?;
                }
            }
            b'}' if format && bytes.get(index + 1) == Some(&b'}') => index += 2,
            _ => index += 1,
        }
    }
    None
}

/// An f-string field, from the character after its `{`: the index after its closing `}`.
fn skip_field(bytes: &[u8], index: usize) -> Option<usize> {
    let (at, byte) = skip_field_code(bytes, index)?;
    match byte {
        b'}' => Some(at + 1),
        b'!' => {
            let mut next = at + 1;
            while next < bytes.len() && bytes[next] != b':' && bytes[next] != b'}' {
                next += 1;
            }
            match bytes.get(next)? {
                b'}' => Some(next + 1),
                _ => skip_spec(bytes, next + 1),
            }
        }
        _ => skip_spec(bytes, at + 1),
    }
}

/// A format spec: text, with nested fields, up to the `}` that closes the field.
fn skip_spec(bytes: &[u8], mut index: usize) -> Option<usize> {
    while index < bytes.len() {
        match bytes[index] {
            b'{' => index = skip_field(bytes, index + 1)?,
            b'}' => return Some(index + 1),
            _ => index += 1,
        }
    }
    None
}
