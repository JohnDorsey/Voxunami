//! The byte form of an interface, for the messages that carry one. Canonical (one interface, one byte string), and a decoder
//! that refuses anything the limits refuse before it has built it, so a hostile message costs a little work, not a lot.

use binary_codec::{ByteReader, ByteWriter, DecodeError};

use crate::model::{
    Anchor, Color, Direction, MAXIMUM_DEPTH, MAXIMUM_NODES, Node, Panel, Ui, UiError,
};

const PANEL: u8 = 0;
const LABEL: u8 = 1;
const BUTTON: u8 = 2;
const BAR: u8 = 3;
const SWATCH: u8 = 4;
const SPACE: u8 = 5;

/// The least bytes a node takes: its tag and something more.
const NODE_BYTES: usize = 2;

fn write_color(writer: &mut ByteWriter, color: Color) {
    writer.write_raw(&color.0);
}

fn read_color(reader: &mut ByteReader<'_>) -> Result<Color, DecodeError> {
    let bytes = reader.read_raw(3)?;
    Ok(Color([bytes[0], bytes[1], bytes[2]]))
}

fn read_flag(reader: &mut ByteReader<'_>) -> Result<bool, DecodeError> {
    match reader.read_u8()? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(DecodeError::Invalid("a flag is neither 0 nor 1")),
    }
}

fn write_panel(writer: &mut ByteWriter, panel: &Panel) {
    match &panel.title {
        Some(title) => {
            writer.write_u8(1);
            writer.write_string(title);
        }
        None => writer.write_u8(0),
    }
    write_color(writer, panel.color);
    writer.write_u8(match panel.direction {
        Direction::Row => 0,
        Direction::Column => 1,
    });
    writer.write_varint(panel.children.len() as u64);
    for child in &panel.children {
        write_node(writer, child);
    }
}

fn write_node(writer: &mut ByteWriter, node: &Node) {
    match node {
        Node::Panel(panel) => {
            writer.write_u8(PANEL);
            write_panel(writer, panel);
        }
        Node::Label { text, color } => {
            writer.write_u8(LABEL);
            writer.write_string(text);
            write_color(writer, *color);
        }
        Node::Button { id, text } => {
            writer.write_u8(BUTTON);
            writer.write_varint(u64::from(*id));
            writer.write_string(text);
        }
        Node::Bar {
            permille,
            cells,
            color,
        } => {
            writer.write_u8(BAR);
            writer.write_varint(u64::from(*permille));
            writer.write_varint(u64::from(*cells));
            write_color(writer, *color);
        }
        Node::Swatch {
            color,
            count,
            selected,
        } => {
            writer.write_u8(SWATCH);
            write_color(writer, *color);
            match count {
                Some(count) => {
                    writer.write_u8(1);
                    writer.write_varint(u64::from(*count));
                }
                None => writer.write_u8(0),
            }
            writer.write_u8(u8::from(*selected));
        }
        Node::Space(size) => {
            writer.write_u8(SPACE);
            writer.write_varint(u64::from(*size));
        }
    }
}

struct Reading {
    nodes: usize,
}

fn read_panel(
    reader: &mut ByteReader<'_>,
    depth: usize,
    reading: &mut Reading,
) -> Result<Panel, DecodeError> {
    if depth > MAXIMUM_DEPTH {
        return Err(DecodeError::Invalid(UiError::TooDeep.reason()));
    }
    let title = if read_flag(reader)? {
        Some(reader.read_string()?.to_owned())
    } else {
        None
    };
    let color = read_color(reader)?;
    let direction = match reader.read_u8()? {
        0 => Direction::Row,
        1 => Direction::Column,
        _ => {
            return Err(DecodeError::Invalid(
                "a direction is neither row nor column",
            ));
        }
    };
    let count = reader.read_count(NODE_BYTES)?;
    let mut children = Vec::with_capacity(count.min(MAXIMUM_NODES));
    for _ in 0..count {
        reading.nodes += 1;
        if reading.nodes > MAXIMUM_NODES {
            return Err(DecodeError::Invalid(UiError::TooManyNodes.reason()));
        }
        children.push(read_node(reader, depth, reading)?);
    }
    Ok(Panel {
        title,
        color,
        direction,
        children,
    })
}

fn read_small(reader: &mut ByteReader<'_>) -> Result<u16, DecodeError> {
    u16::try_from(reader.read_varint()?).map_err(|_| DecodeError::Invalid("a number is too large"))
}

fn read_node(
    reader: &mut ByteReader<'_>,
    depth: usize,
    reading: &mut Reading,
) -> Result<Node, DecodeError> {
    Ok(match reader.read_u8()? {
        PANEL => Node::Panel(read_panel(reader, depth + 1, reading)?),
        LABEL => Node::Label {
            text: reader.read_string()?.to_owned(),
            color: read_color(reader)?,
        },
        BUTTON => Node::Button {
            id: u32::try_from(reader.read_varint()?)
                .map_err(|_| DecodeError::Invalid("a button identifier is too large"))?,
            text: reader.read_string()?.to_owned(),
        },
        BAR => Node::Bar {
            permille: read_small(reader)?,
            cells: read_small(reader)?,
            color: read_color(reader)?,
        },
        SWATCH => Node::Swatch {
            color: read_color(reader)?,
            count: if read_flag(reader)? {
                Some(
                    u32::try_from(reader.read_varint()?)
                        .map_err(|_| DecodeError::Invalid("a count is too large"))?,
                )
            } else {
                None
            },
            selected: read_flag(reader)?,
        },
        SPACE => Node::Space(read_small(reader)?),
        _ => {
            return Err(DecodeError::Invalid(
                "a part is of a kind that is not known",
            ));
        }
    })
}

impl Ui {
    pub fn encode(&self, writer: &mut ByteWriter) {
        writer.write_u8(u8::from(self.modal));
        writer.write_u8(
            Anchor::ALL
                .iter()
                .position(|anchor| *anchor == self.anchor)
                .expect("every anchor is in the list") as u8,
        );
        write_panel(writer, &self.root);
    }

    /// Reads what [`Ui::encode`] wrote, and refuses what the limits refuse.
    pub fn decode(reader: &mut ByteReader<'_>) -> Result<Self, DecodeError> {
        let modal = read_flag(reader)?;
        let anchor = *Anchor::ALL
            .get(usize::from(reader.read_u8()?))
            .ok_or(DecodeError::Invalid("an anchor is not one of the nine"))?;
        let root = read_panel(reader, 1, &mut Reading { nodes: 0 })?;
        let ui = Self {
            modal,
            anchor,
            root,
        };
        ui.check()
            .map_err(|error| DecodeError::Invalid(error.reason()))?;
        Ok(ui)
    }
}
