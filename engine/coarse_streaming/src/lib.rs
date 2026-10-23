//! Coarse levels of detail in the stream (`docs/lod-spec.md`, 3.5 and 38i): the messages and, in the modules that follow, the server and client sides.

mod messages;
mod server;

pub use messages::{
    CoarseNodeUpdate, CoarseNodes, MAXIMUM_CELL_PIXELS, MAXIMUM_UPDATES, SetDetail, ZoomRequest,
};
pub use server::{Brightness, CoarseConfig, CoarseHandle, CoarseServerPlugin};
