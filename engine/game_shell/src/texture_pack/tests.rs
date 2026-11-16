use std::collections::BTreeMap;

use block_models::{BlockDefinition, FacePictures, TileRef};
use data_packs::Tile;
use renderer::PictureWrite;

use super::{PictureApplier, PicturesInUse, read_pictures, swap_pictures, use_pictures_from};
use crate::package_data::PackageData;
use crate::shader_packs::tests::Folder;

fn id(text: &str) -> namespaced_id::NamespacedId {
    text.parse().expect("an identifier")
}

fn plain(size: u32, colour: [u8; 4]) -> Tile {
    Tile::from_rgba(size, (0..size * size).flat_map(|_| colour).collect())
}

/// The bytes of a PNG file of one colour.
fn png_of(size: u32, colour: [u8; 4]) -> Vec<u8> {
    let data: Vec<u8> = (0..size * size).flat_map(|_| colour).collect();
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, size, size);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("a header");
    writer.write_image_data(&data).expect("pixels");
    writer.finish().expect("the end");
    bytes
}

fn put(folder: &Folder, file: &str, size: u32, colour: [u8; 4]) {
    let path = folder.path().join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, png_of(size, colour)).unwrap();
}

const WORLD_TOP: [u8; 4] = [10, 10, 10, 255];
const WORLD_SIDE: [u8; 4] = [20, 20, 20, 255];
const WORLD_BOTTOM: [u8; 4] = [30, 30, 30, 255];
const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];

/// A world with the block `test:stone` (the base name `test:stone`), whose top, side and bottom are three pictures of 8 pixels (places (0, 0), (0, 1), (0, 2)), and the block
/// `test:ore` whose three faces are one picture of 16 pixels (place (1, 0)).
fn world() -> PackageData {
    let place = |class, layer| TileRef::new(class, layer).expect("a place");
    let mut package = PackageData::for_tests();
    package.tile_classes = vec![
        vec![
            plain(8, WORLD_TOP),
            plain(8, WORLD_SIDE),
            plain(8, WORLD_BOTTOM),
        ],
        vec![plain(16, [40, 40, 40, 255])],
    ];
    package.blocks.push((
        id("test:stone"),
        Some(
            BlockDefinition::cube([0.5; 3])
                .with_texture(id("test:stone"))
                .with_pictures(FacePictures {
                    top: place(0, 0),
                    side: place(0, 1),
                    bottom: place(0, 2),
                }),
        ),
    ));
    package.blocks.push((
        id("test:ore"),
        Some(
            BlockDefinition::cube([0.5; 3])
                .with_texture(id("test:ore"))
                .with_pictures(FacePictures {
                    top: place(1, 0),
                    side: place(1, 0),
                    bottom: place(1, 0),
                }),
        ),
    ));
    package
}

fn read(folder: &Folder) -> super::ReadPictures {
    read_pictures(folder.path(), &world()).expect("a folder")
}

fn colour_at(overrides: &BTreeMap<(usize, usize), Tile>, place: (usize, usize)) -> Option<[u8; 4]> {
    overrides
        .get(&place)
        .map(|tile| tile.rgba()[0..4].try_into().unwrap())
}

#[test]
fn a_picture_named_for_a_face_overrides_that_face_alone_and_the_rest_are_left_to_the_world() {
    let folder = Folder::new("pictures_face");
    put(&folder, "test/textures/stone_top.png", 8, RED);
    let read = read(&folder);
    assert_eq!(read.problems, Vec::<String>::new());
    assert_eq!(read.overrides.len(), 1);
    assert_eq!(colour_at(&read.overrides, (0, 0)), Some(RED));
}

#[test]
fn the_names_of_the_faces_are_the_packs_a_picture_with_no_ending_is_every_face_that_has_none_of_its_own()
 {
    let folder = Folder::new("pictures_names");
    put(&folder, "test/textures/stone.png", 8, GREEN);
    put(&folder, "test/textures/stone_bottom.png", 8, BLUE);
    let read = read(&folder);
    assert_eq!(read.problems, Vec::<String>::new());
    assert_eq!(colour_at(&read.overrides, (0, 0)), Some(GREEN), "the top");
    assert_eq!(colour_at(&read.overrides, (0, 1)), Some(GREEN), "the side");
    assert_eq!(
        colour_at(&read.overrides, (0, 2)),
        Some(BLUE),
        "the bottom has its own"
    );
}

#[test]
fn a_picture_of_the_block_that_has_one_picture_for_all_its_faces_is_that_picture() {
    let folder = Folder::new("pictures_shared");
    put(&folder, "test/textures/ore_side.png", 16, RED);
    let read = read(&folder);
    assert_eq!(read.problems, Vec::<String>::new());
    assert_eq!(read.overrides.len(), 1);
    assert_eq!(colour_at(&read.overrides, (1, 0)), Some(RED));
}

#[test]
fn faces_that_share_a_picture_in_the_world_but_have_two_in_the_folder_say_so_and_the_first_is_used()
{
    let folder = Folder::new("pictures_conflict");
    put(&folder, "test/textures/ore_top.png", 16, RED);
    put(&folder, "test/textures/ore_side.png", 16, BLUE);
    let read = read(&folder);
    assert_eq!(
        colour_at(&read.overrides, (1, 0)),
        Some(RED),
        "the top comes first"
    );
    assert_eq!(read.problems.len(), 1, "{:?}", read.problems);
    assert!(
        read.problems[0].contains("test:ore"),
        "{}",
        read.problems[0]
    );
    assert!(read.problems[0].contains("share"), "{}", read.problems[0]);
}

#[test]
fn companions_come_with_the_picture_and_a_picture_with_none_clears_the_worlds() {
    let mut package = world();
    // The world's top has both companions.
    package.tile_classes[0][0] = package.tile_classes[0][0]
        .clone()
        .with_specular(vec![1; 8 * 8 * 4])
        .with_normal(vec![2; 8 * 8 * 4]);
    let folder = Folder::new("pictures_companions");
    put(&folder, "test/textures/stone_top.png", 8, RED);
    put(&folder, "test/textures/stone_top_s.png", 8, [7, 7, 7, 7]);
    put(&folder, "test/textures/stone_side.png", 8, GREEN);
    let read = read_pictures(folder.path(), &package).expect("a folder");
    assert_eq!(read.problems, Vec::<String>::new());
    let top = &read.overrides[&(0, 0)];
    assert_eq!(top.specular().map(|pixels| pixels[0]), Some(7));
    assert_eq!(
        top.normal(),
        None,
        "the world's normal picture belonged to the picture that was replaced"
    );
    // The side had none, and has none.
    assert_eq!(read.overrides[&(0, 1)].specular(), None);
}

#[test]
fn a_companion_alone_keeps_the_worlds_picture_and_the_companion_it_did_not_replace() {
    let mut package = world();
    package.tile_classes[0][0] = package.tile_classes[0][0]
        .clone()
        .with_specular(vec![1; 8 * 8 * 4])
        .with_normal(vec![2; 8 * 8 * 4]);
    let folder = Folder::new("pictures_companion_alone");
    put(&folder, "test/textures/stone_top_n.png", 8, [9, 9, 9, 9]);
    let read = read_pictures(folder.path(), &package).expect("a folder");
    assert_eq!(read.problems, Vec::<String>::new());
    let top = &read.overrides[&(0, 0)];
    assert_eq!(
        top.rgba(),
        package.tile_classes[0][0].rgba(),
        "the picture is the world's"
    );
    assert_eq!(
        top.specular().map(|pixels| pixels[0]),
        Some(1),
        "the specular picture is the world's"
    );
    assert_eq!(
        top.normal().map(|pixels| pixels[0]),
        Some(9),
        "the normal picture is the folder's"
    );
}

#[test]
fn a_file_that_is_not_right_is_a_problem_that_names_it_and_the_rest_are_still_read() {
    let folder = Folder::new("pictures_problems");
    // The wrong size for the place, not a picture, a block the world does not have, a hidden file.
    put(&folder, "test/textures/stone_top.png", 16, RED);
    folder.write("test/textures/stone_side.png", "not a PNG");
    put(&folder, "test/textures/granite.png", 8, RED);
    put(&folder, "test/textures/.stone_bottom.png", 8, RED);
    put(&folder, "test/textures/ore.png", 16, GREEN);
    let read = read(&folder);
    assert_eq!(read.overrides.len(), 1, "{:?}", read.overrides.keys());
    assert_eq!(colour_at(&read.overrides, (1, 0)), Some(GREEN));
    assert_eq!(read.problems.len(), 2, "{:?}", read.problems);
    assert!(
        read.problems
            .iter()
            .any(|problem| problem.contains("stone_top.png") && problem.contains("16")),
        "{:?}",
        read.problems
    );
    assert!(
        read.problems
            .iter()
            .any(|problem| problem.contains("stone_side.png")),
        "{:?}",
        read.problems
    );
}

#[test]
fn a_folder_that_is_not_there_is_an_error() {
    let missing = std::path::Path::new("this/folder/is/not/there");
    let error = read_pictures(missing, &world()).expect_err("no such folder");
    assert!(
        error.contains("this/folder/is/not/there") || error.contains("not there"),
        "{error}"
    );
}

/// Stands in for the renderer: remembers every write, and refuses the places it is told to.
#[derive(Default)]
struct FakeRenderer {
    writes: Vec<((usize, usize), Vec<u8>)>,
    refuse: Option<(usize, usize)>,
}

impl PictureApplier for FakeRenderer {
    fn write(
        &mut self,
        class: usize,
        layer: usize,
        picture: &PictureWrite<'_>,
    ) -> Result<(), String> {
        if self.refuse == Some((class, layer)) {
            return Err("the renderer refuses".to_owned());
        }
        self.writes
            .push(((class, layer), picture.rgba[0..4].to_vec()));
        Ok(())
    }
}

#[test]
fn a_folder_is_written_once_and_what_is_written_again_with_the_same_files_is_no_change() {
    let folder = Folder::new("pictures_swap");
    put(&folder, "test/textures/stone_top.png", 8, RED);
    put(&folder, "test/textures/ore.png", 16, GREEN);
    let package = world();
    let (mut renderer, mut in_use) = (FakeRenderer::default(), PicturesInUse::default());
    let detail = use_pictures_from(&mut renderer, &package, &mut in_use, folder.path())
        .expect("taken")
        .expect("something was written");
    assert!(detail.contains("2 pictures"), "{detail}");
    assert_eq!(
        renderer.writes,
        [((0, 0), RED.to_vec()), ((1, 0), GREEN.to_vec())]
    );
    renderer.writes.clear();
    assert_eq!(
        use_pictures_from(&mut renderer, &package, &mut in_use, folder.path()),
        Ok(None)
    );
    assert!(
        renderer.writes.is_empty(),
        "nothing is written for no change"
    );
}

#[test]
fn a_change_writes_only_what_changed_and_a_picture_taken_out_of_the_folder_is_the_worlds_again() {
    let folder = Folder::new("pictures_restore");
    put(&folder, "test/textures/stone_top.png", 8, RED);
    put(&folder, "test/textures/stone_side.png", 8, BLUE);
    let package = world();
    let (mut renderer, mut in_use) = (FakeRenderer::default(), PicturesInUse::default());
    use_pictures_from(&mut renderer, &package, &mut in_use, folder.path()).expect("taken");
    renderer.writes.clear();
    // The top is edited, the side is deleted.
    put(&folder, "test/textures/stone_top.png", 8, GREEN);
    std::fs::remove_file(folder.path().join("test/textures/stone_side.png")).unwrap();
    let detail = use_pictures_from(&mut renderer, &package, &mut in_use, folder.path())
        .expect("taken")
        .expect("something changed");
    assert!(detail.contains("1 picture"), "{detail}");
    assert!(detail.contains("1 back"), "{detail}");
    assert_eq!(
        renderer.writes,
        [((0, 0), GREEN.to_vec()), ((0, 1), WORLD_SIDE.to_vec())],
        "the edited one, and the world's own side"
    );
}

#[test]
fn a_renderer_that_refuses_leaves_what_was_written_known_and_the_refused_is_tried_again() {
    let folder = Folder::new("pictures_refused");
    put(&folder, "test/textures/stone_top.png", 8, RED);
    put(&folder, "test/textures/stone_side.png", 8, BLUE);
    let package = world();
    let (mut renderer, mut in_use) = (
        FakeRenderer {
            refuse: Some((0, 1)),
            ..FakeRenderer::default()
        },
        PicturesInUse::default(),
    );
    let error = use_pictures_from(&mut renderer, &package, &mut in_use, folder.path())
        .expect_err("the side is refused");
    assert!(error.contains("the renderer refuses"), "{error}");
    assert_eq!(renderer.writes, [((0, 0), RED.to_vec())]);
    renderer.refuse = None;
    renderer.writes.clear();
    use_pictures_from(&mut renderer, &package, &mut in_use, folder.path()).expect("taken");
    assert_eq!(
        renderer.writes,
        [((0, 1), BLUE.to_vec())],
        "the top is known to be written, and only the side is tried again"
    );
}

#[test]
fn problems_are_told_in_what_a_reload_says_but_do_not_stop_the_rest() {
    let folder = Folder::new("pictures_told");
    put(&folder, "test/textures/stone_top.png", 8, RED);
    folder.write("test/textures/stone_side.png", "not a PNG");
    let package = world();
    let (mut renderer, mut in_use) = (FakeRenderer::default(), PicturesInUse::default());
    let detail = use_pictures_from(&mut renderer, &package, &mut in_use, folder.path())
        .expect("taken")
        .expect("one was written");
    assert!(detail.contains("1 picture"), "{detail}");
    assert!(detail.contains("1 problem"), "{detail}");
    let read = swap_pictures(
        &mut renderer,
        &package,
        &mut in_use,
        folder.path(),
        read_pictures(folder.path(), &package).expect("a folder"),
    );
    assert!(read.is_ok());
}

#[test]
fn a_picture_that_was_in_use_and_whose_file_is_now_no_good_stays_as_it_was_and_the_problem_is_told()
{
    let folder = Folder::new("pictures_kept");
    put(&folder, "test/textures/stone_top.png", 8, RED);
    put(&folder, "test/textures/stone_side.png", 8, BLUE);
    let package = world();
    let (mut renderer, mut in_use) = (FakeRenderer::default(), PicturesInUse::default());
    use_pictures_from(&mut renderer, &package, &mut in_use, folder.path()).expect("taken");
    renderer.writes.clear();
    // The top's file is damaged (an editor halfway through writing it, say) and the side's is deleted.
    folder.write("test/textures/stone_top.png", "damaged");
    std::fs::remove_file(folder.path().join("test/textures/stone_side.png")).unwrap();
    let detail = use_pictures_from(&mut renderer, &package, &mut in_use, folder.path())
        .expect("taken")
        .expect("something was told");
    assert!(detail.contains("1 problem"), "{detail}");
    assert_eq!(
        renderer.writes,
        [((0, 1), WORLD_SIDE.to_vec())],
        "only the deleted side goes back to the world's; the damaged top is kept as it was"
    );
    // When the file is good again it is written.
    renderer.writes.clear();
    put(&folder, "test/textures/stone_top.png", 8, GREEN);
    use_pictures_from(&mut renderer, &package, &mut in_use, folder.path()).expect("taken");
    assert_eq!(renderer.writes, [((0, 0), GREEN.to_vec())]);
}

#[test]
fn a_file_of_the_wrong_size_keeps_the_picture_as_it_was_too() {
    let folder = Folder::new("pictures_kept_size");
    put(&folder, "test/textures/stone_top.png", 8, RED);
    let package = world();
    let (mut renderer, mut in_use) = (FakeRenderer::default(), PicturesInUse::default());
    use_pictures_from(&mut renderer, &package, &mut in_use, folder.path()).expect("taken");
    renderer.writes.clear();
    put(&folder, "test/textures/stone_top.png", 16, GREEN);
    use_pictures_from(&mut renderer, &package, &mut in_use, folder.path()).expect("taken");
    assert!(renderer.writes.is_empty(), "the red top is still there");
}
