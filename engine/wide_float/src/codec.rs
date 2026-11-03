//! The saved form of a number.
//!
//! `precision` (a varint of bits), then one byte: 0 for zero, 1 for a number above zero, 2 for one
//! below; for a number that is not zero, its binary `exponent` (a zigzag varint) and its mantissa
//! (a length-prefixed run of little-endian bytes), the value being `± mantissa × 2^exponent`. The
//! mantissa is odd and has no byte of zero at its top, so **a number has one encoding**: saves and
//! replays are compared byte for byte. A number with few bits (a whole number of seconds, a double
//! widened to 4,096 bits) takes few bytes however wide its precision is; a full 4,096 bits is 512
//! bytes and a few more. Reading refuses every other form.

use binary_codec::{ByteReader, ByteWriter, DecodeError};

use crate::{NumberError, Precision, WideFloat};

const ZERO: u8 = 0;
const POSITIVE: u8 = 1;
const NEGATIVE: u8 = 2;

impl WideFloat {
    pub fn write_to(&self, writer: &mut ByteWriter) {
        writer.write_varint(self.precision().bits());
        match self.to_parts() {
            None => writer.write_u8(ZERO),
            Some(parts) => {
                writer.write_u8(if parts.negative { NEGATIVE } else { POSITIVE });
                writer.write_signed_varint(parts.exponent);
                writer.write_bytes(&parts.mantissa);
            }
        }
    }

    pub fn read_from(reader: &mut ByteReader<'_>) -> Result<WideFloat, DecodeError> {
        let precision = Precision::new(reader.read_varint()?)
            .map_err(|_| DecodeError::Invalid("a number's precision is out of range"))?;
        let negative = match reader.read_u8()? {
            ZERO => return Ok(WideFloat::zero(precision)),
            POSITIVE => false,
            NEGATIVE => true,
            _ => return Err(DecodeError::Invalid("a number is zero, above or below")),
        };
        let exponent = reader.read_signed_varint()?;
        let mantissa = reader.read_bytes()?;
        // One encoding for a number: the mantissa is odd and its top byte is not zero.
        match (mantissa.first(), mantissa.last()) {
            (Some(first), Some(last)) if first & 1 == 1 && *last != 0 => {}
            _ => {
                return Err(DecodeError::Invalid(
                    "a mantissa is odd and has no zero byte on top",
                ));
            }
        }
        WideFloat::from_parts(negative, mantissa, exponent, precision).map_err(
            |error| match error {
                NumberError::DoesNotFit { .. } => {
                    DecodeError::Invalid("a mantissa has more bits than the precision")
                }
                _ => DecodeError::Invalid("an exponent is out of range"),
            },
        )
    }
}
