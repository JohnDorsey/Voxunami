//! What is on the player's screen besides the world, and what their keys and mouse do to it.
//!
//! The shell builds a list of interfaces from the state of the session: its status lines (where the player is, the
//! HUD lines of the client plugins), the command being typed and the reply to the last one, the forms the server has
//! shown, and the question the server is waiting for an answer to. Everything here is a pure function of that state and
//! of what the keys and the mouse did, so it can be tested without a window; the window only feeds it input and draws what
//! [`ui::layout`] makes of it.
//!
//! The topmost modal interface (the last one in the list that is modal) takes the player's input: the world hears
//! nothing while there is one.

use plugin_host::{FormId, Prompt, PromptId, UiAction};
use ui::{Anchor, Color, Direction, Layout, MAXIMUM_TEXT_CHARACTERS, Node, Panel, Ui, next_button};

const STATUS_COLOR: Color = Color([20, 20, 30]);
const COMMAND_COLOR: Color = Color([10, 10, 10]);
const QUESTION_COLOR: Color = Color([30, 30, 70]);
/// Text that is not for the player to read closely: where they are and how the game feels.
const DIM_TEXT: Color = Color([210, 210, 210]);
const REPLY_TEXT: Color = Color([255, 230, 150]);

/// The text the window supplies for what it knows and the session does not.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScreenText {
    /// Lines of status, top left: where the player is, the HUD lines of the plugins.
    pub status: Vec<String>,
    /// The command being typed, with its cursor, if one is.
    pub typing: Option<String>,
    /// The latest answer to a command, while it is still worth reading.
    pub reply: Option<String>,
}

/// Where an interface on the screen came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Status,
    /// A panel a client plugin asked for, by its place in the list.
    Overlay(usize),
    Reply,
    Typing,
    Form(FormId),
    Question(PromptId),
}

/// What the player's input did to the screen, for the session to tell the server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenAction {
    /// Chose option `choice` (counted from 0) of the question.
    Answer { question: PromptId, choice: u32 },
    /// Did something with a form.
    Form { form: FormId, action: UiAction },
}

/// The interfaces to draw, in order (later over earlier), and where each came from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Screen {
    pub uis: Vec<Ui>,
    pub sources: Vec<Source>,
}

/// The most letters a line of the window's own text (its status, the command typed, the reply to the last one) may have. The layout breaks it into lines that fit the screen
/// (`ui::layout`), so this is only a limit on what a server's very long reply can make the client draw.
const MAXIMUM_LINE_CHARACTERS: usize = 2000;

fn shortened(text: &str) -> String {
    cut_to(text, MAXIMUM_TEXT_CHARACTERS)
}

/// `text`, or its first letters and dots if it has more than `limit`.
fn cut_to(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    let kept: String = text.chars().take(limit - 3).collect();
    format!("{kept}...")
}

fn lines(anchor: Anchor, color: Color, text_color: Color, lines: &[String]) -> Ui {
    Ui {
        modal: false,
        anchor,
        root: Panel {
            title: None,
            color,
            direction: Direction::Column,
            children: lines
                .iter()
                .map(|line| Node::Label {
                    text: cut_to(line, MAXIMUM_LINE_CHARACTERS),
                    color: text_color,
                })
                .collect(),
        },
    }
}

/// The question the server is waiting for an answer to, as a modal panel: its options are buttons, numbered, and if others
/// are waiting after it the panel says how many.
fn question_ui(question: &Prompt, waiting: usize) -> Ui {
    let mut children: Vec<Node> = question
        .options
        .iter()
        .enumerate()
        .map(|(index, option)| Node::Button {
            id: index as u32,
            text: shortened(&format!("{}) {option}", index + 1)),
        })
        .collect();
    if waiting > 1 {
        children.push(Node::Label {
            text: format!("{} more waiting", waiting - 1),
            color: DIM_TEXT,
        });
    }
    Ui {
        modal: true,
        anchor: Anchor::Center,
        root: Panel {
            title: Some(if question.title.is_empty() {
                "Question".to_owned()
            } else {
                shortened(&question.title)
            }),
            color: QUESTION_COLOR,
            direction: Direction::Column,
            children,
        },
    }
}

/// Builds the screen: the status at the top left, the panels of the client plugins, what the server has shown (panels first,
/// then modal forms), the command line and its reply at the bottom, and the question over everything.
pub fn build(
    questions: &[Prompt],
    forms: &[(FormId, Ui)],
    overlays: &[Ui],
    text: &ScreenText,
) -> Screen {
    let mut screen = Screen::default();
    let mut add = |source: Source, ui: Ui| {
        screen.sources.push(source);
        screen.uis.push(ui);
    };
    if !text.status.is_empty() {
        add(
            Source::Status,
            lines(Anchor::TopLeft, STATUS_COLOR, DIM_TEXT, &text.status),
        );
    }
    for (index, overlay) in overlays.iter().enumerate() {
        add(Source::Overlay(index), overlay.clone());
    }
    // The command line sits closest to the bottom edge and the reply stacks above it.
    if let Some(typing) = &text.typing {
        add(
            Source::Typing,
            lines(
                Anchor::BottomLeft,
                COMMAND_COLOR,
                Color::WHITE,
                std::slice::from_ref(typing),
            ),
        );
    }
    if let Some(reply) = &text.reply {
        add(
            Source::Reply,
            lines(
                Anchor::BottomLeft,
                COMMAND_COLOR,
                REPLY_TEXT,
                std::slice::from_ref(reply),
            ),
        );
    }
    for modal in [false, true] {
        for (id, ui) in forms.iter().filter(|(_, ui)| ui.modal == modal) {
            add(Source::Form(*id), ui.clone());
        }
    }
    if let Some(oldest) = questions.first() {
        add(
            Source::Question(PromptId::new(oldest.id.get())),
            question_ui(oldest, questions.len()),
        );
    }
    screen
}

impl Screen {
    /// The index of the interface that has the player's input: the last modal one.
    pub fn modal(&self) -> Option<usize> {
        self.uis.iter().rposition(|ui| ui.modal)
    }

    /// What pressing a button of the interface at `index` does.
    fn pressed(&self, index: usize, button: u32) -> Option<ScreenAction> {
        match self.sources.get(index)? {
            Source::Question(question) => Some(ScreenAction::Answer {
                question: *question,
                choice: button,
            }),
            Source::Form(form) => Some(ScreenAction::Form {
                form: *form,
                action: UiAction::Pressed(button),
            }),
            _ => None,
        }
    }

    /// What a click at a pixel does: presses the button there, if it is one of the interface that has the input.
    pub fn click(&self, layout: &Layout, x: i32, y: i32) -> Option<ScreenAction> {
        let modal = self.modal()?;
        let button = layout.button_at(x, y).filter(|button| button.ui == modal)?;
        self.pressed(modal, button.id)
    }
}

/// What a key means to the interface that has the input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenKey {
    Next,
    Previous,
    /// Press the button that has the focus.
    Activate,
    /// Put the interface away.
    Dismiss,
    /// Press the button with this place in the interface, counted from 1.
    Number(usize),
}

impl Screen {
    /// What a key does: moves the focus among the buttons of the interface that has the input, or presses one, or puts it
    /// away (only a form can be put away: a question has to be answered). `focus` is the button that has the keyboard focus
    /// and is changed here; it is `None` when none has it.
    pub fn key(&self, focus: &mut Option<u32>, key: ScreenKey) -> Option<ScreenAction> {
        let modal = self.modal()?;
        let ids = self.uis[modal].button_ids();
        match key {
            ScreenKey::Next | ScreenKey::Previous => {
                *focus = next_button(&ids, *focus, key == ScreenKey::Next);
                None
            }
            ScreenKey::Activate => {
                let button = focus.filter(|button| ids.contains(button))?;
                self.pressed(modal, button)
            }
            ScreenKey::Number(number) => {
                let button = *ids.get(number.checked_sub(1)?)?;
                *focus = Some(button);
                self.pressed(modal, button)
            }
            ScreenKey::Dismiss => match self.sources[modal] {
                Source::Form(form) => Some(ScreenAction::Form {
                    form,
                    action: UiAction::Dismissed,
                }),
                _ => None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use plugin_host::PromptId as Id;
    use ui::{Direction as Way, Draw, layout};

    use super::*;

    fn build_without_overlays(
        questions: &[Prompt],
        forms: &[(FormId, Ui)],
        text: &ScreenText,
    ) -> Screen {
        build(questions, forms, &[], text)
    }

    fn question(id: u64, title: &str, options: &[&str]) -> Prompt {
        Prompt {
            id: Id::new(id),
            title: title.to_owned(),
            options: options.iter().map(|option| (*option).to_owned()).collect(),
        }
    }

    fn menu(modal: bool, buttons: &[u32]) -> Ui {
        Ui {
            modal,
            anchor: Anchor::Center,
            root: Panel {
                title: Some("menu".to_owned()),
                color: Color::BLACK,
                direction: Way::Column,
                children: buttons
                    .iter()
                    .map(|id| Node::Button {
                        id: *id,
                        text: format!("b{id}"),
                    })
                    .collect(),
            },
        }
    }

    fn text(status: &[&str], typing: Option<&str>, reply: Option<&str>) -> ScreenText {
        ScreenText {
            status: status.iter().map(|line| (*line).to_owned()).collect(),
            typing: typing.map(str::to_owned),
            reply: reply.map(str::to_owned),
        }
    }

    #[test]
    fn nothing_to_show_is_an_empty_screen_and_each_thing_has_its_place_in_the_order() {
        assert_eq!(
            build_without_overlays(&[], &[], &ScreenText::default()),
            Screen::default()
        );
        let screen = build_without_overlays(
            &[question(4, "Delete?", &["Keep", "Delete"])],
            &[
                (FormId::new(2), menu(true, &[1])),
                (FormId::new(1), menu(false, &[])),
            ],
            &text(&["players 1"], Some("/gamemode"), Some("done")),
        );
        // The status, the command line and its reply, then the panels of the server before its modals, then the question.
        assert_eq!(
            screen.sources,
            [
                Source::Status,
                Source::Typing,
                Source::Reply,
                Source::Form(FormId::new(1)),
                Source::Form(FormId::new(2)),
                Source::Question(Id::new(4)),
            ]
        );
        assert_eq!(screen.uis.len(), 6);
        assert_eq!(screen.modal(), Some(5), "the question has the input");
    }

    #[test]
    fn a_question_is_a_modal_panel_with_a_numbered_button_for_each_option_and_says_how_many_wait() {
        let one = build_without_overlays(
            &[question(7, "Delete the level?", &["Keep it", "Delete it"])],
            &[],
            &ScreenText::default(),
        );
        let ui = &one.uis[0];
        assert!(ui.modal);
        assert_eq!(ui.root.title.as_deref(), Some("Delete the level?"));
        let labels: Vec<(u32, &str)> = ui
            .root
            .children
            .iter()
            .filter_map(|node| match node {
                Node::Button { id, text } => Some((*id, text.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(labels, [(0, "1) Keep it"), (1, "2) Delete it")]);
        assert_eq!(ui.root.children.len(), 2, "nothing else when none wait");

        let three = build_without_overlays(
            &[
                question(1, "", &["A"]),
                question(2, "second", &["B"]),
                question(3, "third", &["C"]),
            ],
            &[],
            &ScreenText::default(),
        );
        assert_eq!(
            three.sources,
            [Source::Question(Id::new(1))],
            "only the oldest is shown"
        );
        assert_eq!(
            three.uis[0].root.title.as_deref(),
            Some("Question"),
            "an empty title is a plain question"
        );
        assert!(matches!(
            three.uis[0].root.children.last(),
            Some(Node::Label { text, .. }) if text == "2 more waiting"
        ));
    }

    #[test]
    fn text_longer_than_an_interface_allows_is_cut_with_dots_so_that_what_a_server_describes_is_always_allowed()
     {
        // A question (a server's words) is cut at what a description may hold, and passes the check.
        let long = "x".repeat(300);
        let screen =
            build_without_overlays(&[question(1, &long, &[&long])], &[], &ScreenText::default());
        for ui in &screen.uis {
            assert_eq!(ui.check(), Ok(()));
        }
        // The window's own lines are wrapped to the screen by the layout and cut only at a very much longer length.
        let screen = build_without_overlays(&[], &[], &text(&[&long], Some(&long), Some(&long)));
        for ui in &screen.uis {
            let Node::Label { text, .. } = &ui.root.children[0] else {
                panic!("a line of text");
            };
            assert_eq!(text, &long, "300 letters are shown whole");
        }
        let endless = "y".repeat(5000);
        let screen = build_without_overlays(&[], &[], &text(&[&endless], Some(&endless), None));
        for ui in &screen.uis {
            let Node::Label { text, .. } = &ui.root.children[0] else {
                panic!("a line of text");
            };
            assert_eq!(text.chars().count(), 2000);
            assert!(text.ends_with("..."));
        }
    }

    /// The text drawn, with where each line starts.
    fn drawn_text(laid: &ui::Layout) -> Vec<(i32, i32, String)> {
        laid.draws
            .iter()
            .filter_map(|draw| match draw {
                ui::Draw::Text { x, y, text, .. } => Some((*x, *y, text.clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_long_reply_and_long_coordinates_are_wrapped_to_the_screen_and_lose_nothing() {
        let places = "places: voxunami_cave:planet, moons:moon, base_blocks:great_river_narrowest, base_blocks:great_river_widest. /tp PLACE goes to the ground of a body nearest to you, or in view of a landmark; /tp PLACE LATITUDE LONGITUDE and /tp PLACE X Y Z (in the body's own axes) say where on a body; /tp X Y Z goes to coordinates of the level you are in";
        let coordinates = ["x", "y", "z"]
            .map(|axis| {
                format!(
                    "{axis}{}0x1fffffffffffffffffffffffffffff",
                    ui::NO_BREAK_SPACE
                )
            })
            .join(" ");
        let status = format!("players 1 | {coordinates}");
        let screen = build_without_overlays(&[], &[], &text(&[&status], None, Some(places)));
        // A window 1280 wide and 720 high draws at scale 2: 614 units of text, 76 letters.
        let (width, height, scale) = (1280, 720, 2);
        let laid = layout(&screen.uis, (width, height), scale, None);
        let drawn = drawn_text(&laid);
        for (x, y, line) in &drawn {
            assert!(line.chars().count() <= 76, "{line:?}");
            assert!(
                x + line.chars().count() as i32 * 8 * scale <= width,
                "{line:?} runs off the right"
            );
            assert!(
                *y >= 0 && y + 8 * scale <= height,
                "{line:?} runs off the top or bottom"
            );
        }
        let said: Vec<&str> = drawn.iter().map(|(_, _, line)| line.as_str()).collect();
        // Everything that was said is there, in order, once.
        let all = said.join(" ");
        assert!(all.contains("players 1 |"), "{all}");
        assert!(all.contains(places), "{all}");
        // The coordinates are on lines whole: a letter is never the last thing on a line.
        let status_lines: Vec<&&str> = said
            .iter()
            .filter(|line| !places.contains(**line))
            .collect();
        assert!(status_lines.len() >= 2, "{status_lines:?}");
        for line in &status_lines {
            assert!(
                !line.ends_with(" x") && !line.ends_with(" y") && !line.ends_with(" z"),
                "{line:?}"
            );
        }
    }

    #[test]
    fn a_click_presses_a_button_of_the_interface_that_has_the_input_and_of_no_other() {
        let screen = build_without_overlays(
            &[question(5, "Q", &["Yes", "No"])],
            &[(FormId::new(3), menu(true, &[10, 11]))],
            &ScreenText::default(),
        );
        let laid = layout(&screen.uis, (800, 600), 2, None);
        // The question is topmost: its buttons answer; the form's, though drawn, do not.
        let buttons: Vec<_> = laid.buttons.iter().collect();
        assert_eq!(buttons.len(), 4);
        for button in &buttons {
            let (x, y) = (button.area.x + 1, button.area.y + 1);
            let action = screen.click(&laid, x, y);
            if button.ui == 1 {
                assert_eq!(
                    action,
                    Some(ScreenAction::Answer {
                        question: Id::new(5),
                        choice: button.id
                    })
                );
            } else {
                assert_eq!(action, None, "a form beneath the question takes no clicks");
            }
        }
        // A click that is on nothing does nothing.
        assert_eq!(screen.click(&laid, 0, 0), None);
    }

    #[test]
    fn a_click_on_a_form_that_has_the_input_presses_its_button() {
        let screen = build_without_overlays(
            &[],
            &[(FormId::new(3), menu(true, &[10, 11]))],
            &ScreenText::default(),
        );
        let laid = layout(&screen.uis, (800, 600), 1, None);
        let second = laid.buttons.iter().find(|button| button.id == 11).unwrap();
        assert_eq!(
            screen.click(&laid, second.area.x, second.area.y),
            Some(ScreenAction::Form {
                form: FormId::new(3),
                action: UiAction::Pressed(11)
            })
        );
        // Nothing modal: a click presses nothing, whatever is under it.
        let free = build_without_overlays(
            &[],
            &[(FormId::new(3), menu(false, &[]))],
            &text(&["x"], None, None),
        );
        let laid = layout(&free.uis, (800, 600), 1, None);
        assert_eq!(free.click(&laid, 5, 5), None);
        assert_eq!(free.modal(), None);
    }

    #[test]
    fn the_keyboard_moves_the_focus_round_the_buttons_and_presses_the_one_that_has_it() {
        let screen = build_without_overlays(
            &[],
            &[(FormId::new(3), menu(true, &[10, 11, 12]))],
            &ScreenText::default(),
        );
        let mut focus = None;
        let form = |action| {
            Some(ScreenAction::Form {
                form: FormId::new(3),
                action,
            })
        };
        assert_eq!(
            screen.key(&mut focus, ScreenKey::Activate),
            None,
            "nothing has the focus yet"
        );
        assert_eq!(screen.key(&mut focus, ScreenKey::Next), None);
        assert_eq!(focus, Some(10));
        screen.key(&mut focus, ScreenKey::Next);
        screen.key(&mut focus, ScreenKey::Next);
        screen.key(&mut focus, ScreenKey::Next);
        assert_eq!(focus, Some(10), "round the end");
        screen.key(&mut focus, ScreenKey::Previous);
        assert_eq!(focus, Some(12));
        assert_eq!(
            screen.key(&mut focus, ScreenKey::Activate),
            form(UiAction::Pressed(12))
        );
        // A number presses the button in that place and gives it the focus.
        assert_eq!(
            screen.key(&mut focus, ScreenKey::Number(2)),
            form(UiAction::Pressed(11))
        );
        assert_eq!(focus, Some(11));
        assert_eq!(screen.key(&mut focus, ScreenKey::Number(0)), None);
        assert_eq!(
            screen.key(&mut focus, ScreenKey::Number(4)),
            None,
            "there is no fourth button"
        );
        // Escape puts a form away.
        assert_eq!(
            screen.key(&mut focus, ScreenKey::Dismiss),
            form(UiAction::Dismissed)
        );
        // A focus on a button that has gone is not pressed.
        let mut stale = Some(99);
        assert_eq!(screen.key(&mut stale, ScreenKey::Activate), None);
    }

    #[test]
    fn a_question_has_to_be_answered_escape_does_not_put_it_away() {
        let screen = build_without_overlays(
            &[question(5, "Q", &["Yes", "No"])],
            &[],
            &ScreenText::default(),
        );
        let mut focus = None;
        assert_eq!(screen.key(&mut focus, ScreenKey::Dismiss), None);
        assert_eq!(
            screen.key(&mut focus, ScreenKey::Number(2)),
            Some(ScreenAction::Answer {
                question: Id::new(5),
                choice: 1
            })
        );
    }

    #[test]
    fn with_nothing_modal_no_key_does_anything() {
        let screen = build_without_overlays(
            &[],
            &[(FormId::new(1), menu(false, &[]))],
            &text(&["a"], None, None),
        );
        let mut focus = None;
        for key in [
            ScreenKey::Next,
            ScreenKey::Previous,
            ScreenKey::Activate,
            ScreenKey::Dismiss,
            ScreenKey::Number(1),
        ] {
            assert_eq!(screen.key(&mut focus, key), None);
        }
        assert_eq!(focus, None);
    }

    #[test]
    fn the_status_is_at_the_top_left_and_the_command_line_at_the_bottom_with_the_reply_above_it() {
        let screen =
            build_without_overlays(&[], &[], &text(&["players 1"], Some("/ho"), Some("ok")));
        let laid = layout(&screen.uis, (800, 600), 1, None);
        let borders: Vec<ui::Rect> = laid
            .draws
            .iter()
            .filter_map(|draw| match draw {
                Draw::Rect { area, color } if *color == ui::BORDER_COLOR => Some(*area),
                _ => None,
            })
            .collect();
        assert_eq!(borders.len(), 3);
        let (status, typing, reply) = (borders[0], borders[1], borders[2]);
        assert_eq!((status.x, status.y), (8, 8));
        assert_eq!(typing.x, 8);
        assert_eq!(typing.y + typing.height, 600 - 8, "on the bottom margin");
        assert!(
            reply.y + reply.height < typing.y,
            "the reply is above what is typed"
        );
    }

    #[test]
    fn the_panels_of_client_plugins_come_after_the_status_before_the_command_line_and_take_no_input()
     {
        let hotbar = Ui {
            modal: false,
            anchor: Anchor::Bottom,
            root: Panel {
                title: None,
                color: Color::BLACK,
                direction: Way::Row,
                children: vec![Node::Button {
                    id: 4,
                    text: "a button that does nothing".to_owned(),
                }],
            },
        };
        let screen = build(
            &[],
            &[],
            &[hotbar.clone(), hotbar],
            &text(&["players 1"], Some("/x"), None),
        );
        assert_eq!(
            screen.sources,
            [
                Source::Status,
                Source::Overlay(0),
                Source::Overlay(1),
                Source::Typing
            ]
        );
        // Nothing modal: clicks and keys reach none of them.
        assert_eq!(screen.modal(), None);
        let laid = layout(&screen.uis, (800, 600), 1, None);
        let button = &laid.buttons[0];
        assert_eq!(
            screen.click(&laid, button.area.x + 1, button.area.y + 1),
            None
        );
        let mut focus = None;
        assert_eq!(screen.key(&mut focus, ScreenKey::Number(1)), None);
    }
}
