//! The user interface model: its limits, its byte form, and where everything goes.

use binary_codec::{ByteReader, ByteWriter, DecodeError};
use proptest::prelude::*;
use ui::{
    Anchor, Color, Direction, Draw, MAXIMUM_BAR_CELLS, MAXIMUM_DEPTH, MAXIMUM_NODES, MAXIMUM_SPACE,
    MAXIMUM_TEXT_CHARACTERS, Node, Panel, Rect, Ui, UiError, layout, next_button, scale_for_height,
};

const RED: Color = Color([200, 30, 30]);

fn label(text: &str) -> Node {
    Node::Label {
        text: text.to_owned(),
        color: Color::WHITE,
    }
}

fn button(id: u32, text: &str) -> Node {
    Node::Button {
        id,
        text: text.to_owned(),
    }
}

fn panel(children: Vec<Node>) -> Panel {
    Panel {
        title: None,
        color: Color([20, 20, 40]),
        direction: Direction::Column,
        children,
    }
}

fn ui(anchor: Anchor, modal: bool, children: Vec<Node>) -> Ui {
    Ui {
        modal,
        anchor,
        root: panel(children),
    }
}

/// An interface that uses every kind of part.
fn everything() -> Ui {
    Ui {
        modal: true,
        anchor: Anchor::BottomRight,
        root: Panel {
            title: Some("Delete the level?".to_owned()),
            color: Color([10, 20, 30]),
            direction: Direction::Row,
            children: vec![
                label("hello"),
                button(7, "Keep it"),
                button(9, "Delete it"),
                Node::Bar {
                    permille: 640,
                    cells: 10,
                    color: RED,
                },
                Node::Swatch {
                    color: Color([1, 2, 3]),
                    count: Some(64),
                    selected: true,
                },
                Node::Swatch {
                    color: Color([4, 5, 6]),
                    count: None,
                    selected: false,
                },
                Node::Space(12),
                Node::Panel(Panel {
                    title: None,
                    color: Color([0, 0, 0]),
                    direction: Direction::Column,
                    children: vec![button(11, "Inner")],
                }),
            ],
        },
    }
}

fn encoded(interface: &Ui) -> Vec<u8> {
    let mut writer = ByteWriter::new();
    interface.encode(&mut writer);
    writer.into_bytes()
}

fn decoded(bytes: &[u8]) -> Result<Ui, DecodeError> {
    let mut reader = ByteReader::new(bytes);
    let interface = Ui::decode(&mut reader)?;
    reader.finish()?;
    Ok(interface)
}

// ---- limits ----

#[test]
fn an_interface_within_the_limits_is_accepted_and_each_limit_is_exact() {
    assert_eq!(everything().check(), Ok(()));

    // Depth: the root counts as one.
    let mut deepest = panel(vec![]);
    for _ in 1..MAXIMUM_DEPTH {
        deepest = panel(vec![Node::Panel(deepest)]);
    }
    let at_the_limit = Ui {
        modal: false,
        anchor: Anchor::Center,
        root: deepest.clone(),
    };
    assert_eq!(at_the_limit.check(), Ok(()));
    let too_deep = Ui {
        root: panel(vec![Node::Panel(deepest)]),
        ..at_the_limit
    };
    assert_eq!(too_deep.check(), Err(UiError::TooDeep));

    // Nodes.
    let spaces = |count: usize| ui(Anchor::Top, false, vec![Node::Space(1); count]);
    assert_eq!(spaces(MAXIMUM_NODES).check(), Ok(()));
    assert_eq!(
        spaces(MAXIMUM_NODES + 1).check(),
        Err(UiError::TooManyNodes)
    );

    // Text, in letters, wherever it is.
    let text = |length: usize| "x".repeat(length);
    assert_eq!(
        ui(
            Anchor::Top,
            false,
            vec![label(&text(MAXIMUM_TEXT_CHARACTERS))]
        )
        .check(),
        Ok(())
    );
    for node in [
        label(&text(MAXIMUM_TEXT_CHARACTERS + 1)),
        button(1, &text(MAXIMUM_TEXT_CHARACTERS + 1)),
    ] {
        assert_eq!(
            ui(Anchor::Top, false, vec![node]).check(),
            Err(UiError::TextTooLong)
        );
    }
    let mut titled = ui(Anchor::Top, false, vec![]);
    titled.root.title = Some(text(MAXIMUM_TEXT_CHARACTERS + 1));
    assert_eq!(titled.check(), Err(UiError::TextTooLong));
    // Letters, not bytes: a long word of two-byte letters within the limit is fine.
    assert_eq!(
        ui(
            Anchor::Top,
            false,
            vec![label(&"é".repeat(MAXIMUM_TEXT_CHARACTERS))]
        )
        .check(),
        Ok(())
    );

    // Buttons are unique, also across nested panels.
    assert_eq!(
        ui(
            Anchor::Top,
            false,
            vec![button(1, "a"), Node::Panel(panel(vec![button(1, "b")]))]
        )
        .check(),
        Err(UiError::DuplicateButton)
    );

    // Bars and spaces.
    let bar = |permille, cells| Node::Bar {
        permille,
        cells,
        color: RED,
    };
    assert_eq!(
        ui(Anchor::Top, false, vec![bar(1000, MAXIMUM_BAR_CELLS)]).check(),
        Ok(())
    );
    assert_eq!(
        ui(Anchor::Top, false, vec![bar(1001, 5)]).check(),
        Err(UiError::BarOverfull)
    );
    assert_eq!(
        ui(Anchor::Top, false, vec![bar(5, MAXIMUM_BAR_CELLS + 1)]).check(),
        Err(UiError::BarTooWide)
    );
    assert_eq!(
        ui(Anchor::Top, false, vec![Node::Space(MAXIMUM_SPACE)]).check(),
        Ok(())
    );
    assert_eq!(
        ui(Anchor::Top, false, vec![Node::Space(MAXIMUM_SPACE + 1)]).check(),
        Err(UiError::SpaceTooBig)
    );
}

#[test]
fn buttons_are_listed_in_the_order_they_are_met_going_down_the_tree_and_the_keyboard_wraps_round_them()
 {
    let ids = everything().button_ids();
    assert_eq!(ids, [7, 9, 11]);
    assert_eq!(next_button(&ids, None, true), Some(7));
    assert_eq!(next_button(&ids, None, false), Some(11));
    assert_eq!(next_button(&ids, Some(7), true), Some(9));
    assert_eq!(next_button(&ids, Some(11), true), Some(7));
    assert_eq!(next_button(&ids, Some(7), false), Some(11));
    assert_eq!(next_button(&ids, Some(9), false), Some(7));
    assert_eq!(
        next_button(&ids, Some(99), true),
        Some(7),
        "a button that is gone starts again"
    );
    assert_eq!(next_button(&[], None, true), None);
}

// ---- the byte form ----

#[test]
fn an_interface_survives_its_byte_form_exactly() {
    for interface in [
        everything(),
        ui(Anchor::Center, false, vec![]),
        ui(Anchor::TopLeft, true, vec![label("a")]),
    ] {
        assert_eq!(decoded(&encoded(&interface)), Ok(interface));
    }
}

#[test]
fn every_anchor_survives() {
    for anchor in Anchor::ALL {
        let interface = ui(anchor, false, vec![]);
        assert_eq!(decoded(&encoded(&interface)).unwrap().anchor, anchor);
    }
}

#[test]
fn a_description_cut_short_anywhere_or_followed_by_more_is_an_error() {
    let bytes = encoded(&everything());
    for length in 0..bytes.len() {
        assert!(decoded(&bytes[..length]).is_err(), "cut at {length}");
    }
    let mut longer = bytes;
    longer.push(0);
    assert_eq!(decoded(&longer), Err(DecodeError::TrailingBytes(1)));
}

fn refusal(bytes: &[u8]) -> String {
    match decoded(bytes) {
        Err(error) => error.to_string(),
        Ok(interface) => panic!("accepted {interface:?}"),
    }
}

/// The bytes of an interface whose root has these children, written by hand.
fn handmade(modal: u8, anchor: u8, children: &[u8], count: u64) -> Vec<u8> {
    let mut writer = ByteWriter::new();
    writer.write_u8(modal);
    writer.write_u8(anchor);
    writer.write_u8(0); // no title
    writer.write_raw(&[1, 2, 3]);
    writer.write_u8(1); // a column
    writer.write_varint(count);
    writer.write_raw(children);
    writer.into_bytes()
}

#[test]
fn what_the_encoder_would_not_write_is_refused_with_the_reason() {
    // The control: the same bytes with nothing wrong.
    assert!(decoded(&handmade(1, 4, &[5, 3], 1)).is_ok());
    let cases: [(Vec<u8>, &str); 6] = [
        (handmade(2, 4, &[], 0), "a flag is neither 0 nor 1"),
        (handmade(0, 9, &[], 0), "an anchor is not one of the nine"),
        (
            handmade(0, 4, &[9, 0], 1),
            "a part is of a kind that is not known",
        ),
        (
            {
                let mut bytes = handmade(0, 4, &[], 0);
                bytes[6] = 2; // the direction
                bytes
            },
            "a direction is neither row nor column",
        ),
        (
            // Two buttons with one identifier: well formed, and against the limits.
            handmade(0, 4, &[2, 1, 1, b'a', 2, 1, 1, b'b'], 2),
            "two buttons have one identifier",
        ),
        (
            // A bar that is 1001 thousandths full.
            handmade(0, 4, &[3, 0xE9, 0x07, 5, 1, 2, 3], 1),
            "a bar is more than full",
        ),
    ];
    for (bytes, reason) in cases {
        assert_eq!(refusal(&bytes), reason);
    }
}

#[test]
fn a_hostile_description_is_refused_before_it_is_built() {
    // Nested one deeper than the limit: refused as it is read, not after a deep tree was made.
    let mut inner = vec![5u8, 0]; // a space
    inner = {
        let mut bytes = Vec::new();
        for _ in 0..MAXIMUM_DEPTH {
            let mut wrapped = vec![0u8, 0, 1, 2, 3, 1, 1]; // a panel node: no title, a colour, a column, one child
            wrapped.extend(&inner);
            bytes = wrapped;
            inner = bytes.clone();
        }
        bytes
    };
    assert_eq!(
        refusal(&handmade(0, 4, &inner, 1)),
        "panels are nested too deeply"
    );
    // More nodes than there may be.
    let spaces: Vec<u8> = (0..=MAXIMUM_NODES).flat_map(|_| [5u8, 0]).collect();
    assert_eq!(
        refusal(&handmade(0, 4, &spaces, MAXIMUM_NODES as u64 + 1)),
        "there are too many parts"
    );
    // A count that promises more than the message holds.
    assert_eq!(
        refusal(&handmade(0, 4, &[], 1_000_000)),
        "the data ends too early"
    );
}

fn arbitrary_color() -> impl Strategy<Value = Color> {
    any::<[u8; 3]>().prop_map(Color)
}

fn arbitrary_text() -> impl Strategy<Value = String> {
    "[ -~]{0,12}"
}

fn arbitrary_leaf(next_id: u32) -> impl Strategy<Value = Node> {
    prop_oneof![
        (arbitrary_text(), arbitrary_color()).prop_map(|(text, color)| Node::Label { text, color }),
        arbitrary_text().prop_map(move |text| Node::Button { id: next_id, text }),
        (0u16..=1000, 0u16..=MAXIMUM_BAR_CELLS, arbitrary_color()).prop_map(
            |(permille, cells, color)| Node::Bar {
                permille,
                cells,
                color
            }
        ),
        (
            arbitrary_color(),
            prop::option::of(any::<u32>()),
            any::<bool>()
        )
            .prop_map(|(color, count, selected)| Node::Swatch {
                color,
                count,
                selected
            }),
        (0u16..=MAXIMUM_SPACE).prop_map(Node::Space),
    ]
}

/// An interface that is within the limits: leaves with distinct button identifiers, one level of nesting.
fn arbitrary_ui() -> impl Strategy<Value = Ui> {
    let leaves =
        (0u32..12).prop_flat_map(|count| (0..count).map(arbitrary_leaf).collect::<Vec<_>>());
    (
        any::<bool>(),
        prop::sample::select(Anchor::ALL.to_vec()),
        prop::option::of(arbitrary_text()),
        arbitrary_color(),
        any::<bool>(),
        leaves,
    )
        .prop_map(|(modal, anchor, title, color, row, mut children)| {
            // Put the last two leaves in a panel of their own, to have some nesting.
            if children.len() > 2 {
                let tail = children.split_off(children.len() - 2);
                children.push(Node::Panel(panel(tail)));
            }
            Ui {
                modal,
                anchor,
                root: Panel {
                    title,
                    color,
                    direction: if row {
                        Direction::Row
                    } else {
                        Direction::Column
                    },
                    children,
                },
            }
        })
}

proptest! {
    #[test]
    fn any_interface_survives_its_byte_form_and_has_one_spelling(interface in arbitrary_ui()) {
        prop_assert_eq!(interface.check(), Ok(()));
        let bytes = encoded(&interface);
        let back = decoded(&bytes).unwrap();
        prop_assert_eq!(&back, &interface);
        prop_assert_eq!(encoded(&back), bytes);
    }

    #[test]
    fn bytes_that_are_not_an_interface_are_an_error_never_a_panic_and_those_that_are_have_one_spelling(
        bytes in prop::collection::vec(any::<u8>(), 0..80)
    ) {
        if let Ok(interface) = decoded(&bytes) {
            prop_assert_eq!(encoded(&interface), bytes);
        }
    }

    #[test]
    fn every_button_of_an_interface_can_be_hit_where_it_is_drawn_inside_its_panel(
        interface in arbitrary_ui(), scale in 1i32..4
    ) {
        let laid = layout(std::slice::from_ref(&interface), (1280, 720), scale, None);
        let panel_area = match laid.draws.iter().find_map(|draw| match draw {
            Draw::Rect { area, color } if *color == ui::BORDER_COLOR => Some(*area),
            _ => None,
        }) {
            Some(area) => area,
            None => return Err(TestCaseError::fail("no panel was drawn")),
        };
        let ids = interface.button_ids();
        prop_assert_eq!(laid.buttons.len(), ids.len());
        for button in &laid.buttons {
            prop_assert!(ids.contains(&button.id));
            let corner_inside = |x: i32, y: i32| panel_area.contains(x, y);
            prop_assert!(corner_inside(button.area.x, button.area.y));
            prop_assert!(corner_inside(
                button.area.x + button.area.width - 1,
                button.area.y + button.area.height - 1
            ));
            prop_assert_eq!(
                laid.button_at(button.area.x, button.area.y).map(|found| found.id),
                Some(button.id)
            );
        }
    }
}

// ---- layout ----

fn rect(x: i32, y: i32, width: i32, height: i32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

#[test]
fn a_label_in_a_panel_in_the_middle_of_the_screen_is_where_the_arithmetic_says() {
    // The label is 2 letters (16 units) by 8; the panel adds a border of 1 and padding of 4 on each side: 26 by 18.
    let interface = ui(Anchor::Center, false, vec![label("Hi")]);
    let laid = layout(&[interface], (200, 100), 1, None);
    assert_eq!(
        laid.draws,
        [
            Draw::Rect {
                area: rect(87, 41, 26, 18),
                color: ui::BORDER_COLOR
            },
            Draw::Rect {
                area: rect(88, 42, 24, 16),
                color: [20, 20, 40, 200]
            },
            Draw::Text {
                x: 92,
                y: 46,
                text: "Hi".to_owned(),
                color: Color::WHITE,
                scale: 1
            },
        ]
    );
    assert!(laid.buttons.is_empty());
}

#[test]
fn at_a_larger_scale_everything_is_the_same_shape_times_the_scale() {
    let interface = everything();
    let one = layout(std::slice::from_ref(&interface), (400, 300), 1, None);
    let three = layout(std::slice::from_ref(&interface), (1200, 900), 3, None);
    assert_eq!(one.draws.len(), three.draws.len());
    // Sizes are three times as big; the panel's place differs only by the screen it is anchored in (its margin is scaled too).
    let sizes = |laid: &ui::Layout| -> Vec<(i32, i32)> {
        laid.draws
            .iter()
            .filter_map(|draw| match draw {
                Draw::Rect { area, .. } => Some((area.width, area.height)),
                Draw::Text { .. } => None,
            })
            .collect()
    };
    let scaled: Vec<(i32, i32)> = sizes(&one)
        .into_iter()
        .map(|(width, height)| {
            if (width, height) == (400, 300) {
                (1200, 900) // the dimming of a modal covers the screen
            } else {
                (width * 3, height * 3)
            }
        })
        .collect();
    assert_eq!(sizes(&three), scaled);
    for draw in &three.draws {
        if let Draw::Text { scale, .. } = draw {
            assert_eq!(*scale, 3);
        }
    }
}

#[test]
fn each_anchor_puts_the_panel_at_its_edge_or_corner_with_a_margin() {
    let screen = (300, 200);
    // A panel of one 5-letter label: 40 + 10 = 50 wide, 8 + 10 = 18 tall.
    let (width, height) = (50, 18);
    let expected = |anchor: Anchor| -> (i32, i32) {
        let x = match anchor {
            Anchor::TopLeft | Anchor::Left | Anchor::BottomLeft => 8,
            Anchor::Top | Anchor::Center | Anchor::Bottom => (screen.0 - width) / 2,
            _ => screen.0 - 8 - width,
        };
        let y = match anchor {
            Anchor::TopLeft | Anchor::Top | Anchor::TopRight => 8,
            Anchor::Left | Anchor::Center | Anchor::Right => (screen.1 - height) / 2,
            _ => screen.1 - 8 - height,
        };
        (x, y)
    };
    for anchor in Anchor::ALL {
        let laid = layout(&[ui(anchor, false, vec![label("abcde")])], screen, 1, None);
        let Draw::Rect { area, .. } = laid.draws[0] else {
            panic!("the panel's border comes first");
        };
        assert_eq!(
            (area.x, area.y, area.width, area.height),
            (expected(anchor).0, expected(anchor).1, width, height),
            "{anchor:?}"
        );
    }
}

#[test]
fn panels_at_one_anchor_stack_away_from_the_edge() {
    let one = |text: &str| ui(Anchor::TopLeft, false, vec![label(text)]);
    let laid = layout(&[one("a"), one("b")], (300, 200), 1, None);
    let borders: Vec<Rect> = laid
        .draws
        .iter()
        .filter_map(|draw| match draw {
            Draw::Rect { area, color } if *color == ui::BORDER_COLOR => Some(*area),
            _ => None,
        })
        .collect();
    // Each is 18 tall; the second is 4 below the first.
    assert_eq!(borders, [rect(8, 8, 18, 18), rect(8, 30, 18, 18)]);
    let bottom = |text: &str| ui(Anchor::BottomLeft, false, vec![label(text)]);
    let laid = layout(&[bottom("a"), bottom("b")], (300, 200), 1, None);
    let ys: Vec<i32> = laid
        .draws
        .iter()
        .filter_map(|draw| match draw {
            Draw::Rect { area, color } if *color == ui::BORDER_COLOR => Some(area.y),
            _ => None,
        })
        .collect();
    assert_eq!(
        ys,
        [200 - 8 - 18, 200 - 8 - 18 - 22],
        "the second is above the first"
    );
}

#[test]
fn a_modal_dims_the_whole_screen_first_and_a_button_has_a_place_a_colour_and_a_focus() {
    let interface = ui(
        Anchor::Center,
        true,
        vec![button(5, "Yes"), button(6, "No")],
    );
    let plain = layout(std::slice::from_ref(&interface), (200, 100), 1, None);
    assert_eq!(
        plain.draws[0],
        Draw::Rect {
            area: rect(0, 0, 200, 100),
            color: ui::BACKDROP_COLOR
        }
    );
    // "Yes" is 3 letters: 24 + 8 = 32 wide, 8 + 8 = 16 tall, and "No" 16 + 8 = 24 wide; the panel is 32 + 10 by 16 + 4 + 16 + 10.
    assert_eq!(plain.buttons.len(), 2);
    assert_eq!(plain.buttons[0].id, 5);
    assert_eq!(plain.buttons[0].area, rect(79 + 5, 27 + 5, 32, 16));
    assert_eq!(plain.buttons[1].area, rect(79 + 5, 27 + 5 + 20, 16 + 8, 16));
    let fill = |laid: &ui::Layout, id: u32| -> [u8; 4] {
        let area = laid
            .buttons
            .iter()
            .find(|button| button.id == id)
            .unwrap()
            .area;
        laid.draws
            .iter()
            .find_map(|draw| match draw {
                Draw::Rect { area: drawn, color } if *drawn == area => Some(*color),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(fill(&plain, 5), ui::BUTTON_COLOR);
    let focused = layout(std::slice::from_ref(&interface), (200, 100), 1, Some(6));
    assert_eq!(fill(&focused, 5), ui::BUTTON_COLOR);
    assert_eq!(fill(&focused, 6), ui::FOCUSED_BUTTON_COLOR);
    // Pixels: inside a button hits it, just outside does not.
    let first = plain.buttons[0].area;
    assert_eq!(plain.button_at(first.x, first.y).map(|b| b.id), Some(5));
    assert_eq!(plain.button_at(first.x + first.width, first.y), None);
    assert_eq!(plain.button_at(first.x - 1, first.y), None);
    assert_eq!(
        plain
            .button_at(first.x, first.y + first.height - 1)
            .map(|b| b.id),
        Some(5)
    );
}

#[test]
fn bars_fill_in_proportion_and_swatches_show_their_count_and_whether_they_are_chosen() {
    let interface = ui(
        Anchor::TopLeft,
        false,
        vec![
            Node::Bar {
                permille: 250,
                cells: 4,
                color: RED,
            },
            Node::Swatch {
                color: Color([9, 8, 7]),
                count: Some(12),
                selected: true,
            },
        ],
    );
    let laid = layout(&[interface], (300, 200), 1, None);
    let rects: Vec<(Rect, [u8; 4])> = laid
        .draws
        .iter()
        .filter_map(|draw| match draw {
            Draw::Rect { area, color } => Some((*area, *color)),
            Draw::Text { .. } => None,
        })
        .collect();
    // Panel border and fill, then the bar (32 wide, a quarter filled), then the swatch (16 square, white border, 2 thick).
    assert_eq!(rects[2], (rect(13, 13, 32, 8), ui::BAR_BACKGROUND));
    assert_eq!(rects[3], (rect(13, 13, 8, 8), [200, 30, 30, 255]));
    assert_eq!(rects[4], (rect(13, 25, 16, 16), [255, 255, 255, 255]));
    assert_eq!(rects[5], (rect(15, 27, 12, 12), [9, 8, 7, 255]));
    let texts: Vec<(i32, i32, &str)> = laid
        .draws
        .iter()
        .filter_map(|draw| match draw {
            Draw::Text { x, y, text, .. } => Some((*x, *y, text.as_str())),
            Draw::Rect { .. } => None,
        })
        .collect();
    assert_eq!(texts, [(33, 29, "12")]);
    // An empty bar draws only its background.
    let empty = ui(
        Anchor::TopLeft,
        false,
        vec![Node::Bar {
            permille: 0,
            cells: 4,
            color: RED,
        }],
    );
    let rects = layout(&[empty], (300, 200), 1, None).draws.len();
    assert_eq!(rects, 3);
}

#[test]
fn a_row_flows_left_to_right_and_a_title_sits_above_the_children() {
    let mut interface = ui(
        Anchor::TopLeft,
        false,
        vec![label("ab"), Node::Space(10), label("c")],
    );
    interface.root.direction = Direction::Row;
    interface.root.title = Some("Title".to_owned());
    let laid = layout(&[interface], (300, 200), 1, None);
    let texts: Vec<(i32, i32, &str)> = laid
        .draws
        .iter()
        .filter_map(|draw| match draw {
            Draw::Text { x, y, text, .. } => Some((*x, *y, text.as_str())),
            Draw::Rect { .. } => None,
        })
        .collect();
    // Content at 13, 13: the title, then the row 12 lower (8 high and a gap of 4): "ab" 16 wide, a gap, 10 of space, a gap, "c".
    assert_eq!(
        texts,
        [
            (13, 13, "Title"),
            (13, 25, "ab"),
            (13 + 16 + 4 + 10 + 4, 25, "c")
        ]
    );
}

#[test]
fn the_scale_is_a_whole_number_that_grows_with_the_screen_height_and_stops() {
    assert_eq!(scale_for_height(0), 1);
    assert_eq!(scale_for_height(359), 1);
    assert_eq!(scale_for_height(360), 1);
    assert_eq!(scale_for_height(720), 2);
    assert_eq!(scale_for_height(1080), 3);
    assert_eq!(scale_for_height(1440), 4);
    assert_eq!(scale_for_height(4320), 4);
}

#[test]
fn a_description_nested_a_thousand_times_is_refused_without_building_it() {
    // A panel node whose only child is a panel node, and so on: without a guard while reading this is as deep as the
    // message is long (and overflows the stack).
    let mut inner = vec![5u8, 0];
    for _ in 0..5_000 {
        let mut wrapped = vec![0u8, 0, 1, 2, 3, 1, 1];
        wrapped.extend(&inner);
        inner = wrapped;
    }
    assert_eq!(
        refusal(&handmade(0, 4, &inner, 1)),
        "panels are nested too deeply"
    );
}

#[test]
fn a_bar_is_filled_by_exact_thousandths_rounded_down() {
    // 3 letters is 24 wide: 333 thousandths is 7.99 and so 7.
    let interface = ui(
        Anchor::TopLeft,
        false,
        vec![Node::Bar {
            permille: 333,
            cells: 3,
            color: RED,
        }],
    );
    let laid = layout(&[interface], (300, 200), 1, None);
    let Draw::Rect { area, .. } = laid.draws[3] else {
        panic!("the filled part comes after the panel and the bar's background");
    };
    assert_eq!((area.width, area.height), (7, 8));
}

#[test]
fn where_buttons_overlap_the_one_drawn_last_is_the_one_hit() {
    // On a tiny screen a panel at the top left and one in the middle cover each other; the second is drawn on top.
    let first = ui(Anchor::TopLeft, false, vec![button(1, "abcdef")]);
    let second = ui(Anchor::Center, false, vec![button(2, "abcdef")]);
    let laid = layout(&[first, second], (60, 40), 1, None);
    let (a, b) = (laid.buttons[0], laid.buttons[1]);
    assert_eq!((a.id, b.id), (1, 2));
    assert_eq!(a.area, rect(13, 13, 56, 16));
    assert_eq!(b.area, rect(2, 12, 56, 16));
    assert_eq!(
        laid.button_at(20, 20).map(|button| button.id),
        Some(2),
        "in both"
    );
    assert_eq!(
        laid.button_at(60, 20).map(|button| button.id),
        Some(1),
        "only in the first"
    );
    assert_eq!(
        laid.button_at(5, 12).map(|button| button.id),
        Some(2),
        "only in the second"
    );
}

// ---- text that is wider than the screen wraps ----

/// The text drawn, in order: where it starts, and what it says.
fn texts(laid: &ui::Layout) -> Vec<(i32, i32, String)> {
    laid.draws
        .iter()
        .filter_map(|draw| match draw {
            Draw::Text { x, y, text, .. } => Some((*x, *y, text.clone())),
            _ => None,
        })
        .collect()
}

const A_LONG_SENTENCE: &str = "the quick brown fox jumps over the lazy dog and then it keeps on going for a long long time without stopping";

#[test]
fn a_label_wider_than_the_screen_is_wrapped_into_lines_that_fit_one_under_the_other() {
    // 400 pixels at scale 1: 8 of margin and 5 of padding and border either side leave 374 units, which is 46 letters.
    let interface = ui(Anchor::TopLeft, false, vec![label(A_LONG_SENTENCE)]);
    let drawn = texts(&layout(&[interface], (400, 720), 1, None));
    assert!(drawn.len() >= 3, "{drawn:?}");
    for (x, _, line) in &drawn {
        assert!(line.chars().count() <= 46, "{line:?}");
        assert!(
            x + line.chars().count() as i32 * 8 <= 400 - 8 - 5,
            "{line:?} at {x} runs off the screen"
        );
    }
    // Each line is a letter-height and a gap under the one before, and together they say the sentence.
    for pair in drawn.windows(2) {
        assert_eq!(pair[1].1 - pair[0].1, 8 + 4);
        assert_eq!(pair[1].0, pair[0].0);
    }
    let said: Vec<&str> = drawn.iter().map(|(_, _, line)| line.as_str()).collect();
    assert_eq!(said.join(" "), A_LONG_SENTENCE);
}

#[test]
fn a_larger_scale_fits_fewer_letters_in_a_line() {
    let interface = ui(Anchor::TopLeft, false, vec![label(A_LONG_SENTENCE)]);
    let at_scale = |scale| {
        texts(&layout(
            std::slice::from_ref(&interface),
            (400, 720),
            scale,
            None,
        ))
    };
    let (one, two) = (at_scale(1), at_scale(2));
    let widest = |drawn: &[(i32, i32, String)]| {
        drawn
            .iter()
            .map(|(_, _, line)| line.chars().count())
            .max()
            .unwrap()
    };
    // At scale 2 the content is 400 / 2 - 16 - 10 = 174 units: 21 letters.
    assert!(widest(&two) <= 21, "{two:?}");
    assert!(widest(&one) > widest(&two));
    assert!(two.len() > one.len());
    for (x, _, line) in &two {
        assert!(
            x + line.chars().count() as i32 * 16 <= 400 - 16 - 10,
            "{line:?}"
        );
    }
}

#[test]
fn a_label_that_fits_is_one_line_and_is_drawn_where_it_always_was() {
    let interface = ui(Anchor::TopLeft, false, vec![label("hello"), label("world")]);
    let drawn = texts(&layout(&[interface], (400, 720), 1, None));
    assert_eq!(
        drawn,
        [
            (8 + 5, 8 + 5, "hello".to_owned()),
            (8 + 5, 8 + 5 + 8 + 4, "world".to_owned())
        ]
    );
}

#[test]
fn a_label_in_a_row_is_not_wrapped_and_a_panel_in_a_column_wraps_to_what_is_left_of_the_screen() {
    let row = Ui {
        modal: false,
        anchor: Anchor::TopLeft,
        root: Panel {
            title: None,
            color: Color([20, 20, 40]),
            direction: Direction::Row,
            children: vec![label(A_LONG_SENTENCE)],
        },
    };
    let drawn = texts(&layout(&[row], (400, 720), 1, None));
    assert_eq!(drawn.len(), 1, "a row keeps its text on a line");
    // A panel inside a panel has 10 units less on each level: 364 units, 45 letters.
    let inner = Node::Panel(panel(vec![label(A_LONG_SENTENCE)]));
    let nested = ui(Anchor::TopLeft, false, vec![inner]);
    let drawn = texts(&layout(&[nested], (400, 720), 1, None));
    assert!(drawn.len() >= 3);
    assert!(
        drawn.iter().all(|(_, _, line)| line.chars().count() <= 45),
        "{drawn:?}"
    );
}

#[test]
fn words_kept_together_by_a_no_break_space_are_wrapped_together_and_drawn_with_a_space() {
    let pair = |axis: &str, value: &str| format!("{axis}{}{value}", ui::NO_BREAK_SPACE);
    let text = [
        pair("x", "0x1234567890abcdef"),
        pair("y", "0x1234567890abcdef"),
        pair("z", "0x1234567890abcdef"),
    ]
    .join(" ");
    let interface = ui(Anchor::TopLeft, false, vec![label(&text)]);
    // 400 pixels is 46 letters: two pairs (20 + 1 + 20) fit, the third goes under.
    let drawn = texts(&layout(&[interface], (400, 720), 1, None));
    let said: Vec<&str> = drawn.iter().map(|(_, _, line)| line.as_str()).collect();
    assert_eq!(
        said,
        [
            "x 0x1234567890abcdef y 0x1234567890abcdef",
            "z 0x1234567890abcdef"
        ]
    );
}

#[test]
fn a_text_taller_than_the_screen_is_cut_with_dots_and_the_panel_fits() {
    let endless = "word ".repeat(2000);
    let interface = ui(Anchor::TopLeft, false, vec![label(&endless)]);
    let laid = layout(&[interface], (400, 360), 1, None);
    let drawn = texts(&laid);
    let last = drawn.last().unwrap();
    assert!(last.2.ends_with("..."), "{last:?}");
    assert!(last.2.chars().count() <= 46);
    // The bottom of the last line is on the screen.
    assert!(last.1 + 8 <= 360, "{last:?}");
    for pair in drawn.windows(2) {
        assert_eq!(pair[1].1 - pair[0].1, 12);
    }
}

#[test]
fn buttons_and_titles_keep_their_places_when_a_label_wraps() {
    let interface = Ui {
        modal: true,
        anchor: Anchor::Center,
        root: Panel {
            title: Some("Question".to_owned()),
            color: Color([20, 20, 40]),
            direction: Direction::Column,
            children: vec![
                label(A_LONG_SENTENCE),
                button(3, "1) yes"),
                button(4, "2) no"),
            ],
        },
    };
    let laid = layout(&[interface], (400, 720), 1, None);
    let ids: Vec<u32> = laid.buttons.iter().map(|button| button.id).collect();
    assert_eq!(ids, [3, 4]);
    // The buttons are under every line of the label.
    let last_line = texts(&laid)
        .into_iter()
        .filter(|(_, _, line)| {
            !line.starts_with("1)") && !line.starts_with("2)") && line != "Question"
        })
        .map(|(_, y, _)| y)
        .max()
        .unwrap();
    assert!(laid.buttons.iter().all(|button| button.area.y > last_line));
}
