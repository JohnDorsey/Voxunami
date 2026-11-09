/// How a block that can be seen through draws its faces and its pictures. A block's mode is written by its author as one word (`"leaves"`), and each mode is a
/// fixed set of answers to the questions a renderer asks:
///
/// | mode | holes in the picture | faces between two voxels of the block | faces seen from behind |
/// |---|---|---|---|
/// | `opaque` | no: the picture is drawn whole, whatever its alpha | not drawn | not drawn |
/// | `leaves` | yes | drawn | drawn |
/// | `glass` | yes | not drawn | drawn |
///
/// In every mode the faces that look at the player are drawn. A mode other than `opaque` is for a block that is `sight_through`: it is what shows through the holes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Transparency {
    #[default]
    Opaque,
    Leaves,
    Glass,
}

impl Transparency {
    /// Every mode, in the order of their codes.
    pub const ALL: [Self; 3] = [Self::Opaque, Self::Leaves, Self::Glass];

    /// The word a block file writes for the mode.
    pub fn word(self) -> &'static str {
        match self {
            Self::Opaque => "opaque",
            Self::Leaves => "leaves",
            Self::Glass => "glass",
        }
    }

    /// The mode a word names.
    pub fn from_word(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.word() == word)
    }

    /// The number the mode is stored as (its place in [`Transparency::ALL`]).
    pub fn code(self) -> u8 {
        match self {
            Self::Opaque => 0,
            Self::Leaves => 1,
            Self::Glass => 2,
        }
    }

    /// The mode a stored number names.
    pub fn from_code(code: u8) -> Option<Self> {
        Self::ALL.get(usize::from(code)).copied()
    }

    /// Whether the clear pixels of a picture are holes (and not drawn).
    pub fn cuts_holes(self) -> bool {
        self != Self::Opaque
    }

    /// Whether the faces between two voxels of the block are drawn, and not only the outside of a body of it.
    pub fn shows_inner_faces(self) -> bool {
        self == Self::Leaves
    }

    /// Whether a face is also drawn to someone behind it, inside the voxel.
    pub fn shows_back_faces(self) -> bool {
        self != Self::Opaque
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_modes_answer_as_the_table_of_their_documentation_says() {
        // (holes, inner faces, back faces) for each mode, written out by hand.
        let expected = [
            (Transparency::Opaque, (false, false, false)),
            (Transparency::Leaves, (true, true, true)),
            (Transparency::Glass, (true, false, true)),
        ];
        for (mode, answers) in expected {
            assert_eq!(
                (
                    mode.cuts_holes(),
                    mode.shows_inner_faces(),
                    mode.shows_back_faces()
                ),
                answers,
                "{mode:?}"
            );
        }
        assert_eq!(Transparency::default(), Transparency::Opaque);
    }

    #[test]
    fn a_mode_survives_its_word_and_its_code_and_nothing_else_is_a_mode() {
        for (position, mode) in Transparency::ALL.into_iter().enumerate() {
            assert_eq!(Transparency::from_word(mode.word()), Some(mode));
            assert_eq!(usize::from(mode.code()), position);
            assert_eq!(Transparency::from_code(mode.code()), Some(mode));
        }
        for word in ["", "Leaves", "clear", "leaves "] {
            assert_eq!(Transparency::from_word(word), None, "{word:?}");
        }
        assert_eq!(Transparency::from_code(3), None);
        assert_eq!(Transparency::from_code(255), None);
    }
}
