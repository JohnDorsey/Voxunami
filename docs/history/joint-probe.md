# Joint probe: do Rapier joints hold when over-constrained? (2026-11-01)

Throwaway programs in `spikes/joints/` (not kept compiling; Rapier 0.36.1 with `enhanced-determinism`, f32, no gravity unless said). They back the choices in `decisions/joints-and-cars.md`.
All numbers are from one run on the owner's machine. A "step" is one Rapier step of 12.5 ms.

**A correction first.** In the first runs this was called "iterations". `IntegrationParameters::num_solver_iterations` is the number of **substeps** the step is divided into
(`dt /= num_solver_iterations`, `staged_island_solver/init.rs`); the solver passes inside a substep are `num_internal_pgs_iterations`. Every table below varies substeps unless it says "pgs".

## Results (impulse joints, ball joints, 2,400 steps)
| Test | Result |
|---|---|
| Door on two consistent joints, 4 m apart; A fixed, B spun | worst pivot gap 0.004 mm (4 substeps) and 0.002 mm (16, 64). B's final spin 0.457 rad/s, as angular momentum says. |
| Three joints not on a line (rigid) | 0.024 mm (4), under 0.001 mm (16, 64) |
| Four-bar loop of four joints | 0.007 mm (4), 0.001 mm (16, 64) |
| Mass ratio 1,600:1 and 12,800,000:1 (small slab on a free huge cube, two joints) | 0.002 mm at both |
| Light bar between two huge blocks | under 0.001 mm |
| Door swung into a fixed wall (solid colliders) | 1.2 mm (4), 0.013 mm (16), 0.001 mm (64); no jitter; the door recoils at about 2 cm/s (not investigated) |
| Second joint's pivot off by 0.1, 1, 10 mm | gap 0.14, 1.4, 14 mm: permanent, 1.44 times the mismatch, no growth, no visible jitter |
| Whole door moved x metres from the origin | worst gap 0.002 mm at 0 m, 0.008 at 100, 0.06 at 1 km, 0.98 mm at 10 km; **the same at 16 and 64 substeps** |
| Multibody set | refuses a second joint on a pair and a joint that closes a loop (`insert` returns `None`; the source says so at `multibody_joint_set.rs:162`) |

- **The error floor is f32 rounding**, not the solver: it grows with the distance from the frame origin like the f32 step at that distance. `FRAME_RADIUS_BRICKS` is 500 bricks (16 km), so a joint at the
  edge of a frame is about 1 mm out. Ways down: a local origin per coupled group, multibody joints (exact in the tree, though the world pose is still f32), or `rapier3d-f64`.
- **Not valid:** the "tree under gravity" energy figures (my metric broke: the cars fall 4 km in the run). Its pivot gaps are valid: multibody 0.000000 m at 1:1 and 8,000:1, impulse 0.000488 m at 8,000:1.
- **Multibody velocity rows of the first run are not valid** (a body's velocity set directly is ignored by a multibody link); only the refusals are results.

## Speed noise, and why 64 substeps looks worse than 16
"Speed" is the swinging car's linear plus angular speed; the true value is 1.14286. `std` over steps 2,000 to 2,400 was mostly a **slow drift**, not noise.

| substeps | speed error | step-to-step jitter (rms) |
|---|---|---|
| 4 | -0.61 % | 0.0000033 |
| 16 | -0.06 % | 0.0000040 |
| 64 | +0.37 % | 0.0000162 |
| 256 | +1.60 % | 0.0000618 |

- Passes per substep (`num_internal_pgs_iterations` 1, 4, 16, 64) change nothing (-0.61 % at 4 substeps for all of them).
- Two effects meet: a **loss** that falls about tenfold from 4 to 16 substeps (the integration step is too big), and a **gain** that grows in proportion to the number of substeps (64 to 256 is
  four times the substeps and 4.3 times the gain). 16 is where they nearly cancel. The cause of the gain is *not found*; I believe it is f32 rounding added at every substep, but have not checked
  (a run with `rapier3d-f64` would say). Consistent and 1 mm-mismatched pivots behave alike.
- So: more substeps is not better. 16 is the value to use; do not raise it for accuracy.

## The move to Rapier in f64 (2026-11-01, owner's decision)
`engine/physics` now uses the `rapier3d-f64` package (0.36.1, `enhanced-determinism`), renamed in `Cargo.toml` so the source still says `rapier3d`. Its interface keeps `f32` for speeds, forces and the offset in a
`WorldPosition`; `engine/physics/src/real.rs` is where numbers cross. Orientations already were `f64` and now reach the solver unrounded. The position of a body is carried to whole bricks in `f64`, so only the offset in a
brick is rounded to `f32` (about 2 micrometres), not a place that is 16 km from the frame's origin (1 millimetre). `CharacterSettings` is `f64`.

What was checked, and how:
- **Windows and Linux agree to the bit.** The pinned hashes of the physics crate (ships, characters, ships against characters, terrain boxes) were taken on Windows and pass under Linux (WSL).
- **Every test of behaviour passed unchanged** in `physics` (39, 19, 14, 43); only hash pins moved, and a note by each says why. Two tests of `voxunami_cave` were changed in what they claim, not only in a number:
  a held ship's turn is exact and a moved ship's is the solver's `f64` one (the test used to claim that the solver rounded to `f32`), and a walker on a planet that goes round a rail is within a tenth of a millimetre of
  where it stood (it was the same to the bit). Measured every ten ticks, that walker moves 4e-6 of a voxel in the first 80 ticks and then rests; with the solver in `f32` it did not move. **The cause is not found.**
- **The box counts of the 24 terrain bricks are the old ones**; only their hash moved.
- **A hard impact resolves differently, and the new answer is the nearer to the converged one.** Busy scene of `bodies.rs`: a ship of 4, turned, dropped from 14 m, hits the ground at 25.6 m/s, which is 0.43 m a substep.
  The two builds are the same to four places until that impact. After it, with the usual 3 substeps, the old build gave a velocity of about (2.5, 5.0, 5.6) and the new about (6.2, -3.9, 7.1). With 12 substeps both give
  about (6.2, -3.9, 7.1), within a fifth of a percent of each other. (With 48 the two agree at the impact to a percent and then tumble differently.) One scene, so it is evidence, not proof. A start moved by one `f32` step
  (5e-7) changed nothing in either build, so this is not rounding noise amplified.
- Not found out: why the old build's coarse answer differs. A guess is that some constant in the contact code is `f32`-sized, which I have not looked for.

## Determinism
`rapier3d-f64` 0.36.1 has the `enhanced-determinism` feature (crates.io metadata: `simba/libm_force`, `parry3d-f64/enhanced-determinism`). In the source it replaces the hash map of pairs with an ordered
`IndexMap` and fixes the transcendental functions to libm; it cannot be combined with `simd8`. Without it the solve order can differ between runs on one machine (Rapier's own regression test for
issue 868) and `sin`/`cos` differ between platforms: either breaks the rule that the simulation is bit-identical on Windows and Linux. Not built or run with f64 here.
