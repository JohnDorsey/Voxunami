//! Messages a game or plugin defines, declared once for both ends.
//!
//! A message type says its kind, how it becomes bytes and how bytes become it, and gives sample
//! values. The client and the server use the same type, so the two ends cannot disagree about a
//! message, and [`check_message`] tests the codec of every declared message the same way, so a
//! message that is declared is a message that round-trips.

use std::fmt::Debug;

use binary_codec::DecodeError;
use namespaced_id::NamespacedId;

/// A game message: the payload of a [`GameMessage`](crate::ClientMessage::GameMessage) of one kind.
pub trait Message: Sized + PartialEq + Debug + 'static {
    /// The kind the message is sent under, `plugin_or_game:name`; unique across a game's plugins.
    fn kind() -> NamespacedId;

    /// The payload. The same value always gives the same bytes.
    fn encode(&self) -> Vec<u8>;

    /// Reads a whole payload; bytes left over, or any malformed field, are an error. Never panics.
    fn decode(payload: &[u8]) -> Result<Self, DecodeError>;

    /// Values that between them exercise every field and every variant, including the extremes.
    /// At least one. Floating-point fields hold ordinary numbers, since a not-a-number is never
    /// equal to itself.
    fn samples() -> Vec<Self>;
}

/// Tests the codec of a message on its samples:
/// - every sample decodes from its own bytes to an equal value;
/// - encoding it again gives the same bytes;
/// - bytes after the payload are refused;
/// - a payload cut short is refused, or else is itself a payload (it decodes to a value that
///   encodes to exactly those bytes).
///
/// The error says what failed, for which sample.
pub fn check_message<M: Message>() -> Result<(), String> {
    let kind = M::kind();
    let samples = M::samples();
    if samples.is_empty() {
        return Err(format!("{kind} has no samples"));
    }
    for (index, sample) in samples.iter().enumerate() {
        let fail = |what: &str| Err(format!("{kind}, sample {index} ({sample:?}): {what}"));
        let bytes = sample.encode();
        if sample.encode() != bytes {
            return fail("encoding it twice gives different bytes");
        }
        match M::decode(&bytes) {
            Ok(decoded) if decoded == *sample => {}
            Ok(decoded) => return fail(&format!("decodes to a different value, {decoded:?}")),
            Err(error) => return fail(&format!("its own bytes do not decode: {error}")),
        }
        let mut longer = bytes.clone();
        longer.push(0);
        if M::decode(&longer).is_ok() {
            return fail("a trailing byte is accepted");
        }
        for length in 0..bytes.len() {
            if let Ok(shorter) = M::decode(&bytes[..length])
                && shorter.encode() != bytes[..length]
            {
                return fail(&format!(
                    "the first {length} of its {} bytes decode to {shorter:?}, which encodes differently",
                    bytes.len()
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU8, Ordering};

    use binary_codec::{ByteReader, ByteWriter};

    use super::*;

    /// A well-made message: two numbers and a flag.
    #[derive(Debug, PartialEq)]
    struct Good {
        count: u32,
        offset: i64,
        flag: bool,
    }

    fn write_good(message: &Good) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        writer.write_varint(u64::from(message.count));
        writer.write_signed_varint(message.offset);
        writer.write_u8(u8::from(message.flag));
        writer.into_bytes()
    }

    fn read_good(reader: &mut ByteReader<'_>) -> Result<Good, DecodeError> {
        Ok(Good {
            count: u32::try_from(reader.read_varint()?)
                .map_err(|_| DecodeError::Invalid("too large"))?,
            offset: reader.read_signed_varint()?,
            flag: match reader.read_u8()? {
                0 => false,
                1 => true,
                _ => return Err(DecodeError::Invalid("not a flag")),
            },
        })
    }

    fn read_whole_good(payload: &[u8]) -> Result<Good, DecodeError> {
        let mut reader = ByteReader::new(payload);
        let message = read_good(&mut reader)?;
        reader.finish()?;
        Ok(message)
    }

    fn good_samples() -> Vec<Good> {
        vec![
            Good {
                count: 0,
                offset: 0,
                flag: false,
            },
            Good {
                count: u32::MAX,
                offset: i64::MIN,
                flag: true,
            },
        ]
    }

    impl Message for Good {
        fn kind() -> NamespacedId {
            "test:good".parse().unwrap()
        }
        fn encode(&self) -> Vec<u8> {
            write_good(self)
        }
        fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
            read_whole_good(payload)
        }
        fn samples() -> Vec<Self> {
            good_samples()
        }
    }

    #[test]
    fn a_well_made_message_passes() {
        assert_eq!(check_message::<Good>(), Ok(()));
    }

    /// A message made of a `Good` one, with its codec swapped for a flawed function.
    macro_rules! flawed {
        ($name:ident, $encode:expr, $decode:expr, $samples:expr) => {
            #[derive(Debug, PartialEq)]
            struct $name(Good);
            impl Message for $name {
                fn kind() -> NamespacedId {
                    "test:flawed".parse().unwrap()
                }
                fn encode(&self) -> Vec<u8> {
                    let encode: fn(&Good) -> Vec<u8> = $encode;
                    encode(&self.0)
                }
                fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
                    let decode: fn(&[u8]) -> Result<Good, DecodeError> = $decode;
                    decode(payload).map(Self)
                }
                fn samples() -> Vec<Self> {
                    let samples: fn() -> Vec<Good> = $samples;
                    samples().into_iter().map(Self).collect()
                }
            }
        };
    }

    // Each of these is wrong in one way the check exists to find.
    flawed!(NoSamples, write_good, read_whole_good, Vec::new);
    flawed!(
        DropsAField,
        |message| {
            let mut writer = ByteWriter::new();
            writer.write_varint(u64::from(message.count));
            writer.write_u8(u8::from(message.flag));
            writer.into_bytes()
        },
        |payload| {
            let mut reader = ByteReader::new(payload);
            let count = u32::try_from(reader.read_varint()?).unwrap_or(0);
            let flag = reader.read_u8()? == 1;
            reader.finish()?;
            Ok(Good {
                count,
                offset: 0,
                flag,
            })
        },
        good_samples
    );
    flawed!(
        AcceptsTrailingBytes,
        write_good,
        |payload| read_good(&mut ByteReader::new(payload)),
        good_samples
    );
    flawed!(
        EncodesDifferentlyEachTime,
        |message| {
            static CALLS: AtomicU8 = AtomicU8::new(0);
            let mut bytes = write_good(message);
            // Only a trailing byte varies, which the flawed decoder below ignores.
            bytes.push(CALLS.fetch_add(1, Ordering::Relaxed) % 2);
            bytes
        },
        |payload| read_good(&mut ByteReader::new(payload)),
        || vec![Good {
            count: 1,
            offset: 1,
            flag: false,
        }]
    );
    flawed!(
        ReadsAShortPayloadAsSomethingElse,
        write_good,
        |payload| {
            // Without the flag, it takes the flag to be false: a cut-short payload that decodes.
            let mut reader = ByteReader::new(payload);
            let count = u32::try_from(reader.read_varint()?)
                .map_err(|_| DecodeError::Invalid("too large"))?;
            let offset = reader.read_signed_varint()?;
            let flag = reader.read_u8().unwrap_or(0) == 1;
            reader.finish()?;
            Ok(Good {
                count,
                offset,
                flag,
            })
        },
        good_samples
    );
    flawed!(
        DecodesToAnotherValue,
        write_good,
        |payload| {
            let mut good = read_whole_good(payload)?;
            good.count = good.count.wrapping_add(1);
            Ok(good)
        },
        good_samples
    );

    fn error_of(result: Result<(), String>) -> String {
        result.expect_err("the flaw is found")
    }

    #[test]
    fn each_kind_of_flaw_is_found_and_named() {
        assert!(error_of(check_message::<NoSamples>()).contains("has no samples"));
        assert!(error_of(check_message::<DropsAField>()).contains("different value"));
        assert!(error_of(check_message::<AcceptsTrailingBytes>()).contains("trailing byte"));
        assert!(
            error_of(check_message::<EncodesDifferentlyEachTime>()).contains("different bytes")
        );
        assert!(
            error_of(check_message::<ReadsAShortPayloadAsSomethingElse>())
                .contains("encodes differently")
        );
        assert!(error_of(check_message::<DecodesToAnotherValue>()).contains("different value"));
    }
}
