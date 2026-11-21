//! Which ships a client is told of when they are too far for their bricks: how big they have to look, and how many.

/// How far ships are told of (see `GridContent::Far`). A server without these tells of a ship only when its bricks are in the region a client watches.
///
/// A client is told of a ship that is in its level, is not told of with its bricks, and looks at least [`minimum_apparent_size`](Self::minimum_apparent_size) long.
/// What a ship looks like of that size is the client's to draw from the nodes it is sent (they are another plugin's: the server only tells of the ship).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FarShipSettings {
    /// The angle, in radians, the longest side of the box of a ship has to span from where the client is for the ship to be told of as far. A ship that is known
    /// is kept until it spans [`LEAVING_FRACTION`] of this, so that one at the edge is not told of and taken back every tick. A pixel at 1,080 lines and a 70 degree
    /// view is 0.0013, so 0.005 is about four pixels.
    pub minimum_apparent_size: f64,
    /// The most ships one client is told of as far. The nearest are kept.
    pub maximum_ships: usize,
}

/// The part of [`FarShipSettings::minimum_apparent_size`] a ship that is already far may fall to before it is let go.
pub const LEAVING_FRACTION: f64 = 0.9;

impl FarShipSettings {
    /// Whether a ship whose box has a longest side of `size` voxels, `distance_squared` (voxels squared) from the client, is to be told of as far. `known` is
    /// whether it is already.
    ///
    /// Written with squares and a product so that a client at the ship (distance 0) sees it, and so that no function the platforms may differ on is used.
    pub fn is_seen(&self, size: f64, distance_squared: f64, known: bool) -> bool {
        let minimum = if known {
            self.minimum_apparent_size * LEAVING_FRACTION
        } else {
            self.minimum_apparent_size
        };
        size * size >= minimum * minimum * distance_squared
    }
}
