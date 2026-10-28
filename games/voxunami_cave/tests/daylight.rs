//! The sun of Cave: a day of twenty minutes that begins on a morning, over the planet (which hides it from its night side), over the overworld (whose ground hides it when
//! it sets), and nowhere else; and the sky that follows it. The clock is the world's, which the server sends, so a world that was left at noon is at noon when it is
//! started again.

mod common;

use common::{Player, Scene, say, tick_and_follow};
use game_shell::GameDefinition;
use renderer::SunOccluder;
use voxunami_cave::Cave;

fn scene_with_a_player() -> (Scene, Player) {
    let mut scene = Scene::in_a_new_world();
    let mut player = scene
        .rig
        .join_walking(Cave::default_camera().position, 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 5);
    player.take_command_replies();
    (scene, player)
}

fn level(name: &str) -> namespaced_id::NamespacedId {
    name.parse().unwrap()
}

#[test]
fn the_levels_of_cave_have_a_sun_and_no_other_level_does() {
    let space = Cave::sun(&level("voxunami_cave:mini_cosmos")).expect("the level of space");
    let overworld = Cave::sun(&level("voxunami_cave:overworld")).expect("the overworld");
    assert!(!space.flat_ground, "planets hide the sun in space");
    assert!(
        overworld.flat_ground,
        "the ground of the overworld hides it"
    );
    assert_eq!(
        Cave::sun(&level("my_pack:somewhere")),
        None,
        "a level Cave knows nothing of"
    );
    // The same sun, the same day.
    assert_eq!(
        (space.day_seconds, space.start_hour, space.noon, space.axis),
        (
            overworld.day_seconds,
            overworld.start_hour,
            overworld.noon,
            overworld.axis
        )
    );
    // A day of twenty minutes that begins in the morning, with the sun above the horizon by day and below it by night where the player starts (up is +y there).
    assert_eq!(space.day_seconds, 1_200);
    assert!((6.0..9.0).contains(&space.start_hour));
    let up = |hours: f64| space.direction_at(hours)[1];
    assert!(up(space.start_hour) > 0.1, "the world begins by day");
    assert!(up(12.0) > 0.9, "the noon sun is high");
    assert!(up(0.0) < -0.9, "and the midnight sun is under the planet");
    // Noon is when the sun is highest.
    assert!((0..240).all(|step| up(f64::from(step) * 0.1) <= up(12.0) + 1e-6));
    // The axis is across noon, so the path is a great circle: 12 hours above the horizon, 12 below.
    let above = (0..2400)
        .filter(|step| up(f64::from(*step) * 0.01) > 0.0)
        .count();
    assert!((1195..1205).contains(&above), "{above}");
}

#[test]
fn a_new_world_begins_on_a_morning_on_the_ground_of_the_planet_with_the_planet_in_the_suns_way() {
    let (_scene, mut player) = scene_with_a_player();
    let sun = Cave::sun(&level("voxunami_cave:mini_cosmos")).unwrap();
    // The sun is where the world's clock puts it (how many ticks old the world is when the test looks depends on how fast the machine is, so the test says it by the clock),
    // and a world of clock 0 is at the start hour.
    let view = player.sun_view().expect("the level of space has a sun");
    let clock = player.client().world().world_clock();
    assert_eq!(view.hours, sun.hours_at(clock, 20));
    assert_eq!(sun.hours_at(0, 20), sun.start_hour);
    // The morning, as a player who joins at the start of a world has it.
    let start = sun.start_hour;
    player.client_command(&format!(".time {start}"));
    let view = player.sun_view().unwrap();
    assert_eq!(view.hours, start);
    assert_eq!(view.daylight, 1.0, "the player stands in the morning sun");
    assert_eq!(view.sun_light, 255);
    assert!(view.sky_light > 50, "{}", view.sky_light);
    assert!(view.sky_color[2] > 0.8, "a blue sky: {:?}", view.sky_color);
    // The planet is the one ball in the sun's way: its solid core, about the middle of the world, a little smaller than the planet.
    let [SunOccluder::Ball { centre, radius }] = view.occluders[..] else {
        panic!(
            "the planet is the only thing in the sun's way: {:?}",
            view.occluders
        );
    };
    assert!((63_000.0..63_710.0).contains(&radius), "{radius}");
    let origin = game_shell::view::camera_at([0.0; 3], 0.0, 0.0).position;
    let from_the_middle = centre.relative_to(origin);
    let distance = from_the_middle
        .iter()
        .map(|part| part * part)
        .sum::<f32>()
        .sqrt();
    assert!(
        distance < 1.0,
        "the core is at the middle of the planet: {from_the_middle:?}"
    );
}

#[test]
fn at_midnight_the_planet_hides_the_sun_and_the_sky_is_black_and_at_noon_it_is_blue() {
    let (mut scene, mut player) = scene_with_a_player();
    let _ = &mut scene;
    assert!(player.client_command(".time midnight").contains("00:00"));
    let night = player.sun_view().unwrap();
    assert_eq!(night.hours, 0.0);
    assert_eq!(night.daylight, 0.0);
    assert_eq!(night.sky_color, [0.0; 3]);
    assert!(player.client_command(".time noon").contains("12:00"));
    let noon = player.sun_view().unwrap();
    assert_eq!(noon.daylight, 1.0);
    assert!(
        noon.sky_color[2] > 0.85 && noon.sky_color[0] > 0.5,
        "{:?}",
        noon.sky_color
    );
}

#[test]
fn the_sun_sets_on_the_planet_over_some_time_and_not_all_at_once() {
    let (_scene, mut player) = scene_with_a_player();
    let mut last = 2.0;
    let mut between = 0;
    for step in 0..=60 {
        let minutes = 17 * 60 + step * 5;
        player.client_command(&format!(".time {}:{:02}", minutes / 60, minutes % 60));
        let daylight = player.sun_view().unwrap().daylight;
        assert!(
            daylight <= last + 1e-6,
            "{minutes}: {daylight} after {last}"
        );
        between += usize::from(daylight > 0.02 && daylight < 0.98);
        last = daylight;
    }
    assert_eq!(last, 0.0, "it is dark by 22:00");
    assert!(
        between >= 8,
        "the twilight takes more than a few steps of five minutes: {between}"
    );
    player.client_command(".time 17");
    assert_eq!(player.sun_view().unwrap().daylight, 1.0);
}

#[test]
fn in_space_far_from_the_planet_the_sky_is_black_even_at_noon() {
    let (mut scene, mut player) = scene_with_a_player();
    player.client_command(".time noon");
    say(
        &mut scene,
        &mut player,
        "/tp base_blocks:great_river_narrowest",
    );
    let view = player.sun_view().unwrap();
    assert_eq!(view.sky_color, [0.0; 3], "no air out there");
    assert_eq!(view.sky_light, 0);
    assert_eq!(view.sun_light, 255, "the sun is as bright");
    // And back on the ground, the air is back.
    say(&mut scene, &mut player, "/tp voxunami_cave:planet");
    let back = player.sun_view().unwrap();
    assert!(back.sky_color[2] > 0.85, "{:?}", back.sky_color);
    assert!(back.sky_light > 50);
}

#[test]
fn the_overworld_has_its_own_day_hidden_by_its_flat_ground() {
    let (mut scene, mut player) = scene_with_a_player();
    say(&mut scene, &mut player, "/level go overworld");
    tick_and_follow(&mut scene, &mut player, 3);
    player.client_command(".time noon");
    let noon = player.sun_view().unwrap();
    assert_eq!(
        noon.occluders,
        [SunOccluder::FlatGround],
        "no planet is in this level"
    );
    assert_eq!(noon.daylight, 1.0);
    assert!(noon.sky_color[2] > 0.85);
    player.client_command(".time midnight");
    let midnight = player.sun_view().unwrap();
    assert_eq!(midnight.daylight, 0.0);
    assert_eq!(midnight.sky_color, [0.0; 3]);
}

// ---- Pictures: what the sun does to what is drawn, through the session as a window runs it (`light_the_scene`) ----

use game_shell::view::{apply_updates, configure_renderer, light_the_scene};
use renderer::{GraphicsContext, VoxelRenderer};

/// The brightness of a pixel: its colour channels decoded from sRGB and added.
#[allow(clippy::disallowed_methods)] // a test of cosmetics, not simulation
fn linear_brightness(pixel: [u8; 3]) -> f64 {
    pixel
        .iter()
        .map(|&byte| {
            let value = f64::from(byte) / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        })
        .sum()
}

/// A picture of what the player sees at this time of day, on every graphics adapter: the pixel at the middle (the ground, as the player looks down at it) and the one at the
/// top left (the sky), for each adapter in turn.
fn pictures_at(
    player: &mut Player,
    updates: &[world_view::MeshUpdate],
    time: &str,
    contexts: &[GraphicsContext],
) -> Vec<(String, [u8; 3], [u8; 3])> {
    assert!(
        player
            .client_command(&format!(".time {time}"))
            .contains("the sun stays at")
    );
    let mut pictures = Vec::new();
    for context in contexts {
        let mut renderer = VoxelRenderer::new(context, wgpu::TextureFormat::Rgba8UnormSrgb);
        configure_renderer(
            &mut renderer,
            player.block_table(),
            player.package().ambient_light,
            player.fog_radius_bricks(),
        );
        apply_updates(&mut renderer, updates.to_vec());
        light_the_scene(&mut renderer, player);
        let (rgba, _) = renderer.render_to_rgba(&player.camera, 160, 90);
        let at = |x: usize, y: usize| {
            let start = (y * 160 + x) * 4;
            [rgba[start], rgba[start + 1], rgba[start + 2]]
        };
        pictures.push((renderer::describe(&context.adapter), at(80, 70), at(4, 4)));
    }
    pictures
}

#[test]
fn the_ground_and_the_sky_of_the_planet_go_from_day_to_night_as_the_world_clock_says_on_every_adapter()
 {
    let contexts = GraphicsContext::all_physical_adapters();
    if contexts.is_empty() {
        assert!(
            std::env::var_os("VOX_REQUIRE_GPU").is_none(),
            "VOX_REQUIRE_GPU is set but no graphics adapter is available"
        );
        eprintln!("no graphics adapter available; skipping GPU test");
        return;
    }
    let mut scene = Scene::in_a_new_world();
    let mut player = scene
        .rig
        .join_walking(Cave::default_camera().position, 0.0, 0.0);
    player.send_pose();
    // Everything the view makes for the renderer, from the start.
    let updates = common::view_after(&mut scene, &mut player);
    // Looking along the ground a little down, with the hills ahead and the sky above them.
    player.camera.pitch = -0.35;
    let noon = pictures_at(&mut player, &updates, "noon", &contexts);
    let evening = pictures_at(&mut player, &updates, "18:30", &contexts);
    let night = pictures_at(&mut player, &updates, "midnight", &contexts);
    for (
        (name, noon_ground, noon_sky),
        ((_, evening_ground, evening_sky), (_, night_ground, night_sky)),
    ) in noon.iter().zip(evening.iter().zip(&night))
    {
        // Noon: the ground is the green of grass in the sun and the sky is the sky's blue.
        assert!(
            noon_ground[1] > 90
                && noon_ground[1] > noon_ground[0]
                && noon_ground[1] > noon_ground[2],
            "{name}: the ground at noon is {noon_ground:?}"
        );
        assert!(
            noon_sky[2] > 200 && noon_sky[2] > noon_sky[0],
            "{name}: the sky at noon is {noon_sky:?}"
        );
        // Night: black ground and a black sky: nothing lights anything.
        assert_eq!(*night_ground, [0, 0, 0], "{name}: the ground at midnight");
        assert_eq!(*night_sky, [0, 0, 0], "{name}: the sky at midnight");
        // At half past six in the evening (the sun is below the horizon but the sky is not dark) the ground is lit by the sky alone, so darker than at noon and not black,
        // and the sky is darker too.
        let (day, dusk) = (
            linear_brightness(*noon_ground),
            linear_brightness(*evening_ground),
        );
        assert!(
            dusk > 0.0 && dusk < day * 0.5,
            "{name}: {day} at noon, {dusk} at dusk"
        );
        assert!(
            linear_brightness(*evening_sky) < linear_brightness(*noon_sky) * 0.8,
            "{name}: the sky at dusk {evening_sky:?}, at noon {noon_sky:?}"
        );
    }
}
