use std::fmt;

use voxel_space::{BRICK_VOXEL_COUNT, LocalVoxelIndex};

use crate::digest::Fnv1a64;
use crate::packed_indices::PackedIndices;

/// The largest palette that 8-bit indices can address; beyond it a channel is stored densely.
const MAXIMUM_PALETTE_LENGTH: usize = 256;

#[derive(Clone)]
enum Representation {
    Uniform(u32),
    Paletted {
        /// Distinct values, in no particular order. Entries may be stale (no voxel uses them)
        /// until the next [`ChannelStorage::compact`].
        palette: Vec<u32>,
        indices: PackedIndices,
    },
    Dense(Box<[u32]>),
}

/// How a [`ChannelStorage`] currently holds its values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageKind {
    Uniform,
    Paletted {
        palette_length: usize,
        bits_per_index: u8,
    },
    Dense,
}

/// The values of one channel for every voxel of a brick, stored compactly.
///
/// There are three representations, switched automatically:
/// - *uniform*: every voxel has the same value, so there is no per-voxel storage at all;
/// - *paletted*: a short list of distinct values plus a 1, 2, 4 or 8-bit index per voxel;
/// - *dense*: one `u32` per voxel, once a channel needs more than 256 distinct values.
///
/// Writes upgrade the representation when they must but never downgrade it, since that would
/// cost a scan per write. Call [`ChannelStorage::compact`] after a batch of edits to get back to
/// the minimal form. Equality compares voxel values, never representations.
#[derive(Clone)]
pub struct ChannelStorage {
    representation: Representation,
}

impl ChannelStorage {
    /// Every voxel holds `value`.
    pub fn uniform(value: u32) -> Self {
        Self {
            representation: Representation::Uniform(value),
        }
    }

    /// Builds storage in minimal form from one value per voxel, in [`LocalVoxelIndex`] order.
    ///
    /// Panics unless `values.len() == BRICK_VOXEL_COUNT`. The palette lists values in order of
    /// first occurrence, so the result is canonical: equal contents give an identical layout.
    pub fn from_values(values: &[u32]) -> Self {
        assert_eq!(
            values.len(),
            BRICK_VOXEL_COUNT,
            "a brick channel needs exactly one value per voxel"
        );
        let mut palette: Vec<u32> = Vec::new();
        let mut slots: Vec<u8> = Vec::with_capacity(BRICK_VOXEL_COUNT);
        for &value in values {
            let slot = match palette.iter().position(|&candidate| candidate == value) {
                Some(slot) => slot,
                None if palette.len() == MAXIMUM_PALETTE_LENGTH => {
                    return Self {
                        representation: Representation::Dense(values.into()),
                    };
                }
                None => {
                    palette.push(value);
                    palette.len() - 1
                }
            };
            slots.push(slot as u8);
        }
        if let [only_value] = palette[..] {
            return Self::uniform(only_value);
        }
        let mut indices = PackedIndices::new(bits_per_index_for(palette.len()));
        for (index, &slot) in slots.iter().enumerate() {
            indices.set(index, usize::from(slot));
        }
        Self {
            representation: Representation::Paletted { palette, indices },
        }
    }

    pub fn get(&self, index: LocalVoxelIndex) -> u32 {
        match &self.representation {
            Representation::Uniform(value) => *value,
            Representation::Paletted { palette, indices } => palette[indices.get(index.index())],
            Representation::Dense(values) => values[index.index()],
        }
    }

    /// Stores `value`. Returns whether that changed the stored value.
    pub fn set(&mut self, index: LocalVoxelIndex, value: u32) -> bool {
        match &mut self.representation {
            Representation::Uniform(current) => {
                let previous = *current;
                if previous == value {
                    return false;
                }
                let mut indices = PackedIndices::new(1);
                indices.set(index.index(), 1);
                self.representation = Representation::Paletted {
                    palette: vec![previous, value],
                    indices,
                };
                true
            }
            Representation::Paletted { palette, indices } => {
                if let Some(slot) = palette.iter().position(|&candidate| candidate == value) {
                    if indices.get(index.index()) == slot {
                        return false;
                    }
                    indices.set(index.index(), slot);
                    return true;
                }
                if palette.len() == MAXIMUM_PALETTE_LENGTH {
                    let mut values: Box<[u32]> = (0..BRICK_VOXEL_COUNT)
                        .map(|position| palette[indices.get(position)])
                        .collect();
                    values[index.index()] = value;
                    self.representation = Representation::Dense(values);
                    return true;
                }
                palette.push(value);
                let required_bits = bits_per_index_for(palette.len());
                if required_bits > indices.bits_per_index() {
                    *indices = indices.repacked(required_bits);
                }
                indices.set(index.index(), palette.len() - 1);
                true
            }
            Representation::Dense(values) => {
                let slot = &mut values[index.index()];
                if *slot == value {
                    return false;
                }
                *slot = value;
                true
            }
        }
    }

    /// Makes every voxel hold `value`, releasing any per-voxel storage.
    pub fn fill(&mut self, value: u32) {
        self.representation = Representation::Uniform(value);
    }

    /// `Some(value)` if every voxel holds `value`, whatever the representation.
    pub fn uniform_value(&self) -> Option<u32> {
        match &self.representation {
            Representation::Uniform(value) => Some(*value),
            Representation::Paletted { palette, indices } => {
                indices.all_slots_equal().then(|| palette[indices.get(0)])
            }
            Representation::Dense(values) => {
                let first = values[0];
                values.iter().all(|&value| value == first).then_some(first)
            }
        }
    }

    /// All values in [`LocalVoxelIndex`] order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = u32> + '_ {
        LocalVoxelIndex::all().map(|index| self.get(index))
    }

    /// Rebuilds the minimal representation: drops unused palette entries, narrows the index
    /// width, and collapses to uniform or from dense to paletted where possible. Voxel values do
    /// not change.
    pub fn compact(&mut self) {
        if matches!(self.representation, Representation::Uniform(_)) {
            return;
        }
        let values: Vec<u32> = self.iter().collect();
        *self = Self::from_values(&values);
    }

    /// A 64-bit digest of the voxel values (FNV-1a over each value as 4 little-endian bytes, in
    /// [`LocalVoxelIndex`] order). It depends only on the values, never on the representation,
    /// and is stable across platforms and versions: it may be saved or sent over the network.
    pub fn content_digest(&self) -> u64 {
        let mut hasher = Fnv1a64::new();
        for value in self.iter() {
            hasher.write(&value.to_le_bytes());
        }
        hasher.finish()
    }

    pub fn kind(&self) -> StorageKind {
        match &self.representation {
            Representation::Uniform(_) => StorageKind::Uniform,
            Representation::Paletted { palette, indices } => StorageKind::Paletted {
                palette_length: palette.len(),
                bits_per_index: indices.bits_per_index(),
            },
            Representation::Dense(_) => StorageKind::Dense,
        }
    }

    /// Bytes allocated on the heap for this channel's values.
    pub fn heap_size_bytes(&self) -> usize {
        match &self.representation {
            Representation::Uniform(_) => 0,
            Representation::Paletted { palette, indices } => {
                palette.len() * size_of::<u32>() + indices.heap_size_bytes()
            }
            Representation::Dense(values) => values.len() * size_of::<u32>(),
        }
    }
}

fn bits_per_index_for(palette_length: usize) -> u8 {
    match palette_length {
        0..=2 => 1,
        3..=4 => 2,
        5..=16 => 4,
        17..=MAXIMUM_PALETTE_LENGTH => 8,
        _ => panic!("a palette of {palette_length} entries does not fit 8-bit indices"),
    }
}

impl PartialEq for ChannelStorage {
    fn eq(&self, other: &Self) -> bool {
        match (&self.representation, &other.representation) {
            (Representation::Uniform(a), Representation::Uniform(b)) => a == b,
            (Representation::Dense(a), Representation::Dense(b)) => a == b,
            _ => self.iter().eq(other.iter()),
        }
    }
}

impl Eq for ChannelStorage {}

impl fmt::Debug for ChannelStorage {
    /// A short summary: a brick channel has 32768 values.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.representation {
            Representation::Uniform(value) => write!(formatter, "ChannelStorage::Uniform({value})"),
            Representation::Paletted { palette, indices } => write!(
                formatter,
                "ChannelStorage::Paletted {{ palette_length: {}, bits_per_index: {} }}",
                palette.len(),
                indices.bits_per_index()
            ),
            Representation::Dense(_) => write!(formatter, "ChannelStorage::Dense"),
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn voxel(index: usize) -> LocalVoxelIndex {
        LocalVoxelIndex::from_index(index).expect("test indices are in range")
    }

    /// Voxel `k` holds the distinct value `k` for `k` in `1..=count`; every other voxel holds 0.
    /// The palette therefore needs `count + 1` entries.
    fn storage_with_distinct_values(count: u32) -> ChannelStorage {
        let mut storage = ChannelStorage::uniform(0);
        for k in 1..=count {
            storage.set(voxel(k as usize), k);
        }
        storage
    }

    fn paletted(palette_length: usize, bits_per_index: u8) -> StorageKind {
        StorageKind::Paletted {
            palette_length,
            bits_per_index,
        }
    }

    #[test]
    fn uniform_storage_reports_its_value_and_costs_no_heap_memory() {
        let storage = ChannelStorage::uniform(42);
        assert_eq!(storage.kind(), StorageKind::Uniform);
        assert_eq!(storage.heap_size_bytes(), 0);
        assert_eq!(storage.uniform_value(), Some(42));
        assert_eq!(storage.get(voxel(0)), 42);
        assert_eq!(storage.get(voxel(BRICK_VOXEL_COUNT - 1)), 42);
    }

    #[test]
    fn writing_the_value_a_uniform_channel_already_holds_changes_nothing() {
        let mut storage = ChannelStorage::uniform(42);
        assert!(!storage.set(voxel(10), 42));
        assert_eq!(storage.kind(), StorageKind::Uniform);
    }

    #[test]
    fn the_first_different_value_creates_a_one_bit_palette() {
        let mut storage = ChannelStorage::uniform(7);
        assert!(storage.set(voxel(10), 9));

        assert_eq!(storage.kind(), paletted(2, 1));
        // 512 words of 8 bytes for the indices, plus two 4-byte palette entries.
        assert_eq!(storage.heap_size_bytes(), 4096 + 8);
        assert_eq!(storage.get(voxel(10)), 9);
        assert_eq!(storage.get(voxel(9)), 7);
        assert_eq!(storage.get(voxel(11)), 7);
    }

    #[test]
    fn palette_width_grows_at_each_boundary_and_keeps_every_value() {
        // (distinct values written after the initial 0, palette length, index width)
        let expectations = [
            (1, 2, 1),
            (2, 3, 2),
            (3, 4, 2),
            (4, 5, 4),
            (15, 16, 4),
            (16, 17, 8),
            (255, 256, 8),
        ];
        let mut storage = ChannelStorage::uniform(0);
        let mut written = 0u32;
        for (target, expected_palette_length, expected_bits) in expectations {
            while written < target {
                written += 1;
                storage.set(voxel(written as usize), written);
            }
            assert_eq!(
                storage.kind(),
                paletted(expected_palette_length, expected_bits),
                "after {target} distinct values"
            );
            for k in 0..=written {
                assert_eq!(
                    storage.get(voxel(k as usize)),
                    k,
                    "voxel {k} after {target}"
                );
            }
        }
    }

    #[test]
    fn a_257th_distinct_value_switches_to_dense_storage_without_losing_data() {
        let mut storage = storage_with_distinct_values(255);
        assert_eq!(storage.kind(), paletted(256, 8));

        assert!(storage.set(voxel(256), 256));

        assert_eq!(storage.kind(), StorageKind::Dense);
        assert_eq!(storage.heap_size_bytes(), BRICK_VOXEL_COUNT * 4);
        for k in 0..=256 {
            assert_eq!(storage.get(voxel(k)), k as u32);
        }
        // Dense storage keeps accepting writes.
        assert!(storage.set(voxel(5), 12345));
        assert!(!storage.set(voxel(5), 12345));
        assert_eq!(storage.get(voxel(5)), 12345);
        assert_eq!(storage.get(voxel(6)), 6);
    }

    #[test]
    fn set_reports_a_change_only_when_the_stored_value_differs_in_every_representation() {
        let representations = [
            ChannelStorage::uniform(3),
            storage_with_distinct_values(5),
            storage_with_distinct_values(300),
        ];
        let expected_kinds = [StorageKind::Uniform, paletted(6, 4), StorageKind::Dense];
        for (mut storage, expected_kind) in representations.into_iter().zip(expected_kinds) {
            assert_eq!(storage.kind(), expected_kind);
            // Voxel 20000 holds 3 in the uniform storage and 0 in the others.
            let current = storage.get(voxel(20000));
            assert!(!storage.set(voxel(20000), current), "{expected_kind:?}");
            assert!(storage.set(voxel(20000), 777), "{expected_kind:?}");
            assert!(!storage.set(voxel(20000), 777), "{expected_kind:?}");
            assert_eq!(storage.get(voxel(20000)), 777);
        }
    }

    #[test]
    fn fill_discards_per_voxel_data_and_becomes_uniform() {
        let mut storage = storage_with_distinct_values(300);
        storage.fill(9);
        assert_eq!(storage.kind(), StorageKind::Uniform);
        assert_eq!(storage.heap_size_bytes(), 0);
        assert_eq!(storage.uniform_value(), Some(9));
    }

    #[test]
    fn compact_drops_unused_palette_entries_and_returns_to_uniform() {
        let mut storage = ChannelStorage::uniform(0);
        storage.set(voxel(10), 5);
        storage.set(voxel(10), 0);
        // The palette still remembers 5 although no voxel holds it.
        assert_eq!(storage.kind(), paletted(2, 1));
        assert_eq!(storage.uniform_value(), Some(0));

        storage.compact();

        assert_eq!(storage.kind(), StorageKind::Uniform);
        assert_eq!(storage.get(voxel(10)), 0);
    }

    #[test]
    fn compact_shrinks_dense_storage_when_few_distinct_values_remain() {
        let mut storage = storage_with_distinct_values(300);
        assert_eq!(storage.kind(), StorageKind::Dense);
        for k in 11..=300 {
            storage.set(voxel(k), 0);
        }

        storage.compact();

        // Values 0..=10 remain: eleven entries need four bits.
        assert_eq!(storage.kind(), paletted(11, 4));
        for k in 0..=10 {
            assert_eq!(storage.get(voxel(k)), k as u32);
        }
        assert_eq!(storage.get(voxel(11)), 0);
    }

    #[test]
    fn compact_narrows_the_index_width() {
        let mut storage = storage_with_distinct_values(20);
        assert_eq!(storage.kind(), paletted(21, 8));
        for k in 3..=20 {
            storage.set(voxel(k), 0);
        }
        storage.compact();
        assert_eq!(storage.kind(), paletted(3, 2));
    }

    #[test]
    fn from_values_lists_the_palette_in_order_of_first_occurrence() {
        let mut values = vec![7u32; BRICK_VOXEL_COUNT];
        values[1] = 3;
        values[2] = 7;
        values[3] = 9;
        values[4] = 3;

        let storage = ChannelStorage::from_values(&values);

        let Representation::Paletted { palette, indices } = &storage.representation else {
            panic!("expected a paletted representation, got {storage:?}");
        };
        assert_eq!(palette, &[7, 3, 9]);
        assert_eq!(indices.bits_per_index(), 2);
        assert!(storage.iter().eq(values.iter().copied()));
    }

    #[test]
    fn from_values_collapses_matching_values_to_uniform() {
        let storage = ChannelStorage::from_values(&vec![5; BRICK_VOXEL_COUNT]);
        assert_eq!(storage.kind(), StorageKind::Uniform);
        assert_eq!(storage.uniform_value(), Some(5));
    }

    #[test]
    #[should_panic(expected = "one value per voxel")]
    fn from_values_rejects_the_wrong_number_of_values() {
        ChannelStorage::from_values(&[0; 10]);
    }

    #[test]
    fn equality_compares_values_not_representations() {
        let plain = ChannelStorage::uniform(0);
        let mut with_stale_palette = ChannelStorage::uniform(0);
        with_stale_palette.set(voxel(3), 8);
        with_stale_palette.set(voxel(3), 0);
        assert_ne!(plain.kind(), with_stale_palette.kind());
        assert_eq!(plain, with_stale_palette);
        assert_eq!(with_stale_palette, plain);

        let mut one_voxel_different = with_stale_palette.clone();
        one_voxel_different.set(voxel(BRICK_VOXEL_COUNT - 1), 1);
        assert_ne!(plain, one_voxel_different);
        assert_ne!(one_voxel_different, plain);
    }

    #[test]
    fn uniform_value_is_exact_for_every_representation() {
        assert_eq!(storage_with_distinct_values(1).uniform_value(), None);
        assert_eq!(storage_with_distinct_values(300).uniform_value(), None);

        // A dense channel whose voxels all ended up equal is uniform, though not yet compacted.
        let mut flattened = storage_with_distinct_values(300);
        for index in 0..BRICK_VOXEL_COUNT {
            flattened.set(voxel(index), 7);
        }
        assert_eq!(flattened.kind(), StorageKind::Dense);
        assert_eq!(flattened.uniform_value(), Some(7));
        flattened.compact();
        assert_eq!(flattened.kind(), StorageKind::Uniform);
    }

    #[test]
    fn uniform_value_is_found_when_every_voxel_uses_a_later_palette_entry() {
        // Palette [0, 5]; overwriting every voxel leaves all of them pointing at entry 1.
        let mut storage = ChannelStorage::uniform(0);
        for index in 0..BRICK_VOXEL_COUNT {
            storage.set(voxel(index), 5);
        }
        assert_eq!(storage.kind(), paletted(2, 1));
        assert_eq!(storage.uniform_value(), Some(5));

        storage.compact();

        assert_eq!(storage.kind(), StorageKind::Uniform);
        assert_eq!(storage.get(voxel(123)), 5);
    }

    /// Reference digest written independently of the production code path.
    fn reference_digest(values: &[u32]) -> u64 {
        let mut state: u64 = 0xcbf2_9ce4_8422_2325;
        for value in values {
            for byte in value.to_le_bytes() {
                state ^= u64::from(byte);
                state = state.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        state
    }

    #[test]
    fn content_digest_matches_an_independent_reference_in_every_representation() {
        let uniform = ChannelStorage::uniform(0xDEAD_BEEF);
        assert_eq!(
            uniform.content_digest(),
            reference_digest(&vec![0xDEAD_BEEF; BRICK_VOXEL_COUNT])
        );
        for distinct in [2u32, 10, 300] {
            let storage = storage_with_distinct_values(distinct);
            let values: Vec<u32> = storage.iter().collect();
            assert_eq!(
                storage.content_digest(),
                reference_digest(&values),
                "{distinct}"
            );
        }
    }

    #[test]
    fn content_digest_ignores_representation_but_not_values() {
        let mut stale = ChannelStorage::uniform(0);
        stale.set(voxel(3), 8);
        stale.set(voxel(3), 0);
        assert_ne!(stale.kind(), StorageKind::Uniform);
        assert_eq!(
            stale.content_digest(),
            ChannelStorage::uniform(0).content_digest()
        );

        // The same values at a different position must not collide.
        let mut at_three = ChannelStorage::uniform(0);
        at_three.set(voxel(3), 8);
        let mut at_four = ChannelStorage::uniform(0);
        at_four.set(voxel(4), 8);
        assert_ne!(at_three.content_digest(), at_four.content_digest());
    }

    #[test]
    fn debug_output_is_a_short_summary() {
        assert_eq!(
            format!("{:?}", ChannelStorage::uniform(4)),
            "ChannelStorage::Uniform(4)"
        );
        assert_eq!(
            format!("{:?}", storage_with_distinct_values(5)),
            "ChannelStorage::Paletted { palette_length: 6, bits_per_index: 4 }"
        );
        assert_eq!(
            format!("{:?}", storage_with_distinct_values(300)),
            "ChannelStorage::Dense"
        );
    }

    proptest! {
        /// The storage must behave exactly like a plain array of 32768 values, whichever
        /// representation the writes drive it through.
        #[test]
        fn behaves_like_a_plain_array_under_random_writes(
            distinct_value_count in prop_oneof![Just(2usize), Just(3), Just(5), Just(17), Just(300)],
            writes in proptest::collection::vec(
                (prop_oneof![3 => 0usize..64, 1 => 0usize..BRICK_VOXEL_COUNT], 0usize..300),
                0..1500,
            ),
        ) {
            let value_for = |choice: usize| {
                ((choice % distinct_value_count) as u32).wrapping_mul(0x9E37_79B1)
            };
            let mut model = vec![0u32; BRICK_VOXEL_COUNT];
            let mut storage = ChannelStorage::uniform(0);

            for (index, choice) in writes {
                let value = value_for(choice);
                let expected_change = model[index] != value;
                prop_assert_eq!(storage.set(voxel(index), value), expected_change);
                model[index] = value;
            }

            for (index, expected) in model.iter().enumerate() {
                prop_assert_eq!(storage.get(voxel(index)), *expected);
            }
            let canonical = ChannelStorage::from_values(&model);
            prop_assert!(storage == canonical, "{storage:?} differs from {canonical:?}");
            let expected_uniform = model.iter().all(|&value| value == model[0]).then_some(model[0]);
            prop_assert_eq!(storage.uniform_value(), expected_uniform);

            let mut compacted = storage.clone();
            compacted.compact();
            prop_assert!(compacted == canonical);
            prop_assert_eq!(compacted.kind(), canonical.kind());
            let mut compacted_again = compacted.clone();
            compacted_again.compact();
            prop_assert_eq!(compacted_again.kind(), compacted.kind());
        }
    }
}
