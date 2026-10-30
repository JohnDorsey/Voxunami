//! The layering the project stands on, read from the workspace itself: games use plugins, plugins use the engine, and
//! the engine knows nothing of either. A crate that breaks this fails here, naming the edge, rather than in a review
//! months later when it is hard to undo.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

/// From the bottom: what a crate may depend on is its own layer and those below it.
const LAYERS: [&str; 4] = ["engine", "plugins", "games", "launcher"];

/// One crate of the workspace and what it depends on, by name and kind (`None` for the normal ones).
struct Member {
    layer: usize,
    dependencies: Vec<(String, Option<String>)>,
}

fn workspace() -> BTreeMap<String, Member> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let output = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .current_dir(root)
        .output()
        .expect("cargo runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: Value = serde_json::from_slice(&output.stdout).unwrap();
    let mut members = BTreeMap::new();
    for package in metadata["packages"].as_array().unwrap() {
        let name = package["name"].as_str().unwrap().to_owned();
        let manifest = Path::new(package["manifest_path"].as_str().unwrap());
        let relative = manifest.strip_prefix(root).unwrap_or_else(|_| {
            panic!(
                "{name} is outside the workspace folder: {}",
                manifest.display()
            )
        });
        let folder = relative.iter().next().unwrap().to_string_lossy();
        let layer = LAYERS
            .iter()
            .position(|layer| *layer == folder)
            .unwrap_or_else(|| {
                panic!(
                    "{name} is in the folder \"{folder}\", which is not a layer; the layers are {LAYERS:?}"
                )
            });
        let dependencies = package["dependencies"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|dependency| dependency["path"].is_string())
            .map(|dependency| {
                (
                    dependency["name"].as_str().unwrap().to_owned(),
                    dependency["kind"].as_str().map(str::to_owned),
                )
            })
            .collect();
        members.insert(
            name,
            Member {
                layer,
                dependencies,
            },
        );
    }
    members
}

#[test]
fn no_crate_depends_on_a_layer_above_it() {
    let members = workspace();
    let mut breaks = Vec::new();
    for (name, member) in &members {
        for (dependency, kind) in &member.dependencies {
            let below = members[dependency].layer;
            if below > member.layer {
                breaks.push(format!(
                    "{name} ({}) depends on {dependency} ({}){}",
                    LAYERS[member.layer],
                    LAYERS[below],
                    kind.as_ref()
                        .map(|kind| format!(" as a {kind}"))
                        .unwrap_or_default()
                ));
            }
        }
    }
    assert!(breaks.is_empty(), "upward edges:\n{}", breaks.join("\n"));
}

#[test]
fn games_do_not_depend_on_each_other() {
    let members = workspace();
    let games = LAYERS.iter().position(|layer| *layer == "games").unwrap();
    let mut breaks = Vec::new();
    for (name, member) in members.iter().filter(|(_, member)| member.layer == games) {
        for (dependency, _) in &member.dependencies {
            if members[dependency].layer == games {
                breaks.push(format!("{name} depends on {dependency}"));
            }
        }
    }
    assert!(
        breaks.is_empty(),
        "a game that needs what another game has promotes it to a plugin or to the engine:\n{}",
        breaks.join("\n")
    );
}

#[test]
fn the_layers_are_all_in_use() {
    // A layer nobody is in would mean the folder names above have drifted from the workspace.
    let members = workspace();
    for (index, layer) in LAYERS.iter().enumerate() {
        assert!(
            members.values().any(|member| member.layer == index),
            "no crate is in {layer}"
        );
    }
}
