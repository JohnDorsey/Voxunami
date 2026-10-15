//! Fixtures for tests of code that uses grids. Enabled by the `test-support` feature.

use std::sync::Arc;

use crate::{ChannelDefinition, ChannelIndex, GridLayout};

pub const MATERIAL_DEFAULT: u32 = 0;
pub const TEMPERATURE_DEFAULT: u32 = 20;

/// A two-channel layout: `test_content:material` (default 0) and `test_content:temperature`
/// (default 20). The differing defaults catch code that assumes every default is zero.
#[derive(Clone, Debug)]
pub struct StandardTestLayout {
    pub layout: Arc<GridLayout>,
    pub material: ChannelIndex,
    pub temperature: ChannelIndex,
}

impl Default for StandardTestLayout {
    fn default() -> Self {
        let definition = |text: &str, default_value| {
            ChannelDefinition::number(
                text.parse().expect("fixture identifiers are valid"),
                default_value,
            )
        };
        let layout = GridLayout::new(vec![
            definition("test_content:material", MATERIAL_DEFAULT),
            definition("test_content:temperature", TEMPERATURE_DEFAULT),
        ])
        .expect("fixture channels are distinct");
        let index_of = |text: &str| {
            layout
                .channel_index(&text.parse().expect("fixture identifiers are valid"))
                .expect("fixture channel exists")
        };
        Self {
            material: index_of("test_content:material"),
            temperature: index_of("test_content:temperature"),
            layout: Arc::new(layout),
        }
    }
}
