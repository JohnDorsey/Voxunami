/// FNV-1a, 64 bit: a simple hash with a fixed published definition, so digests are identical on
/// every platform and in every future version. It detects accidental differences (desync, golden
/// tests); it is not meant to resist an adversary.
pub(crate) struct Fnv1a64(u64);

const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01b3;

impl Fnv1a64 {
    pub(crate) fn new() -> Self {
        Self(OFFSET_BASIS)
    }

    pub(crate) fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(PRIME);
        }
    }

    pub(crate) fn finish(&self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest_of(text: &str) -> u64 {
        let mut hasher = Fnv1a64::new();
        hasher.write(text.as_bytes());
        hasher.finish()
    }

    /// Published FNV-1a 64-bit test vectors.
    #[test]
    fn matches_the_published_test_vectors() {
        assert_eq!(digest_of(""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(digest_of("a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(digest_of("foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn writing_in_pieces_equals_writing_at_once() {
        let mut pieces = Fnv1a64::new();
        pieces.write(b"foo");
        pieces.write(b"bar");
        assert_eq!(pieces.finish(), digest_of("foobar"));
    }
}
