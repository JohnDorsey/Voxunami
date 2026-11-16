//! Reloading the engine's own shaders (`crate::reload`, `docs/hot-reload-plan.md` step 2): what is read from the source folder is put in use only if the renderer takes it.

use renderer::EngineShaderTexts;

use crate::shader_packs::tests::Folder;
use crate::view::{EngineShaderApplier, use_engine_shaders_from};

/// Stands in for the renderer: remembers what it was given and says whether it made anything, and refuses when told to.
#[derive(Default)]
struct FakeRenderer {
    given: Vec<EngineShaderTexts>,
    refuse: Option<String>,
    already_has_it: bool,
}

impl EngineShaderApplier for FakeRenderer {
    fn apply(&mut self, texts: EngineShaderTexts) -> Result<bool, String> {
        if let Some(reason) = &self.refuse {
            return Err(reason.clone());
        }
        self.given.push(texts);
        Ok(!self.already_has_it)
    }
}

/// A folder with every file of the engine's shaders in it, as the program has them.
fn folder_of_shaders(name: &str) -> Folder {
    let folder = Folder::new(name);
    let built_in = EngineShaderTexts::built_in();
    for file in EngineShaderTexts::FILES {
        let text = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../renderer/src")
                .join(file),
        )
        .expect("a file of the shaders");
        folder.write(file, &text);
    }
    assert_eq!(
        EngineShaderTexts::read(folder.path()).as_ref(),
        Ok(built_in),
        "the copy is what the program has"
    );
    folder
}

#[test]
fn what_the_folder_has_is_given_to_the_renderer_and_the_reload_says_so() {
    let folder = folder_of_shaders("engine_shaders_given");
    folder.write("voxel.wgsl", "// an edit");
    let mut renderer = FakeRenderer::default();
    let detail = use_engine_shaders_from(&mut renderer, folder.path()).expect("it is taken");
    let detail = detail.expect("something was made");
    assert!(detail.contains("the engine's shaders"), "{detail}");
    assert!(detail.contains("engine_shaders_given"), "{detail}");
    assert_eq!(renderer.given.len(), 1);
    assert_eq!(renderer.given[0].voxel, "// an edit");
    assert_eq!(
        renderer.given[0].contact_on,
        EngineShaderTexts::built_in().contact_on,
        "the other files are what they were read as"
    );
}

#[test]
fn files_that_are_what_the_renderer_has_change_nothing_and_say_so() {
    let folder = folder_of_shaders("engine_shaders_same");
    let mut renderer = FakeRenderer {
        already_has_it: true,
        ..FakeRenderer::default()
    };
    assert_eq!(
        use_engine_shaders_from(&mut renderer, folder.path()),
        Ok(None)
    );
}

#[test]
fn a_folder_with_a_file_missing_is_an_error_and_the_renderer_is_not_asked() {
    let folder = folder_of_shaders("engine_shaders_lacking");
    std::fs::remove_file(folder.path().join("relief_ray.wgsl")).expect("a file to remove");
    let mut renderer = FakeRenderer::default();
    let error = use_engine_shaders_from(&mut renderer, folder.path()).expect_err("a file is gone");
    assert!(error.contains("relief_ray.wgsl"), "{error}");
    assert!(renderer.given.is_empty());
}

#[test]
fn a_renderer_that_refuses_leaves_the_shaders_in_use_and_the_reason_is_told() {
    let folder = folder_of_shaders("engine_shaders_refused");
    let mut renderer = FakeRenderer {
        refuse: Some("expected `;`, found `}` (line 41)".to_owned()),
        ..FakeRenderer::default()
    };
    let error = use_engine_shaders_from(&mut renderer, folder.path()).expect_err("refused");
    assert!(error.contains("expected `;`"), "{error}");
    assert!(
        error.contains("stay"),
        "the player is told nothing was changed: {error}"
    );
    assert!(renderer.given.is_empty());
}
