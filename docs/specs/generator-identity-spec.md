# Who chooses a generator's identity (a discussion, owner's intent 2026-11-22)

The owner's intent, in their words condensed: **the author of a generator chooses its identity**, so that an author can *deliberately bend or break* the rule that one generator id makes one terrain. Uses: adding minor details that were not there when the player last looked (so long as the player did not modify that terrain into an override brick), and rewriting a generator to be faster without changing its id. A suggested form: `vendor_name.generator_name.major_version.minor_version.patch_version`. A server can be set to accept **only exact matches**, or **the latest version of a partial match**. The owner asked to discuss this in more depth before anything is decided. **A discussion and my opinion; nothing in it is built.** Labels: **[read]** I read it in the code or a document; **[believed]** reasoning I did not run.

It is the second use of wasm in `pack-archive-spec.md`: a generator is code a client must run, so it is a file in a replicated pack.

## What the rule is today, and why it is there

- **[read: `engine/terrain/src/generator.rs`]** `TerrainGenerator` says: a generator is part of the world's file format; a world regenerates unmodified terrain from it every time it is loaded, on every client; once a world has used one, its output for any brick must never change; improve terrain by adding a new generator. `coarse_node` has the same rule "for as long as a recording can be replayed". `CLAUDE.md` says the same as a hard rule, with the owner's exception for before the first release.
- **[read: `design-decisions.md`]** *Clients generate terrain from the seed*; the server **saves and sends only terrain modified since generation** (the override bricks); a coarse tree records which generator version made each region. A world keeps *recipes* (an instance name, a family and parameters, `engine:generator_recipes`) and a generator is made from a recipe at load.
- **[read]** Ids carry the version in their names today: `voxunami_cave:rolling_hills_v1`, `small_planet_v1`.

What the rule protects, by what goes wrong without it **[believed]**:

1. **The server and a client disagree about the ground.** The server's physics collides with *its* terrain; the client draws *its own*. If the two modules differ, a player walks through drawn ground or stands on air. This is the serious one.
2. **A seam where an edit meets a regenerated neighbour.** An override brick keeps the old shape; the brick beside it, regenerated, has moved.
3. **A thing built on the ground** (a ship parked, blocks placed in a brick that stayed generated) hangs in the air or sinks if the ground under it changed.
4. **A recording** of a session replays other terrain.
5. **Two servers hosting one world** with different versions give different unmodified terrain.

## My opinion

**Yes: let the author choose, and separate two things the single id does now.** Confidence medium-high.

- **The id is a name**, chosen by the author, kept in saves and shown to people. It may keep its meaning ("the same terrain") or not; that is the author's promise.
- **What a session needs to agree on is the module's hash**, from the file index of its pack (`pack-archive-spec.md`). The welcome names the generators in play with the **hash of each module**, and a client that has not got that exact module fetches it before it draws terrain. So **danger 1 cannot happen by construction**: the server and a client never run two modules under one id, whatever the author did to the id. Danger 4 goes the same way, because a recording embeds the packs it was made with. This is the reason a generator pack must be `replicate: true`.
- Dangers 2, 3 and 5 are **what the owner is choosing**, and I think it is a reasonable choice for an author who wants minor additions: they are the price of "the terrain got better under the player". They are not engine rules to enforce; they are a thing a **version number can tell**:
  - **patch**: the terrain is the same (a faster rewrite, a fix that changes no brick);
  - **minor**: the same landscape with details added or changed;
  - **major**: a different world.
  The engine cannot check the promise, but an author can: a small command, `check-generators OLD NEW` that samples bricks and says *identical*, or how many differed, would let a patch release be tested for "the same terrain". **[believed: cheap, and it would turn the rule into something checked]**
- **Speed is not free.** A generator in wasm is slower than the compiled one; Cave's terrain is a hot path (making a far node costs about 6 ms a brick **[from my notes of 2026-11-19]**, most of it the generator). Before committing, a spike (the one `architecture-review.md` already recommends) should measure one real generator, native against wasm, on the same bricks. **[believed: wasm is usually within 1.5 to 3 times of native; not measured here]** If it costs too much, compiled-in generators stay for the game's own and wasm is for packs.
- **Bit-identical on every machine stays a hard rule**, and wasm helps (its integer and float operations are deterministic, but not the bits of a NaN, and relaxed SIMD, threads and any clock must be off). What the owner relaxes is *the same id gives the same terrain across versions*, not *the same module gives the same terrain on every machine*.

## The spelling of the id

The owner's `vendor_name.generator_name.major.minor.patch` is easy to read and also a change from every other id in saves (`namespace:name`, checked by `NamespacedId` and read by `read_identifier`). My proposal: keep **`vendor_name:generator_name` as a `NamespacedId`**, and carry the **version beside it as three numbers**, written `vendor_name:generator_name@1.2.3` where a person reads it. The information is the same; the saved form keeps the shape every reader already checks. This is a small decision and the owner's: confidence medium.

## Matching

The world's tree of generators names versions; a server with a newer one installed can use it or not. The owner's two policies are, as I read them:

- **exact**: `1.2.3` is used only if `1.2.3` is there;
- **latest of a partial match**: `1.2` may be satisfied by the latest `1.2.x`, and `1` by the latest `1.x.y`. I would add that **a major is never crossed** (it is a different world), and that the setting says how much may float (`exact`, `patch`, `minor`).

Two questions follow that the owner should answer:

1. **What does the world file store?** (a) The version it was *made with*, resolved up at each server start by the policy; or (b) the version *in use*, changed only when the operator accepts an upgrade (written to the world, with a line in the log). With (a) the terrain can change at any start with no one asking; with (b) it changes when someone decides. **My pick: (b)**, confidence medium: it keeps a world stable by default and an upgrade is an act.
2. **The welcome says the exact version and the hash in play**, so a client never guesses; a partial match is the server's business alone.

## What this needs, and what it does not

- **Nothing in `pack-archive-spec.md` waits for it**: the archive, the index, the cache and the `replicate` key are built first; a generator is one more kind of file in a pack later.
- It needs: wasm in the engine (a spike first, then a host with a narrow interface and fuel), a place in `pack.json` where a pack lists its generators (a name, a version, the module's path; **[believed]**), the welcome naming generators by version and hash, and a world file that stores versions.
- The compiled-in generators (Cave's) are **a different case I have not checked**: they are in the program, so a client whose program is older than the server's would regenerate differently if a compiled generator changed under one id. Whether the handshake already refuses a mismatch of them, I did not look; with bending allowed it must, by version, and the first release's rule ("ids may change until then") is the time to give them versions.
- **`CLAUDE.md`'s hard rule and `TerrainGenerator`'s documentation say the opposite today.** They should be reworded when the owner decides, not before: *a generator's id is its author's to choose; what a session runs is named by its module's hash; the same module gives the same terrain on every machine.* I have not changed them.

## Answered by the owner, 2026-11-24

- **Question 2, the meaning of the three numbers:** agreed (patch: the same terrain; minor: details; major: a different world).
- **What the namespace of a generator id is:** **the id of the pack that owns the generator**, as for blocks today (`my_pack:hills`), so the namespace of a generator always equals its pack's folder name and `check-pack` can rely on it. There is no separate vendor part. An author who ships several packs shows it in the pack ids (a naming convention such as `acme_terrain`), not in the syntax. An optional `author` key in `pack.json` may be added later as metadata only (not built). Shader packs keep their `author:name` ids. The `@1.2.3` stays a way to write the version beside a `NamespacedId`; nothing that reads `NamespacedId` changes.
- **Question 1 (the spelling)** is settled by this: `pack_id:generator_name@1.2.3`, with the version stored as three numbers beside the id. The dotted form is not used.
- **Question 3, what the world stores:** (b), **the version in use**, changed only when the operator accepts an upgrade (written to the world, a line in the log). Not settled: what `/reload` does to a running server when a newer version appears (my expectation: it keeps the version in use until the upgrade is accepted).
- **Question 4, the policies:** `exact`, `patch` and `minor`; a major never floats.
- **Question 5, the spike first:** the owner asked for it on 2026-11-24 (one generator, native against wasm, on the same bricks).
- **Still open:** nothing in the questions; the not-settled `/reload` detail above.

## What the spike found (2026-11-24)

The spike is `history/spikes/wasm/` (`wasm-generator-spike.md` there has the table and what it did not measure). One real generator, Cave's rolling hills, compiled to a wasm module with no imports (108 KB) and run on the same 1,152 bricks natively, in wasmtime (machine code) and in wasmi (an interpreter). **[measured, one machine]**

- **The terrain is the same:** every brick equals the native one voxel for voxel in every configuration. (Linux not run.)
- **Wasmtime costs almost nothing: 1.01 times native, 1.24 times with fuel** (5.3 and 6.5 ms a brick against 5.2). My guess of 1.5 to 3 times above was too pessimistic for this generator; **[believed]** the reason is that it is scalar float and integer code with no memory tricks, which a compiler to machine code handles as well as the native compiler does. A different generator could differ.
- **Wasmi is too slow for a generator: about 14 times native** (71 ms a brick), so a view of thousands of bricks would take minutes. This is what `architecture-review.md` said to check ("a JIT only for a generator, and only if a measurement asks"): **the measurement asks.** Wasmi stays the likelier choice for rules, where speed matters less, but it is not a choice for generators.
- **Fuel is not portable between runtimes** (159 vs 296 million for the same brick). For a generator that is harmless if fuel is only a cut-off that stops a module that never ends, since a generator that finishes gives the same bits whatever the count; it would matter if a budget were part of the simulation (rules). **[believed]**
- **Open for the owner**, since it is a decision about a dependency and about what a client must run: wasmtime is a JIT (more code, a larger build, and a client must trust a JIT's sandbox); wasmi is small and cannot do the work. If clients may not run a JIT (a platform that forbids it), a generator must be compiled in, or the client falls back to the server's bricks (the world-writer spike's subject).

## Questions for the owner

1. The spelling: `vendor_name:generator_name@1.2.3` (my pick) or the dotted form?
2. The meaning of the three numbers as above (patch: the same terrain; minor: details; major: a different world), as the author's promise?
3. What the world stores: the version made with (a) or in use (b)? My pick (b).
4. Are the policies `exact`, `patch` and `minor` (a major never floats) the ones you mean?
5. Is a spike (one generator, native against wasm, on the same bricks) the right first step, before any host is built?
