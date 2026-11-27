//! The engine's own shaders made again from text a developer edited (`VoxelRenderer::set_engine_shaders`), looked at in what they do to a picture on every adapter: an edit that is
//! good shows, an edit the graphics card refuses leaves the picture as it was, and giving the text back is the game's own shader to the pixel.

use renderer::EngineShaderTexts;

use super::shader_packs::block_scene;
use super::*;

/// The line of `voxel.wgsl` that makes the colour of a face the picture has (every pixel of a face goes through it).
const THE_COLOUR_OF_A_FACE: &str =
    "return vec4<f32>(lifted(mix(color, frame.fog_color.rgb, fog)), 1.0);";

fn halved() -> EngineShaderTexts {
    let mut texts = EngineShaderTexts::built_in().clone();
    assert_eq!(
        texts.voxel.matches(THE_COLOUR_OF_A_FACE).count(),
        1,
        "the line the test edits is in the shader once (if the shader changed, so does this line)"
    );
    texts.voxel = texts.voxel.replace(
        THE_COLOUR_OF_A_FACE,
        "return vec4<f32>(lifted(mix(color, frame.fog_color.rgb, fog)) * 0.5, 1.0);",
    );
    texts
}

/// A pack that draws as the game does, but is not the same text, so that choosing it makes the shaders again.
fn pack_that_says_nothing_more() -> renderer::ShaderPack {
    renderer::ShaderPack::new(
        "test:plain",
        "Plain",
        &format!(
            "{}
// nothing more
",
            renderer::default_lighting()
        ),
    )
    .expect("a pack")
}

fn brightness(image: &[u8]) -> u64 {
    image
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| u64::from(pixel[0]) + u64::from(pixel[1]) + u64::from(pixel[2]))
        .sum()
}

#[test]
fn an_edited_engine_shader_shows_and_giving_back_the_program_s_own_text_restores_the_picture() {
    for context in contexts() {
        let name = adapter_name(&context);
        let (mut renderer, camera, _) = block_scene(&context);
        let before = render(&mut renderer, &camera);
        assert!(
            renderer
                .set_engine_shaders(Some(halved()))
                .expect("the card takes an edit that halves the colour"),
            "{name}: something was made"
        );
        assert!(
            !renderer
                .set_engine_shaders(Some(halved()))
                .expect("the same edit"),
            "{name}: the same text again makes nothing"
        );
        let edited = render(&mut renderer, &camera);
        assert_ne!(edited, before, "{name}: the edit shows");
        assert!(
            brightness(&edited) < brightness(&before),
            "{name}: half the colour is darker ({} against {})",
            brightness(&edited),
            brightness(&before)
        );
        assert!(
            renderer
                .set_engine_shaders(None)
                .expect("the program's own text"),
            "{name}: made again from the program's text"
        );
        assert_eq!(render(&mut renderer, &camera), before, "{name}: given back");
        // Handing over the text the program has is no change and makes nothing.
        assert!(
            !renderer
                .set_engine_shaders(Some(EngineShaderTexts::built_in().clone()))
                .expect("the same text"),
            "{name}: nothing to make"
        );
        assert_eq!(render(&mut renderer, &camera), before, "{name}: the same");
    }
}

#[test]
fn an_engine_shader_the_card_refuses_leaves_the_picture_as_it_was_and_says_why() {
    for context in contexts() {
        let name = adapter_name(&context);
        let (mut renderer, camera, _) = block_scene(&context);
        renderer
            .set_engine_shaders(Some(halved()))
            .expect("a good edit");
        let edited = render(&mut renderer, &camera);
        let mut broken = halved();
        broken.voxel = broken
            .voxel
            .replace("* 0.5, 1.0);", "* a_name_nothing_has, 1.0);");
        let error = renderer
            .set_engine_shaders(Some(broken))
            .expect_err("a name that is nowhere is refused")
            .to_string();
        assert!(error.contains("a_name_nothing_has"), "{name}: {error}");
        assert_eq!(
            render(&mut renderer, &camera),
            edited,
            "{name}: still the last good edit"
        );
        // The refused text was not kept as the one in use: it is refused again when it is given again, and what is made afterwards is made from the last good one.
        let mut broken_again = halved();
        broken_again.voxel = broken_again
            .voxel
            .replace("* 0.5, 1.0);", "* a_name_nothing_has, 1.0);");
        assert!(
            renderer.set_engine_shaders(Some(broken_again)).is_err(),
            "{name}: refused again"
        );
        renderer
            .set_shader_pack(Some(&pack_that_says_nothing_more()))
            .expect("a pack the card takes");
        assert_eq!(
            render(&mut renderer, &camera),
            edited,
            "{name}: a pack made after the refusal is made with the last good edit"
        );
    }
}

#[test]
fn a_shader_pack_chosen_afterwards_is_made_with_the_edited_engine_shader() {
    for context in contexts() {
        let name = adapter_name(&context);
        let (mut renderer, camera, _) = block_scene(&context);
        renderer
            .set_engine_shaders(Some(halved()))
            .expect("a good edit");
        let edited = render(&mut renderer, &camera);
        // A pack whose lighting is the game's own, with a comment (the renderer makes nothing for a pack that is the same text, so it has to differ): the shaders are made
        // again, and the text of the engine is the edited one still.
        renderer
            .set_shader_pack(Some(&pack_that_says_nothing_more()))
            .expect("the card takes the pack");
        assert_eq!(
            render(&mut renderer, &camera),
            edited,
            "{name}: a pack does not undo the edit"
        );
    }
}
