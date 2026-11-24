//! What the window does with a line before it sends it: runs the `{.}` holes and puts their results in as text the server
//! will not read as holes (scripting-spec.md 4.2, 4.3 and 4.5).

use std::collections::HashMap;

use line_language::{
    Call, CommandValue, HoleName, MAXIMUM_RESULT_LENGTH, Runner, Segment, Sigil,
    resolve_client_holes, scan,
};
use proptest::prelude::*;

/// The window's commands: each answers with the text it was given for it, and the others are refused as the window refuses them.
struct Window {
    answers: HashMap<&'static str, CommandValue>,
    run: Vec<String>,
}

impl Window {
    fn new(answers: &[(&'static str, CommandValue)]) -> Self {
        Self {
            answers: answers.iter().cloned().collect(),
            run: Vec::new(),
        }
    }
}

impl Runner for Window {
    fn command(&mut self, sigil: Sigil, call: &Call) -> Result<CommandValue, String> {
        if sigil != Sigil::Client {
            return Err("the window cannot wait for the server".to_owned());
        }
        self.run.push(call.name.clone());
        self.answers
            .get(call.name.as_str())
            .cloned()
            .ok_or_else(|| format!("unknown client command .{}", call.name))
    }

    fn code(&mut self, _: &str) -> Result<CommandValue, String> {
        Err("the window cannot run code".to_owned())
    }
}

fn text(value: &str) -> CommandValue {
    CommandValue::Text(value.to_owned())
}

fn resolve(
    sigil: Sigil,
    source: &str,
    window: &mut Window,
) -> Result<String, line_language::Failure> {
    resolve_client_holes(&scan(sigil, source).unwrap(), source, window)
}

#[test]
fn a_line_with_no_client_hole_is_sent_exactly_as_it_was_typed() {
    let mut window = Window::new(&[]);
    for source in [
        "tp 1 2 3",
        r"say \{x\} \\ {hi} } {",
        "a {/b {/c}} d",
        r#"a {>f("}", {1: 2})} é"#,
        "  spaced   out  ",
    ] {
        assert_eq!(resolve(Sigil::Server, source, &mut window).unwrap(), source);
    }
    assert!(window.run.is_empty());
}

#[test]
fn a_client_hole_is_replaced_by_what_the_command_says() {
    let mut window = Window::new(&[("who", text("Alex")), ("n", CommandValue::Integer(7))]);
    assert_eq!(
        resolve(Sigil::Server, "say {.who} has {.n}!", &mut window).unwrap(),
        "say Alex has 7!"
    );
    assert_eq!(window.run, ["who", "n"]);
}

#[test]
fn a_result_goes_in_escaped_so_the_server_reads_it_as_text() {
    let mut window = Window::new(&[("odd", text(r"{/boom} {>1} \ }"))]);
    let sent = resolve(Sigil::Server, "say {.odd}", &mut window).unwrap();
    assert_eq!(sent, r"say \{/boom\} \{>1\} \\ \}");
    // what the server's scanner makes of it is the result, as text
    let layer = scan(Sigil::Server, &sent).unwrap().layer;
    assert_eq!(
        layer.segments,
        [Segment::Text(r"say {/boom} {>1} \ }".to_owned())]
    );
}

#[test]
fn client_holes_inside_server_holes_and_code_are_found_and_the_server_holes_are_left_alone() {
    let mut window = Window::new(&[("who", text("A{b}")), ("n", CommandValue::Integer(3))]);
    let sent = resolve(
        Sigil::Server,
        r#"x {/y {.who} \{ {/z {.n}}} {>f({.who}, {.n}, "{.n}")}"#,
        &mut window,
    )
    .unwrap();
    assert_eq!(
        sent, r#"x {/y A\{b\} \{ {/z 3}} {>f('A{b}', 3, "{.n}")}"#,
        "in commands the result is escaped text, in code it is a Python literal, in a string of code it is not a hole at all"
    );
}

#[test]
fn a_hole_inside_a_client_hole_runs_first_and_only_the_outer_one_is_replaced() {
    let mut window = Window::new(&[("inner", text("who")), ("outer", text("Alex"))]);
    let sent = resolve(Sigil::Server, "say {.outer {.inner}}", &mut window).unwrap();
    assert_eq!(sent, "say Alex");
    assert_eq!(window.run, ["inner", "outer"]);
}

#[test]
fn a_code_line_gets_its_client_holes_as_literals() {
    let mut window = Window::new(&[("who", text("Alex")), ("n", CommandValue::Float(0.5))]);
    assert_eq!(
        resolve(Sigil::Code, "name = {.who}; x = {.n} + 1", &mut window).unwrap(),
        "name = 'Alex'; x = 0.5 + 1"
    );
}

#[test]
fn a_server_hole_inside_a_client_hole_fails_and_names_the_hole() {
    let mut window = Window::new(&[("who", text("x"))]);
    let failure = resolve(Sigil::Server, "say {.who {/x}}", &mut window).unwrap_err();
    assert_eq!(
        failure.hole,
        Some(HoleName {
            number: 2,
            column: 12
        })
    );
    assert!(failure.to_string().contains("cannot wait for the server"));
}

#[test]
fn a_client_command_that_is_unknown_fails_the_line_and_sends_nothing() {
    let mut window = Window::new(&[]);
    let failure = resolve(Sigil::Server, "say {.nope}", &mut window).unwrap_err();
    assert_eq!(
        failure.to_string(),
        "hole 1 (at character 6): unknown client command .nope"
    );
}

#[test]
fn control_characters_in_a_result_are_dropped() {
    let mut window = Window::new(&[("odd", text("a\nb\u{7}c\td"))]);
    assert_eq!(
        resolve(Sigil::Server, "say {.odd}", &mut window).unwrap(),
        "say abcd"
    );
}

#[test]
fn the_line_the_holes_make_must_still_fit_in_a_line() {
    let big = text(&"{".repeat(MAXIMUM_RESULT_LENGTH));
    let mut window = Window::new(&[("big", big)]);
    // each brace is escaped, so the line is twice as long as the result: too long
    let failure = resolve(Sigil::Server, "say {.big}", &mut window).unwrap_err();
    assert!(failure.message.contains("longer"), "{}", failure.message);
}

proptest! {
    /// Whatever a client command says, the line the server gets holds no hole that the result made: scanning it finds the
    /// holes that were typed, and the result is text between them, as it was.
    #[test]
    fn a_result_never_becomes_a_hole(result in r#"[ a-z{}()\\\/.>*é#'"]{0,60}"#) {
        let mut window = Window::new(&[("r", text(&result))]);
        let sent = resolve(Sigil::Server, "say {.r} and {/typed}", &mut window).unwrap();
        let line = scan(Sigil::Server, &sent).unwrap();
        prop_assert_eq!(line.holes, 1);
        prop_assert_eq!(&line.layer.segments[0], &Segment::Text(format!("say {result} and ")));
    }
}

#[test]
fn the_longest_line_there_may_be_counts_its_sigil() {
    let mut window = Window::new(&[("a_lot", text(&"a".repeat(1000)))]);
    // the sigil and 23 more characters and the result make 1,024 bytes: allowed, and one byte more is not
    let fits = format!("{}{{.a_lot}}", "b".repeat(23));
    assert_eq!(
        resolve(Sigil::Server, &fits, &mut window).unwrap().len() + 1,
        1024
    );
    let too_long = format!("{}{{.a_lot}}", "b".repeat(24));
    let failure = resolve(Sigil::Server, &too_long, &mut window).unwrap_err();
    assert!(
        failure.message.contains("1025 bytes"),
        "{}",
        failure.message
    );
}
