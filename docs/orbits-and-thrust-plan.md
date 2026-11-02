# Elliptical orbits, ships on exact rails, and long burns: a plan and an investigation (2026-11-01)

**A proposal, not a decision.** Written at the owner's request after the joint and f64 work (`decisions/joints-and-cars.md`, `history/joint-probe.md`). The owner asked for three things: (A) a plan for elliptical orbits, (B) whether ships can be kept on
128-bit fixed-point rails at all times, with Rapier used only for what only Rapier gives, and (C) how constant or varying thrust for a long time (ion drives, solar sails) could be integrated ahead of time, so that a preview is the
trajectory time warp would give. What is checked in the code is marked **[code]**; what comes from the web or memory is marked **[source]** or **[memory]**; what is my reasoning is marked **[mine]**.

## 0. Where things stand

- **[code]** `MovingGrid::rail: Option<Rail>` exists for any grid and is saved in ship files; `put_ships_on_their_rails` sets the pose of every grid with a rail at the start of a tick (`world.rs`). Only planets get rails today (data packs); `Orbit` is a
  *circle* (`around`, `normal`, `radius`, `period`, `phase`), worked out in 128-bit fixed point (`exact_circle.rs`: CORDIC sine and cosine in 126 bits, `ExactOffset` = `i128` bricks plus a rest in steps of 2^-32 voxel). The velocity is plain `f64`.
- **[code]** The offset is exact, but it is then added to the parent's `WorldPosition` (an `f32` offset in a brick), so the stored pose of a moon is rounded to about 2 micrometres at every tick. It is a function of the tick, so it does not accumulate.
- **[code]** A ship that is far from every player is frozen: its speed is zeroed (`set_grid_frozen`). Parked ships are in files and are not carried by a planet that moves. There is no conversion of a free ship to a rail or back. A ship's gravity is a sum of the pulls of every
  massive grid, in `f32` (`gravity_at`); inside a planet's sphere it leaves out the pull of what the planet goes round (`gravity_in_the_free_fall_of`).
- **[code]** Frames: a `BodyWorld` never moves; a frame that "follows" a planet subtracts the planet's displacement since the frame was made (`Follow`), and the planet's speed from velocities, so what rests on the planet rests in the frame.

## A. Elliptical orbits

**Shape of the change [mine].** `Orbit` gains `eccentricity` (0 up to but not including 1), `periapsis` (the angle of the nearest point in the orbit's plane, from the same reference direction the circle starts at), and `epoch` (a `RailTime`).
`radius` becomes the semi-major axis (for a circle it is the radius), `period` stays, `phase` becomes the **mean anomaly at the epoch**. With `eccentricity` 0 and epoch 0 the result must be **the same bits as the circle today**: that is the first test.
An epoch is needed because a ship that is put on a rail is put there at some tick, not at tick 0: a phase "at time 0" would be a huge angle (n x t) rounded to `f64`, where an epoch keeps the angle in [0, 2 pi).

**Mathematics, all in whole numbers [mine, standard].**
1. Mean anomaly M = phase + (time - epoch) / period, as a fraction of a turn in 128 bits (the existing `turn_fraction`, exact however long the world has run).
2. Kepler's equation E - e sin E = M, solved by Newton's method in 126-bit fixed point with a fixed number of steps and a bisection bracket as a guard (a count, never a time: `design-decisions.md`), starting from a good first estimate for the eccentricity
   **[source]** (Danby, Markley and Mikkola starting values are the usual ones; the search results I read agree that Newton, Danby and Halley all work for e below 1). Cost: a handful of CORDIC sine and cosine (about 130 shift-and-add steps each): microseconds.
3. Position in the plane: x = a (cos E - e), y = a sqrt(1 - e^2) sin E, with the fixed-point square root that the plane code already has; turned by the periapsis angle and into the plane's basis (`exact_circle::plane`), multiplied in 256 bits as the circle is.
4. Speed: v = (a n / r) (-sin E, sqrt(1 - e^2) cos E), n = 2 pi / period, r = a (1 - e cos E), as `f64` from the same exact sine and cosine (as the circle does now).
5. Hyperbolas (e above 1, for flybys and escapes) use sinh and cosh and a different Kepler equation, M = e sinh H - H, with no period; **a second step**, after the ellipse is pinned. Parabolas are not made.

**Conversions [mine].** State to elements (position and velocity against the parent, and the parent's mass parameter mu = G M, which the game has: planets declare a mass, `planet-spec.md`): the standard vectors h = r x v, the eccentricity vector, a from the energy. The degenerate cases (a circle has no periapsis; a flat orbit has no
plane) get a fixed rule (reference direction; the level's +y). Elements to state is the code above. A round trip must return the state to within what the elements' `f64` hold (about 1e-16 of the orbit's size).

**Steps, tests first.**
- A0. Pin the circle as it is (bits of `Orbit::at` for a spread of parameters; the tests in `tests/rail.rs` already do much of it).
- A1. A reference program (like `tools/exact_circle_reference.py`, `mpmath` at 120 digits) for ellipses: eccentricities 0, 0.01, 0.1, 0.5, 0.9, 0.99, 0.999, mean anomalies from 0 round to a turn, including near periapsis for the high ones.
- A2. `exact_kepler.rs`: the solver; tested against A1, with a worst-case count of steps; the same bits on Windows and Linux (it is all integers).
- A3. `Orbit::{eccentricity, periapsis, epoch}`; ship file `engine:rail` version 2 (version 1 reads as e = 0, epoch 0); the data pack syntax (`pack-format.md`, whose examples are checked by a test); checks on the numbers (e < 1, a above 0).
- A4. State to elements and back; tests: a round trip, a circle gives e = 0, the speed is the derivative of the position (finite differences of the exact positions), energy and angular momentum are constant along a rail to within the speed's `f64`.
- A5. The spec (`rails-and-recipes-spec.md`) and `design-decisions.md`.
- Then (separate): hyperbolas; the patched-conics list of segments (the sphere of influence crossings found on the exact rail, at ticks); the conversions from a free ship to a rail and back in `grid_lifecycle`.

## B. Ships on exact rails, Rapier only where only Rapier helps

**The idea, in one sentence [mine].** The rail is the ship's **centre of mass**; Rapier works on the ship's **parts relative to the rail** in a frame that follows it, and on interactions that need contacts; the big, exact numbers never go through `f64`.

**What it needs, and what exists.**
1. *The rail as the authority.* **[code]** Planets already work so (a pose written from the rail each tick). For a ship the same: its pose is the rail's state at the tick, rounded for the world's record. The exact state (`ExactOffset`) must be kept next to the pose, not rebuilt from the rounded pose,
   or the exactness is lost at once (today the rounding happens at `translated_by_bricks`).
2. *Free flight without Rapier.* A ship that only coasts, or burns on a program, is a **kinematic body that is put where its rail says** (the planets' case, `BodyKind::Kinematic`); Rapier does not integrate its gravity at all. This removes the integrator error of `f32` forces
   (`future-goals.md`, "forces over solver steps") from free flight, observed or not, and makes observed and unobserved flight the same trajectory. **Believed [mine]**: the biggest single gain for the owner's goal. Needs: gravity of the model of section A's questions (one parent at a time) for rail and body alike.
3. *A frame that follows the ship's own rail*, for the motion of cars against each other (joints, a docking, a hinge). **[code]** `Follow` does this for planets: places and velocities of the frame are the world's less the carrier's. It would be generalised to follow any rail, but **with no area of effect**: the frame holds only
   the cars of that one ship (the owner does not want stations that carry other ships; see `future-goals.md`, "Tractor beam"). The cars' state in it is small (their offsets from the rail, in `f64`), so a round trip through `f64` loses about 1e-16 of those small numbers, not of a distance from the origin. **[mine]**: this is the "Encke" idea, the part that the rail
   gets exactly and Rapier only the small difference.
4. *The two-level rule.* The sum of the outside forces on all the cars, over the whole mass, is the rail's acceleration; the cars feel in their frame what they feel less that acceleration (so a car pushed by an engine and one that is not are pushed against each other, and the whole is not moved twice). **[mine]** Standard: the centre of mass moves as if
   all the outside forces acted on a point mass; the rest is relative motion. A gravity that differs across the ship (tides) is left out. **Felt acceleration** (`future-goals.md`) comes out of it for free: a player aboard feels the frame's acceleration.
5. *Contact with something outside* (a planet's ground, another ship): needs both in one frame. A ship that comes near another or the ground leaves its own frame for the shared one (the planet's, as ships near a planet are today); while there it is a dynamic body; an impulse changes its rail
   (its elements are made again from the state it leaves with). A ship that is in no contact is kinematic on its rail. The conversion cost is the `f64` rounding of small relative numbers, about 1e-16 (section "What reading a ship back ... costs" of `joint-probe.md` measures the other, larger, `f32` costs this avoids).
6. *Freeze and park become "go on rails".* The loss of speed (`set_grid_frozen`) is replaced by making the elements from the state; loading is the elements at the clock. A ship that coasted unobserved keeps **the same elements** (not made again from a state), so a teleport to it and away again cannot change its orbit. A ship that was pushed or thrusted while observed has new elements.
   Parked ships are then carried exactly by the clock (today a parked ship is left behind by a planet that moves on).
7. *Precision decision [mine].* A relative state in `f64` integrates a burn well enough (a ship 1e9 m from its parent is placed to 1e-7 m per step; a million steps of rounding are 1e-4 m at the worst). What needs 128 bits is the **absolute** place and the angles of the rails of the parent chain (an angle over 10^12 seconds), which are already so.
   So I would keep the burn integrator in `f64` (with `libm`, a fixed sequence of steps) and the rails in fixed point, **not** a 128-bit integrator. This is an estimate; the experiment is E1 below. The decision about the `f32` offset of `WorldPosition` waits for it, as the owner said.

**Experiments, after A** (each a throwaway in `docs/history/spikes/`, then a decision).
- E1. Integrate a circular and an eccentric orbit for 10^5 to 10^7 steps in `f64` (leapfrog, a symplectic Runge-Kutta-Nystrom, an 8th-order method) and compare with the exact rail: the error against steps and against orbit size. Decides section B7.
- E2. A kinematic ship on a rail with two cars and a hinge in a frame that follows the rail: the rail unchanged to the bit by anything the cars do; the cars' motion the same as in a still frame; the cost.
- E3. A ship that coasts through park and load many times: no change of orbit (the owner's goal), against today's behaviour.

## C. Long burns and previews

**What is known [source].** KSP (stock) does not thrust during on-rails time warp; players use mods that fake it (the forum threads say that thrust is ignored on rails and that "physics warp" is limited to a low factor); KSP 2 reportedly lets a ship accelerate under warp. Principia (the KSP mod) has a flight planner
that integrates burns; its library lists adaptive Runge-Kutta-Nystrom methods (Dormand-El-Mikkawy-Prince 4(3), Fine 5(4)) and many symplectic ones (Blanes-Moan, McLachlan), and its default ephemeris settings use a fixed-step Blanes-Moan 14A **[source: its `methods.hpp` and a forum post]**; I did not confirm which it uses for burns.
For low thrust over many orbits the literature averages the equations of the orbital elements over each orbit (Gauss's variational equations; equinoctial elements are the usual choice), with analytic results for constant tangential thrust **[source: arXiv 2204.06959 and the papers beside it]**. An ideal solar sail has an acceleration of beta mu / r^2 cos^2(alpha) along the sail's normal,
alpha the angle between the normal and the sun line, so the trajectory is the integral of that with a control law for alpha **[source: the sail papers returned by the search]**.

**The owner's idea, and why it works [mine].** A thrust program (when, how hard, which way) and a starting state **determine** the trajectory: it is a function, not something that has to be stepped in real time. So:
1. A **burn program** is data: start tick, end tick, thrust (or acceleration) as a piecewise polynomial in time, mass flow, and an attitude law (fixed in the inertial frame, fixed in the frame of the orbit, to the sun, a cone angle for a sail). For an ion drive, mass is m0 minus flow x time, exactly; a sail's thrust depends on the position (1/r^2), which is in the integrand.
2. A **deterministic integrator** (a fixed method, a step that is a pure function of the state and the program, `f64` with `libm`, no wall clock, no `std` transcendental functions; `clippy.toml` already bans them, `powi` too) turns the program into **segments**: state at the ends, and a polynomial (dense output) between, so the place and speed at any tick are one evaluation.
3. The **preview** is the same call on the client or server: the same code, the same inputs, the same bits (a rule the project already holds for terrain and for physics). The **time warp** reads the same segments. A ship that burns at 1x also reads them, so a warped burn and an unwarped one are the same trajectory, which a stepped Rapier burn could not promise.
4. Saved: the program and the starting state, not the segments (they are recomputed; saves hold ids and numbers, as the hard rules say).
5. A change of mind (the player changes the throttle) starts a new program from the state at that tick. A burn that is steered by the keys, tick by tick, is a program of one tick, made as it goes.
6. Gravity during a burn: the parent's pull only (patched conics), plus the thrust. Then the comparison with the exact rail is available as a check: with the thrust at zero the integrator must give back the rail within its own error, which is test C1.

**Integration choices [mine, from the sources].**
- *Cowell* (integrate the whole acceleration): simple; error grows along a long burn.
- *Encke* (integrate the difference from the rail, and make the rail again from the state when the difference is large): the rail part is exact and the integrator only carries the small part; it uses the exact Kepler code of section A for every stage. Probably the best fit here.
- *Averaged elements* (Gauss's equations averaged over an orbit): steps of many orbits, for spirals of thousands of turns; not exact, and its result differs a little from a stepped burn, so if it is used it must be used for **both** the preview and the warp, and then the 1x burn must read the same segments too (point 3). Held in reserve for the very long ones.
- *Step size* from the orbit (a fraction of the local period, so a close pass takes small steps) by an integer rule; cost for a year at a low orbit with 1/200 of a period steps is in the millions of steps: seconds, so a preview is computed in the background, cancellable, and cached by (program, state).

**Open points.** The integrator's order and step rule (E1 and C1 decide); how a program is entered in the UI (not this document); what a sail does when it meets a shadow (the planet's, cheaply, by a cylinder test at stage points) or the heating of its sail; time warp as a game clock that runs faster than ticks (`future-goals.md` already lists it); how many programs a server keeps for ships nobody watches.

## Questions that decide the work

1. **Gravity model on rails.** Patched conics: a ship feels only its sphere's parent, the parent moves on its own rail. Should observed free flight use the *same* model (a kinematic body on the rail), so that observed and unobserved flight agree? (My advice: yes. The n-body pull of everything that weighs enough, which the game uses now, is then used only inside a shared frame for contacts.)
2. **Period.** For a ship that is put on a rail, the period comes from the parent's mu and the semi-major axis (consistent with its gravity). For planets in data packs, keep an explicit `period` as now, or compute it from the mass, or accept either?
3. **Hyperbolas** in the first step or the second? (My advice: the second.)
4. **Epoch.** Add an epoch to every orbit (planets default to 0), as above?
5. **Order of work.** A, then E1 to E3, then the ships' conversions (B5, B6), then C? Or the conversions first, with circles?
