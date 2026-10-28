# The great rivers

Status: first prototype built 2026-10-27 (owner's request: "the generated object that uses all LOD levels"). What I checked by running code is marked **[run]**; what I believe from reading is **[belief]**.

## 1. What the owner asked for

- A generated object that uses **all levels of detail** and spans **the entire universe**: the **great rivers**.
- Made of one new block, **great river water**, which **does not flow**.
- Diameter from **1 billion light years** down to **under 100 metres**, varying randomly along the river.
- **The first prototype has one path across the universe.** Later rivers branch and merge again.
- **Not near the main planet**, and a command to get there.

## 2. The shape

A river is a **path**: knots at whole-voxel positions, joined by straight **courses**. A course is `start + steps * direction`, the direction a vector of at most 5 along each axis. At each knot the river has a **radius**; along a course the **base-2 logarithm of the radius changes in a straight line** (so the radius changes by the same factor over each stretch of the same length). The water is the union of

- a **ball** of the knot's radius at every knot, and
- a **tube** along every course: the voxels whose distance from the course line is under the radius at the nearest point along it, between the planes at right angles to the course through its two ends.

The balls close the gap on the outer side of every bend. Where two courses meet at an angle the two tubes overlap on the inner side (a union, not a swept sphere).

**Why small whole directions.** The offset of a voxel from the start of a course, crossed with the direction, and dotted with it, are exact in an `i128` for any voxel of the world (a coordinate is at most 2^120 and the direction at most 5, so the products stay under 2^125). A tube 80 metres across is as sharp 4 x 10^26 voxels from the origin as at it **[run: `a_tube_is_sharp_at_the_far_end_of_the_world`, and the exact checks against integer arithmetic]**. Only the comparison with the radius is in `f64` (`+ - * /`, `sqrt` and `libm::exp2`, the same bits on every machine).

**The first river** (`RiverPath::across_the_universe(seed)`):

- 93 billion light years across the universe, so it starts at x = -46.5 billion light years and goes on until x is past +46.5 billion. 42 knots for seed 2024 **[run]**.
- Each course advances 0.8 to 3.5 billion light years along x (3 to 5 per step) and wanders sideways by up to 2 per step in y and z. A course that would leave the room turns the other way, so the path stays between 5 billion light years and the edge of the universe in y and within the universe in z.
- **Not near the planet:** the path never comes below y = 5 billion light years, and the widest tube has a radius of 0.5 billion, so no water is within 4.5 billion light years (4 x 10^25 voxels) of the origin, where the first planet is **[run: `no_water_is_near_the_first_planet` is by the bounds, which are sound]**.
- **Radii:** the log of the radius at each knot is drawn evenly between the narrowest and the widest, so every power of two is as likely as any other (83 of them), and two knots chosen by the seed are set to the widest (diameter exactly 1,000,000,000 light years = 9.46 x 10^24 voxels) and the narrowest (**80 metres**) **[run]**. (The owner said "down to just 100 metres" and "under 100 metres": 80 is under.)

**Radius law and the extremes.** Between a knot of radius 2^70 and the next of 2^10 over 2.6 x 10^25 voxels the radius halves every 4 x 10^23 voxels. So the tube is a long horn, not a pinch: the thin parts are thin for thousands of light years.

## 3. How it is generated at every scale

The river is a `DensityField` (positive inside) with **bounds over any box** that are sound and cheap, so the same machinery as terrain works at every detail level from a brick to the root of the octree (116).

**Bounds.** The box is replaced by a ball around its middle (radius half the diagonal). For a tube, the distance of any point of the box from the course is within that radius of the distance of the middle, the position along the course is within it times the length of the direction, and the radius of the tube is monotone in the position, so the value of the piece over the box lies between two numbers. A box beyond either end plane is not touched; a box between them and inside the narrowest radius it can have is all water. The union takes the greatest of each end. Every bound is made loose by a margin of 10^-12 of the sizes it works with (+ 1 voxel), more than the rounding of a sample, so that a sample is always inside its box's bounds. **[run: property test over boxes from one voxel to 2^90, at every kind of place; mutation experiments below.]**

**Pieces near a box.** A river has about 85 pieces; any one brick or node is near two or three. `near` drops the pieces whose upper bound over the box (with room round it) is not positive: the field over the box is then the same as the field of what is left, so a brick costs a few pieces, not eighty.

**Bricks:** bounds say all air or all water for most; a mixed brick samples its 32,768 voxels. **Nodes and `may_show`:** the same code as every density field (`coarse.rs`), over the pieces near the cube and its neighbours, with the river's water standing in for stone, dirt and grass (a river is one block).

**Measured [run, release, one thread, this machine; a throwaway test, not kept]:** a brick near the narrowest part 0.3 ms on average over a 9 x 9 x 9 block (104 of 729 mixed; a mixed one about 2 ms), 3 microseconds where it is all water or all air; a node at level 1 on the thread 52 ms, at level 5 4 ms, at every level from 10 to 116 under 1.3 ms; `may_show` 3 to 17 microseconds.

## 4. The block and the level

- `base_blocks:great_river_water`: a whole cube, blue, **passable** (bodies move through it, as through fire), no light, no fire, no shapes. **It does not flow**: nothing in the game makes a block flow, and this has no rule that would. It is the eighth block (id 7); `voxunami_cave:dev_docking_port` is now 8.
- `base_blocks:great_rivers_v1`: the generator, the fourth of the table (after the hills, the planet and the empty terrain, so that the numbers of the generators of older worlds stand). **Like every generator it is a file format**: a changed river is a new version.
- A new world's `mini_cosmos` has it as its terrain instead of `empty_v1`. A world saved before is loaded as it was (empty space).

## 5. `/river`

`/river` says how many knots the river has and where it is narrowest and widest. `/river narrowest`, `/river widest` and `/river N` put the player three radii and 200 metres from knot N, **with the knot straight ahead of where they are looking** (or at right angles to the river if that place is in the water), at rest. It works in a level whose terrain is the great rivers; elsewhere it says so. It is a stopgap for the redesign of teleport (below).

## 6. What was checked

- **Tests (`terrain::great_river`, 23; `voxunami_cave/tests/great_river.rs`, 8; `rivers`, 3).** The extremes (widest diameter 1,000,000,000 light years to a relative 10^-12, narrowest 80 metres, for four seeds); every power of two in between has a knot; the path goes from -46.5 to +46.5 billion light years, stays above 5 billion in y and inside the universe in z, and is a chain; no water within 4 billion light years of the origin (by the bounds); a tube is exact against integer arithmetic along an axis and along a slant (a box of 324 x 118 x 223 voxels, every voxel), sharp at 2^119 voxels from the origin, and its radius is a power of two along it; a bend is closed by the ball; **the bounds hold every sample in a box** (400 random boxes from 1 voxel to 2^90 across at random distances from the knots of eight rivers); a box across a taper is not called water by the wide end; **making the field near a box changes no sample in it**; the narrowest part is 80 to 100 voxels across a line of voxels from the bricks (the line is at a slant), one run of water with air each side; bricks in the middle of the widest part are water and far from all rivers air; the surface is in the bricks where the voxels can place it (the knots of radius under 2^40); nodes on the surface have water and air cells at every level where it is resolved (100+ nodes); the whole world (root and corner cubes) can be asked about; generation is the same in four threads; **the river of seed 2024 and four surface bricks are pinned** (the file format).
- **Mutation experiments, 13 mutants of `great_river.rs`** (`PROPTEST_DISABLE_FAILURE_PERSISTENCE=1`; the file was restored to the byte afterwards): the upper bound ignoring the box, the lower bound using the wide end, the ball's upper bound ignoring the box, `bounds_through_finer` unknown, an open tube end, `near` keeping nothing or everything, the radius linear instead of a power of two, a lower bound that ignores the ends, ends not skipped, no clearance from the origin, balls dropped, the widest not forced: **all killed**. The first run left one alive (the wide-end lower bound: random boxes seldom span a strong taper), which is why `a_box_across_a_taper_is_not_called_water_by_the_wide_end` exists.
- **The game.** A throwaway picture test (deleted) put a player at the narrowest knot with `/river` and drew the level with the real renderer: from 50 metres the wall of the thread fills the view in the block's blue, with the camera at brick 3.4 x 10^24 (so the renderer's camera-relative positions are fine at that distance), and from 120 metres one brick of the surface was in the view of the test's radius of 3 bricks (the real default is 12). I did not look at the widest part: Cave does not draw a level beyond its bricks, so from 1.4 x 10^25 voxels there is nothing to see until the water is round the player. **[run]**
- **The golden session** (`PINNED`) moved by 152 bytes: 108 for the block, 28 for the generator, 16 for `/river` in the list of commands; the control runs that show this are in the comment on `PINNED`.

## 7. What it does not do, and known limits

- **A thin river vanishes from the coarse nodes.** A cell is drawn when it is at least half full, and a cell is as full as the share of 64 points in it that are water. A tube 80 metres across is seen as nodes up to about level 5 (cells of 32 metres) and not above: from farther the thread is not drawn. In general the river is drawn at a scale as far as its width is more than about one cell. This is the coarse cells' rule for any thin feature, not the river's; a representation that keeps thin things (a cell that holds a line) is a design question. **[run: the node at level 10 on the thread is uniform.]**
- **Cave does not draw the terrain of a level beyond its bricks** (`GameDefinition::coarse_detail` is none), so the river is seen as bricks only. Turning it on draws the overworld's hills as nodes as well, and a distance of 10^26 voxels for the space level is not the overworld's. See the open question in `future-goals.md`.
- **The renderer's depth range.** I believe (arithmetic, not measured) that distances past about 10^19 voxels overflow in the shaders' `f32` squares, which is the next piece of work.
- **`f64` places the surface of a river 2^52 voxels wide to about 2^(r - 52) voxels.** The widest tube's wall is a staircase with steps of about a billion voxels. Nothing sees that.
- **The cost of being near the wall of the widest part.** Bricks within 10^-12 of the radius of the surface (5 x 10^12 voxels at the widest) cannot be classified by the bounds and are sampled voxel by voxel: about 2 ms each, as measured for the mixed bricks at the narrow part **[run; the widest part's cost per voxel is the same, I did not time a brick there]**. Nobody arrives there except by flying for a long time.
- **Rivers do not branch or merge yet.** `GreatRivers` holds a list of paths; a path is a chain from a start.
- **No other game's world has them.** 2048 and Eels are untouched.
