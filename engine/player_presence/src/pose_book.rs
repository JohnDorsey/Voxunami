use std::collections::BTreeMap;

use namespaced_id::NamespacedId;
use protocol::ClientId;
use server::{Audience, CommandLine, ServerWorld};
use voxel_space::LevelIndex;

use crate::messages::{
    PlayerPose, PlayerState, decode_pose, encode_level_change, encode_players, level_kind,
    players_kind, pose_kind,
};

/// The server's record of where every player is, and in which level. A game's logic owns one,
/// hands it the messages, commands and departures, and calls [`PoseBook::tell_everyone`] once per
/// tick.
#[derive(Debug, Default)]
pub struct PoseBook {
    poses: BTreeMap<ClientId, PlayerPose>,
    /// Only players who have moved are listed; everyone else is in the main level.
    levels: BTreeMap<ClientId, LevelIndex>,
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

    /// Puts a player at a pose, for a game that moves players itself instead of taking their word
    /// for where they are (see [`PoseBook::handle_message`]).
    pub fn set_pose(&mut self, client: ClientId, pose: PlayerPose) {
        if self.poses.insert(client, pose) != Some(pose) {
            self.changed = true;
        }
    }

    /// The level the player is in: the main level until they move.
    pub fn level_of(&self, client: ClientId) -> LevelIndex {
        self.levels
            .get(&client)
            .copied()
            .unwrap_or(LevelIndex::MAIN)
    }

    /// Puts a player in a level and tells them so. A game calls this to move players itself (a
    /// portal, a teleport); the `/level go` command is built on it.
    pub fn move_to_level(&mut self, client: ClientId, level: LevelIndex, world: &mut ServerWorld) {
        let Some(id) = world.data().level_id(level) else {
            return; // not a level of this world: nowhere to go
        };
        let id = id.clone();
        if self.level_of(client) != level {
            self.changed = true;
        }
        self.levels.insert(client, level);
        world.send(
            Audience::Client(client),
            level_kind(),
            encode_level_change(&id),
        );
    }

    /// Every player who has reported a pose, in the order of their ids, with the pose and the level
    /// they are in.
    pub fn players(&self) -> impl Iterator<Item = (ClientId, &PlayerPose, LevelIndex)> + '_ {
        self.poses
            .iter()
            .map(|(&client, pose)| (client, pose, self.level_of(client)))
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

    /// Takes in the `/level` command, which tells a player which level they are in and moves them:
    /// `/level` (where am I), `/level list`, `/level go NAME`, where NAME is a level's identifier
    /// or just the part after the colon. Replies to the player. Returns whether the command was
    /// one, so a game can pass on what is not.
    pub fn handle_command(
        &mut self,
        client: ClientId,
        command: &str,
        world: &mut ServerWorld,
    ) -> bool {
        let Some(line) = CommandLine::parse(command) else {
            return false;
        };
        if line.name != "level" {
            return false;
        }
        let here = self.level_of(client);
        let name_of = |world: &ServerWorld, level: LevelIndex| {
            world
                .data()
                .level_id(level)
                .map_or_else(|| level.to_string(), ToString::to_string)
        };
        match line.arguments[..] {
            [] => {
                let text = format!("you are in {}", name_of(world, here));
                world.reply(client, &text);
            }
            ["list"] => {
                let levels: Vec<String> = world
                    .data()
                    .levels()
                    .map(|(level, id, _)| {
                        if level == here {
                            format!("{id} (you are here)")
                        } else {
                            id.to_string()
                        }
                    })
                    .collect();
                world.reply(client, &format!("levels: {}", levels.join(", ")));
            }
            ["go", wanted] => match find_level(world, wanted) {
                Ok(level) if level == here => {
                    let text = format!("you are already in {}", name_of(world, level));
                    world.reply(client, &text);
                }
                Ok(level) => {
                    self.move_to_level(client, level, world);
                    let text = format!("you are now in {}", name_of(world, level));
                    world.reply(client, &text);
                }
                Err(problem) => world.reply(client, &problem),
            },
            _ => world.reply(client, "usage: /level, /level list, /level go NAME"),
        }
        true
    }

    pub fn client_left(&mut self, client: ClientId) {
        self.levels.remove(&client);
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
            .map(|(&client, &pose)| {
                (
                    client,
                    PlayerState {
                        pose,
                        level: self.level_of(client),
                    },
                )
            })
            .collect();
        world.send(Audience::Everyone, players_kind(), encode_players(&players));
        self.changed = false;
    }
}

/// The level a player asked for by name: an identifier, or the part after the colon if only one
/// level has it.
fn find_level(world: &ServerWorld, wanted: &str) -> Result<LevelIndex, String> {
    if let Ok(id) = wanted.parse::<NamespacedId>()
        && let Some(level) = world.level_index(&id)
    {
        return Ok(level);
    }
    let matching: Vec<LevelIndex> = world
        .data()
        .levels()
        .filter(|(_, id, _)| id.name() == wanted)
        .map(|(level, _, _)| level)
        .collect();
    match matching[..] {
        [level] => Ok(level),
        [] => Err(format!(
            "there is no level {wanted}; /level list shows them"
        )),
        _ => Err(format!(
            "{wanted} could mean several levels; use the whole name"
        )),
    }
}
