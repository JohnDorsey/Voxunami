//! Checking the packs a client holds against the package a server sent (`docs/specs/pack-archive-spec.md`, step 5).
//!
//! A server sends its clients the *result* of loading its packs (the package: how every block looks, the pictures) and, since protocol 13, the packs themselves. A client
//! that has all of them loads them with its own copy of the loader, over its own copy of the game, and compares what that makes with what the server sent. If they are
//! the same, the server's package is the packs' (nothing the loader does depends on which program ran it); if they are not, a loader, a schema or an order differs
//! between the two programs, or the server's package is not what its packs make, and that is **told loudly, not hidden**. The client goes on drawing with what the
//! server sent either way: the check only tells.

use std::collections::BTreeMap;

use namespaced_id::NamespacedId;

use crate::package_data::PackageData;

/// What the check of the packs found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackCheck {
    /// Not made yet: the packs the server listed are not all here, or no tick has come since they were.
    Waiting,
    /// There is nothing to check against: the server runs packs that it does not replicate (`"replicate": false`, or given to it in memory), so the packs this client
    /// has are not all that is behind its package.
    NotApplicable,
    /// The client loaded the packs it holds, over its own copy of the game, and made the package the server sent (the blocks by name, the pictures, the properties).
    Agrees,
    /// It did not, or it could not load them: one line for each difference, naming the block or what it is.
    Disagrees(Vec<String>),
}

/// The blocks of a package that have a definition, by name.
fn defined_blocks(
    package: &PackageData,
) -> BTreeMap<&NamespacedId, &block_models::BlockDefinition> {
    package
        .blocks
        .iter()
        .filter_map(|(identifier, definition)| Some((identifier, definition.as_ref()?)))
        .collect()
}

/// How `local`, what the client made of the packs it holds, differs from `server`, what the server sent. **By name, not by number**: the numbers of blocks are the
/// world's (a server that reloaded keeps the number of a block that went, as missing content), so the order and the entries without a definition are not compared.
/// Empty when they are the same.
pub fn differences(server: &PackageData, local: &PackageData) -> Vec<String> {
    let mut lines = Vec::new();
    if server.block_registry != local.block_registry {
        lines.push(format!(
            "the registry of blocks is {} here and {} on the server",
            local.block_registry, server.block_registry
        ));
    }
    if server.block_channel != local.block_channel
        || server.state_channel != local.state_channel
        || server.orientation_channel != local.orientation_channel
        || server.light_channel != local.light_channel
    {
        lines.push("the channels of a brick (block, state, orientation, light) differ".to_owned());
    }
    if server.ambient_light != local.ambient_light {
        lines.push(format!(
            "unlit faces are {} here and {} on the server",
            local.ambient_light, server.ambient_light
        ));
    }
    if server.properties != local.properties {
        lines.push("the properties a block can have differ".to_owned());
    }
    // The blocks that have a definition, by name.
    let (server_blocks, local_blocks) = (defined_blocks(server), defined_blocks(local));
    for (identifier, definition) in &server_blocks {
        match local_blocks.get(identifier) {
            None => lines.push(format!(
                "the server has the block {identifier}, which these packs do not make"
            )),
            Some(made) if made != definition => lines.push(format!(
                "the block {identifier} is not what these packs make of it"
            )),
            Some(_) => {}
        }
    }
    for identifier in local_blocks.keys() {
        if !server_blocks.contains_key(*identifier) {
            lines.push(format!(
                "these packs make the block {identifier}, which the server does not have"
            ));
        }
    }
    if server.tile_classes != local.tile_classes {
        lines.push(
            "the pictures differ (their sizes, or which there are, or their pixels)".to_owned(),
        );
    }
    lines
}

#[cfg(test)]
mod tests {
    use block_models::{BlockDefinition, PropertyDeclaration};
    use data_packs::Tile;

    use super::*;

    fn id(text: &str) -> NamespacedId {
        text.parse().unwrap()
    }

    fn package(blocks: &[(&str, Option<[f32; 3]>)]) -> PackageData {
        PackageData {
            blocks: blocks
                .iter()
                .map(|(name, color)| (id(name), color.map(BlockDefinition::cube)))
                .collect(),
            ..PackageData::for_tests()
        }
    }

    const RED: [f32; 3] = [1.0, 0.0, 0.0];
    const BLUE: [f32; 3] = [0.0, 0.0, 1.0];

    #[test]
    fn the_same_package_differs_in_nothing() {
        let a = package(&[("g:air", Some(RED)), ("g:stone", Some(BLUE))]);
        assert_eq!(differences(&a, &a.clone()), Vec::<String>::new());
    }

    #[test]
    fn the_numbers_of_blocks_and_blocks_with_nothing_to_say_are_not_compared() {
        // The server kept the number of a block a reload took out, as missing content, and has the blocks in the world's order; the client made them in the packs'.
        let server = package(&[
            ("g:air", Some(RED)),
            ("g:gone", None),
            ("g:stone", Some(BLUE)),
        ]);
        let local = package(&[("g:stone", Some(BLUE)), ("g:air", Some(RED))]);
        assert_eq!(differences(&server, &local), Vec::<String>::new());
    }

    #[test]
    fn a_block_that_is_not_what_the_packs_make_is_named() {
        let server = package(&[("g:air", Some(RED)), ("g:stone", Some(BLUE))]);
        let local = package(&[("g:air", Some(RED)), ("g:stone", Some(RED))]);
        assert_eq!(
            differences(&server, &local),
            ["the block g:stone is not what these packs make of it"]
        );
    }

    #[test]
    fn a_block_on_one_side_only_is_named_whichever_side_has_it() {
        let server = package(&[("g:air", Some(RED)), ("g:only_server", Some(BLUE))]);
        let local = package(&[("g:air", Some(RED)), ("g:only_here", Some(BLUE))]);
        assert_eq!(
            differences(&server, &local),
            [
                "the server has the block g:only_server, which these packs do not make",
                "these packs make the block g:only_here, which the server does not have"
            ]
        );
        // a block the server has as missing content and the packs make is a block the server does not have
        let server = package(&[("g:air", Some(RED)), ("g:late", None)]);
        let local = package(&[("g:air", Some(RED)), ("g:late", Some(BLUE))]);
        assert_eq!(
            differences(&server, &local),
            ["these packs make the block g:late, which the server does not have"]
        );
    }

    #[test]
    fn what_is_not_a_block_is_compared_too_and_each_difference_is_a_line() {
        let base = package(&[("g:air", Some(RED))]);
        let mut other = base.clone();
        other.ambient_light = 9;
        other.block_registry = id("g:other_block");
        other.state_channel = Some(id("g:state"));
        other.tile_classes = vec![vec![Tile::from_rgba(8, vec![0; 8 * 8 * 4])]];
        let mut properties = block_models::PropertySchema::new();
        properties
            .declare(PropertyDeclaration {
                id: id("g:extra"),
                kind: block_models::PropertyKind::Bool,
                default: 0,
                doc: "an extra property".to_owned(),
            })
            .unwrap();
        other.properties = properties;
        let lines = differences(&base, &other);
        assert_eq!(lines.len(), 5, "{lines:?}");
        assert!(lines.iter().any(|line| line.contains("registry of blocks")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("channels of a brick"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("unlit faces are 9 here and 0"))
        );
        assert!(lines.iter().any(|line| line.contains("properties")));
        assert!(lines.iter().any(|line| line.contains("pictures")));
    }
}
