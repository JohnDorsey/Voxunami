//! What is on the screen besides the world: a form or a question the server has put there, drawn over the picture on every
//! graphics adapter, what a click or a key on it does, and that a replay draws the same.
//!
//! These need a GPU. Without one they print a notice and pass, unless `VOX_REQUIRE_GPU` is set.

mod common;

use std::sync::{Arc, Mutex};

use common::{Player, Rig, SEED, VIEW_RADIUS};
use game_shell::files::{open_recording, record_connection};
use game_shell::screen::{Screen, ScreenKey, ScreenText, Source};
use game_shell::session::{Mode, Session};
use game_shell::view::{camera_at, configure_renderer, show_screen};
use game_shell::{GameContent, Identity};
use namespaced_id::NamespacedId;
use plugin_host::{
    CommandSpec, FormId, HandlerSpec, Plugin, PromptAnswered, Registrar, UiAction, UiResponded,
};
use renderer::{GraphicsContext, VoxelRenderer};
use transport::Connection;
use ui::{Anchor, Color, Direction, Node, Panel, Ui};
use voxunami_cave::content::CaveContent;
use voxunami_cave::game::Cave;
use voxunami_cave::plugins::cave_plugins;

const WIDTH: u32 = 320;
const HEIGHT: u32 = 180;

fn contexts() -> Vec<GraphicsContext> {
    let contexts = GraphicsContext::all_physical_adapters();
    if contexts.is_empty() {
        assert!(
            std::env::var_os("VOX_REQUIRE_GPU").is_none(),
            "VOX_REQUIRE_GPU is set but no graphics adapter is available"
        );
        eprintln!("no graphics adapter available; skipping GPU test");
    }
    contexts
}

fn menu() -> Ui {
    Ui {
        modal: true,
        anchor: Anchor::Center,
        root: Panel {
            title: Some("Board the ship?".to_owned()),
            color: Color([20, 20, 40]),
            direction: Direction::Column,
            children: vec![
                Node::Button {
                    id: 1,
                    text: "Yes".to_owned(),
                },
                Node::Button {
                    id: 2,
                    text: "No".to_owned(),
                },
            ],
        },
    }
}

type Heard = Arc<Mutex<Vec<UiResponded>>>;
type Answers = Arc<Mutex<Vec<PromptAnswered>>>;

/// Shows the caller the menu with `/menu`, closes it with `/shut`, asks a question with `/ask`.
#[derive(Debug)]
struct Shower {
    heard: Heard,
    answers: Answers,
    last: Arc<Mutex<Option<FormId>>>,
}

impl Plugin for Shower {
    fn id(&self) -> NamespacedId {
        "test:shower".parse().unwrap()
    }

    fn register(&self, registrar: &mut Registrar<'_>) {
        let last = Arc::clone(&self.last);
        registrar.command(
            CommandSpec::new("menu", "shows"),
            move |context, client, _| {
                *last.lock().unwrap() = context.show_ui(client, &menu()).ok();
            },
        );
        let last = Arc::clone(&self.last);
        registrar.command(
            CommandSpec::new("shut", "closes"),
            move |context, client, _| {
                if let Some(id) = last.lock().unwrap().take() {
                    context.close_ui(client, id);
                }
            },
        );
        registrar.command(CommandSpec::new("ask", "asks"), |context, client, _| {
            let _ = context.prompt(client, "Delete the level?", &["Keep it", "Delete it"]);
        });
        let heard = Arc::clone(&self.heard);
        registrar.on::<UiResponded>(HandlerSpec::new("test:hear_forms"), move |event, _| {
            heard.lock().unwrap().push(*event)
        });
        let answers = Arc::clone(&self.answers);
        registrar.on::<PromptAnswered>(HandlerSpec::new("test:hear_answers"), move |event, _| {
            answers.lock().unwrap().push(*event)
        });
    }
}

struct Setup {
    rig: Rig,
    heard: Heard,
    answers: Answers,
}

fn setup() -> Setup {
    let content = CaveContent::new();
    let world = content.new_world(SEED);
    let (heard, answers) = (Heard::default(), Answers::default());
    let mut plugins = cave_plugins(&content);
    plugins.push(Box::new(Shower {
        heard: Arc::clone(&heard),
        answers: Arc::clone(&answers),
        last: Arc::default(),
    }));
    Setup {
        rig: Rig::with_plugins(content, world, plugins),
        heard,
        answers,
    }
}

fn tick(rig: &mut Rig, player: &mut Player, count: usize) {
    for _ in 0..count {
        rig.step();
        player.advance().unwrap();
    }
}

/// The screen of a session without the panels of Cave's own client plugins (the hotbar is always up, and these tests are about what the
/// server shows).
fn without_overlays(screen: Screen) -> Screen {
    let (sources, uis): (Vec<Source>, Vec<Ui>) = screen
        .sources
        .into_iter()
        .zip(screen.uis)
        .filter(|(source, _)| !matches!(source, Source::Overlay(_)))
        .unzip();
    Screen { uis, sources }
}

fn screen_of<C: Connection>(session: &mut Session<C, Cave>) -> Screen {
    without_overlays(session.screen(&ScreenText::default()))
}

/// The picture of an empty sky with the session's screen over it: the image and the layout the buttons were placed by.
fn picture<C: Connection>(
    context: &GraphicsContext,
    session: &mut Session<C, Cave>,
) -> (Vec<u8>, ui::Layout) {
    let mut renderer = VoxelRenderer::new(context, wgpu::TextureFormat::Rgba8UnormSrgb);
    configure_renderer(
        &mut renderer,
        session.block_table(),
        session.package().ambient_light,
        VIEW_RADIUS,
    );
    let laid = show_screen(&mut renderer, &screen_of(session), (WIDTH, HEIGHT), None);
    // The integrated AMD GPU of the development machine now and then leaves out a pixel or two of thin rectangles (see
    // `settled` in the renderer's tests), so the picture is the first one that the frame after it repeats exactly.
    let mut previous = renderer.render_to_rgba(&session.camera, WIDTH, HEIGHT).0;
    for _ in 0..30 {
        let next = renderer.render_to_rgba(&session.camera, WIDTH, HEIGHT).0;
        if next == previous {
            return (next, laid);
        }
        previous = next;
    }
    panic!("the picture never settled");
}

fn pixel(image: &[u8], x: i32, y: i32) -> [u8; 3] {
    let start = ((y as u32 * WIDTH + x as u32) * 4) as usize;
    [image[start], image[start + 1], image[start + 2]]
}

fn close(first: [u8; 3], second: [u8; 3]) -> bool {
    (0..3).all(|channel| first[channel].abs_diff(second[channel]) <= 1)
}

#[test]
fn a_form_is_drawn_over_the_picture_dimming_it_with_its_buttons_where_the_layout_has_them() {
    for context in contexts() {
        let name = renderer::describe(&context.adapter);
        let Setup { mut rig, .. } = setup();
        let mut player = rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
        rig.settle(&mut [&mut player]);
        let (plain, no_layout) = picture(&context, &mut player);
        assert!(no_layout.draws.is_empty() && no_layout.buttons.is_empty());

        player.send_command("/menu");
        tick(&mut rig, &mut player, 2);
        let (with_form, laid) = picture(&context, &mut player);
        // The corner is dimmed (the form is modal); the button is its own colour.
        assert!(
            pixel(&with_form, 0, 0)[2] < pixel(&plain, 0, 0)[2],
            "{name}: dimmed"
        );
        assert_eq!(laid.buttons.len(), 2);
        for button in &laid.buttons {
            let (x, y) = (button.area.x + button.area.width - 2, button.area.y + 1);
            assert!(
                close(pixel(&with_form, x, y), [60, 60, 70]),
                "{name}: button {} is {:?}",
                button.id,
                pixel(&with_form, x, y)
            );
        }
        assert_ne!(plain, with_form, "{name}");
    }
}

#[test]
fn a_click_on_a_button_and_a_key_reach_the_plugin_and_a_form_the_server_closes_is_gone_from_the_screen()
 {
    let Setup { mut rig, heard, .. } = setup();
    let mut player = rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    player.send_command("/menu");
    tick(&mut rig, &mut player, 2);

    let screen = screen_of(&mut player);
    assert_eq!(screen.sources, [Source::Form(FormId::new(1))]);
    let laid = ui::layout(
        &screen.uis,
        (WIDTH as i32, HEIGHT as i32),
        ui::scale_for_height(HEIGHT),
        None,
    );
    let no = laid.buttons.iter().find(|button| button.id == 2).unwrap();
    let action = screen
        .click(&laid, no.area.x + 3, no.area.y + 3)
        .expect("a click on a button");
    player.apply_screen_action(action);
    // The key 1 presses the first button; Escape puts the form away.
    let mut focus = None;
    player.apply_screen_action(screen.key(&mut focus, ScreenKey::Number(1)).unwrap());
    tick(&mut rig, &mut player, 2);
    let heard_now: Vec<UiAction> = heard.lock().unwrap().iter().map(|r| r.action).collect();
    assert_eq!(heard_now, [UiAction::Pressed(2), UiAction::Pressed(1)]);

    player.apply_screen_action(screen.key(&mut focus, ScreenKey::Dismiss).unwrap());
    tick(&mut rig, &mut player, 2);
    assert_eq!(
        heard.lock().unwrap().last().unwrap().action,
        UiAction::Dismissed
    );
    assert_eq!(
        screen_of(&mut player),
        Screen::default(),
        "put away, and gone"
    );

    // One the plugin closes goes too.
    player.send_command("/menu");
    tick(&mut rig, &mut player, 2);
    assert_eq!(screen_of(&mut player).uis.len(), 1);
    player.send_command("/shut");
    tick(&mut rig, &mut player, 2);
    assert_eq!(screen_of(&mut player), Screen::default());
}

#[test]
fn a_question_is_a_modal_panel_that_a_number_key_or_a_click_answers_and_the_server_hears_which() {
    let Setup {
        mut rig, answers, ..
    } = setup();
    let mut player = rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    player.send_command("/ask");
    tick(&mut rig, &mut player, 2);
    let screen = screen_of(&mut player);
    assert!(matches!(screen.sources[..], [Source::Question(_)]));
    assert_eq!(screen.modal(), Some(0));
    // Escape does not put a question away.
    let mut focus = None;
    assert_eq!(screen.key(&mut focus, ScreenKey::Dismiss), None);
    // The second option, by its number.
    player.apply_screen_action(screen.key(&mut focus, ScreenKey::Number(2)).unwrap());
    tick(&mut rig, &mut player, 2);
    let heard: Vec<usize> = answers.lock().unwrap().iter().map(|a| a.choice).collect();
    assert_eq!(heard, [1]);
    assert_eq!(
        screen_of(&mut player),
        Screen::default(),
        "answered, and gone"
    );

    // And by a click on the first.
    player.send_command("/ask");
    tick(&mut rig, &mut player, 2);
    let screen = screen_of(&mut player);
    let laid = ui::layout(
        &screen.uis,
        (WIDTH as i32, HEIGHT as i32),
        ui::scale_for_height(HEIGHT),
        None,
    );
    let first = laid.buttons.iter().find(|button| button.id == 0).unwrap();
    player.apply_screen_action(
        screen
            .click(&laid, first.area.x + 1, first.area.y + 1)
            .unwrap(),
    );
    tick(&mut rig, &mut player, 2);
    assert_eq!(answers.lock().unwrap().last().unwrap().choice, 0);
}

#[test]
fn a_replay_draws_the_form_exactly_as_it_was_drawn_live_and_stops_when_it_was_closed() {
    let directory =
        std::env::temp_dir().join(format!("voxunami_cave_screen_{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("session.voxreplay");
    let Setup { mut rig, .. } = setup();
    let recorded: Box<dyn Connection> =
        record_connection(rig.connector.connect().unwrap(), &path).unwrap();
    let mut player = rig.join_with(recorded, [16.5, 110.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    player.send_command("/menu");
    tick(&mut rig, &mut player, 3);
    // What the live player saw with the form up.
    let live: Vec<Vec<u8>> = contexts()
        .iter()
        .map(|context| picture(context, &mut player).0)
        .collect();
    player.send_command("/shut");
    tick(&mut rig, &mut player, 3);
    let live_after: Vec<Vec<u8>> = contexts()
        .iter()
        .map(|context| picture(context, &mut player).0)
        .collect();
    drop(player);

    let connection: Box<dyn Connection> = Box::new(open_recording(&path).unwrap());
    let mut replay = Session::<_, Cave>::connect(
        connection,
        &Identity::generate(),
        Mode::Replaying,
        VIEW_RADIUS,
        camera_at([0.0; 3], 0.0, 0.0, VIEW_RADIUS),
        || {},
    )
    .unwrap();
    // Play the recording to the tick where the form was up, and then to the end.
    let mut up = None;
    let mut ended = false;
    while replay.advance_one_tick().is_ok() {
        if !replay.forms().is_empty() {
            up = Some(
                contexts()
                    .iter()
                    .map(|context| picture(context, &mut replay).0)
                    .collect::<Vec<_>>(),
            );
        } else if up.is_some() && !ended {
            ended = true;
        }
    }
    assert!(ended, "the form was closed again");
    let up = up.expect("the form was up");
    let after: Vec<Vec<u8>> = contexts()
        .iter()
        .map(|context| picture(context, &mut replay).0)
        .collect();
    // Pictures of the same camera (the replay follows the recorded player) with and without the form: identical to live.
    for (index, _) in contexts().iter().enumerate() {
        let differing: Vec<(usize, usize)> = up[index]
            .chunks(4)
            .zip(live[index].chunks(4))
            .enumerate()
            .filter(|(_, (replayed, live))| replayed != live)
            .map(|(position, _)| (position % WIDTH as usize, position / WIDTH as usize))
            .collect();
        assert!(
            differing.is_empty(),
            "adapter {index}: {} pixels differ, such as {:?}",
            differing.len(),
            &differing[..differing.len().min(10)]
        );
        assert!(
            after[index] == live_after[index],
            "after it: the replayed picture differs from the live one"
        );
        assert_ne!(live[index], live_after[index]);
    }
    std::fs::remove_dir_all(&directory).unwrap();
}

/// Writes a picture to the directory named by `VOX_SAVE_IMAGES`, if there is one, to look at.
fn save_picture(name: &str, adapter: &str, image: &[u8], width: u32, height: u32) {
    let Some(directory) = std::env::var_os("VOX_SAVE_IMAGES").map(std::path::PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&directory).expect("image directory can be created");
    let adapter: String = adapter
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    let file =
        std::fs::File::create(directory.join(format!("{name}_{adapter}.png"))).expect("image file");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(image))
        .expect("the PNG is written");
}

#[test]
fn a_screen_with_everything_on_it_is_drawn_where_the_layout_says_and_can_be_looked_at() {
    const BIG_WIDTH: u32 = 640;
    const BIG_HEIGHT: u32 = 360;
    for context in contexts() {
        let name = renderer::describe(&context.adapter);
        let Setup { mut rig, .. } = setup();
        let mut player = rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
        rig.settle(&mut [&mut player]);
        player.send_command("/menu");
        player.send_command("/ask");
        tick(&mut rig, &mut player, 3);
        let text = ScreenText {
            status: vec![
                "players 1 | 0b10000, 0b1101110, 0b10000".to_owned(),
                "placing stone (whole)".to_owned(),
            ],
            typing: Some("> /gamemode survival_".to_owned()),
            reply: Some("this world is now in survival mode".to_owned()),
        };
        let screen = without_overlays(player.screen(&text));
        assert_eq!(screen.sources.len(), 5, "{:?}", screen.sources);
        let mut renderer = VoxelRenderer::new(&context, wgpu::TextureFormat::Rgba8UnormSrgb);
        configure_renderer(
            &mut renderer,
            player.block_table(),
            player.package().ambient_light,
            VIEW_RADIUS,
        );
        let laid = show_screen(&mut renderer, &screen, (BIG_WIDTH, BIG_HEIGHT), None);
        let image = renderer
            .render_to_rgba(&player.camera, BIG_WIDTH, BIG_HEIGHT)
            .0;
        save_picture("screen_everything", &name, &image, BIG_WIDTH, BIG_HEIGHT);
        let at = |x: i32, y: i32| {
            let start = ((y as u32 * BIG_WIDTH + x as u32) * 4) as usize;
            [image[start], image[start + 1], image[start + 2]]
        };
        // The status panel is under a modal, which dims it: its border is the light grey, darkened, and not black.
        let border = at(8, 8);
        assert!(
            border[0] == border[1] && border[0] > 60 && border[0] < 190,
            "{name}: {border:?}"
        );
        let question = screen.modal().unwrap();
        let buttons: Vec<_> = laid
            .buttons
            .iter()
            .filter(|button| button.ui == question)
            .collect();
        assert_eq!(buttons.len(), 2);
    }
}
