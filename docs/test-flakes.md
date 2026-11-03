# Rare test failures

A register of tests that **failed and were not explained**, kept for a deep bug hunt. A failure that was found, explained and fixed is not an open entry: it is a line at the end with where its story is.
The point of keeping the unexplained ones is to see them together: the same crate again, the same kind of load, the same hour, the same machine.

**How to use it.** When a test fails once in a gate and passes on rerun, do not call it nothing: rerun it (alone, and as the whole binary, the way the gate runs it), check what it depends on, and add an entry
here with what is known and what is not. When it fails again, add the date and the circumstances to its entry. When it is explained, move it to *Explained and fixed* with a pointer to the fix.
A test that waits must wait for a condition with a deadline, never for a fixed time (`CLAUDE.md`).

**What to write for an entry**: the test (path and name); each occurrence (date, commit, what was running, the platform, how it failed); reruns (how many, how, how many failed); what it depends on
(read from the code, marked as read); the beliefs about the cause, each marked **believed** or **checked**; what would settle it.

## Open

### `transport/tests/tcp.rs`: `a_listener_has_nobody_until_somebody_connects_and_then_hands_them_out_in_order`

- **The test.** A listener is bound on `127.0.0.1:0`; a client connects and sends `[1]`; a second client connects and sends `[2]`; the test accepts twice (`accept` waits for a connection with a 10 s deadline) and
  asserts that the first connection accepted received `[1]` and the second `[2]` (`next` waits 10 s for a frame). The assertion at line 260 (`assert_eq!(next(&mut *on_server_first), Ok(Some(vec![1])))` or the one
  after it; the log kept only the line) failed.
- **Occurrence 1: 2026-11-03**, commit `0351b1d` working tree (the E15 change to `wide_kepler`, which `transport` does not depend on: `cargo tree -p transport` names none of `wide_float`, `wide_kepler`,
  `world_storage`), the full gate (`cargo test --workspace --no-fail-fast`, dev profile) on **Windows**, run in the background while the machine was otherwise used by the session. The test binary reported
  `15 passed; 1 failed ... finished in 2.00s`: so **it did not wait out a deadline** (10 s): a frame arrived, and it was not the expected one, or the connection gave an error. Which of the three
  possibilities (the first accepted held `[2]`; an error; a different frame) is **not known**: the message was not kept.
- **Reruns.** 10 times alone (`cargo test --release -p transport --test tcp a_listener_has_nobody`) and 25 times as the whole binary (`cargo test -p transport --test tcp`, dev profile, all 16 tests
  in parallel as the gate does): **0 failures in 35**.
- **What it depends on [read].** The order in which the listener hands out connections (`TcpServerListener::accept`) against the order in which the clients connected. `TcpConnection::connect` returns when the
  client side is connected, but whether the **server side** of the first connection is queued in the listener before the second's is up to the operating system's accept queue and to how the
  listener collects connections (if it has a thread of its own, up to its scheduling). The test asserts an order that nothing in its setup forces between the two accepts.
- **Beliefs about the cause.** **Believed, not tested**: the two connections were accepted in the opposite order from the one they were made in, under load. **Not checked**: whether the listener has a thread that
  can reorder them; whether Windows can complete a loopback handshake out of order.
- **What would settle it.** Print what `next` returned on failure (both frames and both errors) in the assertion messages, then run the binary in a loop under artificial load (several copies at once, or a busy
  CPU thread per core) until it fails, and look at the listener's `accept`. If the order is not guaranteed by the listener, the test should send a value that says which client it is and not
  depend on the order (`accept` twice, and match frames to clients by what they carry).

### `games/voxunami_cave/tests/screen.rs`: `a_replay_draws_the_form_exactly_as_it_was_drawn_live_and_stops_when_it_was_closed`

- **The test.** It draws a form live and again from a replay and compares them pixel for pixel.
- **Occurrence 1: 2026-10-22, Phase 32l** (`build-log.md`): failed once in the full Windows run by **one pixel on one GPU adapter**; passed three times alone; nothing to do with ships; cause not
  found (`docs/history/build-log.md`, "One test of the whole suite failed once in the full Windows run"; the test's name is given there as `a_replay_draws_the_form_exactly_as_it_was_drawn_live...`).
- **Occurrence 2: 2026-11-03** (the full gate of E10, `wide_kepler` and `world_storage` changes): the same file's replay test failed once; **8 reruns passed**; at the next gate (E11) and every gate since it did not
  fail. Not looked into further.
- **Beliefs.** **Checked, for a sister test** (`an_edit_changes_the_picture_and_a_replay_draws_exactly_what_the_player_saw`, explained below): on the integrated AMD GPU the first frames of a fresh renderer sometimes leave
  pixels out, never on the NVIDIA card, and the pictures compared to the pixel are now taken once a frame repeats them (`settled`). **Believed**: the same family of cause for this test (a pixel left out in a
  first frame on one adapter); not checked for this test.
- **What would settle it.** Run it in a loop on each adapter with `VOX_REQUIRE_GPU=1` and log which adapter and which pixel differs; see whether it goes through `settled`.

## Explained and fixed

- `an_edit_changes_the_picture_and_a_replay_draws_exactly_what_the_player_saw` (`games/voxunami_cave/tests/pictures.rs`): the first frames of a fresh renderer on the integrated AMD GPU leave pixels out;
  pictures are taken once a frame repeats them (2026-10-21, Phase 30). `docs/plan.md` (the section on flaky tests) and `docs/specs/ui-spec.md`.
- `a_walker_standing_in_flames_falls_when_the_fire_burns_the_ground_away` and `walkers_pass_through_flames_and_the_cellar_has_the_same_ground`: the test raced a chance fire keyed by the tick a player's first message
  reaches the server (2026-10-18 to 2026-10-19). `docs/plan.md`.
- A fire test that was flaky until it was a test of something, a joining test whose fixed number of idle ticks was not enough on a cold build, and the body put inside terrain (Phase 27f): `docs/history/build-log.md`
  (`grep -n "flaky" docs/history/build-log.md`).
