//! The bytes of a [`CoarseNode`], for the wire (`coarse_streaming`) and for a file (`world_storage`): one cell if they are all the same, else runs of equal
//! cells. What a cell is written as is the caller's (the wire leaves out the exposure and keeps the game's number of a block; a file keeps the exposure and
//! names its blocks), so the runs and the checks are here once and the cell is a closure.

use binary_codec::{ByteReader, ByteWriter, DecodeError};

use crate::{CELLS_PER_NODE, CoarseCell, CoarseNode};

/// The form of a node whose cells are all the same: that cell.
pub const UNIFORM: u8 = 1;
/// The form of any other node: runs of equal cells, each a length and the cell.
pub const RUNS: u8 = 2;

/// Writes the form of a node and then its cells. `normalized` says what a cell is as far as being equal to another goes (the wire does not tell exposure
/// apart: two cells that differ only in it are one run), and `write_cell` writes it (at least three bytes).
pub fn write_node(
    writer: &mut ByteWriter,
    node: &CoarseNode,
    normalized: impl Fn(CoarseCell) -> CoarseCell,
    mut write_cell: impl FnMut(&mut ByteWriter, CoarseCell),
) {
    if let Some(cell) = node.uniform_cell() {
        writer.write_u8(UNIFORM);
        write_cell(writer, normalized(cell));
        return;
    }
    let mut runs: Vec<(u64, CoarseCell)> = Vec::new();
    for cell in node.cells() {
        let cell = normalized(cell);
        match runs.last_mut() {
            Some((length, last)) if *last == cell => *length += 1,
            _ => runs.push((1, cell)),
        }
    }
    writer.write_u8(RUNS);
    writer.write_varint(runs.len() as u64);
    for (length, cell) in runs {
        writer.write_varint(length);
        write_cell(writer, cell);
    }
}

/// Reads a node of the form `tag` (which the caller has read): cells that fill it exactly, or an error and nothing else. `read_cell` reads a cell of at
/// least three bytes.
pub fn read_node(
    reader: &mut ByteReader<'_>,
    tag: u8,
    mut read_cell: impl FnMut(&mut ByteReader<'_>) -> Result<CoarseCell, DecodeError>,
) -> Result<CoarseNode, DecodeError> {
    match tag {
        UNIFORM => Ok(CoarseNode::uniform(read_cell(reader)?)),
        RUNS => {
            // A run is a length of at least one byte and a cell of at least three.
            let count = reader.read_count(4)?;
            let mut cells = Vec::with_capacity(CELLS_PER_NODE);
            for _ in 0..count {
                let length = reader.read_varint()?;
                let cell = read_cell(reader)?;
                if length == 0 || length > (CELLS_PER_NODE - cells.len()) as u64 {
                    return Err(DecodeError::Invalid("a run of cells is empty or too long"));
                }
                cells.extend(std::iter::repeat_n(cell, length as usize));
            }
            if cells.len() != CELLS_PER_NODE {
                return Err(DecodeError::Invalid("the runs of a node do not fill it"));
            }
            Ok(CoarseNode::from_cells(cells))
        }
        _ => Err(DecodeError::Invalid("a node has an unknown form")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(block: u32, exposure: u32) -> CoarseCell {
        CoarseCell {
            block,
            fill: 7,
            light: 9,
            exposure,
        }
    }

    fn write_exact(writer: &mut ByteWriter, cell: CoarseCell) {
        writer.write_varint(u64::from(cell.block));
        writer.write_u8(cell.fill);
        writer.write_u8(cell.light);
        writer.write_varint(u64::from(cell.exposure));
    }

    fn read_exact(reader: &mut ByteReader<'_>) -> Result<CoarseCell, DecodeError> {
        Ok(CoarseCell {
            block: u32::try_from(reader.read_varint()?)
                .map_err(|_| DecodeError::Invalid("a block number is out of range"))?,
            fill: reader.read_u8()?,
            light: reader.read_u8()?,
            exposure: u32::try_from(reader.read_varint()?)
                .map_err(|_| DecodeError::Invalid("an exposure is out of range"))?,
        })
    }

    fn two_kinds_of_cell() -> CoarseNode {
        let cells = (0..CELLS_PER_NODE)
            .map(|index| {
                if index % 3 == 0 {
                    cell(1, 300)
                } else {
                    cell(2, 5)
                }
            })
            .collect();
        CoarseNode::from_cells(cells)
    }

    #[test]
    fn a_node_keeps_everything_a_cell_has_when_the_caller_writes_it_and_cells_apart_by_exposure_are_apart()
     {
        for node in [
            CoarseNode::uniform(cell(3, 99)),
            CoarseNode::uniform(cell(3, 0)),
            two_kinds_of_cell(),
        ] {
            let mut writer = ByteWriter::new();
            write_node(&mut writer, &node, |cell| cell, write_exact);
            let bytes = writer.into_bytes();
            let mut reader = ByteReader::new(&bytes);
            let tag = reader.read_u8().unwrap();
            let read = read_node(&mut reader, tag, read_exact).unwrap();
            reader.finish().unwrap();
            assert_eq!(read, node);
        }
    }

    #[test]
    fn cells_that_the_caller_does_not_tell_apart_are_one_run() {
        let ignoring_exposure = |cell: CoarseCell| CoarseCell {
            exposure: 0,
            ..cell
        };
        let cells = (0..CELLS_PER_NODE)
            .map(|index| cell(1, index as u32))
            .collect();
        let mut writer = ByteWriter::new();
        write_node(
            &mut writer,
            &CoarseNode::from_cells(cells),
            ignoring_exposure,
            write_exact,
        );
        // The form, a run count of one, the length (4 bytes of varint for 32768), the cell: well under a kilobyte, where distinct cells would be 100 kB.
        assert!(writer.into_bytes().len() < 16);
    }

    #[test]
    fn what_does_not_fill_a_node_exactly_is_an_error() {
        let read = |bytes: &[u8]| {
            let mut reader = ByteReader::new(bytes);
            let tag = reader.read_u8()?;
            read_node(&mut reader, tag, read_exact)
        };
        // An unknown form, and the form of "no node" the wire keeps for itself.
        assert!(read(&[0]).is_err());
        assert!(read(&[9]).is_err());
        // A run of nothing, a run longer than the node, and runs that stop short.
        assert!(read(&[RUNS, 1, 0, 1, 1, 1, 0]).is_err());
        assert!(read(&[RUNS, 1, 0xFF, 0xFF, 0x7F, 1, 1, 1, 0]).is_err());
        assert!(read(&[RUNS, 1, 5, 1, 1, 1, 0]).is_err());
    }
}
