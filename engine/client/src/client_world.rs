use std::sync::Arc;

use namespaced_id::NamespacedId;
use protocol::{PROTOCOL_VERSION, ServerMessage, Tick, Welcome};
use region_octree::BrickCube;
use voxel_grid::Brick;
use voxel_grid::Grid;
use voxel_space::{
    BrickBox, BrickCoordinate, GridId, GridPose, LevelIndex, Orientation, WorldPosition,
};
use world_storage::{LevelData, LoadContext, LoadReport, ParsedWorld, SourceTranslator, WorldData};

use crate::client_entities::{ClientEntities, ClientEntity};
use crate::client_grids::ClientGrids;
use crate::error::ClientError;

/// What applying one message changed that a program showing the world needs to react to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientEvent {
    /// The client now watches exactly this region of this level. Edited bricks outside it were
    /// forgotten, and the terrain generators of the newly watched part are known.
    WatchedRegionChanged { level: LevelIndex, region: BrickBox },
    /// A brick's edited contents arrived or changed: a brick of a level's terrain, or of a ship.
    BrickChanged {
        grid: GridId,
        brick: BrickCoordinate,
    },
    /// A message for the game or one of its plugins.
    GameMessage {
        kind: NamespacedId,
        payload: Vec<u8>,
    },
    /// The server's answer to a command this client sent.
    CommandReply { text: String },
    /// Entities appeared, changed or went away.
    EntitiesChanged,
    /// Ships appeared, moved or went away.
    GridsChanged,
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

/// What identifies one ship of a client's view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridSummary {
    pub id: GridId,
    pub level: LevelIndex,
    pub pose: GridPose,
    /// `Grid::content_digest` of the bricks the client holds of the ship.
    pub bricks: u64,
    /// What generates the rest of its bricks, if anything does: the generator by its identifier, and the box of bricks.
    pub terrain: Option<(NamespacedId, BrickBox)>,
}

/// Everything that identifies a client's view, for comparing two views (a live client and a replay
/// of it, say) and for reporting where they differ.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewSummary {
    pub tick: Tick,
    pub watched: Option<(LevelIndex, BrickBox)>,
    /// Every level of the world, in order.
    pub levels: Vec<LevelSummary>,
    /// The entities the client has been told of, in order of id.
    pub entities: Vec<ClientEntity>,
    /// The ships the client has been told of, in order of id.
    pub grids: Vec<GridSummary>,
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
    entities: ClientEntities,
    grids: ClientGrids,
}

impl ClientWorld {
    /// Builds the world a server described. `context` supplies this program's own content and
    /// terrain generators; whatever the server has that this program lacks becomes placeholders, and
    /// the report says what.
    pub fn from_welcome(
        welcome: Welcome,
        context: LoadContext<'_>,
    ) -> Result<(Self, LoadReport), ClientError> {
        // A stream of another version, older or newer, is not understood.
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
                entities: ClientEntities::default(),
                grids: ClientGrids::default(),
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

    /// The entities the server has told this client of.
    pub fn entities(&self) -> &ClientEntities {
        &self.entities
    }

    /// The ships the server has told this client of.
    pub fn grids(&self) -> &ClientGrids {
        &self.grids
    }

    /// Where an entity is in the world: its level, and its position and turn there. An entity aboard a ship is where the ship puts it.
    /// `None` for one aboard a ship the client has not been told of (the ship is outside its picture), which has no place in the world
    /// that the client knows.
    pub fn entity_in_the_world(
        &self,
        entity: &ClientEntity,
    ) -> Option<(LevelIndex, WorldPosition, Orientation)> {
        let pose = entity.pose;
        if let Some(level) = pose.grid.root_level() {
            return Some((level, pose.position, pose.orientation));
        }
        let ship = self.grids.get(pose.grid)?;
        let (position, orientation) = ship.pose.place_in_world(pose.position, pose.orientation)?;
        Some((ship.level, position, orientation))
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
            entities: self.entities.iter().cloned().collect(),
            grids: self
                .grids
                .iter()
                .map(|(id, grid)| GridSummary {
                    id,
                    level: grid.level,
                    pose: grid.pose,
                    bricks: grid.voxels.content_digest(),
                    terrain: grid.terrain.map(|terrain| {
                        (
                            self.world
                                .main()
                                .terrain
                                .generators()
                                .entry(terrain.generator)
                                .expect("a ship's generator comes from this registry")
                                .identifier()
                                .clone(),
                            terrain.extent,
                        )
                    }),
                })
                .collect(),
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
            ServerMessage::Challenge { .. } => Err(ClientError::UnexpectedMessage(
                "a challenge after the welcome",
            )),
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
            ServerMessage::EntitiesSpawned(entities) => {
                self.entities.spawn(entities)?;
                Ok(Some(ClientEvent::EntitiesChanged))
            }
            ServerMessage::EntitiesChanged(changes) => {
                self.entities.change(changes)?;
                Ok(Some(ClientEvent::EntitiesChanged))
            }
            ServerMessage::EntitiesDespawned(ids) => {
                self.entities.despawn(ids)?;
                Ok(Some(ClientEvent::EntitiesChanged))
            }
            ServerMessage::GridsSpawned(states) => {
                let layout = Arc::clone(self.world.main().overrides.layout());
                self.grids.spawn(
                    states,
                    || Grid::new(Arc::clone(&layout)),
                    |source_id| Ok(self.translator.translate_generator(source_id)?),
                )?;
                Ok(Some(ClientEvent::GridsChanged))
            }
            ServerMessage::GridsChanged(changes) => {
                self.grids.change(changes)?;
                Ok(Some(ClientEvent::GridsChanged))
            }
            ServerMessage::GridsDespawned(ids) => {
                self.grids.despawn(ids)?;
                Ok(Some(ClientEvent::GridsChanged))
            }
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
                grid,
                brick,
                contents,
            } => {
                let contents =
                    Arc::try_unwrap(contents).unwrap_or_else(|shared| Brick::clone(&shared));
                match grid.root_level() {
                    Some(level) => {
                        if !self.watched.is_some_and(|(watched, region)| {
                            watched == level && region.contains(brick)
                        }) {
                            return Err(ClientError::BrickOutsideWatchedRegion(grid, brick));
                        }
                        let translated = self.translator.translate_brick(contents)?;
                        self.world
                            .level_mut(level)
                            .expect("the watched level exists")
                            .overrides
                            .insert_brick(brick, Arc::new(translated))
                            .expect("the translated brick has one storage per layout channel");
                    }
                    None => {
                        // The ship is looked for first: contents for one the client was not told of are that mistake, whatever they hold.
                        let ship = self.grids.get_mut(grid)?;
                        ship.insert_brick(brick, self.translator.translate_brick(contents)?);
                    }
                }
                Ok(Some(ClientEvent::BrickChanged { grid, brick }))
            }
            ServerMessage::VoxelEdits {
                grid,
                brick,
                changes,
                resulting_digest,
            } => {
                let voxels = match grid.root_level() {
                    Some(level) => {
                        &mut self
                            .world
                            .level_mut(level)
                            .ok_or(ClientError::UnknownLevel(level))?
                            .overrides
                    }
                    None => &mut self.grids.get_mut(grid)?.voxels,
                };
                let held = voxels
                    .brick_mut(brick)
                    .ok_or(ClientError::EditToUnknownBrick(grid, brick))?;
                for change in changes {
                    let (channel, value) = self
                        .translator
                        .translate_value(usize::from(change.channel), change.value)?;
                    held.set(channel, change.voxel, value);
                }
                if self.verifies_edit_digests && held.content_digest() != resulting_digest {
                    return Err(ClientError::OutOfSync(grid, brick));
                }
                Ok(Some(ClientEvent::BrickChanged { grid, brick }))
            }
        }
    }
}
