//! What Cave's movement commands answer, word for word, so that moving them to the command registry
//! is shown not to change what a player is told for a good call (and the replies to a bad one are the
//! ones the registry then generates, pinned again in their new form).

mod common;

use common::{Player, Rig};

fn reply_to(rig: &mut Rig, player: &mut Player, command: &str) -> Vec<String> {
    player.send_command(command);
    rig.step();
    rig.step();
    rig.settle(&mut [player]);
    player.take_command_replies()
}

#[test]
fn fly_toggles_and_says_which_it_is_now() {
    let mut rig = Rig::new();
    let mut player = rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    player.take_command_replies();
    // The test rig's players already fly.
    assert_eq!(reply_to(&mut rig, &mut player, "/fly"), ["you are walking"]);
    assert_eq!(reply_to(&mut rig, &mut player, "/fly"), ["you are flying"]);
}

#[test]
fn thrusters_toggle_and_say_which_they_are_now() {
    let mut rig = Rig::new();
    let mut player = rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    player.take_command_replies();
    // (They start off.)
    assert_eq!(
        reply_to(&mut rig, &mut player, "/thrusters"),
        ["your thrusters are on"]
    );
    assert_eq!(
        reply_to(&mut rig, &mut player, "/thrusters"),
        ["your thrusters are off"]
    );
}

#[test]
fn tp_moves_the_player_and_explains_what_it_cannot_do() {
    let mut rig = Rig::new();
    let mut player = rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    player.take_command_replies();
    assert_eq!(
        reply_to(&mut rig, &mut player, "/tp 20.5 120 -3.25"),
        ["teleported to 20.5 120 -3.25"]
    );
    // (With no words it lists the places: `places.rs`.)
    for wrong_count in ["/tp 1", "/tp 1 2", "/tp 1 2 3 4"] {
        assert_eq!(
            reply_to(&mut rig, &mut player, wrong_count),
            ["usage: /tp, /tp X Y Z, /tp PLACE [LATITUDE LONGITUDE | X Y Z]"],
            "{wrong_count}"
        );
    }
    // (These said "usage: /tp X Y Z, with numbers" before the commands had a registry that checks them.)
    for (not_numbers, which) in [
        ("/tp a b c", "X"),
        ("/tp 1 2 three", "Z"),
        ("/tp nan 1 1", "X"),
        ("/tp 1 inf 1", "Y"),
    ] {
        assert_eq!(
            reply_to(&mut rig, &mut player, not_numbers),
            [format!("usage: /tp X Y Z; {which} must be a number")],
            "{not_numbers}"
        );
    }
    assert_eq!(
        reply_to(&mut rig, &mut player, "/tp 1e37 0 0"),
        ["that is outside the world"]
    );
}

/// What a player typed, sent the way the window sends it (the holes of the window run first), and what the server answered.
fn typed(rig: &mut Rig, player: &mut Player, line: &str) -> Result<Vec<String>, String> {
    player.send_command_line(line)?;
    rig.step();
    rig.step();
    rig.settle(&mut [player]);
    Ok(player.take_command_replies())
}

#[test]
fn a_hole_runs_a_command_of_the_server_before_the_line_does_and_what_it_did_stays_done() {
    let mut rig = Rig::new();
    let mut player = rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    player.take_command_replies();
    // `{/fly}` runs first, says what it says, and is worth `None`, which `/tp` is given as the word None, and that is not a number
    // (`/tp` takes the rest of the line and answers a bad one itself, as it always did, so the answer is its own).
    let replies = typed(&mut rig, &mut player, "/tp 20.5 {/fly} 3").unwrap();
    assert_eq!(
        replies,
        ["you are walking", "usage: /tp X Y Z; Y must be a number"]
    );
    // the hole ran, and nothing undid it
    assert_eq!(
        typed(&mut rig, &mut player, "/fly").unwrap(),
        ["you are flying"]
    );
}

#[test]
fn a_hole_that_fails_names_itself_and_a_line_that_is_never_closed_says_so() {
    let mut rig = Rig::new();
    let mut player = rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    player.take_command_replies();
    let replies = typed(&mut rig, &mut player, "/tp 1 2 {/nonsense}").unwrap();
    assert_eq!(replies.len(), 1);
    assert!(
        replies[0].starts_with("hole 1 (at character 9): unknown command; try "),
        "{}",
        replies[0]
    );
    // a hole that is never closed is found by the window, which sends nothing
    let error = typed(&mut rig, &mut player, "/tp 1 2 {/fly").unwrap_err();
    assert!(
        error.starts_with("the hole that opens at character 9 is never closed"),
        "{error}"
    );
    assert!(player.take_command_replies().is_empty());
}

#[test]
fn a_hole_of_the_window_is_run_by_the_window_and_the_server_gets_its_answer_as_text() {
    let mut rig = Rig::new();
    let mut player = rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    player.take_command_replies();
    // `.shadows` answers with words, and `/fly` takes none: the server was sent them as words
    let answer = player.client_command(".shadows");
    assert!(!answer.is_empty());
    let replies = typed(&mut rig, &mut player, "/fly {.shadows}").unwrap();
    assert_eq!(replies, ["usage: /fly"]);
    // a hole of the window that fails sends nothing at all
    let error = typed(&mut rig, &mut player, "/fly {.nonsense}").unwrap_err();
    assert!(
        error.starts_with("hole 1 (at character 6): unknown client command .nonsense"),
        "{error}"
    );
    assert_eq!(player.take_command_replies(), Vec::<String>::new());
    assert_eq!(
        typed(&mut rig, &mut player, "/fly").unwrap(),
        ["you are walking"],
        "the failed line did not toggle flying"
    );
}
