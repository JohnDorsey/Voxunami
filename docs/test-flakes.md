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
- **Occurrence 3: 2026-11-07**, the full gate of block-textures step 1 (changes in `block_models`, `data_packs`, `voxel_meshing`, `base_blocks`; none draws a form), `cargo test --workspace --no-fail-fast` on **Windows**: `adapter 1: 1 pixels differ, such as [(157, 68)]`
  (screen.rs:386). **Reruns: 3 alone and 1 as the whole binary, 0 failures in 4.** Adapter 1 is the second adapter of the run (which one that is on this machine was not printed).
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

### `wide_float/tests/codec.rs`: `a_number_has_one_encoding` (a real fault found by the test, reproducible; not a flake)

- **The test.** It encodes a random `WideFloat`, flips bits of one random byte, and asserts that if the changed bytes still decode, they encode back to exactly the changed bytes ("nothing is accepted that would not be written").
- **Occurrence 1: 2026-11-07**, the full gate of block-textures step 1 (`wide_float` and everything it depends on untouched by that change: `cargo tree -p wide_float` names none of `block_models`, `data_packs`, `game_shell`), on **Windows**. Proptest shrank it to
  `number = 0x8000000000000000000000000000000000000000000000000000000000000000001p-129@268, position = Index(1871694973728413922), flip = 2`, with `left` (what the decoded number writes) `[140, 2, 1, 1, 34, 1, 0 ... 8]` and `right` (the changed
  bytes) `[140, 2, 1, 129, 0, 34, 1, 0 ... 8]`: **the reader accepted `129, 0` where the writer writes `1`**, a padded varint. Saved by proptest as `cc d4df15ccde8d77a737900a4221bad59894c63c105779ae513bda1f9538264b13` in
  `engine/wide_float/tests/codec.proptest-regressions`; **that file was not kept in the tree** (it makes the test fail for everyone until the fault is fixed), so to replay the case put the line above in a file of that name.
- **Reruns.** With the seed in place it **failed every time (reproducible)**. With `PROPTEST_DISABLE_FAILURE_PERSISTENCE=1` (the seed ignored) it passed 2 alone and in the whole binary: random cases only meet a padded varint by luck.
- **What it depends on [read].** `ByteReader::read_varint` in `engine/binary_codec/src/lib.rs` rejects a tenth byte over 1 but **accepts a last byte of zero after a continuation** (`0x81 0x00` is read as 1). `read_count`, `read_signed_varint` and everything built on them
  inherit this, so it is not only `wide_float`'s: the **package and block-definition decoders claim "decoding refuses anything the encoder would not write"** (`block_models/src/codec.rs`) and rely on the same reader; their random-bytes property
  tests (`bytes_that_are_not_a_definition_are_an_error...`) could meet it too, by luck. `read_wide_signed` in the same file does refuse its padded form.
- **Beliefs about the cause.** **Read, not tested by a fix**: the missing check is "a varint of more than one byte whose last byte has no payload" (`position > 0 && payload == 0`). **Not checked**: whether any saved world or replay on disk contains a padded varint (the writer never writes one, so
  believed not).
- **What would settle it.** Add that check in `read_varint` with a test of `[0x81, 0x00]`, `[0x80, 0x80, 0x00]` and the ten-byte forms, and run the whole gate (saves and replays are the risk). The owner's to decide, since it changes what a shared decoder accepts.

## Explained and fixed

- `beyond_the_reach_of_the_cascades_the_picture_is_the_one_without_shadows_and_nothing_is_not_a_number` and `a_block_and_a_roof_shade_the_floor_where_the_sun_puts_the_shadow_...` (the two shadow tests of `engine/renderer/tests/gpu_rendering.rs`), 2026-11-07, while block pictures were built: **they failed in 3 to 4 of 10 to 12 runs on the integrated AMD GPU
  (never on the NVIDIA card) when the voxel fragment shader used the texture and sampler bindings, though no test drew a picture.** Measured: the committed renderer 0 of 10 filtered and 0 of 3 whole-binary runs; the shader with the picture code removed 0 of 12; with only
  the two derivatives 0 of 12; with a branch and one sampling call 4 of 12; the full shader 3 of 10 (and 6 of 6, 2 of 5 and 1 of 3 in three earlier variants of the textures' upload). It is the known family of the integrated GPU leaving pixels out of a fresh renderer's first
  frames (`an_edit_changes_the_picture_and_a_replay_draws_exactly_what_the_player_saw` above), which this made far more likely. Not found: why the use of the bindings does it. Fixed by avoiding it: the reading of pictures is a second shader, built and chosen only when a
  palette has pictures and a distance is set (`pictures_off.wgsl` / `pictures_on.wgsl`); after that the two tests failed 0 of 12 (twice) and the five picture tests 0 of 18, and the whole binary passed three times. **Believed, not tested:** a game that draws pictures on that
  GPU may still leave pixels out in its first frames, as any scene may; the tests that compare pictures to the pixel take them once two frames agree.

- `an_edit_changes_the_picture_and_a_replay_draws_exactly_what_the_player_saw` (`games/voxunami_cave/tests/pictures.rs`): the first frames of a fresh renderer on the integrated AMD GPU leave pixels out;
  pictures are taken once a frame repeats them (2026-10-21, Phase 30). `docs/plan.md` (the section on flaky tests) and `docs/specs/ui-spec.md`.
- `a_walker_standing_in_flames_falls_when_the_fire_burns_the_ground_away` and `walkers_pass_through_flames_and_the_cellar_has_the_same_ground`: the test raced a chance fire keyed by the tick a player's first message
  reaches the server (2026-10-18 to 2026-10-19). `docs/plan.md`.
- A fire test that was flaky until it was a test of something, a joining test whose fixed number of idle ticks was not enough on a cold build, and the body put inside terrain (Phase 27f): `docs/history/build-log.md`
  (`grep -n "flaky" docs/history/build-log.md`).
