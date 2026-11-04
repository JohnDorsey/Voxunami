# Planetary mass change (unfinished; not in the first release)

**Decision (owner, 2026-11-04): planetary mass changes do not exist in the first release of the game.** Nothing in play changes the mass of a planet today (verified, below), and nothing will be added that does until
the owner reopens this. What is built stays as it is, tested, and unused by play. This document keeps what is built, what was found about it, and the questions that a future version has to answer, so that the
work is not done twice. It is a *feature on hold*, not a plan.

## What is built (by 2026-11-03)

- `ServerWorld::answer_a_change_of_mass` (`engine/server/src/server_world/coasting.rs`): a ship on a coasting rail compares the mass parameter its elements were made with (`Orbit::mass_parameter`) with the planet's now, and
  re-fits from its place and speed **at the apsis where that is safe** (the owner's rule, 2026-11-02: an increase at the periapsis, a decrease at the apoapsis). The decisions are in `orbits-and-thrust-plan.md` (B2 as built)
  and `future-goals.md` ("A planet whose mass changes").
- Only tests change a planet's mass (`ServerWorld::set_grid_mass`). A planet's mass is set when it is created from its terrain record and saved with it; the physics plugin weighs only grids without terrain.

## The report of 2026-11-03: choices about time and width, and what mass changes do to players

*(Written for the owner at the end of E16 phase B. "Measured" means a throwaway program was run; "read" means from reading the code. The throwaway programs were removed; the numbers are here.)*

### The time grid of a hand-over (E16 phase B, choice 1)

- **Time type:** a time is `{units: i128, units_per_second: u32}`, with units under 2^96 either way. The server clock is a tick count at 20 ticks a second. A hand-over instant uses 2^31 units a second. A time given in
  seconds uses 2^20 units a second. Times in different units compare exactly.
- **2^31 is the finest grid the type allows.** Units per second is a `u32`, so 2^31 is the largest power of two that fits. Going finer means widening it to `u64`: about 90 mentions in 19 files, a few places in source.
- **Three costs of widening:** the exact comparison multiplies a 96-bit count by the rate, which fits a `u128` only while the rate is 32 bits (it needs a wider multiply or a smaller range); the range shrinks as the grid
  gets finer (2^96 over 2^40 a second is about 2^56 seconds, still plenty); and the rail bytes already store the rate as a varint, but the reader refuses anything over `u32`, so the golden byte tests would change.
- **Opinion (high confidence): do not bother.** The ship's state is exact at the instant it is handed over, so there is no jump. A late switch only means the planet's pull acted up to 0.47 ns longer than a true crossing
  would have allowed, about 1e-21 m (derived from the formula, not measured).

### The width of a rail (choice 2)

Constant width works. The +32 bits per hand-over was not needed for a prediction and the live ship to agree to the bit: both run the same function, so they agree at any constant width. It was added because the E4 work found
a close flyby loses 22 to 28 bits, and that was never turned into a distance budget.

- **Measured:** chains of two hand-overs (one through a moon and a planet, two through a slow star with an eccentricity in the hundreds), each capped width against the +32-per-hand-over chain. Capped at 128 bits throughout,
  the end position differs by 2e-30 m after 100 s and 4e-26 m after 10^8 s (scale 1e9 m): about 23 orders of magnitude below a millimetre. At 160 bits and up the chains were identical.
- **Not tested:** chains much longer than two hand-overs (losses might compound there), and a real solar-system scale (about 1,000 times larger, which does not change the conclusion).
- **Recommended rule (moderate confidence):** a rail's width depends only on its own geometry and an accuracy setting, not on history: bits for the farthest distance over the accuracy, plus the bits lost to the closeness of
  the pass, plus the turns since time 0, plus a guard, with a floor of 128 and a cap at the widest-rail setting. Once entering a sphere exists, history-based growth would add 32 bits every time a ship goes in and out of a
  sphere, and a hand-over at the cap costs 0.62 s.

### Planetary mass changes and gameplay

**Nothing in play changes a planet's mass today (read).** The one live way a mass parameter could change is a different `pull_strength` in the game definition between runs.

**A ship on a coasting rail, online or not (read and tested):**
- It does nothing at first. Its elements remember the mass they were made with.
- It re-fits from its place and speed at the right apsis: an increase at the periapsis, a decrease at the apoapsis. A circle or near-circle (eccentricity under 1e-14) re-fits at once.
- A hyperbola re-fits at once for a decrease. For an increase it waits for the periapsis if that is ahead, and re-fits at once if it is behind.
- Place and speed stay continuous, so a player aboard feels no jolt. A map's orbit line would change shape at that moment.
- A decrease that leaves the ship unbound makes it a hyperbola. A planet that pulls nothing takes the ship off its rail.
- The wait can be up to one orbit; a swing of mass larger than the eccentricity moves the apses (accepted by the owner, 2026-11-02).
- Any re-fit resets the ship's stored prediction (its count of sphere changes and its predicted count). Nothing tells a player's prediction to survive it.

**A ship off its rail** (thrusting, turning, near something, or in a level with its own pull) feels the new pull at once through the physics, and goes back on a rail after a quiet second with the new mass.

**Offline players:**
- A pilot's ship stays unless it is set to vanish or the host's logout policy removes it.
- A frozen ship still in memory is processed by the same per-tick code (read: the loop does not skip frozen ships; no test covers it).
- A ship parked in a file (more than 1,024 voxels from every player) is not processed at all. It follows its old orbit until a player comes near and it loads, and then waits for the next right apsis. So a parked ship and a
  watched ship take different paths after the change. The docs already accept this; anyone returning to an orbiting ship could see its orbit change at its next apsis.

**A problem found (measured, in the E16 phase B design):** the size of a sphere of influence depends on the masses, so a mass change moves the sphere.
- *A shrinking sphere.* The exit instant is searched from the start of the ship's orbit, so a ship between the new and the old edge is handed over at a **past** instant: the star's orbit replaces the planet's retroactively
  and its position jumps. Test world (`exit_ships.rs` scale, 20 ticks a second): doubling the star's mass shrinks the planet's sphere from 39,811 to 30,171 voxels; one tick later the ship's position differed from the
  unchanged run by 1,939 voxels with the ship 3 s before the old exit, 18,730 voxels 1 s before, and 30,264 voxels at the old exit: a visible teleport of kilometres.
- *A growing sphere.* Halving the star's mass grows the sphere to 52,531 voxels. A ship just outside it comes off its rail for about 2 seconds, then goes on a planet rail: it is captured. A position jump was not measured; the
  ship keeps its state while it is a free body.
- Planets and moons on files' rails have a stored period and ignore mass changes; only ships answer them.

**Options for the rules, if the feature comes back:**
1. A mass change takes effect for spheres only at a ship's next hand-over or re-fit (cheapest to build).
2. Search for the exit only from the moment of the change (fixes the teleport, but leaves a ship already outside the new sphere on the wrong rail).
3. For fewer rules to memorise: mass changes take effect only at a moment the server picks (a daily tick, or when a planet is "settled"), and every ship answers at the next apsis after that, parked or not.

The leaning (the assistant's, 2026-11-03) was 3 combined with 1, which also removes the parked-versus-watched difference.

**The latent flaw stays in the code until then:** with no mass changes in play it cannot happen, but any future source of a changed mass (mining, cargo, a game update that edits a planet's declared mass, a ship heavy
enough to pull) would trigger the teleport above.

## Future block for Voxunami Cave: the ARGOS device

*(The owner's text, 2026-11-04.)*

**ARGOS device.** "The Alternate Reality Graviton Obligation Swapper puts your ship in contact with an alternate universe where nobody lives, and exchanges the gravitational effects of your universe with the gravitational
effects of that one. Simply put, it allows you to ignore changes in gravity caused by nearby ships and players' modifications to the mass of planets. Quantum physicists are concerned about where that lonely alternate
universe comes from -- some suggesting that every time you turn on this machine, it splits our universe into two duplicate universes, and then kills everyone living in one of them."

*Needs, as I see it (not decided):* a block or ship part of Cave's that, while on, makes the ship's gravity ignore the pull of ships (the threshold `pull_mass_threshold` already separates what pulls) and any change of a planet's
mass since the device was switched on, using the masses at that moment instead; a way to keep those masses per ship (the ship would hold the snapshot, saved with it); and the interplay with rails (a ship on a rail already
ignores ships and answers a planet's mass only at an apsis). It only matters once planets' masses change or ships pull, so it comes with this feature.
