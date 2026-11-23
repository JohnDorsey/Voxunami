//! What the messages say of packs: the list of a server's replicated packs, and the pieces of a pack's archive sent to a client that asks for it
//! (`docs/specs/pack-archive-spec.md`, step 4).

use binary_codec::{ByteReader, ByteWriter, DecodeError};
use pack_archive::{FileIndex, Hash, Limits, cache_file_name};

/// The most packs one list or one request may hold.
pub const MAXIMUM_PACKS: usize = 256;

/// The most bytes of an archive one `PackChunk` carries. A pack is sent in pieces so that one frame never holds a whole pack, and the server sends only so many
/// bytes a tick.
pub const MAXIMUM_PACK_CHUNK: usize = 64 * 1024;

/// The most bytes of a pack's archive a client is told of or sent: what a canonical archive may hold in all.
pub fn maximum_pack_bytes() -> u64 {
    Limits::default().max_total_bytes
}

/// One pack of a server's list: which pack, what is in it, and how big the archive is that a client that lacks it is sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackListing {
    /// The pack's id (its namespace): 1 to 64 characters of `a-z 0-9 _`, which is also the start of its cached file's name.
    pub id: String,
    /// Every file of the pack with its size and hash and whether clients are sent it. Its root hash is the pack's identity.
    pub index: FileIndex,
    /// The size of the canonical archive of the files that are sent: what `PackChunk`s add up to.
    pub archive_size: u64,
}

impl PackListing {
    /// The pack's identity, the hash of its index.
    pub fn root(&self) -> Hash {
        self.index.root_hash()
    }
}

pub(crate) fn write_listing(writer: &mut ByteWriter, listing: &PackListing) {
    writer.write_string(&listing.id);
    writer.write_bytes(&listing.index.to_bytes());
    writer.write_varint(listing.archive_size);
}

pub(crate) fn read_listing(reader: &mut ByteReader<'_>) -> Result<PackListing, DecodeError> {
    let id = reader.read_string()?.to_owned();
    // (The one rule for an id: the file name it makes in a cache.)
    if cache_file_name(&id, &Hash::from_bytes([0; 32])).is_err() {
        return Err(DecodeError::Invalid(
            "a pack's id is not 1 to 64 characters of a-z, 0-9 and _",
        ));
    }
    let index = FileIndex::from_bytes(reader.read_bytes()?)
        .map_err(|_| DecodeError::Invalid("a pack's file index is not the canonical one"))?;
    let archive_size = reader.read_varint()?;
    if archive_size > maximum_pack_bytes() {
        return Err(DecodeError::Invalid(
            "a pack's archive is larger than a client holds",
        ));
    }
    Ok(PackListing {
        id,
        index,
        archive_size,
    })
}

/// A list of packs: at most [`MAXIMUM_PACKS`], each id once.
pub(crate) fn write_listings(writer: &mut ByteWriter, listings: &[PackListing]) {
    writer.write_varint(listings.len() as u64);
    for listing in listings {
        write_listing(writer, listing);
    }
}

pub(crate) fn read_listings(reader: &mut ByteReader<'_>) -> Result<Vec<PackListing>, DecodeError> {
    let count = reader.read_count(2 + 1 + 1)?;
    if count > MAXIMUM_PACKS {
        return Err(DecodeError::Invalid(
            "a list has more packs than a server may have",
        ));
    }
    let mut listings: Vec<PackListing> = Vec::with_capacity(count);
    for _ in 0..count {
        let listing = read_listing(reader)?;
        if listings.iter().any(|earlier| earlier.id == listing.id) {
            return Err(DecodeError::Invalid("a pack is listed twice"));
        }
        listings.push(listing);
    }
    Ok(listings)
}

/// The pieces a pack's archive is sent in, in order: `(offset, bytes)` of at most [`MAXIMUM_PACK_CHUNK`] each. An empty archive has none.
pub fn pack_chunks(archive: &[u8]) -> impl Iterator<Item = (u64, &[u8])> {
    archive
        .chunks(MAXIMUM_PACK_CHUNK)
        .enumerate()
        .map(|(index, bytes)| ((index * MAXIMUM_PACK_CHUNK) as u64, bytes))
}

/// A listing of a pack made of these files, for tests: those under `code/` are withheld.
#[cfg(test)]
pub(crate) fn sample_listing(id: &str, files: &[(&str, &[u8])]) -> PackListing {
    let archive = pack_archive::Archive::from_files(
        files
            .iter()
            .map(|(path, bytes)| ((*path).to_owned(), bytes.to_vec())),
    )
    .unwrap();
    let sent = archive.only(|path| !path.starts_with("code/"));
    PackListing {
        id: id.to_owned(),
        index: FileIndex::of(&archive, |path| !path.starts_with("code/")),
        archive_size: sent.to_bytes().len() as u64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written(listing: &PackListing) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        write_listing(&mut writer, listing);
        writer.into_bytes()
    }

    fn read_whole(bytes: &[u8]) -> Result<PackListing, DecodeError> {
        let mut reader = ByteReader::new(bytes);
        let listing = read_listing(&mut reader)?;
        reader.finish()?;
        Ok(listing)
    }

    #[test]
    fn a_listing_comes_back_equal_and_its_root_is_the_roots_of_its_index() {
        let listing = sample_listing(
            "my_pack",
            &[
                ("pack.json", b"{}"),
                ("blocks/a.json", b"A"),
                ("code/rules.py", b"secret"),
            ],
        );
        let back = read_whole(&written(&listing)).unwrap();
        assert_eq!(back, listing);
        assert_eq!(back.root(), listing.index.root_hash());
        // the withheld file is in the index, which a client sees, and not in the size of what it is sent
        assert!(listing.index.entries().iter().any(|entry| !entry.sent));
    }

    #[test]
    fn a_listing_with_a_bad_id_or_a_bad_index_or_too_big_an_archive_is_refused() {
        let good = sample_listing("my_pack", &[("pack.json", b"{}")]);
        for bad_id in ["", "My_Pack", "my pack", "a-b", &"a".repeat(65)] {
            let mut listing = good.clone();
            listing.id = bad_id.to_owned();
            assert!(read_whole(&written(&listing)).is_err(), "{bad_id:?}");
        }
        let mut writer = ByteWriter::new();
        writer.write_string("my_pack");
        writer.write_bytes(b"not an index");
        writer.write_varint(10);
        assert!(read_whole(&writer.into_bytes()).is_err());
        let mut too_big = good.clone();
        too_big.archive_size = maximum_pack_bytes() + 1;
        assert!(read_whole(&written(&too_big)).is_err());
        let mut as_big = good;
        as_big.archive_size = maximum_pack_bytes();
        assert!(read_whole(&written(&as_big)).is_ok());
    }

    #[test]
    fn a_list_refuses_a_pack_twice_and_more_than_the_most() {
        let one = sample_listing("one", &[("pack.json", b"{}")]);
        let two = sample_listing("two", &[("pack.json", b"{\"id\":\"two\"}")]);
        let mut writer = ByteWriter::new();
        write_listings(&mut writer, &[one.clone(), two.clone()]);
        let bytes = writer.into_bytes();
        assert_eq!(
            read_listings(&mut ByteReader::new(&bytes)).unwrap(),
            [one.clone(), two]
        );
        let mut twice = ByteWriter::new();
        write_listings(&mut twice, &[one.clone(), one.clone()]);
        assert!(read_listings(&mut ByteReader::new(&twice.into_bytes())).is_err());
        let mut many = ByteWriter::new();
        let listings: Vec<PackListing> = (0..=MAXIMUM_PACKS)
            .map(|number| PackListing {
                id: format!("p{number}"),
                ..one.clone()
            })
            .collect();
        write_listings(&mut many, &listings);
        assert!(read_listings(&mut ByteReader::new(&many.into_bytes())).is_err());
    }

    #[test]
    fn an_archive_is_sent_in_pieces_that_put_together_are_the_archive() {
        let archive: Vec<u8> = (0..(MAXIMUM_PACK_CHUNK * 2 + 5))
            .map(|index| (index % 251) as u8)
            .collect();
        let pieces: Vec<(u64, &[u8])> = pack_chunks(&archive).collect();
        assert_eq!(pieces.len(), 3);
        assert_eq!(pieces[0].0, 0);
        assert_eq!(pieces[1].0, MAXIMUM_PACK_CHUNK as u64);
        assert_eq!(pieces[2].1.len(), 5);
        let whole: Vec<u8> = pieces
            .iter()
            .flat_map(|(_, bytes)| bytes.iter().copied())
            .collect();
        assert_eq!(whole, archive);
        assert_eq!(pack_chunks(&[]).count(), 0);
    }
}
