use std::sync::Arc;

use binary_codec::{ByteReader, ByteWriter, DecodeError};
use namespaced_id::NamespacedId;
use region_octree::BrickCube;
use voxel_grid::Brick;
use voxel_space::{
    BrickBox, BrickCoordinate, EntityPose, GridId, GridPose, LevelIndex, LocalVoxelIndex,
};

use crate::MAXIMUM_COMMAND_LENGTH;
use crate::error::ProtocolError;
use crate::types::{ClientId, EntityId, Tick};
use crate::wire::{
    read_brick_box, read_brick_coordinate, read_entity_pose, read_grid, read_grid_pose, read_level,
    write_brick_box, write_brick_coordinate, write_entity_pose, write_grid, write_grid_pose,
    write_level,
};

const KIND_WELCOME: u8 = 1;
const KIND_INTEREST_CHANGED: u8 = 2;
const KIND_BRICK_CONTENTS: u8 = 3;
const KIND_VOXEL_EDITS: u8 = 4;
const KIND_GAME_MESSAGE: u8 = 5;
const KIND_TICK_COMPLETED: u8 = 6;
const KIND_DISCONNECTED: u8 = 7;
const KIND_COMMAND_REPLY: u8 = 8;
const KIND_ENTITIES_SPAWNED: u8 = 9;
const KIND_ENTITIES_CHANGED: u8 = 10;
const KIND_ENTITIES_DESPAWNED: u8 = 11;
const KIND_CHALLENGE: u8 = 12;
const KIND_GRIDS_SPAWNED: u8 = 13;
const KIND_GRIDS_CHANGED: u8 = 14;
const KIND_GRIDS_DESPAWNED: u8 = 15;

/// The server's answer to `Hello`: everything a client needs to start interpreting the stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Welcome {
    pub protocol_version: u32,
    /// Which game the server runs; a recording uses it to find the game that can replay it.
    pub game: NamespacedId,
    /// The id the server gave this connection.
    pub client: ClientId,
    pub ticks_per_second: u32,
    /// The last tick the server completed before this client joined.
    pub tick: Tick,
    /// The time of the world at that tick: whole ticks since the world was made, which unlike the tick is saved with the world and goes on across a restart. The client
    /// adds one for each tick it is told of, and has what everything that depends on the time of the world (the sun, a planet on its rail) is a function of.
    pub world_clock: u64,
    /// A world file holding the world's shape without its contents (`WorldData::describe`): seed,
    /// registry tables, channels, and each level's name and generator setup. Loading it is how a
    /// client learns the numbering every brick and edit below uses, and which level each level
    /// number names (they are numbered in the file's order).
    pub world_description: Vec<u8>,
    /// What a client needs to show the world, which the protocol does not read: the game's package data (how every
    /// block looks, which channels the view reads). Sent in every welcome, so a recording carries it.
    pub package: Vec<u8>,
}

/// Which terrain generator produced the bricks of one aligned cube.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeneratorAssignment {
    pub cube: BrickCube,
    /// An index into the world description's generator table, or `None` if no generator has been
    /// assigned to the cube.
    pub generator: Option<u32>,
}

/// One voxel set to a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoxelChange {
    pub voxel: LocalVoxelIndex,
    /// The channel's position in the world description's channel table.
    pub channel: u16,
    pub value: u32,
}

/// An entity a client has just been told of: what it is, where, and the components the server
/// replicates (each as the bytes its codec made, by identifier).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityState {
    pub id: EntityId,
    pub kind: NamespacedId,
    pub pose: EntityPose,
    pub components: Vec<(NamespacedId, Vec<u8>)>,
}

/// What changed about an entity a client already knows: its pose if that moved, and the replicated
/// components whose bytes changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityChange {
    pub id: EntityId,
    pub pose: Option<EntityPose>,
    pub components: Vec<(NamespacedId, Vec<u8>)>,
}

/// What makes the bricks of a ship that are not sent (a planet: Phase 39). The client generates them itself, as it does the terrain of a level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridTerrainState {
    /// An index into the world description's generator table, as in a `GeneratorAssignment`.
    pub generator: u32,
    /// The bricks the generator makes, in the ship's own coordinates; the ship has nothing generated outside it.
    pub extent: BrickBox,
}

/// A ship a client has just been told of: which level it is in and where it is. Its bricks follow as `BrickContents` of the ship: all of them, or for a ship
/// with terrain only the ones that were edited (the others the client generates).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridState {
    /// Never the root grid of a level, which a client knows from the world description.
    pub id: GridId,
    pub level: LevelIndex,
    pub pose: GridPose,
    /// What generates the bricks that are not sent, if anything does.
    pub terrain: Option<GridTerrainState>,
}

/// A ship the client knows that moved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridChange {
    pub id: GridId,
    pub pose: GridPose,
}

/// Sent from the server to a client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServerMessage {
    /// The first message on a connection.
    Welcome(Welcome),
    /// The client now watches exactly `region` of `level`: it should forget edited bricks outside it
    /// (all of them, if it was watching another level). For every part of `region` it was not already
    /// watching, `assignments` tells it which generator made the terrain, and `BrickContents` for
    /// edited bricks follow.
    InterestChanged {
        level: LevelIndex,
        region: BrickBox,
        assignments: Vec<GeneratorAssignment>,
    },
    /// The whole contents of an edited brick of a grid (the root grid of a level has the level's number), replacing whatever
    /// the client has for it. A brick that has never been edited is not sent: clients generate it. Every brick of a ship is
    /// sent, since nothing generates a ship's.
    BrickContents {
        grid: GridId,
        brick: BrickCoordinate,
        contents: Arc<Brick>,
    },
    /// Changes to a brick the client already has contents for.
    VoxelEdits {
        grid: GridId,
        brick: BrickCoordinate,
        changes: Vec<VoxelChange>,
        /// `Brick::content_digest` of the brick once the changes are applied, in the server's own
        /// numbering, so a client numbering things the same way can notice it has drifted.
        resulting_digest: u64,
    },
    /// A message for the game or one of its plugins.
    GameMessage {
        kind: NamespacedId,
        payload: Vec<u8>,
    },
    /// The tick is finished; everything since the previous `TickCompleted` belongs to it.
    TickCompleted { tick: Tick },
    /// The server is closing this connection, for the stated reason.
    Disconnected { reason: String },
    /// The answer to a `Command` this client sent: at most [`MAXIMUM_COMMAND_LENGTH`] bytes of text.
    CommandReply { text: String },
    /// Entities that came into the region the client watches (or were made in it), in order of id.
    /// (Protocol version 3.)
    EntitiesSpawned(Vec<EntityState>),
    /// Entities the client knows that moved or whose replicated components changed, in order of id.
    EntitiesChanged(Vec<EntityChange>),
    /// Entities the client knew that left the region it watches or no longer exist, in order of id.
    EntitiesDespawned(Vec<EntityId>),
    /// The answer to `Hello`: bytes the server has not used before, which the client signs to prove it holds the
    /// private key of the key it said it is. `Welcome` follows once it has. (Protocol version 4.)
    Challenge { nonce: [u8; 32] },
    /// Ships that came into the region the client watches (or were made in it), in order of id; each is followed in the same tick by
    /// the `BrickContents` of all its bricks. (Protocol version 6.)
    GridsSpawned(Vec<GridState>),
    /// Ships the client knows that moved, in order of id.
    GridsChanged(Vec<GridChange>),
    /// Ships the client knew that left the region it watches or no longer exist, in order of id. The client forgets their bricks.
    GridsDespawned(Vec<GridId>),
}

/// Whether `frame` is a `TickCompleted` message. Recorders use it to know when a tick ends without
/// decoding the whole message.
pub fn is_tick_boundary(frame: &[u8]) -> bool {
    frame.first() == Some(&KIND_TICK_COMPLETED)
}

impl ServerMessage {
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        match self {
            Self::Welcome(welcome) => {
                writer.write_u8(KIND_WELCOME);
                writer.write_u32(welcome.protocol_version);
                writer.write_identifier(&welcome.game);
                writer.write_u32(welcome.client.get());
                writer.write_u32(welcome.ticks_per_second);
                writer.write_u64(welcome.tick.get());
                writer.write_u64(welcome.world_clock);
                writer.write_bytes(&welcome.world_description);
                writer.write_bytes(&welcome.package);
            }
            Self::InterestChanged {
                level,
                region,
                assignments,
            } => {
                writer.write_u8(KIND_INTEREST_CHANGED);
                write_level(&mut writer, *level);
                write_brick_box(&mut writer, *region);
                writer.write_varint(assignments.len() as u64);
                for assignment in assignments {
                    write_brick_coordinate(&mut writer, assignment.cube.origin());
                    writer.write_u8(assignment.cube.level());
                    writer.write_varint(assignment.generator.map_or(0, |id| u64::from(id) + 1));
                }
            }
            Self::BrickContents {
                grid,
                brick,
                contents,
            } => {
                writer.write_u8(KIND_BRICK_CONTENTS);
                write_grid(&mut writer, *grid);
                write_brick_coordinate(&mut writer, *brick);
                let mut encoded = Vec::new();
                contents.encode(&mut encoded);
                writer.write_bytes(&encoded);
            }
            Self::VoxelEdits {
                grid,
                brick,
                changes,
                resulting_digest,
            } => {
                writer.write_u8(KIND_VOXEL_EDITS);
                write_grid(&mut writer, *grid);
                write_brick_coordinate(&mut writer, *brick);
                writer.write_u64(*resulting_digest);
                writer.write_varint(changes.len() as u64);
                for change in changes {
                    writer.write_varint(change.voxel.index() as u64);
                    writer.write_varint(u64::from(change.channel));
                    writer.write_varint(u64::from(change.value));
                }
            }
            Self::GameMessage { kind, payload } => {
                writer.write_u8(KIND_GAME_MESSAGE);
                writer.write_identifier(kind);
                writer.write_bytes(payload);
            }
            Self::TickCompleted { tick } => {
                writer.write_u8(KIND_TICK_COMPLETED);
                writer.write_u64(tick.get());
            }
            Self::Disconnected { reason } => {
                writer.write_u8(KIND_DISCONNECTED);
                writer.write_string(reason);
            }
            Self::CommandReply { text } => {
                writer.write_u8(KIND_COMMAND_REPLY);
                writer.write_string(text);
            }
            Self::EntitiesSpawned(entities) => {
                writer.write_u8(KIND_ENTITIES_SPAWNED);
                writer.write_varint(entities.len() as u64);
                for entity in entities {
                    writer.write_varint(entity.id.get());
                    writer.write_identifier(&entity.kind);
                    write_entity_pose(&mut writer, &entity.pose);
                    write_components(&mut writer, &entity.components);
                }
            }
            Self::EntitiesChanged(changes) => {
                writer.write_u8(KIND_ENTITIES_CHANGED);
                writer.write_varint(changes.len() as u64);
                for change in changes {
                    writer.write_varint(change.id.get());
                    match &change.pose {
                        None => writer.write_u8(0),
                        Some(pose) => {
                            writer.write_u8(1);
                            write_entity_pose(&mut writer, pose);
                        }
                    }
                    write_components(&mut writer, &change.components);
                }
            }
            Self::Challenge { nonce } => {
                writer.write_u8(KIND_CHALLENGE);
                writer.write_raw(nonce);
            }
            Self::EntitiesDespawned(ids) => {
                writer.write_u8(KIND_ENTITIES_DESPAWNED);
                writer.write_varint(ids.len() as u64);
                for id in ids {
                    writer.write_varint(id.get());
                }
            }
            Self::GridsSpawned(grids) => {
                writer.write_u8(KIND_GRIDS_SPAWNED);
                writer.write_varint(grids.len() as u64);
                for grid in grids {
                    write_grid(&mut writer, grid.id);
                    write_level(&mut writer, grid.level);
                    write_grid_pose(&mut writer, &grid.pose);
                    match grid.terrain {
                        None => writer.write_u8(0),
                        Some(terrain) => {
                            writer.write_u8(1);
                            writer.write_varint(u64::from(terrain.generator));
                            write_brick_box(&mut writer, terrain.extent);
                        }
                    }
                }
            }
            Self::GridsChanged(changes) => {
                writer.write_u8(KIND_GRIDS_CHANGED);
                writer.write_varint(changes.len() as u64);
                for change in changes {
                    write_grid(&mut writer, change.id);
                    write_grid_pose(&mut writer, &change.pose);
                }
            }
            Self::GridsDespawned(ids) => {
                writer.write_u8(KIND_GRIDS_DESPAWNED);
                writer.write_varint(ids.len() as u64);
                for id in ids {
                    write_grid(&mut writer, *id);
                }
            }
        }
        writer.into_bytes()
    }

    pub fn decode(frame: &[u8]) -> Result<Self, ProtocolError> {
        let mut reader = ByteReader::new(frame);
        let message = match reader.read_u8().map_err(|_| ProtocolError::EmptyFrame)? {
            KIND_WELCOME => Self::Welcome(Welcome {
                protocol_version: reader.read_u32()?,
                game: reader.read_identifier()?,
                client: ClientId::new(reader.read_u32()?),
                ticks_per_second: reader.read_u32()?,
                tick: Tick::new(reader.read_u64()?),
                world_clock: reader.read_u64()?,
                world_description: reader.read_bytes()?.to_vec(),
                package: reader.read_bytes()?.to_vec(),
            }),
            KIND_INTEREST_CHANGED => {
                let level = read_level(&mut reader)?;
                let region = read_brick_box(&mut reader)?;
                let count = reader.read_count(5)?;
                let mut assignments = Vec::with_capacity(count);
                for _ in 0..count {
                    let origin = read_brick_coordinate(&mut reader)?;
                    let level = reader.read_u8()?;
                    let cube = BrickCube::new(origin, level).map_err(|_| {
                        DecodeError::Invalid("an assignment cube is misaligned or too large")
                    })?;
                    let generator = match reader.read_varint()? {
                        0 => None,
                        plus_one => Some(
                            u32::try_from(plus_one - 1)
                                .map_err(|_| DecodeError::Invalid("a generator id is too large"))?,
                        ),
                    };
                    assignments.push(GeneratorAssignment { cube, generator });
                }
                Self::InterestChanged {
                    level,
                    region,
                    assignments,
                }
            }
            KIND_BRICK_CONTENTS => {
                let grid = read_grid(&mut reader)?;
                let brick = read_brick_coordinate(&mut reader)?;
                let mut encoded = reader.read_bytes()?;
                let contents = Brick::decode(&mut encoded)?;
                if !encoded.is_empty() {
                    return Err(DecodeError::TrailingBytes(encoded.len()).into());
                }
                Self::BrickContents {
                    grid,
                    brick,
                    contents: Arc::new(contents),
                }
            }
            KIND_VOXEL_EDITS => {
                let grid = read_grid(&mut reader)?;
                let brick = read_brick_coordinate(&mut reader)?;
                let resulting_digest = reader.read_u64()?;
                let count = reader.read_count(3)?;
                let mut changes = Vec::with_capacity(count);
                for _ in 0..count {
                    let voxel = usize::try_from(reader.read_varint()?)
                        .ok()
                        .and_then(LocalVoxelIndex::from_index)
                        .ok_or(DecodeError::Invalid("a voxel index is outside the brick"))?;
                    let channel = u16::try_from(reader.read_varint()?)
                        .map_err(|_| DecodeError::Invalid("a channel number is too large"))?;
                    let value = u32::try_from(reader.read_varint()?)
                        .map_err(|_| DecodeError::Invalid("a voxel value is too large"))?;
                    changes.push(VoxelChange {
                        voxel,
                        channel,
                        value,
                    });
                }
                Self::VoxelEdits {
                    grid,
                    brick,
                    changes,
                    resulting_digest,
                }
            }
            KIND_GAME_MESSAGE => Self::GameMessage {
                kind: reader.read_identifier()?,
                payload: reader.read_bytes()?.to_vec(),
            },
            KIND_TICK_COMPLETED => Self::TickCompleted {
                tick: Tick::new(reader.read_u64()?),
            },
            KIND_DISCONNECTED => Self::Disconnected {
                reason: reader.read_string()?.to_owned(),
            },
            KIND_COMMAND_REPLY => {
                let text = reader.read_string()?;
                if text.len() > MAXIMUM_COMMAND_LENGTH {
                    return Err(DecodeError::Invalid("a command reply is too long").into());
                }
                Self::CommandReply {
                    text: text.to_owned(),
                }
            }
            KIND_ENTITIES_SPAWNED => {
                // An id, a kind, a pose (at least 33 bytes) and a component count.
                let count = reader.read_count(40)?;
                let mut entities = Vec::with_capacity(count);
                for _ in 0..count {
                    entities.push(EntityState {
                        id: EntityId::new(reader.read_varint()?),
                        kind: reader.read_identifier()?,
                        pose: read_entity_pose(&mut reader)?,
                        components: read_components(&mut reader)?,
                    });
                }
                Self::EntitiesSpawned(entities)
            }
            KIND_ENTITIES_CHANGED => {
                let count = reader.read_count(3)?;
                let mut changes = Vec::with_capacity(count);
                for _ in 0..count {
                    let id = EntityId::new(reader.read_varint()?);
                    let pose = match reader.read_u8()? {
                        0 => None,
                        1 => Some(read_entity_pose(&mut reader)?),
                        _ => {
                            return Err(
                                DecodeError::Invalid("a pose is neither there nor not").into()
                            );
                        }
                    };
                    changes.push(EntityChange {
                        id,
                        pose,
                        components: read_components(&mut reader)?,
                    });
                }
                Self::EntitiesChanged(changes)
            }
            KIND_CHALLENGE => Self::Challenge {
                nonce: reader.read_raw(32)?.try_into().expect("32 bytes were read"),
            },
            KIND_ENTITIES_DESPAWNED => {
                let count = reader.read_count(1)?;
                let mut ids = Vec::with_capacity(count);
                for _ in 0..count {
                    ids.push(EntityId::new(reader.read_varint()?));
                }
                Self::EntitiesDespawned(ids)
            }
            KIND_GRIDS_SPAWNED => {
                // An id, a level, a pose, and whether there is terrain (at least 32 bytes).
                let count = reader.read_count(32)?;
                let mut grids = Vec::with_capacity(count);
                for _ in 0..count {
                    let id = read_ship(&mut reader)?;
                    let level = read_level(&mut reader)?;
                    let pose = read_grid_pose(&mut reader)?;
                    let terrain = match reader.read_u8()? {
                        0 => None,
                        1 => Some(GridTerrainState {
                            generator: u32::try_from(reader.read_varint()?).map_err(|_| {
                                DecodeError::Invalid("a generator number is too large")
                            })?,
                            extent: read_brick_box(&mut reader)?,
                        }),
                        _ => {
                            return Err(
                                DecodeError::Invalid("a terrain flag is neither 0 nor 1").into()
                            );
                        }
                    };
                    grids.push(GridState {
                        id,
                        level,
                        pose,
                        terrain,
                    });
                }
                Self::GridsSpawned(grids)
            }
            KIND_GRIDS_CHANGED => {
                let count = reader.read_count(30)?;
                let mut changes = Vec::with_capacity(count);
                for _ in 0..count {
                    changes.push(GridChange {
                        id: read_ship(&mut reader)?,
                        pose: read_grid_pose(&mut reader)?,
                    });
                }
                Self::GridsChanged(changes)
            }
            KIND_GRIDS_DESPAWNED => {
                let count = reader.read_count(1)?;
                let mut ids = Vec::with_capacity(count);
                for _ in 0..count {
                    ids.push(read_ship(&mut reader)?);
                }
                Self::GridsDespawned(ids)
            }
            unknown => return Err(ProtocolError::UnknownMessageKind(unknown)),
        };
        reader.finish()?;
        Ok(message)
    }
}

/// The id of a ship: a root grid is a level, which a client knows from the world description and is never told of.
fn read_ship(reader: &mut ByteReader<'_>) -> Result<GridId, DecodeError> {
    let id = read_grid(reader)?;
    if id.root_level().is_some() {
        return Err(DecodeError::Invalid(
            "the root grid of a level is not a ship",
        ));
    }
    Ok(id)
}

/// A list of components: a count, then each identifier and its bytes. The identifiers are in order,
/// once each, so there is one way to write any list.
fn write_components(writer: &mut ByteWriter, components: &[(NamespacedId, Vec<u8>)]) {
    writer.write_varint(components.len() as u64);
    for (identifier, bytes) in components {
        writer.write_identifier(identifier);
        writer.write_bytes(bytes);
    }
}

fn read_components(
    reader: &mut ByteReader<'_>,
) -> Result<Vec<(NamespacedId, Vec<u8>)>, DecodeError> {
    let count = reader.read_count(5)?;
    let mut components: Vec<(NamespacedId, Vec<u8>)> = Vec::with_capacity(count);
    for _ in 0..count {
        let identifier = reader.read_identifier()?;
        if components
            .last()
            .is_some_and(|(last, _)| *last >= identifier)
        {
            return Err(DecodeError::Invalid(
                "components are out of order or repeated",
            ));
        }
        components.push((identifier, reader.read_bytes()?.to_vec()));
    }
    Ok(components)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use voxel_grid::ChannelStorage;
    use voxel_grid::test_support::StandardTestLayout;
    use voxel_space::BRICK_VOXEL_COUNT;
    use voxel_space::test_support::{brick_coordinate, local_voxel_index};

    use super::*;

    fn id(text: &str) -> NamespacedId {
        text.parse().unwrap()
    }

    fn sample_brick() -> Arc<Brick> {
        let fixture = StandardTestLayout::default();
        let mut brick = Brick::new(&fixture.layout);
        brick.set(fixture.material, LocalVoxelIndex::new(1, 2, 3), 7);
        brick.set(fixture.temperature, LocalVoxelIndex::new(0, 0, 31), 900);
        Arc::new(brick)
    }

    fn samples() -> Vec<ServerMessage> {
        vec![
            ServerMessage::Welcome(Welcome {
                protocol_version: 2,
                game: id("voxunami_cave:game"),
                client: ClientId::new(3),
                ticks_per_second: 20,
                tick: Tick::new(12_345),
                world_clock: 9_876_543_210,
                world_description: vec![5; 40],
                package: vec![6; 25],
            }),
            ServerMessage::InterestChanged {
                level: LevelIndex::new(1),
                region: BrickBox::around(BrickCoordinate::new(0, -2, 9), 3),
                assignments: vec![
                    GeneratorAssignment {
                        cube: BrickCube::new(BrickCoordinate::new(-8, 0, 8), 3).unwrap(),
                        generator: Some(0),
                    },
                    GeneratorAssignment {
                        cube: BrickCube::new(BrickCoordinate::new(0, 0, 0), 3).unwrap(),
                        generator: None,
                    },
                ],
            },
            ServerMessage::InterestChanged {
                level: LevelIndex::MAIN,
                region: BrickBox::around(BrickCoordinate::ORIGIN, 0),
                assignments: Vec::new(),
            },
            ServerMessage::BrickContents {
                grid: GridId::root(LevelIndex::new(300)),
                brick: BrickCoordinate::new(-5, 6, 7),
                contents: sample_brick(),
            },
            ServerMessage::BrickContents {
                grid: ship(),
                brick: BrickCoordinate::new(0, 0, -1),
                contents: sample_brick(),
            },
            ServerMessage::VoxelEdits {
                grid: ship(),
                brick: BrickCoordinate::new(0, 0, -1),
                changes: vec![VoxelChange {
                    voxel: LocalVoxelIndex::new(2, 2, 2),
                    channel: 1,
                    value: 9,
                }],
                resulting_digest: 7,
            },
            ServerMessage::VoxelEdits {
                grid: GridId::root(LevelIndex::new(2)),
                brick: BrickCoordinate::new(1, 1, 1),
                changes: vec![
                    VoxelChange {
                        voxel: LocalVoxelIndex::new(0, 0, 0),
                        channel: 0,
                        value: 4,
                    },
                    VoxelChange {
                        voxel: LocalVoxelIndex::new(31, 31, 31),
                        channel: 65_535,
                        value: u32::MAX,
                    },
                ],
                resulting_digest: 0xDEAD_BEEF_0123_4567,
            },
            ServerMessage::GameMessage {
                kind: id("voxunami_cave:entities"),
                payload: vec![0, 1, 2],
            },
            ServerMessage::TickCompleted {
                tick: Tick::new(u64::MAX),
            },
            ServerMessage::Disconnected {
                reason: "the server is shutting down".to_owned(),
            },
            ServerMessage::CommandReply {
                text: "you are in the nether".to_owned(),
            },
            ServerMessage::EntitiesSpawned(vec![
                EntityState {
                    id: EntityId::new(1),
                    kind: id("voxunami_cave:item"),
                    pose: sample_pose(3),
                    components: vec![
                        (id("engine:item_stack"), vec![1, 2, 3]),
                        (id("voxunami_cave:spin"), vec![]),
                    ],
                },
                EntityState {
                    id: EntityId::new(u64::MAX),
                    kind: id("a:b"),
                    pose: sample_pose(-9),
                    components: Vec::new(),
                },
            ]),
            ServerMessage::EntitiesSpawned(Vec::new()),
            ServerMessage::EntitiesChanged(vec![
                EntityChange {
                    id: EntityId::new(2),
                    pose: Some(sample_pose(0)),
                    components: Vec::new(),
                },
                EntityChange {
                    id: EntityId::new(7),
                    pose: None,
                    components: vec![(id("a:health"), vec![9, 0, 0, 0])],
                },
            ]),
            ServerMessage::EntitiesDespawned(vec![EntityId::new(1), EntityId::new(300)]),
            ServerMessage::EntitiesDespawned(Vec::new()),
            ServerMessage::Challenge { nonce: [77; 32] },
            ServerMessage::GridsSpawned(vec![
                GridState {
                    id: ship(),
                    level: LevelIndex::new(1),
                    pose: sample_grid_pose(3),
                    terrain: None,
                },
                GridState {
                    id: GridId::new(u64::MAX),
                    level: LevelIndex::MAIN,
                    pose: sample_grid_pose(-9),
                    terrain: Some(GridTerrainState {
                        generator: 2,
                        extent: BrickBox::around(BrickCoordinate::new(0, -3, 7), 2000),
                    }),
                },
            ]),
            ServerMessage::GridsSpawned(Vec::new()),
            ServerMessage::GridsChanged(vec![GridChange {
                id: ship(),
                pose: sample_grid_pose(0),
            }]),
            ServerMessage::GridsDespawned(vec![ship(), GridId::new(GridId::FIRST_ALLOCATED + 300)]),
        ]
    }

    fn ship() -> GridId {
        GridId::new(GridId::FIRST_ALLOCATED + 5)
    }

    fn sample_grid_pose(brick_x: i128) -> GridPose {
        let pose = sample_pose(brick_x);
        GridPose::new(pose.position, pose.orientation)
    }

    fn sample_pose(brick_x: i128) -> EntityPose {
        EntityPose::new(
            voxel_space::GridId::root(LevelIndex::new(1)),
            voxel_space::WorldPosition::new(BrickCoordinate::new(brick_x, 0, 1), [0.5, 31.0, 2.0]),
            voxel_space::Orientation::from_yaw_pitch(0.5, -0.25),
        )
    }

    #[test]
    fn every_message_survives_encoding() {
        for message in samples() {
            assert_eq!(
                ServerMessage::decode(&message.encode()),
                Ok(message.clone()),
                "{message:?}"
            );
        }
    }

    #[test]
    fn the_clock_of_the_world_is_in_the_welcome_after_the_tick_and_before_the_description() {
        let welcome = ServerMessage::Welcome(Welcome {
            protocol_version: 0x0A0B_0C0D,
            game: id("a:b"),
            client: ClientId::new(1),
            ticks_per_second: 20,
            tick: Tick::new(0x0102),
            world_clock: 0x1112_1314_1516_1718,
            world_description: vec![0xDE],
            package: vec![0xAD],
        });
        assert_eq!(
            welcome.encode(),
            [
                1, // the kind
                0x0D, 0x0C, 0x0B, 0x0A, // the version
                3, b'a', b':', b'b', // the game
                1, 0, 0, 0, // the client
                20, 0, 0, 0, // ticks per second
                2, 1, 0, 0, 0, 0, 0, 0, // the tick
                0x18, 0x17, 0x16, 0x15, 0x14, 0x13, 0x12, 0x11, // the clock
                1, 0xDE, // the description
                1, 0xAD, // the package
            ]
        );
    }

    #[test]
    fn the_wire_layout_is_pinned() {
        // Changing a layout needs a new protocol version.
        assert_eq!(
            ServerMessage::TickCompleted {
                tick: Tick::new(0x0102)
            }
            .encode(),
            [6, 2, 1, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            ServerMessage::Disconnected {
                reason: "bye".to_owned()
            }
            .encode(),
            [7, 3, b'b', b'y', b'e']
        );
        assert_eq!(
            ServerMessage::GameMessage {
                kind: id("a:b"),
                payload: vec![9]
            }
            .encode(),
            [5, 3, b'a', b':', b'b', 1, 9]
        );
        assert_eq!(
            ServerMessage::CommandReply {
                text: "ok".to_owned()
            }
            .encode(),
            [8, 2, b'o', b'k']
        );
        let mut challenge = vec![12];
        challenge.extend([3; 32]);
        assert_eq!(
            ServerMessage::Challenge { nonce: [3; 32] }.encode(),
            challenge
        );
        assert_eq!(
            ServerMessage::EntitiesDespawned(vec![EntityId::new(1), EntityId::new(300)]).encode(),
            [11, 2, 1, 0xAC, 0x02]
        );
        // A ship's number is 2^32 + 5: a varint of 5 bytes. The kind, a count, then the numbers.
        assert_eq!(
            ServerMessage::GridsDespawned(vec![ship(), GridId::new(GridId::FIRST_ALLOCATED)])
                .encode(),
            [
                15, 2, 0x85, 0x80, 0x80, 0x80, 0x10, 0x80, 0x80, 0x80, 0x80, 0x10
            ]
        );
        let mut changed = vec![14, 1, 0x85, 0x80, 0x80, 0x80, 0x10];
        // The brick, then the offset as the bits of 32-bit floats and the orientation as the bits of 64-bit floats (since version 7).
        changed.extend([0, 0, 0]);
        changed.extend(1.0f32.to_bits().to_le_bytes());
        changed.extend(2.0f32.to_bits().to_le_bytes());
        changed.extend(3.0f32.to_bits().to_le_bytes());
        for component in [0.0f64, 0.0, 0.0, 1.0] {
            changed.extend(component.to_bits().to_le_bytes());
        }
        assert_eq!(
            ServerMessage::GridsChanged(vec![GridChange {
                id: ship(),
                pose: GridPose::new(
                    voxel_space::WorldPosition::new(BrickCoordinate::ORIGIN, [1.0, 2.0, 3.0]),
                    voxel_space::Orientation::IDENTITY
                ),
            }])
            .encode(),
            changed
        );
        assert_eq!(
            ServerMessage::EntitiesChanged(vec![EntityChange {
                id: EntityId::new(5),
                pose: None,
                components: vec![(id("a:b"), vec![7])],
            }])
            .encode(),
            [10, 1, 5, 0, 1, 3, b'a', b':', b'b', 1, 7]
        );
        assert_eq!(
            ServerMessage::VoxelEdits {
                grid: GridId::root(LevelIndex::new(1)),
                brick: BrickCoordinate::new(1, 0, 0),
                changes: vec![VoxelChange {
                    voxel: LocalVoxelIndex::new(1, 0, 0),
                    channel: 300,
                    value: 5
                }],
                resulting_digest: 1,
            }
            .encode(),
            [4, 1, 1, 2, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0xAC, 2, 5]
        );
        assert_eq!(
            ServerMessage::InterestChanged {
                level: LevelIndex::new(2),
                region: BrickBox::around(BrickCoordinate::ORIGIN, 1),
                assignments: vec![GeneratorAssignment {
                    cube: BrickCube::new(BrickCoordinate::new(0, 0, 0), 0).unwrap(),
                    generator: Some(2)
                }],
            }
            .encode(),
            [2, 2, 1, 1, 1, 1, 1, 1, 1, 2, 1, 2, 1, 2, 1, 0, 0, 0, 0, 3]
        );
    }

    #[test]
    fn tick_boundaries_are_recognised_from_the_first_byte() {
        for message in samples() {
            assert_eq!(
                is_tick_boundary(&message.encode()),
                matches!(message, ServerMessage::TickCompleted { .. })
            );
        }
        assert!(!is_tick_boundary(&[]));
    }

    #[test]
    fn damaged_frames_are_errors_not_panics() {
        assert_eq!(ServerMessage::decode(&[]), Err(ProtocolError::EmptyFrame));
        assert_eq!(
            ServerMessage::decode(&[0]),
            Err(ProtocolError::UnknownMessageKind(0))
        );
        for message in samples() {
            let frame = message.encode();
            for length in 0..frame.len() {
                assert!(
                    ServerMessage::decode(&frame[..length]).is_err(),
                    "{message:?} truncated to {length}"
                );
            }
            let mut trailing = frame.clone();
            trailing.push(0);
            assert!(ServerMessage::decode(&trailing).is_err(), "{message:?}");
        }
    }

    #[test]
    fn a_voxel_edit_outside_the_brick_is_refused() {
        let edit_frame = |voxel_index: u64| {
            let mut writer = ByteWriter::new();
            writer.write_u8(KIND_VOXEL_EDITS);
            writer.write_varint(0); // level
            for coordinate in [0, 0, 0] {
                writer.write_wide_signed(coordinate);
            }
            writer.write_u64(0); // digest
            writer.write_varint(1); // one change
            writer.write_varint(voxel_index);
            writer.write_varint(0); // channel
            writer.write_varint(0); // value
            writer.into_bytes()
        };
        let last_voxel = (BRICK_VOXEL_COUNT - 1) as u64;
        assert!(ServerMessage::decode(&edit_frame(last_voxel)).is_ok());
        assert!(ServerMessage::decode(&edit_frame(last_voxel + 1)).is_err());
        assert!(ServerMessage::decode(&edit_frame(u64::MAX)).is_err());
    }

    #[test]
    fn a_ship_with_terrain_is_told_of_with_the_generator_and_the_extent_and_a_bad_one_is_refused() {
        let frame = |terrain: &dyn Fn(&mut ByteWriter)| {
            let mut writer = ByteWriter::new();
            writer.write_u8(KIND_GRIDS_SPAWNED);
            writer.write_varint(1);
            write_grid(&mut writer, ship());
            write_level(&mut writer, LevelIndex::MAIN);
            write_grid_pose(&mut writer, &sample_grid_pose(0));
            terrain(&mut writer);
            writer.into_bytes()
        };
        let box_of = |low: [i128; 3], high: [i128; 3]| {
            move |writer: &mut ByteWriter| {
                for corner in [low, high] {
                    for component in corner {
                        writer.write_wide_signed(component);
                    }
                }
            }
        };
        // The right bytes are read as the terrain they say.
        let good = frame(&|writer| {
            writer.write_u8(1);
            writer.write_varint(5);
            box_of([-20; 3], [19; 3])(writer);
        });
        let ServerMessage::GridsSpawned(grids) = ServerMessage::decode(&good).unwrap() else {
            panic!("not the message that was written");
        };
        assert_eq!(
            grids[0].terrain,
            Some(GridTerrainState {
                generator: 5,
                extent: BrickBox::new(
                    BrickCoordinate::new(-20, -20, -20),
                    BrickCoordinate::new(19, 19, 19)
                )
                .unwrap()
            })
        );
        // A flag that is neither 0 nor 1, a generator beyond 32 bits, corners the wrong way round and a message cut short are not.
        assert!(
            ServerMessage::decode(&frame(&|writer| {
                writer.write_u8(2);
                writer.write_varint(0);
                box_of([0; 3], [1; 3])(writer);
            }))
            .is_err()
        );
        assert!(
            ServerMessage::decode(&frame(&|writer| {
                writer.write_u8(1);
                writer.write_varint(u64::from(u32::MAX) + 1);
                box_of([0; 3], [1; 3])(writer);
            }))
            .is_err()
        );
        assert!(
            ServerMessage::decode(&frame(&|writer| {
                writer.write_u8(1);
                writer.write_varint(0);
                box_of([1; 3], [0; 3])(writer);
            }))
            .is_err()
        );
        assert!(ServerMessage::decode(&good[..good.len() - 1]).is_err());
    }

    #[test]
    fn the_root_grid_of_a_level_is_never_told_of_as_a_ship() {
        let root = GridId::root(LevelIndex::new(1));
        for message in [
            ServerMessage::GridsSpawned(vec![GridState {
                id: root,
                level: LevelIndex::MAIN,
                pose: sample_grid_pose(0),
                terrain: None,
            }]),
            ServerMessage::GridsChanged(vec![GridChange {
                id: root,
                pose: sample_grid_pose(0),
            }]),
            ServerMessage::GridsDespawned(vec![root]),
        ] {
            assert!(
                ServerMessage::decode(&message.encode()).is_err(),
                "{message:?}"
            );
        }
    }

    #[test]
    fn a_brick_with_a_malformed_channel_is_refused() {
        let mut frame = ServerMessage::BrickContents {
            grid: GridId::MAIN,
            brick: BrickCoordinate::ORIGIN,
            contents: Arc::new(Brick::from_channels(vec![ChannelStorage::uniform(1)])),
        }
        .encode();
        // Kind, level, three coordinates, brick length, channel count (2 bytes), then the storage tag.
        let tag_position = 1 + 1 + 3 + 1 + 2;
        assert_eq!(frame[tag_position], 0);
        frame[tag_position] = 77;
        assert!(matches!(
            ServerMessage::decode(&frame),
            Err(ProtocolError::MalformedBrick(_))
        ));
    }

    fn arbitrary_brick() -> impl Strategy<Value = Arc<Brick>> {
        proptest::collection::vec((local_voxel_index(), 0u32..40, 0u32..2000), 0..40).prop_map(
            |edits| {
                let fixture = StandardTestLayout::default();
                let mut brick = Brick::new(&fixture.layout);
                for (voxel, material, temperature) in edits {
                    brick.set(fixture.material, voxel, material);
                    brick.set(fixture.temperature, voxel, temperature);
                }
                Arc::new(brick)
            },
        )
    }

    proptest! {
        #[test]
        fn bricks_and_edits_round_trip(
            coordinate in brick_coordinate(),
            contents in arbitrary_brick(),
            changes in proptest::collection::vec((local_voxel_index(), any::<u16>(), any::<u32>()), 0..30),
            digest in any::<u64>(),
            grid in any::<u64>(),
        ) {
            let grid = GridId::new(grid);
            let brick_message = ServerMessage::BrickContents { grid, brick: coordinate, contents };
            prop_assert_eq!(ServerMessage::decode(&brick_message.encode()), Ok(brick_message));
            let edit_message = ServerMessage::VoxelEdits {
                grid,
                brick: coordinate,
                changes: changes
                    .into_iter()
                    .map(|(voxel, channel, value)| VoxelChange { voxel, channel, value })
                    .collect(),
                resulting_digest: digest,
            };
            prop_assert_eq!(ServerMessage::decode(&edit_message.encode()), Ok(edit_message));
        }

        #[test]
        fn decoding_arbitrary_bytes_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..120)) {
            let _ = ServerMessage::decode(&bytes);
        }

        #[test]
        fn changing_any_byte_of_a_valid_frame_never_panics(
            which in 0usize..8,
            position in any::<prop::sample::Index>(),
            replacement in any::<u8>(),
        ) {
            let mut frame = samples()[which].encode();
            let position = position.index(frame.len());
            frame[position] = replacement;
            let _ = ServerMessage::decode(&frame);
        }
    }
}
