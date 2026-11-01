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

## Determinism
`rapier3d-f64` 0.36.1 has the `enhanced-determinism` feature (crates.io metadata: `simba/libm_force`, `parry3d-f64/enhanced-determinism`). In the source it replaces the hash map of pairs with an ordered
`IndexMap` and fixes the transcendental functions to libm; it cannot be combined with `simd8`. Without it the solve order can differ between runs on one machine (Rapier's own regression test for
issue 868) and `sin`/`cos` differ between platforms: either breaks the rule that the simulation is bit-identical on Windows and Linux. Not built or run with f64 here.
