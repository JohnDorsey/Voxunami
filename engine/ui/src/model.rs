//! The tree a user interface is made of, and the limits that keep a hostile description from making a client do unbounded
//! work.

/// A colour as red, green, blue, each 0 to 255.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color(pub [u8; 3]);

impl Color {
    pub const WHITE: Self = Self([255, 255, 255]);
    pub const BLACK: Self = Self([0, 0, 0]);
}

/// Where on the screen a panel sits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl Anchor {
    pub const ALL: [Self; 9] = [
        Self::TopLeft,
        Self::Top,
        Self::TopRight,
        Self::Left,
        Self::Center,
        Self::Right,
        Self::BottomLeft,
        Self::Bottom,
        Self::BottomRight,
    ];
}

/// Which way a panel lays its children out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Row,
    Column,
}

/// A box with an optional title and children that flow in a row or a column.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Panel {
    pub title: Option<String>,
    pub color: Color,
    pub direction: Direction,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Panel(Panel),
    Label {
        text: String,
        color: Color,
    },
    /// Something to press. `id` says which, in the events that come back; it is unique in a [`Ui`].
    Button {
        id: u32,
        text: String,
    },
    /// A bar that is `permille` thousandths full, `cells` letters wide.
    Bar {
        permille: u16,
        cells: u16,
        color: Color,
    },
    /// The colour of a block, as an item's icon, with how many there are if that is worth saying, and whether it is the
    /// one chosen.
    Swatch {
        color: Color,
        count: Option<u32>,
        selected: bool,
    },
    /// Empty room, this many pixels along the way the panel flows.
    Space(u16),
}

/// A whole interface: a panel put at a place on the screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ui {
    /// Whether it takes the player's input (the mouse is freed and the world stops hearing keys) and dims what is behind.
    pub modal: bool,
    pub anchor: Anchor,
    pub root: Panel,
}

/// How deeply panels may be nested, root included.
pub const MAXIMUM_DEPTH: usize = 6;
/// How many nodes (the root's children and theirs, not the root) an interface may have.
pub const MAXIMUM_NODES: usize = 200;
/// How many letters a piece of text may have.
pub const MAXIMUM_TEXT_CHARACTERS: usize = 80;
/// How wide a bar may be, in letters.
pub const MAXIMUM_BAR_CELLS: u16 = 40;
/// How much room a `Space` may make.
pub const MAXIMUM_SPACE: u16 = 64;

/// Why an interface is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum UiError {
    #[error("panels are nested too deeply")]
    TooDeep,
    #[error("there are too many parts")]
    TooManyNodes,
    #[error("a piece of text is too long")]
    TextTooLong,
    #[error("two buttons have one identifier")]
    DuplicateButton,
    #[error("a bar is more than full")]
    BarOverfull,
    #[error("a bar is too wide")]
    BarTooWide,
    #[error("a space is too big")]
    SpaceTooBig,
}

impl UiError {
    /// The same, as the text a decoder reports.
    pub fn reason(self) -> &'static str {
        match self {
            Self::TooDeep => "panels are nested too deeply",
            Self::TooManyNodes => "there are too many parts",
            Self::TextTooLong => "a piece of text is too long",
            Self::DuplicateButton => "two buttons have one identifier",
            Self::BarOverfull => "a bar is more than full",
            Self::BarTooWide => "a bar is too wide",
            Self::SpaceTooBig => "a space is too big",
        }
    }
}

fn check_text(text: &str) -> Result<(), UiError> {
    if text.chars().count() > MAXIMUM_TEXT_CHARACTERS {
        return Err(UiError::TextTooLong);
    }
    Ok(())
}

struct Checking {
    nodes: usize,
    buttons: Vec<u32>,
}

fn check_panel(panel: &Panel, depth: usize, checking: &mut Checking) -> Result<(), UiError> {
    if depth > MAXIMUM_DEPTH {
        return Err(UiError::TooDeep);
    }
    if let Some(title) = &panel.title {
        check_text(title)?;
    }
    for child in &panel.children {
        checking.nodes += 1;
        if checking.nodes > MAXIMUM_NODES {
            return Err(UiError::TooManyNodes);
        }
        match child {
            Node::Panel(inner) => check_panel(inner, depth + 1, checking)?,
            Node::Label { text, .. } => check_text(text)?,
            Node::Button { id, text } => {
                check_text(text)?;
                if checking.buttons.contains(id) {
                    return Err(UiError::DuplicateButton);
                }
                checking.buttons.push(*id);
            }
            Node::Bar {
                permille, cells, ..
            } => {
                if *permille > 1000 {
                    return Err(UiError::BarOverfull);
                }
                if *cells > MAXIMUM_BAR_CELLS {
                    return Err(UiError::BarTooWide);
                }
            }
            Node::Swatch { .. } => {}
            Node::Space(size) => {
                if *size > MAXIMUM_SPACE {
                    return Err(UiError::SpaceTooBig);
                }
            }
        }
    }
    Ok(())
}

impl Ui {
    /// Whether the interface is within the limits above.
    pub fn check(&self) -> Result<(), UiError> {
        let mut checking = Checking {
            nodes: 0,
            buttons: Vec::new(),
        };
        check_panel(&self.root, 1, &mut checking)
    }

    /// The identifiers of its buttons, in the order they are met going down the tree: the order the keyboard walks them
    /// and the numbers pick them.
    pub fn button_ids(&self) -> Vec<u32> {
        fn collect(panel: &Panel, found: &mut Vec<u32>) {
            for child in &panel.children {
                match child {
                    Node::Panel(inner) => collect(inner, found),
                    Node::Button { id, .. } => found.push(*id),
                    _ => {}
                }
            }
        }
        let mut found = Vec::new();
        collect(&self.root, &mut found);
        found
    }
}

/// The button the keyboard moves to from `current` (or the first, if none is chosen), wrapping round the ends.
pub fn next_button(ids: &[u32], current: Option<u32>, forward: bool) -> Option<u32> {
    if ids.is_empty() {
        return None;
    }
    let position = current.and_then(|current| ids.iter().position(|id| *id == current));
    let next = match (position, forward) {
        (None, true) => 0,
        (None, false) => ids.len() - 1,
        (Some(position), true) => (position + 1) % ids.len(),
        (Some(position), false) => (position + ids.len() - 1) % ids.len(),
    };
    Some(ids[next])
}
