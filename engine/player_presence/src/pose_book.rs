use std::collections::BTreeMap;

use namespaced_id::NamespacedId;
use protocol::ClientId;
use server::{Audience, ServerWorld};

use crate::messages::{PlayerPose, decode_pose, encode_players, players_kind, pose_kind};

/// The server's record of where every player is. A game's logic owns one, hands it the messages
/// and departures, and calls [`PoseBook::tell_everyone`] once per tick.
#[derive(Debug, Default)]
pub struct PoseBook {
    poses: BTreeMap<ClientId, PlayerPose>,
    changed: bool,
}

impl PoseBook {
    pub fn new() -> Self {
        Self::default()
    }

    /// The last pose the player reported, if they have reported one.
    pub fn pose_of(&self, client: ClientId) -> Option<&PlayerPose> {
        self.poses.get(&client)
    }

    pub fn len(&self) -> usize {
        self.poses.len()
    }

    pub fn is_empty(&self) -> bool {
        self.poses.is_empty()
    }

    /// Takes in a pose message. Returns whether the message was one (a malformed pose counts: it is
    /// dropped), so a game can pass on what is not.
    pub fn handle_message(
        &mut self,
        client: ClientId,
        kind: &NamespacedId,
        payload: &[u8],
    ) -> bool {
        if *kind != pose_kind() {
            return false;
        }
        if let Ok(pose) = decode_pose(payload)
            && self.poses.insert(client, pose) != Some(pose)
        {
            self.changed = true;
        }
        true
    }

    pub fn client_left(&mut self, client: ClientId) {
        if self.poses.remove(&client).is_some() {
            self.changed = true;
        }
    }

    /// Tells everyone where everyone is, if anything changed since the last time.
    pub fn tell_everyone(&mut self, world: &mut ServerWorld) {
        if !self.changed {
            return;
        }
        let players: Vec<_> = self
            .poses
            .iter()
            .map(|(&client, &pose)| (client, pose))
            .collect();
        world.send(Audience::Everyone, players_kind(), encode_players(&players));
        self.changed = false;
    }
}
