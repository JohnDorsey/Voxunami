//! The saved form of a number: it comes back equal to the bit, has one encoding, is the same
//! bytes in every build (these bytes go into worlds and replays), and cannot be made to panic or
//! to allocate much by a file that lies.

use binary_codec::{ByteReader, ByteWriter, DecodeError};
use proptest::prelude::*;
use wide_float::test_support::{wide_float, wide_float_of};
use wide_float::{Precision, WideFloat};

fn encode(number: &WideFloat) -> Vec<u8> {
    let mut writer = ByteWriter::new();
    number.write_to(&mut writer);
    writer.into_bytes()
}

fn decode(bytes: &[u8]) -> Result<WideFloat, DecodeError> {
    let mut reader = ByteReader::new(bytes);
    let number = WideFloat::read_from(&mut reader)?;
    reader.finish()?;
    Ok(number)
}

fn at(bits: u32) -> Precision {
    Precision::of(bits)
}

fn parse(text: &str, bits: u32) -> WideFloat {
    WideFloat::parse(text, at(bits)).unwrap()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn a_number_comes_back_equal_to_the_bit_with_its_width(number in wide_float()) {
        let bytes = encode(&number);
        let back = decode(&bytes).unwrap();
        prop_assert_eq!(&back, &number);
        prop_assert_eq!(back.precision(), number.precision());
    }

    #[test]
    fn a_number_has_one_encoding(
        number in wide_float(), position in any::<prop::sample::Index>(), flip in 1u8..=255,
    ) {
        // Whatever a changed byte decodes to, if it decodes at all, writes back as the changed bytes:
        // nothing is accepted that would not be written.
        let mut bytes = encode(&number);
        let at = position.index(bytes.len());
        bytes[at] ^= flip;
        let mut reader = ByteReader::new(&bytes);
        if let Ok(other) = WideFloat::read_from(&mut reader) {
            let used = bytes.len() - reader.remaining().len();
            prop_assert_eq!(encode(&other), bytes[..used].to_vec());
        }
    }

    #[test]
    fn numbers_written_one_after_another_are_read_back_one_after_another(
        numbers in proptest::collection::vec(wide_float(), 0..9),
    ) {
        let mut writer = ByteWriter::new();
        for number in &numbers {
            number.write_to(&mut writer);
        }
        let bytes = writer.into_bytes();
        let mut reader = ByteReader::new(&bytes);
        for number in &numbers {
            prop_assert_eq!(&WideFloat::read_from(&mut reader).unwrap(), number);
        }
        prop_assert!(reader.finish().is_ok());
    }

    #[test]
    fn nothing_makes_the_reader_panic(bytes in proptest::collection::vec(any::<u8>(), 0..80)) {
        let _ = decode(&bytes);
    }

    #[test]
    fn a_truncated_number_is_an_error(number in wide_float(), cut in any::<prop::sample::Index>()) {
        let bytes = encode(&number);
        let kept = cut.index(bytes.len());
        prop_assert!(decode(&bytes[..kept]).is_err());
    }

    /// A number of the widest kind the game holds (4,096 bits) is half a kilobyte and a few bytes.
    #[test]
    fn a_wide_number_takes_its_bytes_and_a_few(number in wide_float_of(at(4096))) {
        prop_assert!(encode(&number).len() <= 512 + 8);
    }
}

#[test]
fn a_number_with_few_bits_takes_few_bytes_however_wide_its_width() {
    let width = at(4096);
    // precision (2 bytes for 4,096), kind, exponent, mantissa length, mantissa. 0.1 is 0xcccccccccccccd x 2^-55
    // (52 bits, 7 bytes); 600 is 75 x 2^3; zero has no more than its kind.
    assert_eq!(
        encode(&WideFloat::from_f64(0.1, width).unwrap()).len(),
        2 + 1 + 1 + 1 + 7
    );
    assert_eq!(
        encode(&WideFloat::from_i64(600, width)).len(),
        2 + 1 + 1 + 1 + 1
    );
    assert_eq!(encode(&WideFloat::zero(width)).len(), 2 + 1);
}

#[test]
fn the_bytes_are_the_documented_bytes() {
    // precision (varint) | kind | exponent (zigzag varint) | mantissa (length, little-endian bytes)
    let cases: Vec<(WideFloat, Vec<u8>)> = vec![
        (parse("0x1p0", 128), vec![0x80, 0x01, 1, 0x00, 1, 0x01]),
        (parse("-0x3p-1", 256), vec![0x80, 0x02, 2, 0x01, 1, 0x03]),
        (WideFloat::zero(at(4096)), vec![0x80, 0x20, 0]),
        (
            parse("0x1p100", 128),
            vec![0x80, 0x01, 1, 0xC8, 0x01, 1, 0x01],
        ),
        // 0x1234 x 2^-70 is 4 x 0x48d x 2^-70: the mantissa is made odd (0x48d, little-endian 8d 04) and the exponent is -68 (zigzag 135).
        (
            parse("0x1234p-70", 129),
            vec![0x81, 0x01, 1, 0x87, 0x01, 2, 0x8D, 0x04],
        ),
        (parse("0x7dp3", 1024), vec![0x80, 0x08, 1, 0x06, 1, 0x7D]),
    ];
    for (number, bytes) in cases {
        assert_eq!(encode(&number), bytes, "{number:?}");
        assert_eq!(decode(&bytes).unwrap(), number);
    }
}

#[test]
fn the_reader_refuses_what_the_writer_would_not_write() {
    let refused: Vec<(&str, Vec<u8>)> = vec![
        ("an empty file", vec![]),
        ("a precision below 128", vec![0x7F, 0]),
        ("a precision of 0", vec![0x00, 0]),
        ("a precision above 2^24", vec![0x81, 0x80, 0x80, 0x08, 0]),
        (
            "a precision that is a huge varint",
            vec![
                0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01, 0,
            ],
        ),
        ("a kind of 3", vec![0x80, 0x01, 3]),
        ("a mantissa of nothing", vec![0x80, 0x01, 1, 0x00, 0]),
        ("an even mantissa", vec![0x80, 0x01, 1, 0x00, 1, 0x02]),
        (
            "a zero byte on top of the mantissa",
            vec![0x80, 0x01, 1, 0x00, 2, 0x01, 0x00],
        ),
        (
            "a mantissa longer than the precision",
            vec![
                0x80, 0x01, 1, 0x00, 17, 0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01,
            ],
        ),
        (
            "a mantissa that claims more than the file holds",
            vec![0x80, 0x01, 1, 0x00, 100, 0x01],
        ),
        (
            "an exponent out of range",
            vec![
                0x80, 0x01, 1, 0xFE, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01, 1, 0x01,
            ],
        ),
        // zigzag(-2^40) is 2^41 - 1: an exponent far below the least number, which is not a zero.
        (
            "an exponent far below the range",
            vec![0x80, 0x01, 1, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x3F, 1, 0x01],
        ),
        (
            "an exponent that is not a varint",
            vec![
                0x80, 0x01, 1, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 1, 0x01,
            ],
        ),
    ];
    for (what, bytes) in refused {
        assert!(decode(&bytes).is_err(), "{what} is accepted");
    }
    // The widest mantissa a precision holds is fine; one bit more is not.
    let mut full = vec![0x80, 0x01, 1, 0x00, 16];
    full.extend([0xFF; 16]);
    assert!(decode(&full).is_ok());
    let mut one_more = vec![0x80, 0x01, 1, 0x00, 17];
    one_more.extend([0xFF; 16]);
    one_more.push(0x01);
    assert!(decode(&one_more).is_err());
    // Bytes after a number are the next field's, and a whole-file read says so.
    let mut trailing = encode(&parse("0x1p0", 128));
    trailing.push(7);
    assert!(decode(&trailing).is_err());
}

#[test]
fn a_lying_precision_does_not_allocate_it() {
    // A precision of 2^24 bits with no bytes after it fails at the next field, long before anything
    // of that size could be made.
    let bytes = vec![0x80, 0x80, 0x80, 0x08, 1, 0x00, 0xFF, 0xFF, 0x7F];
    assert!(decode(&bytes).is_err());
}
