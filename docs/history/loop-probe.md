# Loop probe: what jointed cars and colliding ships do when nothing pushes them (2026-11-06)

The owner asked (2026-11-06): how do the two ways of closing a loop of cars (multibody tree + impulse joints for the loop; multibody tree + a direct "MLCP" solve for the loop) compare in cost and in
glitches (phantom forces, free kinetic energy); is our collision physics accurate; can jitter and rattle between ships in contact be eliminated; what do the best simulators do.
Programs: `spikes/loops/` (a standalone project: `cargo run --release --bin loops|probes|collisions`, `all` or a letter as the argument; `collisions` links the real `physics::BodyWorld`).
Like `joint-probe.md`, the programs are **not kept compiling**. Every number is from one run on the owner's Windows machine, Rapier 0.36.1 in `f64` with `enhanced-determinism`; the runs are
deterministic, so a rerun gives the same digits. The rigs are mine (below), not ships of the game. Where a claim is from reading Rapier's source, from the two research agents, or is belief, it says so.

## What I got wrong first (read before trusting anything else)
- **A multibody puts a child where the joint's coordinates say, not where its body is.** My first ring was snapped into a straight 12 m line at its first step and the closing joint then fought a
  12 m gap: velocities of 1e20 from rest. That looked like a Rapier instability and was my setup. The joint frames must be chosen so that the zero coordinates are the pose the child is in
  (`multibody_ball` in `spikes/loops/src/lib.rs`). Every number below is after the fix. (A real hazard for the game when it builds a multibody from ships that already exist.)
- **A multibody ignores the velocity set on its bodies**; the root's velocity is the first six generalized velocities. My first "push" vanished for that reason.
- **Tunnelling:** I reported plates being passed through at 5 to 50 m/s. That was two mistakes of mine: an off-centre hit spins a light plate, so "which side is the cube on" meant nothing, and my
  spike steps a fixed number of times, where `BodyWorld` cuts a tick into more steps for a fast body. Redone through `BodyWorld` with the plate's rotation locked: no tunnelling to 500 m/s (section 8).
- **Second round (same day):** (a) I wrote "Friction is Coulomb and exact" in section 9 after sliding tests only; the default model is Rapier's *simplified* one and a spinning plate loses its spin 1.85 times too
  fast (section 13). (b) The velocity a link of a multibody holds in its body is a copy that lags the integrator's own state (`J q'`, the jacobian times the generalized velocity) by about acceleration times step; where I
  say "bodies" a number was read from the first, "generalized" from the second. For the free runs of sections 2 to 4 they agree to three digits (1.02e-2 against 1.03e-2 m/s, 0.420 against 0.420), so those stand.
- In the chat before this I **believed** Rapier accepts impulse joints between two links of one multibody. Now read in the source (`fill_relative_jacobians`, `generic_joint_constraint_builder.rs`: "e.g. a
  loop closure") and run. And I said a joint fighting overlapping colliders would give "no net drift"; true for impulse joints (momentum exactly 0), **false for a multibody joint** (section 6).

## 1. The rig and the measures
Cars are boxes of 3 x 1 x 1 m, 1000 kg unless said, no colliders (so no contacts), joined end to end by ball joints into a ring of 4 (a closed loop), optionally with a chain of more cars hung off one
of them (the tree). Space, no gravity. The rig moves and spins rigidly (0.45 m/s, 0.5 rad/s), is pushed for one second (a force and a torque), then left alone: **nothing acts on it, so linear momentum,
angular momentum and kinetic energy must not change**. Columns: *dv* = largest change of the centre's velocity after the push (m/s); *centre off* = how far the centre strays from the straight line it
must follow (m); *dL/L*, *dE/E* = angular momentum and energy relative to their values after the push; *gap* = worst distance between the two anchors of any joint. The game's own step is a 50 ms tick
in 3 steps with Rapier's 4 substeps each: 4.2 ms. "Impulse" = ordinary Rapier impulse joints on every joint (maximal coordinates). "Multibody" = the tree on multibody joints. "+ impulse closing" = the
loop's last joint an impulse joint between two links of the same multibody (Rapier's own loop closure). "+ direct" = my prototype (section 5).

## 2. Ring of 4, 40 s free, 4.2 ms (`probes exact`)
| closing | gap m | dv m/s | centre off m | dL/L | dE/E | us/tick |
|---|---|---|---|---|---|---|
| all impulse joints | 2.3e-5 | **1.3e-14** | **2e-13** | 9.1e-3 | -1.5e-2 | 26 |
| multibody, no loop (open chain) | - | 1.0e-2 | 0.12 | 0.13 | -3.6e-2 | 121 |
| multibody + impulse closing | 9.1e-6 | 7.3e-3 | 0.19 | 5.2e-2 | -2.5e-2 | 139 |
| multibody + direct, Rapier's factorization | 7.8e-15 | 1.2e-2 | 0.29 | 7.6e-2 | -2.1e-2 | 170 |
| multibody + direct, true mass matrix | 8.4e-15 | 1.2e-2 | 0.26 | 8.1e-2 | -1.8e-2 | 212 |

At 1.0 ms the same rows read: impulse 1.4e-6 gap, dv 9e-15, dL/L 2.3e-3, dE/E -0.4 %, 103 us; multibody + impulse closing dv 1.8e-3, centre off 0.048, dL/L 1.4e-2, dE/E -0.8 %, 536 us.

Ring of 4 plus a chain of 12 (16 cars), 100 s (`loops b`), 4.2 ms: all impulse gap 8.5e-4, dv 1.5e-14, centre off 4.7e-13, dL/L 1.3e-3, dE/E -0.26 %, 51 us/tick; multibody + impulse closing gap 6.9e-6,
**dv 1.39, centre off 70 m**, dL/L 0.66, dE/E -6.7 %, 1935 us/tick; multibody + direct dv 3.4, centre off 185 m. With a step of 16.7 ms the multibody rig **gains 74 % kinetic energy** (all impulse: -0.9 %).
Mass ratio 1 kg : 10^6 kg (ring 4 + chain 4, 50 s): all impulse gap 2.7e-4, dE/E -1.4 %; multibody + impulse closing dE/E **+1.6 %**, direct **+7.4 %**.
- **Linear momentum.** Impulse joints conserve it to rounding at every step, because each impulse acts equally and opposite. A multibody does not, **even with no loop**: the centre of a spinning rig
  wanders. The loop method hardly matters; the multibody under it does.
- **Energy.** In every run the energy fell (first-order damping of the scheme) except the multibody at a coarse step or a huge mass ratio, where it gained. No impulse-joint run gained energy.
- **The loop itself closes well in both** (9e-6 m for Rapier's own, 1e-14 for the direct solve). Strain from a built-in mismatch of the closing anchor (1 mm to 10 cm) was held as a gap equal to the
  mismatch, without any change of energy or momentum (`probes strain`).

## 3. The step: both schemes are first order (`loops e`, open chain of 4, 20 s)
| step | multibody dv / centre off / dL/L / dE/E | impulse dv / centre off / dL/L / dE/E |
|---|---|---|
| 16.7 ms | 2.6e-2 / 0.26 / 7.0e-2 / -6.5 % | 1e-15 / 1e-14 / 1.1e-2 / -4.7 % |
| 4.2 ms | 6.4e-3 / 0.065 / 2.2e-2 / -1.9 % | 2e-15 / 3e-14 / 2.7e-3 / -1.2 % |
| 1.0 ms | 1.6e-3 / 0.016 / 5.7e-3 / -0.6 % | 1e-14 / 1e-13 / 6.8e-4 / -0.3 % |
| 0.52 ms | 8.0e-4 / 0.008 / 2.9e-3 / -0.4 % | 8e-15 / 5e-14 / 3.4e-4 / -0.16 % |

Halving the step halves every error that is not exactly zero: both converge, to the same trajectory (a reference at 0.13 ms: the two schemes' last car differ by 1.3 mm at 10 s), at first order.
Ring + chain of 12 (16 cars, 40 s, `probes steps`): the multibody's centre is 48 m off its line at 16.7 ms and **2.7 m off at 1.0 ms**; the impulse rig's gap is 15 mm at 16.7 ms and 0.045 mm at 1.0 ms.

## 4. Cost (`loops d`: microseconds per 50 ms tick, ring 4 + chain of n, game step)
| n | all impulse | multibody + impulse closing | multibody + direct | the direct part |
|---|---|---|---|---|
| 0 | 15 | 122 | 121 | 11 |
| 8 | 39 | 963 | 986 | 38 |
| 24 | 86 | 8,450 | 8,410 | 141 |
| 56 | 175 | 71,600 | 71,200 | 562 |
| 120 | 365 | 586,000 | 585,000 | 2,264 |

**Rapier's multibody grows about as the cube of the cars** (n = 56 to 120: 8.3 times for 2.1 times the cars; read in the source: a dense augmented mass matrix and its LU every substep), so past about 30 cars
one ship does not fit a 50 ms tick; impulse joints grow about linearly. The direct closing solve is cheap beside the multibody it sits on (3 solves with Rapier's LU; my loop is 3 rows).

## 5. The direct closing solve (prototype, `close_loop_directly`)
After each Rapier step: refresh the multibody's kinematics, build the 3 rows of the loop's jacobian from the public `body_jacobian`, solve the 3 x 3 system `(J M^-1 J^T) x = rhs` with `M^-1` from
Rapier's `inv_augmented_mass` (or from a true mass matrix built from the jacobians), remove the relative velocity of the anchors (velocity change `M^-1 J^T lambda`) and close the gap with the smallest
displacement in the mass metric (`apply_displacements`). Result: the gap is machine precision after one pass (3e-11 with one pass, 8e-15 with two). I predicted it would add a phantom momentum through
the mismatch between Rapier's (stale, augmented) mass matrix and the true one; **the true mass matrix changed nothing** (the last two rows of the table in section 2), so that was wrong: the drift is the
multibody's. **Not tested:** loops of hinges (parallel hinge axes make the loop's rows dependent, so `J M^-1 J^T` is singular and a plain Cholesky fails), limits and friction in the loop, a loop that
also meets contacts (a solve after the step is operator splitting and may rattle against them; belief), and inside-the-substep placement, which needs a Rapier fork.

## 6. Contacts against joints, "the Kraken" (`probes overlap`)
Two cars (3 x 1 x 1 m, 1000 kg) alone in space and at rest, their boxes overlapping, a ball joint inside the overlap. Nothing acts on them.
| overlap | joint | most kinetic energy | highest speed | largest total momentum |
|---|---|---|---|---|
| 0.5 m | impulse, contacts on | **1.19 MJ** | 23.5 m/s | 0 |
| 0.5 m | multibody, contacts on | 0.70 MJ | 28.8 m/s | **28,800 kg m/s** |
| 0.05 m | impulse / multibody, contacts on | 1.5 kJ / 53 kJ | 0.6 / 7.8 m/s | 0 / 7,100 |
| 0.005 m | impulse / multibody, contacts on | 2 J / 4.2 kJ | 0.02 / 1.8 m/s | 0 / 38 |
| any | contacts off (`contacts_enabled(false)` on the joint) | **0** | 0 | 0 |

The contact solver pushes the overlapping boxes apart, the joint pulls them back, and the fight makes energy from nothing; with a multibody joint it also makes momentum. Rapier skips the contacts
between two bodies if the joint between them has contacts disabled (read: `pair_update.rs`, impulse joints, and multibody joints inside one multibody; run: exactly zero). A static joint between grids
that overlap, and every joint of a ship, must turn them off.

## 7. Fixed joints and the dials
**Chains of fixed joints** (one rigid thing), moving and spinning 20 s (`probes fixed`): multibody keeps the gap at 1e-15 and costs 42 / 110 / 370 us per tick for 4 / 16 / 60 cars but drifts the
centre's velocity by 1.9e-3 / 9.4e-3 / 3.7e-2 m/s; impulse joints conserve momentum exactly (6e-15) and stretch 0.016 / 0.38 / 8.7 mm for 19 / 80 / 286 us. A rigid assembly is best a single body
with several colliders (not measured; belief: Rapier does not collide colliders of one body).
**Dials** (`probes dials`): a chain of 12 on ball impulse joints pulled along its length by 20 kN, worst joint gap and cost; the last car 1,000 kg / 10^6 kg:
| setting | gap (mm) | us/tick |
|---|---|---|
| game (3 x 4 substeps, 1 pgs) | 0.64 / 1.18 | 36 |
| pgs 4 | 0.10 / 0.28 | 48 |
| pgs 16 | 0.015 / 0.042 | 87 |
| 3 x 16 substeps | 0.033 / 0.044 | 112 |
| per-body `additional_solver_iterations` 4 | 0.13 / 0.30 | 62 |
| per-body 16 | 0.021 / 0.033 | 140 |
Rapier has these dials **per body** (`set_additional_solver_iterations`, `set_additional_pgs_iterations`: read; the island takes the largest of its bodies), so a per-ship setting exists already.
Stiffness improves 10 to 40 times for 2 to 4 times the cost; a mass ratio of 10^6 costs 2 to 3 times the gap.

## 8. A spinning body gains (or loses) energy (`probes spin`, one free car, 1 hour)
Rapier's gyroscopic step for a free body is explicit with its angular momentum kept (`gyroscopic_corrected_angvel`; a comment there says an implicit one is "very dissipative"; multibody links use
another, semi-implicit path). A spin about the **largest** axis, which is stable, **drifts to the intermediate axis**, so the energy at constant |L| rises; about the smallest axis it falls. The intermediate axis
is a fixed point of the scheme. dE/E after 1 / 10 / 60 minutes, game step 4.2 ms: tensor (1,2,3), 5 rad/s, largest axis: +0.2 % / **+50 %** / +50 %; 0.5 rad/s: +2e-7 / +3e-6 / +1.5e-4.
Hull 20 x 6 x 4 m (52, 416, 436): 5 rad/s largest axis +1.4e-5 / +4.8 % / +4.8 %; 0.5 rad/s +4e-8 / +4e-7 / +5e-6; smallest axis at 0.5 rad/s -4e-5 / -5.5e-4 / **-1.5 %**. |L| holds to 1e-10. A step of
1.0 ms delays the rise (the hull's +4.8 % at 5 rad/s arrives after 30 minutes instead of 10). So: harmless at a station's rotation (0.1 to 0.5 rad/s) for hours; a ship left tumbling at several rad/s changes axis within minutes.

## 9. Collisions through `BodyWorld` (`collisions`; the game's crate, its default 50 ms tick and 3 steps; restitution is 0 in the game)
- **Momentum and energy.** Head-on and glancing hits conserve momentum (0 to 1e-16) and angular momentum (2e-6 to 6e-5); no run gained energy. A glancing hit (cube of 4 at 10 m/s, 1.5 m off centre) leaves
  54 % of the kinetic energy and the same answer to 1 % from 3 to 96 substeps.
- **Spurious bounce.** (A voxel is a metre in the game's physics, `players-and-ships.md`.) Two equal cubes meeting head on at a relative 10 / 100 / 400 m/s should stop dead; they separate at 0.047 / 0.74 / 1.26 m/s (0.5 %, 0.7 %, 0.3 % of the speed) at 3 substeps. Below 1 m/s: 0.
  Their overlap at the moment of impact: 1.6 mm at 10 m/s, **0.74 voxel at 100 m/s** (0.31 at 12 substeps).
- **Friction in a straight slide** is exact (corrected in the second round, section 13: Rapier runs its *simplified* friction model by default, not one Coulomb constraint per contact, and spin friction
  on a flat face is not exact): the acceleration of a cube on a tilted floor matches `g (sin a - mu cos a)` to four digits at 29.2, 31, 40 and 60 degrees (combined mu 0.55 = (0.6 + 0.5) / 2), it sticks at 27 and
  accelerates at the predicted rate from 29.2 (the threshold is 28.81). **No snagging at brick seams:** a cube sliding at 8 m/s over a seam stops within 0.01 voxel of the friction prediction (11.6 voxels) and its height varies by less than 0.4 mm.
- **Resting contact is jitter-free in these tests**: a cube on the floor and a small cube on a larger one, held awake by a force of 1e-6 N, move at 4e-15 to 2e-13 m/s and sink by 0. (Asleep they move by exactly 0.)
- **Tunnelling** (voxel hull plates 1, 2 and 4 voxels thick, light or heavy, a cube of 8 kg aimed 0 to 3 voxels off centre with the plate's rotation locked): none from 20 to 500 m/s; a body at 3000 m/s goes through
  terrain or plate (the cap of 24 steps of 0.8 voxel is 384 voxels/s). Without the speed-dependent steps (my spike's fixed 12 substeps of 4.2 ms) a cube does go through at 150 m/s, so they are what protects.
- **Sleeping destroys slow motion.** A body alone in space moving at 0.08 m/s or less **falls asleep after 0.5 s and stops dead**; 0.1 m/s and more keeps going (`collisions threshold`). So a 1 kg cube hitting a
  512 kg cube at 10 m/s should give both 0.0195 m/s; the heavy cube sleeps at 0 and the light one comes back at -0.026 m/s. Momentum is not conserved below 0.1 m/s; a slow docking approach or a drifting
  derelict is frozen. (Whether `voxunami_cave` wants this is for the owner; Rapier's `RigidBodyActivation` thresholds would be the dial.)
- **Not tested:** restitution above 0, two grids turned against each other (the odd-angle case of a static joint block), friction between two ships, many contacts, the character controller.

## 10. What the research found (two agents, secondhand; I read none of the pages myself except where marked)
Sources: Rapier user guide and changelog (rapier.rs, github dimforge/rapier), PhysX articulations guide, MuJoCo computation docs, Box2D Solver2D post, Macklin et al. "Small Steps in Physics Simulation", Erez et al. ICRA 2015
(roboti.us), Tsounis et al. 2025 (arXiv 2504.19771), Servin/Lacoursiere SPOOK (ep.liu.se/ecp/027/005), AGX Dynamics manual, Dojo (arXiv 2203.00806), Hairer (RATTLE).
- Loops are closed by compliance (MuJoCo, Drake), iterative impulses (Box2D, Jolt, PhysX loop joints, Rapier) or a **direct solve of the joint rows (AGX, Vortex)**. Evidence at mass ratios of 10^6 exists only for the
  direct/implicit family (vendor material); iterative evidence stops near 1000 : 1, needing about 100 substeps.
- Bounded-energy results exist for RATTLE, variational integrators and SPOOK with converged solves; none was found for contact + friction + truncated iteration. Game engines are bounded by dissipation. Erez et al. found joint-coordinate
  integration conserves linear momentum worse than Cartesian (matches section 2). MuJoCo's docs: explicit gyroscopic terms can gain energy (matches section 8).
- Resting contact, best evidence first: many substeps with one iteration beat many iterations (Macklin; Catto); soft contact with a relax pass; direct solve of joints and normal forces (AGX); convex contact (MuJoCo, Drake).
  Only sleeping gives exactly zero motion. No benchmark of still contact between two co-moving bodies was found.
- Usually ruled out, possibly affordable here: direct factorization of the joint rows (zero fill on trees; cheap at our scale; deterministic with fixed-order `+ - * / sqrt`); implicit integration (Chrono HHT; MuJoCo implicit loses energy);
  volume-overlap contact for voxel bodies (Allard et al.: an LCP from intersection volumes; a CPU integer-grid version is a natural prototype, untested). Poor fits: exact LCP (Lemke/Dantzig/PATH), event-driven exact time of impact, adaptive
  steps, hydroelastic contact (rigid-rigid unsupported; tetrahedral meshes).
- Validation methods worth copying: energy and momentum against analytic cases; Andrews' squeezer for loops; a state hash over all bodies across compilers (Box2D); Galilean invariance (add one velocity to every body, relative motion must
  match to 1e-12); a floating assembly with loops must not move its centre; torque-free tumble; a 10^6 : 1 pair.
- Games with player-built jointed vehicles (Space Engineers, Kerbal Space Program; developer posts and community patches): phantom forces from several connections between the same two bodies and from sequential solves with unequal masses;
  "Clang"/"Kraken" from subgrid colliders overlapping another grid (section 6 reproduces it in numbers); drift and joint failure at dock, undock, load and pack; Space Engineers fixed these by stronger constraints, an opt-in shared
  inertia tensor, keyframed pistons with an impulse limit, no impact damage inside one assembly; Trailmakers merges blocks into weld groups that are one body and lets adjacent groups pass through each other; Star Citizen keeps each
  player in a physics grid local to the ship. KSP forum threads and Squad's dev diary could not be read. Sources: blog.marekrosa.org (2017/11/physics, 2014/12 large worlds), spaceengineers.wiki.gg/wiki/Clang, the KJR and
  KSPCommunityFixes READMEs on GitHub, trailmakers.wiki.gg/wiki/Building, starcitizen.tools Comm-Link August 2014.

## 11. What this suggests (my opinion; the decisions are the owner's)
1. **The choice that matters is the base, not the loop method.** Under the multibody both loop methods inherit first-order momentum drift and a cubic cost; the loop closes well either way. Maximal-coordinate impulse joints with the
   dials conserve linear momentum exactly, cost about linearly, and trade a small stretch (mm, falling 10 to 40 times with the dials) and a first-order loss of angular momentum and energy (a few percent per 100 s at the game's step).
   Confidence: high that this is what the numbers say for these rigs; medium that it holds for real ships (flailing rigs exaggerate the multibody's drift; ships with fixed joints drift less, section 7).
2. A per-ship choice is cheap to offer if it lives in the one box the decision already asks for ("a coupling = two cars and pivot pairs"): `impulse` (default, with dials) and `multibody` (kept for small, articulated, load-free
   assemblies), and the dials (`additional_solver_iterations`, `additional_pgs_iterations`) as ship data. A direct loop solve beside Rapier is worth building only if a loop's gap must be exact; it does nothing for momentum.
3. Whatever the solver: **contacts off between the two bodies of every joint, and between non-adjacent cars of one rigid group** (section 6); a rigid group as **one body** with several colliders where it never moves.
4. Open: the gyroscopic drift (section 8: more substeps for a fast-spinning ship, or our own exact torque-free rotor step), the sleeping thresholds (section 9), the deep overlap at 100 m/s (0.74 voxel at 3 substeps).
5. Not done: hinge loops and singular rows, loops under contacts, a direct solve inside the substep, rotated-grid contacts, and any change to the game. (A wide-number Rapier fork is sized in section 14; two isolated steps are tried there too.)

## 12. A ship on a rail: hold the centre and the spin (`rails`, second round, 2026-11-06)
The owner asked whether the multibody's momentum drift can be removed for a ship on a rail, and whether the rail can be the centre of mass instead of a pinned root car (`orbits-and-thrust-plan.md` section B already makes
the rail the centre of mass). The test: after every step, work out the error of the cars' total momentum (and, in the next modes, of the centre's place, and of the angular momentum about the centre) against what the rail
says, and take it out of every car as **one rigid shift**. A rigid motion changes the velocities of the two anchors of a joint by the same amount (they are the same point), so no joint, loop or relative velocity is
touched. For a multibody the shift goes into the root's first six generalized velocities (read: the root's linear columns of the jacobian are the identity, so they are world axes) and its first three coordinates
(`apply_displacements`). The error is measured on the generalized velocities `J q'`, which is what the next step integrates (the bodies' own copy lags, section "What I got wrong first"). Code: `hold_to_rail` in
`spikes/loops/src/lib.rs`, `rails.rs`. The rail is the line the centre had when the push ended; the outside force, if any, is known (`thrust` below), as the rail's acceleration is.
Multibody + Rapier's impulse closing joint, game step (4.2 ms), worst value after the push; "held" = momentum and centre, "+ spin" = also the angular momentum about the centre:
| rig | | free | held | + spin |
|---|---|---|---|---|
| ring of 4, 40 s | centre velocity change m/s | 1.1e-2 | 9.6e-16 | 9.1e-16 |
| | centre off its line m | 0.26 | 4.5e-13 | 4.5e-13 |
| | spin about the centre, relative change | 1.5e-2 | 1.5e-2 | 1.2e-15 |
| | kinetic energy | -2.3 % | -2.5 % | -0.4 % |
| ring of 4 + chain of 12, 100 s | centre velocity change m/s | 1.39 | 3.7e-15 | 3.6e-15 |
| | centre off its line m | 70.6 | 2.0e-12 | 2.1e-12 |
| | spin about the centre | 7.1e-2 | 7.1e-2 | 1.2e-15 |
| | kinetic energy | -6.7 % | -13.5 % | -0.075 % |
| same, step 16.7 ms (1 substep) | centre velocity change m/s | 5.66 | 2.8e-15 | 1.3e-14 |
| | centre off its line m | 293 | 2.2e-12 | 2.2e-12 |
| | kinetic energy | **+74 %** | -37 % | **+3.2 %** |
The open chain of 16 cars (no loop) is the same story: centre off 13.7 m -> 2e-12, spin changes 53 % -> 3e-15, energy -74.5 % -> -76.5 % (held) -> -3.4 % (+ spin); at 16.7 ms its energy goes +16 % with the spin held.
With a constant 500 N thrust on one car (the rail accelerates; the target momentum is `P0 + F t`): centre velocity error 1.4e-13, centre off its line 1.4e-11 (held). The all-impulse rig (control) is unchanged by the
hold (1.5e-14 -> 2e-15; with the spin held its angular momentum drift goes 1.3e-3 -> 1.5e-15 and its energy -0.26 % -> -0.008 %). Cost: not measurable (16 cars, 1902 us against 1913 us per tick).
The gap of the joints is not made worse by the shift itself, but the trajectory changes, so it can move: 6.9e-6 -> 7.5e-6 m at the game step with the spin held, 1.1e-4 -> 2.6e-4 at 16.7 ms.
- **What it shows.** The multibody's momentum drift is a drift of the *rigid-body modes* (centre place, centre velocity, spin about the centre): they are not preserved by the integrator, and they can be put back without
  touching the joints. The "free energy" of the 16.7 ms rig (+74 %) was almost all the spurious motion of the centre; with it removed the rig shows its real loss (-37 %).
- **What it does not do.** Energy is not a rigid mode; the scheme's damping stays. Restoring the *spin* the scheme has lost also puts energy back, and at a coarse step it overshoots (+3.2 %, +16 %): at the game's step the
  energy error gets smaller (-6.7 % -> -0.075 %), at a coarse step its sign can flip. Not tested: a ship that touches something outside (the target changes by the contact impulses; **correction in section 14**: Rapier keeps the normal impulse of a
  contact as the step's total, but the friction impulse only as the last substep's warm-start, so the sum is not available from the contact pairs and section 14 gets it from a separate contact pass instead),
  a torque from outside with the spin held, hinge loops, a root that is not a free body. Inside the multibody, the relative motion of the parts is the multibody's own, with its first-order error.
- **Root pinned or centre pinned (reasoning, not measured).** A kinematic root car is an anchor of infinite mass: every reaction of the parts that swing against it goes into the rail, so the ship's centre wanders
  off the rail by (mass of the swinging parts / total mass) times their excursion, and the rail gives the ship a push that no engine made. The centre pinned (above) has no outside push by construction.

## 13. Friction model, spin friction and the skin (`models`, second round)
- **Read:** the default `FrictionModel` in 3D is `Simplified`: one Coulomb constraint per group of four contacts of a manifold plus one "twist" constraint; `BodyWorld` keeps the default (`..IntegrationParameters::default()`).
  The collider's `contact_skin` is 0, `normalized_prediction_distance` 0.02 and `normalized_allowed_linear_error` 0.005 (times the length unit, here 1 m).
- **Straight slide** of a 2 x 2 m plate (100 kg, mu 0.6 both): simplified 5.886 m/s^2 = mu g exactly. One Coulomb constraint per contact: 5.063, 14 % low at 1, 4 and 16 iterations (unexplained; not the default).
- **Spin friction of a flat plate** (spun at 3 rad/s on the floor, mean deceleration until the spin is halved): both models give **12.486 rad/s^2** for a 2 m plate and 4.162 for a 6 m plate, which is exactly what four
  point contacts at the corners give. A uniform pressure over the face gives 6.756 and 2.252, so a flat square loses its spin **1.85 times too fast**, whatever its size. The contact manifold of a flat face is its
  corners, so friction torque is as if the weight sat there. (The uniform pressure is itself an assumption; the true one for a rigid plate on a rigid floor is not determined.)
- **Resting offset ("skin")** of that plate after 8 s: **-0.120 mm** (sunk in), the same for 1 kg to 10^6 kg and for the game step, 12 steps and 16 substeps. The skin is nothing a player could see.

## 14. Two isolated steps, and what a wider number would buy (`split`, `accuracy`, third round, 2026-11-06)
The owner asked (a) whether a ship's joints and its collisions can be two separate Rapier steps, the joint step never touching the rail and the collision step (run only where something collidable is near) allowed to;
(b) whether Rapier can be forked to 128 bits or more; (c) what that would do for the accuracy of impulse joints, which the owner expects to keep.

**(a) Two steps.** Read in the source: `PhysicsPipeline::step` takes the body, collider and joint sets as arguments, so two worlds (two pipelines, two sets of bodies) can be stepped one after the other; `BodyWorld`
steps one world with everything. Tried in `split.rs`: a *joint world* (cars without colliders, impulse joints, no gravity) and a *contact world* (colliders, terrain, gravity, no joints). Each step: copy the truth into the
contact world and step it; keep its **velocities** and throw its poses away; put those velocities on the truth's poses in the joint world and step that. The rigid ship is N cubes of 1000 kg joined by fixed impulse joints,
the game step (3 steps of 16.7 ms, 4 substeps). Three schemes: **combined** (one world, what `BodyWorld` does), **per car** (the contact world holds each car as its own free body), **whole ship** (the contact world holds
the ship as ONE body: all its colliders, its total mass and inertia; the velocity change it gets is given to every car as one rigid motion, which no joint resists).
Tower on a floor, 15 s, statistics of the last 5 s (cubes: a flat face stands on four corner contacts):
| cars | scheme | what happened | bottom sag mm | off the axis mm | worst joint gap um |
|---|---|---|---|---|---|
| 2 | combined | stands | -0.138 | 0.05 | 25 |
| 2 | per car | tilts and falls over | | 1448 | 1.4e6 |
| 2 | whole ship | stands | +0.005 | 0.06 | 38 |
| 4 | combined | stands | -0.276 | 0.31 | 354 |
| 4 | per car | falls over | | 3533 | 1.4e6 |
| 4 | whole ship | stands | -0.009 | 0.08 | 23 |
| 8 | combined | stands | -0.552 | 0.55 | 2564 |
| 8 | per car | sinks through the floor (44 m) | | 4908 | 1.1e6 |
| 8 | whole ship | stands | -0.004 | 0.15 | 20 |
| 4, 12 steps a tick | combined / per car / whole ship | stands / falls over / stands | -0.276 / / -0.007 | 0.03 / 3555 / 0.08 | 27 / 1.4e6 / 22 |
The whole-ship tower keeps a residual speed of 0.27 to 0.85 mm/s (the combined step sits at exactly 0, and 0.08 mm/s at 8 cars).
Rod of spheres (one contact point, so that nothing in a head-on hit can turn it; with cubes the combined rod came away turning at 0.34 rad/s, which I take to be the order of the four corner contacts) at 5 m/s into a wall, 4 s, no gravity:
| cars | scheme | into the wall mm | its speed at the end mm/s | worst joint gap um |
|---|---|---|---|---|
| 1 (no joint) | combined | 0 | 0 | |
| 2 | combined | 0 | **1196** (moving away: 24 % of the impact speed, traced) | 0 |
| 2 | per car | 83 | 0 | 0 |
| 2 | whole ship | 0 | 0 | 0 |
| 4 | combined | 0 | **1661** | 88 |
| 4 | per car | 240 | 0 | 121 |
| 4 | whole ship | 0 | 0 | 0 |
| 8 | combined | 4.2 | **2045** | 12958 |
| 8 | per car | 524 | 0 | 3294 |
| 8 | whole ship | 0 | 0 | 0 |
| 4, 12 steps a tick | combined / per car / whole ship | 0 / 60 / 0 | 1297 / 0 / 0 | 0 / 2 / 0 |
The rebound of the **combined** step is not a bug in the probe (one sphere stops dead) and not the step size; it goes away with passes (Rapier's `num_internal_pgs_iterations`), more of them the longer the chain:
| rebound mm/s, 5 m/s impact | 1 pass | 2 | 4 | 8 | 16 | 32 | 16 substeps: 1 pass / 8 passes |
|---|---|---|---|---|---|---|---|
| 2 cars | 1196 | 880 | 266 | 17 | 0.1 | 0 | 1042 / 11 |
| 4 cars | 1661 | 1615 | 1333 | 840 | 335 | 27 | 1297 / 525 |
- **What it shows.** (1) Giving the contact its own pass with the ship's **whole mass** works for a rigid ship: it stands, stops dead, never goes into the wall, and no joint feels the contact (gap 20 to 40 um standing, 0 in the
  hit). (2) Giving it each car's own mass fails, as expected: the contact removes only the contacting car's momentum while gravity (or the rest of the rod) adds the whole ship's and the joints hand it back, so the tower falls over or
  sinks (the cause is my reasoning; the outcomes are measured) and the rod goes 80 to 520 mm into the wall. Four times the steps do not repair it (60 mm for 4 cars, the tower still falls). (3) The ordinary combined step is itself inexact for impacts through a chain at the game's settings: a rebound of 24 to 40 % of the
  impact speed, joint gaps up to 13 mm for 8 cars, standing gaps of 25 um to 2.6 mm (growing with the weight above, down 13 times with 4 times the steps).
- **The rail.** In the whole-ship scheme the change of the ship's momentum in the contact pass is exactly the outside impulse (mass x the velocity change of the one body), friction included, with nothing read from
  Rapier's contact pairs; the joint pass changes the total momentum by at most 7e-12 kg m/s per step (rounding; whole-ship runs; the per-car run of 8 cars reached 1.2e-10; impulse joints, zero gravity). So `rail momentum += contact pass change` and a hold in the joint pass
  (needed for multibody joints, not for impulse joints) is a complete bookkeeping. Read in `writeback_impulses` (`contact_with_twist_friction.rs`): the normal impulse a contact keeps is the step's total (summed over the
  substeps), the friction impulse is kept only as the last substep's warm-start vector; so summing the contact pairs after a combined step gives the normal part only.
- **Cost (a probe, not tuned; microseconds per 50 ms tick, tower):** combined 21 / 28 / 47 at 2 / 4 / 8 cars, two worlds 27 to 29 / 39 to 40 / 65 to 69 (the second world, the copies and, for the whole ship, moving every collider each step).
  A ship far from everything needs the joint world only. Not measured: many colliders per ship (moving them all every step), where the contact world's broad phase cost goes.
- **Not tested, and where it would break.** The whole-ship contact is exact only if the ship moves as one body while it is touched: a hinged limb that touches the ground has an effective mass of its own, and
  the whole-ship answer is wrong for it (too stiff by the ratio). The fix would be one rigid body per *weld group* in the contact world, and for articulated parts either the combined step or iterating the two passes
  (untested: it is the block Gauss-Seidel that the combined solver does inside). Also not tested: a ship touching two things, friction on the ship (only the head-on and the resting cases ran), torque from a
  contact off the ship's centre (the code does it; the scenes never use it), multibody joints in the joint pass, the contact world's inertia when the ship's shape changes (set once here).

**(c) What limits an impulse joint** (`accuracy.rs`, all-impulse ring of 4 + chain of 12, pushed for 1 s then free, 40 s; worst anchor gap, momentum change of the centre, kinetic energy):
| case | worst gap m | dv m/s | energy change | us/tick |
|---|---|---|---|---|
| 1 substep | 1.5e-2 | 7e-15 | -0.38 % | 19 |
| 2 | 3.8e-3 | 1.4e-14 | -0.19 % | 29 |
| **4 (the game)**, 1 pass | **8.5e-4** | 1.5e-14 | -0.10 % | 48 |
| 8 | 1.8e-4 | 1.4e-14 | -0.053 % | 87 |
| 16 | 4.5e-5 | 4.6e-14 | -0.028 % | 171 |
| 32 | 9.8e-6 | 3.0e-14 | -0.015 % | 323 |
| 4 substeps, 2 / 4 / 8 / 16 passes | 4.7e-4 / 1.7e-4 / 7.4e-5 / 3.5e-5 | 2e-14 to 5e-14 | -0.10 % (unchanged) | 53 / 63 / 84 / 125 |
| 16 substeps, 8 passes | 4.6e-6 | 5e-14 | -0.028 % | 306 |
| 12 steps a tick, 4 substeps | 4.5e-5 | 4.5e-14 | -0.026 % | 193 |
The gap falls about 4 times for each doubling of the substeps (second order) and slowly with passes; the momentum is conserved to rounding at every setting; the energy loss is small (-0.1 %) beside the multibody's (-6.7 %).
Then the numbers themselves, game settings; "state in 32 bits" rounds every pose and velocity to the nearest 32-bit float after each step (the arithmetic stays 64-bit); "4 km out" moves the rig by (4000, -3000, 3500) m:
| case | worst gap m | dv m/s | energy change |
|---|---|---|---|
| origin, 64-bit | 8.49e-4 | 1.5e-14 | -1.01e-3 |
| origin, state in 32 bits | **8.50e-4** | **2.7e-6** | -1.01e-3 |
| 4 km out, 64-bit | 8.49e-4 | 1.5e-14 | -1.01e-3 |
| 4 km out, state in 32 bits | 9.69e-4 | 2.7e-6 | -1.03e-3 |
| *16 substeps, 8 passes (solver error small):* origin, 64-bit | 4.6e-6 | 5.2e-14 | -2.83e-4 |
| origin, state in 32 bits | 6.7e-6 | 2.9e-6 | -2.83e-4 |
| 4 km out, 64-bit | 4.6e-6 | 5.4e-14 | -2.83e-4 |
| 4 km out, state in 32 bits | **3.6e-4** | 1.4e-6 | -3.51e-4 |
So at the game's settings the joint error is the solver's, 10^12 times above 64-bit's rounding (about 1e-15 m at 5 m, 5e-13 m at 4 km): taking the numbers down to 32 bits changes the gap by 0.1 % and only breaks the momentum
(1e-14 -> 3e-6 m/s). The 32 bits start to matter when the solver is made good AND the rig is far out (3.6e-4 m), which is the f32 floor of the earlier work (about 1 mm at a frame's edge). 64-bit is already
indifferent to 4 km. Section 5's direct closing solve reached 1e-14, i.e. 64-bit's own floor; a number format wider than that only matters for a solver that reaches it.

**(b) A wide fork.** Counted by `grep` over the sources of the versions the game uses (tests and comments included): `rapier3d-f64` 0.36.1 has 80 276 lines, `parry3d-f64` 0.31.1 79 506, `glamx` 3 207; `Real`
appears 3 405 times; vectors, quaternions and poses are `glam`'s f64 types (glam is not generic over its scalar), nalgebra and simba are used for the dense multibody matrices, `wide` for the solver's SIMD lanes;
calls of `sqrt` 33, `sin` 22, `cos` 16, `powi` 5, `acos` 4, `powf` 3, `exp` 2, `asin` 2, `atan2` 1. A fork therefore means replacing `Real` and the vector, quaternion, pose, SIMD and matrix types in about 160 000
lines and giving the transcendentals a deterministic wide implementation (the repo's `libm` rule), then merging every Rapier and Parry release into it for good. Speed: a throwaway loop of dependent multiply-adds
(the shape of a solver's dot products) took **0.57 ns in f64 and 7.1 ns in double-double** (two f64, about 106 bits, Dekker's products so that it needs no fma and no libm; **12.4 times**), and Rapier's f64 path also
runs on SIMD lanes the wide type would lose, so the real slowdown is larger (not measured). The `f128` primitive is **unstable** on this toolchain (rustc 1.99.0, error E0658; no nightly installed), so it was not timed.
The repo's own `wide_float` (malachite) is correct and canonical, but one whole Kepler evaluation at 128 bits takes 30 to 44 us (E7); its cost per operation was not measured here, and it is built for exactness, not for a solver's inner loop.
- **My opinion (medium-high confidence; the measurements above are real, the verdict is a judgement).** Do not fork Rapier for width: what the owner wants from "exact joints" is limited by the solver, not the number
  (section 14c), and the places that need more than 64 bits (orbits, saved paths) already have it in the rails layer, with the physics world in a local frame where 64-bit is exact to 5e-13 m at 4 km. Spend on
  the solver instead: substeps (4 times better per doubling, linear cost), passes for chains and impacts, a direct solve where a loop must close, the contact-pass-with-the-whole-weld-group of (a), and the
  hold of section 12.

## 15. One body moves, the other does not (`dominance`, fourth round, 2026-11-07)

The owner asked whether a contact can change the momentum of one body only (a player inside a ship must not move it; a player outside, pushing with the suit's thrusters, must). `cargo run --release --bin dominance`: a 100 t box at 2 m/s, an 80 kg box on its +x side at -1 m/s, zero gravity, 2 s.

| case | ship's change of speed (m/s) | player's speed (m/s) | overlap |
|---|---|---|---|
| equal dominance (the EVA bonk) | -2.409e-3 | 2.0116 | 0 mm |
| ship dominates | 0 exactly | 0.6444 | 201 mm |
| ship dominates, hit 1.5 m off centre | 0 exactly | 0.6462 | 206 mm |
| player dominates (control) | -2.056 | -1.0000 | 112 mm |
| ship **kinematic** (velocity-based) | 0 exactly | 2.0141 | 0 mm |

*Verified by running:* a dominant body's velocity does not change at all, but in Rapier 0.36.1 the contact is solved against a dominant body as if it were at rest (read in `contact_with_twist_friction.rs`: the dominant side is given the world-attached id, whose velocity is zero; the table agrees), so dominance alone is the wrong tool for a moving ship. A velocity-based kinematic body is a moving wall that nothing moves, and that is what the player inside needs. *Not verified:* the two cases with a spinning ship (0.5 rad/s) were badly built (the hull turns a whole radian away from the player in the 2 s), so nothing is claimed about them; toggling a body between dynamic and kinematic; a ship with several grids; the cost. *Believed:* the same rule gives the right answer when the ship is the whole-ship body of section 14, because the contact pass sees the ship as one body and the switch is only whether that body is given the contact's impulse back.


## 16. Handing a hit back to a mechanism: what players build (`contraptions`, fifth round, 2026-11-13)

The owner's proposal: run a collision with each ship as ONE rigid body, take the impulse from the way the momenta change, throw the bodies' new places and velocities away, and give the impulse to the cars that were hit, in their own frame, where the joints spread it (section 14 (a), with the impulse put at the hit cars instead of one rigid motion). The first probe of it (`handback.rs`, spheres in a chain) found it right for stiff chains and half right for hinged ones, and left open: resting contact under gravity, friction, wheels, long chains, two strangers, cost, bit-identity, and what players actually build. This round tries it on landing gear, doors, a rover and a ship carrying a ship, on springs and motors. `cd spikes/loops && cargo run --release --bin contraptions -- NAME` (`regress rest gear door rover carrier friction long squeeze approach nearness variants rounds cost fingerprints`); the kit is `src/contraption.rs`; every table is in `spikes/loops/results/contraptions-2026-11-13.txt`, the Windows and Linux fingerprints in `results/fingerprints-*.txt`.

**What ran.** A scene is cars (one body and one collider each, in *groups*: a group is a ship, with a joint world of its own), joints (fixed, ball, hinge, slider; limits; motors in Rapier's force model: a spring and damper for a strut, a hinge held by a motor of finite torque for a door, a velocity drive for a wheel), a floor, gravity and pushes (an engine). The game's settings (1/20 s tick, 3 steps, 4 substeps, 1 pass); the reference is the converged one world (12 steps, 16 substeps, 32 passes). Schemes:
- *one world, game* (what the engine does today) and *converged*;
- *whole*, *cluster*, *per car*: a contact world with the floor and one proxy body for each group, for each set of cars welded by fixed joints, or for each car, re-posed from the group's joint world every step, with gravity; handed back as *rigid* (the proxy's change of velocity as one rigid motion: section 14) or as *impulse* (the contact pairs' impulses, at their points, to the cars they touch: the owner's idea). *+N rounds*: block Gauss-Seidel between the two kinds of world, each round a predicted step of scratch copies of the joint worlds;
- *ghost, impulses*: the contact world is a full copy of the one world (cars, joints, motors, colliders), synced from the joint worlds every step; only its contact pairs' impulses are taken, and handed back as the impulse scheme does;
- *ghost, difference*: two such copies, one with the contacts and one without; what the contacts did (the difference of the two, in place and velocity of every car) is added to the group's own step.
Gravity is in the joint worlds as a force on every car (a free-fall frame and a uniform pull are the same to the joints) and in the contact world so that the contact sees the approach it causes. *The kit reproduces the old probe*: converged, game and whole-rigid to the digit; whole-impulse moved (stranger +0.094 against +0.073) because the old probe used a diagonal inertia and this uses the full tensor.

**Two mistakes of mine that the kit caught** (the old probe is unaffected: its spheres have one contact point each).
1. *A hit given as a change of velocity at the start of a step, with gravity acting through the step, makes support hop*: a tower of cubes at rest jittered at 200 to 700 mm/s, because the support arrives first and the weight after. Given as a *force over the step* (the impulse over dt, acting with gravity and the struts' springs) it stands (0.2 mm/s). Every number below is the force form.
2. *Rapier keeps the impulse on manifold points the solver did not use in a step.* Summing every point's impulse over a box's contacts was 30 to 180 percent off the body's change of momentum. Only the points named by a `solver_contacts` entry count; then the sum is the change of momentum to 1e-14, with friction too (the friction impulse is `tangent_impulse` of the point in the basis `orthonormal_vector` / its cross product with the force direction `-normal`, and it is written only when the friction model is `Coulomb`; the default `Simplified` keeps the last substep's warm start). The sum is kept in a canonical order (sorted by car and pair) so that it does not depend on the order Rapier keeps pairs in.

**Results** (ran; "err" is against the converged one world; the whole tables are in the results file).

| landing gear, 8.4 t on four struts (78 kN/m, 17.5 kN s/m), lands at 2 m/s, 8 s | peak g | least strut extension (0.36 true; 0.05 is bottomed) | settled height error | jitter at rest |
|---|---|---|---|---|
| converged | 2.49 | 0.362 | 0 | 0 |
| one world, game | 2.45 | 0.360 | 0.3 mm | 65 mm/s |
| ghost, impulses | 2.52 | 0.361 | 0.1 mm | 8.5 mm/s |
| ghost, difference | 2.45 | 0.360 | 0.3 mm | 65 mm/s |
| whole, rigid | 18.4 | 0.700 (never compresses) | 239 mm | 1.5 mm/s |
| **whole, impulse (the owner's)** | **13.2** | 0.161 (bottomed at 4 m/s: 0.057) | 6.5 mm | **3,986 mm/s** |
| per car | 20.2 | 0.525 | -702 mm (on its hull) | 586 mm/s |
| per car, +2 rounds | 1.84 | 0.351 | -143 mm | 80 mm/s |
(8 rounds: 2.70 g, 4.9 mm, 16 mm/s, at 7.6 times the cost of none; 4 m/s lands the same way: truth 3.99 g, ghost 3.94, whole impulse 21.0 and bottomed, whole rigid 29.2, per car 28.6.)

| a 150 kg door on a hinge held by a spring motor (30 kN m/rad, 15 kN m at most), hit at its free end at 4 m/s | stranger exit speed | hull speed | door's peak angle |
|---|---|---|---|
| converged (1000 kg stranger) | -1.507 | 0.403 | 0.288 rad |
| one world, game / ghost, impulses | -1.494 / -1.510 | 0.405 / 0.402 | 0.305 / 0.307 |
| whole, rigid | -2.046 | 0.309 | 0 |
| **whole, impulse** | -2.049 | 0.308 | **1.710 (flung to its stop)** |
| cluster = per car | -2.781 | 0.188 | 0.192 |
| per car, +2 rounds | -1.617 | 0.382 | 0.283 |
A 100 kg stranger: converged +0.042, whole -0.379 (it bounces off a "ship"), per car -0.094, per car +2 rounds +0.031, ghost +0.116. A *locked* door (motor 5e6): converged -1.985, whole -2.046 and -2.045 (within 3 percent: **right**), game -1.657 and ghost -1.631 (16 percent off), per car -2.806.

| a rover (1.5 t, four 40 kg wheels on hinges, the rear pair driven by motors; friction 1), speed at 8 s | driving (true 4.994) | coasting from 5 m/s (true 5.000) |
|---|---|---|
| one world, game / ghost impulses | 4.969 / 4.957 | 4.999 / 5.013 |
| whole, rigid / impulse | 0.028 / 0.055 | 0.131 / -0.040 |
| cluster = per car (+2 rounds) | 0.011 (0.000) | 0.005 (-0.002) |

A 20 t carrier on four struts with a 3 t ship on four struts on its deck (two groups): the hull heights after 10 s, error of carrier / carried against converged: game 0.0 / 2.6 mm, ghost -0.2 / -0.2 mm (jitter 11 mm/s against game's 70), whole rigid 238 / 422 mm, **whole impulse 11.5 / 26.4 mm with the feet chattering at 4.7 m/s**, per car -674 / -1,514 mm. The carried ship dropped on the deck at 1.5 m/s: peak g of its hull converged 2.56, game 2.49, ghost 2.55, whole rigid 16.1, whole impulse 10.1, per car 24.1, per car +2 rounds 1.64. With the carrier accelerating at 1.4 m/s/s the carried ship slides on its legs' friction by (converged 0.7) game 1.0, ghost 3.7, whole impulse 39, per car 364 mm.

A hinged chain of 8 or 16 cars, 1000 kg at 5 m/s: *across*, the stranger's exit speed converged -2.170 / -2.174, game -2.174, ghost -2.160, **whole rigid -1.444 / -0.923, whole impulse -1.448 / -0.924** (the far car 428 to 1,214 mm off), per car -2.176 (19 mm off). *Along*, converged -0.553 / -0.232, game **-0.240 / -0.023**, ghost -0.211 / +0.034, whole -0.556 / -0.294, per car -0.556 / -0.193: along a stiff column the whole body is the *most* right, and today's combined step the least. A block sliding on a floor with friction: every scheme stops it within 0.01 m of the right place (friction is carried right). Two strangers squeezing a chain from both ends: no scheme tells (the strangers stop dead in every one but the combined step, which rebounds at 0.84 m/s); not a useful test.

**Why each one fails** (my reading of the numbers; the numbers are measured, the reasons are mine, and a few are checked by the cases above).
1. *The whole body is rigid to the contact*, so (a) a stranger meets the mass of the whole ship at any point, whatever the joints allow (the door: 6.15 t where 50 kg is at the free end; the gear: the whole ship stops in one step, and the 100 kg foot is given its share of the impulse of 8.4 t: the feet were thrown up at 10 to 29 m/s from a 2 m/s landing, traced with `contraptions trace whole`); (b) the *motion of a part against the ship is invisible*, because the contact solver gets a collider's velocity from its body's rigid motion, so a wheel's rolling is a skid (the rover cannot move, and a coasting one brakes), a closing door has no speed at what it closes on; (c) the support among several feet is shared by a rigid solve, so which three of four feet carry the weight flips with millimetres of height and the feet chatter at 1 to 3 m/s while the hull stands still.
2. *A car alone cannot see a load that comes through a joint*: the foot is held up for its own weight only, so the hull sags onto the floor (gear -0.70 m, the rover's chassis drops onto its box, the tower of cubes slides off its axis by 3.5 m). Rounds (predicted joint steps) bring the load back: 8 are needed, at 7.6 times the cost; and wheels did not come back with 2.
3. *A rigid cluster* (weld groups) is per car for everything that is not fixed-jointed: a hinge carries force along the chain, so a hit along it sinks into the cars (81 to 108 mm of overlap against the whole's 31 to 36).
4. *An effective mass per contact* (probe the joint world with a test impulse, give the proxy that mass; read, not built) cannot be right at the instant of a hit (the foot) and for a steady load (the foot and its share of the hull): the mass of a spring-damper depends on how long the push lasts. It is the combined step done by hand.
5. The impulse form keeps flexing (the first probe, `handback.rs`: the hinged chain's internal energy 0.23 against 0.22 kJ, the rigid form's 0), which is why it beats the rigid form at doors and chains; it does not repair 1 to 3.

**Stability and the promises.** *Chaos:* with a 1 t cube standing on the floor 1.5 m from the ship, touching nothing, the whole-impulse ship ends 8 cm and 6.5 m/s from where it ends without the cube (not one bit the same); one world, both ghosts (also with the ghosts' own step doubled), whole rigid and per car are bit for bit the same with and without it. *Bit-identity across machines:* the same scenes (6 scenes x 10 schemes) run on Windows and in Linux (WSL, same lock file): **41 of 60 fingerprints are identical (every one-world and every ghost run), the 19 that differ are all proxy runs** (the proxy's inertia tensor is built with `MassProperties::with_inertia_matrix`, an eigen-decomposition, which I did not trace to a function; the repo's rule forbids std transcendentals, so a proxy would need an eigen solver of its own, or a body frame chosen so that none is needed).
*Coupling interval:* one step of 1/20 s a tick (12 substeps) instead of three of 1/60 s: ghost impulses 2.78 g and the feet 51 mm into the floor (9.7 mm at 1/60), whole rigid 7.4 g, whole impulse still 2.7 m/s of chatter, per car 10.7 g.

**The ghost, and what is wrong with it.** It is the one world run for the contact only, with the cars' state taken from the groups' own joint worlds, so its answer for a contraption is today's (the table rows equal the game's), and the joint worlds keep the mechanism: with the stranger *near* and not touching, the difference is exactly zero, so the mechanism is bit for bit the same as alone, whatever the ghost's own steps. Faults found: (1) *a hit given as an impulse, however fine the ghost's steps, loses when in the step it happened*: a stranger at 5, 20 and 60 m/s onto a stiff chain overlaps it by 23, 93 and 281 mm (the one world: 0, 0, 9 mm), growing with speed times the step (whole impulse 31 / 125 / 376, per car 81 / 325 / 975); finer contact steps did not change it. The *difference* form has no such fault (it is the ghost's own state, so it overlaps as the one world does), nor the impulse form's quiet at rest in exchange (the impulse form: jitter 8.5 mm/s at rest, the difference form 65 like the one world). (2) It inherits what the combined step gets wrong: the stranger that hits a stiff chain rebounds (exit -0.571 where the converged is -1.000 at 5 m/s, 106 mm of the chain's end; 423 mm at 20 m/s, 1.2 m at 60), and that column of the tables is where the whole body is right. (3) Cost: toy, 128 hinged cars hit across, microseconds a tick: one world 590, ghost impulses 1,422, ghost difference 1,708, whole 671 to 701, cluster 1,084, per car 865, per car +2 rounds 2,575. The toy's colliders are spheres, so the contact pass is cheap and the second joint world looks large; in the game the contact pass over voxel colliders is the larger share (section 14: two worlds +30 to 45 percent for a tower, read, not measured for the ghost).

**The old probe's open item, closed.** Extra rounds on the hinged chain now converge steadily (across: exit vx 0.154, 0.197, 0.213, 0.221, 0.222, 0.222 for 0, 1, 2, 4, 8, 32 rounds against the converged 0.195; along: -1.000 to -0.995): what got worse in `handback.rs` was stepping the one joint world again and again in place; here each round is a predicted step of a scratch copy. A fixed point of the rounds is not the converged answer (0.222 against 0.195, the far car 18 mm).

**My opinion (high confidence for the first two, medium for the third).**
1. *The whole-ship proxy, either form, is not suitable for what players build*, because their parts move: gear, doors, wheels and carried ships fail in six tests and in ways that follow from what a rigid contact body is (above), not from tuning. It is right for a group that does not move its joints (a locked door, a column), where it beats today's combined step; the group of such a ship is one body anyway, so that is where it is not needed.
2. *Per-car proxies, with or without a few rounds, are not suitable either* (load through joints; wheels).
3. *What keeps the mechanism exact and the contact right is the one world run for contact only* (the ghost), at two to three joint solves for a group with a stranger in range. It is the owner's answer 6 of 2026-11-09 ("their collision is worked out in a temporary shared frame in which the rails are the authority, and only a real hit changes either") made for joints, and it answers the open part of answer 5 (the size of the impulse at the point of contact): the articulated one, which is what the ghost's pairs give. It lets the window be generous, because a ghost that touches nothing changes nothing, where design A needs a tight one. Not yet known: the ghost with voxel colliders, with rails and frames, with sleeping, the real cost.

**Not tested.** Voxel colliders and real ship shapes; sleeping bodies; frames, rails and the rail's kick in the engine (the ghost's pairs give the exact impulse and moment; the engine side is read, not run); several strangers at once on a groups' legs (a landing on rough ground); joints that break; a door driven closed onto something; a ghost whose numbers really differ from the group's (a stand-in: its own step count, ghosts x2 and x4: the door and the gear move by under 3 percent, the stiff-chain hit by 4 to 11 percent); tick rates other than 20 Hz; one parameter set for each contraption (masses, stiffness and damping were chosen to be plausible, not scanned: the failures follow from structure, so I expect them to hold across them, which is not shown).


## 17. The ghost with every car movable, part priorities, and fixes of the failed methods (`contraptions`, sixth round, 2026-11-13)

The owner's questions after section 16: what if the shared world holds every car as a body that can move (not one rigid body for a ship) and gives its impulses to the separate worlds; can *part priorities* improve those calculations; and can any of the failed methods be fixed? "Part priorities" is not a term of these documents; I tried the four things it can mean in this engine (below). If another meaning was intended, it is untried. `cargo run --release --bin contraptions -- fixes|priorities|effort|welds|priority|lod|rovertrace|cost|fingerprints`; tables in `spikes/loops/results/contraptions-2026-11-13-second-round.txt`.

**(1) That is the ghost of section 16.** Its shared world already holds every car of every ship as its own body *with its joints and motors*; only its contact impulses (or the difference of two such worlds) go back to the groups' own worlds. What failed in section 16 was a shared world with *one rigid body per ship* (whole body) or with *cars that are free of their joints* (per car). This round's scoreboard puts them side by side on ten scenes (the converged answer first; "mm" columns are the hull height error; the heights themselves are 1.146 m for the gear, 1.159 and 2.569 for the carrier):

| scheme | tower off axis mm | gear peak g (2.49) | door angle (0.288), exit (-1.507) | rover driving (4.99), coasting (5.00) | carrier, carried mm | carried ship lands g (2.56) | chain of 16 across, exit (-2.174) | chain of 8 along, exit (-0.553) | stiff chain at 20 m/s, exit (-4.000), overlap mm |
|---|---|---|---|---|---|---|---|---|---|
| one world, game (today) | 0 | 2.45 | 0.305, -1.494 | 4.97, 5.00 | 0, 3 | 2.49 | -2.174 | -0.240 | -2.283, 0 |
| **ghost, impulses** | 1 | 2.52 | 0.307, -1.510 | 4.96, 5.01 | 0, 0 | 2.55 | -2.165 | -0.211 | -2.283, 93 |
| ghost, difference | 0 | 2.45 | 0.305, -1.494 | 4.97, 5.00 | 0, 3 | 2.49 | -2.174 | -0.240 | -2.283, 0 |
| **welded ghost, impulses** | 1 | 2.52 | 0.307, -1.510 | 4.96, 5.01 | 0, 0 | 2.55 | -2.165 | -0.211 | **-4.000**, 125 |
| **welded ghosts, difference** | 0 | 2.45 | 0.305, -1.494 | 4.97, 5.00 | 0, 3 | 2.49 | -2.174 | -0.240 | **-4.000, 0** |
| whole body, impulse | 2 | 13.24 | 1.710, -2.049 | 0.06, -0.04 | 12, 26 | 10.07 | -0.924 | -0.556 | -4.000, 125 |
| per car | 3,548 | 20.20 | 0.192, -2.781 | 0.01, 0.01 | -674, -1,514 | 24.10 | -2.180 | -0.556 | -4.000, 325 |
| per car + load feedback | 122 | 2.57 | 0.304, -1.490 | 4.84, 5.55 | 0, 1 | 2.32 | -1.995 | **+0.469** | -0.623, 227 |
| cluster + load (persistent) | 1 | 2.59 | 0.301, -1.510 | 4.41, 7.50 | 0, 1 | 2.43 | -2.172 | +0.045 | -4.000, 125 |
| cluster + load + 2 rounds | 1 | 2.68 | 0.301, -1.524 | 4.93, 5.18 | 10, 21 | 2.64 | -2.181 | -0.197 | -4.000, 125 |

**(2) Part priorities, in four forms** (ran).
- *Solver work.* Rapier gives extra substeps and passes to a whole island, the largest request of its bodies (`additional_solver_iterations`, `additional_pgs_iterations`; read in `rigid_body.rs`), so a priority per part is a priority per ship and what touches it. The rebound that the combined step gives a stranger on a stiff chain (exit -0.571 at 5 m/s where -1.000 is true) is a matter of *passes*, not substeps: extra (substeps, passes) of (0, 3): -0.730, (0, 7): -0.941, **(0, 15): -0.998**, (4, 0): -0.595, (4, 7): -0.946, (12, 31): -1.000; the same at 20 m/s: -2.283, -2.918, -3.763, **-3.992**, -2.381, -3.785, -4.000. Cost in the landing gear scene, microseconds a tick, for none and then the six: 112, 134, 161, **226**, 155, 267, 1,274. The hinged chain of 8 hit along is slower to cure (true -0.553: -0.240 none, -0.465 at (0, 15), -0.550 at (12, 31)). **It must be set before the touch, not by it:** the same work given only after a step in which something was touched did nothing for the stiff chain (-0.571, -2.283) and made the hinged chain bounce (+0.864 at (4, 7), +3.663 at (12, 31)). So the priority is by *nearness* (a stranger in range), the same room that makes a ghost exist.
- *Welding the rigid parts.* Cars joined by fixed joints (and a joint the scene marks as immobilized) are one body in the ghost, the mobile joints between such bodies are kept (anchors moved into the bodies' frames). The stranger on a stiff chain: exit -1.000 / -4.000 / -12.000 at 5 / 20 / 60 m/s, exactly the converged answer (the combined step: -0.571 / -2.283 / -7.047), at *less* than the cost of the plain ghost (39 against 50 microseconds a tick; the combined step 23). A door held by a very stiff motor and marked immobilized: -2.046 (true -1.985; the combined step -1.657, 17 percent off). Nothing is lost where there is nothing to weld (the gear, the carrier and the rover rows are the ghost's). **Mislabelling costs:** the same weld on the spring door gives -2.045 for -1.507, which is the whole-body answer; a weld is right only for a joint that does not move while it is touched. This is the owner's whole-body idea kept where it is right.
- *Who is given the impulse.* A priority per car, and the car of the higher priority is left out of the impulses of a contact with one of a lower: a 100 kg crate at 5 m/s into the wall of an 8 t ship that has the higher priority: the ship's speed after 1 s is exactly 0 (the combined step: -0.062 m/s), the crate ends at +0.048 m/s (it stops against a ship that does not move; the residue is the contact's softness), momentum is not kept (505 N s) *by design*. That is the owner's "things inside a ship never move its rail" (answer 7) as a rule on the hand-back; it works on the impulse forms (ghost, welded ghost, per car, whole body). Rapier's own dominance does not do it (section 15: the dominated body sees the ship at rest); the rule does not make anything immovable in the solve, only in who is given the result.
- *A level of detail by distance* (the far cars of a chain welded, the ghost's difference form): the hinged chain of 16 hit across at the last car gives the same answer however many of the first cars are welded (exit -2.174, far car 3.9 mm), while the cost falls from 247 to 139 microseconds a tick with 12 of 16 welded. Hit *along* the chain the answer moves the wrong way (the combined step's -0.023 becomes -0.017, +0.098, +0.242 with 4, 8, 12 welded; true -0.232): a wave along the chain needs the far cars. So a level of detail by distance is a saving for a hit whose answer stays near where it happened, and not for a shock along a column.

**(3) Fixes of the failed methods** (ran).
- *Whole body, rigid handback:* cannot be fixed (rigid by definition: no strut, no wheel, no door).
- *Whole body, impulse (the owner's):* its failure is moving parts, so the fix is not to put them in the one body: weld the rigid parts, keep the mobile joints (the welded ghost above). Its other faults (the support among four feet is shared by a rigid solve and flips; the proxy's inertia tensor makes the run differ between Windows and Linux) go with the whole body of a mobile ship; the welded ghost has neither (0 of 6 fingerprints differ).
- *Per car (and cluster): a feedback of the load the joints carry.* Each car's net joint force of the last step (its change of velocity over the step, times its mass, less gravity and what was handed back) is put on its proxy as an outside force in the contact pass, so that the foot is held up for the hull's weight too. It cures the resting and soft cases: the gear 2.57 g and 0.6 mm (true 2.49 and 0), the spring door 0.304 and -1.490, the carrier 0 and 1 mm, the hinged chain of 16 across -2.172 with the persistent filter (the last two steps must agree, per component), at no extra solve. It does *not* cure: a shock along a stiff hinged column (+0.469 and +0.045 for -0.553: the estimate of the last step is poison in a hit); and **a rolling wheel** (the coasting rover gains speed, 5.55 and 7.50 for 5.00). The trace of that (`rovertrace`) is a start-up and a loop: with no estimate in the first steps the chassis falls and the wheels sink 5 cm; the late feedback then rings at about 3 Hz (wheel height 0.349 to 0.380 m; the combined step 0.395), and as the contact's lever arm changes with the sinking, the wheels' spin goes up and comes back out as thrust (energy from the loop, not from the wheels). Taking only the part along the contact normal changed nothing (same 5.55), so the friction is not the cause; relaxing (0.5, 0.25) made it worse (6.21, 8.04); the persistent filter too (7.50). *Rounds close it:* with two rounds of predicted joint steps and the clusters welded the same scenes read gear 2.68 g (8 mm), door 0.301 and -1.524, rover 4.93 and 5.18, carrier 10 and 21 mm, landing 2.64, chain across -2.181, stiff -4.000, along -0.197 (true -0.553: still off): within 10 percent but for the hit along a hinged column, at 4.3 times the combined step's cost (the ghost: 1.9 times). It is a cheaper-looking road that ends at the ghost's cost with worse answers.
- *Not built, reasoned:* an effective mass for each contact (probe the joint world with a test impulse): it would take the hit at the instant right and the steady load wrong, and the load feedback takes the load right and the instant wrong, so the two together are what a per-car scheme would need; I did not build the first. *A hook on the contact solver* (`modify_solver_contacts` can set a tangent velocity, a conveyor belt) would let a whole body see a wheel's rolling, and nothing else of what it cannot see (a door's closing, a strut's give); not built.

**Cost** (toy, microseconds a tick; hinged chains of 8 / 32 / 128 / 256 cars): one world 60 / 147 / 577 / 873; ghost, impulses 88 / 297 / 1,099 / 1,805; ghost, difference 134 / 440 / 1,582 / 2,680; welded ghosts (nothing to weld in a hinged chain) 100 to 129 / 310 to 455 / 1,135 to 1,655 / 1,922 to 2,826; cluster + load + 2 rounds 223 / 714 / 2,462 / 4,538. As in section 16, a toy: the colliders are spheres, so the second joint world looks large; with voxel contacts the combined step is mostly contact.

**My opinion** (medium confidence; the measurements are real, the choice among the forms is a judgement).
1. *The ghost with every car movable and its joints in place is the right contact pass*, and welding what is rigid in it (fixed joints, immobilized joints) is a free gain in accuracy and cost: it is the owner's whole-body idea applied where it is exact. Where a mechanism moves, the shared world must have the joints.
2. *Of the priorities, "who is given the impulse" and "weld the rigid parts" are cheap and clear wins; "solver work" is a real cure for the rebound but only if set before the touch (by nearness), at about twice the island's cost for 15 passes; "level of detail by distance" is a saving for local hits only.*
3. *The failed per-car methods can be fixed for resting and soft contacts, not for shocks through stiff chains or rolling wheels, and the cure that does reach them (rounds) costs more than the ghost.* I would not build them.
4. The difference form of the ghost is the one with no timing fault; the impulse form is quieter at rest and gives the exact impulse for the rail's kick; the rule of who is given the impulse needs the impulse form.

**Not tested.** The same list as section 16 (voxel colliders, sleeping, rails and frames in the engine, rough ground, breaking joints, a stranger on a loaded carrier, clamps, other tick rates, a scan of parameters); nearness as a trigger of the extra solver work (here it is on from the start or by a touch); a weld that follows a joint being immobilized and mobilized again in play (the weld sets are built once from rest poses here, and need the cars to start unrotated); the cost of building the welded body's inertia in the game (done once here; an eigen-decomposition, so it must not differ between machines: the welded ghosts' fingerprints agree on Windows and Linux).
