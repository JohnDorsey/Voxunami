use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::time::{Duration, Instant};

use block_models::{FacePicture, TileRef};
use proptest::prelude::*;
use voxel_space::{FaceDirection, GridId, VoxelCoordinate};

use super::base64::{decode, encode};
use super::*;

// ---- base64 ----

#[test]
fn base64_is_the_standard_one_with_padding() {
    for (bytes, text) in [
        (&b""[..], ""),
        (b"f", "Zg=="),
        (b"fo", "Zm8="),
        (b"foo", "Zm9v"),
        (b"foob", "Zm9vYg=="),
        (b"fooba", "Zm9vYmE="),
        (b"foobar", "Zm9vYmFy"),
        (&[0xfb, 0xff, 0xbf], "+/+/"),
    ] {
        assert_eq!(encode(bytes), text);
        assert_eq!(decode(text).as_deref(), Some(bytes));
    }
}

#[test]
fn base64_that_is_not_base64_is_refused() {
    for text in [
        "Zg=",
        "Zg",
        "Z===",
        "Zm9v====",
        "Zm=v",
        "Zm9!",
        "=Zm9",
        "Zm9v Zg==",
        "Zg==Zm9v",
        "Zm9v\n",
    ] {
        assert_eq!(decode(text), None, "{text:?}");
    }
}

proptest! {
    #[test]
    fn any_bytes_come_back_from_their_base64(bytes in proptest::collection::vec(any::<u8>(), 0..300)) {
        prop_assert_eq!(decode(&encode(&bytes)), Some(bytes));
    }
}

// ---- the messages ----

fn json(line: &[u8]) -> serde_json::Value {
    assert_eq!(line.last(), Some(&b'\n'));
    serde_json::from_slice(&line[..line.len() - 1]).unwrap()
}

#[test]
fn the_games_messages_are_the_lines_the_protocol_describes() {
    assert_eq!(
        ToTool::Hello {
            host: "A Game".into()
        }
        .to_line(),
        b"{\"host\":\"A Game\",\"protocol\":1,\"type\":\"hello\"}\n"
    );
    let tiles = ToTool::Tiles(vec![
        CatalogueTile {
            id: "0:3".into(),
            group: Some("base_blocks".into()),
            files: vec!["stone_top".into(), "stone".into()],
            size: 16,
        },
        CatalogueTile {
            id: "1:0".into(),
            group: None,
            files: vec!["x".into()],
            size: 8,
        },
    ]);
    assert_eq!(
        json(&tiles.to_line()),
        serde_json::json!({"type": "tiles", "tiles": [
            {"id": "0:3", "group": "base_blocks", "files": ["stone_top", "stone"], "size": 16},
            {"id": "1:0", "files": ["x"], "size": 8},
        ]})
    );
    let pointer = ToTool::Pointer(Pointer {
        tile: Some("0:3".into()),
        x: -3,
        y: 19,
        left: true,
        right: false,
        segment: 7,
    });
    assert_eq!(
        json(&pointer.to_line()),
        serde_json::json!({"type": "pointer", "tile": "0:3", "x": -3, "y": 19, "left": true, "right": false, "segment": 7})
    );
    let nothing = ToTool::Pointer(Pointer {
        tile: None,
        x: 0,
        y: 0,
        left: false,
        right: true,
        segment: 0,
    });
    assert_eq!(json(&nothing.to_line())["tile"], serde_json::Value::Null);
}

#[test]
fn the_tools_messages_are_read_and_a_bad_one_says_what_is_wrong() {
    assert_eq!(
        FromTool::parse(br#"{"type":"welcome","protocol":1,"tool":"VirtualAtlas","tile_size":16}"#),
        Ok(FromTool::Welcome {
            tool: "VirtualAtlas".into(),
            tile_size: 16
        })
    );
    assert_eq!(
        FromTool::parse(br#"{"type":"known","ids":["a"],"missing":["b","c"]}"#),
        Ok(FromTool::Known {
            ids: vec!["a".into()],
            missing: vec!["b".into(), "c".into()]
        })
    );
    let pixels = encode_base64(&[1, 2, 3, 4]);
    assert_eq!(
        FromTool::parse(
            format!(r#"{{"type":"tile","id":"a","size":1,"pixels":"{pixels}"}}"#).as_bytes()
        ),
        Ok(FromTool::Tile {
            id: "a".into(),
            size: 1,
            rgba: vec![1, 2, 3, 4]
        })
    );
    assert_eq!(
        FromTool::parse(br#"{"type":"error","text":"no"}"#),
        Ok(FromTool::Error { text: "no".into() })
    );
    assert_eq!(
        FromTool::parse(br#"{"type":"from the future","x":1}"#),
        Ok(FromTool::Unknown)
    );
    for bad in [
        &br#"not json"#[..],
        br#"{"no":"type"}"#,
        br#"{"type":"welcome","tool":"x"}"#,
        br#"{"type":"known","ids":[1],"missing":[]}"#,
        br#"{"type":"tile","id":"a","size":2,"pixels":"AAAA"}"#,
        br#"{"type":"tile","id":"a","size":1,"pixels":"!!!!"}"#,
        br#"{"type":"tile","id":"a","size":-1,"pixels":"AAAAAA=="}"#,
    ] {
        assert!(
            FromTool::parse(bad).is_err(),
            "{}",
            String::from_utf8_lossy(bad)
        );
    }
}

#[test]
fn lines_are_cut_from_whatever_pieces_arrive_and_blank_lines_are_left_out() {
    let mut buffer = LineBuffer::default();
    assert_eq!(buffer.feed(b"one\ntw"), vec![b"one".to_vec()]);
    assert_eq!(buffer.feed(b"o\n\n  \nthree"), vec![b"two".to_vec()]);
    assert_eq!(buffer.feed(b"\n"), vec![b"three".to_vec()]);
    let mut slow = LineBuffer::default();
    let found: Vec<Vec<u8>> = b"a\nbc\n"
        .iter()
        .flat_map(|byte| slow.feed(&[*byte]))
        .collect();
    assert_eq!(found, vec![b"a".to_vec(), b"bc".to_vec()]);
}

#[test]
fn a_line_that_is_too_long_is_thrown_away_with_its_rest_and_the_next_line_is_read() {
    let mut buffer = LineBuffer::default();
    assert!(buffer.feed(&vec![b'x'; 9 * 1024 * 1024]).is_empty());
    assert!(buffer.feed(b"still the long line").is_empty());
    assert_eq!(buffer.feed(b"\nok\n"), vec![b"ok".to_vec()]);
}

// ---- the connection ----

fn listener() -> (TcpListener, SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    (listener, address)
}

fn wait_for<T>(mut look: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(found) = look() {
            return found;
        }
        assert!(
            Instant::now() < deadline,
            "what was waited for did not happen"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn a_connection_sends_what_is_queued_and_hears_the_tool_in_lines() {
    let (listener, address) = listener();
    let mut connection = ToolConnection::connect(address).unwrap();
    let (mut tool, _) = listener.accept().unwrap();
    tool.set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    connection.send(&ToTool::Hello {
        host: "A Game".into(),
    });
    connection.send(&ToTool::Pointer(Pointer {
        tile: None,
        x: 0,
        y: 0,
        left: false,
        right: false,
        segment: 0,
    }));
    connection.poll();
    let mut heard = Vec::new();
    let mut piece = [0u8; 4096];
    while heard.iter().filter(|byte| **byte == b'\n').count() < 2 {
        let count = tool.read(&mut piece).unwrap();
        heard.extend_from_slice(&piece[..count]);
    }
    assert_eq!(
        String::from_utf8(heard).unwrap(),
        "{\"host\":\"A Game\",\"protocol\":1,\"type\":\"hello\"}\n{\"left\":false,\"right\":false,\"segment\":0,\"tile\":null,\"type\":\"pointer\",\"x\":0,\"y\":0}\n"
    );
    // The tool talks back, in two pieces and with a line that is no message between.
    tool.write_all(b"{\"type\":\"error\",\"te").unwrap();
    tool.write_all(b"xt\":\"hm\"}\nnonsense\n").unwrap();
    let mut said = Vec::new();
    wait_for(|| {
        said.extend(connection.poll());
        (said.len() >= 2).then_some(())
    });
    assert_eq!(said[0], Ok(FromTool::Error { text: "hm".into() }));
    assert!(said[1].is_err());
    assert!(connection.is_open());
    drop(tool);
    wait_for(|| {
        connection.poll();
        (!connection.is_open()).then_some(())
    });
    // After that nothing is sent and nothing is heard, and nothing goes wrong.
    connection.send(&ToTool::Hello { host: "x".into() });
    assert!(connection.poll().is_empty());
}

#[test]
fn nothing_listening_is_an_error_and_not_a_wait() {
    let (listener, address) = listener();
    drop(listener);
    let started = Instant::now();
    assert!(ToolConnection::connect(address).is_err());
    assert!(started.elapsed() < Duration::from_secs(5));
}

// ---- the path ----

fn picture(across: FaceDirection, up: FaceDirection) -> FacePicture {
    FacePicture {
        tile: TileRef::new(0, 2).unwrap(),
        across,
        up,
        cut_out: false,
    }
}

fn voxel(x: i128, y: i128, z: i128) -> VoxelCoordinate {
    VoxelCoordinate::new(x, y, z)
}

#[test]
fn along_a_run_of_voxels_the_pixels_go_on_across_the_join_and_they_are_the_pictures_own_pixels_modulo_its_size()
 {
    // The side that looks at +z (across +x, up +y), the pointer going right along three voxels at the height 0.3 of each.
    let mut path = PaintPath::default();
    let side = picture(FaceDirection::PositiveX, FaceDirection::PositiveY);
    let mut xs = Vec::new();
    for (index, x) in [4i128, 5, 6].into_iter().enumerate() {
        for fraction in [0.1, 0.55, 0.9] {
            let at = path.place(
                GridId::MAIN,
                voxel(x, 10, 7),
                FaceDirection::PositiveZ,
                [fraction, 0.3, 1.0],
                side,
                16,
            );
            assert_eq!(at.segment, 1, "one segment all along (voxel {index})");
            // 0.3 up from the voxel's bottom is 0.7 down from its top: pixel row 11 of 16 (floor 11.2), counted from the first voxel's top, at the same row.
            assert_eq!(at.y.rem_euclid(16), 11);
            xs.push(at.x);
        }
    }
    // The first voxel is the anchor: its pixels are the picture's; the next voxel's go on from 16.
    assert_eq!(xs, vec![1, 8, 14, 17, 24, 30, 33, 40, 46]);
}

#[test]
fn a_surface_that_turns_or_changes_starts_a_new_segment_and_so_does_a_lift() {
    let mut path = PaintPath::default();
    let side = picture(FaceDirection::PositiveX, FaceDirection::PositiveY);
    let place =
        |path: &mut PaintPath,
         voxel: VoxelCoordinate,
         face: FaceDirection,
         picture: FacePicture,
         grid: GridId| { path.place(grid, voxel, face, [0.5; 3], picture, 16).segment };
    let first = place(
        &mut path,
        voxel(0, 0, 0),
        FaceDirection::PositiveZ,
        side,
        GridId::MAIN,
    );
    assert_eq!(
        place(
            &mut path,
            voxel(1, 0, 0),
            FaceDirection::PositiveZ,
            side,
            GridId::MAIN
        ),
        first,
        "the next voxel on the plane"
    );
    assert_eq!(
        place(
            &mut path,
            voxel(1, 3, 0),
            FaceDirection::PositiveZ,
            side,
            GridId::MAIN
        ),
        first,
        "and a higher one"
    );
    let other_plane = place(
        &mut path,
        voxel(1, 3, 1),
        FaceDirection::PositiveZ,
        side,
        GridId::MAIN,
    );
    assert_ne!(other_plane, first, "a step in or out is another plane");
    let other_face = place(
        &mut path,
        voxel(1, 3, 1),
        FaceDirection::PositiveY,
        picture(FaceDirection::PositiveX, FaceDirection::NegativeZ),
        GridId::MAIN,
    );
    assert_ne!(other_face, other_plane, "round a corner is another face");
    let other_turn = place(
        &mut path,
        voxel(1, 3, 1),
        FaceDirection::PositiveY,
        picture(FaceDirection::NegativeX, FaceDirection::NegativeZ),
        GridId::MAIN,
    );
    assert_ne!(
        other_turn, other_face,
        "the same face turned another way is another picture"
    );
    let mut other_tile = picture(FaceDirection::NegativeX, FaceDirection::NegativeZ);
    other_tile.tile = TileRef::new(0, 3).unwrap();
    let other = place(
        &mut path,
        voxel(1, 3, 1),
        FaceDirection::PositiveY,
        other_tile,
        GridId::MAIN,
    );
    assert_ne!(other, other_turn, "another picture");
    let ship = place(
        &mut path,
        voxel(1, 3, 1),
        FaceDirection::PositiveY,
        other_tile,
        GridId::new(GridId::FIRST_ALLOCATED),
    );
    assert_ne!(ship, other, "another grid");
    path.lift();
    let lifted = place(
        &mut path,
        voxel(1, 3, 1),
        FaceDirection::PositiveY,
        other_tile,
        GridId::new(GridId::FIRST_ALLOCATED),
    );
    assert_ne!(lifted, ship, "after the pointer was over nothing");
}

proptest! {
    /// However a picture sits on a face, the pixel of the path (modulo the size) is the pixel `place_of` names for the same point, and neighbouring voxels on the plane
    /// are a whole picture apart.
    #[test]
    fn the_pixel_of_the_path_is_the_pixel_the_cpu_mapping_names(
        face_index in 0u8..6,
        turn in 0usize..4,
        up_positive in any::<bool>(),
        along in 0.001f64..0.999,
        beside in 0.001f64..0.999,
        anchor in [-50i128..50, -50i128..50, -50i128..50],
        step in -3i128..4,
        size in prop::sample::select(vec![8u32, 16, 32]),
    ) {
        let face = FaceDirection::from_index(face_index).unwrap();
        let in_plane: Vec<FaceDirection> = FaceDirection::ALL.into_iter().filter(|direction| direction.axis() != face.axis()).collect();
        let across = in_plane[turn];
        let other_axis = (0..3).find(|axis| *axis != face.axis() && *axis != across.axis()).unwrap();
        let up = FaceDirection::from_index((other_axis * 2) as u8 + u8::from(up_positive)).unwrap();
        let picture = picture(across, up);
        let mut point = [0.0; 3];
        point[face.axis()] = if face.is_positive() { 1.0 } else { 0.0 };
        point[across.axis()] = along;
        point[up.axis()] = beside;
        let mut path = PaintPath::default();
        let first = path.place(GridId::MAIN, voxel(anchor[0], anchor[1], anchor[2]), face, point, picture, size);
        let expected = picture.place_of(point).pixel(size as usize);
        prop_assert_eq!((first.x.rem_euclid(i64::from(size)) as usize, first.y.rem_euclid(i64::from(size)) as usize), (expected[0], expected[1]));
        // The voxel `step` along `across` on the same plane: the same pixel of the picture, a whole number of pictures on in the direction `across` goes.
        let mut moved = anchor;
        moved[across.axis()] += step;
        let there = path.place(GridId::MAIN, voxel(moved[0], moved[1], moved[2]), face, point, picture, size);
        prop_assert_eq!(there.segment, first.segment);
        prop_assert_eq!(there.y, first.y);
        let pictures_on = i64::from(size) * step as i64 * if across.is_positive() { 1 } else { -1 };
        prop_assert_eq!(there.x - first.x, pictures_on);
    }
}
