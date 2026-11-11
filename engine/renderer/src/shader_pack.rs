//! Shader packs (`docs/specs/shader-pack-spec.md`): a player's own `pack_shade`, the function that makes the colour of a face of what the engine knows of it.
//!
//! The engine works out what is lit (the sun's shadow from the map and the exact walk of the ray, the sky over a place, the horizon of the planet, the lamps) and gives the
//! pack a `Surface` with the answers, and the pack makes a colour of it; the fog and the sky are still the engine's. A pack is one WGSL file of functions
//! and constants whose entry is
//!
//! ```wgsl
//! fn pack_shade(s: Surface) -> vec3<f32>
//! ```
//!
//! and which may read the frame's uniform (`frame.sun_direction`, `frame.sun_light`, ...). It cannot ask the shadow map, the voxels, the palette or a picture anything: the
//! words for them are not allowed, nor is a binding of its own, nor a loop (a loop that does not end hangs the graphics card, and the engine's own queries are bounded; a
//! pack writes its steps out). So a pack cannot bring back a gap the engine closed by choosing a bias of its own, which packs of other games do each for themselves.
//!
//! [`ShaderPack::new`] checks all this and that the text, put after the engine's, is a shader the shader language accepts, and says what is wrong with the lines of the pack's own
//! file. What it cannot know is what the graphics card says of it: that is [`crate::VoxelRenderer::set_shader_pack`]'s.

use std::fmt;

use crate::voxel_renderer::{SunWalk, voxel_source};

/// Which version of the interface a pack is written for: the fields of `Surface` and what `pack_shade` is given and returns. A pack's `pack.json` names it, and a version of the
/// engine that has changed the interface refuses the pack.
pub const SHADER_PACK_INTERFACE: u32 = 1;

/// The default pack's `pack_shade`: how the engine draws with no pack.
pub(crate) const DEFAULT_LIGHTING: &str = include_str!("lighting_default.wgsl");

/// The text of the default pack's lighting file: how the engine draws with no pack, and a place to start a pack from.
pub fn default_lighting() -> &'static str {
    DEFAULT_LIGHTING
}

/// A place in a pack's file that is wrong, and what is wrong with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShaderPackProblem {
    /// From 1, in the pack's text; `None` for a problem of the whole.
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub message: String,
}

impl fmt::Display for ShaderPackProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.line, self.column) {
            (Some(line), Some(column)) => {
                write!(formatter, "line {line}, column {column}: {}", self.message)
            }
            (Some(line), None) => write!(formatter, "line {line}: {}", self.message),
            _ => write!(formatter, "{}", self.message),
        }
    }
}

/// Everything wrong with a pack that was found: the lexical rules all at once, and the first thing the shader language says of a text that passes them.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{}", .problems.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n"))]
pub struct ShaderPackError {
    pub problems: Vec<ShaderPackProblem>,
}

impl ShaderPackError {
    fn one(message: impl Into<String>) -> Self {
        Self {
            problems: vec![ShaderPackProblem {
                line: None,
                column: None,
                message: message.into(),
            }],
        }
    }

    /// The graphics card refused a shader of the pack that the shader language accepted.
    pub(crate) fn refused_by_the_graphics_card(message: &str) -> Self {
        Self::one(format!(
            "the graphics card refuses the pack's shader (the shader language's checks passed): {message}"
        ))
    }
}

/// A pack: its id and name, and the text of its `pack_shade`, checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShaderPack {
    id: String,
    name: String,
    lighting: String,
}

impl ShaderPack {
    /// A pack with this id and name and the text of its lighting file; an error says everything that is wrong with the text.
    pub fn new(id: &str, name: &str, lighting: &str) -> Result<Self, ShaderPackError> {
        check_lighting(lighting)?;
        Ok(Self {
            id: id.to_owned(),
            name: name.to_owned(),
            lighting: lighting.to_owned(),
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// The text of the pack's `pack_shade` and what goes with it.
    pub fn lighting(&self) -> &str {
        &self.lighting
    }
}

/// The names of what the engine keeps to itself: the bindings of the voxel shader, which would let a pack ask the map, the voxels and the pictures what it likes.
const KEPT_BY_THE_ENGINE: [&str; 15] = [
    "palette",
    "shadow_maps",
    "shadow_sampler",
    "placement",
    "contact_windows",
    "tables",
    "pool",
    "partial_pool",
    "shapes",
    "pictures_0",
    "pictures_1",
    "pictures_2",
    "pictures_3",
    "picture_sampler",
    "entity_boxes_block",
];

/// The text with its comments blanked (a comment is spaces: the lines and columns of the rest stay where they were).
fn without_comments(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut blank = bytes.to_vec();
    let mut at = 0;
    let mut depth = 0usize;
    while at < bytes.len() {
        if depth == 0 && bytes[at..].starts_with(b"//") {
            while at < bytes.len() && bytes[at] != b'\n' {
                blank[at] = b' ';
                at += 1;
            }
        } else if bytes[at..].starts_with(b"/*") {
            depth += 1;
            blank[at] = b' ';
            blank[at + 1] = b' ';
            at += 2;
        } else if depth > 0 && bytes[at..].starts_with(b"*/") {
            depth -= 1;
            blank[at] = b' ';
            blank[at + 1] = b' ';
            at += 2;
        } else {
            if depth > 0 && bytes[at] != b'\n' {
                blank[at] = b' ';
            }
            at += 1;
        }
    }
    String::from_utf8(blank).expect("blanking ASCII comment bytes leaves valid text")
}

/// The words of a line of text, with where each begins (1-based column), and the `@` and `<` characters as words of their own.
fn words(line: &str) -> Vec<(u32, &str)> {
    let mut found = Vec::new();
    let mut start = None;
    for (index, character) in line.char_indices() {
        let in_word = character.is_alphanumeric() || character == '_';
        match (start, in_word) {
            (None, true) => start = Some(index),
            (Some(begin), false) => {
                found.push((begin as u32 + 1, &line[begin..index]));
                start = None;
            }
            _ => {}
        }
        if !in_word && (character == '@' || character == '<') {
            found.push((index as u32 + 1, &line[index..index + 1]));
        }
    }
    if let Some(begin) = start {
        found.push((begin as u32 + 1, &line[begin..]));
    }
    found
}

/// The rules about words: no loop, no binding, no variable outside a function, none of the engine's own names, no texture. Every place that breaks one.
fn lexical_problems(text: &str) -> Vec<ShaderPackProblem> {
    let blank = without_comments(text);
    let mut problems = Vec::new();
    let mut defines_the_entry = false;
    let mut problem = |line: usize, column: u32, message: String| {
        problems.push(ShaderPackProblem {
            line: Some(line as u32 + 1),
            column: Some(column),
            message,
        });
    };
    for (line_number, line) in blank.lines().enumerate() {
        let found = words(line);
        for (index, &(column, word)) in found.iter().enumerate() {
            let next = found.get(index + 1).map(|&(_, next)| next);
            match word {
                "loop" | "while" | "for" => problem(
                    line_number,
                    column,
                    format!("`{word}` is not allowed in a pack: a loop that does not end hangs the graphics card, so a pack writes its steps out"),
                ),
                "@" if matches!(next, Some("group" | "binding")) => problem(
                    line_number,
                    column,
                    "a pack has no bindings of its own: it is given a `Surface` and the frame's uniform".to_owned(),
                ),
                "var" if next == Some("<") => problem(
                    line_number,
                    column,
                    "a pack has no variables outside its functions: use `const`, or a `var` inside a function".to_owned(),
                ),
                "fn" if next == Some("pack_shade") => defines_the_entry = true,
                _ if KEPT_BY_THE_ENGINE.contains(&word) => problem(
                    line_number,
                    column,
                    format!("`{word}` is the engine's: a pack is given what the engine works out (the `Surface`), and does not ask the shadow map, the voxels or the pictures itself (rename a name of your own that is the same)"),
                ),
                _ if word.starts_with("texture") => problem(
                    line_number,
                    column,
                    format!("`{word}`: a pack reads no textures (the engine gives it the colour of the face)"),
                ),
                _ => {}
            }
        }
    }
    if !defines_the_entry && problems.is_empty() {
        problems.push(ShaderPackProblem {
            line: None,
            column: None,
            message: "the pack does not define `fn pack_shade(s: Surface) -> vec3<f32>`".to_owned(),
        });
    }
    problems
}

/// What the shader language says of the engine's shader of one kind with this text of the pack after it, as problems with the lines of the pack's text.
fn language_problem(with_pictures: bool, walk: SunWalk, text: &str) -> Option<ShaderPackProblem> {
    let before = voxel_source(with_pictures, walk, "");
    let source = format!("{before}{text}");
    let first_line_of_the_pack = before.matches('\n').count() as u32 + 1;
    let module = match naga::front::wgsl::parse_str(&source) {
        Ok(module) => module,
        Err(error) => {
            let location = error.location(&source);
            return Some(problem_at(
                location,
                first_line_of_the_pack,
                error.message(),
                error.labels().next().map(|(_, label)| label),
            ));
        }
    };
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    match validator.validate(&module) {
        Ok(_) => None,
        Err(error) => {
            let location = error.location(&source);
            let message = error.as_inner().to_string();
            let label = error.spans().next().map(|(_, label)| label.as_str());
            Some(problem_at(
                location,
                first_line_of_the_pack,
                &message,
                label,
            ))
        }
    }
}

fn problem_at(
    location: Option<naga::SourceLocation>,
    first_line_of_the_pack: u32,
    message: &str,
    label: Option<&str>,
) -> ShaderPackProblem {
    let message = match label {
        Some(label) if !label.is_empty() && label != message => format!("{message} ({label})"),
        _ => message.to_owned(),
    };
    match location {
        Some(location) if location.line_number >= first_line_of_the_pack => ShaderPackProblem {
            line: Some(location.line_number - first_line_of_the_pack + 1),
            column: Some(location.line_position),
            message,
        },
        _ => ShaderPackProblem {
            line: None,
            column: None,
            message: format!(
                "the pack does not fit the engine's shader: {message} (is `pack_shade` `fn pack_shade(s: Surface) -> vec3<f32>`, and does the pack define a name the engine has?)"
            ),
        },
    }
}

/// Checks the text of a pack's lighting: the words it may not use, and that the engine's shader of the plainest kind and of the heaviest, with the text after it, is accepted by
/// the shader language. (The pack's text sees only the `Surface` and the frame's uniform, which every kind has, so the kinds between are as these.)
pub(crate) fn check_lighting(text: &str) -> Result<(), ShaderPackError> {
    let problems = lexical_problems(text);
    if !problems.is_empty() {
        return Err(ShaderPackError { problems });
    }
    let heaviest = SunWalk::Voxels {
        shapes: true,
        other_windows: true,
        entities: true,
    };
    for (with_pictures, walk) in [(false, SunWalk::None), (true, heaviest)] {
        if let Some(problem) = language_problem(with_pictures, walk, text) {
            return Err(ShaderPackError {
                problems: vec![problem],
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
