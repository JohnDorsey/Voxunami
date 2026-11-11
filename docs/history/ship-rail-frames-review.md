# How a turning ship rides a rail: a review of steps 4c-1 and 4c-2

Written 2026-11-10, branch `worktree-dev-ball-joint`, commits `249559d` (4c-1) and `a86d1ea` (4c-2). The owner asked to review before the next step (the sleep rule). Line numbers are those of `a86d1ea`.

**How to read the claims.** *Read* means I read the code again for this report and did not run anything for the claim. *Ran* means a test or a probe run (in the session that built it, or in the re-run noted at the end). Where the two differ I say so.

## 0. The short version

Before 4c-2 a ship that turned could not be on a rail. Three separate rules kept it off: the candidate rule refused it, the upkeep rule took it off, and the world overwrote its turn with the rail's (which is none). A rail holds a ship's **centre of mass** and nothing else, so the turn has to come from somewhere; 4c-2 gives it to the bodies (Rapier).

The way it works:

1. The world stops refusing and stops overwriting the turn (three small edits in `server` and `world_storage`).
2. The bodies plugin gives such a ship **a frame of its own that goes with the ship's centre of mass** (`Followed::RailPoint`). In that frame the ship is a free body at rest, nothing pulls on it, nothing else is in it. The only thing that happens to it is Rapier's turning step.
3. Each tick the rail moves the centre of mass; the plugin's ordinary "someone moved this ship" check puts the body back at rest at the point; the bodies turn it; the new facing and spin are written to the world; the second `apply_rails` of the tick places the origin from that facing.

Where the risk is, in order (details in section 8):

| | Piece | Why | My confidence it is right |
|---|---|---|---|
| 1 | The per-tick re-sync that holds the body at rest at the point (section 4.5) | I wrote that the whole design depends on it and **it is implicit**: there is no line of code that says "re-centre", it falls out of a check written for something else | **Corrected on 2026-11-11 (section 10): it does not matter.** A mutant that switches the re-sync off for riders passes all 16 tests, because nothing pushes the body, so it stays at rest at the point without it. What I wrote here, "the bit-equality tests would fail if it were wrong", was wrong. (Whether the re-sync happens every tick I still did not count.) |
| 2 | `set_can_sleep(false)` on the rider (4.4) | It is a patch for a Rapier behaviour; the sleep rule (item 7) will want to own this | High that it works (ran, mutant caught); open question how it meets item 7 |
| 3 | What no test covers (section 8.3): a block edit on a railed turning ship, a ship tumbling after a blow, many riders at once, Linux | Plausible edge cases | I had no evidence either way. **Since tested, except Linux: section 10** |
| 4 | The world-side edits (section 3) | Small; each pinned and each mutant caught | High |

Writing this found five things that are wrong or stale and are not behaviour (section 8.1): two doc comments that now say the opposite of the code, a doc comment glued to the wrong function (older than this work), the decision document's "what the code does today" list, and a count in the build log (`never_asleep.rs` has 3 tests, the log says 2). **They are fixed in the commit that carries this report; the code change is comments only.** A sixth item, some warts in a test file, is listed and not changed.

## 1. The ground it stands on

### 1.1 Frames

Rapier is a float library, and a ship 400 km from the origin has no precision to spare. So the plugin (`engine/grid_bodies/src/plugin.rs`) does not keep one physics world; it keeps **one `BodyWorld` per place**, each with an origin at the corner of a brick near its ships:

```rust
/// The bodies of one place in a level, in one frame: the frame's origin is the brick of the first ship that made it.
struct Frame {
    level: LevelIndex,
    /// The point the frame goes with, if it goes with one (see [`Follow`]).
    follow: Option<Follow>,
    bodies: BodyWorld,
    ships: BTreeMap<GridId, Ship>,
    planets: BTreeMap<GridId, Planet>,
    terrain: BTreeSet<BrickCoordinate>,
}
```

A ship belongs to the first frame of its level that is near enough (500 bricks, about 16 km) and *goes with the same thing*. A ship farther than twice that from its frame's origin leaves and enters another; its colliders travel with it (`take_colliders`, step "colliders move between frames", measured 47.9 ms to 0.2-0.3 ms for a 100,000-block ship).

`follow` is the interesting field. A frame can **go with a point** that moves in the world: today the frame of ships carried by a planet on a rail goes with the planet. Inside such a frame the planet stands still, and the ships' places and velocities are the world's less where the point has gone and how fast it goes. That is how a ship resting on a planet that moves 30 km/s stays at rest in its frame.

### 1.2 Rails

A ship on a rail has no physics of its own: `ship.rail: Option<Rail>` is an orbit (or a spin), and the world puts the ship wherever the orbit says at the world's clock. The relevant fact for this review is **what the rail places**: not the ship's origin but its centre of mass. In `put_ships_on_their_rails_remembering` (`engine/world_storage/src/world.rs`, line 414):

```rust
            // What a rail has is where the centre of mass goes: the origin of the ship's coordinates is that less the centre of mass, turned as the ship is. (A planet's
            // centre of mass is its origin, and a ship that is not turned has its origin as far from its middle as it was.)
            let position = match position {
                Some(middle) if ship.centre_of_mass != [0.0; 3] => {
                    let aside = GridPose::new(middle, orientation)
                        .direction_to_world(ship.centre_of_mass)
                        .map(|part| -part);
```

So a ship that turns while its centre of mass follows the rail has an origin that **swings round the centre of mass**: `origin = railPoint − R·c`, with `R` the ship's current orientation and `c` its centre of mass in its own coordinates. The same function is how the world already placed a turned ship on a rail before 4c-2; what changed is only who decides `R`.

A ship is put on a rail by `update_coasting` after a quiet second (`QUIET_SECONDS = 1`): nothing disturbed it, no planet is too near, the orbit fits. Anything that pushes or moves it calls `disturb`, which drops the rail where the ship is (`engine/server/src/server_world/coasting.rs`, line 924):

```rust
    pub(super) fn disturb(&mut self, grid: GridId) {
        self.quiet_since.remove(&grid);
        let Some(ship) = self.data.grids_mut().get_mut(grid) else {
            return;
        };
        if ship.terrain.is_none() && ship.rail.as_ref().is_some_and(is_a_coasting_rail) {
            ship.rail = None;
        }
    }
```

The callers are `set_grid_pose`, `apply_grid_force`, `apply_grid_torque`, `set_grid_held`, `set_grid_velocity`. **`move_grid_by_bodies` is deliberately not one**: it is what the bodies call every tick, and it must not take a ship off its rail.

### 1.3 The tick, in order

From `engine/server/src/server.rs` (line 249) and the plugin's registration (`Phase::Physics`):

```
Server::step
  advance_clock(1)
  apply_rails            #1  every railed ship goes where its rail says at this time
  logic.tick             ...  the physics system:  Bodies::step
        follow_points          each frame that goes with a point: shift and velocity from the world
        leaving / sync_ship    ships change frames; a body is made, or set from the world if "moved"
        apply_gravity          pull on each free ship
        forces, torques        applied from the world's queues
        bodies.step()          Rapier, 3 to 24 substeps per frame
        write_back             pose, velocity, spin go back to the world
  update_coasting          ships left alone are put on rails; others come off
  apply_rails            #2  the same time again: every railed ship is placed from the facing the bodies just wrote
  send updates to clients
```

`apply_rails` runs twice because the second run exists so that a ship put on a rail *in this tick* is on it before any client is told where it is. For a rider it has a second job, which is the reason the pose is right at the end of the tick: it re-derives the origin from the facing the bodies wrote.

### 1.4 What kept a turning ship off a rail before

Three independent rules, each written when a rail was "a place and not a turn":

```rust
// coasting_candidate: a ship that turned was never put on a rail
if ship.angular_velocity != [0.0; 3] {
    return Err(NotCoasting::Turning);
}

// has_to_leave_its_rail: a ship on a rail that began to turn came off
ship.held
    || ship.angular_velocity != [0.0; 3]
    || self ...

// put_ships_on_their_rails: every tick, the rail's spin (none) was written over the ship's
ship.angular_velocity = state.angular_velocity;
```

The third is the one that matters physically: even if the first two had been lifted, a ship's spin would have been zeroed every tick by the world. 4c-0 pinned this (`292b72f`) before anything changed so that the change would be visible as a change.

## 2. Step 4c-1: a frame follows a point, not a planet

No behaviour change; the groundwork. `Follow` used to hold `planet: GridId` and read the planet's pose each tick. It now holds a `Followed`, and everything that depends on *what* is followed lives there (`plugin.rs`, line 257):

```rust
struct Follow {
    followed: Followed,
    /// Where the point was when the frame was made.
    anchor: WorldPosition,
    /// How far it has gone since, as of this tick.
    shift: Displacement,
    /// How fast it goes in the world, as of this tick.
    velocity: [f64; 3],
}
```

`Follow` is only geometry: a place in the world becomes `world − shift`, a velocity becomes `world − velocity`, and back.

```rust
fn local(&self, world: WorldPosition) -> Option<WorldPosition> { world.displaced(self.shift.negated()) }
fn world(&self, local: WorldPosition) -> Option<WorldPosition> { local.displaced(self.shift) }
```

`Followed` knows where its point is (`state`), what pulls on what falls with it (`gravity`), and which ships belong. With one case (`Planet`) this was a seam, not a generalisation; its value was that adding `RailPoint` in 4c-2 made the compiler name every `match` that needed an answer.

*Ran:* the 11 tests of `frames_that_follow.rs`, `frame_change.rs`, `fast_ships.rs`, `ship_frame_carry.rs` and the 32 unit tests of `grid_bodies` passed unmodified before and after. Two hand mutants were caught, one of them only by a single test (the free-fall gravity), which is why 4c-2 had to give the rail frame's zero gravity a test of its own. That test does not exist in a form that can see it: see 8.3.

## 3. Step 4c-2, the world side

Three edits and a change to freezing. All are small; each has a test that fails if it is put back (mutants D, E, H, F in section 7).

### 3.1 The candidate rule: turning no longer refuses a rail

`engine/server/src/server_world/coasting.rs`, line 358:

```rust
        // (A ship that turns may ride a rail, which holds its centre of mass and nothing else: the bodies do the turn. Not one that is coupled to another by a joint, until a rail
        // carries a group: its car would be on a rail of its own, in a frame of its own, and the joint would die with the body that has it.)
        if ship.angular_velocity != [0.0; 3]
            && self
                .coupled_ships
                .iter()
                .any(|&(first, second)| first == grid || second == grid)
        {
            return Err(NotCoasting::Turning);
        }
```

`NotCoasting::Turning` now means "turning *and* coupled". The reason for the exception: a joint lives in the `BodyWorld` of one frame and dies with a body that leaves it (`couplings.rs`). A jointed car on a rail of its own would be in a frame of its own and the joint would be cut. That is 4c-3.

### 3.2 The upkeep rule: turning is not a reason to leave

`has_to_leave_its_rail` (line 777) lost one line: `|| ship.angular_velocity != [0.0; 3]`. What still takes a ship off: held, a level with a pull, nothing to go round, something near (`near_with(…, 1.0)`; putting on uses a factor of 2, so there is hysteresis).

### 3.3 The overwrite: a rail with no spin has no turn to give

`world.rs`, line 436:

```rust
            // A rail with no spin has no turn to give: a ship's own turn is left to whoever turns it (the bodies, for a ship), and a planet has none.
            if rail_spins || ship.terrain.is_some() {
                ship.angular_velocity = state.angular_velocity;
            }
```

`rail_spins` is `rail.spin.is_some()`, read inside the earlier block so the borrow of the rail ends before the ship is changed. Planets (`terrain.is_some()`) and ships on a spin rail are unchanged, as the tests of `rails_that_are_not_an_orbit_round_a_planet_are_left_alone_and_so_are_planets` show. The orientation was already `state.orientation.unwrap_or(ship.pose.orientation)`: a rail with no spin keeps the ship's facing, which is what the rider needs.

### 3.4 Freezing a ship on a rail stops its turn

`server_world.rs`, line 1286. A ship no player is near is frozen: the bodies hold it. The owner's answer 4 (accepted): a frozen turning ship stops turning, but its rail goes on.

```rust
        let on_a_rail = ship.terrain.is_none() && ship.rail.is_some();
        if frozen && !ship.frozen {
            if !on_a_rail {
                ship.linear_velocity = [0.0; 3];
            }
            ship.angular_velocity = [0.0; 3];
        }
        ship.frozen = frozen;
```

Before, a railed ship's spin was never nonzero so the second line did not matter; now it must be zeroed or the frozen ship would keep a spin the bodies no longer apply. **Time warp:** this is the one rule that would change if ships are to keep turning during a warp (the owner's note of 2026-11-10). Everything else is compatible with it: the rail holds only the centre of mass, and the turn is the ship's own data.

## 4. Step 4c-2, the bodies side

### 4.1 Who rides: `Followed::of`

`plugin.rs`, line 312. Called for every ship, several times a tick.

```rust
    fn of(world: &ServerWorld, id: GridId, ship: &MovingGrid) -> Option<Self> {
        if let Some(planet) = world.carrier_of(id) {
            return Some(Self::Planet(planet));
        }
        let turns_on_a_rail = !ship.held
            && !ship.frozen
            && ship.angular_velocity != [0.0; 3]
            && ship.rail.as_ref().is_some_and(|rail| rail.spin.is_none());
        turns_on_a_rail.then_some(Self::RailPoint(id))
    }
```

Precedence: a planet that carries a ship wins. (A ship on a rail is carried by none, so the two never overlap; the order is moot today.) The riding test is *derived from the world's data every time*, never stored. That is the property that makes the transitions in section 6 work: when the world changes the data, the next tick's answer changes, and the ship changes frame.

`!ship.held` and `!ship.frozen` are redundant given the world's invariants: holding disturbs (drops the rail), and freezing zeroes the turn. I left them as a guard for data written by hand. Mutant B (removing `!ship.frozen`) is therefore *not caught*, and should not be: no honest sequence of calls produces the difference.

### 4.2 What a rail point is

```rust
            Self::RailPoint(ship) => {
                let grid = world.data().grids().get(ship)?;
                let middle = grid.pose.local_to_world(WorldPosition::new(
                    BrickCoordinate::ORIGIN,
                    grid.centre_of_mass,
                ))?;
                Some((middle, grid.linear_velocity))
            }
```

The point is the ship's **centre of mass in the world**, and its speed is the ship's `linear_velocity`, which the rail wrote in this tick's first `apply_rails`. The point is where the rail has the centre of mass, because that is what the rail placed. Three more answers, all about what is *not* in the frame:

```rust
    fn gravity(self, world: &ServerWorld, there: WorldPosition) -> Option<[f64; 3]> {
        match self {
            Self::Planet(planet) => Some(world.gravity_in_the_free_fall_of(planet, there)),
            Self::RailPoint(_) => None,           // nothing pulls what is in the frame at all
        }
    }
    fn has_planets(self) -> bool { matches!(self, Self::Planet(_)) }
```

`apply_gravity` does `continue` on `None`, and `sync_planets` skips a frame whose `followed` has no planets. The rider's frame has **one body in it**. The reasoning: the ship is on its orbit, so it is in free fall with its own centre of mass; the pull the bodies would add is exactly what the rail already accounts for. (Applying it would change nothing anyone can see: the pull on a body is the same at every point of it, so it turns nothing, and the place and speed it would change are overwritten, the place by the rail and the speed by the re-set at rest of 4.5. So skipping it is cost and tidiness, not behaviour; I left it unmutated for want of an output that sees it, section 7.)

### 4.3 Entering and leaving a frame

Frames are matched by *equality of what they go with*:

```rust
// the leaving loop, Bodies::step, line 833
leaving.extend(frame.ships.keys().filter(|id| {
    !present.iter().any(|(held, _)| held == *id)
        || world.data().grids().get(**id).is_none_or(|ship| {
            Followed::of(world, **id, ship) != follow.map(|follow| follow.followed)
                || local_in(follow, ship.pose.position).is_none_or(|place| {
                    farther_than(2 * FRAME_RADIUS_BRICKS, place.brick(), origin)
                })
        })
}));

// the entering side, sync_ship, line 950
let near = |key: &FrameKey, frame: &Frame| {
    key.0 == level_index
        && frame.follow.map(|follow| follow.followed) == followed
        && local_in(frame.follow, ship.pose.position).is_some_and(|place| {
            !farther_than(FRAME_RADIUS_BRICKS, place.brick(), frame.bodies.origin())
        })
};
```

`RailPoint(id)` equals only itself, so **every rider gets a frame of its own** and no other ship can enter it. A ship that stops riding (its rail is dropped, it stops turning, it is frozen or held) fails the equality, leaves, and enters an ordinary frame in the same step, bringing its colliders and weighing records (`Carried`). The reverse is the same. Frames with no ships are dropped (`self.frames.retain`).

The cost of a frame of its own was measured in the 4c-0 probe (one run, test profile): a `BodyWorld` with one ship of one brick is about 48 KB, and a tick of three steps is 1.3 µs for a still body and 5.0 µs for one that turns, so a thousand riders are 36 to 64 MB (by the size of the ship) and 5 ms a tick. That is a probe on a bare world, not a measurement of the plugin with a thousand riders (8.3). **Corrected 2026-11-11 (section 10): the "5 ms a tick" is the physics step alone. Measured in the engine, a rider costs about 38 µs a tick in all (60 riders: 2.3 ms against 0.03 for an empty world), which is 38 ms a tick for a thousand if it stays linear (not measured at that size), most of a 50 ms tick. The memory figure was not measured again.**

### 4.4 What a rider is to the solver

`sync_ship`, line 942:

```rust
        // A ship that turns on a rail is a free body in a frame of its own (the rail holds its centre of mass, the bodies turn it); any other ship on a rail is a fixed one.
        let followed = Followed::of(world, id, ship);
        let rides = matches!(followed, Some(Followed::RailPoint(_)));
        let kind = body_kind(ship.held, ship.frozen, ship.rail.is_some() && !rides);
```

`body_kind` maps held, frozen or on-a-rail to a fixed body; the `&& !rides` is what makes a turning ship dynamic. A non-turning ship on a rail is still a fixed body in an ordinary frame, as before (the owner's answer 3, a frame for every ship near a player, comes after 4c-2).

And line 1018, when the body is made:

```rust
            if rides {
                // (Rapier takes the speed from a body it puts to sleep, and a ship that turns slowly would stop: the turn of a ship on a rail is for good.)
                let _ = frame.bodies.set_can_sleep(BodyId(id.get()), false);
            }
```

This was found by a test, not predicted. A box at 0.002 rad/s was railed and then stopped dead after about 1.5 s: Rapier puts a body whose speed is under its threshold to sleep and **zeroes its velocity**, the world then read ω = 0, `Followed::of` stopped saying `RailPoint`, and the ship left its frame for good. It is the ship audit's PHY-2 reached by a new road. `engine/physics/tests/never_asleep.rs` pins both halves (a free body at 0.002 rad/s is stopped by default; with `set_can_sleep(false)` it keeps its turn to 1e-12). `BodyWorld::set_can_sleep` already existed (made for the cars of a mechanism, `bodies/couplings.rs`):

```rust
        } else {
            *body.activation_mut() = RigidBodyActivation::cannot_sleep();
            body.wake_up(true);
        }
```

It is called once, when the body is made. That is enough because the body is removed whenever the ship stops riding. How this meets the sleep rule is in section 8.4.

### 4.5 The subtle part: why the body stays at rest at the point

This is the piece I'd read hardest, because it is **implicit**. There is no line that re-centres the body. Here is why it happens (*read and worked on paper, not traced*).

Write `A` for the point's world place when the frame was made (`anchor`), `P(t)` for the rail's centre-of-mass place at tick `t`, `c` for the ship's centre of mass in its own coordinates, `R` for its facing.

- Tick `t` begins. `apply_rails` #1 puts the ship at `origin = P(t) − R·c` and sets `linear_velocity = v(t)` (the rail's). It does not touch ω or `R`.
- `follow_points` computes `shift = P(t) − A` and `follow.velocity = v(t)`.
- `sync_ship` computes `moved`. In a frame that follows something:

```rust
                Some(_) => {
                    written.0.orientation != ship.pose.orientation
                        || written.1 != ship.linear_velocity
                        || written.2 != ship.angular_velocity
                        || match (
                            local_in(follow, ship.pose.position),
                            frame.bodies.state(BodyId(id.get())),
                        ) {
                            (Some(place), Some(body)) => !same_place(place, body.position),
                            _ => true,
                        }
                }
```

  `written` is what the plugin last wrote *to the world* for this ship: the velocity `v(t−1) + (the body's tiny local velocity)`. The rail's new velocity `v(t)` is not bit-equal to it (an orbit's velocity changes every tick: the tests' ships go about 38 voxels a tick, 760 m/s, on a circle of 70,000 voxels, so they turn by v²/r ≈ 8 m/s², about 0.4 m/s each tick; that is arithmetic from the tests' own numbers), so `moved` is true every tick.
- `moved` calls `set_state` with `ship_state_in(...)`: position `origin − shift = (P(t) − R·c) − (P(t) − A) = A − R·c`; linear velocity `world − follow.velocity = v(t) − v(t) = 0` exactly; orientation and ω from the world, which are what the bodies wrote last tick.

So every tick the body is put **at rest, with its centre of mass at `A`**, no matter where the rail has gone or how fast. Rapier then sees a free body, still, in empty space, with some spin. That is why:

- the turn is the same bits at 70 km and 400 km and at a quarter more speed (the test below): nothing the body sees depends on the world place or the speed;
- `steps_for_this_tick` is constant: it sizes the substeps by the fastest body's speed, `linvel + angvel × reach`, and `linvel` is zero here (a free ship falling at 42 m/s gets a fourth substep and its turn differs in the last digits; the rider does not);
- the world velocity written back is `v(t) + 0`, so the ship's path in the world is the rail's, bit for bit (test: speed bits equal to an unspun twin).

**Corrected on 2026-11-11 (section 10).** The paragraph above says why the body is at rest at the point *if* the re-sync happens every tick (read, not counted). It does not say that the result *needs* it, and it does not: a mutant that makes `moved` false for riders (so that the body is set once, when the frame is made, and never again) passes all 16 tests of `turning_on_a_rail.rs`. The reason is that nothing acts on a rider (no pull, no planet, no other ship, no force: a force would drop the rail first), so a body that starts at rest at the point stays there, turning about its centre of mass, and the world's place for it is the frame's shift plus that. The re-sync is harmless and it would repair a body that something did move, but I called it "the whole design" and "the piece I'd read hardest", and that was wrong.

One consequence worth knowing: `set_state` each tick means the body's linear velocity is reset *by construction*. Anything that tried to push the rider (a force) would act for one step and be erased. That is fine because forces `disturb` the ship and drop the rail first, in the same tick.

### 4.6 Writing back

In a frame that follows something, `write_back` writes every ship, asleep or not:

```rust
                    let now = (
                        GridPose::new(position, state.orientation),
                        velocity_from(Some(follow), state.linear_velocity),
                        state.angular_velocity,
                    );
                    if now == record.written { continue; }
                    if world.move_grid_by_bodies(*id, now.0, now.1, now.2).is_ok() {
                        record.written = now;
                    }
```

`move_grid_by_bodies` writes pose, velocity and spin and does **not** disturb, so the rail survives. Then `update_coasting`, then `apply_rails` #2 places the origin from the new facing.

### 4.7 One tick of a rider, start to end

| Step | What happens to a box turning on a rail |
|---|---|
| `apply_rails` #1 | Centre of mass → `P(t)`; origin = `P(t) − R·c`; velocity = `v(t)`; spin and facing untouched |
| `follow_points` | `shift = P(t) − A`, `velocity = v(t)` |
| leaving check | `Followed::of` = `RailPoint(id)` = the frame's: stays |
| `sync_ship` | `moved` is true (velocity changed); `set_state`: at `A − R·c`, at rest, with the spin |
| gravity, planets | skipped: the frame has none |
| forces | none (a force would have dropped the rail already) |
| `bodies.step` | Rapier's gyroscopic step turns the box about its centre of mass, 3+ substeps |
| `write_back` | world ← facing' and spin', velocity `v(t)` |
| `update_coasting` | stays on the rail unless held, near something, in a level with a pull |
| `apply_rails` #2 | Same `P(t)`; origin = `P(t) − R'·c`; velocity `v(t)`; spin and facing' untouched |

## 5. Why the numbers behave as they do

**Sleeping.** Rapier zeroes the velocity of a body it puts to sleep. A free body turning at 0.002 rad/s was asleep and stopped within 200 steps. (*Ran*, `never_asleep.rs`; I did not read out Rapier's threshold, only found that this turn is under it.)

**Substeps.** `steps_for_this_tick` (`engine/physics/src/bodies.rs`, line 839):

```rust
        for (_, body) in self.bodies.iter() {
            if !body.is_dynamic() { continue; }
            let mut speed = body.linvel().length();
            let turn = body.angvel().length();
            if turn > 0.0 {
                // The farthest point of the body from its middle, by the boxes of its colliders.
                ...
                speed += turn * reach;
            }
            fastest = fastest.max(speed);
        }
        let needed = (fastest * self.settings.time_step / MOST_A_STEP_MOVES).ceil();
        (needed as u32).clamp(base, MOST_STEPS.max(base))
```

The number of substeps depends on the fastest body in the frame. The rider's frame has one body at rest, so its count depends only on its own turn. This is the "speed-independence" the design wanted (item 1 of the audit's list of what forbids mechanisms).

**The gyroscopic drift.** Rapier's step for a body's spin is explicit and of the first order. For a body spun about one of its own principal axes the spin does not change, so there is no error. For a body spun about no axis of its own (a box 2×3×5 at 0.37 rad/s), angular momentum drifts: 1.1e-3 of itself over 100 ticks with one substep, 2.8e-4 with four, 3.8e-4 in the Cave with three. Halving the step halves it. *Ran* (a probe on a bare `BodyWorld`, not kept, and the Cave test). The loop probe's earlier figure of 2e-10 was for the axis case. The owner's answer 5 is to leave it; what that means is that a ship left wobbling for an hour is well off the momentum it started with. The tests assert 2e-3 and say why.

## 6. Transitions and edge cases

| Situation | What happens | Evidence |
|---|---|---|
| A ship turning and coasting, left alone for a second | `update_coasting` fits an orbit and sets the rail (`spin: None`); next tick `Followed::of` says `RailPoint`; the ship leaves its ordinary frame and enters a new one with its colliders carried | *Ran:* `a_ship_that_turns_and_coasts_is_put_on_a_rail_and_goes_on_turning` |
| A force, torque, move, hold or new velocity | `disturb` drops the rail; `Followed::of` says nothing; the ship leaves the rider's frame with the velocity the rail gave it and the spin it had | *Ran:* `a_push_takes_a_turning_ship_off_its_rail_and_the_turn_goes_on` |
| Something comes near (64 voxels plus a second of closing) | `has_to_leave_its_rail` drops the rail the same way | *Read.* Docking reaches 6 m by default, so the rail is off long before a port could pull (*read*: `DockingSettings::range`, `NEAR_MARGIN`) |
| Freezing | The turn is zeroed, the rail stays, `Followed::of` says nothing; the ship becomes a fixed body in an ordinary frame; unfreezing leaves it not turning | *Ran:* `a_frozen_ship_on_a_rail_stops_turning_and_goes_on_round` |
| Save and load | The ship's spin is saved as it always was; after a load the rail and spin are there, `Followed::of` says `RailPoint` again | *Ran:* 20 ticks after a save and load are bit-equal to 20 ticks of the world that went on |
| Coupled by a joint, turning | Refused a rail (`NotCoasting::Turning`) until 4c-3 | *Ran:* `what_a_ship_cannot_coast_for_is_said` |
| The spin becomes exactly zero | `Followed::of` says nothing, the ship leaves the rider's frame and is a fixed body on its rail | *Read* |
| The rail is refitted (a change in the mass of what it goes round) | The frame stays; `Followed::state` reads the ship's world velocity each tick | *Read* |
| A sphere-of-influence hand-over | The rail's centre changes; the velocity is read from the world; the body is re-centred each tick anyway | *Read* |
| A block is built on or taken off a railed turning ship | The ship is weighed again, stays on its rail and goes on turning; its centre of mass stays on the rail's path and the whole ship takes a step by the shift of the centre (the owner's choice); a cut in two gives each part the speed of its own centre | *Ran*, section 10 (this was **not examined** when this report was written) |
| Time warp | Not built; the design leaves it possible (the rail holds only the centre of mass) | |

## 7. The tests

**`games/voxunami_cave/tests/turning_on_a_rail.rs`** (9 tests; the ships are put on rails directly with `install_a_rail`, setting facing and velocity first and the rail last, because setting either drops a coasting rail):

```rust
    scene.run(move |world| {
        let position = world.data().grids().get(ship).unwrap().pose.position;
        world.set_grid_pose(ship, GridPose::new(position, facing)).unwrap();
        world.set_grid_velocity(ship, [speed, 0.0, 0.0], spin).unwrap();
        let time = RailTime::of_tick(world.data().clock(), TICKS_PER_SECOND);
        let rail = world.coasting_rail(ship, time)
            .expect("the ship is clear of the ground and alone, so a rail may have it");
        world.set_grid_rail(ship, Some(rail)).unwrap();
    });
```

| Test | What it pins |
|---|---|
| `a_ship_that_turns_and_coasts_is_put_on_a_rail_and_goes_on_turning` | The whole road from a free tumbling cube to a rider, 1e-9 on the spin and 1e-6 rad on the angle |
| `a_ship_that_turns_on_a_rail_goes_where_one_that_does_not_turn_goes` | The middle within 0.01 voxel of an unspun twin; velocity bits equal; with a guard that the ship really turned |
| `the_turn_of_a_ship_on_a_rail_is_the_same_bits_wherever_it_is_and_however_fast_it_goes` | A wobbling box: identical bits at 70 km, at 400 km, at 1.25× speed; with a guard that it wobbled |
| `a_ship_that_wobbles_on_a_rail_keeps_its_angular_momentum_and_its_energy_to_the_steps_own_error` | Momentum and energy within 2e-3 at 100, 200, 300 and 400 ticks (the drift measured over 100 ticks is 3.8e-4; I did not record it at 400) |
| `a_ship_that_wobbles_on_a_rail_turns_as_a_free_ship_does` | Within 1e-12 of the same box free in an ordinary frame, over 80 ticks (see the note in the test: after about 100 the free one gets a fourth substep from its fall) |
| `a_ship_that_turns_slowly_on_a_rail_goes_on_turning` | 0.002 rad/s survives |
| `a_frozen_ship_on_a_rail_stops_turning_and_goes_on_round` | The rule of 3.4 |
| `a_push_takes_a_turning_ship_off_its_rail_and_the_turn_goes_on` | The exit; speed within 1 percent, spin within 1e-6 |
| `a_turning_ship_on_a_rail_that_is_saved_and_loaded_goes_on_as_if_it_had_not_been` | Bit-equal after a save and load |

**`engine/server/.../coasting_ships.rs`**: four new tests and four changed (the 4c-0 pin that said turning was refused now says the opposite). **`engine/physics/tests/never_asleep.rs`**: three tests (a free body at 0.002 rad/s sleeps and stops; with `set_can_sleep(false)` it keeps its turn; wake and re-sleep).

**Mutants** (each broken by hand, `PROPTEST_DISABLE_FAILURE_PERSISTENCE=1`, then put back; *ran*, before the commit):

| | Mutant | Caught by |
|---|---|---|
| A | rail ships stay fixed bodies | 7 Cave tests |
| B | `!ship.frozen` dropped from `Followed::of` | **not caught** (redundant, 4.1) |
| C | rider may sleep | the slow-turn test |
| D | the world overwrites the spin again | 2 server tests |
| E | turning refused again | 2 server tests |
| F | freezing keeps the turn | 1 server test |
| G | the rail point has no speed | 2 Cave tests |
| H | a coupled turning ship is allowed | 1 server test |

Not seen by any output: a rider with gravity or planets in its frame (the speed is reset every tick), so skipping them is cost and tidiness only.

## 8. Findings

### 8.1 Things that are wrong or stale, none of them behaviour

(Items 1 to 5 were fixed in the commit that carries this report; the line numbers below are those of `a86d1ea`, before the fix. Item 6 was not changed then; it was fixed with the tests of section 10.)

1. **`Follow`'s doc comment (`plugin.rs`, line 251) is stale.** It says "What a frame goes with: the planet on a rail that carries the ships in it", then describes the free fall and the missing terrain of a planet frame. `Followed` is documented right; `Follow` was not updated by 4c-1 or 4c-2. A rider's frame goes with a ship's centre of mass, has no gravity, and keeps no terrain for the same reason a planet frame does not (`keep_terrain`: `level_ground = follow.is_none()`).
2. **`body_kind`'s doc comment (line 378) says a ship on a rail is a fixed body, with no exception**, and the caller now passes `on_a_rail && !rides`. The function is right; the comment above it does not say what 4c-2 does.
3. **A doc comment is glued to the wrong function.** The line "Whether two places of a frame are the same (see `SAME_PLACE_VOXELS`)." sits on the line above `body_kind`'s doc (line 377), and `same_place` (line 389) has none. Not mine: it is there since `2423215` (planets on rails). It makes the rustdoc of `body_kind` start with the wrong sentence.
4. **`docs/decisions/joints-and-cars.md`, "What the code does today that these would forbid" (line 158)**: items 3 (a rail holds a place and not a turn) and 4 (bodies can sleep) were true on 2026-11-09 and are not now, for riders. The list is dated, and the "Built" note records the change, but a reader of the list alone is misled.
5. **The build log says `never_asleep.rs` has 2 tests; it has 3.** Fixed in the same commit as this report.
6. **Test-file warts** in `turning_on_a_rail.rs`: `on_a_rail` just calls `on_a_rail_is` (two names for one helper); `length` just calls `common::length`; the save test writes `let (saved, _) = build(); let mut saved = saved;`.

### 8.2 Design caveats worth knowing, not defects

- **One `BodyWorld` per rider**, so N turning ships on rails are N worlds. Measured only on a bare world (4.3).
- **The re-centring is a side effect** of a check written for "someone else moved this ship" (4.5). ~~A change to that check would break the rider without any test naming the cause.~~ **Corrected (section 10): a change to that check does not break the rider**; the mutant that switched it off passed every test. The side effect is not load-bearing.
- **`Followed::of` runs twice a tick per ship** (the leaving loop and `sync_ship`; the frame matching reuses the second answer). Each is a map lookup and a few comparisons; I did not measure it.
- **The drift of the first-order step** is the owner's answer 5 (leave it). The exact torque-free rotor step is the cure and is also what a warp would use.

### 8.3 What nothing tests (so I have no evidence either way)

(As written on 2026-11-10. Items 1, 2 and 3 are tested since, and item 6 is looked at: section 10. Items 4 and 5 stand.)

1. **A block built on or removed from a railed turning ship.** An edit changes the centre of mass; `Followed::state` reads `grid.centre_of_mass`, which the plugin updates at the end of the step (`weighed`). The rail would then place the new centre of mass at the rail point (the owner's "keep the centre-of-mass step" decision of 2026-11-10), the frame's point jumps, and the body is re-set the next tick. I believe it works, because the re-sync handles any jump; I have not run it.
2. **A ship tumbling after a blow** now goes on a rail after its quiet second and keeps tumbling. The design wants that. No test flies one, and the first-order drift (section 5) applies to it.
3. **Many riders at once**, in the engine and not on a bare world.
4. **Gravity and planets being skipped** (mutant analysis, 7).
5. **Linux.** Nothing in this work was run there. The determinism claim (same bits on both) rests on the same arithmetic as the rest of the physics, and is not checked.
6. **The `moved` flag every tick** (4.5). I derived it; I did not count it.

### 8.4 How this meets the sleep rule (item 7), next (built 2026-11-11: the rule exists and the call was deleted, see section 11)

The accepted rule is: a ship may sleep only while touching another ship or the ground. A rider touches nothing, so under that rule it never sleeps, and the `set_can_sleep(false)` call in `sync_ship` becomes redundant, not wrong. Two ways to take it: leave the call (it states the reason at the place it matters), or let the rule cover it and delete the call, keeping `never_asleep.rs` as the test that proves the rule covers riders. I would keep the test either way. I would also keep the explicit call until the rule exists and is tested for riders, since the test (mutant C) is the only thing that sees the PHY-2 road.

### 8.5 My opinion

The world-side edits (section 3) are small, each is pinned, and I am confident in them. The bodies side is a small amount of code (`Followed::RailPoint`, `Followed::of`, an `if rides`, two `continue`s) whose correctness depends on one implicit mechanism (4.5) and one patch (4.4). Both are tested through their outputs, not by name. If I were reviewing this without having written it I would ask for exactly two additions before building on it: a test that names the re-centring (a rider's body stays at rest at the anchor over 200 ticks, checked through `BodyWorld::state` if the plugin can expose a view for tests, or through the world's speed bits), and the block-edit case of 8.3.1. Neither is large. (**Done 2026-11-11, section 10, with a correction:** the block-edit case is tested; for the re-centring there turned out to be nothing to name, since the result does not depend on it. What is tested is the function that sets a rider's state, which a re-sync and the first making of the body both use.)

## 9. Re-run for this report

*Ran* at the start of this review, on the tree of `a86d1ea`: `turning_on_a_rail` 9 passed, `never_asleep` 3 passed, the 34 tests of `coasting_ships` passed. That is the three files this report is about; the whole workspace was last run before the commit (3,181 passed, 4 ignored), and was run again after the comment edits of this commit (see the commit message for the result).

## 10. Follow-up, 2026-11-11: the untested cases, tested

The owner asked for tests for the cases of 8.3 before the sleep rule is built. **Seven tests were added to `games/voxunami_cave/tests/turning_on_a_rail.rs` (now 16) and one unit test to `engine/grid_bodies/src/plugin.rs` (now 33).** Everything here was *ran*, on Windows, unless it says *read*. The numbers are what the tests measured; each test says its own in a comment.

| Test | What it pins | Measured |
|---|---|---|
| `the_tensor_that_the_tests_use_is_the_one_of_a_box` | The helper that works out a centre of mass and a tensor from voxels gives the formula of a box (a test of the test's own ruler) | exact to 1e-9 |
| `blocks_placed_on_or_broken_from_a_ship_that_turns_on_a_rail_change_how_it_is_weighed_and_nothing_else` | Four blocks placed (a bar) and a corner broken, on a box that wobbles on a rail: weight and centre of mass are those of the voxels; it stays on its rail; its turn goes on from where it was (against an untouched twin); its centre of mass stays on the rail's path; and it turns by the **new** tensor | turn 6e-5 rad and 2e-3 rad/s from the twin's after the tick; centres 7e-15 voxels apart; momentum drift 1e-4 to 4e-4, energy 6e-5 at most, in 300 ticks |
| `an_edit_steps_the_whole_ship_by_the_shift_of_its_centre_of_mass_in_the_tick_of_the_edit` | **Today's decision (owner, 2026-11-10, `ship-centre-of-mass-edit-options.md`):** the centre stays on the rail and the whole ship (its origin) takes a step of minus the shift, turned with the ship. Whoever builds option A changes this test on purpose | right to 1.4e-4 voxel for a shift of 0.35 |
| `a_ship_that_turns_on_a_rail_and_is_cut_in_two_gives_each_part_the_speed_its_own_centre_had` | Two blocks joined by a bar, cut in the middle, turning on a rail: two new ships, neither with the old number, on no rail, still turning; one moves against the other by omega crossed with the arm between their centres; the rider's frame is gone | 6e-5 m/s of 3.1; the mean of their speeds is 0.83 from the ship's, which is two ticks of 0.41 (the rail's own turn in the tick of the cut, and the pull of the tick the parts have had) |
| `a_blow_off_the_centre_sets_a_ship_on_a_rail_tumbling_and_it_is_put_on_a_rail_again_still_tumbling` | A force off the centre, once on a ship that was not turning and once on one that was: it takes the rail off, pushes and turns as the weight and the tensor say, the ship is on a rail again 20 ticks later (the second of quiet) and still tumbling, and nothing jumps at the hand-on (path, turn, angular momentum, energy) | push right to 1e-15; spin change right to 4e-4 rad/s from rest and to 1 percent from a turn; greatest kink 0.0207 (that of the circle); a tick's turn within 3e-4 of its spin; momentum 6e-4 and energy 8e-4 at most, over the free flight, the hand-on and 100 ticks on the rail |
| `a_ship_that_turns_on_a_rail_is_taken_off_it_by_a_ship_that_comes_near_and_the_two_meet_as_bodies` | A cube B put 40 voxels ahead of a turning cube A that is on a rail, closing at 20 m/s: A loses its rail in that tick and keeps its turn; then they touch, and B does not go through A. A rider is alone in its frame, so this is what makes a rider meet anything | spin kept to 1e-9; after the meeting A's spin is (0.9, 1.2, -3.1); the middles came to 4.19 voxels; they closed at 7.5 after, from 20 |
| `many_ships_that_turn_on_rails_each_turn_as_one_alone_does_each_in_a_frame_of_its_own` | 40 riders of four kinds, 400 voxels apart, put on rails in one tick: 40 frames, and after 60 ticks each one's turn is, bit for bit, that of a ship of its kind alone in its own world | |
| (`plugin.rs`) `a_ship_that_turns_on_a_rail_is_given_rest_with_its_centre_of_mass_at_the_point_of_its_frame_whatever_the_rail_does` | The function that sets a rider's body from the world: no speed at all, exactly, and its centre of mass is the anchor of the frame, however far the point has gone and however its speed and the ship's facing turn | |

**What they found. No fault in the code.** The four cases of 8.3.1, 8.3.2 and 8.3.3 do what 4c-2 meant. Four of my first versions were wrong, and the code was right each time: (1) the cut test expected a mean speed within 0.6 of the ship's, and it is 0.83 because the rail's own turn is one tick ahead of the pull (found by working out that 0.83 is two times 0.41, then written into the test); (2) the meeting test measured the gap between the cubes' *origins* (their corners), and a turned cube's corner is 2.6 from its middle, so "closest 1.47" was no pass-through at all (it measures the middles now); (3) the many-riders test put the ships on rails one at a time in one tick, and the later ships were still at rest, so the near rule saw 761 m/s of closing and refused the rail (a real property: ships put on rails in one tick must all be set going first, which is now `set_going_in` then `put_on_its_rail_in`); (4) the meeting test first had B 5 m/s slower, and a slower ship falls to a lower orbit, so it passed 6.5 voxels under A without touching and the test would have said nothing (20 m/s touches).

**Mutants** (each broken by hand with a script that makes a byte-for-byte backup and puts it back; `PROPTEST_DISABLE_FAILURE_PERSISTENCE=1`):

| Mutant | Caught by |
|---|---|
| a rider is not weighed again after an edit (the world keeps the old weight) | the edit test and the step test |
| the body is weighed once and an edit does not reach it (the world is right) | the edit test, at "the angular momentum changed by 0.50 of itself" |
| the parts of a cut get the speed of the ship, not of their own centres | the cut test, and only it |
| a rider ignores a ship that comes near | the meeting test, and only it |
| every rider follows one point, so one frame | the many-riders test, and only it |
| a turning ship is refused a rail again | 15 of the 16 |
| a blow does not take the rail | the blow test and the push test |
| velocity in a frame ignores the speed of the point | the new unit test and three older ones |
| **a rider is not set at rest again each tick** (`moved` is false for riders) | **nothing: all 16 pass** |

(A first try at "the body is weighed once" took the body's mass away altogether; every body was weightless, the physics ran away on all cores, it was stopped by hand and it says nothing.)

**A correction to this report.** The last mutant is the re-centring of 4.5, and it matters: I called it the piece the whole design depends on (risk 1 in section 0) and said the bit-equality tests would fail without it. They do not. Nothing acts on a rider, so a body set at rest at the point once stays there; the re-sync is harmless and would repair a body that something moved, but no output depends on it. 0, 4.5, 8.2 and 8.5 are corrected where they stand. That also answers the request I made in 8.5 for "a test that names the re-centring": there is nothing observable to name. (Whether `moved` is true every tick in the unmutated code I still did not count.)

**The cost in the engine, which I had only from a bare world** (a throwaway test, not kept; Windows, the test profile, a loopback server with no clients, two runs that agreed):

| Ships | Turning, on rails (one frame each) | Not turning, on rails (shared frames) |
|---|---|---|
| 0 | 0.034 ms a tick | 0.034 |
| 1 | 0.075 | 0.108 |
| 20 | 0.76 to 0.83 | 1.15 to 1.21 (1 frame) |
| 60 | 2.30 to 2.32 (60 frames) | 3.32 (2 frames) |

About **38 µs a tick for each rider** in all, linear from 20 to 60, and less than the 55 µs of a ship that does not turn (I did not look into why that is more). The 5 µs of the 4c-0 probe was the physics step alone, so section 4.3's "a thousand riders are 5 ms a tick" was wrong by a factor of seven or more: 38 ms for a thousand if it stays linear, **extrapolated, not measured**, of a 50 ms tick. The cost is the server's work for each ship (the rails twice a tick, the near rule, the plugin's per-ship look) more than the extra worlds; that would be the place to look if a thousand ships on rails is a case that matters.

**Still not tested:** Linux; a rider's frame having no pull and no planets (no output sees it); a refit of the rail, a sphere-of-influence hand-over, and a spin that becomes exactly zero, with a rider (*read* only); more than 60 riders; what a player sees of a step when an edit is made.

The workspace after this follow-up: 3,189 passed, 0 failed, 4 ignored on Windows (3,181 before, and 8 new tests). The whole workspace was run before a clippy fix to the new test file; after the fix the 16 tests of that file, the 33 of `grid_bodies` and the test of the documents were run again, and fmt, clippy on the whole workspace and `cargo doc` with `-D warnings` are clean.

See also: `docs/decisions/joints-and-cars.md` (the design and the owner's answers), `docs/history/build-log.md` ("Joints, step 4c-0", "Joints, step 4c-1", "Joints, step 4c-2", "Joints, tests for what a rider meets"), `docs/history/loop-probe.md` (the drift of the step), `docs/plan.md` (the order of steps).

## 11. The sleep rule, 2026-11-11

Section 8.4 said to keep the explicit `set_can_sleep(false)` of a rider until the rule existed and was tested for riders. It exists (`BodyWorld` lets a dynamic body sleep only while it touches something) and is tested for riders: with the rule switched off, `a_ship_that_turns_slowly_on_a_rail_goes_on_turning` fails (*ran*, a mutant). The call where a rider's body is made (`plugin.rs`, the `if rides` block) was deleted. `never_asleep.rs` has 2 tests now (the third, wake and sleep again in free space, is in `sleep_only_in_contact.rs` on the ground, where a body can sleep). The statements above about Rapier putting a free body to sleep (sections 4.4 and 5) describe the world before the rule.
