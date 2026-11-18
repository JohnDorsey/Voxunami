//! Painting on the world's pictures with a tool that is another program (`docs/world-painting-options.md`, `tools/virtual_atlas/BRIDGE.md` for the protocol).
//!
//! The window is the view: in paint mode the cursor is free, and each frame the game says to the tool which picture the pointer is over and at which pixel
//! ([`Painter::step`], which asks [`Session::target_surface_toward`](crate::session::Session::target_surface_toward) what is under the pointer). The tool draws with
//! its own brushes, colours and history, and sends the picture back as it changes; the game writes it into the renderer (`VoxelRenderer::set_picture`), on this client alone.
//! Nothing here is known to the server, so a recording is unaffected.
//!
//! The protocol is the tool's, not this game's: a picture is told to it as an id and the names of the files it may be, and the tool answers in ids. Nothing in the
//! protocol is about one game, and nothing in this module is about one game either; the names of the pictures come from the package (`catalogue`).

mod base64;
mod catalogue;
mod connection;
mod painter;
mod path;
mod protocol;

pub use catalogue::{Catalogue, catalogue_of};
pub use connection::ToolConnection;
pub use painter::{Buttons, Painter};
pub use path::{PaintPath, PathPoint};
pub use protocol::{
    CatalogueTile, DEFAULT_PORT, FromTool, LineBuffer, PBR_PROFILE, PROTOCOL, Pointer, ToTool,
    decode_base64, encode_base64,
};

#[cfg(test)]
mod tests;
