# Physics spike: can Rapier carry Voxunami's rigid voxel structures?

Question: for free-moving voxel structures (airships, aircraft, spaceships, walking machines, vehicles carried by
other vehicles) at arbitrary rotation, in Voxunami Cave (1 m voxels) and Voxunami Sculpt (1/32 m voxels), should we
use an existing physics engine or write our own?

**Recommendation: use Rapier (rapier3d 0.36.1, parry3d 0.31.1), behind a thin Voxunami adapter.** It has a native voxel
collider that handles everything we asked of it, was deterministic across Windows and Linux in every test, and the few
things it lacks (buoyancy, per-brick material densities, splitting) are small, well-understood code on our side.
Confidence: high that it is fit for purpose for structures; moderate on how it scales to thousands of players, which
this spike did not test.

Reproduce: `cd docs/history/spikes/physics && cargo run --release -- all` (or one of `determinism scale origin perf seams
collisions joints buoyancy misc`). The project is standalone and not part of the cargo workspace. Everything below was
measured on a Ryzen 7600X, single thread, release build, 60 Hz steps, unless stated. "Verified" means a test printed it.

## What Rapier gives us (verified by reading the source and by running it)

- `Voxels` collider: a chunked sparse voxel shape with its own BVH. Voxel-vs-voxel contacts, voxel-vs-convex and
  ray casts are implemented. It classifies voxels as interior / face / edge / vertex to avoid the "internal edges"
  problem (snagging on seams between adjacent voxels).
- In-place edits: `collider.shape_mut().as_voxels_mut().set_voxel(key, filled)`. It flags a shape change.
- Joints: fixed, revolute (with limits, velocity and position motors), prismatic, spherical, pin-slot, spring, rope and
  generic; both impulse joints and multibody (articulated) joints. `IntegrationParameters::length_unit` rescales tolerances.
- Optional `enhanced-determinism` feature (forces software math), a `parallel` feature (not used here), 3D `f32` by default.

## Findings

| Question | Result |
|---|---|
| Deterministic across OSes? | Yes. 600 steps of terrain + ship + motor-driven hinge + 6 cubes gave identical position/rotation hashes on Windows and Linux (Ubuntu in WSL2) at both voxel sizes. The whole battery below (62 output lines) is identical on both apart from timings. Without `enhanced-determinism` the results were also identical across these two OSes but differ from the enhanced build, so the feature changes the maths. Same CPU architecture and compiler on both sides, so other architectures are untested. |
| Works at 1/32 m voxels? | Yes. A 24x8x12-voxel ship dropped on flat ground rests at 0.003 voxels of sink, zero jitter, and falls asleep, with plain metres as units (and with units = voxels, `length_unit` 32). |
| Arbitrary rotation? | A ship tilted 0.9 rad about (1,2,3) and dropped lands flat, sleeps, deepest hull corner 0.11 m below the surface (contact skin). |
| Terrain collider size / build cost | 2.6 M solid voxels: built in 180 ms, 5.1 MiB (2 bytes per voxel). |
| Cost of many structures | 100 awake ships (2304 voxels each) resting on terrain: 0.18 ms per step. One awake 120 000-voxel ship: 0.008 ms. Simultaneous landings of 100 ships: 4.5 ms for the worst steps. Sleeping bodies cost almost nothing. |
| Voxel-vs-voxel impacts | Two 100x30x40 (120 000-voxel) ships ramming at 16 m/s: worst step 10 ms, no interpenetration (bounced apart). |
| Edits | 1536 voxel edits to the terrain collider in 0.1 ms; a resting ship fell into the pit I dug under it. A body's mass updates by itself when its voxels are removed (1000 to 500). |
| Joints under extreme mass ratio | A 576:1 mass ratio (ship carrying a light vehicle) on a motor-driven hinge or a fixed joint: anchor separation under 0.0001 m for 10 s, ship undisturbed. A hinge motor held a 580 000 kg ship at its target angle (0.499 of 0.5 rad). |
| Articulated chains | 6-link chain: impulse joints drift up to 8 mm at anchors; multibody joints stay at 0.0000 m. |
| Distance from the origin | `f32` poses break down far out: with 1 m voxels a ship sank 4 voxels at 10^6 m but was fine at 10^5 m; with 1/32 m voxels it sank 0.03 voxels at 10^4 m and was fine below. |
| Chunked terrain colliders | Sliding at 40 m/s across tile seams: speed and hop height match a single collider, but sideways deflection grows as tiles shrink: 0.0003 m/s (one collider), 0.11 (32-voxel tiles), 0.50 (8-voxel tiles). |
| Per-body gravity in any direction | Works as an applied force; velocity after 1 s for gravity (3,-4,0)/5 g was (5.886, -7.848, 0) as expected. |
| Buoyancy | Not provided; custom per-voxel force works and is cheap: a smooth one-voxel ramp gives exactly the expected draft (4.00 voxels) and zero tilt at both voxel sizes for 0.008 ms per step on 2304 voxels. All-or-nothing counting gives 3.67 to 4.61 voxels and up to 6 degrees of wobble. |
| Own mass properties | `ColliderBuilder::mass_properties` and `RigidBodyBuilder::additional_mass_properties` both work, so we can sum per-material densities ourselves. |
| Finding disconnected pieces | Not Rapier's job; a flood fill over a 119 000-voxel structure takes 1 ms. |

## Gotchas found

- `RigidBody::reset_forces` does **not** clear accumulated torque; call `reset_torques` too. My first buoyancy test
  capsized a stable ship because of this.
- Forces persist until reset, so per-tick forces (gravity, buoyancy) must reset first.
- Mass properties take effect at the first step; reading `mass()` right after creating a body can show 0.
- Voxel keys are `i32` and local to a collider, which is fine for structures and for terrain in a local frame.
- Naively counting a voxel as submerged or not makes the force jump by whole rows; ramp it over one voxel.

## What this means for the architecture

1. **A physics adapter crate owns Rapier.** Nothing else in the engine imports it. Rapier is `f32`, so physics runs in a
   *local frame* near the action (measured fine to 100 km from the frame origin with 1 m voxels, and to 10 km with
   1/32 m voxels; keep well inside that), and the adapter converts to and from `WorldPosition`. The origin is rebased as the region of interest moves.
2. **Terrain colliders are built lazily around bodies, from bricks, as large merged `Voxels` colliders** (several
   bricks per collider, 32 voxels at the very least) to keep seam deflection low. Only surface layers need to exist.
3. **Structures are dynamic bodies with their own `Voxels` collider**; assembling, splitting and merging back into
   terrain are operations on our own grids followed by rebuilding or editing the collider. Mass properties come from our
   material densities via explicit mass properties.
4. **Gravity is applied per body as a force** from the gravity field, which also gives gravity zones and
   toward-terrain gravity for free. Buoyancy and aerodynamic forces are the same mechanism.
5. **Joints for walkers, axles, hinges and carried vehicles are Rapier joints**; use multibody joints for long
   articulated chains.
6. **Determinism**: enable `enhanced-determinism`, step single-threaded on the authoritative server, and keep the
   cross-platform hash test (a cheap form of it is the `determinism` experiment here).

## Not tested (open risks)

- Thousands of players and structures at once, and multithreaded stepping (`parallel` feature) with determinism.
- Characters walking on voxel terrain (Rapier's kinematic controller) in arbitrary gravity; likely a capsule body with
  custom forces, but not tried.
- Hollow hulls: buoyancy here counts solid voxels; a watertight hull displaces its enclosed air, which needs a
  flood fill from outside to find.
- Stacking, wheels and friction models beyond defaults; aerodynamics for aircraft; very large mass ratios beyond 576:1.
- Tuning for 1/32 m voxels with many small fast objects (continuous collision detection); tunnelling was not examined. (It was, on 2026-10-21, for ships: see `moving-grids-spec.md`, step 32j.)
- Other CPU architectures and compiler versions.
