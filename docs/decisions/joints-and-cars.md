# Joints and cars: what was decided (owner, 2026-11-01; nothing is built yet)

A joint lets one ship have parts that move against each other. The first one is a prototype, the **dev ball joint**, a block that couples two cars like a ball hitch.
This file records the talk that came before any code; the measurements behind the physics choices are in `joint-probe.md`. Status key as in `design-decisions.md`.

## Words (owner)
- **Ship** is the whole vehicle; a **car** is the blocks of one grid of it. (The code's `Ship` is one grid; it is to be renamed to say so, a mechanical change, not yet made.)
- **Dock / undock** are used only for docking ports, and **never** for anything else. **Lock / unlock** are the same act for joints. **Dock and lock merge grids; nothing else does.**
  Undock and unlock split them.
- **Couple / decouple**: two joint blocks come together and hold, or are parted. (Not "connect": that word is the network's, `ServerMessage::Disconnected`.)
  Coupling is automatic when the blocks meet; decoupling is a command. **Mobilize / immobilize**: let the joint move, or make it rigid at its present angle, without merging.
- "Hinge" is reserved for a joint with one axis. "Port" is not shortened from "docking port", because it may name fluid attachments later.
- The command is `/joint lock|unlock|mobilize|immobilize|decouple`. **Decouple is always available**: on an immobilized joint it mobilizes and decouples in one step, on a locked one it
  unlocks and decouples in one step.

## The block
- Id `dev_ball_joint` (creative only, like the dev docking port, which stays as it is). 0.75 x 0.5 x 0.5 m. It is put on the face that was clicked, facing away from it, one small
  end flush with the face and centred, the other end 0.25 m short of the next cell. It has **its own face property, `engine:joint_face`**, and its own finder, so that `ports_of`,
  `docked_pairs` and `dock` never see a joint as a docking port. The geometry the two share (`Port`, `PortView`, `approach`) gets a neutral name. A block may later have both properties
  (a joint that is also a docking port).
- Two such blocks facing across a cell boundary, 0.5 m apart at the tips, are **coupled**; a ball of 0.5 m diameter is drawn in the gap, and only then. The ball is two hemispheres, one in
  each cell: bent, the join shows; that is accepted for the prototype.
- States of a pair: apart, coupled (mobile), coupled and immobilized, locked. A joint is **derived from block states and positions and never saved**.

## Coupling, lock, unlock
- Couple when the pivots are within 0.3 m and the faces within 45 degrees; until then pull with 10 kN each (as for docking, independent of mass) until the pivots are within 0.05 m, then the joint.
  Coupling applies positional forces only, no torque toward 180 degrees.
- **Lock** is the only thing that turns the cars toward alignment (180 degrees, or the nearest turn that lines the grids up cube for cube). Within the docking snap (0.3 m, 10 degrees) it snaps
  and merges the grids. A lock request lasts until it succeeds or the joint is mobilized; on an immobilized joint it is refused with "mobilize first".
- **Unlock** splits the grids and lands in *coupled (mobile)*. Lock and unlock act on the **pair of cars**: unlock cuts every joint between them at once (the flood fill in `undock` returns
  `Welded` if the halves touch anywhere else).
- Cars collide with each other. Coupling across physics frames is refused for now.

## Physics (decided for the prototype; the reasoning is in `joint-probe.md`)
- **A tree of cars on Rapier multibody joints, one coupling per pair of cars.** A second coupling on the same pair, and a coupling that would close a loop, are refused with a message
  (Rapier's multibody set refuses them itself: `insert` returns `None`). The root car of a coupled group is **the heaviest car**. An immobilized joint is a fixed multibody joint.
- Impulse joints are the fallback if loops cannot be done another way; then at least 16 substeps (`num_solver_iterations`), not more (see `joint-probe.md`).
- To keep loops possible later: the physics layer is given a **coupling = two cars and a list of pivot pairs**, never one joint per block, so the choice of solver is inside one box.
- The **integer consistency rule** applies when a second joint couples the same two cars: the squared distance between the pivots must be equal in both cars' grids, in half-voxel integers
  (and, from four joints, the handedness). Without it the cars are permanently strained by the mismatch (measured: 1.44 times it). It does not apply to the closing link of a loop of
  three or more cars; that depends on the pose.
- Future, not scheduled: several couplings per pair reduced to one equivalent constraint (two on a line are a hinge, three not on a line are rigid), then a direct solver for loops
  that sits beside Rapier. Not a fork of Rapier.

## Bounds and lifecycle
- For choosing which bricks need colliders, a ship's bounds are the sum of its cars' longest dimensions used as a diameter, per car (a to-do is to improve this, `future-goals.md`).
- A coupled group **freezes and is parked together**, by the nearest of its cars, so that a joint never anchors an active car to a frozen one.
- Later, a joint between a planet and a car: lock and unlock to a planet reuse `dock_into_planet` and `undock_from_planet`; the open problems are in `future-goals.md`.
