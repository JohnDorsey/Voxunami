use std::path::{Path, PathBuf};

use crate::engine_shaders::EngineShaderTexts;
use crate::shader_pack::{DEFAULT_LIGHTING, Relief, Shading};
use crate::shader_settings::{SettingChoices, SettingValue, ShaderSettings};
use crate::voxel_renderer::{SunWalk, voxel_source};

/// Every kind of voxel shader the engine makes: with and without pictures (and, with them, each relief), each way of walking the sun's ray.
fn every_kind() -> Vec<(bool, Relief, SunWalk)> {
    let mut walks = vec![SunWalk::None, SunWalk::Shade];
    for shapes in [false, true] {
        for other_windows in [false, true] {
            for entities in [false, true] {
                walks.push(SunWalk::Voxels {
                    shapes,
                    other_windows,
                    entities,
                });
            }
        }
    }
    let mut kinds = Vec::new();
    for walk in walks {
        kinds.push((false, Relief::Flat, walk));
        for relief in Relief::ALL {
            kinds.push((true, relief, walk));
        }
    }
    kinds
}

fn shading_with(relief: Relief, engine: Option<EngineShaderTexts>) -> Shading {
    let choices = SettingChoices::from([(
        "relief".to_owned(),
        SettingValue::Choice(relief.name().to_owned()),
    )]);
    Shading {
        lighting: DEFAULT_LIGHTING.to_owned(),
        settings: ShaderSettings::resolve(&choices).expect("a word of the relief"),
        engine: engine.map(std::sync::Arc::new),
    }
}

/// FNV-1a: the same on every machine and every version of Rust, as a number a test can name.
fn fingerprint(texts: &[String]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for text in texts {
        for byte in text.bytes().chain([0xff]) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

fn every_text(engine: impl Fn() -> Option<EngineShaderTexts>) -> Vec<String> {
    every_kind()
        .into_iter()
        .map(|(with_pictures, relief, walk)| {
            voxel_source(with_pictures, walk, &shading_with(relief, engine()))
        })
        .collect()
}

/// The folder the engine's shaders are in, which a developer's build reads them from.
fn source_folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

#[test]
fn the_text_of_every_kind_of_voxel_shader_is_what_it_was_before_the_parts_were_made_replaceable() {
    // (Taken from the code as it was before the parts were a struct. In this merge the fingerprint is of main's own `voxel_source`, with `include_str!`, for all 40
    // kinds, the shade of faces too; the texts made from the parts were compared with it one by one and are the same to the byte.)
    let texts = every_text(|| None);
    assert_eq!(texts.len(), 10 * 4);
    assert_eq!(fingerprint(&texts), 12_501_583_373_592_185_547);
}

#[test]
fn the_parts_read_from_the_source_folder_are_the_parts_the_program_has() {
    let read = EngineShaderTexts::read(&source_folder()).expect("the folder of the shaders");
    assert_eq!(&read, EngineShaderTexts::built_in());
    // So a program that reads its shaders from there draws as one that does not, until a file is edited.
    assert_eq!(
        every_text(|| Some(read.clone())),
        every_text(|| None),
        "the same 40 texts"
    );
}

#[test]
fn every_part_is_a_file_and_every_file_is_named_once() {
    let mut names = EngineShaderTexts::FILES.to_vec();
    assert_eq!(names.len(), 17);
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 17, "no file twice");
    for name in names {
        assert!(
            source_folder().join(name).is_file(),
            "{name} is in the source folder"
        );
    }
}

#[test]
fn a_part_that_is_replaced_is_in_the_text_where_it_is_used_and_nowhere_else() {
    let marker = "// MARKER-OF-THE-TEST";
    let mut texts = EngineShaderTexts::built_in().clone();
    texts.contact_on = format!("{}\n{marker}", texts.contact_on);
    let marked = every_text(|| Some(texts.clone()));
    for ((_, _, walk), text) in every_kind().into_iter().zip(&marked) {
        assert_eq!(
            text.contains(marker),
            matches!(walk, SunWalk::Voxels { .. }),
            "the walk of the sun's ray is in the text of a kind that walks it, and only there"
        );
    }
    // The part is told apart from what is not replaced: the voxel shader itself.
    let mut texts = EngineShaderTexts::built_in().clone();
    texts.voxel = format!("{marker}\n{}", texts.voxel);
    assert!(
        every_text(|| Some(texts.clone()))
            .iter()
            .all(|text| text.starts_with(marker)),
        "the voxel shader starts every kind"
    );
}

#[test]
fn a_folder_that_lacks_a_file_is_refused_and_the_file_is_named() {
    let folder =
        std::env::temp_dir().join(format!("engine_shaders_lacking_{}", std::process::id()));
    std::fs::create_dir_all(&folder).expect("a folder");
    for name in EngineShaderTexts::FILES {
        if *name != "windows_on.wgsl" {
            std::fs::write(folder.join(name), "// a part").expect("a file");
        }
    }
    let error = EngineShaderTexts::read(&folder).expect_err("one file is missing");
    std::fs::remove_dir_all(&folder).ok();
    assert!(error.contains("windows_on.wgsl"), "{error}");
    let nowhere =
        EngineShaderTexts::read(Path::new("this/folder/is/not/there")).expect_err("no such folder");
    assert!(nowhere.contains("voxel.wgsl"), "{nowhere}");
}

#[test]
fn the_shaders_are_accepted_by_the_shader_language_when_made_from_the_files_of_the_folder() {
    let read = EngineShaderTexts::read(&source_folder()).expect("the folder of the shaders");
    for source in every_text(|| Some(read.clone())) {
        let module = naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|error| panic!("{}", error.emit_to_string(&source)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|error| panic!("{error:?}"));
    }
}
