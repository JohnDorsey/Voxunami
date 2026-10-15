//! Stable `namespace:name` identifiers.
//!
//! Anything written to a save file or sent over the network refers to content by
//! [`NamespacedId`], never by a runtime number. Numbers are assigned per run (see the
//! `content_registry` crate) and change whenever the set of plugins changes.
//!
//! The namespace is the plugin that owns the content, so two plugins can both define `stone`
//! without clashing: `base_blocks:stone`, `fantasy_blocks:stone`.

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

/// Longest allowed namespace or name, in characters.
pub const MAXIMUM_PART_LENGTH: usize = 64;

/// A validated `namespace:name` identifier. Cloning is cheap (shared text).
///
/// Each part starts with a lowercase ASCII letter, continues with lowercase ASCII letters, digits
/// or `_`, and has at most [`MAXIMUM_PART_LENGTH`] characters. Ordering is by namespace, then
/// name, which is deliberately not the order of the raw text (`'1'` sorts before `':'`).
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct NamespacedId {
    text: Arc<str>,
    /// Byte index of the `:` within `text`.
    separator_index: usize,
}

/// Which half of an identifier a [`NamespacedIdError`] refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdPart {
    Namespace,
    Name,
}

impl fmt::Display for IdPart {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Namespace => "namespace",
            Self::Name => "name",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum NamespacedIdError {
    #[error("{text:?} must contain exactly one ':' between the namespace and the name")]
    WrongSeparatorCount { text: String },
    #[error(
        "{part} {value:?} must start with a lowercase letter, contain only lowercase letters, \
         digits and '_', and have at most {} characters",
        MAXIMUM_PART_LENGTH
    )]
    InvalidPart { part: IdPart, value: String },
}

impl NamespacedId {
    pub fn new(namespace: &str, name: &str) -> Result<Self, NamespacedIdError> {
        validate_part(IdPart::Namespace, namespace)?;
        validate_part(IdPart::Name, name)?;
        Ok(Self {
            text: format!("{namespace}:{name}").into(),
            separator_index: namespace.len(),
        })
    }

    pub fn namespace(&self) -> &str {
        &self.text[..self.separator_index]
    }

    pub fn name(&self) -> &str {
        &self.text[self.separator_index + 1..]
    }

    /// The full `namespace:name` text.
    pub fn as_str(&self) -> &str {
        &self.text
    }
}

fn validate_part(part: IdPart, value: &str) -> Result<(), NamespacedIdError> {
    let bytes = value.as_bytes();
    let is_valid = (1..=MAXIMUM_PART_LENGTH).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_');
    if is_valid {
        Ok(())
    } else {
        Err(NamespacedIdError::InvalidPart {
            part,
            value: value.to_owned(),
        })
    }
}

impl FromStr for NamespacedId {
    type Err = NamespacedIdError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut parts = text.split(':');
        match (parts.next(), parts.next(), parts.next()) {
            (Some(namespace), Some(name), None) => Self::new(namespace, name),
            _ => Err(NamespacedIdError::WrongSeparatorCount {
                text: text.to_owned(),
            }),
        }
    }
}

impl fmt::Display for NamespacedId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.text)
    }
}

impl fmt::Debug for NamespacedId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("NamespacedId")
            .field(&&*self.text)
            .finish()
    }
}

impl Ord for NamespacedId {
    fn cmp(&self, other: &Self) -> Ordering {
        self.namespace()
            .cmp(other.namespace())
            .then_with(|| self.name().cmp(other.name()))
    }
}

impl PartialOrd for NamespacedId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::test_support::{namespaced_id, valid_part};

    fn parse(text: &str) -> Result<NamespacedId, NamespacedIdError> {
        text.parse()
    }

    #[test]
    fn accepts_well_formed_identifiers() {
        for text in [
            "base_blocks:stone",
            "a:b",
            "tile_2048:slide_left",
            "z9_:q0_",
        ] {
            let id = parse(text).unwrap_or_else(|error| panic!("{text:?} should parse: {error}"));
            assert_eq!(id.as_str(), text);
        }
    }

    #[test]
    fn splits_into_namespace_and_name() {
        let id = parse("base_blocks:stone_brick").unwrap();
        assert_eq!(id.namespace(), "base_blocks");
        assert_eq!(id.name(), "stone_brick");
    }

    #[test]
    fn rejects_wrong_separator_counts() {
        for text in ["", "stone", "a:b:c", "::"] {
            assert_eq!(
                parse(text),
                Err(NamespacedIdError::WrongSeparatorCount {
                    text: text.to_owned()
                }),
                "{text:?}"
            );
        }
    }

    #[test]
    fn rejects_invalid_parts_and_reports_which_part() {
        let cases = [
            (":stone", IdPart::Namespace, ""),
            ("base:", IdPart::Name, ""),
            ("Base:stone", IdPart::Namespace, "Base"),
            ("base:Stone", IdPart::Name, "Stone"),
            ("1base:stone", IdPart::Namespace, "1base"),
            ("base:_stone", IdPart::Name, "_stone"),
            ("base:stone-block", IdPart::Name, "stone-block"),
            ("base:stone block", IdPart::Name, "stone block"),
            ("bäse:stone", IdPart::Namespace, "bäse"),
        ];
        for (text, part, value) in cases {
            assert_eq!(
                parse(text),
                Err(NamespacedIdError::InvalidPart {
                    part,
                    value: value.to_owned()
                }),
                "{text:?}"
            );
        }
    }

    #[test]
    fn enforces_the_maximum_part_length() {
        let longest = "a".repeat(MAXIMUM_PART_LENGTH);
        let too_long = "a".repeat(MAXIMUM_PART_LENGTH + 1);
        assert!(NamespacedId::new(&longest, &longest).is_ok());
        assert_eq!(
            NamespacedId::new(&too_long, "name"),
            Err(NamespacedIdError::InvalidPart {
                part: IdPart::Namespace,
                value: too_long.clone()
            })
        );
        assert_eq!(
            NamespacedId::new("space", &too_long),
            Err(NamespacedIdError::InvalidPart {
                part: IdPart::Name,
                value: too_long
            })
        );
    }

    #[test]
    fn orders_by_namespace_then_name_not_by_raw_text() {
        // Raw text order puts "a1:b" first because '1' (0x31) sorts before ':' (0x3A).
        assert!("a1:b" < "a:z");
        // Identifier order compares namespaces first: "a" < "a1".
        assert!(parse("a:z").unwrap() < parse("a1:b").unwrap());
        assert!(parse("a:b").unwrap() < parse("a:c").unwrap());
    }

    #[test]
    fn debug_output_shows_the_text() {
        assert_eq!(
            format!("{:?}", parse("base:stone").unwrap()),
            r#"NamespacedId("base:stone")"#
        );
    }

    proptest! {
        #[test]
        fn display_then_parse_round_trips(id in namespaced_id()) {
            prop_assert_eq!(id.to_string().parse::<NamespacedId>(), Ok(id));
        }

        #[test]
        fn parsing_agrees_with_new(namespace in valid_part(), name in valid_part()) {
            let from_parts = NamespacedId::new(&namespace, &name).unwrap();
            let from_text = format!("{namespace}:{name}").parse::<NamespacedId>().unwrap();
            prop_assert_eq!(from_text, from_parts);
        }

        #[test]
        fn ordering_matches_the_namespace_name_pair(a in namespaced_id(), b in namespaced_id()) {
            prop_assert_eq!(
                a.cmp(&b),
                (a.namespace(), a.name()).cmp(&(b.namespace(), b.name()))
            );
        }

        #[test]
        fn arbitrary_text_never_panics_and_accepted_text_is_preserved(text in ".*") {
            if let Ok(id) = text.parse::<NamespacedId>() {
                prop_assert_eq!(id.as_str(), text.as_str());
            }
        }
    }
}
