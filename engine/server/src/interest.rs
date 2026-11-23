//! What a client has asked to watch of a level (`protocol::ClientMessage::SetInterest`): the region round its body and the free regions of viewpoints that have none.

use voxel_space::{BrickBox, BrickCoordinate, LevelIndex};

/// What a client watches: one level, the region round its body (which has the entities, the ships and the messages of the game that reach a place, as well as the
/// bricks) and its free regions (which have the edited bricks of the level's terrain and nothing else).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Watch {
    pub(crate) level: LevelIndex,
    pub(crate) region: BrickBox,
    pub(crate) free: Vec<BrickBox>,
}

impl Watch {
    /// The level and the region round the body: what everything but the terrain is told of.
    pub(crate) fn body(&self) -> (LevelIndex, BrickBox) {
        (self.level, self.region)
    }

    /// Every region, the body's first.
    pub(crate) fn regions(&self) -> Vec<BrickBox> {
        std::iter::once(self.region)
            .chain(self.free.iter().copied())
            .collect()
    }

    /// Whether the brick of `level` is in any of the regions.
    pub(crate) fn covers(&self, level: LevelIndex, brick: BrickCoordinate) -> bool {
        self.level == level
            && (self.region.contains(brick) || self.free.iter().any(|free| free.contains(brick)))
    }
}

/// The bricks that `now` has and `before` has not, as boxes that do not overlap: found without visiting the bricks that both share. The boxes of `now` come
/// first, each one's pieces after the bricks of the boxes before it are taken out of it, so that a brick in two of them is in one piece.
pub(crate) fn newly_covered(now: &[BrickBox], before: &[BrickBox]) -> Vec<BrickBox> {
    let mut pieces = Vec::new();
    for (index, &region) in now.iter().enumerate() {
        let mut rest = vec![region];
        for &other in before.iter().chain(&now[..index]) {
            rest = rest
                .into_iter()
                .flat_map(|piece| piece.without(other))
                .collect();
        }
        pieces.extend(rest);
    }
    pieces
}

#[cfg(test)]
mod tests;
