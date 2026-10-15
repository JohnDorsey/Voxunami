use voxel_space::BRICK_VOXEL_COUNT;

/// One small unsigned integer per voxel of a brick, packed into 64-bit words.
///
/// Supported widths are 1, 2, 4 and 8 bits. They all divide 64, so a slot never straddles two
/// words, and the slot count per word is a power of two, so locating a slot needs only shifts.
#[derive(Clone)]
pub(crate) struct PackedIndices {
    bits_per_index: u8,
    words: Box<[u64]>,
}

impl PackedIndices {
    /// All slots start at zero.
    pub(crate) fn new(bits_per_index: u8) -> Self {
        assert!(
            matches!(bits_per_index, 1 | 2 | 4 | 8),
            "unsupported index width {bits_per_index}"
        );
        let word_count = BRICK_VOXEL_COUNT * usize::from(bits_per_index) / 64;
        Self {
            bits_per_index,
            words: vec![0; word_count].into_boxed_slice(),
        }
    }

    pub(crate) fn bits_per_index(&self) -> u8 {
        self.bits_per_index
    }

    pub(crate) fn get(&self, index: usize) -> usize {
        let (word_index, shift) = self.locate(index);
        ((self.words[word_index] >> shift) & self.mask()) as usize
    }

    pub(crate) fn set(&mut self, index: usize, value: usize) {
        debug_assert!(value as u64 <= self.mask(), "value {value} does not fit");
        let (word_index, shift) = self.locate(index);
        let mask = self.mask();
        let word = &mut self.words[word_index];
        *word = (*word & !(mask << shift)) | ((value as u64) << shift);
    }

    /// The same slot values at a different width, which must be able to hold them.
    pub(crate) fn repacked(&self, bits_per_index: u8) -> Self {
        let mut repacked = Self::new(bits_per_index);
        for index in 0..BRICK_VOXEL_COUNT {
            repacked.set(index, self.get(index));
        }
        repacked
    }

    /// True if every slot holds the same value. Exact, because slots fill whole words.
    pub(crate) fn all_slots_equal(&self) -> bool {
        let mask = self.mask();
        // `u64::MAX / mask` is the pattern `0b…0001_0001` that repeats a slot value across a word.
        let pattern = self.get(0) as u64 * (u64::MAX / mask);
        self.words.iter().all(|&word| word == pattern)
    }

    pub(crate) fn words(&self) -> &[u64] {
        &self.words
    }

    /// Rebuilds from stored words. `None` if the width or word count is wrong.
    pub(crate) fn from_words(bits_per_index: u8, words: Vec<u64>) -> Option<Self> {
        if !matches!(bits_per_index, 1 | 2 | 4 | 8)
            || words.len() != BRICK_VOXEL_COUNT * usize::from(bits_per_index) / 64
        {
            return None;
        }
        Some(Self {
            bits_per_index,
            words: words.into_boxed_slice(),
        })
    }

    /// True if every slot holds a value below `limit`.
    pub(crate) fn all_slots_below(&self, limit: usize) -> bool {
        (0..BRICK_VOXEL_COUNT).all(|index| self.get(index) < limit)
    }

    pub(crate) fn heap_size_bytes(&self) -> usize {
        self.words.len() * size_of::<u64>()
    }

    fn mask(&self) -> u64 {
        (1u64 << self.bits_per_index) - 1
    }

    /// The word holding slot `index`, and the bit offset of the slot within it.
    fn locate(&self, index: usize) -> (usize, u32) {
        let slots_per_word_log2 = 6 - self.bits_per_index.trailing_zeros();
        let word_index = index >> slots_per_word_log2;
        let slot = index & ((1 << slots_per_word_log2) - 1);
        (word_index, slot as u32 * u32::from(self.bits_per_index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDTHS: [u8; 4] = [1, 2, 4, 8];

    fn pattern_value(index: usize, bits_per_index: u8) -> usize {
        // Varies quickly with the index so neighbouring slots differ.
        (index * 7 + index / 3) % (1usize << bits_per_index)
    }

    #[test]
    fn word_counts_match_the_width() {
        for (bits, expected_bytes) in [(1u8, 4096usize), (2, 8192), (4, 16384), (8, 32768)] {
            assert_eq!(PackedIndices::new(bits).heap_size_bytes(), expected_bytes);
        }
    }

    #[test]
    fn new_slots_are_zero() {
        for bits in WIDTHS {
            let indices = PackedIndices::new(bits);
            assert!((0..BRICK_VOXEL_COUNT).all(|index| indices.get(index) == 0));
        }
    }

    #[test]
    fn every_slot_reads_back_what_was_written_without_disturbing_neighbours() {
        for bits in WIDTHS {
            let mut indices = PackedIndices::new(bits);
            for index in 0..BRICK_VOXEL_COUNT {
                indices.set(index, pattern_value(index, bits));
            }
            for index in 0..BRICK_VOXEL_COUNT {
                assert_eq!(
                    indices.get(index),
                    pattern_value(index, bits),
                    "{bits} bits"
                );
            }
        }
    }

    #[test]
    fn overwriting_a_slot_replaces_only_that_slot() {
        for bits in WIDTHS {
            let maximum = (1usize << bits) - 1;
            let mut indices = PackedIndices::new(bits);
            for index in 0..BRICK_VOXEL_COUNT {
                indices.set(index, maximum);
            }
            indices.set(100, 0);
            assert_eq!(indices.get(99), maximum);
            assert_eq!(indices.get(100), 0);
            assert_eq!(indices.get(101), maximum);
            indices.set(100, maximum);
            assert!((0..BRICK_VOXEL_COUNT).all(|index| indices.get(index) == maximum));
        }
    }

    #[test]
    fn repacking_to_a_wider_width_preserves_every_slot() {
        let mut narrow = PackedIndices::new(1);
        for index in 0..BRICK_VOXEL_COUNT {
            narrow.set(index, pattern_value(index, 1));
        }
        for wider in [2, 4, 8] {
            let repacked = narrow.repacked(wider);
            assert_eq!(repacked.bits_per_index(), wider);
            for index in 0..BRICK_VOXEL_COUNT {
                assert_eq!(repacked.get(index), pattern_value(index, 1));
            }
        }
    }

    #[test]
    fn all_slots_equal_is_exact() {
        for bits in WIDTHS {
            let maximum = (1usize << bits) - 1;
            let mut indices = PackedIndices::new(bits);
            assert!(indices.all_slots_equal(), "all zero, {bits} bits");

            for index in 0..BRICK_VOXEL_COUNT {
                indices.set(index, maximum);
            }
            assert!(indices.all_slots_equal(), "all maximum, {bits} bits");

            for position in [0, 1, 63, 64, BRICK_VOXEL_COUNT - 1] {
                indices.set(position, maximum - 1);
                assert!(
                    !indices.all_slots_equal(),
                    "slot {position} differs, {bits} bits"
                );
                indices.set(position, maximum);
            }
        }
    }
}
