//! PROTOTYPE (branch graphics-ao-from-volume-prototype): where the corner shade of a whole-cube quad comes from, chosen by the environment variable `VOX_SHADE_MODE`.

use std::sync::OnceLock;

/// Where the shade comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShadeMode {
    /// The mesher works out the four corner levels of each quad and merges only quads shaded alike (the way phases 3 and 4 built it; the default).
    Vertex,
    /// The mesher merges as if nothing were shaded and flags the quads; the fragment shader works the levels out from the voxels that block the sun in the renderer's window.
    Volume,
    /// No shade at all: for measuring what each of the others costs.
    Nothing,
}

/// The mode this process runs in (`VOX_SHADE_MODE` = `volume` or `none`; anything else is `Vertex`).
pub fn shade_mode() -> ShadeMode {
    static MODE: OnceLock<ShadeMode> = OnceLock::new();
    *MODE.get_or_init(
        || match std::env::var("VOX_SHADE_MODE").as_deref().unwrap_or("") {
            "volume" => ShadeMode::Volume,
            "none" => ShadeMode::Nothing,
            _ => ShadeMode::Vertex,
        },
    )
}
