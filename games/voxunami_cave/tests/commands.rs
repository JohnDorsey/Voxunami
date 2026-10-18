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
fn tp_moves_the_player_and_explains_what_it_cannot_do() {
    let mut rig = Rig::new();
    let mut player = rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    player.take_command_replies();
    assert_eq!(
        reply_to(&mut rig, &mut player, "/tp 20.5 120 -3.25"),
        ["teleported to 20.5 120 -3.25"]
    );
    for wrong_count in ["/tp", "/tp 1", "/tp 1 2", "/tp 1 2 3 4"] {
        assert_eq!(
            reply_to(&mut rig, &mut player, wrong_count),
            ["usage: /tp X Y Z"],
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
        reply_to(&mut rig, &mut player, "/tp 1e30 0 0"),
        ["that is outside the world"]
    );
}
