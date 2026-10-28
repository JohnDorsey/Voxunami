//! Where everything goes: from interfaces and a screen size to rectangles and text at whole pixels, the same on every machine.
//!
//! All sizes are worked out in *units* (one unit is a pixel at scale 1: a letter is 8 by 8 units) and multiplied by the
//! scale, a whole number, when they are placed.

use crate::model::{Anchor, Color, Direction, Node, Panel, Ui};
use crate::wrap::wrap_text;

/// How wide and tall a letter is, in units.
pub const GLYPH: i32 = 8;
const PADDING: i32 = 4;
const GAP: i32 = 4;
const BORDER: i32 = 1;
const MARGIN: i32 = 8;
const BUTTON_PADDING: i32 = 4;
const BAR_HEIGHT: i32 = 8;
const SWATCH: i32 = 16;

/// The colours the layout paints with besides the ones an interface names.
pub const BORDER_COLOR: [u8; 4] = [200, 200, 200, 255];
pub const BACKDROP_COLOR: [u8; 4] = [0, 0, 0, 120];
pub const BUTTON_COLOR: [u8; 4] = [60, 60, 70, 255];
pub const FOCUSED_BUTTON_COLOR: [u8; 4] = [110, 110, 160, 255];
pub const BAR_BACKGROUND: [u8; 4] = [30, 30, 30, 255];
const PANEL_ALPHA: u8 = 200;

/// A rectangle of pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

/// One thing to paint, in order, later over earlier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Draw {
    /// A flat rectangle, with alpha.
    Rect { area: Rect, color: [u8; 4] },
    /// Text with its top left at a pixel, each letter `8 * scale` pixels square.
    Text {
        x: i32,
        y: i32,
        text: String,
        color: Color,
        scale: i32,
    },
}

/// A button's place on the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ButtonArea {
    pub id: u32,
    pub area: Rect,
    /// The index (in the list given to [`layout`]) of the interface it belongs to.
    pub ui: usize,
}

/// What to paint, and where the buttons are.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Layout {
    pub draws: Vec<Draw>,
    pub buttons: Vec<ButtonArea>,
}

impl Layout {
    /// The button under a pixel, if any (the last one drawn, if they overlap).
    pub fn button_at(&self, x: i32, y: i32) -> Option<&ButtonArea> {
        self.buttons
            .iter()
            .rev()
            .find(|button| button.area.contains(x, y))
    }
}

/// The scale to draw at for a screen this tall: a whole number, so that letters are exact.
pub fn scale_for_height(height: u32) -> i32 {
    (height / 360).clamp(1, 4) as i32
}

fn text_width(text: &str) -> i32 {
    text.chars().count() as i32 * GLYPH
}

/// A size in units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Size {
    width: i32,
    height: i32,
}

fn measure(node: &Node, along: Direction) -> Size {
    match node {
        Node::Panel(panel) => measure_panel(panel),
        Node::Label { text, .. } => Size {
            width: text_width(text),
            height: GLYPH,
        },
        Node::Button { text, .. } => Size {
            width: text_width(text) + 2 * BUTTON_PADDING,
            height: GLYPH + 2 * BUTTON_PADDING,
        },
        Node::Bar { cells, .. } => Size {
            width: i32::from(*cells) * GLYPH,
            height: BAR_HEIGHT,
        },
        Node::Swatch { count, .. } => Size {
            width: SWATCH + count.map_or(0, |count| GAP + text_width(&count.to_string())),
            height: SWATCH,
        },
        Node::Space(size) => match along {
            Direction::Row => Size {
                width: i32::from(*size),
                height: 0,
            },
            Direction::Column => Size {
                width: 0,
                height: i32::from(*size),
            },
        },
    }
}

fn measure_panel(panel: &Panel) -> Size {
    let mut content = Size {
        width: 0,
        height: 0,
    };
    for (index, child) in panel.children.iter().enumerate() {
        let size = measure(child, panel.direction);
        match panel.direction {
            Direction::Row => {
                content.width += size.width + if index > 0 { GAP } else { 0 };
                content.height = content.height.max(size.height);
            }
            Direction::Column => {
                content.height += size.height + if index > 0 { GAP } else { 0 };
                content.width = content.width.max(size.width);
            }
        }
    }
    let mut width = content.width;
    let mut height = content.height;
    if let Some(title) = &panel.title {
        width = width.max(text_width(title));
        height += GLYPH + if panel.children.is_empty() { 0 } else { GAP };
    }
    Size {
        width: width + 2 * (PADDING + BORDER),
        height: height + 2 * (PADDING + BORDER),
    }
}

struct Painter<'a> {
    draws: &'a mut Vec<Draw>,
    buttons: &'a mut Vec<ButtonArea>,
    scale: i32,
    focus: Option<u32>,
    ui: usize,
}

impl Painter<'_> {
    fn rect(&mut self, x: i32, y: i32, size: Size, color: [u8; 4]) {
        self.draws.push(Draw::Rect {
            area: Rect {
                x,
                y,
                width: size.width * self.scale,
                height: size.height * self.scale,
            },
            color,
        });
    }

    fn text(&mut self, x: i32, y: i32, text: &str, color: Color) {
        self.draws.push(Draw::Text {
            x,
            y,
            text: text.to_owned(),
            color,
            scale: self.scale,
        });
    }

    /// Paints a panel with its top left at a pixel.
    fn panel(&mut self, x: i32, y: i32, panel: &Panel) {
        let size = measure_panel(panel);
        let scale = self.scale;
        self.rect(x, y, size, BORDER_COLOR);
        let inner = Size {
            width: size.width - 2 * BORDER,
            height: size.height - 2 * BORDER,
        };
        let [red, green, blue] = panel.color.0;
        self.rect(
            x + BORDER * scale,
            y + BORDER * scale,
            inner,
            [red, green, blue, PANEL_ALPHA],
        );
        let mut cursor_x = x + (BORDER + PADDING) * scale;
        let mut cursor_y = y + (BORDER + PADDING) * scale;
        if let Some(title) = &panel.title {
            self.text(cursor_x, cursor_y, title, Color::WHITE);
            cursor_y += (GLYPH + if panel.children.is_empty() { 0 } else { GAP }) * scale;
        }
        for child in &panel.children {
            let child_size = measure(child, panel.direction);
            self.node(cursor_x, cursor_y, child);
            match panel.direction {
                Direction::Row => cursor_x += (child_size.width + GAP) * scale,
                Direction::Column => cursor_y += (child_size.height + GAP) * scale,
            }
        }
    }

    fn node(&mut self, x: i32, y: i32, node: &Node) {
        let scale = self.scale;
        match node {
            Node::Panel(panel) => self.panel(x, y, panel),
            Node::Label { text, color } => self.text(x, y, text, *color),
            Node::Button { id, text } => {
                let size = measure(node, Direction::Row);
                let color = if self.focus == Some(*id) {
                    FOCUSED_BUTTON_COLOR
                } else {
                    BUTTON_COLOR
                };
                self.rect(x, y, size, color);
                self.text(
                    x + BUTTON_PADDING * scale,
                    y + BUTTON_PADDING * scale,
                    text,
                    Color::WHITE,
                );
                self.buttons.push(ButtonArea {
                    id: *id,
                    area: Rect {
                        x,
                        y,
                        width: size.width * scale,
                        height: size.height * scale,
                    },
                    ui: self.ui,
                });
            }
            Node::Bar {
                permille,
                cells,
                color,
            } => {
                let width = i32::from(*cells) * GLYPH;
                self.rect(
                    x,
                    y,
                    Size {
                        width,
                        height: BAR_HEIGHT,
                    },
                    BAR_BACKGROUND,
                );
                let filled = width * i32::from(*permille) / 1000;
                if filled > 0 {
                    let [red, green, blue] = color.0;
                    self.rect(
                        x,
                        y,
                        Size {
                            width: filled,
                            height: BAR_HEIGHT,
                        },
                        [red, green, blue, 255],
                    );
                }
            }
            Node::Swatch {
                color,
                count,
                selected,
            } => {
                let border = if *selected {
                    [255, 255, 255, 255]
                } else {
                    [60, 60, 60, 255]
                };
                self.rect(
                    x,
                    y,
                    Size {
                        width: SWATCH,
                        height: SWATCH,
                    },
                    border,
                );
                let [red, green, blue] = color.0;
                let edge = if *selected { 2 } else { 1 };
                self.rect(
                    x + edge * scale,
                    y + edge * scale,
                    Size {
                        width: SWATCH - 2 * edge,
                        height: SWATCH - 2 * edge,
                    },
                    [red, green, blue, 255],
                );
                if let Some(count) = count {
                    self.text(
                        x + (SWATCH + GAP) * scale,
                        y + (SWATCH - GLYPH) / 2 * scale,
                        &count.to_string(),
                        Color::WHITE,
                    );
                }
            }
            Node::Space(_) => {}
        }
    }
}

/// The panel with the text of its labels broken into lines that fit `units` of width (the width its content may have), and the panels in it too, to what is left of that.
/// The labels of a row are left as they are: nothing says how a row is to share its width.
fn wrapped(panel: &Panel, units: i32) -> Panel {
    if panel.direction == Direction::Row {
        return panel.clone();
    }
    let columns = letters_in(units);
    let mut children = Vec::with_capacity(panel.children.len());
    for child in &panel.children {
        match child {
            Node::Label { text, color } => {
                children.extend(
                    wrap_text(text, columns)
                        .into_iter()
                        .map(|line| Node::Label {
                            text: line,
                            color: *color,
                        }),
                );
            }
            Node::Panel(inner) => {
                children.push(Node::Panel(wrapped(inner, units - 2 * (PADDING + BORDER))));
            }
            other => children.push(other.clone()),
        }
    }
    Panel {
        title: panel.title.clone(),
        color: panel.color,
        direction: panel.direction,
        children,
    }
}

/// How many letters a width of units holds, at least one.
fn letters_in(units: i32) -> usize {
    (units / GLYPH).max(1) as usize
}

/// Takes the last lines off a column of text that is taller than `units` until it fits, and says so with dots at the end of the last line left: a text that is longer
/// than the screen is tall (a very long reply) is cut and not drawn off it.
fn cut_to_height(panel: &mut Panel, units: i32, columns: usize) {
    // Only a panel of nothing but text (what the window says, and a reply): a button, a bar or a swatch that was cut off could not be answered or shown, and the work of
    // a description that is not that is bounded by its limits.
    let only_text = panel
        .children
        .iter()
        .all(|child| matches!(child, Node::Label { .. }));
    if panel.direction != Direction::Column || !only_text {
        return;
    }
    let mut cut = false;
    while measure_panel(panel).height > units && panel.children.len() > 1 {
        panel.children.pop();
        cut = true;
    }
    if cut && let Some(Node::Label { text, .. }) = panel.children.last_mut() {
        let kept: String = text.chars().take(columns.saturating_sub(3)).collect();
        *text = format!("{}...", kept.trim_end());
    }
}

/// Lays out these interfaces, in order (later ones over earlier), on a screen of `screen` pixels at `scale` (a whole
/// number from 1), with the keyboard focus on the button `focus`. Panels at one anchor stack away from the edge.
pub fn layout(uis: &[Ui], screen: (i32, i32), scale: i32, focus: Option<u32>) -> Layout {
    let scale = scale.max(1);
    let mut result = Layout::default();
    // How far each anchor's stack has grown, in pixels, away from the edge (or down from the middle).
    let mut stacked = [0i32; 9];
    for (index, ui) in uis.iter().enumerate() {
        // Text wider than the screen is broken into lines, and text taller than it is cut.
        let content = screen.0 / scale - 2 * MARGIN - 2 * (PADDING + BORDER);
        let mut root = wrapped(&ui.root, content);
        cut_to_height(
            &mut root,
            screen.1 / scale - 2 * MARGIN,
            letters_in(content),
        );
        let size = measure_panel(&root);
        let (width, height) = (size.width * scale, size.height * scale);
        let slot = Anchor::ALL
            .iter()
            .position(|anchor| *anchor == ui.anchor)
            .expect("every anchor is in the list");
        let margin = MARGIN * scale;
        let x = match slot % 3 {
            0 => margin,
            1 => (screen.0 - width) / 2,
            _ => screen.0 - margin - width,
        };
        let y = match slot / 3 {
            0 => margin + stacked[slot],
            1 => (screen.1 - height) / 2 + stacked[slot],
            _ => screen.1 - margin - height - stacked[slot],
        };
        stacked[slot] += height + GAP * scale;
        if ui.modal {
            result.draws.push(Draw::Rect {
                area: Rect {
                    x: 0,
                    y: 0,
                    width: screen.0,
                    height: screen.1,
                },
                color: BACKDROP_COLOR,
            });
        }
        let mut painter = Painter {
            draws: &mut result.draws,
            buttons: &mut result.buttons,
            scale,
            focus,
            ui: index,
        };
        painter.panel(x, y, &root);
    }
    result
}
