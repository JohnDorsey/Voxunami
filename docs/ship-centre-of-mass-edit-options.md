# A ship on a rail has a block placed or broken: what its place does, and three ways to make it smoother

**Options; nothing here is built.** Written 2026-11-10 from the owner's replies to the ship audit (`decisions/joints-and-cars.md`, "The owner's reply to the ship audit"). **Decided (owner, 2026-11-10): the present behaviour stays for now**; the three options are on the owner's unscheduled list (`future-goals.md`) as possible enhancements. "Ran" is a script or a probe that was run, "read" is the code read and not run.

## What happens now (ran, in the ship audit, finding PHY-3b)

A ship on a rail is placed by its **centre of mass**: the ship's origin is the rail's point less the ship's turn applied to the centre of mass (`put_ships_on_their_rails_remembering`). The rail is not touched by an edit. So when a block is placed or broken and the centre of mass moves in the ship by `d`, the centre stays on its true orbit and **every block of the ship moves by the same `d`, turned with the ship, at once, in one tick.** The audit saw a 0.5 m jump. For a block of mass `m` at distance `r` from the old centre of a ship of mass `M` after the edit, `d = m r / M`: a block on the end of a long station moves the whole station by a fraction of a block, a small ship that gains a heavy tank moves by metres. A ship that turns also changes velocity at each point (`omega x d`); today the ship does not turn on a rail, so there is none.

What a player sees: from outside, the whole ship takes a step. From aboard, the world outside takes it, and the player, who stands in the ship's axes, goes with the ship. The orbit is unharmed and nothing is lost: the one cost is that the step can be seen.

## The three options

**A. Fit the rail again at the edit (no offset).** The blocks keep their places and their speeds; the rail is moved instead. The new centre of mass is at the rail's point plus `R d`, and moves with the ship's own velocity there: the rail's velocity plus `omega x (R d)` (nothing for a ship that does not turn). One orbit fit (114 to 145 us, measured in E10) in the tick of the edit, in `ServerWorld::set_grid_mass`, which is where the world learns the new centre. A merge and a split are the same act and already work this way. Result: the ship does not move at the edit, the centre of mass is on a true orbit before and after, and nothing is kept that has to be removed later. A group (step 4c-3) is the same with the group's centre of mass. What the player gets for it: a station built 20 m above the rail's old point has a period 0.5 s longer, as it should.

**B. Defer the fit with a saved offset (the owner's variant).** The rail never changes. The ship is placed on it at an offset: while the offset exists the centre of mass is not on the rail but `R x offset` from it, the offset being the sum of the shifts of the edits, so no block moves at the edit. Later, when nobody is near, the offset is deleted and the ship is placed on the rail again (a step, which nobody sees). *Ran* (`docs/history/spikes/rails/offset_vs_refit.py`, a circle of 70,000 m at 761.5 m/s, the new centre keeping the old velocity):
- a shift **along the path** stays exactly on the true path (0.001 m after 20 orbits);
- a shift **across the path** (radial) is a displaced copy of an orbit and not an orbit: it leaves the true path by `6 pi e` an orbit (9.4 m an orbit for 0.5 m; 47 m in 0.8 h, 188 m in 3.2 h), a relative speed of `3 n e` = 0.033 m/s for each metre of shift at this orbit (`n` = 0.0109 rad/s). A 0.5 m edit drifts 0.016 m/s, a 30 m station about 1 m/s. It is seen only against a body that falls by the real field (another ship, a walker, a thing let go), and a ship that comes near one is taken off its rail by `update_coasting` and goes on from the state it has, so the meeting itself has no jump.

*The worst that could happen* (read, believed; nothing was run for these):
1. The offset **grows without end** when a station is built block by block, so some fold is needed, and the fold is a fit of the rail from the true state, which is option A. B is A, later.
2. The rail's point is not the centre of mass any more, and I believe six or more places assume it is: the placement, the fit after a push, the hand-over at the edge of a sphere of influence, the predicted path, the readout, and the near rule. A place that forgets the offset makes a jump of the offset at some rare event, which is the bug the owner wants to avoid and is hard to find.
3. The offset has to be **saved** in the ship's file (older files read as none) and turned with the ship.
4. It is only right with the 4c frame. With the rail's placement of today a turn is about the rail's point and not about the centre of mass.
5. Altitude and atmosphere tests use the rail's altitude while the ship is `e` away from it: harmless against kilometre margins for a few metres, not for hundreds.

**C. New blocks weigh nothing for a time, then their weight is ramped in.** The centre of mass does not move at the edit; it moves, smoothly, over the ramp. *Reasoned, not run:*
- It does not remove the shift, it spreads it: the whole ship glides by `d` over the ramp instead of stepping. A glide of a ship that must stay on a rail is only invisible if the rail is moved along with it, which is option A applied every tick of the ramp (or B with an offset that changes). So C is not a way round A or B; it is a way to make them gentler.
- It needs a time (or a list of pending mass) for each new block, saved with the ship.
- Its real uses: a ship that turns, where a sudden block changes the spin by conservation of angular momentum and a ramp makes that smooth; and the **game idea** that a block under construction weighs nothing until it is finished. As a game idea it is separate from the rail.

## What each gives and costs

| | now | A: fit at the edit | B: saved offset | C: ramped mass |
|---|---|---|---|---|
| Ship steps at the edit | yes, by `d` | no | no | no (glides) |
| Centre of mass on a true orbit | yes | yes | no (drifts `3 n e`) | yes if combined with A |
| Rail's point is the centre of mass | yes | yes | no | yes |
| Saved in the ship file | nothing | the centre of mass (see the trap below) | the offset | the pending mass |
| Cost | none | one fit (114 to 145 us) at an edit | none until the fold, then one fit | a list and a per-tick update |
| Places that can go wrong | none | one hook | six or more | one more place for the weight |

## Traps for whoever builds A, or B's fold

- **The hook.** The world learns the centre of mass in one place, `ServerWorld::set_grid_mass`, called by the bodies plugin after the ship is placed for the tick, so the move of the rail must be made there and **before the second `apply_rails` of the tick**.
- **The first weighing of a loaded ship is not an edit.** A ship brought in from a file has a centre of mass of 0 until it is weighed. The file must keep the centre of mass, or the first weighing be recognised for what it is, otherwise every load moves every rail.
- **A ship that is not on a rail** is not covered here: I did not check what an edit does to the body of a free ship.

## What I would do (opinion, medium to high confidence)

Fit at the edit (A) if the step is ever judged worth removing: it has the fewest places to go wrong and it is what the physics says. Take C only as a feature of the game (construction), and then together with A. Do not take B: its drift is small for small edits, but it keeps the question open and needs six places to be right, and the one thing it keeps that A does not, a planned arrival to the second, is a loss of A only if the fit changes the period, which a small edit changes by little (not checked: a fit that keeps the old arrival time was not tried).

## Before building

Measure how large the step is in the cases that matter (a block on a station, a tank on a small ship; the formula above gives the size), and look at what the player who made the edit sees: they stand in the ship's axes, so it is the world outside, not their ship, that steps. If the step is too small to notice in those cases, the present behaviour is enough.
