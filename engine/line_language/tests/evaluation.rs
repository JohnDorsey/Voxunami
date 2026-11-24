//! How a line is evaluated: the order, what a hole is worth where, and what an error says (scripting-spec.md 4.4 and 4.5).

use std::collections::HashMap;

use line_language::{
    Call, CommandValue, HoleName, Item, MAXIMUM_LINE_LENGTH, MAXIMUM_RESULT_LENGTH, Runner, Sigil,
    evaluate, scan,
};

/// A runner that answers by the name of the command, and writes down what it was asked.
#[derive(Default)]
struct Script {
    asked: Vec<(Sigil, Call)>,
    code: Vec<String>,
    answers: HashMap<&'static str, Result<CommandValue, String>>,
}

impl Script {
    fn answer(mut self, name: &'static str, value: CommandValue) -> Self {
        self.answers.insert(name, Ok(value));
        self
    }

    fn refuse(mut self, name: &'static str, message: &str) -> Self {
        self.answers.insert(name, Err(message.to_owned()));
        self
    }

    fn names(&self) -> Vec<&str> {
        self.asked
            .iter()
            .map(|(_, call)| call.name.as_str())
            .collect()
    }

    fn call(&self, name: &str) -> &Call {
        &self
            .asked
            .iter()
            .find(|(_, call)| call.name == name)
            .unwrap_or_else(|| panic!("{name} was never run: {:?}", self.names()))
            .1
    }
}

impl Runner for Script {
    fn command(&mut self, sigil: Sigil, call: &Call) -> Result<CommandValue, String> {
        self.asked.push((sigil, call.clone()));
        self.answers
            .get(call.name.as_str())
            .cloned()
            .unwrap_or(Ok(CommandValue::None))
    }

    fn code(&mut self, code: &str) -> Result<CommandValue, String> {
        self.code.push(code.to_owned());
        Ok(CommandValue::Integer(2))
    }
}

fn run(
    sigil: Sigil,
    source: &str,
    script: &mut Script,
) -> Result<CommandValue, line_language::Failure> {
    evaluate(&scan(sigil, source).unwrap(), script)
}

fn text(value: &str) -> CommandValue {
    CommandValue::Text(value.to_owned())
}

#[test]
fn holes_run_innermost_first_and_left_to_right_and_the_outer_command_last() {
    let mut script = Script::default();
    run(Sigil::Server, "x {/a {/b}} {/c {/d} {/e}}", &mut script).unwrap();
    assert_eq!(script.names(), ["b", "a", "d", "e", "c", "x"]);
    assert!(
        script
            .asked
            .iter()
            .all(|(sigil, _)| *sigil == Sigil::Server)
    );
}

#[test]
fn a_command_is_its_name_and_its_words() {
    let mut script = Script::default();
    run(Sigil::Server, "  tp   1  2.5\t-3 ", &mut script).unwrap();
    let call = script.call("tp");
    let words: Vec<&str> = call.items.iter().map(|item| item.text.as_str()).collect();
    assert_eq!(words, ["1", "2.5", "-3"]);
    assert!(
        call.items
            .iter()
            .all(|item| !item.has_hole && item.whole.is_none())
    );
    // no words at all is a call with no name
    let mut script = Script::default();
    run(Sigil::Server, "  ", &mut script).unwrap();
    assert_eq!(
        script.asked[0].1,
        Call {
            name: String::new(),
            items: Vec::new()
        }
    );
}

#[test]
fn a_hole_that_is_a_whole_word_keeps_its_value_and_one_joined_to_text_is_text() {
    let mut script = Script::default()
        .answer("n", CommandValue::Integer(5))
        .answer("who", text("Alex"));
    run(
        Sigil::Server,
        "x {/n} a{/n} {/n}{/who} {/who}b {/n}",
        &mut script,
    )
    .unwrap();
    let items = &script.call("x").items;
    assert_eq!(items.len(), 5);
    assert_eq!(items[0].whole, Some(CommandValue::Integer(5)));
    assert_eq!(items[0].text, "5");
    assert!(items[0].has_hole);
    assert_eq!(
        (items[1].text.as_str(), items[1].whole.as_ref()),
        ("a5", None)
    );
    assert_eq!(
        (items[2].text.as_str(), items[2].whole.as_ref()),
        ("5Alex", None)
    );
    assert_eq!(
        (items[3].text.as_str(), items[3].whole.as_ref()),
        ("Alexb", None)
    );
    assert!(items[1].has_hole && items[2].has_hole && items[3].has_hole);
    assert_eq!(items[4].whole, Some(CommandValue::Integer(5)));
}

#[test]
fn what_a_hole_gives_is_written_the_way_python_prints_it() {
    let mut script = Script::default()
        .answer("none", CommandValue::None)
        .answer("yes", CommandValue::Bool(true))
        .answer("float", CommandValue::Float(2.0))
        .answer(
            "list",
            CommandValue::List(vec![CommandValue::Integer(1), text("a")]),
        );
    run(
        Sigil::Server,
        "x a{/none} a{/yes} a{/float} a{/list}",
        &mut script,
    )
    .unwrap();
    let words: Vec<&str> = script
        .call("x")
        .items
        .iter()
        .map(|item| item.text.as_str())
        .collect();
    assert_eq!(words, ["aNone", "aTrue", "a2.0", "a[1, 'a']"]);
}

#[test]
fn a_value_with_spaces_is_one_value_and_its_words_are_for_whoever_takes_the_rest() {
    let mut script = Script::default().answer("pair", text("1 2"));
    run(Sigil::Server, "x {/pair} y{/pair}", &mut script).unwrap();
    let items = &script.call("x").items;
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].whole, Some(text("1 2")));
    assert_eq!(items[0].text, "1 2");
    assert_eq!(items[1].text, "y1 2");
}

#[test]
fn a_result_is_never_scanned_again() {
    let tricky = r"{/boom} {>1} {.x} \{ \} \\ }";
    let mut script = Script::default().answer("tricky", text(tricky));
    run(Sigil::Server, "x {/tricky}", &mut script).unwrap();
    assert_eq!(
        script.names(),
        ["tricky", "x"],
        "the braces in a result must not run anything"
    );
    assert_eq!(script.call("x").items[0].text, tricky, "nor be unescaped");
    assert!(script.code.is_empty());
}

#[test]
fn a_hole_cannot_be_the_name_of_a_command() {
    let mut script = Script::default().answer("n", text("tp"));
    for source in ["{/n} 1 2", "a{/n} 1"] {
        let failure = run(Sigil::Server, source, &mut script).unwrap_err();
        assert_eq!(failure.hole, None, "{source}");
        assert!(
            failure.message.contains("name of a command"),
            "{source}: {}",
            failure.message
        );
    }
    assert!(!script.names().contains(&"tp"));
}

#[test]
fn a_hole_that_fails_names_itself_and_says_what_had_already_run() {
    let mut script = Script::default().refuse("bad", "usage: /bad X");
    let failure = run(Sigil::Server, "x {/a} {/bad} {/c}", &mut script).unwrap_err();
    assert_eq!(
        failure.hole,
        Some(HoleName {
            number: 2,
            column: 9
        })
    );
    assert_eq!(failure.holes_already_run, 1);
    let shown = failure.to_string();
    assert!(
        shown.starts_with("hole 2 (at character 9): usage: /bad X"),
        "{shown}"
    );
    assert!(
        shown.contains("1 hole before it already ran and is not undone"),
        "{shown}"
    );
    assert_eq!(
        script.names(),
        ["a", "bad"],
        "the line stops at the first failure"
    );
}

#[test]
fn a_first_failure_has_nothing_to_say_about_what_ran() {
    let mut script = Script::default().refuse("bad", "no");
    let failure = run(Sigil::Server, "x {/bad}", &mut script).unwrap_err();
    assert_eq!(failure.to_string(), "hole 1 (at character 4): no");
}

#[test]
fn the_innermost_hole_that_fails_is_the_one_named() {
    let mut script = Script::default().refuse("deep", "boom");
    let failure = run(Sigil::Server, "x {/a {/b {/deep}}}", &mut script).unwrap_err();
    assert_eq!(
        failure.hole,
        Some(HoleName {
            number: 3,
            column: 12
        })
    );
    assert!(
        failure
            .to_string()
            .starts_with("hole 3 (at character 12): boom")
    );
}

#[test]
fn the_outer_command_that_fails_after_its_holes_ran_says_so_without_naming_a_hole() {
    let mut script = Script::default().refuse("x", "usage: /x A");
    let failure = run(Sigil::Server, "x {/a} {/b}", &mut script).unwrap_err();
    assert_eq!(failure.hole, None);
    let shown = failure.to_string();
    assert!(shown.starts_with("usage: /x A"), "{shown}");
    assert!(
        shown.contains("2 holes before it already ran and are not undone"),
        "{shown}"
    );
    let mut alone = Script::default().refuse("x", "usage: /x A");
    assert_eq!(
        run(Sigil::Server, "x", &mut alone).unwrap_err().to_string(),
        "usage: /x A"
    );
}

#[test]
fn a_result_longer_than_a_result_may_be_fails_the_hole() {
    let mut script = Script::default().answer("long", text(&"a".repeat(MAXIMUM_RESULT_LENGTH + 1)));
    let failure = run(Sigil::Server, "x {/long}", &mut script).unwrap_err();
    assert_eq!(
        failure.hole,
        Some(HoleName {
            number: 1,
            column: 4
        })
    );
    assert!(failure.message.contains("1025"), "{}", failure.message);
    assert_eq!(script.names(), ["long"]);
    // one that leaves room for the rest of the line is allowed
    let mut script = Script::default().answer("long", text(&"a".repeat(MAXIMUM_LINE_LENGTH - 10)));
    run(Sigil::Server, "x {/long}", &mut script).unwrap();
}

#[test]
fn a_line_that_the_holes_make_too_long_fails_at_the_hole_that_does_it() {
    let third = text(&"a".repeat(MAXIMUM_LINE_LENGTH / 3));
    let mut script = Script::default().answer("half", third);
    let failure = run(Sigil::Server, "x {/half} {/half} {/half}", &mut script).unwrap_err();
    assert_eq!(failure.hole.unwrap().number, 3);
    assert!(failure.message.contains("longer"), "{}", failure.message);
}

#[test]
fn a_client_line_runs_as_a_client_command() {
    let mut script = Script::default().answer("time", text("12:00"));
    let value = run(Sigil::Client, "ambient {.time}", &mut script).unwrap();
    assert_eq!(value, CommandValue::None);
    assert_eq!(script.asked[0].0, Sigil::Client);
    assert_eq!(script.asked[1].0, Sigil::Client);
    assert_eq!(script.call("ambient").items[0].whole, Some(text("12:00")));
}

#[test]
fn a_code_hole_is_run_as_code_after_its_blanks_are_dropped() {
    let mut script = Script::default();
    run(Sigil::Server, "x {> 1+1 }", &mut script).unwrap();
    assert_eq!(script.code, ["1+1 "]);
    assert_eq!(
        script.call("x").items[0].whole,
        Some(CommandValue::Integer(2))
    );
}

#[test]
fn a_client_hole_inside_code_comes_in_as_a_python_literal_never_as_code() {
    let mut script = Script::default()
        .answer("who", text("a'b\\c"))
        .answer("n", CommandValue::Integer(7))
        .answer("code", text("1); evil("));
    run(
        Sigil::Server,
        r#"x {>f({.who}, {.n}, {.code}, "{.n}")}"#,
        &mut script,
    )
    .unwrap();
    assert_eq!(
        script.code,
        [r#"f("a'b\\c", 7, '1); evil(', "{.n}")"#],
        "text becomes a quoted string, a number a number, and a hole in a string is text"
    );
}

#[test]
fn a_code_line_is_run_as_code() {
    let mut script = Script::default().answer("n", CommandValue::Integer(3));
    let value = run(Sigil::Code, " 1 + {.n}", &mut script).unwrap();
    assert_eq!(value, CommandValue::Integer(2));
    assert_eq!(script.code, ["1 + 3"]);
}

#[test]
fn the_refusal_of_a_runner_to_run_code_names_the_hole() {
    struct NoCode;
    impl Runner for NoCode {
        fn command(&mut self, _: Sigil, _: &Call) -> Result<CommandValue, String> {
            Ok(CommandValue::None)
        }
        fn code(&mut self, _: &str) -> Result<CommandValue, String> {
            Err("scripts are not built".to_owned())
        }
    }
    let failure = evaluate(&scan(Sigil::Server, "x {>1}").unwrap(), &mut NoCode).unwrap_err();
    assert_eq!(
        failure.to_string(),
        "hole 1 (at character 4): scripts are not built"
    );
}

#[test]
fn an_item_knows_the_first_hole_it_holds_so_a_wrong_kind_can_name_it() {
    let mut script = Script::default().answer("n", text("a"));
    run(Sigil::Server, "x b {/n}", &mut script).unwrap();
    let items = &script.call("x").items;
    assert_eq!(items[0].first_hole, None);
    assert_eq!(
        items[1].first_hole,
        Some(HoleName {
            number: 1,
            column: 6
        })
    );
    let _: &Item = &items[0];
}
