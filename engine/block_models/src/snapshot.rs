//! A readable description of a [`BlockTable`], for golden tests that must show *what* moved when a pin does.
//!
//! A hash of the package's bytes says that something changed; this says which block and which behaviour. Each block is one line: its number
//! and name, its colour, how many states it has, and for state 0 standing in no turn the material of each face, the front, axis, docking and joint
//! faces and how it is placed. Then a digest of everything the table answers for the block, over every state and every one of the 24
//! orientations, so a change the plain fields do not show still moves the line.

use std::fmt::Write;

use voxel_space::FaceDirection;

use crate::{BlockTable, ORIENTATION_COUNT};

/// FNV-1a over `bytes`, the hash the golden sessions use.
pub fn fnv_hash(bytes: &[u8]) -> u64 {
    let mut state: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in bytes {
        state ^= u64::from(byte);
        state = state.wrapping_mul(0x0000_0100_0000_01b3);
    }
    state
}

fn face_name(face: Option<FaceDirection>) -> String {
    face.map_or_else(|| "none".to_string(), |face| format!("{face:?}"))
}

/// Everything the table answers for one block, in every state and orientation, as text (one line each), for finding what moved when a digest does.
pub fn explain_block(table: &BlockTable, block: u32) -> String {
    let mut text = String::new();
    for state in 0..table.state_count(block) {
        writeln!(
            text,
            "state {state} material {} placement {:?}",
            table.material_of(block, state),
            table.placement(block, state)
        )
        .expect("writing to a string");
        for orientation in 0..ORIENTATION_COUNT as u32 {
            let model = table.model_of(block, state, orientation);
            let shape = table.shape(model);
            write!(
                text,
                "o{orientation} solid {} opaque {} sight_through {} light_through {} transparency {} boxes {} front {} axis {} docking {} joint {}",
                table.is_solid(model),
                table.is_opaque_cube(model),
                table.is_sight_through(model),
                table.is_light_through(model),
                table.transparency(model).word(),
                shape.boxes().len(),
                face_name(table.front_face(block, state, orientation)),
                face_name(table.axis_face(block, state, orientation)),
                face_name(table.docking_face(block, state, orientation)),
                face_name(table.joint_face(block, state, orientation)),
            )
            .expect("writing to a string");
            for face in FaceDirection::ALL {
                write!(
                    text,
                    " {}:{}/{:016x}",
                    face.index(),
                    table.material_of_face(block, state, orientation, face),
                    table.hides(model, face),
                )
                .expect("writing to a string");
            }
            text.push('\n');
        }
    }
    text
}

/// One line for each block of `table`, named by `names` in the order of the blocks, and a last line for the palette.
pub fn describe_blocks(table: &BlockTable, names: &[String]) -> String {
    let mut text = String::new();
    for (number, name) in names.iter().enumerate() {
        let block = number as u32;
        let [red, green, blue] = table
            .color(block)
            .map(|channel| (channel * 255.0).round() as u8);
        let faces: Vec<String> = FaceDirection::ALL
            .iter()
            .map(|&face| table.material_of_face(block, 0, 0, face).to_string())
            .collect();
        writeln!(
            text,
            "{number:>2} {name}: colour #{red:02x}{green:02x}{blue:02x}, {} states, state 0 faces [{}], front {}, axis {}, docking {}, joint {}, placement {:?}, behaviour {:016x}",
            table.state_count(block),
            faces.join(" "),
            face_name(table.front_face(block, 0, 0)),
            face_name(table.axis_face(block, 0, 0)),
            face_name(table.docking_face(block, 0, 0)),
            face_name(table.joint_face(block, 0, 0)),
            table.placement(block, 0),
            fnv_hash(explain_block(table, block).as_bytes()),
        )
        .expect("writing to a string");
    }
    let palette = table.palette();
    let palette_bytes: Vec<u8> = palette
        .iter()
        .flat_map(|colour| {
            colour
                .iter()
                .flat_map(|channel| channel.to_bits().to_le_bytes())
        })
        .collect();
    let picture_bytes: Vec<u8> = table
        .palette_pictures()
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    writeln!(
        text,
        "palette: {} colours, digest {:016x}, pictures {} on it, digest {:016x}",
        palette.len(),
        fnv_hash(&palette_bytes),
        table
            .palette_pictures()
            .iter()
            .filter(|word| **word != 0)
            .count(),
        fnv_hash(&picture_bytes)
    )
    .expect("writing to a string");
    text
}
