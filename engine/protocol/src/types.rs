use std::fmt;

/// The most characters a nickname may have.
pub const MAXIMUM_NICKNAME_LENGTH: usize = 32;

/// A server tick: the unit of game time. Tick 0 is a world before its first step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tick(u64);

impl Tick {
    pub const ZERO: Self = Self(0);

    pub const fn new(count: u64) -> Self {
        Self(count)
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    /// The tick after this one. Panics on overflow, which at any real tick rate takes far longer
    /// than the universe has existed.
    pub fn next(self) -> Self {
        Self(self.0.checked_add(1).expect("tick count overflowed"))
    }
}

impl fmt::Display for Tick {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "tick {}", self.0)
    }
}

/// Identifies one connection to a server for as long as the server runs. Ids are handed out in the
/// order clients join and are never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClientId(u32);

impl ClientId {
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for ClientId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "client {}", self.0)
    }
}

/// Identifies one entity of a world for as long as the world lives. Ids come from a counter that is
/// saved with the world and are never reused, so a recording, a reload and a second machine agree on
/// which entity an id means.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityId(u64);

impl EntityId {
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "entity {}", self.0)
    }
}

/// A player's identity: the public key of the keypair their install holds (see the `identity` crate).
/// A server knows a player by it, so it is the same on every connection and in every world.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlayerKey([u8; 32]);

impl PlayerKey {
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for PlayerKey {
    /// The key as 64 hexadecimal digits.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for PlayerKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "PlayerKey({self})")
    }
}

/// What a player is called, and nothing more: a label that a stranger chose, unique among the players of a server at the
/// time and never a reason to trust anyone (a player is their [`PlayerKey`]).
///
/// One to [`MAXIMUM_NICKNAME_LENGTH`] characters of `A-Z`, `a-z`, `0-9` and `_`, and nothing else, so that a nickname
/// pasted into a command can never hold a space, a brace or a quote that changes what the command means.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Nickname(String);

impl Nickname {
    /// What a player with no usable name is called.
    pub const FALLBACK: &'static str = "player";

    /// `text` as a nickname, if it already is one.
    pub fn parse(text: &str) -> Option<Self> {
        let valid = !text.is_empty()
            && text.len() <= MAXIMUM_NICKNAME_LENGTH
            && text
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
        valid.then(|| Self(text.to_owned()))
    }

    /// The nickname closest to what a player asked for: every other character becomes `_`, characters past the
    /// limit are dropped, and nothing at all becomes [`Nickname::FALLBACK`]. A valid nickname is returned unchanged.
    pub fn sanitized(asked_for: &str) -> Self {
        let text: String = asked_for
            .chars()
            .take(MAXIMUM_NICKNAME_LENGTH)
            .map(|character| {
                if character.is_ascii_alphanumeric() || character == '_' {
                    character
                } else {
                    '_'
                }
            })
            .collect();
        if text.is_empty() {
            Self(Self::FALLBACK.to_owned())
        } else {
            Self(text)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether the two are the same name for the purpose of telling players apart: the letters of a nickname are
    /// compared without regard to case, so that `alex` cannot pass for `Alex`.
    pub fn same_as(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }

    /// This nickname with `_2`, `_3`, ... after it (the start is cut short to make room), for a player who asked for a
    /// name that is taken.
    pub fn with_number(&self, number: u32) -> Self {
        let suffix = format!("_{number}");
        let room = MAXIMUM_NICKNAME_LENGTH.saturating_sub(suffix.len());
        let mut text: String = self.0.chars().take(room).collect();
        text.push_str(&suffix);
        Self(text)
    }
}

impl fmt::Display for Nickname {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(test)]
mod nickname_tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn a_nickname_is_one_to_thirty_two_letters_digits_and_underscores_and_nothing_else() {
        for good in ["Alex", "a", "_", "A_1", "x".repeat(32).as_str()] {
            assert_eq!(Nickname::parse(good).unwrap().as_str(), good);
        }
        let too_long = "x".repeat(33);
        for bad in [
            "",
            " ",
            "a b",
            "a-b",
            "a.b",
            "a{b",
            "a}b",
            "a/b",
            "a>b",
            "a\\b",
            "a\"b",
            "a'b",
            "é",
            "a\n",
            "a\u{0}",
            too_long.as_str(),
        ] {
            assert_eq!(Nickname::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn what_a_player_asked_for_is_made_safe_not_refused() {
        let cases = [
            ("Alex", "Alex"),
            ("Alex Smith", "Alex_Smith"),
            ("{>1+1}", "__1_1_"),
            ("/tp 1 2 3", "_tp_1_2_3"),
            ("éé", "__"),
            ("", "player"),
        ];
        for (asked, made) in cases {
            assert_eq!(Nickname::sanitized(asked).as_str(), made, "{asked:?}");
        }
        let long = Nickname::sanitized(&"ab".repeat(40));
        assert_eq!(long.as_str().len(), MAXIMUM_NICKNAME_LENGTH);
        assert!(long.as_str().starts_with("abab"));
    }

    #[test]
    fn two_names_that_differ_only_in_case_are_the_same_name() {
        let alex = Nickname::parse("Alex").unwrap();
        assert!(alex.same_as(&Nickname::parse("aLEX").unwrap()));
        assert!(!alex.same_as(&Nickname::parse("Alex_2").unwrap()));
    }

    #[test]
    fn a_number_goes_after_the_name_and_the_name_gives_way_for_it() {
        let alex = Nickname::parse("Alex").unwrap();
        assert_eq!(alex.with_number(2).as_str(), "Alex_2");
        assert_eq!(alex.with_number(10).as_str(), "Alex_10");
        let long = Nickname::parse(&"x".repeat(32)).unwrap();
        assert_eq!(
            long.with_number(2).as_str(),
            format!("{}_2", "x".repeat(30))
        );
        assert_eq!(
            long.with_number(1234).as_str(),
            format!("{}_1234", "x".repeat(27))
        );
    }

    proptest! {
        #[test]
        fn whatever_is_asked_for_the_result_is_a_valid_nickname(asked in any::<String>()) {
            let made = Nickname::sanitized(&asked);
            prop_assert_eq!(Nickname::parse(made.as_str()), Some(made.clone()));
            // and a name that was fine is not touched
            if let Some(fine) = Nickname::parse(&asked) {
                prop_assert_eq!(fine, made);
            }
        }

        #[test]
        fn a_numbered_name_is_valid_ends_in_its_number_and_is_never_the_name_it_came_from(
            asked in any::<String>(),
            number in 2u32..100_000,
        ) {
            let base = Nickname::sanitized(&asked);
            let numbered = base.with_number(number);
            prop_assert!(Nickname::parse(numbered.as_str()).is_some());
            let suffix = format!("_{number}");
            prop_assert!(numbered.as_str().ends_with(&suffix));
            prop_assert!(!base.same_as(&numbered));
        }
    }
}
