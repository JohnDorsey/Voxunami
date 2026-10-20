//! From a layout to flat rectangles: the last step before a renderer, with text made of rectangles too, one for each
//! run of lit pixels in a row of a letter, so that every edge is on a pixel boundary.

use font8x8::legacy::BASIC_LEGACY;

use crate::layout::{Draw, Layout, Rect};
use crate::model::Color;

/// A flat rectangle to paint, with alpha (sRGB, straight alpha).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Painted {
    pub area: Rect,
    pub color: [u8; 4],
}

/// What a letter the font does not have (anything but ASCII) looks like: a hollow box.
const UNKNOWN_GLYPH: [u8; 8] = [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00];

/// The rows of a letter, top to bottom; in each the lowest bit is the pixel at the left.
fn glyph(letter: char) -> [u8; 8] {
    match BASIC_LEGACY.get(letter as usize) {
        // The control characters have no picture in the font; they are not text.
        Some(rows) if letter >= ' ' && letter != '\u{7f}' => *rows,
        _ => UNKNOWN_GLYPH,
    }
}

fn paint_text(painted: &mut Vec<Painted>, x: i32, y: i32, text: &str, color: Color, scale: i32) {
    let [red, green, blue] = color.0;
    for (index, letter) in text.chars().enumerate() {
        let left = x + index as i32 * 8 * scale;
        for (row, bits) in glyph(letter).into_iter().enumerate() {
            let top = y + row as i32 * scale;
            let mut column = 0;
            while column < 8 {
                if bits >> column & 1 == 0 {
                    column += 1;
                    continue;
                }
                let start = column;
                while column < 8 && bits >> column & 1 == 1 {
                    column += 1;
                }
                painted.push(Painted {
                    area: Rect {
                        x: left + start * scale,
                        y: top,
                        width: (column - start) * scale,
                        height: scale,
                    },
                    color: [red, green, blue, 255],
                });
            }
        }
    }
}

/// Everything a layout draws as flat rectangles, in order.
pub fn paint(layout: &Layout) -> Vec<Painted> {
    let mut painted = Vec::new();
    for draw in &layout.draws {
        match draw {
            Draw::Rect { area, color } => painted.push(Painted {
                area: *area,
                color: *color,
            }),
            Draw::Text {
                x,
                y,
                text,
                color,
                scale,
            } => paint_text(&mut painted, *x, *y, text, *color, *scale),
        }
    }
    painted
}
