//! Schemes of the multiscale light compared on terrain that is the same for all of them: a platform in
//! the sky that is flat on one side of a lamp and hilly on the other. The cheap test checks the terrain;
//! the others are benchmarks and picture makers, and are ignored unless asked for:
//!
//! ```text
//! cargo test --release -p voxunami_cave --test multiscale_modes -- --ignored --nocapture
//! ```
//!
//! `MULTISCALE_SET` says which schemes: `presets` (the default: 32 levels with cells up to 4 voxels, 16
//! up to 8, 8 up to 16, 4 up to 32) or `custom` (see [`custom_schemes`]). The tests print tables, and
//! write `OUTPUT_DIRECTORY/mode_*.rgba` (raw pictures, 1280 by 720) for whoever wants to look. Set
//! `MULTISCALE_OUTPUT` to say where (a directory); otherwise the pictures are not written.

mod common;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use common::{Rig, VIEW_RADIUS};
use game_shell::GameContent;
use game_shell::view::{apply_updates, atmosphere, configure_renderer};
use renderer::{GraphicsContext, VoxelRenderer};
use voxel_space::{GridId, VoxelCoordinate};
use voxunami_cave::content::CaveContent;
use voxunami_cave::multiscale_light::{
    LightScheme, MAXIMUM_SCALES, MultiScaleLighting, ScaledLightWorld, default_budgets,
    gives_light, level_of_word,
};

/// Schemes whose cells are not all powers of two.
fn custom_schemes() -> Vec<LightScheme> {
    [
        (8, &[1, 3, 7, 17][..]),
        (8, &[1, 5, 17]),
        (8, &[2, 5, 11, 17]),
        (4, &[1, 3, 5, 7, 11, 17, 23]),
    ]
    .into_iter()
    .map(|(levels, widths)| LightScheme::new(levels, widths).unwrap())
    .collect()
}

/// The schemes to compare, as `MULTISCALE_SET` says.
fn schemes() -> Vec<LightScheme> {
    match std::env::var("MULTISCALE_SET").as_deref() {
        Ok("custom") => custom_schemes(),
        _ => LightScheme::ALL.to_vec(),
    }
}

/// The command that asks for a scheme, with its widths.
fn command_for(scheme: LightScheme) -> String {
    let widths: Vec<String> = scheme.widths().iter().map(u16::to_string).collect();
    format!("/light multiscale {} {}", scheme.levels(), widths.join(" "))
}

/// What names a scheme's picture: its levels and widths.
fn file_label(scheme: LightScheme) -> String {
    let widths: Vec<String> = scheme.widths().iter().map(u16::to_string).collect();
    format!("{}_levels_cells_{}", scheme.levels(), widths.join("_"))
}

/// Where the platform's ground starts, and how high the flat side of it is.
const BASE: i128 = 296;
const FLAT: i128 = 304;
/// The platform is this many voxels along x and z.
const SIZE: i128 = 96;
/// The lamp, over the middle of the platform: flat ground to one side (x below this), hills to the other.
const LAMP: [i128; 3] = [48, 306, 48];

/// The height of the ground at a column: flat for the half of the platform on the lamp's low-x side, and for
/// the other half hills that rise from flat at the lamp to 14 voxels over the flat across 24 voxels,
/// as two triangle waves (no noise, so every scheme gets the same terrain on every machine).
fn ground(x: i128, z: i128) -> i128 {
    if x < LAMP[0] {
        return FLAT;
    }
    let ramp = (x - LAMP[0]).min(24);
    let wave = |value: i128, period: i128| {
        let phase = value.rem_euclid(2 * period);
        let distance = if phase < period {
            phase
        } else {
            2 * period - phase
        };
        distance * 7 / period
    };
    FLAT + (wave(x, 18) + wave(z, 14)) * ramp / 24
}

/// What is at a voxel, as the three blocks of the scene: air, the ground's stone, the lamp.
fn block_at(content: &CaveContent, at: VoxelCoordinate) -> u32 {
    if [at.x(), at.y(), at.z()] == LAMP {
        content.blocks.lamp
    } else if (0..SIZE).contains(&at.x())
        && (0..SIZE).contains(&at.z())
        && (BASE..ground(at.x(), at.z())).contains(&at.y())
    {
        content.blocks.stone
    } else {
        content.blocks.air
    }
}

/// A world with the platform and the lamp in it as edits, for a server to start from.
fn scene_world(content: &CaveContent) -> world_storage::WorldData {
    let mut world = content.new_overworld_world(7);
    let channel = content.block_channel();
    let overrides = &mut world.main_mut().overrides;
    for x in 0..SIZE {
        for z in 0..SIZE {
            for y in BASE..ground(x, z) {
                overrides.set(channel, VoxelCoordinate::new(x, y, z), content.blocks.stone);
            }
        }
    }
    overrides.set(
        channel,
        VoxelCoordinate::new(LAMP[0], LAMP[1], LAMP[2]),
        content.blocks.lamp,
    );
    world
}

#[test]
fn the_terrain_is_flat_on_one_side_of_the_lamp_and_hilly_on_the_other() {
    let flat: Vec<i128> = (0..LAMP[0])
        .flat_map(|x| (0..SIZE).map(move |z| ground(x, z)))
        .collect();
    assert!(flat.iter().all(|&height| height == FLAT));
    let hilly: Vec<i128> = (LAMP[0]..SIZE)
        .flat_map(|x| (0..SIZE).map(move |z| ground(x, z)))
        .collect();
    let (lowest, highest) = (*hilly.iter().min().unwrap(), *hilly.iter().max().unwrap());
    assert_eq!(lowest, FLAT, "hills start from the flat ground");
    assert!(highest >= FLAT + 10, "and rise well over it: {highest}");
    assert!(highest <= FLAT + 14);
    // At the lamp's column the ground is flat, and the lamp is in the air two voxels over it.
    assert_eq!(ground(LAMP[0], LAMP[2]), FLAT);
    let content = CaveContent::new();
    assert_eq!(
        block_at(&content, VoxelCoordinate::new(LAMP[0], LAMP[1], LAMP[2])),
        content.blocks.lamp
    );
    assert_eq!(
        block_at(&content, VoxelCoordinate::new(LAMP[0], FLAT - 1, LAMP[2])),
        content.blocks.stone
    );
    assert_eq!(
        block_at(&content, VoxelCoordinate::new(LAMP[0], FLAT + 1, LAMP[2])),
        content.blocks.air
    );
    // Nothing outside the platform.
    assert_eq!(
        block_at(&content, VoxelCoordinate::new(SIZE, FLAT - 1, 10)),
        content.blocks.air
    );
}

/// The scene as the light rule reads it, counting what it reads and writes.
struct Scene<'a> {
    content: &'a CaveContent,
    lamp_present: bool,
    words: [HashMap<VoxelCoordinate, u32>; MAXIMUM_SCALES],
    reads: u64,
    writes: u64,
}

impl ScaledLightWorld for Scene<'_> {
    fn block_and_state(&mut self, _: GridId, at: VoxelCoordinate) -> Option<(u32, u32)> {
        self.reads += 1;
        let block = block_at(self.content, at);
        let block = if block == self.content.blocks.lamp && !self.lamp_present {
            self.content.blocks.air
        } else {
            block
        };
        Some((block, 0))
    }

    fn word(&mut self, scale: usize, _: GridId, anchor: VoxelCoordinate) -> u32 {
        self.words[scale].get(&anchor).copied().unwrap_or(0)
    }

    fn set_word(&mut self, scale: usize, _: GridId, anchor: VoxelCoordinate, value: u32) {
        self.writes += 1;
        self.words[scale].insert(anchor, value);
    }
}

fn lamp_voxel() -> VoxelCoordinate {
    VoxelCoordinate::new(LAMP[0], LAMP[1], LAMP[2])
}

/// How a scheme fills and empties on the scene: ticks and time, with the budgets the game uses and
/// with none.
#[test]
#[ignore = "a benchmark: run with --ignored --nocapture"]
fn rule_benchmark() {
    let content = CaveContent::new();
    let table = content.block_table();
    println!(
        "\n| scheme | budgets | fill: ticks | fill: ms | fill: cells written | fill: voxels read | cells lit (by scale) | empty: ticks | empty: ms | unlimited: ticks |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|");
    for scheme in schemes() {
        let budgets = default_budgets(scheme);
        let run = |budgets: &[usize]| {
            let mut scene = Scene {
                content: &content,
                lamp_present: true,
                words: Default::default(),
                reads: 0,
                writes: 0,
            };
            let mut rule = MultiScaleLighting::new(table.clone(), scheme);
            rule.voxel_changed(GridId::MAIN, lamp_voxel());
            let started = Instant::now();
            let mut ticks = 0;
            while !rule.is_settled() && ticks < 20_000 {
                rule.step_within(&mut scene, budgets);
                ticks += 1;
            }
            let fill_ms = started.elapsed().as_secs_f64() * 1000.0;
            let lit: Vec<usize> = (0..scheme.scales())
                .map(|scale| {
                    scene.words[scale]
                        .values()
                        .filter(|&&word| level_of_word(word) > 0)
                        .count()
                })
                .collect();
            let fill = (ticks, fill_ms, scene.writes, scene.reads);
            // Take the lamp away.
            scene.lamp_present = false;
            rule.voxel_changed(GridId::MAIN, lamp_voxel());
            let started = Instant::now();
            let mut empty_ticks = 0;
            while !rule.is_settled() && empty_ticks < 20_000 {
                rule.step_within(&mut scene, budgets);
                empty_ticks += 1;
            }
            let empty = (empty_ticks, started.elapsed().as_secs_f64() * 1000.0);
            let all_dark = scene
                .words
                .iter()
                .all(|words| words.values().all(|&word| !gives_light(word)));
            assert!(all_dark, "{scheme:?} went dark");
            (fill, lit, empty)
        };
        let (fill, lit, empty) = run(&budgets);
        let (unlimited, _, _) = run(&vec![usize::MAX; scheme.scales()]);
        println!(
            "| {} | {:?} | {} | {:.0} | {} | {} | {:?} | {} | {:.0} | {} |",
            scheme.name(),
            budgets,
            fill.0,
            fill.1,
            fill.2,
            fill.3,
            lit,
            empty.0,
            empty.1,
            unlimited.0
        );
    }
}

fn percentile(sorted: &[f64], share: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * share) as usize]
}

/// What a tick of the real server costs with each light, on the scene with a player in it: the first
/// 400 ticks after the light is asked for (when the lamp's light is spreading).
#[test]
#[ignore = "a benchmark: run with --ignored --nocapture"]
fn server_benchmark() {
    println!(
        "\n| light | switching tick ms | mean tick ms (next 400) | median | 95th | worst | ticks over 10 ms | ticks over 50 ms |"
    );
    println!("|---|---|---|---|---|---|---|---|");
    let mut models = vec![("classic".to_owned(), "/light classic".to_owned())];
    for scheme in schemes() {
        models.push((
            format!("multiscale, {}", scheme.name()),
            command_for(scheme),
        ));
    }
    for (name, command) in models {
        let content = CaveContent::new();
        let world = scene_world(&content);
        let mut rig = Rig::with_world(content, world);
        let mut player = rig.join([48.5, 340.0, 48.5], 0.0, 0.0);
        rig.settle(&mut [&mut player]);
        // Asking for a light clears every light and has the new one look at every lamp in the world,
        // which reads every edited brick: that tick is timed on its own. (Classic is on already: go
        // to another and back, so that the lamp is looked at afresh.)
        if command.contains("classic") {
            player.send_command("/light multiscale 4");
            rig.step();
        }
        player.send_command(&command);
        let started = Instant::now();
        rig.step();
        let switch_ms = started.elapsed().as_secs_f64() * 1000.0;
        let mut times = Vec::new();
        for _ in 0..400 {
            let started = Instant::now();
            rig.step();
            times.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        let mean = times.iter().sum::<f64>() / times.len() as f64;
        let over = |limit: f64| times.iter().filter(|&&time| time > limit).count();
        let (over_10, over_50) = (over(10.0), over(50.0));
        times.sort_by(f64::total_cmp);
        println!(
            "| {name} | {switch_ms:.1} | {mean:.2} | {:.2} | {:.2} | {:.2} | {over_10} | {over_50} |",
            percentile(&times, 0.5),
            percentile(&times, 0.95),
            times.last().unwrap(),
        );
    }
}

fn write_rgba(name: &str, pixels: &[u8]) {
    if let Some(directory) = std::env::var_os("MULTISCALE_OUTPUT") {
        let directory = PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join(format!("{name}.rgba")), pixels).unwrap();
    }
}

/// A picture from straight above, of the scene lit by each light, after the light has spread.
#[test]
#[ignore = "makes pictures: run with --ignored --nocapture and MULTISCALE_OUTPUT"]
fn pictures() {
    let contexts = GraphicsContext::all_physical_adapters();
    let Some(context) = contexts.first() else {
        println!("no graphics adapter");
        return;
    };
    let mut models = vec![("classic".to_owned(), "/light classic".to_owned(), 90)];
    for scheme in schemes() {
        let ticks = match scheme {
            LightScheme::LEVELS_4 => 600,
            _ if LightScheme::ALL.contains(&scheme) => 900,
            _ => 1500,
        };
        // The presets keep the names their pictures had.
        let label = if LightScheme::ALL.contains(&scheme) {
            format!("{}_levels", scheme.levels())
        } else {
            file_label(scheme)
        };
        models.push((label, command_for(scheme), ticks));
    }
    for (name, command, ticks) in models {
        let content = CaveContent::new();
        let world = scene_world(&content);
        let mut rig = Rig::with_world(content, world);
        let mut player = rig.join([48.5, 364.0, 48.5], 0.0, 0.0);
        rig.settle(&mut [&mut player]);
        if command.contains("classic") {
            // Classic is on already, but has not looked at the lamp: switch away and back.
            player.send_command("/light multiscale 4");
            rig.step();
        }
        player.send_command(&command);
        for _ in 0..ticks {
            rig.step();
        }
        player.advance().unwrap();
        let updates = player.settle(|| {}).unwrap();
        player.camera.look_by(0.0, -std::f32::consts::FRAC_PI_2);
        let mut renderer = VoxelRenderer::new(context, wgpu::TextureFormat::Rgba8UnormSrgb);
        configure_renderer(
            &mut renderer,
            player.block_table(),
            player.package().ambient_light,
            VIEW_RADIUS,
        );
        // No fog: it would hide the terrain, which is black where no light reaches.
        let mut clear = atmosphere(VIEW_RADIUS, 0);
        clear.fog_start = 1.0e6;
        clear.fog_end = 2.0e6;
        renderer.set_atmosphere(clear);
        apply_updates(&mut renderer, updates);
        let (pixels, statistics) = renderer.render_to_rgba(&player.camera, 1280, 720);
        println!("{name}: {} quads drawn", statistics.quads_drawn);
        write_rgba(&format!("mode_{name}"), &pixels);
    }
}
