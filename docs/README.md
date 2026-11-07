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
- `architecture-review.md`: the direction review of 2026-10-29: findings and proposals (milestones M0 to M4, eleven decisions).
  **Proposals, not decisions**, apart from what `design-decisions.md` records as taken.

## Reference

- `pack-format.md`: writing a data pack (blocks, patches, planets). Its examples are checked by a test.
- `protocol-and-replay.md`: the message stream and replay files.
- `world-file-format.md`: the saved world.

## Specs (one per part or phase; the part's code cites them)

Plugins and content: `plugin-api-spec.md`, `property-bags-spec.md`, `messages-and-commands-spec.md`, `client-interface-spec.md`,
`entities-spec.md`, `content-package-spec.md`, `data-packs-spec.md`, `ui-spec.md`. The game Voxunami 2048: `2048-spec.md`.
The world and the sky: `lod-spec.md` (levels of detail), `planet-spec.md`, `rails-and-recipes-spec.md` (orbits, planets from data),
`great-rivers-spec.md`, `places-spec.md` (`/tp`), `sun-spec.md`, `shadow-spec.md`. Ships: `moving-grids-spec.md`.

## History

- `decision-diary.md`: the old, whole text of `design-decisions.md` (2026-10-30), kept for the stories and numbers behind the standing rules. Not maintained.
- `build-log.md`: what each phase built and measured (about 290 KB). `grep -n "^Phase" docs/history/build-log.md` lists the phases.
- `plugin-architecture-review.md`: the 2026-10-17 reasoning behind the plugin shape.
- `joint-probe.md` (2026-11-01: do Rapier joints hold when over-constrained, with its program in `spikes/joints/`).
- `loop-probe.md` (2026-11-06: loops of cars on multibody or impulse joints, phantom momentum and free energy, cost against the number of cars, overlapping colliders, spin, and the accuracy of `BodyWorld` collisions; programs in `spikes/loops/`).
- `light-spike.md`, `lod-spike.md`, `physics-spike.md`, with their programs in `spikes/` and pictures in `multiscale-modes/`.
  The programs were written against the code of their day and are **not kept compiling**; to run one, check out the commit it names.

## Keeping them

- A phase ends by writing what it built into the build log and the decision into `design-decisions.md`; a spec gets its as-built
  notes. If it changed an interface, the reference is updated in the same commit.
- A document that is no longer true is fixed or moved to `history/`, never left to mislead. Deleting is safe in that git keeps
  everything, but the code cites specs by name, so a spec is archived, not deleted.
- The size of a document is a cost paid by every reader who loads it. A standing document that passes about 20 KB is split by area, as `design-decisions.md` was.
