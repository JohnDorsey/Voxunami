//! What the window does with a line before it sends it to the server: runs the client holes in it and puts their results in
//! (scripting-spec.md 4.2, 4.3 and 4.5).
//!
//! The window is the only place a client command can run, and the server must be sent a line it can read without help. So every
//! `{.}` hole that is not inside another one is run here and replaced by its result: in command text as text, with the escapes the
//! server reads back as the same text (a result is never read as a hole), and in code as a Python literal (a result is never read
//! as code). The rest of the line is sent as it was typed, byte for byte.

use crate::evaluate::{Evaluation, Failure, MAXIMUM_LINE_LENGTH, Runner, Stop};
use crate::scan::{Layer, Line, Segment, Sigil, escape};

#[derive(Clone, Copy)]
enum Language {
    Command,
    Code,
}

/// Runs the client holes of a line and gives the line to send, without its sigil. `text` is the text the line was scanned from.
/// `runner` runs the window's commands and refuses the rest.
pub fn resolve_client_holes(
    line: &Line,
    text: &str,
    runner: &mut dyn Runner,
) -> Result<String, Failure> {
    let mut evaluation = Evaluation::new(runner);
    let language = match line.sigil {
        Sigil::Code => Language::Code,
        Sigil::Server | Sigil::Client => Language::Command,
    };
    let mut replacements: Vec<(usize, usize, String)> = Vec::new();
    if let Err(stop) = collect(&mut evaluation, &line.layer, language, &mut replacements) {
        return Err(evaluation.settle(stop));
    }
    let mut resolved = String::with_capacity(text.len());
    let mut copied = 0;
    for (start, end, replacement) in replacements {
        resolved.push_str(&text[copied..start]);
        resolved.push_str(&replacement);
        copied = end;
    }
    resolved.push_str(&text[copied..]);
    // the sigil that starts the line is one byte more than the text
    let length = resolved.len() + 1;
    if length > MAXIMUM_LINE_LENGTH {
        return Err(evaluation.failure(
            None,
            format!(
                "the line would be longer than {MAXIMUM_LINE_LENGTH} bytes once the holes are filled in ({length} bytes)"
            ),
        ));
    }
    Ok(resolved)
}

/// Runs the outermost client holes of a layer, in order, and writes down what replaces each. A server hole or a code hole is not
/// run, but may hold client holes, which are.
fn collect(
    evaluation: &mut Evaluation<'_>,
    layer: &Layer,
    language: Language,
    replacements: &mut Vec<(usize, usize, String)>,
) -> Result<(), Stop> {
    for segment in &layer.segments {
        let Segment::Hole(hole) = segment else {
            continue;
        };
        match hole.sigil {
            Sigil::Client => {
                let value = evaluation.hole(hole)?;
                let replacement = match language {
                    Language::Command => {
                        let text: String = value
                            .to_text()
                            .chars()
                            .filter(|character| !character.is_control())
                            .collect();
                        escape(&text)
                    }
                    Language::Code => value.to_python(),
                };
                replacements.push((hole.start, hole.end, replacement));
            }
            Sigil::Server => collect(evaluation, &hole.body, Language::Command, replacements)?,
            Sigil::Code => collect(evaluation, &hole.body, Language::Code, replacements)?,
        }
    }
    Ok(())
}
