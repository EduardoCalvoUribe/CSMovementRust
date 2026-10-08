# Divergences

One entry per mismatch with the real game (plan §9.7): the capture that showed it, the cause, the fix,
and whether it corrects the reference. Captures come from CS:GO 1.38.8.1 (see `docs/verification.md`);
capture names are `<scenario>_<mode>_<tickrate>`. Entries marked **open** are measured mismatches that
are not resolved yet; entries marked **unverified** are choices no capture has exercised.

The measured CS:GO behaviors below were derived from captured numbers, bit for bit where stated, and
implemented from those measurements and the public SDK 2013 structure. No leaked code was used.

## Collision

### D1. The sweep's end distance comes from the end point (resolved)
`clip_box` used to compute the plane distance at the end of a sweep as `d1 + delta·n` (exact in real
arithmetic, and it kept tangent slides from re-hitting a plane in f32). Captures pick Source's form,
`d2 = (start + delta)·n - dist`: with it the first slope contact of `S13-46-slide_vanilla_64` (ticks
13 and 14) is bit-exact; with the old form z was 4 ULP off. The tangent problem the old form avoided is
handled by D2.

### D2. Brush clipping uses the Quake 2 "both ends in front" test (resolved)
A plane is skipped only when both ends of the sweep are in front of it (`d1 > 0 && d2 > 0`); the 1/32
epsilon enters only the hit fraction. The Quake 3 form we had (`d2 >= DIST_EPSILON || d2 >= d1`, plus a
1e-4 tangent tolerance) refused to let a sweep end inside the 1/32 gap. Evidence: on steep slides at 128
tick the server lets the hull end about 0.0004 from the slope and slides on from there
(`S13-50-slide_vanilla_128` tick 29 onward); a player pressed into a ledge face creeps 0.0286 into the gap
(`S14-*-cj_vanilla_128` tick 103); with the old test our player stuck on steep slopes (all bumps fraction 0,
velocity zeroed) where the server slid. This one change took the 128-tick failures from 24 to 3 and made
a separate "ignore a re-hit of the plane just clipped against" rule unnecessary.

### D3. Brush bevels (resolved for BSP maps)
Primitive brushes carry their six axial bounding planes as bevels, exact for boxes and for wedges
extruded along an axis (all the test level uses). BSP maps (M9) use each brush's sides exactly as the
compiler wrote them, including vbsp's axial and edge bevels, so arbitrary convex brushes are exact
there. Checked by replaying every capture on the compiled test map through the BSP backend: the same
verdict for all 156 captures (D18).

### D4. Grounded categorization probes a step further and snaps (resolved)
For a player who was already grounded and walking, `CategorizePosition` probes `2 + step_size` units
down, and if the player stays grounded the origin is snapped to wherever that main probe hit, walkable or
not (support may come from a quadrant probe), when the probe's fraction is strictly between 0 and 1. The
reference says only that such a probe exists [Ref §12.4]. Evidence:
- walking down 18-unit stairs drops exactly 18 units on the tick the hull leaves the upper step
  (`S12-18down_vanilla_64` tick 59); `StayOnGround`'s 18-unit trace ends exactly 1/32 above the lower
  step and misses;
- a player who lands hovering inside the 2-unit probe is pulled down on the next still command
  (`S4_vanilla_64` tick 47), with no movement at all;
- walking into a non-walkable 46-60 degree ramp keeps the player grounded and snaps them back onto the
  ramp after each 1/32 clip push (`S13-46-walk_*`, `S13-60-walk_*`), and sliding off a steep ramp snaps
  onto it (`S13-46-slide_vanilla_128` tick 77);
- a still player whose probe starts inside the 1/32 gap (fraction 0) is not moved (`S0`, z stays
  0.03124994).
An earlier fix (calling `StayOnGround` on `WalkMove`'s low-speed early-out) covered only the second case
and was replaced by this one.

### D15. Brush planes are built the way the BSP compiler builds them (resolved)
vbsp builds a plane from three points with x87 arithmetic: cross product, length rounded to f32, an f32
reciprocal, each component multiplied and rounded, and the distance from the rounded normal. Building
in plain f32 or f64 put several ramp normals and distances 1 ULP off the compiled map, which changes
slope contacts. The construction now reproduces every slope plane in the compiled test map bit for bit
(checked against the BSP plane lump). This is map compilation, not simulation; per-tick math stays f32.

### D16. ClipVelocity pushes off by 1/32 (resolved; differs from SDK 2013)
When a clipped velocity still points into the plane (`out·n < 0` after the clip), CS:GO adds
`normal / 32` instead of removing the residual (`out -= normal * adjust` in SDK 2013). Evidence: on
slope slides the server's post-clip velocity points away from the slope at exactly 1/32 unit/s on
exactly the ticks where the clip's residual is negative; the formula reproduces the captured velocity
bits (`S13-46-slide_vanilla_64` ticks 15, 18).

### D17. Traces work in box-centre coordinates (resolved)
As Source's `Ray_t` does (public `cmodel.h`), a hull trace starts from `origin + (mins + maxs) / 2` and the
end point is converted back by subtracting that offset. The rounding at the centre's magnitude is
observable: a resting player at z = 0.03124994 comes back from the first walking trace at exactly
0.03125 (`S1_vanilla_64` tick 0). With feet-space arithmetic, S1 was off by 1 ULP in z for the whole run.

How the end point `centre + fraction * delta - offset` is rounded depends on what stopped the sweep:
- stopped by an axis-aligned box brush (the BSP's separately traced "box brushes"): plain f32;
- stopped by a general brush (our wedges), or not stopped at all: the sum is kept in extended precision
  (the 32-bit engine's x87 code) and rounded once; only the centre start is an f32.
Each rule alone leaves dozens of captures 1-4 ULP off in z (airborne moves need the second, floor
landings and stair snaps the first, steep-slope contacts the second); together they took the BIT_EXACT
count from 25 to 45 of 77 at 64 tick and from 27 to 42 of 79 at 128 tick. The extended-precision branch
is the one place collision arithmetic is wider than f32, deliberately, to match the engine.

### D21. Box brushes decide hit or miss from exact touch times (resolved)
For axis-aligned box brushes (the BSP's separately traced box brushes, D17), the sweep hits when the
exact time it touches the box, `max(d1 / (d1 - d2))` over entering planes, is no later than the exact
time it leaves, `min(d1 / (d1 - d2))` over leaving planes. There is no epsilon in that test, and touching
exactly as the sweep leaves or ends still counts. The reported fraction and plane are unchanged: the
entering plane's `(d1 - 1/32) / (d1 - d2)`, clamped to 0. General brushes (our wedges) keep the Quake
form, `enter < leave` with the leave fraction `(d1 + 1/32) / (d1 - d2)`.

Found with the edgebug threshold sweep (`S16t-*`, `compare scenarios`): run yaws narrowed to adjacent
f32 values on each side of every switch between landing, edgebug and missing the platform's edge
[Ref §14]. Measured as the hull's overlap with the platform edge at the moment of entry, the real game
misses with up to 0.00038 units of overlap and edgebugs from 0.014. The Quake rule required more
than 1/32 (it called real edgebugs misses, `S16t-jump-1-miss_vanilla_64` in the first batch). A box
grown by 1/32, then a box with no epsilon on leave only, each moved the threshold past real misses.
The touch-time rule was then checked prospectively: the threshold pairs it generated, captured afterwards,
are all on the predicted side, bit-exact (`S16t-*` in `crates/movement/tests/captures/`, both tick rates,
three approaches). A wall face that a sweep's end point rounds onto exactly
(`P19s-0-bottomup_vanilla_64` tick 124) is why the comparison is inclusive: with `<` the sweep ended
embedded and the player stopped dead where the server slid along the wall. Nothing else changed: every
earlier capture keeps its verdict and BIT_EXACT count.

### D22. Enter fractions are clamped before planes are compared (resolved; differs from SDK 2013)
Among the planes a sweep enters, the one with the largest enter fraction is the hit plane, each
fraction clamped to 0 first, so of several planes whose 1/32 gap the hull starts in, the first one
listed wins. Public SDK 2013's general brush code compares unclamped fractions and clamps only the
stored result, which lets the plane the hull is furthest from entering win, and lets a later brush
with a negative fraction replace an earlier hit. That form was tried as the mechanism of pixelsurfs
[Ref §19]: a hull pressed within 1/32 of a wall, with its feet less than 1/32 above a horizontal seam
between two stacked brushes, then hits the lower brush's walkable top face, its vertical velocity is
clipped, the ground probe misses, and the player glides along the wall on every command. Searching
our model with it found glides of up to 230 commands on the test level's top-down slab lane, and it
predicted the same glides on the bottom-up lane once the compiled map's brush order was used. The
real game glided on neither: with feet 0.0078 above a seam and the hull 0.0005 from the wall it
fell straight through (`P19s-*_vanilla_64` tick 157, `P19s-*_vanilla_128`). So, at least for box
brushes, CS:GO does not pick a seam's top face this way, and stacked box brushes alone do not make
pixelsurfs. The `P19-*` and `P19s-*` captures stay as regression tests of that.

## Duck

### D5. Duck speed, transition rates and crouch spam (resolved)
Measured on `S15-ground`, `S15-spam` and `S15-slow` at 64 and 128 tick:
- each press or release costs 2 duck speed, floored at 0 (not 1.5);
- duck speed then recovers by 3 per second, every command, inside `Duck` after the penalty;
- a press is refused (the player keeps unducking) while duck speed, before this command's recovery, is
  below 1.5 (accepted at exactly 1.5);
- ducking on the ground moves `duck_amount` at `0.8 * duck_speed` per second; unducking (and a refused
  press) at `max(duck_speed, 1.5)` per second;
- the transition uses the recovered speed.
CS:GO also strips `IN_DUCK` from the saved buttons when a press is refused, but its press/release
detection still sees the held key; the comparator's re-sync accounts for that.

### D6. Duck hull timing (resolved)
On the ground the duck hull comes in when `duck_amount` reaches 1 and the standing hull returns once an
unduck brings it to 0.75 or below (between 0.7529 and 0.71875 in `S15-slow_vanilla_64`, exactly at 0.75
in `S15-ceiling_vanilla_64`), feet in place. In the air both transitions complete at once with the 9-unit
shift [Ref §10.1] (`S15-air`, `S14-*-cj`). `m_bDucked` clears as soon as an unduck starts, so the
comparator checks the hull through `FL_DUCKING`.

### D7. No "can't jump while unducking" check (unverified)
SDK 2013 refuses a jump during the unduck transition; the reference doesn't mention it for CS:GO. No
scenario jumps mid-unduck yet.

### D13. Duck speed crop and duck acceleration multipliers (resolved)
- The `1 - 0.66 d` crop of inputs and the move maximum [Ref §10.3] uses the duck amount produced by this
  command's transition: it is applied at the end of `Duck` (SDK 2013's `HandleDuckingSpeedCrop` runs
  before the transition). Releasing duck under a ceiling raises the wish speed on the first command
  (`S15-ceiling_vanilla_64` tick 160).
- The acceleration duck multiplier applies from the first command of a duck (`m_bDucking`), not only once
  the duck hull is in: a begin-duck-jump accelerates at 0.34 on its first command (`S14-*-dj` tick 0).
- With the duck hull in, the multiplier is `1 - 0.66` evaluated in f32 (0.33999997, so 250 times it is
  84.99999); during the transition it is the literal 0.34 (85.0). Both are bit-exact in the crouch-walk
  captures at 64 and 128 tick.

### D14. Duck speed recovers three times faster away from where it was last full (resolved)
`S17-jumpbug-256` and `S17-duckbug-256` at 64 tick showed duck speed recovering by 0.140625 per command
(9 per second) instead of 0.046875 for a stretch of the fall. Ten probe captures (`D14-*`, each varying
one factor of S17) isolated the trigger:
- not the place (it happens on the flat floor), not held keys (forward or none), not time since the
  jump (a later duck press moves the onset by the same amount);
- it needs horizontal movement: a standing jump never shows it, and a 30 unit/s crouch-jump (S14) has
  recovered before it could;
- the onset is when the horizontal distance between the origin at the start of the command and the
  origin at the start of the last command that ended with full duck speed (8) exceeds 64 units. This
  fits every probe exactly: 198 u/s (`D14-flat`, `-late`, `-flatw`), 250 u/s (`D14-run`), 130 u/s
  (`D14-walk`, which rules out "60 units from the press"), and a decelerating ground slide
  (`D14-slide`, which rules out the distance actually travelled since the press);
- recovery then runs at 9 per second until duck speed is full again, where the anchor resets.
It applies on the ground and in the air. `Duck` now keeps that anchor in `PlayerState` and picks the
rate before recovering; both S17 captures at 64 tick are bit-exact and promoted with three probes.
The 128-tick S17 captures never moved 64 units with reduced duck speed, which is why they didn't show it.

## Speed and acceleration

### D8. Walk and duck crops in the air (partly verified)
The crops scale inputs and the move maximum [Ref §10.3], [Ref §8]. Ground walking (`S3`, bit-exact) and
crouch-jumps (`S14`) are verified; shift-walking in the air is not exercised by any scenario.

## Ladders

### D9. Ladder brushes are player-solid (resolved)
CS:GO's invisible-ladder brushes block the player as well as marking the ladder: walking at the ladder
stops 1/32 short of it (`S18-a_vanilla_64` tick 55). The primitive world now treats ladder brushes as
solid for movement traces. Climb speed `200 * ladder_scale` is consistent with `S18` (WITHIN_EPS); walking
and ducking on ladders are not exercised.

## Modes

### D10. KZ modes against GOKZ 3.6.4 (resolved by porting GOKZ)
Our first KZ hooks were designs from the reference's numbers. Captures at 128 tick matched jumps, bhops
and long jumps, but not prestrafe: both modes diverged from the first turning command (`S20-*`), and on
the first grounded command after a fast long-jump landing (`M9-lj-*`) our hooks cut speed to 250 where
GOKZ left 250.45. With the project now GPL-3.0, both modes are ported from GOKZ 3.6.4 and MovementAPI
2.4.4 (`docs/modes-notes.md`). Results: `S20-a/b` in both modes WITHIN_EPS with every horizontal column
bit-exact (the one landing `origin_z` tick is the open landing difference below); the `M9-lj-*` runs
BIT_EXACT end to end; no other KZ capture got worse. The 250.45 came from GOKZ reading the previous
command's strafe key and turn on the command after landing and adding two 0.0009 increments.
Also found in the GOKZ source: KZTimer, like SimpleKZ, runs only at 128 tick, which is why GOKZ kept the
player in Vanilla at 64 tick during capture.

## BSP maps (M9)

### D18. BSP collision against the primitive world and the real game (resolved for brushes)
The BSP backend walks the node tree (near side first, without the engine's fraction-based early out)
and clips each candidate brush with the primitive world's routine; axis-aligned six-sided brushes take
the box-brush end-point rounding of D17. Replaying every capture on `csmove_capture.bsp` (the compiled
test level) gives the same verdict for all 77 captures at 64 tick and 79 at 128 tick, and the same
BIT_EXACT counts (47 and 42), so tree order, plane order within a brush and box-brush detection don't
change any result there. The test map's ladder brush has contents `LADDER | TRANSLUCENT | GRATE`; GRATE
is in `MASK_PLAYERSOLID`, which is why ladders block the player (D9).

### D19. Displacements, props and surfaces (unverified)
- Displacement collision is our own swept box against each triangle (both faces, the edge planes, and
  every box-triangle separating axis as bevels). The engine's displacement trace is a separate routine
  (`CDispCollTree`) whose epsilons and end-point rounding we haven't measured; no capture covers a
  displacement yet. The split diagonal of each quad (a checkerboard) is also unverified.
- Displacement vertices are laid out from the corner nearest `startPosition`, rows toward the next
  corner. On de_dust2 this layout makes 23% more vertices coincide between neighbouring displacements
  than the transposed one, which is the check it rests on.
- Static props are not solid, materials' surface properties (friction) are not read, brush entities are
  solid only at their spawn position, and triggers do nothing (M10).

### D20. No `CheckStuck` (open, out of the movement path we compare)
A player whose hull starts exactly touching a wall is inside solid for both of us (exact contact counts
as inside, as in brush tracing), and neither moves. The engine then runs SDK 2013's `CheckStuck`, which
periodically tries small offsets from a fixed table and moves the player to the first free one:
`M9-lj-0_vanilla_64` (a runway start with the hull touching kz_longjumps_v4096's back wall) stays put
for 36 commands and is then nudged 0.125 units sideways and dropped onto the floor. We don't model
`CheckStuck`; the scenario now starts one unit off the wall. Normal play never starts inside solid.

## Measurement

### D11. Landing-origin correction (resolved against GOKZ)
`jumpstats` reconstructs the landing point by moving from the landing command's start origin along the
velocity its sweep used (horizontal velocity after that command's air acceleration, vertical velocity
minus half a gravity step) until the feet reach the resting height under the final origin, 1/32 above
the floor; takeoff is the jump command's start origin; long jumps add 32. Fitted to GOKZ 3.6.4's
in-game reports, not taken from its code: on kz_longjumps_v4096 at 128 tick, six long jumps in KZTimer
and SimpleKZ (`M9-lj-*`) report 266.6992 (one 266.6991) in game and 266.6992 from our tracker, on our
own replay and on the captured trajectory alike. Using the horizontal velocity from before the air
acceleration, as we did, gave 266.6093. Other jump types (bhops, ladder jumps) are not compared yet.

## Remaining small differences (open)
After the fixes above, the captures that are WITHIN_EPS rather than WITHIN_ULP are off on one or two
ticks each (re-synced): one `origin_z` tick on some ramp walks and 128-tick ledge jumps, one `vel_x` tick
on long-jump strafes, and the ladder climb velocity (155.99998 vs 155.99997 on every climbing tick).
Computing view-angle sin/cos the x87 way (wide, rounded once) changed nothing, so the ladder and strafe
cases have another cause.

## Out of scope

- **Fall-damage slowdown.** Landing hard enough to take damage sets a recovering velocity modifier
  (`m_flVelocityModifier`) that scales acceleration. It's damage, not movement, and isn't modeled; the
  capture rig resets it between runs.
