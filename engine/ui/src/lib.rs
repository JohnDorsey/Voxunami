//! A small user interface that a server can describe and a client can draw: a tree of panels, labels, buttons, bars and
//! swatches (see [`Ui`]), the limits that keep a hostile description cheap, its byte form for the messages that carry it, and
//! a layout that turns it into rectangles and text at whole pixels. Nothing here draws or knows of a window.
//! See `docs/ui-spec.md`.

mod codec;
mod layout;
mod model;
mod paint;

pub use layout::{
    BACKDROP_COLOR, BAR_BACKGROUND, BORDER_COLOR, BUTTON_COLOR, ButtonArea, Draw,
    FOCUSED_BUTTON_COLOR, GLYPH, Layout, Rect, layout, scale_for_height,
};
pub use model::{
    Anchor, Color, Direction, MAXIMUM_BAR_CELLS, MAXIMUM_DEPTH, MAXIMUM_NODES, MAXIMUM_SPACE,
    MAXIMUM_TEXT_CHARACTERS, Node, Panel, Ui, UiError, next_button,
};
pub use paint::{Painted, paint};
