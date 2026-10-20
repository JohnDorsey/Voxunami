//! Putting packs together: reading every file, ordering packs by what they depend on, adding their blocks, and
//! applying their patches. Every problem is collected, and a patch that cannot be applied changes nothing.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use block_models::{BlockDefinition, PropertyDeclaration, PropertyKind, PropertySchema};
use namespaced_id::NamespacedId;

use crate::block::read_block;
use crate::error::{PackError, PackErrors};
use crate::manifest::{PackManifest, read_manifest};
use crate::patch::{Operation, Patch, PatchValue, Place, PropertyValue, read_patch};
use crate::walk::{nearest, quoted};

/// A file of a pack, read but not yet understood: its name (without `.json`) and its text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawFile {
    pub name: String,
    pub text: String,
}

/// A pack's files, as found in its folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawPack {
    /// The folder's name, which must be the pack's id.
    pub folder: String,
    /// The text of `pack.json`.
    pub manifest: String,
    /// The files of `blocks/`.
    pub blocks: Vec<RawFile>,
    /// The files of `patches/`.
    pub patches: Vec<RawFile>,
}

/// Every block there is after the packs are in, and what was left undone on purpose.
#[derive(Clone, Debug, PartialEq)]
pub struct Loaded {
    /// The blocks the game had (patched where a pack said so), in their order, then the blocks the packs
    /// added, in the order the packs were applied.
    pub blocks: Vec<(NamespacedId, BlockDefinition)>,
    /// What was skipped because a patch said it was optional, one line each. Show them to whoever runs the
    /// server.
    pub warnings: Vec<String>,
}

/// A pack read and understood, with the names of its files for messages.
struct ReadPack {
    manifest: PackManifest,
    manifest_file: String,
    blocks: Vec<(String, NamespacedId, BlockDefinition)>,
    patches: Vec<(String, Patch)>,
}

/// Loads `packs` on top of `base`, the blocks the game has. `schema` says which properties exist.
///
/// The files of a pack are applied in order of their names and the packs in dependency order, ties by the pack's
/// id, so the result does not depend on the order `packs` is given in.
pub fn load_packs(
    packs: &[RawPack],
    base: &[(NamespacedId, BlockDefinition)],
    schema: &PropertySchema,
) -> Result<Loaded, PackErrors> {
    let mut errors = Vec::new();
    let read: Vec<ReadPack> = packs
        .iter()
        .filter_map(|pack| read_pack(pack, schema, &mut errors))
        .collect();
    if !errors.is_empty() {
        return Err(PackErrors(errors));
    }
    let order = order_by_dependencies(&read, base, &mut errors);
    if !errors.is_empty() {
        return Err(PackErrors(errors));
    }

    let mut working: Vec<(NamespacedId, BlockDefinition)> = base.to_vec();
    let mut index: HashMap<NamespacedId, usize> = working
        .iter()
        .enumerate()
        .map(|(position, (identifier, _))| (identifier.clone(), position))
        .collect();
    let mut warnings = Vec::new();
    // For each pack, the namespaces whose blocks it may patch: its own, and the ones it names as needs, and
    // what those packs name in turn.
    let mut named: HashMap<&str, BTreeSet<String>> = HashMap::new();
    for pack_index in order {
        let pack = &read[pack_index];
        let id = pack.manifest.id.as_str();
        let mut allowed: BTreeSet<String> = BTreeSet::from([id.to_owned()]);
        for dependency in &pack.manifest.depends_on {
            allowed.insert(dependency.clone());
            if let Some(inherited) = named.get(dependency.as_str()) {
                allowed.extend(inherited.iter().cloned());
            }
        }
        named.insert(id, allowed.clone());

        let mut blocks: Vec<&(String, NamespacedId, BlockDefinition)> =
            pack.blocks.iter().collect();
        blocks.sort_by(|first, second| first.0.cmp(&second.0));
        for (file, identifier, definition) in blocks {
            if index.contains_key(identifier) {
                errors.push(PackError {
                    file: file.clone(),
                    path: String::new(),
                    message: format!("the block {identifier} already exists"),
                });
                continue;
            }
            index.insert(identifier.clone(), working.len());
            working.push((identifier.clone(), definition.clone()));
        }

        let mut patches: Vec<&(String, Patch)> = pack.patches.iter().collect();
        patches.sort_by(|first, second| first.0.cmp(&second.0));
        for (file, patch) in patches {
            match apply_patch(file, patch, &mut working, &index, schema, &allowed) {
                Ok(Applied::Done) => {}
                Ok(Applied::Skipped(why)) => warnings.push(format!("{file}: skipped: {why}")),
                Err(mut found) => errors.append(&mut found),
            }
        }
    }
    if !errors.is_empty() {
        return Err(PackErrors(errors));
    }
    Ok(Loaded {
        blocks: working,
        warnings,
    })
}

fn read_pack(
    pack: &RawPack,
    schema: &PropertySchema,
    errors: &mut Vec<PackError>,
) -> Option<ReadPack> {
    let manifest_file = format!("{}/pack.json", pack.folder);
    let manifest = match read_manifest(&manifest_file, &pack.manifest) {
        Ok(manifest) => manifest,
        Err(found) => {
            errors.extend(found.0);
            return None;
        }
    };
    if manifest.id != pack.folder {
        errors.push(PackError {
            file: manifest_file,
            path: "/id".to_owned(),
            message: format!(
                "the folder is called \"{}\" but pack.json says \"{}\"; they must be the same",
                pack.folder, manifest.id
            ),
        });
        return None;
    }
    let mut good = true;
    let mut blocks = Vec::new();
    for file in &pack.blocks {
        let name = format!("{}/blocks/{}.json", pack.folder, file.name);
        match read_block(&name, &manifest.id, &file.name, &file.text, schema) {
            Ok((identifier, definition)) => blocks.push((name, identifier, definition)),
            Err(found) => {
                errors.extend(found.0);
                good = false;
            }
        }
    }
    let mut patches = Vec::new();
    for file in &pack.patches {
        let name = format!("{}/patches/{}.json", pack.folder, file.name);
        match read_patch(&name, &file.text) {
            Ok(patch) => patches.push((name, patch)),
            Err(found) => {
                errors.extend(found.0);
                good = false;
            }
        }
    }
    good.then_some(ReadPack {
        manifest,
        manifest_file,
        blocks,
        patches,
    })
}

/// The packs, each after those it depends on, ties by id: positions in `read`. Problems with the dependencies
/// (one that does not exist, or a cycle) go in `errors`.
fn order_by_dependencies(
    read: &[ReadPack],
    base: &[(NamespacedId, BlockDefinition)],
    errors: &mut Vec<PackError>,
) -> Vec<usize> {
    let base_namespaces: BTreeSet<&str> = base
        .iter()
        .map(|(identifier, _)| identifier.namespace())
        .collect();
    let by_id: BTreeMap<&str, usize> = read
        .iter()
        .enumerate()
        .map(|(position, pack)| (pack.manifest.id.as_str(), position))
        .collect();
    // What each pack waits for among the packs.
    let mut waiting_for: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); read.len()];
    for (position, pack) in read.iter().enumerate() {
        for (index, dependency) in pack.manifest.depends_on.iter().enumerate() {
            match by_id.get(dependency.as_str()) {
                Some(&other) => {
                    waiting_for[position].insert(other);
                }
                None if base_namespaces.contains(dependency.as_str()) => {}
                None => errors.push(PackError {
                    file: pack.manifest_file.clone(),
                    path: format!("/depends_on/{index}"),
                    message: format!(
                        "\"{dependency}\" is not a pack here and the game has no blocks of that name"
                    ),
                }),
            }
        }
    }
    if !errors.is_empty() {
        return Vec::new();
    }
    let mut order = Vec::with_capacity(read.len());
    let mut done = vec![false; read.len()];
    loop {
        // The next pack, by id, whose dependencies are all done.
        let next = by_id.values().copied().find(|&position| {
            !done[position] && waiting_for[position].iter().all(|&other| done[other])
        });
        match next {
            Some(position) => {
                done[position] = true;
                order.push(position);
            }
            None => break,
        }
    }
    if order.len() < read.len() {
        let stuck: Vec<&str> = by_id
            .iter()
            .filter(|(_, position)| !done[**position])
            .map(|(id, _)| *id)
            .collect();
        for &position in by_id.values().filter(|&&position| !done[position]) {
            errors.push(PackError {
                file: read[position].manifest_file.clone(),
                path: "/depends_on".to_owned(),
                message: format!(
                    "these packs wait for each other, so none can load first: {}",
                    quoted(&stuck)
                ),
            });
        }
        return Vec::new();
    }
    order
}

enum Applied {
    Done,
    Skipped(String),
}

fn error(file: &str, path: String, message: impl Into<String>) -> PackError {
    PackError {
        file: file.to_owned(),
        path,
        message: message.into(),
    }
}

/// Applies one patch to the block it names, or says why it cannot be applied, changing nothing in that case.
fn apply_patch(
    file: &str,
    patch: &Patch,
    working: &mut [(NamespacedId, BlockDefinition)],
    index: &HashMap<NamespacedId, usize>,
    schema: &PropertySchema,
    allowed: &BTreeSet<String>,
) -> Result<Applied, Vec<PackError>> {
    let target = &patch.target;
    if !allowed.contains(target.namespace()) {
        return Err(vec![error(
            file,
            "/target".to_owned(),
            format!(
                "{target} is in \"{}\", which this pack does not depend on (add it to \"depends_on\" in pack.json)",
                target.namespace()
            ),
        )]);
    }
    let Some(&position) = index.get(target) else {
        let message = format!("there is no block {target} (is its pack or plugin loaded?)");
        return if patch.optional {
            Ok(Applied::Skipped(message))
        } else {
            Err(vec![error(file, "/target".to_owned(), message)])
        };
    };
    let mut edited = working[position].1.clone();
    let mut errors = Vec::new();
    for (number, operation) in patch.operations.iter().enumerate() {
        let path = format!("/operations/{number}");
        match apply_operation(&mut edited, operation, schema, &path) {
            Ok(()) => {}
            Err(Failure::Undeclared(name)) if patch.optional => {
                return Ok(Applied::Skipped(format!(
                    "the property {name} is not declared here (an optional patch)"
                )));
            }
            Err(Failure::Undeclared(name)) => errors.push(error(
                file,
                format!("{path}/property"),
                undeclared_message(&name, schema),
            )),
            Err(Failure::Other { path, message }) => errors.push(error(file, path, message)),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    working[position].1 = edited;
    Ok(Applied::Done)
}

enum Failure {
    Undeclared(String),
    Other { path: String, message: String },
}

fn other(path: &str, suffix: &str, message: impl Into<String>) -> Failure {
    Failure::Other {
        path: format!("{path}{suffix}"),
        message: message.into(),
    }
}

fn undeclared_message(name: &str, schema: &PropertySchema) -> String {
    let declared: Vec<&str> = schema
        .iter()
        .map(|declaration| declaration.id.as_str())
        .collect();
    let hint = nearest(name, &declared)
        .map(|near| format!(" (did you mean \"{near}\"?)"))
        .unwrap_or_default();
    format!(
        "the property \"{name}\" is not declared{hint}; the properties are {}",
        quoted(&declared)
    )
}

fn apply_operation(
    block: &mut BlockDefinition,
    operation: &Operation,
    schema: &PropertySchema,
    path: &str,
) -> Result<(), Failure> {
    match operation {
        Operation::AddState { shape } => {
            block.add_state((**shape).clone());
            Ok(())
        }
        Operation::Test { place, value } => match place {
            Place::Property { key, state } => {
                let (declaration, wanted) =
                    property_target(block, key, *state, value, schema, path)?;
                let actual = block.property_value(key, *state, declaration.default);
                if actual == wanted {
                    Ok(())
                } else {
                    Err(other(
                        path,
                        "",
                        format!(
                            "the test failed: {key} is {}, not {}",
                            show(declaration, actual),
                            show(declaration, wanted)
                        ),
                    ))
                }
            }
            Place::Color => match value {
                PatchValue::Color(wanted) if block.color == *wanted => Ok(()),
                PatchValue::Color(wanted) => Err(other(
                    path,
                    "",
                    format!(
                        "the test failed: the colour is {:?}, not {wanted:?}",
                        block.color
                    ),
                )),
                _ => unreachable!("the reader gives a colour for a colour"),
            },
            Place::SeeThrough => match value {
                PatchValue::Flag(wanted) if block.see_through == *wanted => Ok(()),
                PatchValue::Flag(wanted) => Err(other(
                    path,
                    "",
                    format!(
                        "the test failed: see_through is {}, not {wanted}",
                        block.see_through
                    ),
                )),
                _ => unreachable!("the reader gives a flag for a flag"),
            },
            Place::StateColors => match value {
                PatchValue::Colors(wanted) if block.state_colors() == wanted.as_slice() => Ok(()),
                PatchValue::Colors(wanted) => Err(other(
                    path,
                    "",
                    format!(
                        "the test failed: the state colours are {:?}, not {wanted:?}",
                        block.state_colors()
                    ),
                )),
                _ => unreachable!("the reader gives colours for colours"),
            },
        },
        Operation::Set { place, value } => match place {
            Place::Property { key, state } => {
                let (_, number) = property_target(block, key, *state, value, schema, path)?;
                block.set_property_value(key.clone(), *state, number);
                Ok(())
            }
            Place::Color => {
                if let PatchValue::Color(color) = value {
                    block.color = *color;
                }
                Ok(())
            }
            Place::SeeThrough => {
                if let PatchValue::Flag(flag) = value {
                    block.see_through = *flag;
                }
                Ok(())
            }
            Place::StateColors => {
                if let PatchValue::Colors(colors) = value {
                    block.set_state_colors(colors.clone());
                }
                Ok(())
            }
        },
        Operation::Remove { place } => match place {
            Place::Property { key, state } => {
                if schema.get(key).is_none() {
                    return Err(Failure::Undeclared(key.to_string()));
                }
                check_state(block, *state, path)?;
                if block.remove_property_value(key, *state) {
                    Ok(())
                } else {
                    Err(other(
                        path,
                        "",
                        format!("{key} is not set on this block, so there is nothing to remove"),
                    ))
                }
            }
            Place::StateColors => {
                if block.state_colors().is_empty() {
                    return Err(other(
                        path,
                        "",
                        "this block has no state colours, so there is nothing to remove",
                    ));
                }
                block.set_state_colors(Vec::new());
                Ok(())
            }
            Place::Color | Place::SeeThrough => {
                unreachable!("the reader refuses to remove what a block always has")
            }
        },
    }
}

/// The declaration of a property and the number `value` stands for, after checking that the property exists, that
/// the state exists, and that the value fits.
fn property_target<'a>(
    block: &BlockDefinition,
    key: &NamespacedId,
    state: Option<u32>,
    value: &PatchValue,
    schema: &'a PropertySchema,
    path: &str,
) -> Result<(&'a PropertyDeclaration, i64), Failure> {
    let declaration = schema
        .get(key)
        .ok_or_else(|| Failure::Undeclared(key.to_string()))?;
    check_state(block, state, path)?;
    let PatchValue::Property(value) = value else {
        unreachable!("the reader gives a property value for a property");
    };
    let value_path = "/value";
    let number = match (declaration.kind, value) {
        (PropertyKind::Bool, PropertyValue::Bool(flag)) => i64::from(*flag),
        (PropertyKind::Bool, PropertyValue::Integer(_)) => {
            return Err(other(
                path,
                value_path,
                "expected true or false, found a number",
            ));
        }
        (PropertyKind::Integer { .. }, PropertyValue::Bool(_)) => {
            return Err(other(
                path,
                value_path,
                "expected a whole number, found true or false",
            ));
        }
        (PropertyKind::Integer { minimum, maximum }, PropertyValue::Integer(number)) => {
            if !(minimum..=maximum).contains(number) {
                return Err(other(
                    path,
                    value_path,
                    format!("{number} is outside {minimum} to {maximum}"),
                ));
            }
            *number
        }
    };
    Ok((declaration, number))
}

fn check_state(block: &BlockDefinition, state: Option<u32>, path: &str) -> Result<(), Failure> {
    match state {
        Some(state) if state as usize >= block.state_count() => Err(other(
            path,
            "/state",
            format!(
                "this block has {} states (0 to {}), not a state {state}",
                block.state_count(),
                block.state_count() - 1
            ),
        )),
        _ => Ok(()),
    }
}

/// A property's value as the file would write it.
fn show(declaration: &PropertyDeclaration, value: i64) -> String {
    match declaration.kind {
        PropertyKind::Bool => (value != 0).to_string(),
        PropertyKind::Integer { .. } => value.to_string(),
    }
}
