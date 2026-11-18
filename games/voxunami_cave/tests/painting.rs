//! Paint mode against a real Cave session and a tool that is a few lines of Rust on a socket: what the game says (the pictures it has, where the pointer is on them), and what
//! it does with what the tool says back (writes the picture, keeping its companions). The protocol is `tools/virtual_atlas/BRIDGE.md`; the tool here is not VirtualAtlas, which is
//! the point of the protocol being its own.

mod common;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use common::{Rig, above_the_ground, voxels_of};
use game_shell::paint_bridge::{Buttons, Painter, encode_base64};
use game_shell::texture_pack::PictureApplier;
use renderer::PictureWrite;
use serde_json::{Value, json};
use voxel_space::VoxelCoordinate;
use voxunami_cave::game::request_block_with_state;

/// What a write was: the place's size number and layer, the picture, and its specular and normal companions.
type Written = (usize, usize, Vec<u8>, Option<Vec<u8>>, Option<Vec<u8>>);

/// A renderer that remembers what it was asked to write.
#[derive(Default)]
struct Writes {
    written: Vec<Written>,
}

impl PictureApplier for Writes {
    fn write(
        &mut self,
        class: usize,
        layer: usize,
        picture: &PictureWrite<'_>,
    ) -> Result<(), String> {
        self.written.push((
            class,
            layer,
            picture.rgba.to_vec(),
            picture.specular.map(<[u8]>::to_vec),
            picture.normal.map(<[u8]>::to_vec),
        ));
        Ok(())
    }
}

/// The tool: a listener, and what it has heard.
struct Tool {
    stream: std::net::TcpStream,
    heard: Vec<u8>,
}

impl Tool {
    fn lines(&mut self, at_least: usize) -> Vec<Value> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let complete = self.heard.iter().filter(|byte| **byte == b'\n').count();
            if complete >= at_least {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the game did not say {at_least} lines"
            );
            let mut piece = [0u8; 65536];
            match self.stream.read(&mut piece) {
                Ok(0) => break,
                Ok(count) => self.heard.extend_from_slice(&piece[..count]),
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        || error.kind() == std::io::ErrorKind::TimedOut => {}
                Err(error) => panic!("{error}"),
            }
        }
        let text = String::from_utf8(std::mem::take(&mut self.heard)).unwrap();
        text.lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn say(&mut self, message: Value) {
        self.stream
            .write_all(format!("{message}\n").as_bytes())
            .unwrap();
    }
}

fn listen() -> (TcpListener, std::net::SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    (listener, address)
}

/// Runs frames until `done` holds of what the tool has heard since.
fn until_the_tool_hears(
    painter: &mut Painter,
    player: &game_shell::session::Session<
        Box<dyn transport::Connection>,
        voxunami_cave::game::Cave,
    >,
    tool: &mut Tool,
    aim: [f64; 3],
    buttons: Buttons,
    applier: &mut Writes,
    lines: usize,
) -> (Vec<Value>, Vec<String>) {
    let mut notes = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let eyes = voxels_of(&player.camera);
        let direction = [aim[0] - eyes[0], aim[1] - eyes[1], aim[2] - eyes[2]];
        let target = player.target_surface_toward(direction);
        notes.extend(painter.step(player.package(), target.as_ref(), buttons, applier));
        tool.stream
            .set_read_timeout(Some(Duration::from_millis(20)))
            .unwrap();
        let mut piece = [0u8; 65536];
        if let Ok(count) = tool.stream.read(&mut piece) {
            tool.heard.extend_from_slice(&piece[..count]);
        }
        if tool.heard.iter().filter(|byte| **byte == b'\n').count() >= lines {
            return (tool.lines(lines), notes);
        }
        assert!(
            Instant::now() < deadline,
            "the game did not say {lines} lines"
        );
    }
}

#[test]
fn the_game_tells_the_tool_its_pictures_and_where_the_pointer_is_and_writes_what_the_tool_sends_back()
 {
    let mut rig = Rig::new();
    let (mut player, above) = above_the_ground(&mut rig, None);
    // Two whole blocks of stone side by side on the ground: their tops are one flat run.
    let (first, second) = (
        above,
        VoxelCoordinate::new(above.x() + 1, above.y(), above.z()),
    );
    request_block_with_state(&mut player, first, "base_blocks:stone", 0);
    request_block_with_state(&mut player, second, "base_blocks:stone", 0);
    rig.settle(&mut [&mut player]);

    let (listener, address) = listen();
    let mut painter = Painter::connect(address, "Cave under test", player.package()).unwrap();
    let (stream, _) = listener.accept().unwrap();
    let mut tool = Tool {
        stream,
        heard: Vec::new(),
    };
    let mut applier = Writes::default();

    // The top of the stone, as the tool should be told it: every point is a pixel of its picture. The pointer is at (0.3, 0.6) of the first block's top.
    let (top_y, row) = (above.y() as f64 + 1.0, above.z() as f64 + 0.6);
    let on_first = [above.x() as f64 + 0.3, top_y, row];
    let on_second = [above.x() as f64 + 1.4, top_y, row];
    let (heard, _) = until_the_tool_hears(
        &mut painter,
        &player,
        &mut tool,
        on_first,
        Buttons {
            left: true,
            right: false,
        },
        &mut applier,
        3,
    );
    assert_eq!(heard[0]["type"], "hello");
    assert_eq!(heard[0]["protocol"], 1);
    assert_eq!(heard[1]["type"], "tiles");
    let tiles = heard[1]["tiles"].as_array().unwrap();
    let stone = tiles
        .iter()
        .find(|tile| tile["files"][0] == "stone_top")
        .expect("the stone's top is a picture of the world");
    assert_eq!(
        stone["files"],
        json!(["stone_top", "stone", "stone_side", "stone_bottom"])
    );
    assert_eq!(stone["group"], "base_blocks");
    assert_eq!(stone["size"], 16);
    let id = stone["id"].as_str().unwrap().to_owned();
    // The pointer: on the stone's tile, at pixel (4, 9) of it (0.3 and 0.6 of 16, the top as it is made having across +x and up -z), the left button down.
    assert_eq!(heard[2]["type"], "pointer");
    assert_eq!(heard[2]["tile"], id);
    assert_eq!(
        (heard[2]["x"].as_i64(), heard[2]["y"].as_i64()),
        (Some(4), Some(9))
    );
    assert_eq!(
        (heard[2]["left"].as_bool(), heard[2]["right"].as_bool()),
        (Some(true), Some(false))
    );
    let segment = heard[2]["segment"].as_u64().unwrap();

    // Over the next block, a voxel on, the pixels go on (a picture and 0.4 of the next on), in the same segment.
    let (heard, _) = until_the_tool_hears(
        &mut painter,
        &player,
        &mut tool,
        on_second,
        Buttons {
            left: true,
            right: false,
        },
        &mut applier,
        1,
    );
    assert_eq!(heard[0]["tile"], id);
    assert_eq!(
        (heard[0]["x"].as_i64(), heard[0]["y"].as_i64()),
        (Some(22), Some(9))
    );
    assert_eq!(heard[0]["segment"].as_u64(), Some(segment));

    // Over the sky, no tile, and nothing says the same twice.
    let sky = [on_first[0], top_y + 40.0, on_first[2]];
    let eyes = voxels_of(&player.camera);
    let up = [sky[0] - eyes[0], 100.0, sky[2] - eyes[2]];
    let mut notes = painter.step(
        player.package(),
        player.target_surface_toward(up).as_ref(),
        Buttons::default(),
        &mut applier,
    );
    notes.extend(painter.step(
        player.package(),
        player.target_surface_toward(up).as_ref(),
        Buttons::default(),
        &mut applier,
    ));
    assert!(notes.is_empty(), "{notes:?}");
    let heard = tool.lines(1);
    assert_eq!(
        heard.len(),
        1,
        "the same pointer twice is said once: {heard:?}"
    );
    assert_eq!(heard[0]["tile"], Value::Null);
    assert_eq!(
        (heard[0]["left"].as_bool(), heard[0]["right"].as_bool()),
        (Some(false), Some(false))
    );

    // The tool sends a picture of the stone's top: the game writes it, as the picture of that place.
    let red: Vec<u8> = (0..16 * 16).flat_map(|_| [255u8, 0, 0, 255]).collect();
    tool.say(json!({"type": "tile", "id": id, "size": 16, "pixels": encode_base64(&red)}));
    let notes = step_until(&mut painter, &player, &mut applier, |applier| {
        !applier.written.is_empty()
    });
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(applier.written.len(), 1);
    let (class, layer, rgba, ..) = applier.written[0].clone();
    assert_eq!(rgba, red);
    let place_text = format!("{class}:{layer}");
    assert_eq!(
        place_text, id,
        "it is written where the tool was told the picture is"
    );

    // A picture with companions keeps them: the dirt has a normal picture, which the tool does not know.
    let dirt = tiles
        .iter()
        .find(|tile| {
            tile["files"][0] == "dirt_top"
                || tile["files"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|name| name == "dirt")
        })
        .expect("the dirt is a picture of the world");
    let dirt_id = dirt["id"].as_str().unwrap();
    let blue: Vec<u8> = (0..16 * 16).flat_map(|_| [0u8, 0, 255, 255]).collect();
    tool.say(json!({"type": "tile", "id": dirt_id, "size": 16, "pixels": encode_base64(&blue)}));
    step_until(&mut painter, &player, &mut applier, |applier| {
        applier.written.len() == 2
    });
    let (dirt_class, dirt_layer) = {
        let (class, layer) = dirt_id.split_once(':').unwrap();
        (
            class.parse::<usize>().unwrap(),
            layer.parse::<usize>().unwrap(),
        )
    };
    let world = &player.package().tile_classes[dirt_class][dirt_layer];
    assert!(
        world.normal().is_some(),
        "the dirt has a normal picture in the world"
    );
    let (.., specular, normal) = applier.written[1].clone();
    assert_eq!(normal.as_deref(), world.normal());
    assert_eq!(specular.as_deref(), world.specular());

    // And a picture with a specular companion keeps that one: the lamp's glow is not the tool's to know.
    let lamp = tiles
        .iter()
        .find(|tile| {
            tile["files"]
                .as_array()
                .unwrap()
                .iter()
                .any(|name| name == "lamp")
        })
        .expect("the lamp is a picture of the world");
    let lamp_id = lamp["id"].as_str().unwrap();
    tool.say(json!({"type": "tile", "id": lamp_id, "size": 16, "pixels": encode_base64(&blue)}));
    step_until(&mut painter, &player, &mut applier, |applier| {
        applier.written.len() == 3
    });
    let (lamp_class, lamp_layer) = lamp_id.split_once(':').unwrap();
    let world = &player.package().tile_classes[lamp_class.parse::<usize>().unwrap()]
        [lamp_layer.parse::<usize>().unwrap()];
    assert!(
        world.specular().is_some(),
        "the lamp has a specular picture in the world"
    );
    let (.., specular, _) = applier.written[2].clone();
    assert_eq!(specular.as_deref(), world.specular());

    // What is wrong with what the tool says is told to the player and written nowhere.
    tool.say(json!({"type": "tile", "id": "9:9", "size": 16, "pixels": encode_base64(&blue)}));
    tool.say(json!({"type": "tile", "id": id, "size": 8, "pixels": encode_base64(&vec![0u8; 8 * 8 * 4])}));
    tool.say(json!({"type": "error", "text": "no such file"}));
    tool.stream.write_all(b"{ not json\n").unwrap();
    let mut told = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while told.len() < 4 {
        told.extend(painter.step(player.package(), None, Buttons::default(), &mut applier));
        assert!(Instant::now() < deadline, "{told:?}");
    }
    assert!(told[0].contains("does not have"), "{told:?}");
    assert!(told[1].contains("8 pixels"), "{told:?}");
    assert_eq!(told[2], "the tool: no such file");
    assert!(told[3].contains("not a message"), "{told:?}");
    assert_eq!(applier.written.len(), 3, "none of those was written");

    // A new package: the renderer's pictures were made again, so what the tool changed is written again, and the tool is told the pictures again.
    let mut again = Writes::default();
    let notes = painter.package_replaced(player.package(), &mut again);
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(again.written.len(), 3);
    // (What is queued goes out with the next frame.)
    painter.step(player.package(), None, Buttons::default(), &mut applier);
    assert_eq!(tool.lines(1)[0]["type"], "tiles");

    // The tool going away is told once, and the game goes on.
    drop(tool);
    let mut told = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while painter.is_connected() {
        told.extend(painter.step(player.package(), None, Buttons::default(), &mut applier));
        assert!(Instant::now() < deadline);
    }
    told.extend(painter.step(player.package(), None, Buttons::default(), &mut applier));
    assert_eq!(told, vec!["the tool went away".to_owned()]);
}

fn step_until(
    painter: &mut Painter,
    player: &game_shell::session::Session<
        Box<dyn transport::Connection>,
        voxunami_cave::game::Cave,
    >,
    applier: &mut Writes,
    done: impl Fn(&Writes) -> bool,
) -> Vec<String> {
    let mut notes = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(applier) {
        notes.extend(painter.step(player.package(), None, Buttons::default(), applier));
        assert!(
            Instant::now() < deadline,
            "what was waited for did not happen: {notes:?}"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    notes
}

/// What an undo and a redo in the tool look like to the game: the tool sends the picture as it was before the stroke, and then as it was after it. Each is written, though the
/// first is the picture the world already has (and the third is the one written before), and every one keeps the companions that the tool does not know.
#[test]
fn a_picture_the_tool_sends_again_as_it_was_is_written_again_so_that_undo_and_redo_in_the_tool_show_in_the_game()
 {
    let mut rig = Rig::new();
    let player = rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    let (listener, address) = listen();
    let mut painter = Painter::connect(address, "Cave under test", player.package()).unwrap();
    let (stream, _) = listener.accept().unwrap();
    let mut tool = Tool {
        stream,
        heard: Vec::new(),
    };
    let mut applier = Writes::default();
    // (What was queued at connecting goes out with the first frame; the tool waits for it, but not for ever.)
    tool.stream
        .set_read_timeout(Some(Duration::from_millis(50)))
        .unwrap();
    painter.step(player.package(), None, Buttons::default(), &mut applier);
    let heard = tool.lines(2);
    let dirt = heard[1]["tiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tile| {
            tile["files"]
                .as_array()
                .unwrap()
                .iter()
                .any(|name| name == "dirt")
        })
        .expect("the dirt is a picture of the world");
    let id = dirt["id"].as_str().unwrap();
    let (class, layer) = id.split_once(':').unwrap();
    let world = &player.package().tile_classes[class.parse::<usize>().unwrap()]
        [layer.parse::<usize>().unwrap()];
    assert!(
        world.normal().is_some(),
        "the dirt has a normal picture in the world"
    );
    let original = world.rgba().to_vec();
    let stroke: Vec<u8> = (0..16 * 16).flat_map(|_| [255u8, 0, 0, 255]).collect();
    assert_ne!(stroke, original);

    for (count, picture) in [&stroke, &original, &stroke, &original]
        .into_iter()
        .enumerate()
    {
        tool.say(json!({"type": "tile", "id": id, "size": 16, "pixels": encode_base64(picture)}));
        let notes = step_until(&mut painter, &player, &mut applier, |applier| {
            applier.written.len() == count + 1
        });
        assert!(notes.is_empty(), "{notes:?}");
    }
    for (index, (_, _, rgba, specular, normal)) in applier.written.iter().enumerate() {
        assert_eq!(
            rgba,
            if index % 2 == 0 { &stroke } else { &original },
            "write {index}"
        );
        assert_eq!(normal.as_deref(), world.normal(), "write {index}");
        assert_eq!(specular.as_deref(), world.specular(), "write {index}");
    }
}

/// What a tool that sends companion pictures does to the game: the game says which convention it reads, a picture with `companions` is written with exactly those (a file
/// that is not there is none, even if the world's picture had one), an empty set clears them, and a picture that says nothing of them keeps the ones it last had.
#[test]
fn companions_the_tool_sends_are_written_with_the_picture_and_a_missing_one_is_none() {
    let mut rig = Rig::new();
    let player = rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    let (listener, address) = listen();
    let mut painter = Painter::connect(address, "Cave under test", player.package()).unwrap();
    let (stream, _) = listener.accept().unwrap();
    let mut tool = Tool {
        stream,
        heard: Vec::new(),
    };
    let mut applier = Writes::default();
    tool.stream
        .set_read_timeout(Some(Duration::from_millis(50)))
        .unwrap();
    painter.step(player.package(), None, Buttons::default(), &mut applier);
    let heard = tool.lines(2);
    assert_eq!(heard[0]["profile"], "labpbr_1_3");
    let dirt = heard[1]["tiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tile| {
            tile["files"]
                .as_array()
                .unwrap()
                .iter()
                .any(|name| name == "dirt")
        })
        .expect("the dirt is a picture of the world");
    let id = dirt["id"].as_str().unwrap();
    let (class, layer) = id.split_once(':').unwrap();
    let world = &player.package().tile_classes[class.parse::<usize>().unwrap()]
        [layer.parse::<usize>().unwrap()];
    assert!(
        world.normal().is_some(),
        "the dirt has a normal picture in the world"
    );
    let picture: Vec<u8> = (0..16 * 16).flat_map(|_| [1u8, 2, 3, 255]).collect();
    let specular: Vec<u8> = (0..16 * 16).flat_map(|_| [50u8, 60, 70, 80]).collect();

    // A welcome that names the game's convention is only said; another one is told to the player.
    tool.say(json!({"type": "welcome", "protocol": 1, "tool": "A tool", "tile_size": 16, "profile": "labpbr_1_3"}));
    let notes = step_until_noted(&mut painter, &player, &mut applier, 1);
    assert_eq!(notes, vec!["A tool is connected".to_owned()]);
    tool.say(json!({"type": "welcome", "protocol": 1, "tool": "A tool", "tile_size": 16, "profile": "some_other_pbr"}));
    let notes = step_until_noted(&mut painter, &player, &mut applier, 2);
    assert!(
        notes[1].contains("some_other_pbr") && notes[1].contains("labpbr_1_3"),
        "{notes:?}"
    );

    // Only a specular picture: that is the specular, and the normal the world had is gone (the tool's file for it would not exist).
    tool.say(json!({
        "type": "tile", "id": id, "size": 16, "pixels": encode_base64(&picture),
        "companions": {"_s": encode_base64(&specular)},
    }));
    step_until(&mut painter, &player, &mut applier, |applier| {
        applier.written.len() == 1
    });
    let (_, _, rgba, written_specular, written_normal) = applier.written[0].clone();
    assert_eq!(rgba, picture);
    assert_eq!(written_specular.as_deref(), Some(&specular[..]));
    assert_eq!(written_normal, None);

    // A picture that says nothing of companions keeps the ones it last had, and not the world's.
    tool.say(json!({"type": "tile", "id": id, "size": 16, "pixels": encode_base64(&picture)}));
    step_until(&mut painter, &player, &mut applier, |applier| {
        applier.written.len() == 2
    });
    assert_eq!(applier.written[1].3.as_deref(), Some(&specular[..]));
    assert_eq!(applier.written[1].4, None);

    // An empty set clears both; a suffix the game does not read is ignored.
    tool.say(json!({
        "type": "tile", "id": id, "size": 16, "pixels": encode_base64(&picture),
        "companions": {"_x": encode_base64(&specular)},
    }));
    step_until(&mut painter, &player, &mut applier, |applier| {
        applier.written.len() == 3
    });
    assert_eq!(
        (applier.written[2].3.clone(), applier.written[2].4.clone()),
        (None, None)
    );

    // A new package writes the tool's pictures again with the companions it last sent.
    let mut again = Writes::default();
    assert!(
        painter
            .package_replaced(player.package(), &mut again)
            .is_empty()
    );
    assert_eq!(again.written.len(), 1);
    assert_eq!(
        (again.written[0].3.clone(), again.written[0].4.clone()),
        (None, None)
    );

    // Both files, as a tool under LabPBR sends them.
    let normal: Vec<u8> = (0..16 * 16).flat_map(|_| [128u8, 128, 255, 255]).collect();
    tool.say(json!({
        "type": "tile", "id": id, "size": 16, "pixels": encode_base64(&picture),
        "companions": {"_s": encode_base64(&specular), "_n": encode_base64(&normal)},
    }));
    step_until(&mut painter, &player, &mut applier, |applier| {
        applier.written.len() == 4
    });
    assert_eq!(applier.written[3].3.as_deref(), Some(&specular[..]));
    assert_eq!(applier.written[3].4.as_deref(), Some(&normal[..]));
}

/// Runs frames until the game has told the player `count` things.
fn step_until_noted(
    painter: &mut Painter,
    player: &game_shell::session::Session<
        Box<dyn transport::Connection>,
        voxunami_cave::game::Cave,
    >,
    applier: &mut Writes,
    count: usize,
) -> Vec<String> {
    let mut notes = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while notes.len() < count {
        notes.extend(painter.step(player.package(), None, Buttons::default(), applier));
        assert!(
            Instant::now() < deadline,
            "what was waited for did not happen: {notes:?}"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    notes
}

#[test]
fn a_game_with_no_tool_listening_is_told_so() {
    let mut rig = Rig::new();
    let player = rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    let (listener, address) = listen();
    drop(listener);
    assert!(Painter::connect(address, "Cave under test", player.package()).is_err());
}
