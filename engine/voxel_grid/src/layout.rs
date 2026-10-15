use namespaced_id::NamespacedId;
use rustc_hash::FxHashMap;

/// Most channels one layout can define.
pub const MAXIMUM_CHANNEL_COUNT: usize = 1 << 16;

/// Selects one channel of a [`GridLayout`]. Only a layout hands these out; using an index with a
/// grid or brick of a different layout is a bug.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChannelIndex(u16);

impl ChannelIndex {
    /// The position of the channel in its layout.
    pub fn get(self) -> usize {
        usize::from(self.0)
    }
}

/// A named per-voxel value and what an untouched voxel holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelDefinition {
    pub identifier: NamespacedId,
    pub default_value: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum GridLayoutError {
    #[error("channel {0} is defined more than once")]
    DuplicateChannel(NamespacedId),
    #[error("a layout can define at most {maximum} channels, got {count}")]
    TooManyChannels { count: usize, maximum: usize },
}

/// The ordered list of channels shared by a grid and all of its bricks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridLayout {
    channels: Vec<ChannelDefinition>,
    index_by_identifier: FxHashMap<NamespacedId, ChannelIndex>,
}

impl GridLayout {
    pub fn new(channels: Vec<ChannelDefinition>) -> Result<Self, GridLayoutError> {
        if channels.len() > MAXIMUM_CHANNEL_COUNT {
            return Err(GridLayoutError::TooManyChannels {
                count: channels.len(),
                maximum: MAXIMUM_CHANNEL_COUNT,
            });
        }
        let mut index_by_identifier = FxHashMap::default();
        for (position, definition) in channels.iter().enumerate() {
            let index = ChannelIndex(position as u16);
            if index_by_identifier
                .insert(definition.identifier.clone(), index)
                .is_some()
            {
                return Err(GridLayoutError::DuplicateChannel(
                    definition.identifier.clone(),
                ));
            }
        }
        Ok(Self {
            channels,
            index_by_identifier,
        })
    }

    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    /// Definitions in channel index order.
    pub fn channels(&self) -> &[ChannelDefinition] {
        &self.channels
    }

    pub fn channel_indices(&self) -> impl ExactSizeIterator<Item = ChannelIndex> + use<> {
        (0..self.channels.len() as u32).map(|position| ChannelIndex(position as u16))
    }

    pub fn channel_index(&self, identifier: &NamespacedId) -> Option<ChannelIndex> {
        self.index_by_identifier.get(identifier).copied()
    }

    /// Panics if `index` does not belong to this layout.
    pub fn definition(&self, index: ChannelIndex) -> &ChannelDefinition {
        &self.channels[index.get()]
    }

    /// Panics if `index` does not belong to this layout.
    pub fn default_value(&self, index: ChannelIndex) -> u32 {
        self.definition(index).default_value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn definition(text: &str, default_value: u32) -> ChannelDefinition {
        ChannelDefinition {
            identifier: text.parse().expect("test identifiers are valid"),
            default_value,
        }
    }

    #[test]
    fn channel_indices_follow_definition_order() {
        let layout = GridLayout::new(vec![
            definition("test:material", 0),
            definition("test:temperature", 20),
            definition("test:moisture", 5),
        ])
        .unwrap();

        assert_eq!(layout.channel_count(), 3);
        let by_name = |text: &str| layout.channel_index(&text.parse().unwrap()).unwrap().get();
        assert_eq!(by_name("test:material"), 0);
        assert_eq!(by_name("test:temperature"), 1);
        assert_eq!(by_name("test:moisture"), 2);
        assert_eq!(layout.channel_index(&"test:unknown".parse().unwrap()), None);
    }

    #[test]
    fn definitions_and_defaults_are_retrievable_by_index() {
        let layout = GridLayout::new(vec![
            definition("test:material", 0),
            definition("test:temperature", 20),
        ])
        .unwrap();
        let temperature = layout
            .channel_index(&"test:temperature".parse().unwrap())
            .unwrap();
        assert_eq!(layout.default_value(temperature), 20);
        assert_eq!(
            layout.definition(temperature).identifier.as_str(),
            "test:temperature"
        );
    }

    #[test]
    fn channel_indices_enumerates_every_channel_in_order() {
        let layout =
            GridLayout::new(vec![definition("test:a", 0), definition("test:b", 0)]).unwrap();
        let listed: Vec<usize> = layout.channel_indices().map(ChannelIndex::get).collect();
        assert_eq!(listed, [0, 1]);
    }

    #[test]
    fn duplicate_channels_are_rejected() {
        let result = GridLayout::new(vec![
            definition("test:material", 0),
            definition("test:other", 0),
            definition("test:material", 9),
        ]);
        assert_eq!(
            result,
            Err(GridLayoutError::DuplicateChannel(
                "test:material".parse().unwrap()
            ))
        );
    }

    #[test]
    fn an_empty_layout_is_allowed() {
        assert_eq!(GridLayout::new(Vec::new()).unwrap().channel_count(), 0);
    }

    #[test]
    fn the_channel_limit_is_enforced() {
        let definitions = |count: usize| -> Vec<ChannelDefinition> {
            (0..count)
                .map(|number| definition(&format!("test:channel_{number}"), 0))
                .collect()
        };
        assert!(GridLayout::new(definitions(MAXIMUM_CHANNEL_COUNT)).is_ok());
        assert_eq!(
            GridLayout::new(definitions(MAXIMUM_CHANNEL_COUNT + 1)),
            Err(GridLayoutError::TooManyChannels {
                count: MAXIMUM_CHANNEL_COUNT + 1,
                maximum: MAXIMUM_CHANNEL_COUNT
            })
        );
    }
}
