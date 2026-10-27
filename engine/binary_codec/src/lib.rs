//! Reading and writing the primitive values of saved files and network messages: little-endian fixed
//! integers, LEB128 variable-length integers, zigzag signed integers, and length-prefixed UTF-8 strings.
//!
//! The reader is bounds-checked everywhere and never panics on malformed input.

use namespaced_id::{NamespacedId, NamespacedIdError};

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    #[error("the data ends too early")]
    UnexpectedEnd,
    #[error("a variable-length number is malformed or too large")]
    InvalidVarint,
    #[error("text is not valid UTF-8")]
    InvalidUtf8,
    #[error("invalid identifier: {0}")]
    InvalidIdentifier(#[from] NamespacedIdError),
    #[error("{0} unexpected bytes follow the data")]
    TrailingBytes(usize),
    #[error("{0}")]
    Invalid(&'static str),
}

#[derive(Debug, Default)]
pub struct ByteWriter {
    bytes: Vec<u8>,
}

impl ByteWriter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    pub fn write_u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    pub fn write_u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub fn write_u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub fn write_varint(&mut self, mut value: u64) {
        loop {
            let low = (value & 0x7F) as u8;
            value >>= 7;
            if value == 0 {
                self.bytes.push(low);
                return;
            }
            self.bytes.push(low | 0x80);
        }
    }

    /// Zigzag encoding: small magnitudes of either sign take few bytes.
    pub fn write_signed_varint(&mut self, value: i64) {
        self.write_varint(((value << 1) ^ (value >> 63)) as u64);
    }

    /// A signed integer of up to 128 bits (a coordinate): a byte for the number of bytes that follow,
    /// then the zigzag value in that many little-endian bytes, the fewest that hold it (zero takes
    /// none). The length byte is the whole reason this is not a varint: a wider coordinate type later
    /// (256 bits, 512 bits) needs no new format, only a reader that accepts a longer length.
    pub fn write_wide_signed(&mut self, value: i128) {
        let zigzag = ((value << 1) ^ (value >> 127)) as u128;
        let length = (128 - zigzag.leading_zeros() as usize).div_ceil(8);
        self.write_u8(length as u8);
        self.write_raw(&zigzag.to_le_bytes()[..length]);
    }

    pub fn write_raw(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    /// A length-prefixed run of bytes.
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        self.write_varint(bytes.len() as u64);
        self.write_raw(bytes);
    }

    pub fn write_string(&mut self, text: &str) {
        self.write_bytes(text.as_bytes());
    }

    pub fn write_identifier(&mut self, identifier: &NamespacedId) {
        self.write_string(identifier.as_str());
    }
}

#[derive(Clone, Debug)]
pub struct ByteReader<'a> {
    remaining: &'a [u8],
}

impl<'a> ByteReader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { remaining: bytes }
    }

    pub fn remaining(&self) -> &'a [u8] {
        self.remaining
    }

    /// Replaces the unread bytes, for decoders that consume part of the input themselves.
    pub fn set_remaining(&mut self, remaining: &'a [u8]) {
        self.remaining = remaining;
    }

    /// Errors if any bytes are left unread.
    pub fn finish(&self) -> Result<(), DecodeError> {
        match self.remaining.len() {
            0 => Ok(()),
            extra => Err(DecodeError::TrailingBytes(extra)),
        }
    }

    pub fn read_raw(&mut self, count: usize) -> Result<&'a [u8], DecodeError> {
        if self.remaining.len() < count {
            return Err(DecodeError::UnexpectedEnd);
        }
        let (taken, rest) = self.remaining.split_at(count);
        self.remaining = rest;
        Ok(taken)
    }

    pub fn read_u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.read_raw(1)?[0])
    }

    pub fn read_u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(
            self.read_raw(4)?.try_into().expect("four bytes were read"),
        ))
    }

    pub fn read_u64(&mut self) -> Result<u64, DecodeError> {
        Ok(u64::from_le_bytes(
            self.read_raw(8)?.try_into().expect("eight bytes were read"),
        ))
    }

    pub fn read_varint(&mut self) -> Result<u64, DecodeError> {
        let mut value = 0u64;
        for position in 0..10 {
            let byte = self.read_u8()?;
            let payload = u64::from(byte & 0x7F);
            // The tenth byte may only contribute the single top bit of a u64.
            if position == 9 && payload > 1 {
                return Err(DecodeError::InvalidVarint);
            }
            value |= payload << (7 * position);
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(DecodeError::InvalidVarint)
    }

    pub fn read_signed_varint(&mut self) -> Result<i64, DecodeError> {
        let zigzag = self.read_varint()?;
        Ok((zigzag >> 1) as i64 ^ -((zigzag & 1) as i64))
    }

    /// The counterpart of [`ByteWriter::write_wide_signed`]. A number has one encoding (saves and
    /// replays are compared byte for byte), so a length over 16 and a top byte of zero are errors.
    pub fn read_wide_signed(&mut self) -> Result<i128, DecodeError> {
        let length = usize::from(self.read_u8()?);
        if length > 16 {
            return Err(DecodeError::InvalidVarint);
        }
        let digits = self.read_raw(length)?;
        if digits.last() == Some(&0) {
            return Err(DecodeError::InvalidVarint);
        }
        let mut bytes = [0u8; 16];
        bytes[..length].copy_from_slice(digits);
        let zigzag = u128::from_le_bytes(bytes);
        Ok((zigzag >> 1) as i128 ^ -((zigzag & 1) as i128))
    }

    /// A varint that is used as a length or count: it must be representable and, because every
    /// counted item takes at least `minimum_item_bytes`, cannot exceed what is left to read.
    pub fn read_count(&mut self, minimum_item_bytes: usize) -> Result<usize, DecodeError> {
        let count = usize::try_from(self.read_varint()?).map_err(|_| DecodeError::InvalidVarint)?;
        if count.saturating_mul(minimum_item_bytes) > self.remaining.len() {
            return Err(DecodeError::UnexpectedEnd);
        }
        Ok(count)
    }

    pub fn read_bytes(&mut self) -> Result<&'a [u8], DecodeError> {
        let length = self.read_count(1)?;
        self.read_raw(length)
    }

    pub fn read_string(&mut self) -> Result<&'a str, DecodeError> {
        std::str::from_utf8(self.read_bytes()?).map_err(|_| DecodeError::InvalidUtf8)
    }

    pub fn read_identifier(&mut self) -> Result<NamespacedId, DecodeError> {
        Ok(self.read_string()?.parse()?)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn varint_bytes(value: u64) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        writer.write_varint(value);
        writer.into_bytes()
    }

    #[test]
    fn varints_use_the_leb128_layout() {
        assert_eq!(varint_bytes(0), [0x00]);
        assert_eq!(varint_bytes(127), [0x7F]);
        assert_eq!(varint_bytes(128), [0x80, 0x01]);
        assert_eq!(varint_bytes(300), [0xAC, 0x02]);
        assert_eq!(varint_bytes(u64::MAX).len(), 10);
        assert_eq!(*varint_bytes(u64::MAX).last().unwrap(), 0x01);
    }

    #[test]
    fn zigzag_keeps_small_negative_numbers_short() {
        let mut writer = ByteWriter::new();
        for value in [0i64, -1, 1, -2, 2, 63, -64, 64] {
            writer.write_signed_varint(value);
        }
        assert_eq!(writer.into_bytes(), [0, 1, 2, 3, 4, 126, 127, 128, 1]);
    }

    #[test]
    fn malformed_varints_are_rejected() {
        // Eleven continuation bytes, and a tenth byte carrying more than one bit.
        assert_eq!(
            ByteReader::new(&[0x80; 11]).read_varint(),
            Err(DecodeError::InvalidVarint)
        );
        let mut too_big = vec![0xFF; 9];
        too_big.push(0x02);
        assert_eq!(
            ByteReader::new(&too_big).read_varint(),
            Err(DecodeError::InvalidVarint)
        );
        assert_eq!(
            ByteReader::new(&[0x80]).read_varint(),
            Err(DecodeError::UnexpectedEnd)
        );
    }

    fn wide_bytes(value: i128) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        writer.write_wide_signed(value);
        writer.into_bytes()
    }

    #[test]
    fn wide_integers_are_a_length_then_that_many_zigzag_bytes() {
        assert_eq!(wide_bytes(0), [0]);
        assert_eq!(wide_bytes(-1), [1, 1]);
        assert_eq!(wide_bytes(1), [1, 2]);
        assert_eq!(wide_bytes(63), [1, 126]);
        assert_eq!(wide_bytes(-64), [1, 127]);
        assert_eq!(wide_bytes(64), [1, 128]);
        assert_eq!(wide_bytes(-65), [1, 129]);
        assert_eq!(wide_bytes(128), [2, 0, 1]);
        assert_eq!(wide_bytes(1 << 62), [8, 0, 0, 0, 0, 0, 0, 0, 0x80]);
        assert_eq!(wide_bytes(1 << 63), [9, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        let mut largest = vec![16];
        largest.extend_from_slice(&[0xFE; 1]);
        largest.extend_from_slice(&[0xFF; 15]);
        assert_eq!(wide_bytes(i128::MAX), largest);
        let mut smallest = vec![16];
        smallest.extend_from_slice(&[0xFF; 16]);
        assert_eq!(wide_bytes(i128::MIN), smallest);
    }

    #[test]
    fn malformed_wide_integers_are_rejected() {
        let read = |bytes: &[u8]| ByteReader::new(bytes).read_wide_signed();
        // Not the shortest form: a zero where the top byte should be.
        assert_eq!(read(&[1, 0]), Err(DecodeError::InvalidVarint));
        assert_eq!(read(&[2, 5, 0]), Err(DecodeError::InvalidVarint));
        assert_eq!(
            read(&[16, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            Err(DecodeError::InvalidVarint)
        );
        // Wider than 128 bits.
        assert_eq!(read(&[17; 18]), Err(DecodeError::InvalidVarint));
        assert_eq!(read(&[255]), Err(DecodeError::InvalidVarint));
        // The data ends inside the number.
        assert_eq!(read(&[]), Err(DecodeError::UnexpectedEnd));
        assert_eq!(read(&[3, 1, 2]), Err(DecodeError::UnexpectedEnd));
    }

    #[test]
    fn strings_and_identifiers_round_trip_and_validate() {
        let mut writer = ByteWriter::new();
        writer.write_string("héllo");
        writer.write_identifier(&"base_blocks:stone".parse().unwrap());
        writer.write_string("Not:Valid");
        let bytes = writer.into_bytes();
        let mut reader = ByteReader::new(&bytes);
        assert_eq!(reader.read_string().unwrap(), "héllo");
        assert_eq!(
            reader.read_identifier().unwrap().as_str(),
            "base_blocks:stone"
        );
        assert!(matches!(
            reader.read_identifier(),
            Err(DecodeError::InvalidIdentifier(_))
        ));
        assert_eq!(reader.finish(), Ok(()));
        assert_eq!(
            ByteReader::new(&[2, 0xFF, 0xFE]).read_string(),
            Err(DecodeError::InvalidUtf8)
        );
    }

    #[test]
    fn counts_cannot_claim_more_items_than_there_are_bytes() {
        let mut writer = ByteWriter::new();
        writer.write_varint(1_000_000);
        writer.write_raw(&[1, 2, 3]);
        let bytes = writer.into_bytes();
        assert_eq!(
            ByteReader::new(&bytes).read_count(1),
            Err(DecodeError::UnexpectedEnd)
        );
        assert_eq!(
            ByteReader::new(&bytes).read_bytes(),
            Err(DecodeError::UnexpectedEnd)
        );
    }

    #[test]
    fn finish_reports_leftover_bytes() {
        let mut reader = ByteReader::new(&[1, 2, 3]);
        reader.read_u8().unwrap();
        assert_eq!(reader.finish(), Err(DecodeError::TrailingBytes(2)));
    }

    proptest! {
        #[test]
        fn wide_integers_round_trip_in_the_fewest_bytes(value in any::<i128>(), small in -70_000i128..70_000) {
            for value in [value, small, value >> 64, value >> 100] {
                let bytes = wide_bytes(value);
                let zigzag = ((value << 1) ^ (value >> 127)) as u128;
                prop_assert_eq!(bytes.len(), 1 + (128 - zigzag.leading_zeros() as usize).div_ceil(8));
                let mut reader = ByteReader::new(&bytes);
                prop_assert_eq!(reader.read_wide_signed(), Ok(value));
                prop_assert_eq!(reader.finish(), Ok(()));
            }
        }

        #[test]
        fn numbers_round_trip(unsigned in any::<u64>(), signed in any::<i64>(), small in any::<u32>()) {
            let mut writer = ByteWriter::new();
            writer.write_varint(unsigned);
            writer.write_signed_varint(signed);
            writer.write_u32(small);
            writer.write_u64(unsigned);
            let bytes = writer.into_bytes();
            let mut reader = ByteReader::new(&bytes);
            prop_assert_eq!(reader.read_varint(), Ok(unsigned));
            prop_assert_eq!(reader.read_signed_varint(), Ok(signed));
            prop_assert_eq!(reader.read_u32(), Ok(small));
            prop_assert_eq!(reader.read_u64(), Ok(unsigned));
            prop_assert_eq!(reader.finish(), Ok(()));
        }

        #[test]
        fn the_reader_never_panics_on_arbitrary_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..64)) {
            let mut reader = ByteReader::new(&bytes);
            let _ = reader.read_varint();
            let _ = reader.read_signed_varint();
            let _ = reader.read_wide_signed();
            let _ = reader.read_string();
            let _ = reader.read_identifier();
            let _ = reader.read_count(3);
            let _ = reader.read_u64();
        }
    }
}
