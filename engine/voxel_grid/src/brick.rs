use voxel_space::LocalVoxelIndex;

use crate::channel_storage::ChannelStorage;
use crate::digest::Fnv1a64;
use crate::layout::{ChannelIndex, GridLayout};

/// A cube of [`BRICK_VOXEL_COUNT`](voxel_space::BRICK_VOXEL_COUNT) voxels: one
/// [`ChannelStorage`] per channel of the layout it was made for.
///
/// Channel indices are only meaningful for that layout; passing one from another layout is a bug
/// and panics (or, if the other layout is longer, reads the wrong channel).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Brick {
    channels: Vec<ChannelStorage>,
}

impl Brick {
    /// Every channel uniform at its default value.
    pub fn new(layout: &GridLayout) -> Self {
        Self {
            channels: layout
                .channels()
                .iter()
                .map(|definition| ChannelStorage::uniform(definition.default_value))
                .collect(),
        }
    }

    /// One storage per channel, in layout order. The grid checks the count when the brick is
    /// inserted.
    pub fn from_channels(channels: Vec<ChannelStorage>) -> Self {
        Self { channels }
    }

    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    pub fn channel(&self, channel: ChannelIndex) -> &ChannelStorage {
        &self.channels[channel.get()]
    }

    pub fn channel_mut(&mut self, channel: ChannelIndex) -> &mut ChannelStorage {
        &mut self.channels[channel.get()]
    }

    pub fn get(&self, channel: ChannelIndex, voxel: LocalVoxelIndex) -> u32 {
        self.channel(channel).get(voxel)
    }

    /// Returns whether the stored value changed.
    pub fn set(&mut self, channel: ChannelIndex, voxel: LocalVoxelIndex, value: u32) -> bool {
        self.channel_mut(channel).set(voxel, value)
    }

    /// A 64-bit digest of every channel: FNV-1a over the channel count (4 little-endian bytes)
    /// followed by each channel's [`ChannelStorage::content_digest`] (8 little-endian bytes), in
    /// channel order. Only meaningful between bricks of the same layout; stable like the channel
    /// digest.
    pub fn content_digest(&self) -> u64 {
        let mut hasher = Fnv1a64::new();
        hasher.write(&(self.channels.len() as u32).to_le_bytes());
        for channel in &self.channels {
            hasher.write(&channel.content_digest().to_le_bytes());
        }
        hasher.finish()
    }

    /// Compacts every channel. See [`ChannelStorage::compact`].
    pub fn compact(&mut self) {
        for channel in &mut self.channels {
            channel.compact();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel_storage::StorageKind;
    use crate::test_support::{MATERIAL_DEFAULT, StandardTestLayout, TEMPERATURE_DEFAULT};

    fn voxel(x: u8, y: u8, z: u8) -> LocalVoxelIndex {
        LocalVoxelIndex::new(x, y, z)
    }

    #[test]
    fn a_new_brick_holds_every_channel_at_its_default() {
        let fixture = StandardTestLayout::default();
        let brick = Brick::new(&fixture.layout);

        assert_eq!(brick.channel_count(), 2);
        assert_eq!(
            brick.get(fixture.material, voxel(3, 4, 5)),
            MATERIAL_DEFAULT
        );
        assert_eq!(
            brick.get(fixture.temperature, voxel(31, 31, 31)),
            TEMPERATURE_DEFAULT
        );
        assert_eq!(brick.channel(fixture.material).kind(), StorageKind::Uniform);
        assert_eq!(
            brick.channel(fixture.temperature).uniform_value(),
            Some(TEMPERATURE_DEFAULT)
        );
    }

    #[test]
    fn channels_are_independent() {
        let fixture = StandardTestLayout::default();
        let mut brick = Brick::new(&fixture.layout);

        assert!(brick.set(fixture.material, voxel(1, 2, 3), 9));

        assert_eq!(brick.get(fixture.material, voxel(1, 2, 3)), 9);
        assert_eq!(
            brick.get(fixture.temperature, voxel(1, 2, 3)),
            TEMPERATURE_DEFAULT
        );
        assert_eq!(
            brick.get(fixture.material, voxel(1, 2, 4)),
            MATERIAL_DEFAULT
        );
    }

    #[test]
    fn set_reports_whether_the_value_changed() {
        let fixture = StandardTestLayout::default();
        let mut brick = Brick::new(&fixture.layout);
        assert!(!brick.set(fixture.material, voxel(0, 0, 0), MATERIAL_DEFAULT));
        assert!(brick.set(fixture.material, voxel(0, 0, 0), 4));
        assert!(!brick.set(fixture.material, voxel(0, 0, 0), 4));
    }

    #[test]
    fn compact_restores_minimal_storage_in_every_channel() {
        let fixture = StandardTestLayout::default();
        let mut brick = Brick::new(&fixture.layout);
        brick.set(fixture.material, voxel(0, 0, 0), 4);
        brick.set(fixture.material, voxel(0, 0, 0), MATERIAL_DEFAULT);
        brick.set(fixture.temperature, voxel(7, 7, 7), 99);
        brick.set(fixture.temperature, voxel(7, 7, 7), TEMPERATURE_DEFAULT);
        assert_ne!(brick.channel(fixture.material).kind(), StorageKind::Uniform);

        brick.compact();

        assert_eq!(brick.channel(fixture.material).kind(), StorageKind::Uniform);
        assert_eq!(
            brick.channel(fixture.temperature).kind(),
            StorageKind::Uniform
        );
    }

    #[test]
    fn content_digest_covers_every_channel_and_the_channel_count() {
        let fixture = StandardTestLayout::default();
        let untouched = Brick::new(&fixture.layout);
        let mut material_edited = untouched.clone();
        material_edited.set(fixture.material, voxel(0, 0, 0), 1);
        let mut temperature_edited = untouched.clone();
        temperature_edited.set(fixture.temperature, voxel(0, 0, 0), 1);

        let digests = [
            untouched.content_digest(),
            material_edited.content_digest(),
            temperature_edited.content_digest(),
        ];
        assert_ne!(digests[0], digests[1]);
        assert_ne!(digests[0], digests[2]);
        assert_ne!(digests[1], digests[2]);

        let mut compacted = material_edited.clone();
        compacted.compact();
        assert_eq!(compacted.content_digest(), digests[1]);

        let fewer_channels = Brick::from_channels(vec![ChannelStorage::uniform(0)]);
        assert_ne!(fewer_channels.content_digest(), untouched.content_digest());
    }

    #[test]
    fn bricks_are_equal_when_every_channel_holds_equal_values() {
        let fixture = StandardTestLayout::default();
        let mut left = Brick::new(&fixture.layout);
        let mut right = Brick::new(&fixture.layout);
        assert_eq!(left, right);

        left.set(fixture.temperature, voxel(1, 1, 1), 50);
        assert_ne!(left, right);
        right.set(fixture.temperature, voxel(1, 1, 1), 50);
        assert_eq!(left, right);
    }
}
