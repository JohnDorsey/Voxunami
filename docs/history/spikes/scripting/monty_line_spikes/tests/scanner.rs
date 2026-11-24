//! Spike S2 of scripting-spec.md: where does a `{>...}` hole end, checked against Monty's own tokenizer.
//!
//! Run: `cargo test --release -- --nocapture` (PROPTEST_DISABLE_FAILURE_PERSISTENCE=1 for mutation runs).

use monty::MontyRun;
use monty_line_spikes::{hole_end_lexical, hole_end_naive, hole_end_ruff};
use monty_types::CompileOptions;
use proptest::prelude::*;
use proptest::strategy::{Strategy, ValueTree};
use proptest::test_runner::{Config, TestRunner};

/// Complete Python expressions whose text is full of braces, quotes and backslashes.
fn expression() -> impl Strategy<Value = String> {
    let leaf = prop_oneof![
        Just("1".to_owned()),
        Just("42".to_owned()),
        Just("3.5".to_owned()),
        Just("x".to_owned()),
        Just("yy".to_owned()),
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
            (inner.clone(), inner.clone(), inner.clone()).prop_map(|(a, b, c)| format!("{a}[{b}:{c}]")),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("{{k: {a} for k in {b}}}")),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("{{{a} for _ in {b}}}")),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("(lambda t: {a})({b})")),
            (inner.clone(), inner.clone(), inner.clone()).prop_map(|(a, b, c)| format!("({a} if {b} else {c})")),
            inner.clone().prop_map(|a| format!("({a}).attr")),
            inner.clone().prop_map(|a| format!("x.get({a})")),
            inner.prop_map(|a| format!("{a}  ")),
        ]
    })
}

/// Text after a hole: braces, quotes, backslashes, hashes and more holes, as chat might hold.
fn tail() -> impl Strategy<Value = String> {
    r#"[ a-z{}()'"\\#>/.:!]{0,24}"#
}

proptest! {
    #![proptest_config(Config { cases: 4000, .. Config::default() })]

    /// The hole ends exactly after the expression, for the reference and for the hand-written scanner.
    #[test]
    fn both_scanners_find_the_end_of_a_complete_expression(expression in expression(), tail in tail()) {
        let text = format!("{expression}}}{tail}");
        prop_assert_eq!(hole_end_ruff(&text), Some(expression.len()), "ruff: {:?}", text);
        prop_assert_eq!(hole_end_lexical(&text), Some(expression.len()), "lexical: {:?}", text);
    }
}

fn samples(count: usize) -> Vec<String> {
    let mut runner = TestRunner::deterministic();
    let strategy = expression();
    (0..count).map(|_| strategy.new_tree(&mut runner).unwrap().current()).collect()
}

/// The generator must make code that Monty compiles, or the test above proves nothing.
#[test]
fn the_generated_expressions_compile_in_monty() {
    let mut failures = Vec::new();
    let all = samples(3000);
    for code in &all {
        if let Err(error) = MontyRun::new(code.clone(), "probe.py", vec![], CompileOptions::default()) {
            failures.push((code.clone(), error.to_string().lines().last().unwrap_or("").to_owned()));
        }
    }
    println!("monty compiled {} of {} generated expressions", all.len() - failures.len(), all.len());
    let mut distinct: Vec<_> = failures.iter().map(|(code, error)| (error.clone(), code.clone())).collect();
    distinct.sort();
    distinct.dedup_by(|a, b| a.0 == b.0);
    for (error, code) in distinct.iter().take(10) {
        println!("  rejected: {code:?}  ->  {error}");
    }
    assert!(failures.is_empty(), "the generator makes code Monty rejects; see the list above");
}

/// Mutation check: the counter that knows nothing about strings must fail this test, or the test is blind.
#[test]
fn a_scanner_that_only_counts_braces_is_caught() {
    let mut runner = TestRunner::deterministic();
    let strategy = (expression(), tail());
    let total = 2000;
    let mut wrong = 0;
    for _ in 0..total {
        let (expression, tail) = strategy.new_tree(&mut runner).unwrap().current();
        let text = format!("{expression}}}{tail}");
        if hole_end_naive(&text) != Some(expression.len()) {
            wrong += 1;
        }
    }
    println!("the brace counter was wrong on {wrong} of {total}");
    assert!(wrong > 100);
}

/// Differential run on soup: where do the hand-written scanner and ruff's tokens disagree?
/// Prints classes of disagreement and asserts nothing: the disagreements are the finding.
#[test]
fn where_the_two_scanners_disagree_on_soup() {
    let alphabet: Vec<char> = "{}()[]\"'\\fFrRbB#:!=, .a1x\n".chars().collect();
    let strategy = proptest::collection::vec(proptest::sample::select(alphabet), 0..24);
    let mut runner = TestRunner::deterministic();
    let total = 60_000;
    let (mut same, mut different) = (0usize, 0usize);
    let mut classes: std::collections::BTreeMap<&'static str, (usize, String)> = Default::default();
    for _ in 0..total {
        let chars = strategy.new_tree(&mut runner).unwrap().current();
        let text: String = chars.into_iter().collect();
        let (ruff, lexical) = (hole_end_ruff(&text), hole_end_lexical(&text));
        if ruff == lexical {
            same += 1;
            continue;
        }
        different += 1;
        let class = if text.contains('#') {
            "a # (ruff: a comment that hides the rest of the line; lexical: ordinary)"
        } else if text.contains('\n') {
            "a newline (ruff ends a logical line)"
        } else if ruff.is_none() && lexical.is_some() {
            "ruff found no end, lexical did"
        } else if ruff.is_some() && lexical.is_none() {
            "ruff found an end, lexical did not"
        } else {
            "both found an end, a different one"
        };
        let entry = classes.entry(class).or_insert((0, text.clone()));
        entry.0 += 1;
        if text.len() < entry.1.len() {
            entry.1 = text;
        }
    }
    println!("soup: {total} strings, scanners agree on {same}, differ on {different}");
    for (class, (count, shortest)) in &classes {
        println!("  {count:>6}  {class}   e.g. {shortest:?}  ruff {:?} lexical {:?}", hole_end_ruff(shortest), hole_end_lexical(shortest));
    }
}

/// Leading blanks after the sigil: Monty reads them as indentation, so the host must strip them (`{> 1+1}`).
#[test]
fn a_leading_blank_is_an_indentation_error_in_monty() {
    assert!(MontyRun::new("1+1".to_owned(), "p.py", vec![], CompileOptions::default()).is_ok());
    let error = MontyRun::new(" 1+1".to_owned(), "p.py", vec![], CompileOptions::default()).err().map(|e| e.to_string());
    println!("a leading blank: {error:?}");
    assert!(error.is_some());
    assert!(MontyRun::new("1+1   ".to_owned(), "p.py", vec![], CompileOptions::default()).is_ok(), "trailing blanks are fine");
}

/// The odd corners, written down.
#[test]
fn corner_cases_agree() {
    let cases: &[(&str, Option<usize>)] = &[
        (r#"{"a": 1}["a"]}"#, Some(13)),
        (r#"{1, 2} | {3}} tail"#, Some(12)),
        (r#""}"}"#, Some(3)),
        (r#"f"{'}'}"}"#, Some(8)),
        (r#"f"{x:{'}'}}"}"#, Some(12)),
        (r#".5}"#, Some(2)),
        (r#"{.5, .25}}"#, Some(9)),
    ];
    for (text, expected) in cases {
        assert_eq!(hole_end_ruff(text), *expected, "ruff {text:?}");
        assert_eq!(hole_end_lexical(text), *expected, "lexical {text:?}");
    }
}

/// Can a nested hole start inside Monty code? Only if no valid code has `{/`, `{>` or `{.name` outside a string.
/// By the grammar none has: an expression cannot start with `/`, `>` or a dot followed by a name. `{.5}` and `{...}` are
/// valid (a set holding 0.5, a set holding Ellipsis), so a `{.` is a hole only when a letter follows.
#[test]
fn no_valid_code_has_a_sigil_after_a_brace_except_a_dot_before_a_number_or_dot() {
    use ruff_python_ast::token::TokenKind;
    use ruff_python_parser::{Mode, ParseOptions, parse_unchecked};
    use ruff_text_size::Ranged;
    let alphabet: Vec<char> = "{}()[] /.>,:abx1 5=+-".chars().collect();
    let strategy = proptest::collection::vec(proptest::sample::select(alphabet), 0..16);
    let mut runner = TestRunner::deterministic();
    let (mut valid, mut with_brace_dot_digit) = (0usize, 0usize);
    for _ in 0..200_000 {
        let text: String = strategy.new_tree(&mut runner).unwrap().current().into_iter().collect();
        let parsed = parse_unchecked(&text, ParseOptions::from(Mode::Module));
        if !parsed.errors().is_empty() {
            continue;
        }
        valid += 1;
        let tokens: Vec<_> = parsed.tokens().iter().collect();
        for pair in tokens.windows(3) {
            if pair[0].kind() == TokenKind::Lbrace && pair[0].end() == pair[1].start() {
                match pair[1].kind() {
                    TokenKind::Slash | TokenKind::Greater => panic!("valid code with a sigil after a brace: {text:?}"),
                    TokenKind::Dot if pair[2].kind() == TokenKind::Identifier => panic!("valid code with {{.name: {text:?}"),
                    TokenKind::Float | TokenKind::Ellipsis | TokenKind::Dot => with_brace_dot_digit += 1,
                    _ => {}
                }
            }
        }
    }
    println!("{valid} valid snippets looked at; {with_brace_dot_digit} had a brace followed by a number or an ellipsis (valid, and not a hole)");
    assert!(valid > 1000);
    // the two valid cases that start with a dot, written down: they are sets, so a hole must not claim them
    for text in ["{.5}", "{.5, .25}", "{...}"] {
        let parsed = parse_unchecked(text, ParseOptions::from(Mode::Module));
        assert!(parsed.errors().is_empty(), "{text} should be valid Python");
        assert!(MontyRun::new(text.to_owned(), "p.py", vec![], CompileOptions::default()).is_ok(), "{text} should compile in Monty");
    }
    let _ = with_brace_dot_digit;
}
