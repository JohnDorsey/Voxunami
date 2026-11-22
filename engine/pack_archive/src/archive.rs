//! The canonical archive: a set of files, written as one zip that nothing else can spell, and read back only from that spelling.

use std::collections::BTreeMap;

use crate::error::ArchiveError;

const LOCAL_SIGNATURE: u32 = 0x0403_4b50;
const CENTRAL_SIGNATURE: u32 = 0x0201_4b50;
const END_SIGNATURE: u32 = 0x0605_4b50;
const LOCAL_HEADER_LENGTH: usize = 30;
const CENTRAL_HEADER_LENGTH: usize = 46;
const END_RECORD_LENGTH: usize = 22;
/// 1980-01-01, the first date a zip can hold; the time of day is 00:00.
const FIXED_DATE: u16 = 0x0021;
/// The version "2.0" (stored entries), made by MS-DOS (the high byte 0).
const VERSION: u16 = 20;
const MAX_PATH_LENGTH: usize = 255;
/// What the format can hold without zip64: the count is a 16-bit field (the value 65535 means zip64) and every offset a 32-bit one (0xFFFFFFFF means zip64).
const FORMAT_MAX_FILES: usize = 65_534;
const FORMAT_MAX_BYTES: u64 = 4_000_000_000;

/// How much the reader will hold. A canonical archive says every size in its directory, so the limits are checked before a byte of a file is copied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    pub max_files: usize,
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
}

impl Default for Limits {
    /// 10,000 files, 256 MiB for one, 1 GiB in all: far above any pack seen so far (Cave's is 24 KB) and far below what could hurt a machine.
    fn default() -> Self {
        Limits {
            max_files: 10_000,
            max_file_bytes: 256 << 20,
            max_total_bytes: 1 << 30,
        }
    }
}

/// A set of files by path, whose archive is one and only one byte sequence.
///
/// The form (the *canonical* one): entries sorted by the bytes of their paths; every entry stored, never compressed; the date fixed at 1980-01-01 00:00; no
/// flags, extra fields, comments, data descriptors or zip64; the central directory right after the last file and the end record right after it, with no
/// comment; paths of only `a-z`, `0-9`, `_`, `-`, `.` and `/` (so two paths can never differ in case alone). [`Archive::from_bytes`] accepts only that and
/// [`Archive::to_bytes`] writes only that, so the same files are the same bytes on every machine and in every order they were given.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Archive {
    files: BTreeMap<String, Vec<u8>>,
}

/// Whether `path` is allowed in an archive, and if not, why: the one rule for a file name in a pack, of a folder or a zip.
pub fn check_path(path: &str) -> Result<(), ArchiveError> {
    let refuse = |reason| {
        Err(ArchiveError::InvalidPath {
            path: path.to_owned(),
            reason,
        })
    };
    if path.is_empty() {
        return refuse("it is empty");
    }
    if path.len() > MAX_PATH_LENGTH {
        return refuse("it is longer than 255 bytes");
    }
    if !path
        .bytes()
        .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.' | b'/'))
    {
        return refuse("it has a character other than a-z, 0-9, _, -, . and /");
    }
    for part in path.split('/') {
        match part {
            "" => return refuse("it has an empty part (a leading, trailing or doubled slash)"),
            "." | ".." => return refuse("it has a part that is . or .."),
            _ => {}
        }
    }
    Ok(())
}

/// The archive keeps files by path; a path that is also the folder of another path is refused.
fn check_folders(files: &BTreeMap<String, Vec<u8>>) -> Result<(), ArchiveError> {
    for path in files.keys() {
        for (at, _) in path.match_indices('/') {
            if files.contains_key(&path[..at]) {
                return Err(ArchiveError::FileAndFolder {
                    path: path[..at].to_owned(),
                });
            }
        }
    }
    Ok(())
}

impl Archive {
    /// The archive of these files, in any order. Refuses a path that is not allowed, a path twice, a path that is a file and a folder, and more than the zip
    /// format holds.
    pub fn from_files(
        files: impl IntoIterator<Item = (String, Vec<u8>)>,
    ) -> Result<Archive, ArchiveError> {
        let mut map = BTreeMap::new();
        let mut total = 0u64;
        for (path, bytes) in files {
            check_path(&path)?;
            total += bytes.len() as u64;
            if map.insert(path.clone(), bytes).is_some() {
                return Err(ArchiveError::Duplicate { path });
            }
        }
        if map.len() > FORMAT_MAX_FILES {
            return Err(ArchiveError::TooManyFiles {
                count: map.len(),
                max: FORMAT_MAX_FILES,
            });
        }
        if total > FORMAT_MAX_BYTES {
            return Err(ArchiveError::TooLarge {
                max: FORMAT_MAX_BYTES,
            });
        }
        check_folders(&map)?;
        Ok(Archive { files: map })
    }

    /// The files, sorted by path.
    pub fn files(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.files
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
    }

    /// The bytes of one file.
    pub fn get(&self, path: &str) -> Option<&[u8]> {
        self.files.get(path).map(Vec::as_slice)
    }

    /// The files for which `keep` is true: what a server sends of an archive that holds files it withholds.
    #[must_use]
    pub fn only(&self, keep: impl Fn(&str) -> bool) -> Archive {
        Archive {
            files: self
                .files
                .iter()
                .filter(|(path, _)| keep(path))
                .map(|(path, bytes)| (path.clone(), bytes.clone()))
                .collect(),
        }
    }

    /// The canonical bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (path, bytes) in &self.files {
            // `from_files` kept the totals below 4 GB and the count below 65,535, so these casts cannot truncate
            let offset = out.len() as u32;
            let crc = crc32fast::hash(bytes);
            let size = bytes.len() as u32;
            let name_length = path.len() as u16;

            out.extend_from_slice(&LOCAL_SIGNATURE.to_le_bytes());
            out.extend_from_slice(&VERSION.to_le_bytes());
            out.extend_from_slice(&[0, 0, 0, 0]); // flags, method (stored)
            out.extend_from_slice(&0u16.to_le_bytes()); // time
            out.extend_from_slice(&FIXED_DATE.to_le_bytes());
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&size.to_le_bytes()); // compressed size
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(&name_length.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // extra length
            out.extend_from_slice(path.as_bytes());
            out.extend_from_slice(bytes);

            central.extend_from_slice(&CENTRAL_SIGNATURE.to_le_bytes());
            central.extend_from_slice(&VERSION.to_le_bytes()); // made by
            central.extend_from_slice(&VERSION.to_le_bytes()); // needed
            central.extend_from_slice(&[0, 0, 0, 0]); // flags, method
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&FIXED_DATE.to_le_bytes());
            central.extend_from_slice(&crc.to_le_bytes());
            central.extend_from_slice(&size.to_le_bytes());
            central.extend_from_slice(&size.to_le_bytes());
            central.extend_from_slice(&name_length.to_le_bytes());
            central.extend_from_slice(&[0; 12]); // extra, comment, disk, internal attributes, external attributes
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(path.as_bytes());
        }
        let directory_offset = out.len() as u32;
        let count = self.files.len() as u16;
        out.extend_from_slice(&central);
        out.extend_from_slice(&END_SIGNATURE.to_le_bytes());
        out.extend_from_slice(&[0; 4]); // this disk, the disk of the directory
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&directory_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // comment length
        out
    }

    /// Reads an archive, accepting the canonical form and nothing else; every other zip is refused with the reason that names it. Never panics, whatever
    /// the bytes, and holds nothing beyond `limits`.
    pub fn from_bytes(bytes: &[u8], limits: &Limits) -> Result<Archive, ArchiveError> {
        let malformed = |reason| ArchiveError::Malformed { reason };
        let end = bytes
            .len()
            .checked_sub(END_RECORD_LENGTH)
            .ok_or_else(|| malformed("shorter than an end record"))?;
        if u32_at(bytes, end) != Some(END_SIGNATURE) {
            return Err(malformed(
                "no end record in the last 22 bytes (an archive comment or bytes after the end?)",
            ));
        }
        let field16 = |at| u16_at(bytes, end + at).unwrap_or(0);
        let field32 = |at| u32_at(bytes, end + at).unwrap_or(0);
        if field16(4) != 0 || field16(6) != 0 {
            return Err(malformed("the archive is split over disks"));
        }
        if field16(8) == u16::MAX
            || field16(10) == u16::MAX
            || field32(12) == u32::MAX
            || field32(16) == u32::MAX
        {
            return Err(ArchiveError::Zip64);
        }
        if field16(8) != field16(10) {
            return Err(malformed("the two counts of entries differ"));
        }
        if field16(20) != 0 {
            return Err(malformed("the archive has a comment"));
        }
        let count = usize::from(field16(10));
        if count > limits.max_files {
            return Err(ArchiveError::TooManyFiles {
                count,
                max: limits.max_files,
            });
        }
        let directory_offset = field32(16) as usize;
        let directory_size = field32(12) as usize;
        if directory_offset.checked_add(directory_size) != Some(end) {
            return Err(malformed(
                "the central directory is not where the canonical form puts it",
            ));
        }

        let mut files = BTreeMap::new();
        let mut total = 0u64;
        let mut cursor = directory_offset;
        let mut previous: Option<String> = None;
        for _ in 0..count {
            let header = bytes
                .get(cursor..cursor + CENTRAL_HEADER_LENGTH)
                .filter(|_| cursor + CENTRAL_HEADER_LENGTH <= end)
                .ok_or_else(|| malformed("the central directory ends inside an entry"))?;
            if u32_at(header, 0) != Some(CENTRAL_SIGNATURE) {
                return Err(malformed(
                    "an entry of the central directory has no signature",
                ));
            }
            let at16 = |at| u16_at(header, at).unwrap_or(0);
            let at32 = |at| u32_at(header, at).unwrap_or(0);
            let name_length = usize::from(at16(28));
            let name_bytes = bytes
                .get(cursor + CENTRAL_HEADER_LENGTH..cursor + CENTRAL_HEADER_LENGTH + name_length)
                .filter(|_| cursor + CENTRAL_HEADER_LENGTH + name_length <= end)
                .ok_or_else(|| malformed("a name runs past the central directory"))?;
            let path = match std::str::from_utf8(name_bytes) {
                Ok(path) => path.to_owned(),
                Err(_) => {
                    return Err(ArchiveError::InvalidPath {
                        path: String::from_utf8_lossy(name_bytes).into_owned(),
                        reason: "it is not text",
                    });
                }
            };
            check_path(&path)?;
            cursor += CENTRAL_HEADER_LENGTH + name_length;

            if at32(20) == u32::MAX || at32(24) == u32::MAX || at32(42) == u32::MAX {
                return Err(ArchiveError::Zip64);
            }
            if at16(30) != 0 {
                return Err(ArchiveError::ExtraField { path });
            }
            if at16(32) != 0 {
                return Err(ArchiveError::Comment { path });
            }
            if at16(10) != 0 {
                return Err(ArchiveError::Compressed { path });
            }
            if at16(8) != 0 {
                return Err(ArchiveError::Flags { path });
            }
            let size = u64::from(at32(24));
            if u64::from(at32(20)) != size {
                return Err(malformed(
                    "an entry says its stored size differs from its size",
                ));
            }
            if size > limits.max_file_bytes {
                return Err(ArchiveError::FileTooLarge {
                    path,
                    size,
                    max: limits.max_file_bytes,
                });
            }
            total += size;
            if total > limits.max_total_bytes {
                return Err(ArchiveError::TooLarge {
                    max: limits.max_total_bytes,
                });
            }
            match &previous {
                Some(before) if *before == path => return Err(ArchiveError::Duplicate { path }),
                Some(before) if *before > path => return Err(ArchiveError::NotSorted { path }),
                _ => {}
            }

            // the local header and the data
            let local = at32(42) as usize;
            let data_start = local
                .checked_add(LOCAL_HEADER_LENGTH)
                .and_then(|after_header| after_header.checked_add(name_length))
                .filter(|start| *start <= directory_offset)
                .ok_or_else(|| malformed("an entry starts outside the archive"))?;
            let data_end = data_start
                .checked_add(size as usize)
                .filter(|stop| *stop <= directory_offset)
                .ok_or_else(|| malformed("an entry runs past the files"))?;
            let local_header = &bytes[local..local + LOCAL_HEADER_LENGTH];
            if u32_at(local_header, 0) != Some(LOCAL_SIGNATURE) {
                return Err(malformed(
                    "an entry has no local header where the directory says",
                ));
            }
            if u16_at(local_header, 6) != Some(0) {
                return Err(ArchiveError::Flags { path });
            }
            if u16_at(local_header, 28) != Some(0) {
                return Err(ArchiveError::ExtraField { path });
            }
            if usize::from(u16_at(local_header, 26).unwrap_or(0)) != name_length {
                return Err(malformed(
                    "a local header names an entry with another length",
                ));
            }
            let data = &bytes[data_start..data_end];
            if crc32fast::hash(data) != at32(16) {
                return Err(ArchiveError::Checksum { path });
            }
            files.insert(path.clone(), data.to_vec());
            previous = Some(path);
        }
        if cursor != end {
            return Err(malformed(
                "the central directory has bytes beyond its entries",
            ));
        }
        check_folders(&files)?;
        let archive = Archive { files };

        // Everything above understood the fields; this decides whether the bytes are the writer's, which is the whole of "canonical".
        let canonical = archive.to_bytes();
        if canonical != bytes {
            let offset = canonical
                .iter()
                .zip(bytes)
                .position(|(a, b)| a != b)
                .unwrap_or(canonical.len().min(bytes.len()));
            return Err(ArchiveError::NotCanonical { offset });
        }
        Ok(archive)
    }
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    let end = at.checked_add(2)?;
    Some(u16::from_le_bytes(bytes.get(at..end)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let end = at.checked_add(4)?;
    Some(u32::from_le_bytes(bytes.get(at..end)?.try_into().ok()?))
}
