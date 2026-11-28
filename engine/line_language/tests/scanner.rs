//! Where holes start and end, and what text is left (scripting-spec.md sections 4.1 to 4.3).

use line_language::{
    Hole, Layer, MAXIMUM_DEPTH, MAXIMUM_HOLES, ScanErrorKind, Segment, Sigil, escape, scan,
};
use proptest::prelude::*;
use proptest::strategy::{Strategy, ValueTree};
use proptest::test_runner::{Config, TestRunner};

fn text(value: &str) -> Segment {
    Segment::Text(value.to_owned())
}

fn layer_of(sigil: Sigil, source: &str) -> Layer {
    scan(sigil, source)
        .unwrap_or_else(|error| panic!("{source:?} should scan: {error}"))
        .layer
}

fn hole_in(segment: &Segment) -> &Hole {
    match segment {
        Segment::Hole(hole) => hole,
        other => panic!("expected a hole, found {other:?}"),
    }
}

fn error_of(sigil: Sigil, source: &str) -> line_language::ScanError {
    scan(sigil, source).expect_err(&format!("{source:?} should not scan"))
}

/// What the one hole of `{>` + `rest` holds as code, or `None` when the scanner found no end.
fn code_of_hole(rest: &str) -> Option<String> {
    let line = scan(Sigil::Server, &format!("a {{>{rest}")).ok()?;
    match line.layer.segments.as_slice() {
        [Segment::Text(_), Segment::Hole(hole), ..] => match hole.body.segments.as_slice() {
            [] => Some(String::new()),
            [Segment::Text(code)] => Some(code.clone()),
            other => panic!("code with holes in it: {other:?}"),
        },
        other => panic!("no hole in {other:?}"),
    }
}

#[test]
fn a_line_with_no_hole_is_one_piece_of_text() {
    let line = scan(Sigil::Server, "tp 50 65 12").unwrap();
    assert_eq!(line.sigil, Sigil::Server);
    assert_eq!(line.layer.segments, [text("tp 50 65 12")]);
    assert_eq!(line.holes, 0);
    assert!(scan(Sigil::Server, "").unwrap().layer.segments.is_empty());
}

#[test]
fn the_models_line_of_the_owner_has_a_code_hole_inside_a_command() {
    let source = r#"tp 50 65 {>server.players.by_nickname("Alex").z + 5}"#;
    let layer = layer_of(Sigil::Server, source);
    assert_eq!(layer.segments.len(), 2);
    assert_eq!(layer.segments[0], text("tp 50 65 "));
    let hole = hole_in(&layer.segments[1]);
    assert_eq!(hole.sigil, Sigil::Code);
    assert_eq!(
        hole.body.segments,
        [text(r#"server.players.by_nickname("Alex").z + 5"#)]
    );
    assert_eq!(hole.number, 1);
    assert_eq!(&source[hole.start..hole.end], &source[9..]);
}

#[test]
fn holes_nest_and_are_numbered_by_where_their_brace_opens() {
    let source = "a {/b {.c} {>1}} {.d}";
    let line = scan(Sigil::Server, source).unwrap();
    assert_eq!(line.holes, 4);
    let outer = hole_in(&line.layer.segments[1]);
    assert_eq!((outer.sigil, outer.number), (Sigil::Server, 1));
    let inner = hole_in(&outer.body.segments[1]);
    assert_eq!((inner.sigil, inner.number), (Sigil::Client, 2));
    let code = hole_in(&outer.body.segments[3]);
    assert_eq!((code.sigil, code.number), (Sigil::Code, 3));
    let last = hole_in(&line.layer.segments[3]);
    assert_eq!((last.sigil, last.number), (Sigil::Client, 4));
    assert_eq!(&source[inner.start..inner.end], "{.c}");
    assert_eq!(&source[outer.start..outer.end], "{/b {.c} {>1}}");
    assert_eq!(&source[last.start..last.end], "{.d}");
}

#[test]
fn a_hole_says_the_character_it_starts_at_counting_the_sigil_as_the_first() {
    let line = scan(Sigil::Server, "a {/b} {.c}").unwrap();
    assert_eq!(hole_in(&line.layer.segments[1]).column, 4);
    assert_eq!(hole_in(&line.layer.segments[3]).column, 9);
    let wide = scan(Sigil::Server, "é {/b}").unwrap();
    assert_eq!(hole_in(&wide.layer.segments[1]).column, 4);
}

#[test]
fn a_brace_not_followed_by_a_sigil_is_plain_text_and_a_bare_one_can_close_a_hole() {
    assert_eq!(
        layer_of(Sigil::Server, "say {hi} {").segments,
        [text("say {hi} {")]
    );
    assert_eq!(layer_of(Sigil::Server, "a } b").segments, [text("a } b")]);
    // `{hi` inside a hole is plain, so the next `}` ends the hole, as the spec says
    let layer = layer_of(Sigil::Server, "a {/b {hi} c");
    assert_eq!(layer.segments.len(), 3);
    assert_eq!(hole_in(&layer.segments[1]).body.segments, [text("b {hi")]);
    assert_eq!(layer.segments[2], text(" c"));
}

#[test]
fn the_three_escapes_are_decoded_once_and_nothing_else_is() {
    assert_eq!(
        layer_of(Sigil::Server, r"a \\ b").segments,
        [text(r"a \ b")]
    );
    assert_eq!(
        layer_of(Sigil::Server, r"\{>1+1\}").segments,
        [text("{>1+1}")]
    );
    assert_eq!(layer_of(Sigil::Server, r"\{\{ \}").segments, [text("{{ }")]);
    // every other backslash stays, a trailing one too
    assert_eq!(
        layer_of(Sigil::Server, r"a\nb \x \").segments,
        [text(r"a\nb \x \")]
    );
    // an escaped brace does not close a hole and does not open one
    let layer = layer_of(Sigil::Server, r"a {/b \} c}");
    assert_eq!(hole_in(&layer.segments[1]).body.segments, [text("b } c")]);
    // \\ then a brace: the backslash is spent on the backslash, so the brace opens a hole
    let layer = layer_of(Sigil::Server, r"\\{/b}");
    assert_eq!(layer.segments[0], text(r"\"));
    assert_eq!(hole_in(&layer.segments[1]).sigil, Sigil::Server);
}

#[test]
fn a_brace_and_a_star_are_kept_for_unpacking_and_are_an_error() {
    for source in ["a {*b}", "a {**b}", "a {/b {*c}}"] {
        let error = error_of(Sigil::Server, source);
        assert_eq!(error.kind, ScanErrorKind::ReservedBrace, "{source}");
    }
    assert_eq!(error_of(Sigil::Server, "ab {*c}").column, 5);
    // escaped, it is text
    assert_eq!(
        layer_of(Sigil::Server, r"a \{*b\}").segments,
        [text("a {*b}")]
    );
}

#[test]
fn a_hole_that_is_not_closed_says_where_it_opened() {
    for (source, column) in [
        ("a {/b", 4),
        ("a {/b {.c}", 4),
        ("a {.b {>c", 8),
        ("{>1", 2),
    ] {
        let error = error_of(Sigil::Server, source);
        assert_eq!(error.kind, ScanErrorKind::Unclosed, "{source}");
        assert_eq!(error.column, column, "{source}");
    }
}

#[test]
fn holes_nested_too_deep_and_too_many_holes_are_refused() {
    let nested = |levels: usize| format!("{}x{}", "{/".repeat(levels), "}".repeat(levels));
    assert!(scan(Sigil::Server, &nested(MAXIMUM_DEPTH)).is_ok());
    let error = error_of(Sigil::Server, &nested(MAXIMUM_DEPTH + 1));
    assert_eq!(error.kind, ScanErrorKind::TooDeep);
    assert_eq!(error.column, 2 + 2 * MAXIMUM_DEPTH);
    let many = |count: usize| "{/x} ".repeat(count);
    assert_eq!(
        scan(Sigil::Server, &many(MAXIMUM_HOLES)).unwrap().holes,
        MAXIMUM_HOLES
    );
    assert_eq!(
        error_of(Sigil::Server, &many(MAXIMUM_HOLES + 1)).kind,
        ScanErrorKind::TooManyHoles
    );
}

#[test]
fn a_client_hole_is_found_in_a_client_line_and_a_code_line() {
    let line = scan(Sigil::Client, "ambient {.time}").unwrap();
    assert_eq!(line.sigil, Sigil::Client);
    assert_eq!(hole_in(&line.layer.segments[1]).sigil, Sigil::Client);
    let code = scan(Sigil::Code, "x = {.who} + 1").unwrap();
    assert_eq!(code.sigil, Sigil::Code);
    assert_eq!(code.layer.segments.len(), 3);
    assert_eq!(code.layer.segments[0], text("x = "));
    assert_eq!(hole_in(&code.layer.segments[1]).sigil, Sigil::Client);
    assert_eq!(code.layer.segments[2], text(" + 1"));
}

// ---- code ----

#[test]
fn code_holes_end_where_python_would_end_the_expression() {
    // the corner cases of spike S2, with the end each tokenizer agreed on (docs/history/spikes/scripting)
    let cases: &[(&str, Option<usize>)] = &[
        (r#"{"a": 1}["a"]}"#, Some(13)),
        (r#"{1, 2} | {3}} tail"#, Some(12)),
        (r#""}"}"#, Some(3)),
        (r#"f"{'}'}"}"#, Some(8)),
        (r#"f"{x:{'}'}}"}"#, Some(12)),
        (r#".5}"#, Some(2)),
        (r#"{.5, .25}}"#, Some(9)),
        (r#"{...}}"#, Some(5)),
        (r#"'it\'s}'}"#, Some(8)),
        (r#"r'\d}'}"#, Some(6)),
        (r##""""a}"b"""}"##, Some(10)),
    ];
    for (rest, end) in cases {
        let expected = end.map(|end| rest[..end].to_owned());
        assert_eq!(code_of_hole(rest), expected, "{rest:?}");
    }
    assert_eq!(code_of_hole(r#""never closed}"#), None);
    assert_eq!(code_of_hole("(1, 2"), None);
}

#[test]
fn a_hash_is_an_ordinary_character_in_a_code_hole() {
    assert_eq!(code_of_hole("1 # two}"), Some("1 # two".to_owned()));
}

#[test]
fn blanks_after_the_sigil_of_code_are_dropped() {
    assert_eq!(code_of_hole("  1+1}"), Some("1+1".to_owned()));
    assert_eq!(code_of_hole("\t1+1 }"), Some("1+1 ".to_owned()));
    let line = scan(Sigil::Code, "   1+1").unwrap();
    assert_eq!(line.layer.segments, [text("1+1")]);
}

#[test]
fn a_dot_and_a_letter_in_code_is_a_client_hole_and_a_dot_and_a_number_is_a_set() {
    let layer = layer_of(Sigil::Server, "a {>f({.who}, {.5}, {...})}");
    let code = hole_in(&layer.segments[1]);
    assert_eq!(code.body.segments.len(), 3);
    assert_eq!(code.body.segments[0], text("f("));
    let client = hole_in(&code.body.segments[1]);
    assert_eq!((client.sigil, client.number), (Sigil::Client, 2));
    assert_eq!(code.body.segments[2], text(", {.5}, {...})"));
}

#[test]
fn a_client_hole_inside_a_string_of_code_is_text() {
    let layer = layer_of(Sigil::Server, r#"a {>f("{.who}", '{.x}', f"{y}")}"#);
    let code = hole_in(&layer.segments[1]);
    assert_eq!(code.body.segments, [text(r#"f("{.who}", '{.x}', f"{y}")"#)]);
}

#[test]
fn a_server_hole_or_a_code_hole_inside_code_is_refused() {
    for source in ["a {>{/b}}", "a {>1 + {>2}}", ">{/x}", ">f({>1})"] {
        let sigil = if source.starts_with('>') {
            Sigil::Code
        } else {
            Sigil::Server
        };
        let source = source.strip_prefix('>').unwrap_or(source);
        let error = scan(sigil, source).expect_err(source);
        assert!(
            matches!(error.kind, ScanErrorKind::ReservedInCode(_)),
            "{source}: {error:?}"
        );
    }
    // but not inside a string, and not when escaped
    assert!(scan(Sigil::Code, r#""{/x} {>y}""#).is_ok());
    assert_eq!(layer_of(Sigil::Code, r"\{/x\}").segments, [text("{/x}")]);
}

#[test]
fn escapes_in_code_are_decoded_outside_strings_and_left_alone_inside() {
    assert_eq!(
        layer_of(Sigil::Code, r"\{1, 2\}").segments,
        [text("{1, 2}")]
    );
    assert_eq!(
        layer_of(Sigil::Code, r#"len("a\\b\{") + 1"#).segments,
        [text(r#"len("a\\b\{") + 1"#)]
    );
    // an escaped closing brace does not end the hole
    assert_eq!(code_of_hole(r"\{1\}}"), Some("{1}".to_owned()));
}

/// Complete Python expressions whose text is full of braces, quotes and backslashes (the generator of spike S2).
fn expression() -> impl Strategy<Value = String> {
    let leaf = prop_oneof![
        Just("1".to_owned()),
        Just("3.5".to_owned()),
        Just("x".to_owned()),
        Just("None".to_owned()),
        Just(r#""}""#.to_owned()),
        Just(r#"'{'"#.to_owned()),
        Just(r#""a{b}c""#.to_owned()),
        Just(r#"'it\'s}'"#.to_owned()),
        Just(r#""\"}""#.to_owned()),
        Just(r#"r'\d}'"#.to_owned()),
        Just(r#"b'}'"#.to_owned()),
        Just(r#""""a}"b""""#.to_owned()),
        Just(r#"'''}'''"#.to_owned()),
        Just(r#"f"{x}""#.to_owned()),
        Just(r#"f'{x!r}'"#.to_owned()),
        Just(r#"f"{x:>{w}}""#.to_owned()),
        Just(r#"f"{{}}""#.to_owned()),
        Just(r#"f"{ {1: 2}[1] }""#.to_owned()),
        Just(r#"f"{'}'}""#.to_owned()),
        Just(r#"f'{"}"}'"#.to_owned()),
        Just(r#"f"{x}}}""#.to_owned()),
        Just(r#"f"{{{x}}}""#.to_owned()),
        Just(r#"f"{x=}""#.to_owned()),
        Just(r#"f"{x:{'>'}{w}}""#.to_owned()),
        Just(r#"rf'\{x}'"#.to_owned()),
        Just(r#"f'\N{BULLET}'"#.to_owned()),
        Just(r#"f"{f'{x}'}""#.to_owned()),
        Just(r#"f"{x["a"]}""#.to_owned()),
        Just(r#"f"{{""#.to_owned()),
        Just(r#"f"}}""#.to_owned()),
        Just(r#"f"a}}{{b""#.to_owned()),
        Just(r#"f"{x["}"]}""#.to_owned()),
        Just(r#"f'{d['}']}'"#.to_owned()),
        Just(r#"f"{'a' if x else "}"}""#.to_owned()),
    ];
    leaf.prop_recursive(4, 24, 3, |inner| {
        prop_oneof![
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("({a} + {b})")),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("[{a}, {b}]")),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("{{{a}: {b}}}")),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("{{{a}, {b}}}")),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("f({a}, {b})")),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("{a}[{b}]")),
            (inner.clone(), inner.clone(), inner.clone())
                .prop_map(|(a, b, c)| format!("{a}[{b}:{c}]")),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("{{k: {a} for k in {b}}}")),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("{{{a} for _ in {b}}}")),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("(lambda t: {a})({b})")),
            (inner.clone(), inner.clone(), inner.clone())
                .prop_map(|(a, b, c)| format!("({a} if {b} else {c})")),
            inner.clone().prop_map(|a| format!("({a}).attr")),
            inner.clone().prop_map(|a| format!("x.get({a})")),
            inner.prop_map(|a| format!("{a}  ")),
        ]
    })
}

proptest! {
    #![proptest_config(Config { cases: 3000, .. Config::default() })]

    /// The hole ends exactly after a complete expression, whatever braces and quotes it holds and
    /// whatever plain text surrounds the hole.
    #[test]
    fn a_code_hole_ends_after_a_complete_expression(expression in expression()) {
        let line = scan(Sigil::Server, &format!("before {{>{expression}}} after")).unwrap();
        prop_assert_eq!(line.holes, 1);
        let segments = &line.layer.segments;
        prop_assert_eq!(segments.len(), 3);
        prop_assert_eq!(&segments[0], &text("before "));
        prop_assert_eq!(&hole_in(&segments[1]).body.segments, &vec![text(&expression)]);
        prop_assert_eq!(&segments[2], &text(" after"));
    }
}

/// The mutation check of the spike: a scanner that only counts braces must fail the property above, or the property is blind.
#[test]
fn a_scanner_that_only_counts_braces_would_be_caught_by_the_generator() {
    let mut runner = TestRunner::deterministic();
    let strategy = expression();
    let total = 2000;
    let wrong = (0..total)
        .filter(|_| {
            let expression = strategy.new_tree(&mut runner).unwrap().current();
            let mut depth = 0usize;
            let end = format!("{expression}}}")
                .bytes()
                .position(|byte| match byte {
                    b'{' => {
                        depth += 1;
                        false
                    }
                    b'}' if depth == 0 => true,
                    b'}' => {
                        depth -= 1;
                        false
                    }
                    _ => false,
                });
            end != Some(expression.len())
        })
        .count();
    assert!(
        wrong > 100,
        "the brace counter was wrong on only {wrong} of {total}"
    );
}

// ---- properties of any text ----

fn walk<'a>(layer: &'a Layer, parent: Option<&Hole>, found: &mut Vec<&'a Hole>) {
    for segment in &layer.segments {
        if let Segment::Hole(hole) = segment {
            if let Some(parent) = parent {
                assert!(parent.start < hole.start && hole.end < parent.end);
            }
            found.push(hole);
            walk(&hole.body, Some(hole), found);
        }
    }
}

proptest! {
    /// Whatever is typed, scanning does not panic; when it succeeds the holes lie inside the text, inside each other, and are
    /// numbered by where they open.
    #[test]
    fn any_text_scans_or_says_why_not(
        source in r#"[ a-z{}()\[\]'"\\#>/.*:!é{}/.>]{0,40}"#,
        sigil in prop_oneof![Just(Sigil::Server), Just(Sigil::Client), Just(Sigil::Code)],
    ) {
        if let Ok(line) = scan(sigil, &source) {
            let mut found = Vec::new();
            walk(&line.layer, None, &mut found);
            prop_assert_eq!(found.len(), line.holes);
            let mut by_start: Vec<_> = found.iter().map(|hole| (hole.start, hole.number)).collect();
            by_start.sort_unstable();
            let numbers: Vec<usize> = by_start.iter().map(|&(_, number)| number).collect();
            prop_assert_eq!(numbers, (1..=line.holes).collect::<Vec<_>>());
            for hole in found {
                prop_assert!(source.is_char_boundary(hole.start) && source.is_char_boundary(hole.end));
                prop_assert!(source[hole.start..].starts_with('{'), "a hole starts at its brace");
                prop_assert!(source[..hole.end].ends_with('}'), "a hole ends after its brace");
            }
        }
    }

    /// Text with no brace and no backslash is one piece of text, unchanged.
    #[test]
    fn plain_text_is_unchanged(source in "[^{}\\\\]{1,60}") {
        for sigil in [Sigil::Server, Sigil::Client, Sigil::Code] {
            let layer = scan(sigil, &source).unwrap().layer;
            // code drops the blanks after its sigil, as it is documented to
            let expected = if sigil == Sigil::Code { source.trim_start() } else { &source };
            if expected.is_empty() {
                prop_assert!(layer.segments.is_empty());
            } else {
                prop_assert_eq!(layer.segments, vec![text(expected)]);
            }
        }
    }

    /// Escaping text and scanning it gives the text back, however many braces and backslashes it has: this is what lets a client
    /// put its own result into a line without that result ever being read as a hole.
    #[test]
    fn escaped_text_scans_back_to_itself(source in r#"[ a-z{}()\\\/.>*é#]{1,40}"#) {
        for sigil in [Sigil::Server, Sigil::Client] {
            let layer = scan(sigil, &escape(&source)).unwrap().layer;
            prop_assert_eq!(layer.segments, vec![text(&source)]);
        }
    }
}

#[test]
fn escape_puts_a_backslash_before_a_backslash_and_each_brace() {
    assert_eq!(escape(r"a{b}c\d"), r"a\{b\}c\\d");
    assert_eq!(escape("plain"), "plain");
}
