# Divergences and unverified choices

The plan (§9.7) asks for one entry per resolved mismatch with the real game. No real-server captures exist
yet, so there are no measured divergences. This file records the opposite case for now: places where the
implementation made a choice the reference does not pin down, or departs from a literal port. These are the
first things to check when captures arrive.

Each entry gives the choice, why, and what a capture would need to show.

## Collision

### D1. `d2` is computed from `d1` plus the projected motion
`world/primitive.rs` computes the end distance of a sweep against a plane as `d1 + delta·n` rather than
`end·n - dist`. The two are equal in exact arithmetic. With the second form, a move clipped tangent to a
non-axial plane picks up rounding noise and can read as entering the brush at fraction 0, which pins the
player to a 50 degree slope forever. Found by `collision::slides_off_slope_above_threshold`.
*Capture check:* S13 slide on 46/50/60 degree ramps.

### D2. Tangent tolerance of 1e-4 units
Even with D1, `v·n` after `ClipVelocity` keeps about 1e-5 of rounding noise when velocity components are
near 100 units/s, enough to read as entering a plane the hull is resting against from inside the
`DIST_EPSILON` gap. A sweep that starts in front of a plane and approaches it by less than
`TANGENT_TOLERANCE` (1e-4, 300 times smaller than `DIST_EPSILON`) is treated as tangent. Source's real
behavior here may differ, and this is the most likely place for surf/ramp-bug differences [Ref §13.4].
*Capture check:* S13 and any surf capture; compare bump fractions with DHooks traces.

### D3. Brush bevels are axial only
Brushes carry their six axial bounding planes as bevels. That is exact for boxes and for wedges extruded
along an axis (all the test level uses), but not for arbitrary convex brushes, which BSP compilers also give
edge bevels. Relevant only for M9 (BSP).

### D4. Grounded step-size probe
[Ref §12.4] mentions a longer downward probe for a previously grounded walking player. It is implemented as
the SDK 2013 `StayOnGround` at the end of `WalkMove` (trace down by step size, snap if walkable), and
`CategorizePosition` keeps the 2-unit probe. *Capture check:* S12 walking down 16/18-unit stairs and
slopes, ground flag per tick.

## Duck

### D5. Duck transition rates
Duck speed starts at 8 per second, each press or release costs 2 (floor 1.5), and it recovers at 3 per
second (`MovementConfig::duck_speed_*`). The reference only says these mechanisms exist [Ref §10.3].
*Capture check:* S15, `duck_amount` and `duck_speed` columns.

### D6. Airborne transitions snap `duck_amount`
In the air, ducking and unducking complete in one command (hull, ±9 origin shift, re-categorize) and
`duck_amount` snaps to 1 or 0. On the ground the hull swaps when the transition finishes. The reference
establishes the airborne shift and the grounded/airborne split [Ref §10.1], not the timing of
`duck_amount` itself, which affects the `1 - 0.66 d` crop.

### D7. No "can't jump while unducking" check
SDK 2013 refuses a jump during the unduck transition. The reference describes CS:GO selecting the duck/reset
branch from either the ducking or ducked flag [Ref §9, §10.2] and doesn't mention the refusal, so it is
omitted.

## Speed and acceleration

### D8. Walk and duck crops scale inputs and the move maximum, in the air too
`CheckParameters` scales inputs and `max_speed` by `1 - 0.66 d` [Ref §10.3], by 0.52 while walking (not
ducked), and by the stamina factor [Ref §8]. Applying walk in the air is a choice. *Capture check:* S3, S5
with shift held.

### D9. Ladder climb speed
Climb speed is `200 * ladder_scale` (0.78 Vanilla, 1.0 KZ). Walking and ducking don't change it yet,
though [Ref §18] says they can. The ledge helper flag is recorded but not simulated.

## Modes

### D10. KZ mode hook algorithms are designs, not ports
The GOKZ sources could not be read during implementation (see `modes-notes.md`), so the prestrafe
growth/decay rates, SimpleKZ grace interval, perf windows, and the KZTimer jump+duck suppression rule are
our own designs that reproduce the reference's numbers (276 cap, 380 perf cap, SimpleKZ takeoff formula).
*Capture check:* the M-* scenarios.

## Measurement

### D11. Landing-origin correction
`jumpstats` reconstructs the landing point by moving from the previous command's origin along the velocity
the sweep used until the feet reach the final height. MovementAPI's correction distinguishes more cases
[Ref §16]. The HUD shows both corrected and raw distances.
