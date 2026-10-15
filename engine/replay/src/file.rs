//! The replay file: a header, then one checksummed record per frame, appended as they arrive.
//!
//! ```text
//! magic "VOXREPLY" (8 bytes) | format version (u32, currently 1)
//! then, until the end of the file, per frame:
//!     frame length (varint) | CRC-32 of the frame (u32) | the frame
//! ```
//!
//! A frame is one encoded server message exactly as the client received it; the first is always the
//! server's welcome, which names the game and describes the world. Nothing is rewritten after it is
//! appended, so a recording interrupted by a crash is readable up to its last complete record.

use std::io::{self, Read, Write};

use binary_codec::ByteWriter;

const MAGIC: &[u8; 8] = b"VOXREPLY";

/// The version of this file layout. The protocol version inside the welcome is separate.
pub const FORMAT_VERSION: u32 = 1;

/// A frame this long is taken to be damage, not data.
const MAXIMUM_FRAME_LENGTH: u64 = 1 << 28;

#[derive(Debug, thiserror::Error)]
pub enum ReplayError {
    #[error("this is not a Voxunami replay file")]
    NotAReplay,
    #[error("the replay file has format version {0}, which this program does not understand")]
    UnsupportedFormatVersion(u32),
    #[error("the replay ends in the middle of frame {record} (counting from 0)")]
    Truncated { record: u64 },
    #[error("frame {record} (counting from 0) is damaged")]
    Damaged { record: u64 },
    #[error("the recording does not start with the server's welcome")]
    NoWelcome,
    #[error("reading the replay failed: {0}")]
    Io(#[from] io::Error),
}

/// Appends frames to a replay file.
#[derive(Debug)]
pub struct ReplayWriter<W: Write> {
    output: W,
    frames_written: u64,
}

impl<W: Write> ReplayWriter<W> {
    /// Starts a replay on `output`, writing its header.
    pub fn new(mut output: W) -> io::Result<Self> {
        output.write_all(MAGIC)?;
        output.write_all(&FORMAT_VERSION.to_le_bytes())?;
        Ok(Self {
            output,
            frames_written: 0,
        })
    }

    pub fn append(&mut self, frame: &[u8]) -> io::Result<()> {
        let mut header = ByteWriter::new();
        header.write_varint(frame.len() as u64);
        header.write_u32(crc32fast::hash(frame));
        self.output.write_all(&header.into_bytes())?;
        self.output.write_all(frame)?;
        self.frames_written += 1;
        Ok(())
    }

    pub fn frames_written(&self) -> u64 {
        self.frames_written
    }

    /// Hands what has been appended to the operating system, so a crash of this program afterwards
    /// loses nothing of it.
    pub fn flush(&mut self) -> io::Result<()> {
        self.output.flush()
    }

    pub fn into_inner(self) -> W {
        self.output
    }
}

/// Reads the frames of a replay file in order.
#[derive(Debug)]
pub struct ReplayReader<R: Read> {
    input: R,
    frames_read: u64,
}

impl<R: Read> ReplayReader<R> {
    /// Checks the header.
    pub fn new(mut input: R) -> Result<Self, ReplayError> {
        let mut header = [0u8; 12];
        match input.read_exact(&mut header) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
                return Err(ReplayError::NotAReplay);
            }
            Err(error) => return Err(error.into()),
        }
        if &header[..8] != MAGIC {
            return Err(ReplayError::NotAReplay);
        }
        let version = u32::from_le_bytes(header[8..].try_into().expect("four bytes"));
        if version != FORMAT_VERSION {
            return Err(ReplayError::UnsupportedFormatVersion(version));
        }
        Ok(Self {
            input,
            frames_read: 0,
        })
    }

    /// The next frame, or `None` at a clean end of the file. A record cut short by the end of the
    /// file is [`ReplayError::Truncated`] (what an interrupted recording looks like); a record whose
    /// checksum fails is [`ReplayError::Damaged`]. Frames before the problem were returned intact.
    pub fn next_frame(&mut self) -> Result<Option<Vec<u8>>, ReplayError> {
        let record = self.frames_read;
        let Some(length) = self.read_length(record)? else {
            return Ok(None);
        };
        if length > MAXIMUM_FRAME_LENGTH {
            return Err(ReplayError::Damaged { record });
        }
        let mut checksum = [0u8; 4];
        self.read_rest_of_record(&mut checksum, record)?;
        let mut frame = vec![0u8; length as usize];
        self.read_rest_of_record(&mut frame, record)?;
        if crc32fast::hash(&frame) != u32::from_le_bytes(checksum) {
            return Err(ReplayError::Damaged { record });
        }
        self.frames_read += 1;
        Ok(Some(frame))
    }

    pub fn frames_read(&self) -> u64 {
        self.frames_read
    }

    /// The varint at the start of a record; `None` if the file ends exactly here.
    fn read_length(&mut self, record: u64) -> Result<Option<u64>, ReplayError> {
        let mut length = 0u64;
        for position in 0..10 {
            let mut byte = [0u8; 1];
            match self.input.read_exact(&mut byte) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
                    return if position == 0 {
                        Ok(None)
                    } else {
                        Err(ReplayError::Truncated { record })
                    };
                }
                Err(error) => return Err(error.into()),
            }
            let payload = u64::from(byte[0] & 0x7F);
            if position == 9 && payload > 1 {
                return Err(ReplayError::Damaged { record });
            }
            length |= payload << (7 * position);
            if byte[0] & 0x80 == 0 {
                return Ok(Some(length));
            }
        }
        Err(ReplayError::Damaged { record })
    }

    fn read_rest_of_record(&mut self, buffer: &mut [u8], record: u64) -> Result<(), ReplayError> {
        self.input.read_exact(buffer).map_err(|error| {
            if error.kind() == io::ErrorKind::UnexpectedEof {
                ReplayError::Truncated { record }
            } else {
                error.into()
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn write_replay(frames: &[Vec<u8>]) -> Vec<u8> {
        let mut writer = ReplayWriter::new(Vec::new()).unwrap();
        for frame in frames {
            writer.append(frame).unwrap();
        }
        assert_eq!(writer.frames_written(), frames.len() as u64);
        writer.into_inner()
    }

    fn read_all(bytes: &[u8]) -> (Vec<Vec<u8>>, Option<ReplayError>) {
        let mut reader = ReplayReader::new(Cursor::new(bytes)).expect("a valid header");
        let mut frames = Vec::new();
        loop {
            match reader.next_frame() {
                Ok(Some(frame)) => frames.push(frame),
                Ok(None) => return (frames, None),
                Err(error) => return (frames, Some(error)),
            }
        }
    }

    fn sample_frames() -> Vec<Vec<u8>> {
        vec![
            vec![1, 2, 3],
            Vec::new(),
            (0..200u8).collect(),
            vec![0xFF; 300], // a length that needs two varint bytes
        ]
    }

    #[test]
    fn frames_come_back_unchanged_and_in_order() {
        let bytes = write_replay(&sample_frames());
        let (frames, problem) = read_all(&bytes);
        assert_eq!(frames, sample_frames());
        assert!(problem.is_none());
        assert_eq!(read_all(&write_replay(&[])).0, Vec::<Vec<u8>>::new());
    }

    #[test]
    fn the_layout_is_pinned() {
        let bytes = write_replay(&[vec![7, 8]]);
        assert_eq!(&bytes[..12], b"VOXREPLY\x01\x00\x00\x00");
        assert_eq!(bytes[12], 2, "the frame length");
        assert_eq!(&bytes[13..17], crc32fast::hash(&[7, 8]).to_le_bytes());
        assert_eq!(&bytes[17..], [7, 8]);
    }

    #[test]
    fn a_file_that_is_not_a_replay_or_is_from_the_future_is_refused() {
        assert!(matches!(
            ReplayReader::new(Cursor::new(b"not a replay file at all".to_vec())),
            Err(ReplayError::NotAReplay)
        ));
        assert!(matches!(
            ReplayReader::new(Cursor::new(Vec::new())),
            Err(ReplayError::NotAReplay)
        ));
        let mut future = write_replay(&[]);
        future[8] = 2;
        assert!(matches!(
            ReplayReader::new(Cursor::new(future)),
            Err(ReplayError::UnsupportedFormatVersion(2))
        ));
    }

    #[test]
    fn a_recording_cut_off_anywhere_yields_every_complete_frame_and_then_says_it_was_cut() {
        let frames = sample_frames();
        let bytes = write_replay(&frames);
        // Where each record ends, so the cut positions that fall exactly between records can be told
        // from the ones that fall inside a record.
        let mut record_ends = vec![12usize];
        for frame in &frames {
            let mut one = ByteWriter::new();
            one.write_varint(frame.len() as u64);
            record_ends
                .push(record_ends.last().unwrap() + one.into_bytes().len() + 4 + frame.len());
        }
        assert_eq!(*record_ends.last().unwrap(), bytes.len());

        for cut in 12..=bytes.len() {
            let (read, problem) = read_all(&bytes[..cut]);
            let complete = record_ends
                .iter()
                .skip(1)
                .filter(|&&end| end <= cut)
                .count();
            assert_eq!(read, frames[..complete], "cut at {cut}");
            if record_ends.contains(&cut) {
                assert!(problem.is_none(), "cut at {cut} is between records");
            } else {
                assert!(
                    matches!(problem, Some(ReplayError::Truncated { record }) if record == complete as u64),
                    "cut at {cut}: {problem:?}"
                );
            }
        }
    }

    #[test]
    fn changing_any_single_byte_of_a_record_is_noticed() {
        let frames = sample_frames();
        let bytes = write_replay(&frames);
        for position in 12..bytes.len() {
            for flip in [0x01u8, 0x80] {
                let mut damaged = bytes.clone();
                damaged[position] ^= flip;
                let (read, problem) = read_all(&damaged);
                assert!(
                    problem.is_some() || read != frames,
                    "byte {position} ^ {flip:#x} went unnoticed"
                );
                // Whatever was read before the problem is a prefix of the truth.
                assert_eq!(read, frames[..read.len()], "byte {position}");
            }
        }
    }

    #[test]
    fn an_absurd_length_is_damage_not_an_allocation() {
        let mut bytes = write_replay(&[]);
        let mut length = ByteWriter::new();
        length.write_varint(u64::MAX >> 1);
        bytes.extend(length.into_bytes());
        let (_, problem) = read_all(&bytes);
        assert!(matches!(problem, Some(ReplayError::Damaged { record: 0 })));
    }
}
