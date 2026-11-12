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
- `planetary-mass-change.md`: the feature "a planet's mass changes", **held back: not in the first release** (owner, 2026-11-04); what is built, what it does to players, a flaw found, and Cave's ARGOS device.
- `test-flakes.md`: tests that failed once and were not explained, kept together for a deep bug hunt (what failed, when, the reruns, what is believed and what was checked).
- `ship-terrain-cost.md`: what a fast ship costs the server and the options to cut it, **held back** until ships of several grids are understood.
- `terrain-collider-options.md`: the bigger options for what a fast ship costs the server (2026-11-07), ranked, with the measurement that shows where a tick goes. Item 1 is done; the rest is not scheduled.
- `leaf-cost-options.md`: what leaves and glass cost on the GPU (measured 2026-11-08) and the options to cut it, with a correction of an overstated claim. **A proposal**; nothing is built.
- `ship-centre-of-mass-edit-options.md`: what a placed or broken block does to the place of a ship on a rail (the whole ship steps by the shift of its centre of mass, today and kept for now, owner 2026-11-10) and three options to smooth it (fit the rail at the edit, a saved offset, ramped mass for new blocks). **Options**; nothing is built.
- `motion-in-frames-options.md`: why walker and ship motion bugs keep appearing (speed stored against a frame whose id can change, ship acceleration differenced from history, a private physics world per walker), a measured case (a real split mid-jump changes the jump) and six options. **Options**; nothing is built.
- `architecture-review.md`: the direction review of 2026-10-29: findings and proposals (milestones M0 to M4, eleven decisions).
  **Proposals, not decisions**, apart from what `design-decisions.md` records as taken.

## Reference

- `pack-format.md`: writing a data pack (blocks, patches, planets). Its examples are checked by a test.
- `shader-pack-format.md`: writing a shader pack (a folder with `pack.json` and `lighting.wgsl`), what it is given, what it may not do, and how to use one.
- `protocol-and-replay.md`: the message stream and replay files.
- `world-file-format.md`: the saved world.

## Specs (one per part or phase; the part's code cites them)

Plugins and content: `plugin-api-spec.md`, `property-bags-spec.md`, `messages-and-commands-spec.md`, `client-interface-spec.md`,
`entities-spec.md`, `content-package-spec.md`, `data-packs-spec.md`, `block-textures-spec.md` (textures, and the game's own blocks as JSON; **not built**), `ui-spec.md`. The game Voxunami 2048: `2048-spec.md`.
The world and the sky: `lod-spec.md` (levels of detail), `planet-spec.md`, `rails-and-recipes-spec.md` (orbits, planets from data),
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
- `light-spike.md`, `lod-spike.md`, `physics-spike.md`, with their programs in `spikes/` and pictures in `multiscale-modes/`.
  The programs were written against the code of their day and are **not kept compiling**; to run one, check out the commit it names.

## Keeping them

- A phase ends by writing what it built into the build log and the decision into `design-decisions.md`; a spec gets its as-built
  notes. If it changed an interface, the reference is updated in the same commit.
- A document that is no longer true is fixed or moved to `history/`, never left to mislead. Deleting is safe in that git keeps
  everything, but the code cites specs by name, so a spec is archived, not deleted.
- The size of a document is a cost paid by every reader who loads it. A standing document that passes about 20 KB is split by area, as `design-decisions.md` was.
