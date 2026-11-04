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

### `world_view/tests/ship_terrain.rs`: `beyond_the_extent_there_is_nothing_so_the_edge_of_a_planet_shows_its_side`

- **The test.** A planet's mesher (3 worker threads, view radius 3) is settled once (`ShipTerrainMesher::settle`) at the origin; the test asserts that the bricks it shows are the 18 inside the extent (`assert_eq!(shown, expected)` at line 183).
- **Occurrence 1: 2026-11-03**, working tree of E16 phase B (changes in `wide_kepler`, `world_storage` and `server`; `world_view` has `world_storage` as a dev-dependency (`cargo tree`: it test-supports the scene), and so `wide_kepler` below it, but the test builds a scene and meshes it and does not use a rail [read]), the full gate (`cargo test --workspace --no-fail-fast`, dev profile) on
  **Windows**, run in the background. The binary reported `5 passed; 1 failed ... finished in 0.17s`. The message was not kept by the gate's filter.
- **Occurrence 2: 2026-11-03**, the same working tree, in the reruns: **1 failure in 25** whole-binary runs (`cargo test --release -p world_view --test ship_terrain`; the 3 runs before them alone passed). **The message was kept:** `left` (what was shown) was
  **one brick, `(0, 0, 0)`**, against the 18 expected: `settle` returned when most of the bricks had not been meshed (or loaded) yet; it did not time out and did not show a wrong brick.
- **What it depends on [read].** `WorldMesher::settle` (`mesher.rs`) loops on `update` until `pending_jobs() == 0 && scheduled_last_update == 0`, sleeping 200 us between; the bricks of a planet come from the generator through
  `ShipTerrainSource`. A `settle` that ends because **nothing was pending at one look** ends the test's wait, whatever the workers or the source have not delivered yet.
- **Beliefs about the cause.** **Believed, not tested**: an `update` that found no work scheduled and none pending while the first bricks were still on their way (a worker holding a finished job that `update` had not collected counts as pending, but the
  source handing out a brick later than the first look would not). **Not checked**: whether the source or the generator has a thread of its own; whether this fails more under load.
- **What would settle it.** Print `scheduled_last_update`, `pending_jobs()` and the number of loaded bricks at the exit of `settle` on a failing run (loop the binary until it fails, 4 percent of runs here), and look at whether the missing bricks are in the source and not yet
  asked, or asked and not yet returned. A `settle` that waits for a condition with a deadline (all the bricks the view wants are meshed) would be the fix for the test; if the mesher can end its work with bricks wanted and not asked, it is the mesher's.

## Explained and fixed

- `an_edit_changes_the_picture_and_a_replay_draws_exactly_what_the_player_saw` (`games/voxunami_cave/tests/pictures.rs`): the first frames of a fresh renderer on the integrated AMD GPU leave pixels out;
  pictures are taken once a frame repeats them (2026-10-21, Phase 30). `docs/plan.md` (the section on flaky tests) and `docs/specs/ui-spec.md`.
- `a_walker_standing_in_flames_falls_when_the_fire_burns_the_ground_away` and `walkers_pass_through_flames_and_the_cellar_has_the_same_ground`: the test raced a chance fire keyed by the tick a player's first message
  reaches the server (2026-10-18 to 2026-10-19). `docs/plan.md`.
- A fire test that was flaky until it was a test of something, a joining test whose fixed number of idle ticks was not enough on a cold build, and the body put inside terrain (Phase 27f): `docs/history/build-log.md`
  (`grep -n "flaky" docs/history/build-log.md`).
