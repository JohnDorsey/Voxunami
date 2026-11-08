//! The pictures of blocks: which file is the picture of each face, the set of pictures there are (the game's and each pack's, a later pack replacing an earlier
//! one's by file name), and the settling of every block's pictures and of the colour that follows them (`docs/specs/block-textures-spec.md`, sections 4 and 5).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use block_models::{BlockDefinition, FacePictures, TileRef, engine};
pub use block_models::{MOST_TILE_BYTES, MOST_TILE_SIZES, MOST_TILES_OF_A_SIZE};
use namespaced_id::NamespacedId;

use crate::error::{PackError, PackErrors};
use crate::load::RawPack;
use crate::tile::{Tile, average_color};

/// How far, as a distance between sRGB colours (each from 0 to 1), a colour a block chose may be from the average of its pictures before the loader says so.
const FAR_FROM_THE_AVERAGE: f32 = 0.35;

/// A picture file of the game's own (not of a pack): the id it has (such as `base_blocks:log_top`), the file's name for messages, and the bytes of the PNG.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameTexture {
    pub id: NamespacedId,
    pub file: String,
    pub bytes: Vec<u8>,
}

/// The picture of each face of a block: the ids of tiles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pictures {
    pub top: NamespacedId,
    pub side: NamespacedId,
    pub bottom: NamespacedId,
}

/// The pictures blocks use, once everything is settled: each block's pictures, the tiles they are (only those some block uses), and where each tile is among the tiles
/// of a world: the sizes in use smallest first, and within a size by name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Textures {
    pub tiles: BTreeMap<NamespacedId, Tile>,
    pub pictures: BTreeMap<NamespacedId, Pictures>,
    pub places: BTreeMap<NamespacedId, TileRef>,
}

impl Textures {
    /// The tiles by size, smallest first, each size's tiles in the order of their places: what a client puts on the GPU.
    pub fn classes(&self) -> Vec<(u32, Vec<&Tile>)> {
        let mut classes: Vec<(u32, Vec<(TileRef, &Tile)>)> = Vec::new();
        for (id, tile) in &self.tiles {
            let place = self.places[id];
            while classes.len() <= place.class() {
                classes.push((0, Vec::new()));
            }
            classes[place.class()].0 = tile.size();
            classes[place.class()].1.push((place, tile));
        }
        classes
            .into_iter()
            .map(|(size, mut tiles)| {
                tiles.sort_by_key(|(place, _)| *place);
                (size, tiles.into_iter().map(|(_, tile)| tile).collect())
            })
            .collect()
    }
}

fn problem(file: impl Into<String>, path: &str, message: impl Into<String>) -> PackError {
    PackError {
        file: file.into(),
        path: path.to_owned(),
        message: message.into(),
    }
}

/// `{namespace}:{name}{suffix}`, if that is a name a picture can have.
fn with_suffix(base: &NamespacedId, suffix: &str) -> Option<NamespacedId> {
    NamespacedId::new(base.namespace(), &format!("{}{suffix}", base.name())).ok()
}

/// The pictures of a block whose base name is `base`, from the pictures there are (`has` says whether one is): for the top, `base_top` and then `base`;
/// for the sides, `base_side`, then `base`, then the top's; for the bottom, `base_bottom` and then the top's. `None` if the top has no picture.
pub fn resolve_pictures(
    base: &NamespacedId,
    has: impl Fn(&NamespacedId) -> bool,
) -> Option<Pictures> {
    let find = |suffix: &str| with_suffix(base, suffix).filter(|candidate| has(candidate));
    let top = find("_top").or_else(|| has(base).then(|| base.clone()))?;
    let side = find("_side")
        .or_else(|| has(base).then(|| base.clone()))
        .unwrap_or_else(|| top.clone());
    let bottom = find("_bottom").unwrap_or_else(|| top.clone());
    Some(Pictures { top, side, bottom })
}

/// Every picture there is, each with the file it came from and whether a pack (and not the game) provided it.
#[derive(Debug, Default)]
pub(crate) struct Available {
    tiles: BTreeMap<NamespacedId, Entry>,
}

#[derive(Debug)]
struct Entry {
    tile: Tile,
    file: String,
    from_a_pack: bool,
}

impl Available {
    /// The game's own pictures. Every problem is added to `errors`.
    pub(crate) fn of_the_game(textures: &[GameTexture], errors: &mut Vec<PackError>) -> Self {
        let mut available = Self::default();
        for texture in textures {
            match Tile::from_png(&texture.file, &texture.bytes) {
                Ok(tile) => {
                    if let Some(first) = available.tiles.get(&texture.id) {
                        errors.push(problem(
                            &texture.file,
                            "",
                            format!(
                                "the picture {} already exists (from {})",
                                texture.id, first.file
                            ),
                        ));
                    } else {
                        available.tiles.insert(
                            texture.id.clone(),
                            Entry {
                                tile,
                                file: texture.file.clone(),
                                from_a_pack: false,
                            },
                        );
                    }
                }
                Err(error) => errors.push(error),
            }
        }
        available
    }

    /// The pictures of a pack, which may replace pictures of the namespaces in `allowed` (its own and those it depends on). A file in the folder of the
    /// pack is a picture of the pack's namespace (`oak_top.png` is `my_pack:oak_top`); a file in a folder named for a namespace is a picture of that namespace
    /// (`base_blocks/log_top.png` is `base_blocks:log_top`), and replaces one that is there.
    pub(crate) fn add_pack(
        &mut self,
        pack: &RawPack,
        allowed: &BTreeSet<String>,
        warnings: &mut Vec<String>,
        errors: &mut Vec<PackError>,
    ) {
        let mut textures: Vec<&crate::load::RawTexture> = pack.textures.iter().collect();
        textures.sort_by(|first, second| first.name.cmp(&second.name));
        for texture in textures {
            let file = format!("{}/textures/{}.png", pack.folder, texture.name);
            let (namespace, name) = match texture.name.split_once('/') {
                Some((namespace, name)) => (namespace, name),
                None => (pack.folder.as_str(), texture.name.as_str()),
            };
            let id = match NamespacedId::new(namespace, name) {
                Ok(id) => id,
                Err(error) => {
                    errors.push(problem(
                        &file,
                        "",
                        format!(
                            "the file name \"{}\" is not a picture's name: {error}",
                            texture.name
                        ),
                    ));
                    continue;
                }
            };
            let replacing = namespace != pack.folder;
            if replacing && !allowed.contains(namespace) {
                errors.push(problem(
                    &file,
                    "",
                    format!(
                        "pictures of \"{namespace}\" can be put in by a pack that depends on it (add it to \"depends_on\" in pack.json)"
                    ),
                ));
                continue;
            }
            let tile = match Tile::from_png(&file, &texture.bytes) {
                Ok(tile) => tile,
                Err(error) => {
                    errors.push(error);
                    continue;
                }
            };
            match self.tiles.get(&id) {
                Some(first) if !replacing => errors.push(problem(
                    &file,
                    "",
                    format!("the picture {id} already exists (from {})", first.file),
                )),
                Some(first) => {
                    warnings.push(format!("{file}: replaces {id} from {}", first.file));
                    self.insert(id, tile, file);
                }
                None => self.insert(id, tile, file),
            }
        }
    }

    fn insert(&mut self, id: NamespacedId, tile: Tile, file: String) {
        self.tiles.insert(
            id,
            Entry {
                tile,
                file,
                from_a_pack: true,
            },
        );
    }

    fn contains(&self, id: &NamespacedId) -> bool {
        self.tiles.contains_key(id)
    }
}

/// How far two sRGB colours are from each other.
fn distance(first: [f32; 3], second: [f32; 3]) -> f32 {
    first
        .iter()
        .zip(second)
        .map(|(first, second)| (first - second) * (first - second))
        .sum::<f32>()
        .sqrt()
}

/// Settles the pictures of every block that has a texture: finds the picture of each face, and works out the colour of the blocks whose colour follows their
/// pictures. `labels` gives the file of each block that a pack wrote; the others are the game's own, which get no warnings. Problems go to `errors`, and
/// things worth saying to `warnings`.
pub(crate) fn settle(
    blocks: &mut [(NamespacedId, BlockDefinition)],
    available: &Available,
    labels: &HashMap<NamespacedId, String>,
    warnings: &mut Vec<String>,
    errors: &mut Vec<PackError>,
) -> Textures {
    let mut settled = Textures::default();
    let mut used: BTreeSet<NamespacedId> = BTreeSet::new();
    for (identifier, definition) in blocks.iter_mut() {
        let Some(texture) = definition.texture().cloned() else {
            continue;
        };
        let label = labels
            .get(identifier)
            .cloned()
            .unwrap_or_else(|| format!("block {identifier}"));
        let from_a_pack = labels.contains_key(identifier);
        let Some(pictures) = resolve_pictures(&texture, |candidate| available.contains(candidate))
        else {
            errors.push(problem(
                &label,
                "/texture",
                format!(
                    "\"{texture}\" names no picture: there is no {texture}_top and no {texture}"
                ),
            ));
            continue;
        };
        let tile_of = |id: &NamespacedId| &available.tiles[id].tile;
        let average = average_color(&[(tile_of(&pictures.top), 1), (tile_of(&pictures.side), 4)]);
        if definition.color_from_pictures() {
            match average {
                Some(color) => definition.set_color_from_pictures(color),
                None => {
                    errors.push(problem(
                        &label,
                        "/texture",
                        format!(
                            "the pictures of \"{texture}\" are completely transparent, so the colour cannot follow them (give a \"color\")"
                        ),
                    ));
                    continue;
                }
            }
        } else if from_a_pack
            && let Some(average) = average
            && distance(definition.color, average) > FAR_FROM_THE_AVERAGE
        {
            warnings.push(format!(
                "{label}: its color is {:.2} from the average of its pictures; far views and the hotbar use the color, so this is only to say so",
                distance(definition.color, average)
            ));
        }
        if from_a_pack {
            let turned = |key: &str| {
                key.parse::<NamespacedId>()
                    .map_or(0, |key| definition.property_value(&key, None, 0))
            };
            let never_turned =
                turned(engine::AXIS_FACE.id()) == 0 && turned(engine::FRONT_FACE.id()) == 0;
            if never_turned && (pictures.top != pictures.side || pictures.top != pictures.bottom) {
                warnings.push(format!(
                    "{label}: its top picture differs from its other faces, but nothing turns the block (no engine:axis_face or engine:front_face), so its top is always up"
                ));
            }
        }
        for id in [&pictures.top, &pictures.side, &pictures.bottom] {
            used.insert(id.clone());
            settled
                .tiles
                .entry(id.clone())
                .or_insert_with(|| available.tiles[id].tile.clone());
        }
        settled.pictures.insert(identifier.clone(), pictures);
    }
    let before = errors.len();
    check_limits(&used, available, errors);
    if errors.len() == before {
        settled.places = places_of(&settled.tiles);
        for (identifier, definition) in blocks.iter_mut() {
            if let Some(pictures) = settled.pictures.get(identifier) {
                let place = |id: &NamespacedId| settled.places[id];
                definition.set_pictures(Some(FacePictures {
                    top: place(&pictures.top),
                    side: place(&pictures.side),
                    bottom: place(&pictures.bottom),
                }));
            }
        }
    }
    for (id, entry) in &available.tiles {
        if entry.from_a_pack && !used.contains(id) {
            warnings.push(format!("{}: no block uses this picture", entry.file));
        }
    }
    settled
}

/// Where each of the tiles is: its size's number (the sizes in use, smallest first), and its number among the tiles of that size (by name). The limits are kept.
fn places_of(tiles: &BTreeMap<NamespacedId, Tile>) -> BTreeMap<NamespacedId, TileRef> {
    let sizes: BTreeSet<u32> = tiles.values().map(Tile::size).collect();
    let mut next: BTreeMap<u32, usize> = BTreeMap::new();
    tiles
        .iter()
        .map(|(id, tile)| {
            let class = sizes
                .iter()
                .position(|size| *size == tile.size())
                .expect("the size is among the sizes");
            let layer = next.entry(tile.size()).or_insert(0);
            let place = TileRef::new(class, *layer).expect("the limits were kept");
            *layer += 1;
            (id.clone(), place)
        })
        .collect()
}

/// At most [`MOST_TILE_SIZES`] sizes among the pictures that are used, and [`MOST_TILES_OF_A_SIZE`] pictures of a size.
fn check_limits(used: &BTreeSet<NamespacedId>, available: &Available, errors: &mut Vec<PackError>) {
    let mut by_size: BTreeMap<u32, Vec<&NamespacedId>> = BTreeMap::new();
    for id in used {
        by_size
            .entry(available.tiles[id].tile.size())
            .or_default()
            .push(id);
    }
    if by_size.len() > MOST_TILE_SIZES {
        let sizes: Vec<String> = by_size.keys().map(u32::to_string).collect();
        for (size, ids) in by_size.iter().skip(MOST_TILE_SIZES) {
            for id in ids {
                errors.push(problem(
                    &available.tiles[*id].file,
                    "",
                    format!(
                        "the pictures that blocks use are of {} sizes ({}) and at most {MOST_TILE_SIZES} are allowed at once; this one is of {size} pixels",
                        by_size.len(),
                        sizes.join(", ")
                    ),
                ));
            }
        }
    }
    let bytes: usize = used
        .iter()
        .map(|id| available.tiles[id].tile.rgba().len())
        .sum();
    if bytes > MOST_TILE_BYTES {
        let biggest = used
            .iter()
            .max_by_key(|id| available.tiles[*id].tile.size())
            .expect("there are pictures if they take bytes");
        errors.push(problem(
            &available.tiles[biggest].file,
            "",
            format!(
                "the pictures that blocks use take {bytes} bytes as pixels and a client is sent at most {MOST_TILE_BYTES}; this is the largest"
            ),
        ));
    }
    for (size, ids) in &by_size {
        if ids.len() > MOST_TILES_OF_A_SIZE {
            errors.push(problem(
                &available.tiles[ids[MOST_TILES_OF_A_SIZE]].file,
                "",
                format!(
                    "{} pictures of {size} pixels are used and at most {MOST_TILES_OF_A_SIZE} of a size are allowed",
                    ids.len()
                ),
            ));
        }
    }
}

/// Settles the pictures of the game's own blocks, from the game's own pictures: the colours that follow pictures are worked out, and the tiles every face of
/// every block with a texture has are returned.
pub fn settle_game_pictures(
    blocks: &mut [(NamespacedId, BlockDefinition)],
    textures: &[GameTexture],
) -> Result<Textures, PackErrors> {
    let mut errors = Vec::new();
    let available = Available::of_the_game(textures, &mut errors);
    let settled = settle(
        blocks,
        &available,
        &HashMap::new(),
        &mut Vec::new(),
        &mut errors,
    );
    if errors.is_empty() {
        Ok(settled)
    } else {
        Err(PackErrors(errors))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::load::RawTexture;

    fn id(text: &str) -> NamespacedId {
        text.parse().unwrap()
    }

    // ---- which file is the picture of which face ----

    /// The pictures `base` gets when `present` of its files exist, written out by hand for every one of the 16 combinations of
    /// (plain, top, side, bottom): the table of the spec, and not the code that implements it.
    #[test]
    fn the_picture_of_each_face_is_the_first_file_that_exists_in_the_order_of_the_spec() {
        // (plain, top, side, bottom) present -> the files of (top, side, bottom), or None for a block with no pictures.
        let rows: [([bool; 4], Option<[&str; 3]>); 16] = [
            ([false, false, false, false], None),
            ([true, false, false, false], Some(["", "", ""])),
            ([false, true, false, false], Some(["_top", "_top", "_top"])),
            ([true, true, false, false], Some(["_top", "", "_top"])),
            ([false, false, true, false], None),
            ([true, false, true, false], Some(["", "_side", ""])),
            ([false, true, true, false], Some(["_top", "_side", "_top"])),
            ([true, true, true, false], Some(["_top", "_side", "_top"])),
            ([false, false, false, true], None),
            ([true, false, false, true], Some(["", "", "_bottom"])),
            (
                [false, true, false, true],
                Some(["_top", "_top", "_bottom"]),
            ),
            ([true, true, false, true], Some(["_top", "", "_bottom"])),
            ([false, false, true, true], None),
            ([true, false, true, true], Some(["", "_side", "_bottom"])),
            (
                [false, true, true, true],
                Some(["_top", "_side", "_bottom"]),
            ),
            ([true, true, true, true], Some(["_top", "_side", "_bottom"])),
        ];
        let base = id("my_pack:oak");
        for (present, wanted) in rows {
            let files: BTreeSet<NamespacedId> = ["", "_top", "_side", "_bottom"]
                .iter()
                .zip(present)
                .filter(|(_, present)| *present)
                .map(|(suffix, _)| id(&format!("my_pack:oak{suffix}")))
                .collect();
            let found = resolve_pictures(&base, |candidate| files.contains(candidate));
            let wanted = wanted.map(|[top, side, bottom]| Pictures {
                top: id(&format!("my_pack:oak{top}")),
                side: id(&format!("my_pack:oak{side}")),
                bottom: id(&format!("my_pack:oak{bottom}")),
            });
            assert_eq!(
                found, wanted,
                "present (plain, top, side, bottom): {present:?}"
            );
        }
    }

    #[test]
    fn a_picture_is_found_in_the_namespace_of_the_base_name_and_a_name_too_long_to_add_to_has_no_such_file()
     {
        let elsewhere = id("other:oak_top");
        assert_eq!(
            resolve_pictures(&id("my_pack:oak"), |candidate| *candidate == elsewhere),
            None,
            "a picture of another namespace is not this block's"
        );
        let long = format!("my_pack:{}", "a".repeat(63));
        // (`a...a_top` is 67 characters, which is no name.)
        assert_eq!(resolve_pictures(&id(&long), |_| false), None);
        assert_eq!(
            resolve_pictures(&id(&long), |candidate| candidate.as_str() == long),
            Some(Pictures {
                top: id(&long),
                side: id(&long),
                bottom: id(&long)
            })
        );
    }

    // ---- the pictures there are ----

    fn png_of(size: u32, rgba: [u8; 4]) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, size, size);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let data: Vec<u8> = (0..size * size).flat_map(|_| rgba).collect();
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&data).unwrap();
        writer.finish().unwrap();
        bytes
    }

    fn game_texture(id_text: &str, size: u32, rgba: [u8; 4]) -> GameTexture {
        GameTexture {
            id: id(id_text),
            file: format!("game/{id_text}.png"),
            bytes: png_of(size, rgba),
        }
    }

    fn pack_with(folder: &str, textures: &[(&str, u32, [u8; 4])]) -> RawPack {
        RawPack {
            folder: folder.to_owned(),
            manifest: String::new(),
            blocks: Vec::new(),
            patches: Vec::new(),
            bodies: Vec::new(),
            textures: textures
                .iter()
                .map(|(name, size, rgba)| RawTexture {
                    name: (*name).to_owned(),
                    bytes: png_of(*size, *rgba),
                })
                .collect(),
        }
    }

    fn allowed(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    struct Added {
        available: Available,
        warnings: Vec<String>,
        errors: Vec<String>,
    }

    fn added(game: &[GameTexture], pack: &RawPack, may_replace: &[&str]) -> Added {
        let mut errors = Vec::new();
        let mut available = Available::of_the_game(game, &mut errors);
        let mut warnings = Vec::new();
        available.add_pack(pack, &allowed(may_replace), &mut warnings, &mut errors);
        Added {
            available,
            warnings,
            errors: errors.iter().map(ToString::to_string).collect(),
        }
    }

    #[test]
    fn a_pack_puts_its_pictures_in_its_own_namespace_by_file_name() {
        let pack = pack_with(
            "my_pack",
            &[("oak_top", 16, [1, 2, 3, 255]), ("oak", 16, [4, 5, 6, 255])],
        );
        let result = added(&[], &pack, &["my_pack"]);
        assert!(result.errors.is_empty() && result.warnings.is_empty());
        let ids: Vec<&str> = result
            .available
            .tiles
            .keys()
            .map(NamespacedId::as_str)
            .collect();
        assert_eq!(ids, ["my_pack:oak", "my_pack:oak_top"]);
    }

    #[test]
    fn a_pack_replaces_a_picture_of_a_namespace_it_depends_on_by_a_file_of_that_name_in_a_folder_of_that_namespace_and_says_so()
     {
        let game = [game_texture(
            "base_blocks:stone_top",
            16,
            [100, 100, 100, 255],
        )];
        let pack = pack_with(
            "retexture",
            &[
                ("base_blocks/stone_top", 16, [200, 0, 0, 255]),
                ("base_blocks/stone_side", 16, [0, 200, 0, 255]),
            ],
        );
        let result = added(&game, &pack, &["retexture", "base_blocks"]);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        // One replaced a picture of the game (said), and the other is a new picture of its namespace (not said).
        assert_eq!(
            result.warnings,
            [
                "retexture/textures/base_blocks/stone_top.png: replaces base_blocks:stone_top from game/base_blocks:stone_top.png"
            ]
        );
        let top = &result.available.tiles[&id("base_blocks:stone_top")];
        assert_eq!(&top.tile.rgba()[0..4], [200, 0, 0, 255]);
        assert_eq!(top.file, "retexture/textures/base_blocks/stone_top.png");
        assert!(result.available.contains(&id("base_blocks:stone_side")));
    }

    #[test]
    fn a_pack_may_not_put_pictures_in_a_namespace_it_does_not_depend_on() {
        let pack = pack_with("retexture", &[("elsewhere/stone_top", 16, [1, 1, 1, 255])]);
        let result = added(&[], &pack, &["retexture"]);
        assert_eq!(
            result.errors,
            [
                "retexture/textures/elsewhere/stone_top.png: pictures of \"elsewhere\" can be put in by a pack that depends on it (add it to \"depends_on\" in pack.json)"
            ]
        );
        assert!(result.available.tiles.is_empty());
    }

    #[test]
    fn a_picture_that_is_a_name_of_its_own_pack_and_already_exists_is_refused_not_replaced() {
        // (A game with a plugin of the pack's own name: the pack may not take a picture of that plugin without saying so.)
        let game = [game_texture("my_pack:oak", 16, [1, 1, 1, 255])];
        let pack = pack_with("my_pack", &[("oak", 16, [2, 2, 2, 255])]);
        let result = added(&game, &pack, &["my_pack"]);
        assert_eq!(
            result.errors,
            [
                "my_pack/textures/oak.png: the picture my_pack:oak already exists (from game/my_pack:oak.png)"
            ]
        );
        assert_eq!(
            &result.available.tiles[&id("my_pack:oak")].tile.rgba()[0..4],
            [1, 1, 1, 255]
        );
    }

    #[test]
    fn a_file_that_is_no_picture_name_or_no_picture_is_refused_and_does_not_stop_the_others() {
        let mut pack = pack_with(
            "my_pack",
            &[
                ("Oak Top", 16, [1, 1, 1, 255]),
                ("fine", 16, [1, 1, 1, 255]),
            ],
        );
        pack.textures.push(RawTexture {
            name: "broken".to_owned(),
            bytes: b"not a picture".to_vec(),
        });
        let result = added(&[], &pack, &["my_pack"]);
        assert_eq!(result.errors.len(), 2, "{:?}", result.errors);
        assert!(result.errors[0].starts_with(
            "my_pack/textures/Oak Top.png: the file name \"Oak Top\" is not a picture's name: "
        ));
        assert!(
            result.errors[1]
                .starts_with("my_pack/textures/broken.png: this is not a PNG picture: ")
        );
        assert!(result.available.contains(&id("my_pack:fine")));
    }

    // ---- settling the pictures of blocks ----

    fn block(color: [f32; 3], texture: Option<&str>, follows: bool) -> BlockDefinition {
        let mut definition = BlockDefinition::cube(color);
        if let Some(texture) = texture {
            definition = definition.with_texture(id(texture));
            if follows {
                definition = definition.with_color_from_pictures();
            }
        }
        definition
    }

    struct Settled {
        blocks: Vec<(NamespacedId, BlockDefinition)>,
        textures: Textures,
        warnings: Vec<String>,
        errors: Vec<String>,
    }

    fn settled(
        blocks: Vec<(NamespacedId, BlockDefinition)>,
        game: &[GameTexture],
        packs: &[RawPack],
        labelled: &[&str],
    ) -> Settled {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let mut available = Available::of_the_game(game, &mut errors);
        for pack in packs {
            let names = [pack.folder.as_str()];
            available.add_pack(pack, &allowed(&names), &mut warnings, &mut errors);
        }
        let labels: HashMap<NamespacedId, String> = labelled
            .iter()
            .map(|name| (id(name), format!("file of {name}")))
            .collect();
        let mut blocks = blocks;
        let textures = settle(&mut blocks, &available, &labels, &mut warnings, &mut errors);
        Settled {
            blocks,
            textures,
            warnings,
            errors: errors.iter().map(ToString::to_string).collect(),
        }
    }

    fn close(found: [f32; 3], wanted: [f32; 3]) {
        for (found, wanted) in found.into_iter().zip(wanted) {
            assert!((found - wanted).abs() < 1e-4, "{found:?} is not {wanted:?}");
        }
    }

    #[test]
    fn the_colour_that_follows_the_pictures_is_the_top_once_and_each_of_the_four_sides_once_and_the_bottom_not_at_all()
     {
        let game = [
            game_texture("g:log_top", 16, [255, 255, 255, 255]),
            game_texture("g:log_side", 16, [0, 0, 0, 255]),
            game_texture("g:log_bottom", 16, [255, 0, 0, 255]),
        ];
        let result = settled(
            vec![(id("g:log"), block([0.0; 3], Some("g:log"), true))],
            &game,
            &[],
            &[],
        );
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        // One white, four black, and the red bottom not counted: a fifth of the light.
        close(result.blocks[0].1.color, [0.4845; 3]);
        assert!(
            result.blocks[0].1.color_from_pictures(),
            "it goes on following"
        );
        let pictures = &result.textures.pictures[&id("g:log")];
        assert_eq!(
            (
                pictures.top.as_str(),
                pictures.side.as_str(),
                pictures.bottom.as_str()
            ),
            ("g:log_top", "g:log_side", "g:log_bottom")
        );
        assert_eq!(result.textures.tiles.len(), 3);
    }

    #[test]
    fn a_colour_the_author_gave_is_not_touched_and_a_block_without_a_texture_is_not_looked_at() {
        let game = [game_texture("g:log", 16, [255, 255, 255, 255])];
        let result = settled(
            vec![
                (id("g:log"), block([0.1, 0.2, 0.3], Some("g:log"), false)),
                (id("g:plain"), block([0.4, 0.5, 0.6], None, false)),
            ],
            &game,
            &[],
            &[],
        );
        assert!(result.errors.is_empty());
        assert_eq!(result.blocks[0].1.color, [0.1, 0.2, 0.3]);
        assert_eq!(result.blocks[1].1.color, [0.4, 0.5, 0.6]);
        assert!(!result.textures.pictures.contains_key(&id("g:plain")));
    }

    #[test]
    fn a_texture_with_no_file_is_an_error_that_names_the_block_and_what_was_looked_for() {
        let result = settled(
            vec![
                (
                    id("my_pack:oak"),
                    block([0.0; 3], Some("my_pack:oak"), true),
                ),
                (id("base:lamp"), block([0.0; 3], Some("base:lamp"), false)),
            ],
            &[],
            &[],
            &["my_pack:oak"],
        );
        assert_eq!(
            result.errors,
            [
                "file of my_pack:oak: /texture: \"my_pack:oak\" names no picture: there is no my_pack:oak_top and no my_pack:oak",
                "block base:lamp: /texture: \"base:lamp\" names no picture: there is no base:lamp_top and no base:lamp",
            ]
        );
    }

    #[test]
    fn a_colour_cannot_follow_pictures_that_are_completely_transparent() {
        let game = [game_texture("g:ghost", 16, [255, 255, 255, 0])];
        let result = settled(
            vec![(id("g:ghost"), block([0.0; 3], Some("g:ghost"), true))],
            &game,
            &[],
            &[],
        );
        assert_eq!(
            result.errors,
            [
                "block g:ghost: /texture: the pictures of \"g:ghost\" are completely transparent, so the colour cannot follow them (give a \"color\")"
            ]
        );
    }

    #[test]
    fn pictures_are_numbered_by_size_smallest_first_and_within_a_size_by_name_and_every_block_is_told_its_places()
     {
        let game = [
            game_texture("g:zebra", 16, [10, 10, 10, 255]),
            game_texture("g:apple", 16, [20, 20, 20, 255]),
            game_texture("g:big", 64, [30, 30, 30, 255]),
            game_texture("g:tiny", 8, [40, 40, 40, 255]),
        ];
        let mut blocks = vec![
            (id("g:one"), block([0.0; 3], Some("g:zebra"), true)),
            (id("g:two"), block([0.0; 3], Some("g:apple"), true)),
            (id("g:three"), block([0.0; 3], Some("g:big"), false)),
            (id("g:four"), block([0.0; 3], Some("g:tiny"), true)),
            (id("g:none"), block([0.0; 3], None, false)),
        ];
        let textures = settle_game_pictures(&mut blocks, &game).unwrap();
        let place = |text: &str| textures.places[&id(text)];
        // 8, 16, 64: three sizes; the two of 16 by name.
        assert_eq!((place("g:tiny").class(), place("g:tiny").layer()), (0, 0));
        assert_eq!((place("g:apple").class(), place("g:apple").layer()), (1, 0));
        assert_eq!((place("g:zebra").class(), place("g:zebra").layer()), (1, 1));
        assert_eq!((place("g:big").class(), place("g:big").layer()), (2, 0));
        let sizes: Vec<(u32, usize)> = textures
            .classes()
            .iter()
            .map(|(size, tiles)| (*size, tiles.len()))
            .collect();
        assert_eq!(sizes, [(8, 1), (16, 2), (64, 1)]);
        assert_eq!(
            textures.classes()[1].1[1].rgba()[0],
            10,
            "the zebra is second among the 16s"
        );
        // Each block is told the places of its own pictures; a block with no texture has none.
        let told = |name: &str| {
            blocks
                .iter()
                .find(|(identifier, _)| *identifier == id(name))
                .unwrap()
                .1
                .pictures()
                .copied()
        };
        let one = told("g:one").expect("told");
        assert_eq!(
            (one.top, one.side, one.bottom),
            (place("g:zebra"), place("g:zebra"), place("g:zebra"))
        );
        assert_eq!(told("g:three").unwrap().top, place("g:big"));
        assert_eq!(told("g:none"), None);
    }

    #[test]
    fn a_block_told_its_places_keeps_the_top_side_and_bottom_apart() {
        let game = [
            game_texture("g:log_top", 16, [255, 255, 255, 255]),
            game_texture("g:log_side", 16, [0, 0, 0, 255]),
            game_texture("g:log_bottom", 16, [128, 128, 128, 255]),
        ];
        let mut blocks = vec![(id("g:log"), block([0.0; 3], Some("g:log"), true))];
        let textures = settle_game_pictures(&mut blocks, &game).unwrap();
        let pictures = blocks[0].1.pictures().copied().unwrap();
        let tiles = textures.classes();
        let pixel = |place: TileRef| tiles[place.class()].1[place.layer()].rgba()[0];
        assert_eq!(
            (
                pixel(pictures.top),
                pixel(pictures.side),
                pixel(pictures.bottom)
            ),
            (255, 0, 128)
        );
    }

    #[test]
    fn the_pictures_that_are_used_are_the_tiles_and_a_picture_two_blocks_use_is_one_tile() {
        let game = [
            game_texture("g:a", 16, [10, 10, 10, 255]),
            game_texture("g:b", 16, [20, 20, 20, 255]),
            game_texture("g:unused", 16, [30, 30, 30, 255]),
        ];
        let result = settled(
            vec![
                (id("g:one"), block([0.0; 3], Some("g:a"), true)),
                (id("g:two"), block([0.0; 3], Some("g:a"), true)),
                (id("g:three"), block([0.0; 3], Some("g:b"), true)),
            ],
            &game,
            &[],
            &[],
        );
        let tiles: Vec<&str> = result
            .textures
            .tiles
            .keys()
            .map(NamespacedId::as_str)
            .collect();
        assert_eq!(
            tiles,
            ["g:a", "g:b"],
            "the one nobody uses is not among them"
        );
        assert_eq!(result.textures.pictures.len(), 3);
    }

    // ---- limits ----

    #[test]
    fn at_most_four_sizes_may_be_used_and_the_files_of_the_fifth_are_named() {
        let game: Vec<GameTexture> = [8u32, 16, 32, 64, 128]
            .iter()
            .map(|size| game_texture(&format!("g:p{size}"), *size, [1, 2, 3, 255]))
            .collect();
        let blocks: Vec<(NamespacedId, BlockDefinition)> = [8u32, 16, 32, 64, 128]
            .iter()
            .map(|size| {
                (
                    id(&format!("g:b{size}")),
                    block([0.0; 3], Some(&format!("g:p{size}")), true),
                )
            })
            .collect();
        let result = settled(blocks, &game, &[], &[]);
        assert_eq!(
            result.errors,
            [
                "game/g:p128.png: the pictures that blocks use are of 5 sizes (8, 16, 32, 64, 128) and at most 4 are allowed at once; this one is of 128 pixels"
            ]
        );
        // Four sizes are fine, and a size nobody uses does not count.
        let four: Vec<GameTexture> = [8u32, 16, 32, 64, 128]
            .iter()
            .map(|size| game_texture(&format!("g:p{size}"), *size, [1, 2, 3, 255]))
            .collect();
        let blocks: Vec<(NamespacedId, BlockDefinition)> = [8u32, 16, 32, 64]
            .iter()
            .map(|size| {
                (
                    id(&format!("g:b{size}")),
                    block([0.0; 3], Some(&format!("g:p{size}")), true),
                )
            })
            .collect();
        assert!(settled(blocks, &four, &[], &[]).errors.is_empty());
    }

    #[test]
    fn the_pixels_of_all_the_pictures_in_use_may_take_at_most_8_mib() {
        // 129 pictures of 256 pixels are 33 MiB... too many for one size's 256? No: 129 is under 256 of the size, but over the bytes: 129 * 256 KiB.
        let count = 33;
        let game: Vec<GameTexture> = (0..count)
            .map(|index| game_texture(&format!("g:p{index:03}"), 256, [1, 2, 3, 255]))
            .collect();
        let blocks: Vec<(NamespacedId, BlockDefinition)> = (0..count)
            .map(|index| {
                (
                    id(&format!("g:b{index:03}")),
                    block([0.0; 3], Some(&format!("g:p{index:03}")), true),
                )
            })
            .collect();
        let result = settled(blocks.clone(), &game, &[], &[]);
        assert_eq!(result.errors.len(), 1, "{:?}", result.errors);
        assert!(
            result.errors[0].contains("take 8650752 bytes as pixels and a client is sent at most 8388608; this is the largest"),
            "{:?}",
            result.errors
        );
        // 32 of them are exactly 8 MiB, which is allowed.
        assert!(
            settled(blocks[..32].to_vec(), &game, &[], &[])
                .errors
                .is_empty()
        );
    }

    #[test]
    fn at_most_256_pictures_of_a_size_may_be_used() {
        let count = MOST_TILES_OF_A_SIZE + 1;
        let game: Vec<GameTexture> = (0..count)
            .map(|index| game_texture(&format!("g:p{index:03}"), 8, [1, 2, 3, 255]))
            .collect();
        let blocks: Vec<(NamespacedId, BlockDefinition)> = (0..count)
            .map(|index| {
                (
                    id(&format!("g:b{index:03}")),
                    block([0.0; 3], Some(&format!("g:p{index:03}")), true),
                )
            })
            .collect();
        let result = settled(blocks.clone(), &game, &[], &[]);
        assert_eq!(
            result.errors,
            [
                "game/g:p256.png: 257 pictures of 8 pixels are used and at most 256 of a size are allowed"
            ]
        );
        let fewer = settled(blocks[..count - 1].to_vec(), &game, &[], &[]);
        assert!(fewer.errors.is_empty(), "{:?}", fewer.errors);
    }

    // ---- what is worth saying ----

    #[test]
    fn a_picture_no_block_uses_is_said_for_a_pack_and_not_for_the_game() {
        let game = [game_texture("g:spare", 16, [1, 1, 1, 255])];
        let pack = pack_with(
            "my_pack",
            &[("spare", 16, [1, 1, 1, 255]), ("oak", 16, [1, 1, 1, 255])],
        );
        let result = settled(
            vec![(
                id("my_pack:oak"),
                block([0.0; 3], Some("my_pack:oak"), true),
            )],
            &game,
            &[pack],
            &["my_pack:oak"],
        );
        assert!(result.errors.is_empty());
        assert_eq!(
            result.warnings,
            ["my_pack/textures/spare.png: no block uses this picture"]
        );
    }

    #[test]
    fn a_block_with_a_different_top_that_nothing_turns_is_said_and_one_that_is_turned_or_plain_is_not()
     {
        let pack = pack_with(
            "my_pack",
            &[
                ("log_top", 16, [255, 255, 255, 255]),
                ("log_side", 16, [0, 0, 0, 255]),
                ("slab", 16, [90, 90, 90, 255]),
            ],
        );
        let turned = block([0.5; 3], Some("my_pack:log"), false)
            .with_axis_face(voxel_space::FaceDirection::PositiveY);
        let result = settled(
            vec![
                (
                    id("my_pack:upright"),
                    block([0.5; 3], Some("my_pack:log"), false),
                ),
                (id("my_pack:turned"), turned),
                (
                    id("my_pack:slab"),
                    block([0.35; 3], Some("my_pack:slab"), false),
                ),
            ],
            &[],
            &[pack],
            &["my_pack:upright", "my_pack:turned", "my_pack:slab"],
        );
        assert!(result.errors.is_empty());
        assert_eq!(
            result.warnings,
            [
                "file of my_pack:upright: its top picture differs from its other faces, but nothing turns the block (no engine:axis_face or engine:front_face), so its top is always up"
            ]
        );
    }

    #[test]
    fn a_colour_far_from_the_average_of_the_pictures_is_said_once_for_a_pack_block_and_never_for_the_games()
     {
        let pack = pack_with("my_pack", &[("oak", 16, [255, 255, 255, 255])]);
        let game = [game_texture("g:oak", 16, [255, 255, 255, 255])];
        let result = settled(
            vec![
                (
                    id("my_pack:oak"),
                    block([0.0, 0.0, 0.0], Some("my_pack:oak"), false),
                ),
                (
                    id("my_pack:near"),
                    block([0.95, 0.95, 0.95], Some("my_pack:oak"), false),
                ),
                (id("g:oak"), block([0.0, 0.0, 0.0], Some("g:oak"), false)),
            ],
            &game,
            &[pack],
            &["my_pack:oak", "my_pack:near"],
        );
        assert_eq!(result.warnings.len(), 1, "{:?}", result.warnings);
        assert!(result.warnings[0].starts_with(
            "file of my_pack:oak: its color is 1.73 from the average of its pictures"
        ));
    }

    // ---- the game's own ----

    #[test]
    fn the_games_blocks_are_settled_from_the_games_pictures_alone() {
        let game = [
            game_texture("g:stone", 16, [128, 128, 128, 255]),
            game_texture("g:broken", 16, [0, 0, 0, 255]),
        ];
        let mut blocks = vec![(id("g:stone"), block([0.0; 3], Some("g:stone"), true))];
        let textures = settle_game_pictures(&mut blocks, &game).unwrap();
        close(blocks[0].1.color, [128.0 / 255.0; 3]);
        assert_eq!(textures.tiles.len(), 1);
        let mut missing = vec![(id("g:wood"), block([0.0; 3], Some("g:wood"), true))];
        let errors = settle_game_pictures(&mut missing, &game).unwrap_err();
        assert_eq!(errors.0.len(), 1);
        let bad = [GameTexture {
            id: id("g:bad"),
            file: "game/bad.png".to_owned(),
            bytes: b"x".to_vec(),
        }];
        assert!(settle_game_pictures(&mut [], &bad).is_err());
    }
}
