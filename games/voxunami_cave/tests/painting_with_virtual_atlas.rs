//! Paint mode against the real VirtualAtlas: the program is started as a process (on a dummy display, with a copy of the tool and two tiles in a folder of its own), the game
//! connects, the pointer goes over a block of stone with the left button down, and the picture the game would show comes back with the stroke in it. The two halves of the
//! protocol (`tools/virtual_atlas/BRIDGE.md`) are each tested against a stand-in of the other; this is the one test of them against each other.
//!
//! It needs `python` with numpy, pillow and pygame; without them it prints a notice and passes, unless `VOX_REQUIRE_PYTHON` is set.

mod common;

use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use common::{Rig, above_the_ground, voxels_of};
use game_shell::paint_bridge::{Buttons, Painter};
use game_shell::texture_pack::PictureApplier;
use renderer::PictureWrite;
use voxunami_cave::game::request_block_with_state;

/// What the game was asked to write.
#[derive(Default)]
struct Writes {
    written: Vec<(usize, usize, Vec<u8>)>,
}

impl PictureApplier for Writes {
    fn write(
        &mut self,
        class: usize,
        layer: usize,
        picture: &PictureWrite<'_>,
    ) -> Result<(), String> {
        self.written.push((class, layer, picture.rgba.to_vec()));
        Ok(())
    }
}

/// The tool, running; it is stopped when this goes away.
struct RunningTool {
    child: Child,
    folder: PathBuf,
}

impl Drop for RunningTool {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

fn python_has_what_the_tool_needs() -> bool {
    Command::new("python")
        .args(["-c", "import numpy, PIL, pygame"])
        .env("PYGAME_HIDE_SUPPORT_PROMPT", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn a_free_port() -> u16 {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A tile of one colour, as the bytes of a PNG file.
fn png_of(colour: [u8; 4]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, 16, 16);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    let pixels: Vec<u8> = (0..16 * 16).flat_map(|_| colour).collect();
    writer.write_image_data(&pixels).unwrap();
    writer.finish().unwrap();
    bytes
}

/// Starts the tool in a folder of its own: a copy of the program and a tile `stone.png` (all of one colour).
fn start_the_tool(port: u16, tile: [u8; 4]) -> RunningTool {
    let program = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins/base_blocks/textures/VirtualAtlas.py");
    let folder = std::env::temp_dir().join(format!(
        "virtual_atlas_bridge_test_{}_{port}",
        std::process::id()
    ));
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::copy(&program, folder.join("VirtualAtlas.py")).unwrap();
    std::fs::write(folder.join("stone.png"), png_of(tile)).unwrap();
    std::fs::write(
        folder.join("atlas.json"),
        r#"{"tile_size": 16, "coordinates_to_names": {"(0, 0)": "stone.png"}, "coordinates_to_data": {}}"#,
    )
    .unwrap();
    let child = Command::new("python")
        .args([
            "VirtualAtlas.py",
            "--bridge",
            &port.to_string(),
            "--group",
            "base_blocks",
        ])
        .current_dir(&folder)
        .env("SDL_VIDEODRIVER", "dummy")
        .env("SDL_AUDIODRIVER", "dummy")
        .env("PYGAME_HIDE_SUPPORT_PROMPT", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("python starts");
    RunningTool { child, folder }
}

#[test]
fn a_stroke_over_a_block_of_stone_is_painted_by_virtual_atlas_and_comes_back_as_the_picture_to_show()
 {
    if !python_has_what_the_tool_needs() {
        assert!(
            std::env::var_os("VOX_REQUIRE_PYTHON").is_none(),
            "VOX_REQUIRE_PYTHON is set but python with numpy, pillow and pygame is not there"
        );
        eprintln!("python with numpy, pillow and pygame is not there; skipping this test");
        return;
    }
    let mut rig = Rig::new();
    let (mut player, above) = above_the_ground(&mut rig, None);
    request_block_with_state(&mut player, above, "base_blocks:stone", 0);
    rig.settle(&mut [&mut player]);

    let background = [60u8, 120, 180, 255];
    let port = a_free_port();
    let _tool = start_the_tool(port, background);
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    // The tool takes a moment to start.
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut painter = loop {
        match Painter::connect(address, "Cave under test", player.package()) {
            Ok(painter) => break painter,
            Err(error) => {
                assert!(
                    Instant::now() < deadline,
                    "the tool did not start listening: {error}"
                );
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    };

    let mut applier = Writes::default();
    let mut notes: Vec<String> = Vec::new();
    // Over the top of the stone, along the row 0.6 of the way across it (pixel row 9), from 0.2 to 0.8 of its width (pixels 3 to 12): one place at a time, as frames come.
    let top = above.y() as f64 + 1.0;
    let row = above.z() as f64 + 0.6;
    let frame = |painter: &mut Painter,
                 across: f64,
                 buttons: Buttons,
                 applier: &mut Writes,
                 notes: &mut Vec<String>| {
        let eyes = voxels_of(&player.camera);
        let at = [above.x() as f64 + across, top, row];
        let direction = [at[0] - eyes[0], at[1] - eyes[1], at[2] - eyes[2]];
        let target = player.target_surface_toward(direction);
        assert!(
            target.is_some(),
            "the pointer is over the stone at {across}"
        );
        notes.extend(painter.step(player.package(), target.as_ref(), buttons, applier));
        std::thread::sleep(Duration::from_millis(25));
    };
    // Say hello first: wait until the tool has welcomed the game, with the pointer over the stone and no button down.
    let deadline = Instant::now() + Duration::from_secs(60);
    while !notes.iter().any(|note| note.contains("is connected")) {
        frame(
            &mut painter,
            0.5,
            Buttons::default(),
            &mut applier,
            &mut notes,
        );
        assert!(
            Instant::now() < deadline,
            "the tool did not welcome the game: {notes:?}"
        );
    }
    while !notes.iter().any(|note| note.contains("has files for")) {
        frame(
            &mut painter,
            0.5,
            Buttons::default(),
            &mut applier,
            &mut notes,
        );
        assert!(
            Instant::now() < deadline,
            "the tool did not say which pictures it has: {notes:?}"
        );
    }
    assert!(
        notes.iter().any(|note| note.contains("not for")),
        "the tool has the stone and not the rest: {notes:?}"
    );
    // The stroke: the button goes down on pixel 3 and the pointer goes to pixel 12, and the button comes up.
    for across in [0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8] {
        frame(
            &mut painter,
            across,
            Buttons {
                left: true,
                right: false,
            },
            &mut applier,
            &mut notes,
        );
    }
    frame(
        &mut painter,
        0.8,
        Buttons::default(),
        &mut applier,
        &mut notes,
    );
    // The picture comes back while it is drawn and when it is done: wait for one with the whole stroke in it.
    let painted = |rgba: &[u8], column: usize, row: usize| {
        rgba[(row * 16 + column) * 4..(row * 16 + column) * 4 + 4] != background
    };
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some((_, _, rgba)) = applier.written.last()
            && (3..=12).all(|column| painted(rgba, column, 9))
        {
            break;
        }
        frame(
            &mut painter,
            0.8,
            Buttons::default(),
            &mut applier,
            &mut notes,
        );
        assert!(
            Instant::now() < deadline,
            "the stroke did not come back: {} pictures, notes {notes:?}",
            applier.written.len()
        );
    }
    let (class, layer, rgba) = applier.written.last().unwrap().clone();
    // Where the stroke is and where it is not: the pixels 3 to 12 of the row 9, and nothing else.
    for row in 0..16 {
        for column in 0..16 {
            let on_the_stroke = row == 9 && (3..=12).contains(&column);
            assert_eq!(
                painted(&rgba, column, row),
                on_the_stroke,
                "pixel ({column}, {row})"
            );
        }
    }
    // It is written where the world has the stone's picture (the place the game named to the tool).
    let stone = player.package().tile_classes[class][layer].size();
    assert_eq!(stone, 16);
    assert!(
        notes
            .iter()
            .all(|note| !note.contains("refused") && !note.contains("not a message")),
        "{notes:?}"
    );
}
