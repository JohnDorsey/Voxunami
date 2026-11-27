//! Turning what a *source* wrote (a world file, or a server) into the loaded world's own numbering.
//!
//! A source numbers its content (block types, terrain generators) and orders its channels in its
//! own way, and tells the reader how in tables. Loading builds a [`SourceTranslator`] from those
//! tables; it then translates every brick, voxel change and generator reference the source
//! supplies, whether they all arrive at load time (a file) or trickle in later (a server).

use std::collections::HashMap;
use std::sync::Arc;

use namespaced_id::NamespacedId;
use terrain::GeneratorId;
use voxel_grid::{Brick, ChannelIndex, ChannelStorage, ChannelValueKind, GridLayout};

use crate::engine_sections::ChannelRecord;

/// The source supplied something its own tables do not account for.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TranslationError {
    #[error("channel {channel} uses the registry {registry}, which the source has no table for")]
    ChannelRegistryNotInSource {
        channel: NamespacedId,
        registry: NamespacedId,
    },
    #[error("a brick has {found} channels but the source's channel table lists {expected}")]
    BrickChannelCountMismatch { expected: usize, found: usize },
    #[error("a change refers to channel number {0}, but the source lists fewer channels")]
    ChannelOutsideTable(usize),
    #[error(
        "channel {channel} holds id {value}, but registry {registry} only has {entries} entries"
    )]
    ValueOutsideRegistry {
        channel: NamespacedId,
        registry: NamespacedId,
        value: u32,
        entries: usize,
    },
    #[error(
        "an assignment refers to generator id {0}, which the source's generator table does not have"
    )]
    GeneratorOutsideTable(u32),
    #[error("entries were appended to the registry {0}, which the source has no table for")]
    RegistryNotInSource(NamespacedId),
    #[error("{entry} was appended to the registry {registry}, which already lists it")]
    AppendedEntryAlreadyListed {
        registry: NamespacedId,
        entry: NamespacedId,
    },
    #[error(
        "{identifiers} entries were appended to the registry {registry} with {numbers} numbers"
    )]
    AppendedEntryCountsDiffer {
        registry: NamespacedId,
        identifiers: usize,
        numbers: usize,
    },
}

/// What a source's values in one content channel mean.
#[derive(Debug)]
struct ContentValues {
    registry: NamespacedId,
    /// How many entries the source's table for this registry lists; larger values are invalid.
    source_entry_count: usize,
    /// Source id to runtime id. `None` when nobody supplies this registry: the ids stay as the
    /// source wrote them, and are saved back unchanged.
    mapping: Option<Vec<u32>>,
}

#[derive(Debug)]
struct SourceChannel {
    identifier: NamespacedId,
    layout_index: ChannelIndex,
    content: Option<ContentValues>,
}

/// Translates a source's channel positions, content ids and generator ids into runtime terms.
#[derive(Debug)]
pub struct SourceTranslator {
    layout: Arc<GridLayout>,
    channels: Vec<SourceChannel>,
    generators: Vec<GeneratorId>,
    /// The entries of each registry the source lists, in the source's order: what it said at the start and what it has appended since.
    source_tables: HashMap<NamespacedId, Vec<NamespacedId>>,
}

impl SourceTranslator {
    /// `source_channels` is the source's channel table in its order; every one must be in `layout`.
    /// `table_lengths` and `mappings` are per registry kind: how many entries the source lists, and
    /// source id to runtime id where a registry is supplied. `generators` maps source generator ids.
    pub(crate) fn new(
        layout: Arc<GridLayout>,
        source_channels: &[ChannelRecord],
        tables: &HashMap<&NamespacedId, &Vec<NamespacedId>>,
        mappings: HashMap<NamespacedId, Vec<u32>>,
        generators: Vec<GeneratorId>,
    ) -> Result<Self, TranslationError> {
        let mut channels = Vec::with_capacity(source_channels.len());
        for record in source_channels {
            let content = match &record.value_kind {
                ChannelValueKind::Number => None,
                ChannelValueKind::Content { registry } => {
                    let source_entry_count = tables
                        .get(registry)
                        .ok_or_else(|| TranslationError::ChannelRegistryNotInSource {
                            channel: record.identifier.clone(),
                            registry: registry.clone(),
                        })?
                        .len();
                    Some(ContentValues {
                        registry: registry.clone(),
                        source_entry_count,
                        mapping: mappings.get(registry).cloned(),
                    })
                }
            };
            channels.push(SourceChannel {
                identifier: record.identifier.clone(),
                layout_index: layout
                    .channel_index(&record.identifier)
                    .expect("every source channel is in the layout"),
                content,
            });
        }
        Ok(Self {
            layout,
            channels,
            generators,
            source_tables: tables
                .iter()
                .map(|(kind, entries)| ((*kind).clone(), (*entries).clone()))
                .collect(),
        })
    }

    /// Whether the source may append these entries to the registry `registry`: it has a table for it, and lists none of them (and none twice). What
    /// [`SourceTranslator::append_content`] checks first, for a caller that wants to know before it gives the entries numbers.
    pub fn check_append(
        &self,
        registry: &NamespacedId,
        identifiers: &[NamespacedId],
    ) -> Result<(), TranslationError> {
        let table = self
            .source_tables
            .get(registry)
            .ok_or_else(|| TranslationError::RegistryNotInSource(registry.clone()))?;
        let mut seen: std::collections::HashSet<&NamespacedId> = table.iter().collect();
        for identifier in identifiers {
            if !seen.insert(identifier) {
                return Err(TranslationError::AppendedEntryAlreadyListed {
                    registry: registry.clone(),
                    entry: identifier.clone(),
                });
            }
        }
        Ok(())
    }

    /// The source has appended these entries to the registry `registry` (a server that replaced its package): they are the next numbers of its table, in order, and
    /// `local_ids` are the numbers the loaded world gives them (one for each). Values past the old table are valid from now on, as far as these. Nothing changes if
    /// it is refused: for a registry the source has no table for, a number count that is not the entries', or an entry the table has already (or twice).
    pub fn append_content(
        &mut self,
        registry: &NamespacedId,
        identifiers: &[NamespacedId],
        local_ids: &[u32],
    ) -> Result<(), TranslationError> {
        self.check_append(registry, identifiers)?;
        if identifiers.len() != local_ids.len() {
            return Err(TranslationError::AppendedEntryCountsDiffer {
                registry: registry.clone(),
                identifiers: identifiers.len(),
                numbers: local_ids.len(),
            });
        }
        for channel in &mut self.channels {
            let Some(content) = channel
                .content
                .as_mut()
                .filter(|content| content.registry == *registry)
            else {
                continue;
            };
            content.source_entry_count += identifiers.len();
            if let Some(mapping) = &mut content.mapping {
                mapping.extend_from_slice(local_ids);
            }
        }
        self.source_tables
            .get_mut(registry)
            .expect("the table was found above")
            .extend_from_slice(identifiers);
        Ok(())
    }

    /// Whether the source numbers its channels and content exactly as the loaded world does, so a
    /// value means the same on both sides and a brick's digest can be compared across them.
    pub fn is_identity(&self) -> bool {
        self.layout.channel_count() == self.channels.len()
            && self.channels.iter().enumerate().all(|(position, channel)| {
                channel.layout_index.get() == position
                    && channel
                        .content
                        .as_ref()
                        .and_then(|content| content.mapping.as_ref())
                        .is_none_or(|mapping| {
                            mapping
                                .iter()
                                .enumerate()
                                .all(|(source_id, &local_id)| local_id as usize == source_id)
                        })
            })
    }

    /// The same brick in the loaded world's channel order and ids. Channels the source does not
    /// have are at their default.
    pub fn translate_brick(&self, brick: Brick) -> Result<Brick, TranslationError> {
        let source_storages = brick.into_channels();
        if source_storages.len() != self.channels.len() {
            return Err(TranslationError::BrickChannelCountMismatch {
                expected: self.channels.len(),
                found: source_storages.len(),
            });
        }
        let mut storages: Vec<ChannelStorage> = self
            .layout
            .channels()
            .iter()
            .map(|definition| ChannelStorage::uniform(definition.default_value))
            .collect();
        for (channel, mut storage) in self.channels.iter().zip(source_storages) {
            if let Some(content) = &channel.content {
                Self::check_in_table(channel, content, storage.max_value())?;
                if let Some(mapping) = &content.mapping {
                    storage.remap_values(|value| mapping[value as usize]);
                }
            }
            storages[channel.layout_index.get()] = storage;
        }
        Ok(Brick::from_channels(storages))
    }

    /// The loaded world's channel and value for a value the source wrote in its channel number
    /// `source_channel`.
    pub fn translate_value(
        &self,
        source_channel: usize,
        value: u32,
    ) -> Result<(ChannelIndex, u32), TranslationError> {
        let channel = self
            .channels
            .get(source_channel)
            .ok_or(TranslationError::ChannelOutsideTable(source_channel))?;
        let translated = match &channel.content {
            None => value,
            Some(content) => {
                Self::check_in_table(channel, content, value)?;
                content
                    .mapping
                    .as_ref()
                    .map_or(value, |mapping| mapping[value as usize])
            }
        };
        Ok((channel.layout_index, translated))
    }

    pub fn translate_generator(&self, source_id: u32) -> Result<GeneratorId, TranslationError> {
        self.generators
            .get(source_id as usize)
            .copied()
            .ok_or(TranslationError::GeneratorOutsideTable(source_id))
    }

    fn check_in_table(
        channel: &SourceChannel,
        content: &ContentValues,
        largest_value: u32,
    ) -> Result<(), TranslationError> {
        if largest_value as usize >= content.source_entry_count {
            return Err(TranslationError::ValueOutsideRegistry {
                channel: channel.identifier.clone(),
                registry: content.registry.clone(),
                value: largest_value,
                entries: content.source_entry_count,
            });
        }
        Ok(())
    }
}
