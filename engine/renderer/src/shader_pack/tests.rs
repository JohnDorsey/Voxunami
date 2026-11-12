use super::*;

/// A pack that is the default's equation with the sun's light halved: it reads the frame, defines a helper function and a constant of its own.
const HALVED_SUN: &str = "
const SUN_SHARE: f32 = 0.5;

fn screen(first: f32, second: f32) -> f32 {
    return first + second * (1.0 - first);
}

fn pack_shade(s: Surface) -> vec3<f32> {
    let sun = frame.sun_light.x * SUN_SHARE * max(dot(s.normal, frame.sun_direction.xyz), 0.0) * s.sun_visible;
    return s.albedo * screen(s.side * s.lamp, sun);
}
";

fn problems_of(text: &str) -> Vec<ShaderPackProblem> {
    ShaderPack::new("test:pack", "Test", text)
        .expect_err("the pack is refused")
        .problems
}

#[test]
fn the_default_lighting_is_a_pack_the_checks_accept_and_so_is_one_that_reads_the_frame_and_has_functions_of_its_own()
 {
    assert_eq!(check_lighting(DEFAULT_LIGHTING), Ok(()));
    let pack = ShaderPack::new("test:halved", "Halved sun", HALVED_SUN).expect("a valid pack");
    assert_eq!(pack.id(), "test:halved");
    assert_eq!(pack.name(), "Halved sun");
    assert_eq!(pack.lighting(), HALVED_SUN);
}

#[test]
fn a_pack_that_does_not_define_pack_shade_is_refused_whole() {
    let problems = problems_of("fn something_else(s: Surface) -> vec3<f32> { return s.albedo; }");
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].line, None);
    assert!(
        problems[0].message.contains("does not define"),
        "{problems:?}"
    );
}

#[test]
fn a_mistake_in_the_pack_is_found_at_its_line_of_the_pack_whatever_the_engine_has_before_it() {
    // A syntax error on the fourth line (the comment is the first).
    let text = "// a pack\nfn pack_shade(s: Surface) -> vec3<f32> {\n    let a = 1.0;\n    return s.albedo +;\n}\n";
    let problems = problems_of(text);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].line, Some(4), "{problems:?}");
    // A mistake of types, on the third line.
    let text = "fn pack_shade(s: Surface) -> vec3<f32> {\n    let a = 1.0;\n    let b: f32 = vec3<f32>(a);\n    return s.albedo;\n}\n";
    let problems = problems_of(text);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].line, Some(3), "{problems:?}");
    // An unknown name, on the second.
    let text =
        "fn pack_shade(s: Surface) -> vec3<f32> {\n    return s.albedo * nothing_called_this;\n}\n";
    assert_eq!(problems_of(text)[0].line, Some(2));
}

#[test]
fn a_pack_whose_pack_shade_does_not_fit_the_engines_call_is_refused_and_told_what_to_write() {
    for text in [
        "fn pack_shade(s: Surface) -> f32 { return 1.0; }",
        "fn pack_shade(s: f32) -> vec3<f32> { return vec3<f32>(s); }",
        "fn pack_shade() -> vec3<f32> { return vec3<f32>(1.0); }",
    ] {
        let problems = problems_of(text);
        assert_eq!(problems.len(), 1, "{text}: {problems:?}");
        assert!(
            problems[0]
                .message
                .contains("fn pack_shade(s: Surface) -> vec3<f32>"),
            "{text}: {problems:?}"
        );
    }
}

#[test]
fn a_pack_that_defines_a_name_the_engine_has_is_refused() {
    let text = "fn sunlight(a: f32) -> f32 { return a; }\nfn pack_shade(s: Surface) -> vec3<f32> { return s.albedo; }";
    let problems = problems_of(text);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].line, Some(1), "{problems:?}");
}

#[test]
fn loops_are_not_allowed_and_every_one_is_named_with_its_place() {
    let text = "fn pack_shade(s: Surface) -> vec3<f32> {
    var total = 0.0;
    for (var i = 0u; i < 4u; i = i + 1u) { total = total + 1.0; }
    loop { break; }
    while (total < 3.0) { total = total + 1.0; }
    return s.albedo * total;
}";
    let problems = problems_of(text);
    let found: Vec<(Option<u32>, Option<u32>)> = problems
        .iter()
        .map(|problem| (problem.line, problem.column))
        .collect();
    assert_eq!(
        found,
        [(Some(3), Some(5)), (Some(4), Some(5)), (Some(5), Some(5))],
        "{problems:?}"
    );
    assert!(
        problems
            .iter()
            .all(|problem| problem.message.contains("hangs"))
    );
}

#[test]
fn a_binding_a_variable_of_its_own_and_the_engines_names_are_not_allowed_and_a_comment_may_say_them()
 {
    let text = "// the shadow_maps are not for a pack, nor is a loop or a texture
/* palette, @binding(3), textureSample, for */
@group(2) @binding(9) var<uniform> mine: vec4<f32>;
var<private> counter: f32;
fn pack_shade(s: Surface) -> vec3<f32> {
    let a = textureSample(pictures_0, picture_sampler, vec2<f32>(0.0));
    let b = palette[0];
    return s.albedo * a.x * b.x;
}";
    let problems = problems_of(text);
    let lines: Vec<u32> = problems.iter().filter_map(|problem| problem.line).collect();
    // Line 3: `@group`, `@binding` and `var<`; line 4: `var<`; line 6: textureSample, pictures_0, picture_sampler; line 7: palette. Nothing from the comments (lines 1 and 2).
    assert_eq!(lines, [3, 3, 3, 4, 6, 6, 6, 7], "{problems:?}");
    assert!(problems.iter().all(|problem| problem.column.is_some()));
}

#[test]
fn a_problem_says_where_it_is_and_the_error_says_all_of_them() {
    let problems = problems_of(
        "fn pack_shade(s: Surface) -> vec3<f32> {\n    for (;;) {}\n    return s.albedo;\n}",
    );
    assert_eq!(
        problems[0].to_string(),
        format!("line 2, column 5: {}", problems[0].message)
    );
    let error = ShaderPack::new("test:pack", "Test", "fn pack_shade(s: Surface) -> vec3<f32> {\n    loop {}\n    while (true) {}\n    return s.albedo;\n}").unwrap_err();
    let shown = error.to_string();
    assert!(
        shown.contains("line 2, column 5") && shown.contains("line 3, column 5"),
        "{shown}"
    );
    assert_eq!(shown.lines().count(), 2);
}

#[test]
fn comments_are_blanked_in_place_so_that_the_lines_and_columns_of_the_rest_do_not_move() {
    let blank = without_comments("a // b\nc /* d\ne */ f\n/* /* nested */ still */ g");
    // (The last comment is 24 characters, and a space and the `g` follow it.)
    assert_eq!(
        blank,
        format!("a     \nc     \n     f\n{}g", " ".repeat(25))
    );
    assert_eq!(blank.lines().count(), 4);
    assert_eq!(without_comments("no comment"), "no comment");
}

#[test]
fn a_name_that_only_a_heavier_kind_of_the_engines_shader_has_is_refused_too() {
    // `walk_window` is the walk of the sun's ray (`contact_on.wgsl`): the plainest shader has none, and a pack that defines one is checked against the heaviest as well.
    let text = "fn walk_window(a: f32) -> f32 { return a; }\nfn pack_shade(s: Surface) -> vec3<f32> { return s.albedo; }";
    let problems = problems_of(text);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].line, Some(1), "{problems:?}");
}
