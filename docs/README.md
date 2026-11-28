# The documents

A map of this folder. Documents are of four kinds, and the kind decides how far to trust one and whether to read it at all.

| Kind | Folder | True now? | Read it |
|---|---|---|---|
| **Standing** | `docs/*.md`, `decisions/` | Kept true: wrong text is a bug | When the task touches what it covers |
| **Reference** | `reference/` | Describes what the code does; a mismatch is a bug | When you use or change that interface |
| **Specs** | `specs/` | True when written; **as-built notes** at the end say what changed | For *why* a part is shaped as it is |
| **History** | `history/` | A record of its day; may be out of date | Only to look something up (`grep`), never whole |

The names of documents are unique, so the prose says "`lod-spec.md`" without a folder, and a test (`launcher/tests/docs.rs`) checks that
every such name and every `docs/...` path in the code and the documents is a real file.

## Standing

- `design-decisions.md`: the rules that hold for every game and the shape of the project, in standing form (what, who, where to read
  more). Read before architectural work. It points to one document per area in `decisions/` (`world-and-sky.md`,
  `players-and-ships.md`, `joints-and-cars.md`, `plugins-and-clients.md`, `packs-and-content.md`), each about 10 to 17 KB: read the one for the area you touch.
  The story of each decision (the phase, the numbers, what was found) is in `history/decision-diary.md`. A number that is also in the
  code belongs in the code and its tests, with a pointer.
- `plan.md`: the build plan, what is proposed, what is open.
- `future-goals.md`: owner wishes that are not scheduled, with what each would need.
- `orbits-and-thrust-plan.md`: a plan for elliptical orbits and an investigation of ships kept on exact rails and of long burns with previews. **A proposal**; nothing is built.
- `monty-step-budget-plan.md`: the plan for a deterministic step budget in the Monty fork, so a ship computer is cut off at the same instruction on every machine. **Built** (phases 0 to 6, 2026-11-16), and followed by a deterministic memory limit, `max_model_memory`, which the plan's "Not part of this plan" section records with its measurements (`history/spikes/scripting/monty_memory_results_2026-11-16.txt`). The fork's own `FORK.md` is the reference.
- `planetary-mass-change.md`: the feature "a planet's mass changes", **held back: not in the first release** (owner, 2026-11-04); what is built, what it does to players, a flaw found, and Cave's ARGOS device.
- `test-flakes.md`: tests that failed once and were not explained, kept together for a deep bug hunt (what failed, when, the reruns, what is believed and what was checked).
- `ship-terrain-cost.md`: what a fast ship costs the server and the options to cut it, **held back** until ships of several grids are understood.
- `terrain-collider-options.md`: the bigger options for what a fast ship costs the server (2026-11-07), ranked, with the measurement that shows where a tick goes. Item 1 is done; the rest is not scheduled.
- `leaf-cost-options.md`: what leaves and glass cost on the GPU (measured 2026-11-08) and the options to cut it, with a correction of an overstated claim. **A proposal**; nothing is built.
- `ship-centre-of-mass-edit-options.md`: what a placed or broken block does to the place of a ship on a rail (the whole ship steps by the shift of its centre of mass, today and kept for now, owner 2026-11-10) and three options to smooth it (fit the rail at the edit, a saved offset, ramped mass for new blocks). **Options**; nothing is built.
- `folder-structures-review.md`: every folder and file the program reads or writes (data packs, the client's texture folder, shader packs, the game data folder, worlds), what is watched and what is not, what does not fit together (two `pack.json`, mirrored picture layouts, a hand-kept list of embedded pictures) and a proposal for one pack format that a server and a client both read (2026-11-19). **A proposal**; nothing is built.
- `world-painting-options.md`: how VirtualAtlas could paint on a block's face in the running game (2026-11-15): how the painter sees the game, how a pointer becomes a tile pixel (the shader says, not the CPU), how a change reaches the game (a client picture override that is also the texture pack), what "pack reloading" means in three sizes, and who may run such a tool without opening a way to cheat. **A proposal** that is built through its fourth step: the picture override, the CPU surface hit with its oracle, the bridge (`.paint`) and VirtualAtlas's side (`tools/virtual_atlas/BRIDGE.md`, the protocol, which is about no one game).
- `hot-reload-plan.md`: every kind of content the game could reload while it runs (shader packs, the engine's own shaders, pictures, block data, bodies, plugin rules), what each touches, and an order of steps with the reason for it: first what only this player sees, then what changes the package and so a recording, then what needs a new runtime. **A proposal**; nothing is built.
- `motion-in-frames-options.md`: why walker and ship motion bugs keep appearing (speed stored against a frame whose id can change, ship acceleration differenced from history, a private physics world per walker), a measured case (a real split mid-jump changes the jump) and six options. **Options**; 1 (tests) and 2 (one resolver for stale ids) are built, the rest is not.
- `architecture-review.md`: the direction review of 2026-10-29: findings and proposals (milestones M0 to M4, eleven decisions).
  **Proposals, not decisions**, apart from what `design-decisions.md` records as taken.

## Reference

- `pack-format.md`: writing a data pack (blocks, patches, planets). Its examples are checked by a test.
- `shader-pack-format.md`: writing a shader pack (a folder with `shaderpack.json` and `lighting.wgsl`), what it is given, what it may not do, and how to use one.
- `protocol-and-replay.md`: the message stream and replay files.
- `world-file-format.md`: the saved world.

## Specs (one per part or phase; the part's code cites them)

Plugins and content: `plugin-api-spec.md`, `property-bags-spec.md`, `messages-and-commands-spec.md`, `client-interface-spec.md`,
`entities-spec.md`, `content-package-spec.md`, `data-packs-spec.md`, `block-textures-spec.md` (textures, and the game's own blocks as JSON; **not built**), `ui-spec.md`, `scripting-spec.md` (**phases 1, 2 and the first half of 3 are built**: Monty in the chat and command line as `{/...}`, `{.}` and `{>}` holes that nest, chat, nicknames, command trees; the permission system, persistent scripts and the rest are still to come), `generator-identity-spec.md` (**answered 2026-11-24 and 25**; first written 2026-11-22: the author of a generator chooses its id, a session is tied to the module's hash, versions and matching; the spike's findings), `wasm-generator-host-spec.md` (**a plan, answered in part, 2026-11-25** (the recipe's format change is still open; section 12): a wasm generator is a `GeneratorFamily`, the module's contract, a host crate with a pool of instances and a load-time self-check, versions in packs, the client's late-built generator, platforms with no JIT, the order of the work), `procedural-materials-spec.md` (**a discussion, awaiting the owner**, 2026-11-25, apart from the primitive, which is BLAKE3 since 2026-11-26: why bricks have palettes and what limits them, string ids in Minecraft, Luanti and here, and materials made for each planet whose origin must not be found from a block in hand: the plan reviewed against `seed-secrecy-survey.md`, with rules and questions), `pack-archive-spec.md` (**a proposal, answered in part**, 2026-11-20: one canonical zip for a pack, the same on a server and a client, sent in chunks, cached by hash, with the owner's questions), `package-replacement-spec.md` (**a proposal, awaiting the owner**: what the stream says when the package changes while the world runs, so that a recording still equals what was seen; step 4 of `hot-reload-plan.md`). The game Voxunami 2048: `2048-spec.md`.
The world and the sky: `lod-spec.md` (levels of detail), `viewpoints-spec.md` (**a draft, nothing built**: the camera as a viewpoint that asks for terrain, several kept warm and one drawn, for a ship view and a map view that switch at once, a free camera and an orbit preview; with measurements), `far-nodes-spec.md` (a plan: far nodes for edits and ships), `planet-spec.md`, `rails-and-recipes-spec.md` (orbits, planets from data),
`great-rivers-spec.md`, `places-spec.md` (`/tp`), `sun-spec.md`, `shadow-spec.md`, `contact-trace-spec.md` (**a design; phases 0 to 2 are built (the renderer too), and the cost in real Cave is over the budget: the owner's to decide**: the sun's exact short ray through the voxels that removes the lit strip at the foot of a wall), `shader-pack-spec.md` (swappable shaders: the engine works out what is lit and a pack makes the colour; **built: the interface, and the player's side (`--shader-pack`, `.shader`)**). Ships: `moving-grids-spec.md`.

## History

- `decision-diary.md`: the old, whole text of `design-decisions.md` (2026-10-30), kept for the stories and numbers behind the standing rules. Not maintained.
- `build-log.md`: what each phase built and measured (about 290 KB). `grep -n "^Phase" docs/history/build-log.md` lists the phases.
- `plugin-architecture-review.md`: the 2026-10-17 reasoning behind the plugin shape.
- `joint-probe.md` (2026-11-01: do Rapier joints hold when over-constrained, with its program in `spikes/joints/`).
- `loop-probe.md` (2026-11-06: loops of cars on multibody or impulse joints, phantom momentum and free energy, cost against the number of cars, overlapping colliders, spin, and the accuracy of `BodyWorld` collisions; programs in `spikes/loops/`).
- `ship-rail-frames-review.md` (2026-11-10: how a ship that turns rides a rail, steps 4c-1 and 4c-2 of the joints plan, read through with snippets, what is tested and what is not).
- `contact-shadow-spike.md` (2026-11-09: the lit strip at the foot of a wall measured at every sun and cascade, its cause in the map's depth bias, and a prototype that walks the sun's ray through a voxel volume to remove it, with its cost on the integrated GPU; the code is in commits `3f6ffb1` and `e166faf` of the branch `worktree-shaders`, and out of the tree: see the document).
- `shader-systems-survey.md` (2026-11-10: how shader packs work in the block-game genre, OptiFine and Iris above all, and in other games and engines, set against our `shader-pack-spec.md`, with what to keep, what is too narrow, and the owner's open questions; read from primary sources, each claim marked as re-read, inferred, run or not read).
- `shade-study.md` (2026-11-11: whether the shade is legible at a distance, measured on ten fixed Cave views, with the shade's share of the sky's light at 70, 130 and 190; 38% of the far terrain is nearly black today).
- `pixelated-shadows-study.md` (2026-11-11: what it costs to make each pixel of a block's picture wholly lit or shaded, as Complementary does, for everything including ships and planets: the snap is nearly free, the shadow map's resolution is what costs, measured at 1024, 2048 and 4096).
- `parallax-spike.md` (2026-11-13: parallax for block pictures as square prisms with shaded sides, against smooth parallax occlusion and offset limiting: what each looks like (pictures in `parallax-spike/`) and what it costs on the integrated GPU; the spike is commit `7d8f955`; prisms were then built as a shader pack's option, on while tested: "Afterwards" at its end, and `block-textures-spec.md` "Prisms").
- `interoperability-survey.md` (2026-11-13: what Voxunami could take from Veloren, other games, avatar standards and editor plugins, and how third-party avatars, builds, ships, textures, controls and mods could appear in the game; ten ranked moves, the licence facts, wild options, what VRM and VR would cost, the scripting languages games use, a measured Python setup cost, and the sandboxed engines with generator expressions compared by running them (Rune, Starlark, Monty, QuickJS-ng, Tcl, jaq; programs and output in `spikes/scripting/`), and the questions only the owner can answer; research and not legal advice, each claim marked as re-read, read, secondary, recalled or inferred; Mojang's terms were not readable).
- `seed-secrecy-survey.md` (2026-11-25, with a primer on hashes, keys and salts at its top since 2026-11-26: how seeds are cracked (Minecraft's structure seeds, Randar), what servers and cryptography do about it, what searching a secret costs by its size (measured, then extended), and what this engine's own seed mixer lets an attacker do (measured); each claim marked as read, measured, web, recalled or believed; the review of the owner's plan is `specs/procedural-materials-spec.md`).
- `spikes/seed_probe/` (2026-11-25: the program behind the survey's numbers: `terrain::mix` run backwards, Java's `Random`, BLAKE3's cost and a search through it, the palettes of real bricks; `results_2026-11-25.txt` there).
- `spikes/wasm/` (2026-11-24: Cave's rolling-hills generator as a wasm module against native, in wasmtime and wasmi: the same bricks voxel for voxel, wasmtime 1.01 times native, wasmi 14 times; `wasm-generator-spike.md` there, discussed in `specs/generator-identity-spec.md`).
- `permissions-survey.md` (2026-11-23: how game servers, Minecraft mods and large access-control systems manage permissions, set against the needs of the command line and scripting; twelve findings, a usability study of 19 systems on thirteen admin tasks with its scoreboard, what studies of people show about conflict rules and effective-permission views, eleven design decisions with a recommendation and a confidence each, the places where it touches decisions already made, and ten questions for the owner; desk research, nothing run and no one observed, each claim marked as re-read, read, secondary, recalled, inferred or checked; the five research streams' raw notes are in `history/permissions-survey/`, and a playtester protocol is in the fifth).
- `light-spike.md`, `lod-spike.md`, `physics-spike.md`, with their programs in `spikes/` and pictures in `multiscale-modes/`; the three programs behind `viewpoints-spec.md` section 3 are in `spikes/viewpoints/` (no document of their own).
  The programs were written against the code of their day and are **not kept compiling**; to run one, check out the commit it names.

## Keeping them

- A phase ends by writing what it built into the build log and the decision into `design-decisions.md`; a spec gets its as-built
  notes. If it changed an interface, the reference is updated in the same commit.
- A document that is no longer true is fixed or moved to `history/`, never left to mislead. Deleting is safe in that git keeps
  everything, but the code cites specs by name, so a spec is archived, not deleted.
- The size of a document is a cost paid by every reader who loads it. A standing document that passes about 20 KB is split by area, as `design-decisions.md` was.
