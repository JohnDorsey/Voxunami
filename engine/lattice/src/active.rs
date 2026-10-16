use std::collections::BTreeSet;

use binary_codec::{ByteReader, ByteWriter, DecodeError};
use namespaced_id::NamespacedId;
use voxel_space::{FaceDirection, LevelIndex, VoxelCoordinate};
use world_storage::Section;

/// The version of the saved form this program writes.
const SAVED_VERSION: u32 = 1;

/// The voxels a rule still has to look at, in a fixed order (by level, then by position), so that
/// what a rule does with them never depends on how they got there.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActiveVoxels {
    voxels: BTreeSet<(LevelIndex, VoxelCoordinate)>,
}

impl ActiveVoxels {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a voxel; does nothing if it is there already.
    pub fn insert(&mut self, level: LevelIndex, voxel: VoxelCoordinate) {
        self.voxels.insert((level, voxel));
    }

    /// Adds a voxel and the six voxels around it: the ones whose next value may depend on it.
    pub fn insert_with_neighbours(&mut self, level: LevelIndex, voxel: VoxelCoordinate) {
        self.insert(level, voxel);
        for face in FaceDirection::ALL {
            let [dx, dy, dz] = face.offset();
            if let Some(neighbour) = voxel.checked_offset(dx, dy, dz) {
                self.insert(level, neighbour);
            }
        }
    }

    pub fn remove(&mut self, level: LevelIndex, voxel: VoxelCoordinate) {
        self.voxels.remove(&(level, voxel));
    }

    pub fn contains(&self, level: LevelIndex, voxel: VoxelCoordinate) -> bool {
        self.voxels.contains(&(level, voxel))
    }

    pub fn len(&self) -> usize {
        self.voxels.len()
    }

    pub fn is_empty(&self) -> bool {
        self.voxels.is_empty()
    }

    /// The voxels, in their fixed order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (LevelIndex, VoxelCoordinate)> + '_ {
        self.voxels.iter().copied()
    }

    /// Empties the set and returns what was in it, in order: a rule takes the voxels it is going to
    /// look at this tick, and puts back (or adds) the ones it will look at next.
    pub fn take(&mut self) -> Vec<(LevelIndex, VoxelCoordinate)> {
        std::mem::take(&mut self.voxels).into_iter().collect()
    }

    /// Writes these voxels. Levels are named by `level_ids`, in level order, since level numbers
    /// belong to one running server; voxels of a level that is not listed are not written.
    pub fn write_to(&self, writer: &mut ByteWriter, level_ids: &[NamespacedId]) {
        let named: Vec<(&NamespacedId, VoxelCoordinate)> = self
            .voxels
            .iter()
            .filter_map(|&(level, voxel)| Some((level_ids.get(level.get() as usize)?, voxel)))
            .collect();
        // Each level that has voxels is named once, and the voxels refer to it by position.
        let mut levels: Vec<&NamespacedId> = Vec::new();
        for (name, _) in &named {
            if !levels.contains(name) {
                levels.push(name);
            }
        }
        writer.write_varint(levels.len() as u64);
        for level in &levels {
            writer.write_identifier(level);
        }
        writer.write_varint(named.len() as u64);
        for (name, voxel) in &named {
            let position = levels
                .iter()
                .position(|known| known == name)
                .expect("every level was listed");
            writer.write_varint(position as u64);
            for component in [voxel.x(), voxel.y(), voxel.z()] {
                writer.write_signed_varint(component);
            }
        }
    }

    /// Reads voxels written by [`ActiveVoxels::write_to`]; `level_index` gives the level a name
    /// stands for in this world, and voxels of a level it has no answer for are dropped.
    pub fn read_from(
        reader: &mut ByteReader<'_>,
        level_index: &impl Fn(&NamespacedId) -> Option<LevelIndex>,
    ) -> Result<Self, DecodeError> {
        // A level's name takes at least three bytes.
        let level_count = reader.read_count(3)?;
        let mut levels = Vec::with_capacity(level_count);
        for _ in 0..level_count {
            levels.push(reader.read_identifier()?);
        }
        // A voxel takes a byte for its level and at least three for its position.
        let count = reader.read_count(4)?;
        let mut active = Self::new();
        for _ in 0..count {
            let position = usize::try_from(reader.read_varint()?)
                .map_err(|_| DecodeError::Invalid("no such level"))?;
            let name = levels
                .get(position)
                .ok_or(DecodeError::Invalid("no such level"))?;
            let voxel = VoxelCoordinate::try_new(
                reader.read_signed_varint()?,
                reader.read_signed_varint()?,
                reader.read_signed_varint()?,
            )
            .map_err(|_| DecodeError::Invalid("a voxel coordinate is out of range"))?;
            // A voxel of a level this world no longer has is not looked at.
            if let Some(level) = level_index(name) {
                active.insert(level, voxel);
            }
        }
        Ok(active)
    }

    /// The section to save these voxels alone in; see [`ActiveVoxels::write_to`].
    pub fn to_section(&self, identifier: &NamespacedId, level_ids: &[NamespacedId]) -> Section {
        let mut writer = ByteWriter::new();
        self.write_to(&mut writer, level_ids);
        Section {
            identifier: identifier.clone(),
            version: SAVED_VERSION,
            payload: writer.into_bytes(),
        }
    }

    /// The voxels saved in the section called `identifier` among a world's sections, if there is
    /// one this program can read; `level_index` gives the level a name stands for in this world. A
    /// newer version of the section is left alone (and so kept in the file) and counts as none.
    pub fn from_sections(
        sections: &[Section],
        identifier: &NamespacedId,
        level_index: impl Fn(&NamespacedId) -> Option<LevelIndex>,
    ) -> Option<Self> {
        let section = sections.iter().find(|section| {
            section.identifier == *identifier && section.version == SAVED_VERSION
        })?;
        let mut reader = ByteReader::new(&section.payload);
        let active = Self::read_from(&mut reader, &level_index).ok()?;
        reader.finish().ok()?;
        Some(active)
    }
}

impl FromIterator<(LevelIndex, VoxelCoordinate)> for ActiveVoxels {
    fn from_iter<I: IntoIterator<Item = (LevelIndex, VoxelCoordinate)>>(iterator: I) -> Self {
        Self {
            voxels: iterator.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn name(text: &str) -> NamespacedId {
        format!("test_game:{text}").parse().unwrap()
    }

    fn voxel(x: i64, y: i64, z: i64) -> VoxelCoordinate {
        VoxelCoordinate::new(x, y, z)
    }

    fn levels() -> Vec<NamespacedId> {
        vec![name("overworld"), name("cellar")]
    }

    fn index_of(identifier: &NamespacedId) -> Option<LevelIndex> {
        levels()
            .iter()
            .position(|known| known == identifier)
            .map(|position| LevelIndex::new(position as u32))
    }

    const MAIN: LevelIndex = LevelIndex::MAIN;
    const SECOND: LevelIndex = LevelIndex::new(1);

    #[test]
    fn voxels_are_kept_once_and_in_order_of_level_then_position() {
        let mut active = ActiveVoxels::new();
        assert!(active.is_empty());
        active.insert(SECOND, voxel(0, 0, 0));
        active.insert(MAIN, voxel(5, 0, 0));
        active.insert(MAIN, voxel(-1, 9, 9));
        active.insert(MAIN, voxel(5, 0, 0));
        assert_eq!(active.len(), 3);
        assert!(active.contains(MAIN, voxel(5, 0, 0)));
        assert!(!active.contains(SECOND, voxel(5, 0, 0)));
        let order: Vec<_> = active.iter().collect();
        assert_eq!(
            order,
            [
                (MAIN, voxel(-1, 9, 9)),
                (MAIN, voxel(5, 0, 0)),
                (SECOND, voxel(0, 0, 0))
            ]
        );
        active.remove(MAIN, voxel(5, 0, 0));
        assert_eq!(active.len(), 2);
        assert_eq!(
            active.take(),
            [(MAIN, voxel(-1, 9, 9)), (SECOND, voxel(0, 0, 0))]
        );
        assert!(active.is_empty());
    }

    #[test]
    fn a_voxel_with_its_neighbours_is_seven_voxels() {
        let mut active = ActiveVoxels::new();
        active.insert_with_neighbours(MAIN, voxel(1, 1, 1));
        assert_eq!(active.len(), 7);
        for (x, y, z) in [
            (1, 1, 1),
            (0, 1, 1),
            (2, 1, 1),
            (1, 0, 1),
            (1, 2, 1),
            (1, 1, 0),
            (1, 1, 2),
        ] {
            assert!(active.contains(MAIN, voxel(x, y, z)), "{x} {y} {z}");
        }
    }

    #[test]
    fn voxels_survive_saving_by_level_name() {
        let identifier = name("pending");
        let active: ActiveVoxels = [
            (MAIN, voxel(3, -4, 5)),
            (SECOND, voxel(-1_000_000_000_000, 7, 0)),
            (MAIN, voxel(0, 0, 0)),
        ]
        .into_iter()
        .collect();
        let section = active.to_section(&identifier, &levels());
        assert_eq!(section.identifier, identifier);
        assert_eq!(
            ActiveVoxels::from_sections(std::slice::from_ref(&section), &identifier, index_of),
            Some(active.clone())
        );
        // Another section of the world, with another name, is not it.
        assert_eq!(
            ActiveVoxels::from_sections(&[section], &name("other"), index_of),
            None
        );
    }

    #[test]
    fn voxels_of_a_level_the_world_does_not_have_are_dropped_when_loading_and_saving() {
        let identifier = name("pending");
        let active: ActiveVoxels = [(MAIN, voxel(1, 2, 3)), (SECOND, voxel(4, 5, 6))]
            .into_iter()
            .collect();
        // The second level is not named when saving...
        let only_main = vec![name("overworld")];
        let section = active.to_section(&identifier, &only_main);
        let all_levels =
            ActiveVoxels::from_sections(std::slice::from_ref(&section), &identifier, index_of);
        assert_eq!(
            all_levels,
            Some([(MAIN, voxel(1, 2, 3))].into_iter().collect())
        );
        // ...and one the loading world lacks is not restored.
        let full = active.to_section(&identifier, &levels());
        let loaded = ActiveVoxels::from_sections(&[full], &identifier, |id| {
            (*id == name("overworld")).then_some(MAIN)
        });
        assert_eq!(loaded, Some([(MAIN, voxel(1, 2, 3))].into_iter().collect()));
    }

    #[test]
    fn nothing_active_is_a_small_section_that_reads_back_as_nothing() {
        let identifier = name("pending");
        let section = ActiveVoxels::new().to_section(&identifier, &levels());
        assert_eq!(section.payload.len(), 2);
        assert_eq!(
            ActiveVoxels::from_sections(&[section], &identifier, index_of),
            Some(ActiveVoxels::new())
        );
        assert_eq!(
            ActiveVoxels::from_sections(&[], &identifier, index_of),
            None
        );
    }

    #[test]
    fn a_newer_or_damaged_section_counts_as_none() {
        let identifier = name("pending");
        let good = [(MAIN, voxel(1, 2, 3))]
            .into_iter()
            .collect::<ActiveVoxels>()
            .to_section(&identifier, &levels());
        let read =
            |section: Section| ActiveVoxels::from_sections(&[section], &identifier, index_of);
        assert!(read(good.clone()).is_some());
        assert_eq!(
            read(Section {
                version: SAVED_VERSION + 1,
                ..good.clone()
            }),
            None
        );
        for length in 0..good.payload.len() {
            let damaged = Section {
                payload: good.payload[..length].to_vec(),
                ..good.clone()
            };
            assert_eq!(read(damaged), None, "{length}");
        }
        let mut trailing = good.clone();
        trailing.payload.push(0);
        assert_eq!(read(trailing), None);
        // A voxel naming a level that is not in the list.
        let mut bad_level = good.clone();
        let level_byte = bad_level.payload.len() - 4;
        bad_level.payload[level_byte] = 5;
        assert_eq!(read(bad_level), None);
    }

    proptest! {
        #[test]
        fn decoding_arbitrary_bytes_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..80)) {
            let identifier = name("pending");
            let _ = ActiveVoxels::from_sections(
                &[Section { identifier: identifier.clone(), version: SAVED_VERSION, payload: bytes }],
                &identifier,
                index_of,
            );
        }

        #[test]
        fn any_active_voxels_round_trip(
            voxels in proptest::collection::vec((0u32..2, -1000i64..1000, -1000i64..1000, -1000i64..1000), 0..40)
        ) {
            let identifier = name("pending");
            let active: ActiveVoxels = voxels
                .into_iter()
                .map(|(level, x, y, z)| (LevelIndex::new(level), voxel(x, y, z)))
                .collect();
            let section = active.to_section(&identifier, &levels());
            prop_assert_eq!(ActiveVoxels::from_sections(&[section], &identifier, index_of), Some(active));
        }
    }
}
