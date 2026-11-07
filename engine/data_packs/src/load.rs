//! Putting packs together: reading every file, ordering packs by what they depend on, adding their blocks, and
//! applying their patches. Every problem is collected, and a patch that cannot be applied changes nothing.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use block_models::{BlockDefinition, PropertyDeclaration, PropertyKind, PropertySchema, engine};
use namespaced_id::NamespacedId;

use crate::block::read_block;
use crate::body::{Body, BodyContext, read_body};
use crate::error::{PackError, PackErrors};
use crate::manifest::{PackManifest, read_manifest};
use crate::patch::{Operation, Patch, PatchValue, Place, PropertyValue, read_patch};
use crate::pictures::{Available, GameTexture, Textures, settle};
use crate::walk::{nearest, quoted};

/// A file of a pack, read but not yet understood: its name (without `.json`) and its text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawFile {
    pub name: String,
    pub text: String,
}

/// A picture file of a pack, read but not yet understood: its name in the `textures` folder without `.png` (`oak_top`, or `base_blocks/log_top` for a file in
/// a folder named for a namespace) and its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawTexture {
    pub name: String,
    pub bytes: Vec<u8>,
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
    /// The files of `bodies/`.
    pub bodies: Vec<RawFile>,
    /// The picture files of `textures/`.
    pub textures: Vec<RawTexture>,
}

/// Every block there is after the packs are in, and what was left undone on purpose.
#[derive(Clone, Debug, PartialEq)]
pub struct Loaded {
    /// The blocks the game had (patched where a pack said so), in their order, then the blocks the packs
    /// added, in the order the packs were applied.
    pub blocks: Vec<(NamespacedId, BlockDefinition)>,
    /// The bodies the packs added (the game's own are not here), in the order the packs were applied and by file name in each.
    pub bodies: Vec<(NamespacedId, Body)>,
    /// The pictures the blocks use (the game's blocks and the packs'), once every pack has put in and replaced pictures.
    pub textures: Textures,
    /// What was skipped because a patch said it was optional, and what is worth knowing about the packs (a picture that replaces another, one that no
    /// block uses), one line each. Show them to whoever runs the server.
    pub warnings: Vec<String>,
}

/// A pack read and understood, with the names of its files for messages.
struct ReadPack {
    manifest: PackManifest,
    manifest_file: String,
    blocks: Vec<(String, NamespacedId, BlockDefinition)>,
    patches: Vec<(String, Patch)>,
    bodies: Vec<(String, NamespacedId, Body)>,
}

/// Loads `packs` on top of `base`, the blocks the game has, and `game_textures`, the pictures it has. `schema` says which properties exist, and `bodies`
/// what the packs' bodies may name (the families of generators the game has and the levels of its worlds).
///
/// The files of a pack are applied in order of their names and the packs in dependency order, ties by the pack's
/// id, so the result does not depend on the order `packs` is given in.
pub fn load_packs(
    packs: &[RawPack],
    base: &[(NamespacedId, BlockDefinition)],
    game_textures: &[GameTexture],
    schema: &PropertySchema,
    bodies: &BodyContext<'_>,
) -> Result<Loaded, PackErrors> {
    let mut errors = Vec::new();
    let read: Vec<ReadPack> = packs
        .iter()
        .filter_map(|pack| read_pack(pack, schema, bodies, &mut errors))
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
    let mut available = Available::of_the_game(game_textures, &mut errors);
    let mut labels: HashMap<NamespacedId, String> = HashMap::new();
    let mut loaded_bodies = Vec::new();
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
        if let Some(raw) = packs.iter().find(|raw| raw.folder == pack.manifest.id) {
            available.add_pack(raw, &allowed, &mut warnings, &mut errors);
        }

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
            if let Some(texture) = definition.texture()
                && !allowed.contains(texture.namespace())
            {
                errors.push(PackError {
                    file: file.clone(),
                    path: "/texture".to_owned(),
                    message: format!(
                        "\"{texture}\" is a picture of \"{}\", which this pack does not depend on (add it to \"depends_on\" in pack.json)",
                        texture.namespace()
                    ),
                });
            }
            labels.insert(identifier.clone(), file.clone());
            index.insert(identifier.clone(), working.len());
            working.push((identifier.clone(), definition.clone()));
        }

        let mut bodies: Vec<&(String, NamespacedId, Body)> = pack.bodies.iter().collect();
        bodies.sort_by(|first, second| first.0.cmp(&second.0));
        for (_, identifier, body) in bodies {
            loaded_bodies.push((identifier.clone(), body.clone()));
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
    let textures = settle(
        &mut working,
        &available,
        &labels,
        &mut warnings,
        &mut errors,
    );
    if !errors.is_empty() {
        return Err(PackErrors(errors));
    }
    Ok(Loaded {
        blocks: working,
        bodies: loaded_bodies,
        textures,
        warnings,
    })
}

fn read_pack(
    pack: &RawPack,
    schema: &PropertySchema,
    bodies_context: &BodyContext<'_>,
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
    let mut bodies = Vec::new();
    for file in &pack.bodies {
        let name = format!("{}/bodies/{}.json", pack.folder, file.name);
        match read_body(&name, &manifest.id, &file.name, &file.text, bodies_context) {
            Ok((identifier, body)) => bodies.push((name, identifier, body)),
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
        bodies,
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
        if let Operation::Set {
            place: Place::Texture,
            value: PatchValue::Texture(texture),
        } = operation
            && !allowed.contains(texture.namespace())
        {
            errors.push(error(
                file,
                format!("{path}/value"),
                format!(
                    "\"{texture}\" is a picture of \"{}\", which this pack does not depend on (add it to \"depends_on\" in pack.json)",
                    texture.namespace()
                ),
            ));
            continue;
        }
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
            Place::Texture => match value {
                PatchValue::Texture(wanted) if block.texture() == Some(wanted) => Ok(()),
                PatchValue::Texture(wanted) => Err(other(
                    path,
                    "",
                    format!(
                        "the test failed: the texture is {}, not {wanted}",
                        block
                            .texture()
                            .map_or_else(|| "none".to_owned(), ToString::to_string)
                    ),
                )),
                _ => unreachable!("the reader gives a texture for a texture"),
            },
            Place::FaceColors => match value {
                PatchValue::FaceColors(wanted) if block.face_colors() == wanted => Ok(()),
                PatchValue::FaceColors(wanted) => Err(other(
                    path,
                    "",
                    format!(
                        "the test failed: the face colours are {:?}, not {wanted:?}",
                        block.face_colors()
                    ),
                )),
                _ => unreachable!("the reader gives face colours for face colours"),
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
                    block.set_color(*color);
                }
                Ok(())
            }
            Place::Texture => {
                if let PatchValue::Texture(texture) = value {
                    block.set_texture(Some(texture.clone()));
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
            Place::FaceColors => {
                if let PatchValue::FaceColors(colors) = value {
                    block.set_face_colors(*colors);
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
            Place::Texture => {
                if block.texture().is_none() {
                    return Err(other(
                        path,
                        "",
                        "this block has no texture, so there is nothing to remove",
                    ));
                }
                if block.color_from_pictures() {
                    return Err(other(
                        path,
                        "",
                        "the colour of this block is the average of its pictures, so give it a color (set \"color\") before taking its texture away",
                    ));
                }
                block.set_texture(None);
                Ok(())
            }
            Place::FaceColors => {
                if block.face_colors().iter().all(Option::is_none) {
                    return Err(other(
                        path,
                        "",
                        "this block has no face colours, so there is nothing to remove",
                    ));
                }
                block.set_face_colors([None; 6]);
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
        (PropertyKind::Bool, PropertyValue::Word(_)) => {
            return Err(other(
                path,
                value_path,
                "expected true or false, found text",
            ));
        }
        (PropertyKind::Integer { .. }, PropertyValue::Bool(_)) => {
            return Err(other(
                path,
                value_path,
                "expected a whole number, found true or false",
            ));
        }
        (PropertyKind::Integer { .. }, PropertyValue::Word(_)) => {
            return Err(other(
                path,
                value_path,
                "expected a whole number, found text",
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
        (PropertyKind::Face, PropertyValue::Integer(number)) => {
            if !(0..=6).contains(number) {
                return Err(other(
                    path,
                    value_path,
                    format!("{number} is outside 0 to 6"),
                ));
            }
            *number
        }
        (PropertyKind::Face, PropertyValue::Word(word)) => {
            match engine::face_code_from_word(word) {
                Some(code) => i64::from(code),
                None => return Err(other(path, value_path, crate::block::no_such_face(word))),
            }
        }
        (PropertyKind::Face, PropertyValue::Bool(_)) => {
            return Err(other(
                path,
                value_path,
                format!("{}, found true or false", crate::block::expected_face()),
            ));
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
        PropertyKind::Face => usize::try_from(value)
            .ok()
            .and_then(|code| engine::FACE_WORDS.get(code))
            .map_or_else(|| value.to_string(), |word| (*word).to_owned()),
    }
}
