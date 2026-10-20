//! Painting a layout: letters as rectangles.

use ui::{Anchor, Color, Direction, Draw, Layout, Node, Panel, Rect, Ui, layout, paint};

/// The pixels a text covers, as lines of `#` and `.`, `width` by `height` pixels from its top left corner.
fn picture(text: &str, scale: i32, width: usize, height: usize) -> Vec<String> {
    let laid = Layout {
        draws: vec![Draw::Text {
            x: 0,
            y: 0,
            text: text.to_owned(),
            color: Color([10, 20, 30]),
            scale,
        }],
        buttons: Vec::new(),
    };
    let mut grid = vec![vec!['.'; width]; height];
    for painted in paint(&laid) {
        assert_eq!(painted.color, [10, 20, 30, 255]);
        let Rect {
            x,
            y,
            width: w,
            height: h,
        } = painted.area;
        for row in y..y + h {
            for column in x..x + w {
                assert_eq!(grid[row as usize][column as usize], '.', "no pixel twice");
                grid[row as usize][column as usize] = '#';
            }
        }
    }
    grid.into_iter()
        .map(|row| row.into_iter().collect())
        .collect()
}

fn lines(rows: &[&str]) -> Vec<String> {
    rows.iter().map(|row| (*row).to_owned()).collect()
}

#[test]
fn letters_are_drawn_the_way_the_font_has_them_with_the_lowest_bit_at_the_left() {
    assert_eq!(
        picture("A", 1, 8, 8),
        lines(&[
            "..##....", ".####...", "##..##..", "##..##..", "######..", "##..##..", "##..##..",
            "........"
        ])
    );
    assert_eq!(
        picture("L", 1, 8, 8),
        lines(&[
            "####....", ".##.....", ".##.....", ".##.....", ".##...#.", ".##..##.", "#######.",
            "........"
        ])
    );
    assert_eq!(
        picture("!", 1, 8, 8),
        lines(&[
            "...##...", "..####..", "..####..", "...##...", "...##...", "........", "...##...",
            "........"
        ])
    );
}

#[test]
fn letters_follow_each_other_eight_pixels_apart_and_a_space_is_empty() {
    let two = picture("L L", 1, 24, 8);
    let one = picture("L", 1, 8, 8);
    for (row, expected) in one.iter().enumerate() {
        assert_eq!(&two[row][0..8], expected);
        assert_eq!(&two[row][8..16], "........");
        assert_eq!(&two[row][16..24], expected);
    }
}

#[test]
fn at_a_larger_scale_each_pixel_is_a_square_of_the_scale() {
    let small = picture("A", 1, 8, 8);
    let large = picture("A", 3, 24, 24);
    for (row, line) in small.iter().enumerate() {
        for (column, pixel) in line.chars().enumerate() {
            for dy in 0..3 {
                for dx in 0..3 {
                    assert_eq!(
                        large[row * 3 + dy].chars().nth(column * 3 + dx),
                        Some(pixel)
                    );
                }
            }
        }
    }
}

#[test]
fn what_the_font_does_not_have_is_a_hollow_box_and_control_characters_are_not_text() {
    let boxed = lines(&[
        ".######.", ".#....#.", ".#....#.", ".#....#.", ".#....#.", ".#....#.", ".######.",
        "........",
    ]);
    for text in ["\u{e9}", "\u{65e5}", "\u{7f}", "\n"] {
        assert_eq!(picture(text, 1, 8, 8), boxed, "{text:?}");
    }
}

#[test]
fn a_run_of_lit_pixels_in_a_row_is_one_rectangle_so_text_is_not_a_rectangle_for_every_pixel() {
    // The top row of the L is four pixels in a row: one rectangle, 4 wide.
    let laid = Layout {
        draws: vec![Draw::Text {
            x: 0,
            y: 0,
            text: "L".to_owned(),
            color: Color::WHITE,
            scale: 1,
        }],
        buttons: Vec::new(),
    };
    let painted = paint(&laid);
    assert_eq!(
        painted[0].area,
        Rect {
            x: 0,
            y: 0,
            width: 4,
            height: 1
        }
    );
    // Seven rows are lit, and the fifth and sixth have two runs each (the foot of the L): 9 rectangles for 24 pixels.
    assert_eq!(painted.len(), 9);
    let pixels: i32 = painted.iter().map(|p| p.area.width * p.area.height).sum();
    assert_eq!(pixels, 24);
}

#[test]
fn a_whole_interface_paints_its_rectangles_and_its_text_in_the_order_it_is_drawn() {
    let interface = Ui {
        modal: false,
        anchor: Anchor::Center,
        root: Panel {
            title: None,
            color: Color([20, 20, 40]),
            direction: Direction::Column,
            children: vec![Node::Label {
                text: "Hi".to_owned(),
                color: Color::WHITE,
            }],
        },
    };
    let laid = layout(&[interface], (200, 100), 1, None);
    let painted = paint(&laid);
    // The border and the fill first, exactly as the layout has them, then the text's rectangles, all white and all inside
    // the 16 by 8 the text covers at (92, 46).
    assert_eq!(
        painted[0].area,
        Rect {
            x: 87,
            y: 41,
            width: 26,
            height: 18
        }
    );
    assert_eq!(painted[1].color, [20, 20, 40, 200]);
    let text = &painted[2..];
    assert!(!text.is_empty());
    for part in text {
        assert_eq!(part.color, [255, 255, 255, 255]);
        assert!(part.area.x >= 92 && part.area.x + part.area.width <= 108);
        assert!(part.area.y >= 46 && part.area.y + part.area.height <= 54);
    }
}
