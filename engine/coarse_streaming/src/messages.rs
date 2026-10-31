//! The two messages of coarse levels of detail (`docs/specs/lod-spec.md`, 3.5), carried as game messages: the client says how it wants far things drawn, and the
//! server sends the coarse nodes that differ from what the client works out for itself.
//!
//! Everything is keyed by the **grid** (`GridId`), never by "the terrain of the level": a ship's nodes are sent the same way, and a ship that has terrain
//! generators of its own (Phase 39, `lod-spec.md` 3.8) is a grid whose unedited cells the client derives.

use binary_codec::{ByteReader, ByteWriter, DecodeError};
use coarse_cells::{
    CELLS_PER_NODE, CoarseCell, CoarseNode, DetailSettings, MAXIMUM_DETAIL_LEVEL, Zoom,
};
use namespaced_id::NamespacedId;
use protocol::{Message, read_brick_coordinate, write_brick_coordinate};
use region_octree::BrickCube;
use voxel_space::{BrickCoordinate, GridId};

fn kind(text: &str) -> NamespacedId {
    text.parse().expect("constant identifier is valid")
}

/// The most cell sizes in pixels that a client may ask for: a cell wider than this is not a picture of anything.
pub const MAXIMUM_CELL_PIXELS: f32 = 64.0;

/// A direction and a width the client looks through a telescope with (see [`coarse_cells::Zoom`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoomRequest {
    /// The way it looks, in the world's axes (the server turns it into the axes of each grid). Any length but 0; the server normalises it.
    pub direction: [f32; 3],
    /// Half the angle of the cone that is drawn finely, in radians, over 0 and not over a right angle.
    pub half_angle: f32,
    /// The size of a pixel inside the cone (see [`DetailSettings::pixel_size`]).
    pub pixel_size: f32,
}

/// Client to server: how this client wants things drawn, and how fast it can be told. Sent when it joins and whenever a setting changes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SetDetail {
    /// How many pixels across a cell may be before it is split (`.detail`).
    pub cell_pixels: f32,
    /// How big a pixel is as a share of the distance: `2 * tan(field_of_view / 2) / viewport_height`.
    pub pixel_size: f32,
    /// The most bytes a second of coarse nodes the client wants. 0 asks for none.
    pub bytes_per_second: u32,
    /// How many bricks the client draws as bricks round the brick it is in (a ball of this radius, with the fringe of a cube of two bricks, see
    /// [`coarse_cells::FineRegion`]): the server leaves out the nodes there, and the client draws no node of a level finer than 1 anywhere.
    pub fine_radius: u8,
    pub zoom: Option<ZoomRequest>,
}

impl SetDetail {
    /// The settings the selection takes.
    pub fn settings(&self) -> DetailSettings {
        DetailSettings {
            cell_pixels: f64::from(self.cell_pixels),
            pixel_size: f64::from(self.pixel_size),
        }
    }

    /// The zoom cone, with its direction made a unit vector and the cosine and sine of its angle worked out with `libm` (the same on every machine).
    pub fn zoom(&self) -> Option<Zoom> {
        let zoom = self.zoom?;
        let direction = zoom.direction.map(f64::from);
        let length = (direction[0] * direction[0]
            + direction[1] * direction[1]
            + direction[2] * direction[2])
            .sqrt();
        let half = f64::from(zoom.half_angle);
        Some(Zoom {
            direction: direction.map(|component| component / length),
            half_angle_cosine: libm::cos(half),
            half_angle_sine: libm::sin(half),
            pixel_size: f64::from(zoom.pixel_size),
        })
    }
}

fn write_f32(writer: &mut ByteWriter, value: f32) {
    writer.write_u32(value.to_bits());
}

fn read_f32(reader: &mut ByteReader<'_>) -> Result<f32, DecodeError> {
    let value = f32::from_bits(reader.read_u32()?);
    if value.is_finite() {
        Ok(value)
    } else {
        Err(DecodeError::Invalid("a number is not finite"))
    }
}

impl Message for SetDetail {
    fn kind() -> NamespacedId {
        kind("engine:set_detail")
    }

    fn encode(&self) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        write_f32(&mut writer, self.cell_pixels);
        write_f32(&mut writer, self.pixel_size);
        writer.write_varint(u64::from(self.bytes_per_second));
        writer.write_u8(self.fine_radius);
        match self.zoom {
            None => writer.write_u8(0),
            Some(zoom) => {
                writer.write_u8(1);
                for component in zoom.direction {
                    write_f32(&mut writer, component);
                }
                write_f32(&mut writer, zoom.half_angle);
                write_f32(&mut writer, zoom.pixel_size);
            }
        }
        writer.into_bytes()
    }

    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = ByteReader::new(payload);
        let cell_pixels = read_f32(&mut reader)?;
        let pixel_size = read_f32(&mut reader)?;
        if !(cell_pixels > 0.0 && cell_pixels <= MAXIMUM_CELL_PIXELS) {
            return Err(DecodeError::Invalid("the size of a cell is not sensible"));
        }
        if !(pixel_size > 0.0 && pixel_size <= 1.0) {
            return Err(DecodeError::Invalid("the size of a pixel is not sensible"));
        }
        let bytes_per_second = u32::try_from(reader.read_varint()?)
            .map_err(|_| DecodeError::Invalid("a rate is out of range"))?;
        let fine_radius = reader.read_u8()?;
        let zoom = match reader.read_u8()? {
            0 => None,
            1 => {
                let direction = [
                    read_f32(&mut reader)?,
                    read_f32(&mut reader)?,
                    read_f32(&mut reader)?,
                ];
                let half_angle = read_f32(&mut reader)?;
                let zoom_pixel_size = read_f32(&mut reader)?;
                let length_squared: f32 = direction
                    .iter()
                    .map(|component| component * component)
                    .sum();
                if length_squared <= 0.0 {
                    return Err(DecodeError::Invalid("a zoom has no direction"));
                }
                if !(half_angle > 0.0 && half_angle <= std::f32::consts::FRAC_PI_2) {
                    return Err(DecodeError::Invalid(
                        "a zoom has an angle that is not sensible",
                    ));
                }
                if !(zoom_pixel_size > 0.0 && zoom_pixel_size <= 1.0) {
                    return Err(DecodeError::Invalid(
                        "a zoom has a pixel that is not sensible",
                    ));
                }
                Some(ZoomRequest {
                    direction,
                    half_angle,
                    pixel_size: zoom_pixel_size,
                })
            }
            _ => return Err(DecodeError::Invalid("a zoom flag is neither 0 nor 1")),
        };
        reader.finish()?;
        Ok(Self {
            cell_pixels,
            pixel_size,
            bytes_per_second,
            fine_radius,
            zoom,
        })
    }

    fn samples() -> Vec<Self> {
        vec![
            Self {
                cell_pixels: 3.0,
                pixel_size: 0.001_297,
                bytes_per_second: 250_000,
                fine_radius: 12,
                zoom: None,
            },
            Self {
                cell_pixels: 0.5,
                pixel_size: 1.0,
                bytes_per_second: 0,
                fine_radius: 0,
                zoom: Some(ZoomRequest {
                    direction: [0.0, -0.5, 2.0],
                    half_angle: 0.05,
                    pixel_size: 0.000_13,
                }),
            },
            Self {
                cell_pixels: MAXIMUM_CELL_PIXELS,
                pixel_size: 0.000_001,
                bytes_per_second: u32::MAX,
                fine_radius: u8::MAX,
                zoom: Some(ZoomRequest {
                    direction: [1.0, 0.0, 0.0],
                    half_angle: std::f32::consts::FRAC_PI_2,
                    pixel_size: 0.000_000_1,
                }),
            },
        ]
    }
}

/// One node the server says the client has to use in place of the one it would derive, or (with `node` of `None`) no longer: the client goes back to
/// deriving it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoarseNodeUpdate {
    pub grid: GridId,
    /// A cube of bricks of detail level 1 or more.
    pub cube: BrickCube,
    /// Its cells, without their exposure (which is working data for making parents and is not sent).
    pub node: Option<CoarseNode>,
}

/// Server to client: nodes that differ from what the client derives, coarsest first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoarseNodes(pub Vec<CoarseNodeUpdate>);

/// The most updates in a message; a client refuses more.
pub const MAXIMUM_UPDATES: usize = 256;

const NO_NODE: u8 = 0;
const UNIFORM: u8 = 1;
const RUNS: u8 = 2;

fn write_cell(writer: &mut ByteWriter, cell: CoarseCell) {
    writer.write_varint(u64::from(cell.block));
    writer.write_u8(cell.fill);
    writer.write_u8(cell.light);
}

fn read_cell(reader: &mut ByteReader<'_>) -> Result<CoarseCell, DecodeError> {
    let block = u32::try_from(reader.read_varint()?)
        .map_err(|_| DecodeError::Invalid("a block number is out of range"))?;
    Ok(CoarseCell {
        block,
        fill: reader.read_u8()?,
        light: reader.read_u8()?,
        exposure: 0,
    })
}

/// The bytes of a node: one cell if they are all the same, else runs of equal cells (a length, then the cell), whose lengths add up to the whole node.
fn write_node(writer: &mut ByteWriter, node: &CoarseNode) {
    if let Some(cell) = node.uniform_cell() {
        writer.write_u8(UNIFORM);
        write_cell(writer, cell);
        return;
    }
    let mut runs: Vec<(u64, CoarseCell)> = Vec::new();
    for cell in node.cells() {
        let cell = CoarseCell {
            exposure: 0,
            ..cell
        };
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

fn read_node(reader: &mut ByteReader<'_>, tag: u8) -> Result<CoarseNode, DecodeError> {
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

impl Message for CoarseNodes {
    fn kind() -> NamespacedId {
        kind("engine:coarse_nodes")
    }

    fn encode(&self) -> Vec<u8> {
        assert!(
            self.0.len() <= MAXIMUM_UPDATES,
            "too many updates for a message"
        );
        let mut writer = ByteWriter::new();
        writer.write_varint(self.0.len() as u64);
        for update in &self.0 {
            writer.write_varint(update.grid.get());
            write_brick_coordinate(&mut writer, update.cube.origin());
            writer.write_u8(update.cube.level());
            match &update.node {
                None => writer.write_u8(NO_NODE),
                Some(node) => write_node(&mut writer, node),
            }
        }
        writer.into_bytes()
    }

    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = ByteReader::new(payload);
        // A grid, a cube (three coordinates and a level) and a form.
        let count = reader.read_count(6)?;
        if count > MAXIMUM_UPDATES {
            return Err(DecodeError::Invalid("too many updates in a message"));
        }
        let mut updates = Vec::with_capacity(count);
        for _ in 0..count {
            let grid = GridId::new(reader.read_varint()?);
            let origin = read_brick_coordinate(&mut reader)?;
            let level = reader.read_u8()?;
            if level == 0 || level > MAXIMUM_DETAIL_LEVEL {
                return Err(DecodeError::Invalid(
                    "a node has a detail level that is not one",
                ));
            }
            let cube = BrickCube::new(origin, level)
                .map_err(|_| DecodeError::Invalid("a node is not aligned to its size"))?;
            let tag = reader.read_u8()?;
            let node = if tag == NO_NODE {
                None
            } else {
                Some(read_node(&mut reader, tag)?)
            };
            updates.push(CoarseNodeUpdate { grid, cube, node });
        }
        reader.finish()?;
        Ok(Self(updates))
    }

    fn samples() -> Vec<Self> {
        let cube = |origin: [i128; 3], level: u8| {
            BrickCube::new(BrickCoordinate::new(origin[0], origin[1], origin[2]), level).unwrap()
        };
        let cell = |block: u32, fill: u8, light: u8| CoarseCell {
            block,
            fill,
            light,
            exposure: 0,
        };
        let mut cells = vec![cell(0, 0, 255); CELLS_PER_NODE];
        for (index, entry) in cells.iter_mut().enumerate().skip(1000).take(2000) {
            *entry = cell(1 + (index / 500) as u32, 255, 40);
        }
        cells[5] = cell(70_000, 128, 3);
        vec![
            Self(vec![]),
            Self(vec![CoarseNodeUpdate {
                grid: GridId::MAIN,
                cube: cube([4, -8, 0], 2),
                node: None,
            }]),
            Self(vec![
                CoarseNodeUpdate {
                    grid: GridId::MAIN,
                    cube: cube([-8, 0, 16], 3),
                    node: Some(CoarseNode::uniform(cell(2, 255, 7))),
                },
                CoarseNodeUpdate {
                    grid: GridId::new(GridId::FIRST_ALLOCATED + 4),
                    cube: cube([0, 0, 0], 40),
                    node: Some(CoarseNode::from_cells(cells)),
                },
            ]),
            // The coarsest nodes there are: the whole world (its origin is the lowest brick), and cubes past the levels of 40 and 64 that were once the limits.
            Self(vec![
                CoarseNodeUpdate {
                    grid: GridId::MAIN,
                    cube: cube([voxel_space::BRICK_COORDINATE_MINIMUM; 3], 116),
                    node: Some(CoarseNode::uniform(cell(1, 128, 255))),
                },
                CoarseNodeUpdate {
                    grid: GridId::MAIN,
                    cube: cube([0, -(1 << 64), 1 << 70], 64),
                    node: Some(CoarseNode::uniform(cell(2, 255, 7))),
                },
                CoarseNodeUpdate {
                    grid: GridId::MAIN,
                    cube: cube([1 << 41, 0, 0], 41),
                    node: None,
                },
            ]),
        ]
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use protocol::check_message;

    use super::*;

    #[test]
    fn both_messages_round_trip_on_their_samples_and_refuse_what_is_cut_short_or_followed_by_more()
    {
        assert_eq!(check_message::<SetDetail>(), Ok(()));
        assert_eq!(check_message::<CoarseNodes>(), Ok(()));
    }

    #[test]
    fn a_node_that_is_one_cell_costs_a_few_bytes_and_runs_of_equal_cells_cost_by_the_run() {
        let one = CoarseNodes(vec![CoarseNodeUpdate {
            grid: GridId::MAIN,
            cube: BrickCube::new(BrickCoordinate::ORIGIN, 1).unwrap(),
            node: Some(CoarseNode::uniform(CoarseCell::empty(0, 255))),
        }]);
        assert!(one.encode().len() < 16, "{}", one.encode().len());
        // Ground below and sky above: two runs, however big the node.
        let cells = (0..CELLS_PER_NODE)
            .map(|index| {
                if index < CELLS_PER_NODE / 2 {
                    CoarseCell {
                        block: 1,
                        fill: 255,
                        light: 0,
                        exposure: 0,
                    }
                } else {
                    CoarseCell::empty(0, 255)
                }
            })
            .collect();
        let two = CoarseNodes(vec![CoarseNodeUpdate {
            grid: GridId::MAIN,
            cube: BrickCube::new(BrickCoordinate::ORIGIN, 1).unwrap(),
            node: Some(CoarseNode::from_cells(cells)),
        }]);
        assert!(two.encode().len() < 24, "{}", two.encode().len());
        assert_eq!(CoarseNodes::decode(&two.encode()), Ok(two));
    }

    #[test]
    fn the_exposure_of_a_cell_is_not_sent_and_comes_back_as_nothing() {
        let sent = CoarseCell {
            block: 3,
            fill: 200,
            light: 9,
            exposure: 4096,
        };
        let update = |node| {
            CoarseNodes(vec![CoarseNodeUpdate {
                grid: GridId::MAIN,
                cube: BrickCube::new(BrickCoordinate::ORIGIN, 4).unwrap(),
                node: Some(node),
            }])
        };
        let got = CoarseNodes::decode(&update(CoarseNode::uniform(sent)).encode()).unwrap();
        assert_eq!(
            got.0[0].node,
            Some(CoarseNode::uniform(CoarseCell {
                exposure: 0,
                ..sent
            }))
        );
        // Two nodes that differ only in exposure are the same bytes.
        assert_eq!(
            update(CoarseNode::uniform(sent)).encode(),
            update(CoarseNode::uniform(CoarseCell {
                exposure: 0,
                ..sent
            }))
            .encode()
        );
    }

    #[test]
    fn nodes_of_many_cells_that_differ_only_in_exposure_are_the_same_bytes_and_runs_follow_the_cells_not_the_exposure()
     {
        let node = |exposure_of: &dyn Fn(usize) -> u32| {
            let cells = (0..CELLS_PER_NODE)
                .map(|index| CoarseCell {
                    block: 1 + (index / 4096) as u32,
                    fill: 255,
                    light: 3,
                    exposure: exposure_of(index),
                })
                .collect();
            CoarseNodes(vec![CoarseNodeUpdate {
                grid: GridId::MAIN,
                cube: BrickCube::new(BrickCoordinate::ORIGIN, 1).unwrap(),
                node: Some(CoarseNode::from_cells(cells)),
            }])
        };
        let flat = node(&|_| 0);
        let ragged = node(&|index| (index % 7) as u32 * 256);
        assert_eq!(flat.encode(), ragged.encode());
        // Eight runs, of 4096 cells.
        assert!(flat.encode().len() < 64, "{}", flat.encode().len());
        assert_eq!(CoarseNodes::decode(&ragged.encode()), Ok(flat));
    }

    #[test]
    fn a_message_with_more_updates_than_the_limit_is_refused() {
        let build = |count: usize| {
            let mut writer = ByteWriter::new();
            writer.write_varint(count as u64);
            for _ in 0..count {
                writer.write_varint(0);
                for component in [0i128, 0, 0] {
                    writer.write_wide_signed(component);
                }
                writer.write_u8(1);
                writer.write_u8(NO_NODE);
            }
            writer.into_bytes()
        };
        assert!(CoarseNodes::decode(&build(MAXIMUM_UPDATES)).is_ok());
        assert!(CoarseNodes::decode(&build(MAXIMUM_UPDATES + 1)).is_err());
    }

    #[test]
    fn damaged_nodes_are_errors() {
        let good = CoarseNodes::samples().pop().unwrap().encode();
        // A cube that is not aligned to its size, a level of 0 or over the maximum, runs that do not fill the node or overfill it.
        let build = |change: &dyn Fn(&mut ByteWriter)| {
            let mut writer = ByteWriter::new();
            writer.write_varint(1);
            writer.write_varint(0);
            change(&mut writer);
            writer.into_bytes()
        };
        let cube = |writer: &mut ByteWriter, x: i128, level: u8| {
            for component in [x, 0, 0] {
                writer.write_wide_signed(component);
            }
            writer.write_u8(level);
        };
        assert!(
            CoarseNodes::decode(&build(&|w| {
                cube(w, 1, 2);
                w.write_u8(NO_NODE);
            }))
            .is_err(),
            "misaligned"
        );
        assert!(
            CoarseNodes::decode(&build(&|w| {
                cube(w, 0, 0);
                w.write_u8(NO_NODE);
            }))
            .is_err(),
            "level 0"
        );
        assert!(
            CoarseNodes::decode(&build(&|w| {
                cube(w, 0, MAXIMUM_DETAIL_LEVEL + 1);
                w.write_u8(NO_NODE);
            }))
            .is_err(),
            "too coarse"
        );
        assert!(
            CoarseNodes::decode(&build(&|w| {
                cube(w, 0, 2);
                w.write_u8(9);
            }))
            .is_err(),
            "unknown form"
        );
        let runs = |lengths: &[u64]| {
            build(&|w| {
                cube(w, 0, 2);
                w.write_u8(RUNS);
                w.write_varint(lengths.len() as u64);
                for &length in lengths {
                    w.write_varint(length);
                    write_cell(w, CoarseCell::empty(0, 1));
                }
            })
        };
        assert!(CoarseNodes::decode(&runs(&[CELLS_PER_NODE as u64])).is_ok());
        assert!(
            CoarseNodes::decode(&runs(&[CELLS_PER_NODE as u64 - 1])).is_err(),
            "short"
        );
        assert!(
            CoarseNodes::decode(&runs(&[CELLS_PER_NODE as u64, 1])).is_err(),
            "long"
        );
        assert!(
            CoarseNodes::decode(&runs(&[0, CELLS_PER_NODE as u64])).is_err(),
            "empty run"
        );
        assert!(CoarseNodes::decode(&good).is_ok());
    }

    #[test]
    fn a_setting_that_is_not_sensible_is_refused() {
        let sample = SetDetail::samples().remove(0);
        for bad in [
            SetDetail {
                cell_pixels: 0.0,
                ..sample
            },
            SetDetail {
                cell_pixels: -1.0,
                ..sample
            },
            SetDetail {
                cell_pixels: MAXIMUM_CELL_PIXELS + 1.0,
                ..sample
            },
            SetDetail {
                pixel_size: 0.0,
                ..sample
            },
            SetDetail {
                pixel_size: 2.0,
                ..sample
            },
            SetDetail {
                cell_pixels: f32::NAN,
                ..sample
            },
            SetDetail {
                pixel_size: f32::INFINITY,
                ..sample
            },
        ] {
            assert!(SetDetail::decode(&bad.encode()).is_err(), "{bad:?}");
        }
        let zoom = |zoom: ZoomRequest| SetDetail {
            zoom: Some(zoom),
            ..sample
        };
        let good = ZoomRequest {
            direction: [0.0, 0.0, -1.0],
            half_angle: 0.1,
            pixel_size: 0.0001,
        };
        assert!(SetDetail::decode(&zoom(good).encode()).is_ok());
        for bad in [
            ZoomRequest {
                direction: [0.0; 3],
                ..good
            },
            ZoomRequest {
                half_angle: 0.0,
                ..good
            },
            ZoomRequest {
                half_angle: 2.0,
                ..good
            },
            ZoomRequest {
                pixel_size: 0.0,
                ..good
            },
            ZoomRequest {
                direction: [f32::NAN, 0.0, 1.0],
                ..good
            },
        ] {
            assert!(SetDetail::decode(&zoom(bad).encode()).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_zoom_becomes_a_unit_direction_and_the_cosine_and_sine_of_its_angle() {
        let detail = SetDetail {
            zoom: Some(ZoomRequest {
                direction: [0.0, 3.0, 4.0],
                half_angle: 0.5,
                pixel_size: 0.0002,
            }),
            ..SetDetail::samples().remove(0)
        };
        let zoom = detail.zoom().unwrap();
        assert!((zoom.direction[1] - 0.6).abs() < 1e-6 && (zoom.direction[2] - 0.8).abs() < 1e-6);
        assert!((zoom.half_angle_cosine - libm::cos(0.5)).abs() < 1e-12);
        assert!((zoom.half_angle_sine - libm::sin(0.5)).abs() < 1e-12);
        assert_eq!(detail.settings().cell_pixels, 3.0);
        assert_eq!(SetDetail::samples().remove(0).zoom(), None);
    }

    proptest! {
        #[test]
        fn decoding_arbitrary_bytes_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..120)) {
            let _ = SetDetail::decode(&bytes);
            let _ = CoarseNodes::decode(&bytes);
        }
    }
}
