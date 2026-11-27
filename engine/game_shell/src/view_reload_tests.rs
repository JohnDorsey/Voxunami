//! Reloading shader packs (`crate::reload`, `docs/hot-reload-plan.md` step 1): what is read again is put in use only if the renderer takes it, and the player's choices stay.

use renderer::{SettingChoices, SettingValue};

use crate::client_commands::ClientSettings;
use crate::shader_packs::tests::Folder;
use crate::view::{
    ReadShaderPacks, ShaderApplier, ShaderPacks, read_shader_packs, swap_shader_packs,
};

const LIGHTING: &str = "fn pack_shade(s: Surface) -> vec3<f32> { return s.albedo; }";
const BRIGHTER: &str = "fn pack_shade(s: Surface) -> vec3<f32> { return s.albedo * 1.5; }";

/// Stands in for the renderer: remembers what it was given, and refuses when told to.
#[derive(Default)]
struct FakeRenderer {
    given: Vec<(Option<String>, Option<String>, SettingChoices)>,
    refuse: Option<String>,
}

impl ShaderApplier for FakeRenderer {
    fn apply(
        &mut self,
        pack: Option<&renderer::ShaderPack>,
        choices: &SettingChoices,
    ) -> Result<(), String> {
        if let Some(reason) = &self.refuse {
            return Err(reason.clone());
        }
        self.given.push((
            pack.map(|pack| pack.id().to_owned()),
            pack.map(|pack| pack.lighting().to_owned()),
            choices.clone(),
        ));
        Ok(())
    }
}

fn pack(folder: &Folder, at: &str, id: &str, lighting: &str) {
    folder.write(
        &format!("{at}/shaderpack.json"),
        &format!(r#"{{ "id": "{id}", "name": "Pack {at}" }}"#),
    );
    folder.write(&format!("{at}/lighting.wgsl"), lighting);
}

fn read(folder: &Folder) -> ReadShaderPacks {
    read_shader_packs(Some(folder.path()), None).expect("the folder can be read")
}

/// The state of a game that started with these packs and chose `chosen`, as `prepare_shader_packs` leaves it.
fn started(folder: &Folder, chosen: Option<&str>) -> (ShaderPacks, ClientSettings) {
    let read = read(folder);
    let mut settings = ClientSettings::new(0);
    settings.shader_packs = read.names;
    settings.shader_pack = chosen.map(str::to_owned);
    settings.pack_settings = read.choices;
    (
        ShaderPacks {
            packs: read.shelf.packs,
            files: read.files,
        },
        settings,
    )
}

#[test]
fn files_that_are_what_is_in_use_change_nothing_and_say_so() {
    let folder = Folder::new("view_same");
    pack(&folder, "cel", "tester:cel", LIGHTING);
    let (mut packs, mut settings) = started(&folder, Some("tester:cel"));
    let mut renderer = FakeRenderer::default();
    let outcome = swap_shader_packs(&mut renderer, &mut packs, &mut settings, read(&folder));
    assert_eq!(outcome, Ok(None));
    assert_eq!(settings.shader_pack.as_deref(), Some("tester:cel"));
    assert_eq!(
        renderer.given.len(),
        1,
        "the renderer is asked, and it makes nothing of what it has"
    );
}

#[test]
fn an_edited_pack_is_drawn_with_from_then_on_and_the_choice_of_pack_stays() {
    let folder = Folder::new("view_edit");
    pack(&folder, "cel", "tester:cel", LIGHTING);
    let (mut packs, mut settings) = started(&folder, Some("tester:cel"));
    pack(&folder, "cel", "tester:cel", BRIGHTER);
    let mut renderer = FakeRenderer::default();
    let outcome = swap_shader_packs(&mut renderer, &mut packs, &mut settings, read(&folder))
        .expect("accepted")
        .expect("something changed");
    assert_eq!(outcome, "1 shader pack, drawing with `Pack cel`");
    assert_eq!(renderer.given[0].1.as_deref(), Some(BRIGHTER));
    assert_eq!(packs.packs[0].lighting(), BRIGHTER);
    assert_eq!(settings.shader_pack.as_deref(), Some("tester:cel"));
}

#[test]
fn a_pack_the_renderer_refuses_leaves_everything_as_it_was() {
    let folder = Folder::new("view_refused");
    pack(&folder, "cel", "tester:cel", LIGHTING);
    let (mut packs, mut settings) = started(&folder, Some("tester:cel"));
    let before_packs = packs.packs.clone();
    let before_names = settings.shader_packs.clone();
    pack(&folder, "cel", "tester:cel", BRIGHTER);
    pack(&folder, "other", "tester:other", LIGHTING);
    let mut renderer = FakeRenderer {
        refuse: Some("line 4: no such function".to_owned()),
        ..FakeRenderer::default()
    };
    let error = swap_shader_packs(&mut renderer, &mut packs, &mut settings, read(&folder))
        .expect_err("refused");
    assert!(error.contains("line 4: no such function"), "{error}");
    assert!(error.contains("Pack cel"), "{error}");
    assert_eq!(
        packs.packs, before_packs,
        "the packs in use are the old ones"
    );
    assert_eq!(settings.shader_packs, before_names);
    assert_eq!(settings.shader_pack.as_deref(), Some("tester:cel"));
}

#[test]
fn a_chosen_pack_that_is_gone_goes_back_to_the_games_own_shader_and_says_so() {
    let folder = Folder::new("view_gone");
    pack(&folder, "cel", "tester:cel", LIGHTING);
    pack(&folder, "other", "tester:other", LIGHTING);
    let (mut packs, mut settings) = started(&folder, Some("tester:cel"));
    std::fs::remove_dir_all(folder.path().join("cel")).unwrap();
    let mut renderer = FakeRenderer::default();
    let outcome = swap_shader_packs(&mut renderer, &mut packs, &mut settings, read(&folder))
        .expect("accepted")
        .expect("changed");
    assert_eq!(
        outcome,
        "1 shader pack, drawing with the game's own shader (the pack that was chosen is gone)"
    );
    assert_eq!(renderer.given[0].0, None);
    assert_eq!(settings.shader_pack, None);
}

#[test]
fn the_settings_in_the_file_are_what_the_renderer_is_given() {
    let folder = Folder::new("view_settings");
    pack(&folder, "cel", "tester:cel", LIGHTING);
    let (mut packs, mut settings) = started(&folder, Some("tester:cel"));
    folder.write("cel.settings.json", r#"{ "relief": "flat" }"#);
    let mut renderer = FakeRenderer::default();
    let outcome = swap_shader_packs(&mut renderer, &mut packs, &mut settings, read(&folder));
    assert!(matches!(outcome, Ok(Some(_))), "{outcome:?}");
    assert_eq!(
        renderer.given[0].2,
        SettingChoices::from([("relief".to_owned(), SettingValue::Choice("flat".to_owned()))])
    );
    assert_eq!(
        settings.chosen_settings(),
        renderer.given[0].2,
        "the session's choices are the file's"
    );
}

#[test]
fn a_pack_that_is_no_good_is_a_problem_and_the_good_ones_are_used() {
    let folder = Folder::new("view_problem");
    pack(&folder, "cel", "tester:cel", LIGHTING);
    let (mut packs, mut settings) = started(&folder, Some("tester:cel"));
    folder.write("broken/shaderpack.json", "{ not json");
    let mut renderer = FakeRenderer::default();
    let outcome = swap_shader_packs(&mut renderer, &mut packs, &mut settings, read(&folder))
        .expect("accepted")
        .expect("a problem is news");
    assert_eq!(
        outcome,
        "1 shader pack, drawing with `Pack cel`; 1 problem (on the error stream)"
    );
}

#[test]
fn what_the_game_wrote_into_a_settings_file_itself_is_not_a_change() {
    let folder = Folder::new("view_own_write");
    pack(&folder, "cel", "tester:cel", LIGHTING);
    let (mut packs, mut settings) = started(&folder, Some("tester:cel"));
    // The player chooses `.shadersetting relief flat`; the window writes the file in the same frame.
    settings.pack_settings.insert(
        Some("tester:cel".to_owned()),
        SettingChoices::from([("relief".to_owned(), SettingValue::Choice("flat".to_owned()))]),
    );
    let warnings = packs.files.save_changes(&settings.pack_settings);
    assert_eq!(warnings, Vec::<String>::new());
    assert!(folder.path().join("cel.settings.json").exists());
    // The watcher sees the file and a reload reads it: it is what is in use.
    let mut renderer = FakeRenderer::default();
    let outcome = swap_shader_packs(&mut renderer, &mut packs, &mut settings, read(&folder));
    assert_eq!(outcome, Ok(None));
    // And nothing is written again.
    let before = std::fs::read_to_string(folder.path().join("cel.settings.json")).unwrap();
    assert_eq!(
        packs.files.save_changes(&settings.pack_settings),
        Vec::<String>::new()
    );
    assert_eq!(
        std::fs::read_to_string(folder.path().join("cel.settings.json")).unwrap(),
        before
    );
}

#[test]
fn after_a_reload_the_window_does_not_write_back_what_the_player_edited_by_hand() {
    let folder = Folder::new("view_hand_edit");
    pack(&folder, "cel", "tester:cel", LIGHTING);
    let (mut packs, mut settings) = started(&folder, Some("tester:cel"));
    let by_hand = r#"{ "relief":   "flat" }"#;
    folder.write("cel.settings.json", by_hand);
    let mut renderer = FakeRenderer::default();
    swap_shader_packs(&mut renderer, &mut packs, &mut settings, read(&folder)).expect("accepted");
    // The window saves what changed every frame: the file is what it holds already, so it is left as the player wrote it.
    assert_eq!(
        packs.files.save_changes(&settings.pack_settings),
        Vec::<String>::new()
    );
    assert_eq!(
        std::fs::read_to_string(folder.path().join("cel.settings.json")).unwrap(),
        by_hand
    );
}

#[test]
fn a_chosen_pack_that_was_just_broken_is_kept_as_it_was_and_the_problem_is_told() {
    let folder = Folder::new("view_broken_chosen");
    pack(&folder, "cel", "tester:cel", LIGHTING);
    let (mut packs, mut settings) = started(&folder, Some("tester:cel"));
    let before = packs.packs.clone();
    // Seen in a real window: a loop in the pack (not allowed) took the pack off the shelf, and the game went back to its own shader.
    pack(
        &folder,
        "cel",
        "tester:cel",
        "fn pack_shade(s: Surface) -> vec3<f32> { for (var i = 0; i < 3; i++) { } return s.albedo; }",
    );
    let mut renderer = FakeRenderer::default();
    let error = swap_shader_packs(&mut renderer, &mut packs, &mut settings, read(&folder))
        .expect_err("kept as it was");
    assert!(error.contains("`for` is not allowed"), "{error}");
    assert_eq!(packs.packs, before);
    assert_eq!(settings.shader_pack.as_deref(), Some("tester:cel"));
    assert!(renderer.given.is_empty(), "the renderer was not asked");
}
