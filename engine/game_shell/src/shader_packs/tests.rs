use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

/// A folder in the temporary directory that is removed with it.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Self {
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "voxunami_shader_packs_{name}_{}_{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    /// Writes a file in it (and the folders on the way).
    fn write(&self, file: &str, text: &str) -> &Self {
        let path = self.0.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
        self
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const PACK_JSON: &str =
    r#"{ "id": "tester:cel", "name": "Cel shading", "interface": 1, "description": "Steps." }"#;
const LIGHTING: &str = "fn pack_shade(s: Surface) -> vec3<f32> { return s.albedo * floor(s.sun_visible * 3.0) / 3.0; }";

fn good_pack(folder: &Folder, at: &str, id: &str, name: &str) {
    folder.write(
        &format!("{at}/pack.json"),
        &format!(r#"{{ "id": "{id}", "name": "{name}" }}"#),
    );
    folder.write(&format!("{at}/lighting.wgsl"), LIGHTING);
}

fn problems_of(folder: &Folder) -> Vec<String> {
    read_pack(folder.path()).expect_err("the folder is not a pack")
}

#[test]
fn a_folder_with_a_pack_json_and_a_lighting_file_is_a_pack() {
    let folder = Folder::new("good");
    folder.write("pack.json", PACK_JSON);
    folder.write("lighting.wgsl", LIGHTING);
    let pack = read_pack(folder.path()).expect("a pack");
    assert_eq!(pack.id(), "tester:cel");
    assert_eq!(pack.name(), "Cel shading");
    assert_eq!(pack.lighting(), LIGHTING);
    // The interface and the format may be left out (and the description).
    let plain = Folder::new("plain");
    plain.write("pack.json", r#"{ "id": "tester:plain", "name": "Plain" }"#);
    plain.write("lighting.wgsl", LIGHTING);
    assert!(read_pack(plain.path()).is_ok());
}

#[test]
fn a_folder_that_is_not_a_pack_is_told_what_is_missing_or_wrong_in_which_file() {
    let folder = Folder::new("missing");
    let problems = problems_of(&folder);
    assert_eq!(problems.len(), 1);
    assert!(
        problems[0].contains("there is no pack.json"),
        "{problems:?}"
    );
    folder.write("pack.json", "{ not json");
    let problems = problems_of(&folder);
    assert!(problems[0].contains("pack.json"), "{problems:?}");
    folder.write("pack.json", r#"[1, 2]"#);
    assert!(problems_of(&folder)[0].contains("not a JSON object"));
    // Everything wrong at once: a key that is not one, a name missing, an id that is not namespaced, and no lighting file.
    folder.write(
        "pack.json",
        r#"{ "id": "plain", "color": 3, "format": 2, "interface": 9 }"#,
    );
    let problems = problems_of(&folder);
    let all = problems.join("\n");
    for wanted in [
        "`color` is not a key",
        "`name` is missing",
        "not a namespaced id",
        "`format` is 2",
        "`interface` is 9",
        "there is no lighting.wgsl",
    ] {
        assert!(all.contains(wanted), "{wanted}: {all}");
    }
    assert_eq!(problems.len(), 6, "{all}");
    // Text that is not text.
    folder.write("pack.json", r#"{ "id": 4, "name": "", "description": 5 }"#);
    let all = problems_of(&folder).join("\n");
    assert!(all.contains("`id` is a text that is not empty"), "{all}");
    assert!(all.contains("`name` is a text that is not empty"), "{all}");
    assert!(all.contains("`description` is a text"), "{all}");
}

#[test]
fn a_mistake_in_the_lighting_is_told_with_the_file_and_the_line() {
    let folder = Folder::new("mistake");
    folder.write("pack.json", PACK_JSON);
    folder.write(
        "lighting.wgsl",
        "// a pack\nfn pack_shade(s: Surface) -> vec3<f32> {\n    for (;;) {}\n    return s.albedo;\n}\n",
    );
    let problems = problems_of(&folder);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].contains("lighting.wgsl, line 3, column 5"),
        "{problems:?}"
    );
    // A problem of the whole has no line.
    folder.write("lighting.wgsl", "fn other() {}");
    let problems = problems_of(&folder);
    assert!(
        problems[0].contains("lighting.wgsl: the pack does not define"),
        "{problems:?}"
    );
}

#[test]
fn a_folder_of_packs_gives_the_good_ones_by_id_and_the_problems_of_the_others_and_ignores_files() {
    let folder = Folder::new("shelf");
    good_pack(&folder, "zebra", "tester:zebra", "Zebra");
    good_pack(&folder, "apple", "tester:apple", "Apple");
    good_pack(&folder, "copy", "tester:apple", "Another apple");
    folder.write("broken/pack.json", "{}");
    folder.write("README.txt", "not a pack");
    let shelf = read_directory(folder.path()).expect("a folder");
    let ids: Vec<&str> = shelf.packs.iter().map(ShaderPack::id).collect();
    assert_eq!(ids, ["tester:apple", "tester:zebra"]);
    assert_eq!(
        shelf.packs[0].name(),
        "Apple",
        "the first of two with an id"
    );
    let all = shelf.problems.join("\n");
    assert!(
        all.contains("the id tester:apple is the id of Apple too"),
        "{all}"
    );
    assert!(all.contains("broken"), "{all}");
    assert!(!all.contains("README"), "{all}");
    assert_eq!(
        shelf.names(),
        [
            ShaderPackName {
                id: "tester:apple".to_owned(),
                name: "Apple".to_owned()
            },
            ShaderPackName {
                id: "tester:zebra".to_owned(),
                name: "Zebra".to_owned()
            },
        ]
    );
}

#[test]
fn a_folder_that_does_not_exist_is_an_error_and_an_empty_one_is_no_packs() {
    let folder = Folder::new("empty");
    let shelf = read_directory(folder.path()).unwrap();
    assert!(shelf.packs.is_empty() && shelf.problems.is_empty());
    let error = read_directory(&folder.path().join("nothing_here")).unwrap_err();
    assert!(error.contains("shader packs folder"), "{error}");
}

fn names() -> Vec<ShaderPackName> {
    [
        ("someone:cel", "Cel shading"),
        ("someone:celestial", "Celestial"),
        ("other:flat", "Flat"),
    ]
    .map(|(id, name)| ShaderPackName {
        id: id.to_owned(),
        name: name.to_owned(),
    })
    .to_vec()
}

#[test]
fn a_pack_is_picked_by_its_id_its_name_in_any_case_or_the_one_beginning_a_text_begins() {
    let names = names();
    assert_eq!(pick(&names, "someone:cel"), Ok(0));
    assert_eq!(
        pick(&names, "SOMEONE:CEL"),
        Ok(0),
        "the id in full, in any case"
    );
    assert_eq!(pick(&names, "CEL SHADING"), Ok(0));
    // The beginning of a name that is not the beginning of an id.
    assert_eq!(pick(&names, "cel sh"), Ok(0));
    assert_eq!(pick(&names, "Celest"), Ok(1));
    // The part after the colon, in full, beats a beginning of another's.
    assert_eq!(pick(&names, "cel"), Ok(0));
    assert_eq!(
        pick(&names, "celestial"),
        Ok(1),
        "a name in full beats a beginning of another"
    );
    assert_eq!(pick(&names, "fl"), Ok(2));
    assert_eq!(pick(&names, "other"), Ok(2));
    // Two begin so.
    let error = pick(&names, "ce").unwrap_err();
    assert!(
        error.contains("could be any of") && error.contains("Celestial"),
        "{error}"
    );
    let error = pick(&names, "nothing").unwrap_err();
    assert!(
        error.contains("there is no shader pack called") && error.contains("Flat (other:flat)"),
        "{error}"
    );
    assert!(pick(&[], "any").unwrap_err().contains("there are none"));
}

#[test]
fn the_shelf_of_a_player_is_the_default_folder_if_it_is_there_and_the_one_asked_for_with_the_first_of_two_with_an_id_kept()
 {
    let data = Folder::new("data");
    good_pack(&data, "one", "tester:one", "One");
    good_pack(
        &data,
        "shared",
        "tester:shared",
        "Shared (the data folder's)",
    );
    let asked = Folder::new("asked");
    good_pack(&asked, "two", "tester:two", "Two");
    good_pack(
        &asked,
        "shared",
        "tester:shared",
        "Shared (the asked folder's)",
    );
    let shelf = shelf_of(Some(data.path()), Some(asked.path())).unwrap();
    let ids: Vec<&str> = shelf.packs.iter().map(ShaderPack::id).collect();
    assert_eq!(ids, ["tester:one", "tester:shared", "tester:two"]);
    let shared = shelf
        .packs
        .iter()
        .find(|pack| pack.id() == "tester:shared")
        .unwrap();
    assert_eq!(shared.name(), "Shared (the data folder's)");
    assert_eq!(shelf.problems.len(), 1, "{:?}", shelf.problems);
    // A default folder that is not there is no packs and no problem; one that was asked for and is not there is an error.
    let nowhere = data.path().join("nowhere");
    let shelf = shelf_of(Some(&nowhere), None).unwrap();
    assert!(shelf.packs.is_empty() && shelf.problems.is_empty());
    assert!(shelf_of(None, Some(&nowhere)).is_err());
    assert!(shelf_of(None, None).unwrap().packs.is_empty());
}

/// The folder of the example packs the renderer has.
fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../renderer/example_shader_packs")
}

#[test]
fn the_example_packs_are_good_packs() {
    let shelf = read_directory(&examples()).expect("the examples folder");
    assert_eq!(shelf.problems, Vec::<String>::new());
    let ids: Vec<&str> = shelf.packs.iter().map(ShaderPack::id).collect();
    assert_eq!(ids, ["voxunami:cel", "voxunami:shadowless"]);
}

#[test]
fn checking_shader_packs_says_ok_for_each_and_every_problem_of_the_rest_and_takes_a_pack_or_a_folder_of_them()
 {
    assert!(run_check_shader_packs(vec![examples().display().to_string()]).is_ok());
    assert!(run_check_shader_packs(vec![examples().join("cel").display().to_string()]).is_ok());
    let folder = Folder::new("checked");
    good_pack(&folder, "fine", "tester:fine", "Fine");
    folder.write("bad/pack.json", r#"{ "id": "tester:bad", "name": "Bad" }"#);
    folder.write(
        "bad/lighting.wgsl",
        "fn pack_shade(s: Surface) -> vec3<f32> {\n  loop {}\n  return s.albedo;\n}",
    );
    let error = run_check_shader_packs(vec![folder.path().display().to_string()]).unwrap_err();
    assert!(error.contains("lighting.wgsl, line 2, column 3"), "{error}");
    let empty = Folder::new("none");
    assert!(
        run_check_shader_packs(vec![empty.path().display().to_string()])
            .unwrap_err()
            .contains("no shader packs")
    );
    assert!(
        run_check_shader_packs(vec![])
            .unwrap_err()
            .contains("usage")
    );
    assert!(
        run_check_shader_packs(vec!["a".into(), "b".into()])
            .unwrap_err()
            .contains("usage")
    );
}
