use std::sync::Arc;

use namespaced_id::NamespacedId;
use protocol::{PROTOCOL_VERSION, ServerMessage, Tick, Welcome};
use region_octree::BrickCube;
use voxel_grid::Brick;
use voxel_space::{BrickBox, BrickCoordinate, LevelIndex};
use world_storage::{LevelData, LoadContext, LoadReport, ParsedWorld, SourceTranslator, WorldData};

use crate::error::ClientError;

/// What applying one message changed that a program showing the world needs to react to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientEvent {
    /// The client now watches exactly this region of this level. Edited bricks outside it were
    /// forgotten, and the terrain generators of the newly watched part are known.
    WatchedRegionChanged { level: LevelIndex, region: BrickBox },
    /// A brick's edited contents arrived or changed.
    BrickChanged {
        level: LevelIndex,
        brick: BrickCoordinate,
    },
    /// A message for the game or one of its plugins.
    GameMessage {
        kind: NamespacedId,
        payload: Vec<u8>,
    },
    /// The server's answer to a command this client sent.
    CommandReply { text: String },
}

/// Facts about the connection, from the server's welcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerInfo {
    pub game: NamespacedId,
    pub client: protocol::ClientId,
    pub ticks_per_second: u32,
}

/// What identifies one level of a client's view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LevelSummary {
    /// `Grid::content_digest` of the edited bricks the client holds in this level.
    pub edited_bricks: u64,
    /// The terrain generator the client believes made each region, by identifier.
    pub generator_assignments: Vec<(BrickCube, Option<NamespacedId>)>,
}

/// Everything that identifies a client's view, for comparing two views (a live client and a replay
/// of it, say) and for reporting where they differ.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewSummary {
    pub tick: Tick,
    pub watched: Option<(LevelIndex, BrickBox)>,
    /// Every level of the world, in order.
    pub levels: Vec<LevelSummary>,
}

/// A client's picture of the world: terrain it generates itself where the server says which
/// generator to use, plus the edited bricks the server has sent for the region it watches.
///
/// It is built from server messages only, applied in order by [`ClientWorld::apply`]; the same
/// messages always give the same view, which is what makes a recording of them a replay.
#[derive(Debug)]
pub struct ClientWorld {
    info: ServerInfo,
    world: WorldData,
    translator: SourceTranslator,
    /// Edit digests can only be checked when the server and this program number content alike.
    verifies_edit_digests: bool,
    tick: Tick,
    watched: Option<(LevelIndex, BrickBox)>,
}

impl ClientWorld {
    /// Builds the world a server described. `context` supplies this program's own content and
    /// terrain generators; whatever the server has that this program lacks becomes placeholders, and
    /// the report says what.
    pub fn from_welcome(
        welcome: Welcome,
        context: LoadContext<'_>,
    ) -> Result<(Self, LoadReport), ClientError> {
        if welcome.protocol_version != PROTOCOL_VERSION {
            return Err(ClientError::UnsupportedProtocolVersion(
                welcome.protocol_version,
            ));
        }
        let (world, report, translator) =
            ParsedWorld::parse(&welcome.world_description)?.load_with_translator(context)?;
        let verifies_edit_digests = translator.is_identity();
        Ok((
            Self {
                info: ServerInfo {
                    game: welcome.game,
                    client: welcome.client,
                    ticks_per_second: welcome.ticks_per_second,
                },
                world,
                translator,
                verifies_edit_digests,
                tick: welcome.tick,
                watched: None,
            },
            report,
        ))
    }

    pub fn server_info(&self) -> &ServerInfo {
        &self.info
    }

    /// The terrain and edits of every level as this client knows them. Only the watched region of
    /// the watched level is meaningful.
    pub fn world(&self) -> &WorldData {
        &self.world
    }

    /// The last tick this view is complete for.
    pub fn tick(&self) -> Tick {
        self.tick
    }

    /// The level and region the server has told this client it watches.
    pub fn watched(&self) -> Option<(LevelIndex, BrickBox)> {
        self.watched
    }

    /// The data of the level being watched.
    pub fn watched_level(&self) -> Option<(LevelIndex, &LevelData)> {
        let (level, _) = self.watched?;
        Some((level, self.world.level(level)?))
    }

    /// Whether the numbering of content and channels matches the server's, which lets the client
    /// check each edit against the server's digest of the result.
    pub fn verifies_edit_digests(&self) -> bool {
        self.verifies_edit_digests
    }

    pub fn summary(&self) -> ViewSummary {
        ViewSummary {
            tick: self.tick,
            watched: self.watched,
            levels: self
                .world
                .levels()
                .map(|(_, _, level)| {
                    let generators = level.terrain.generators();
                    LevelSummary {
                        edited_bricks: level.overrides.content_digest(),
                        generator_assignments: level
                            .terrain
                            .assignments()
                            .leaves()
                            .into_iter()
                            .map(|(cube, generator)| {
                                (
                                    cube,
                                    generator.map(|id| {
                                        generators
                                            .entry(id)
                                            .expect("assigned generators come from this registry")
                                            .identifier()
                                            .clone()
                                    }),
                                )
                            })
                            .collect(),
                    }
                })
                .collect(),
        }
    }

    /// Applies one message from the server. `TickCompleted` advances [`ClientWorld::tick`]; the view
    /// is consistent only between ticks, so callers apply a tick's messages together.
    pub fn apply(&mut self, message: ServerMessage) -> Result<Option<ClientEvent>, ClientError> {
        match message {
            ServerMessage::Welcome(_) => Err(ClientError::UnexpectedMessage("a second welcome")),
            ServerMessage::Disconnected { reason } => {
                Err(ClientError::DisconnectedByServer(reason))
            }
            ServerMessage::TickCompleted { tick } => {
                let expected = self.tick.next();
                if tick != expected {
                    return Err(ClientError::TickOutOfOrder {
                        expected,
                        found: tick,
                    });
                }
                self.tick = tick;
                Ok(None)
            }
            ServerMessage::GameMessage { kind, payload } => {
                Ok(Some(ClientEvent::GameMessage { kind, payload }))
            }
            ServerMessage::CommandReply { text } => Ok(Some(ClientEvent::CommandReply { text })),
            ServerMessage::InterestChanged {
                level,
                region,
                assignments,
            } => {
                if self.world.level(level).is_none() {
                    return Err(ClientError::UnknownLevel(level));
                }
                // Edited bricks outside the new region are forgotten, and so is everything held
                // of every other level.
                for index in 0..self.world.level_count() {
                    let index = LevelIndex::new(index as u32);
                    let data = self.world.level_mut(index).expect("the level exists");
                    let forgotten: Vec<BrickCoordinate> = data
                        .overrides
                        .bricks()
                        .map(|(brick, _)| brick)
                        .filter(|brick| index != level || !region.contains(*brick))
                        .collect();
                    for brick in forgotten {
                        data.overrides.remove_brick(brick);
                    }
                }
                for assignment in assignments {
                    let generator = assignment
                        .generator
                        .map(|source_id| self.translator.translate_generator(source_id))
                        .transpose()?;
                    self.world
                        .level_mut(level)
                        .expect("the level was checked")
                        .terrain
                        .set_assignment(assignment.cube, generator);
                }
                self.watched = Some((level, region));
                Ok(Some(ClientEvent::WatchedRegionChanged { level, region }))
            }
            ServerMessage::BrickContents {
                level,
                brick,
                contents,
            } => {
                if !self
                    .watched
                    .is_some_and(|(watched, region)| watched == level && region.contains(brick))
                {
                    return Err(ClientError::BrickOutsideWatchedRegion(level, brick));
                }
                let contents =
                    Arc::try_unwrap(contents).unwrap_or_else(|shared| Brick::clone(&shared));
                let translated = self.translator.translate_brick(contents)?;
                self.world
                    .level_mut(level)
                    .expect("the watched level exists")
                    .overrides
                    .insert_brick(brick, Arc::new(translated))
                    .expect("the translated brick has one storage per layout channel");
                Ok(Some(ClientEvent::BrickChanged { level, brick }))
            }
            ServerMessage::VoxelEdits {
                level,
                brick,
                changes,
                resulting_digest,
            } => {
                let held = self
                    .world
                    .level_mut(level)
                    .ok_or(ClientError::UnknownLevel(level))?
                    .overrides
                    .brick_mut(brick)
                    .ok_or(ClientError::EditToUnknownBrick(level, brick))?;
                for change in changes {
                    let (channel, value) = self
                        .translator
                        .translate_value(usize::from(change.channel), change.value)?;
                    held.set(channel, change.voxel, value);
                }
                if self.verifies_edit_digests && held.content_digest() != resulting_digest {
                    return Err(ClientError::OutOfSync(level, brick));
                }
                Ok(Some(ClientEvent::BrickChanged { level, brick }))
            }
        }
    }
}
