//! What the Voxunami Cave server does is in its plugins (see [`crate::plugins`]); this holds the numbers
//! they share with the rest of the game.

/// How far from a player, in voxels, a block can be changed.
pub const REACH: f64 = 10.0;

/// How many voxels the light rule looks at in a tick at most. A lamp's light is some thousands of
/// voxels a tick as it spreads; more than this at once (many lamps) takes a few ticks longer.
pub const LIGHT_VOXELS_PER_TICK: usize = 20_000;

/// How far from a player, in voxels, fires burn: farther away they are frozen.
pub const FIRE_RANGE: f64 = 64.0;
