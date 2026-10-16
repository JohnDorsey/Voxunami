//! The part of the light that the voxels do not show, saved with the world: the voxels whose light
//! the rule still has to look at (see [`crate::lighting`]). A world saved while light is still
//! spreading carries on spreading when it is loaded.
//!
//! Levels are named by identifier, since level numbers belong to one running server.

use binary_codec::{ByteReader, ByteWriter, DecodeError};
use namespaced_id::NamespacedId;
use voxel_space::VoxelCoordinate;
use world_storage::Section;

/// The version of the saved light this program writes.
const VERSION: u32 = 1;

pub fn section_id() -> NamespacedId {
    "voxunami_cave:light"
        .parse()
        .expect("constant identifier is valid")
}

/// A voxel the light rule still has to look at, and the level it is in.
pub type PendingVoxel = (NamespacedId, VoxelCoordinate);

/// The section to save for these voxels, which must be in the order the rule will look at them
/// (the order is kept).
pub fn section(pending: &[PendingVoxel]) -> Section {
    // Each level is named once, and the voxels refer to it by position.
    let mut levels: Vec<&NamespacedId> = Vec::new();
    for (level, _) in pending {
        if !levels.contains(&level) {
            levels.push(level);
        }
    }
    let mut writer = ByteWriter::new();
    writer.write_varint(levels.len() as u64);
    for level in &levels {
        writer.write_identifier(level);
    }
    writer.write_varint(pending.len() as u64);
    for (level, voxel) in pending {
        let position = levels
            .iter()
            .position(|known| *known == level)
            .expect("every level was listed");
        writer.write_varint(position as u64);
        for component in [voxel.x(), voxel.y(), voxel.z()] {
            writer.write_signed_varint(component);
        }
    }
    Section {
        identifier: section_id(),
        version: VERSION,
        payload: writer.into_bytes(),
    }
}

fn read(payload: &[u8]) -> Result<Vec<PendingVoxel>, DecodeError> {
    let mut reader = ByteReader::new(payload);
    // A level's name takes at least three bytes.
    let level_count = reader.read_count(3)?;
    let mut levels = Vec::with_capacity(level_count);
    for _ in 0..level_count {
        levels.push(reader.read_identifier()?);
    }
    // A voxel takes a byte for its level and at least three for its position.
    let count = reader.read_count(4)?;
    let mut pending = Vec::with_capacity(count);
    for _ in 0..count {
        let position = usize::try_from(reader.read_varint()?)
            .map_err(|_| DecodeError::Invalid("no such level"))?;
        let level = levels
            .get(position)
            .ok_or(DecodeError::Invalid("no such level"))?
            .clone();
        let voxel = VoxelCoordinate::try_new(
            reader.read_signed_varint()?,
            reader.read_signed_varint()?,
            reader.read_signed_varint()?,
        )
        .map_err(|_| DecodeError::Invalid("a voxel coordinate is out of range"))?;
        pending.push((level, voxel));
    }
    reader.finish()?;
    Ok(pending)
}

/// The voxels still to be looked at in a world's sections, if the world has some this program can
/// read. A newer version of the section is left alone (and so kept in the file) and counts as none.
pub fn read_pending(sections: &[Section]) -> Option<Vec<PendingVoxel>> {
    let section = sections
        .iter()
        .find(|section| section.identifier == section_id() && section.version == VERSION)?;
    read(&section.payload).ok()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn level(name: &str) -> NamespacedId {
        format!("voxunami_cave:{name}").parse().unwrap()
    }

    fn voxel(x: i64, y: i64, z: i64) -> VoxelCoordinate {
        VoxelCoordinate::new(x, y, z)
    }

    #[test]
    fn pending_voxels_survive_saving_in_the_order_they_were_in() {
        let pending = vec![
            (level("overworld"), voxel(3, -4, 5)),
            (level("cellar"), voxel(-1_000_000_000_000, 7, 0)),
            (level("overworld"), voxel(0, 0, 0)),
            (level("overworld"), voxel(3, -4, 6)),
        ];
        let section = section(&pending);
        assert_eq!(section.identifier, section_id());
        assert_eq!(read_pending(std::slice::from_ref(&section)), Some(pending));
    }

    #[test]
    fn nothing_pending_is_a_small_section_that_reads_back_as_nothing() {
        let section = section(&[]);
        assert_eq!(section.payload.len(), 2);
        assert_eq!(read_pending(&[section]), Some(Vec::new()));
        assert_eq!(read_pending(&[]), None);
    }

    #[test]
    fn a_newer_or_damaged_section_counts_as_none() {
        let good = section(&[(level("overworld"), voxel(1, 2, 3))]);
        let newer = Section {
            version: VERSION + 1,
            ..good.clone()
        };
        assert_eq!(read_pending(&[newer]), None);
        for length in 0..good.payload.len() {
            let damaged = Section {
                payload: good.payload[..length].to_vec(),
                ..good.clone()
            };
            assert_eq!(read_pending(&[damaged]), None, "{length}");
        }
        let mut trailing = good.clone();
        trailing.payload.push(0);
        assert_eq!(read_pending(&[trailing]), None);
        // A voxel naming a level that is not in the list.
        let mut bad_level = good.clone();
        let level_byte = bad_level.payload.len() - 4;
        bad_level.payload[level_byte] = 5;
        assert_eq!(read_pending(&[bad_level]), None);
    }

    proptest! {
        #[test]
        fn decoding_arbitrary_bytes_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..80)) {
            let _ = read_pending(&[Section {
                identifier: section_id(),
                version: VERSION,
                payload: bytes,
            }]);
        }

        #[test]
        fn any_pending_voxels_round_trip(
            voxels in proptest::collection::vec((0usize..3, -1000i64..1000, -1000i64..1000, -1000i64..1000), 0..40)
        ) {
            let pending: Vec<PendingVoxel> = voxels
                .into_iter()
                .map(|(level_number, x, y, z)| (level(&format!("level_{level_number}")), voxel(x, y, z)))
                .collect();
            prop_assert_eq!(read_pending(&[section(&pending)]), Some(pending));
        }
    }
}
