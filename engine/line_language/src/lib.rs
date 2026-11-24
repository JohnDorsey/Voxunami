//! The language of a typed line (`docs/specs/scripting-spec.md`, section 4).
//!
//! A line starts with a sigil that says what language it is in: `/` a command for the server, `.` a command for the window, `>`
//! code. Inside it a *hole* is `{` and a sigil, text in the language that sigil names, and `}`; holes nest, and what a hole gives
//! takes its place. Three escapes (`\\`, `\{`, `\}`) make a backslash or a brace plain text, and nothing a hole gives is read as
//! a hole again.
//!
//! * [`scan`] finds the holes and decodes the escapes.
//! * [`evaluate`] runs them innermost first and left to right, and hands each command to a [`Runner`] as a [`Call`].
//! * [`resolve_client_holes`] is what the window does to a line before it sends it: it runs the `{.}` holes, which only the
//!   window can run, and puts their results in so that the server reads them as plain text (or, in code, as literals).
//!
//! The crate knows no command and no interpreter. What a command is lives where commands are run (`plugin_host` on the server,
//! `game_shell` in the window); what code is, in the interpreter.

mod evaluate;
mod resolve;
mod scan;
mod value;

pub use evaluate::{
    Call, Failure, HoleName, Item, MAXIMUM_LINE_LENGTH, MAXIMUM_RESULT_LENGTH, Runner, evaluate,
};
pub use resolve::resolve_client_holes;
pub use scan::{
    Hole, Layer, Line, MAXIMUM_DEPTH, MAXIMUM_HOLES, ScanError, ScanErrorKind, Segment, Sigil,
    escape, scan,
};
pub use value::CommandValue;
