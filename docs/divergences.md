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

### D3. Brush bevels are axial only (unverified)
Brushes carry their six axial bounding planes as bevels, exact for boxes and for wedges extruded along an
axis (all the test level uses). Arbitrary convex brushes also need edge bevels; relevant for M9 (BSP).

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

### D14. Duck speed recovers three times faster mid-air in S17 at 64 tick (open)
In both 64-tick runs of `S17-jumpbug-256` and `S17-duckbug-256`, duck speed recovers by 0.140625 per
command instead of 0.046875 from tick 38 until it reaches 8, while the player is airborne, crouched and
holding duck. Every other column, the event sequence and the technique outcome match; the same scenarios
at 128 tick don't show it, and no other capture does. Cause unknown (it isn't timing: it happens on the
same tick in two sessions). It affects only the speed of a later duck transition.

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

### D10. KZ modes against GOKZ 3.6.4 (partly resolved, prestrafe open)
The GOKZ sources were not read (see `modes-notes.md`); the hooks are our own designs reproducing the
reference's numbers. Captured at 128 tick (`docs/verification.md`): jumps, perfect bhops and long jumps
match in both modes (most bit-exact). One measured fix: on a perfect bhop SimpleKZ moves the takeoff onto
the ground under the player instead of jumping from where the landing left them hovering inside the
ground probe (`S7_simplekz_128` tick 223); the mode's jump hook now receives the ground height and does
that. **Open:** the prestrafe curves differ from the first turning command (`S20-*_kztimer_128`,
`S20-*_simplekz_128`); exact agreement needs GOKZ's GPL-3.0 algorithms, which is a licensing decision.
KZTimer at 64 tick has no GOKZ counterpart (GOKZ 3.6.4 keeps the player in Vanilla at 64 tick).

## Measurement

### D11. Landing-origin correction (unverified)
`jumpstats` reconstructs the landing point by moving from the previous command's origin along the velocity
the sweep used until the feet reach the final height; MovementAPI's correction distinguishes more cases
[Ref §16]. The movement of the long jumps themselves is verified (`S19`), the jump-stat distance
reported in game is not compared.

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
