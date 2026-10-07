# Mode notes (M7)

All three modes run on the same core (`process_movement`); a mode is a `MovementConfig` preset plus hooks
(`MovementMode` trait in `crates/movement/src/modes/mod.rs`).

The KZ modes are ported from **GOKZ 3.6.4** (`gokz-mode-kztimer.sp`, `gokz-mode-simplekz.sp`; KZGlobalTeam,
GPL-3.0), including the player state they read from **MovementAPI 2.4.4** (DanZay, GPL-3.0). This project
is GPL-3.0 for that reason (see `README.md`). Earlier versions used our own hook designs from the
reference's description [Ref §20]; captures showed their prestrafe differed (docs/divergences.md D10),
and the port replaced them.

## Hooks the port needed
GOKZ changes the game through function hooks, so the core exposes the same points:
- `player_max_speed`: GOKZ's `GetPlayerMaxSpeed` hook. The move data's maximum speed starts from it
  (250 × the prestrafe modifier) before the walk/duck crops, on the ground and in the air.
- `air_wish_speed`: GOKZ's `AirAccelerate` pre-hook, which divides the wish speed by the modifier.
- `can_unduck`: GOKZ's `CanUnduck` detour (no unduck on the command after landing fully ducked).
- `pre_command` / `post_command`: `OnPlayerRunCmd`, and MovementAPI's post-think bookkeeping (turning
  flags from the eye yaw of consecutive commands, whether the command walk-moved).
- `on_jump`: `Movement_OnJumpPre` (perf handling). `on_categorize`: MovementAPI's categorize-position hook
  (landing command and velocity, takeoff speed, GOKZ's slope fix).

Timing details that decide bit-exactness, all checked against captures: GOKZ reads the *previous*
command's buttons (`m_nButtons`) and turning flags in `OnPlayerRunCmd`; SimpleKZ's turn rate compares
this command's yaw with the eye yaw stored at the end of the previous `OnPlayerRunCmd`.

## Vanilla (S7)
Config only, no hooks. Values: [Ref §3] and the Vanilla column of [Ref §20].

## KZTimer (S8)
Config: accelerate 6.5, airaccelerate 100, friction 5.0, no weapon-speed scaling, no stamina, stock
anti-bhop off, ladder scale 1.0, velocity component limit 2000, ledge helper off [Ref §20]. 128 tick only
(GOKZ refuses to load otherwise; the app locks the tickrate).

Ported: the prestrafe velocity modifier (`CalcPrestrafeVelMod`, adapted in GOKZ from KZTimerGlobal: up
to 1.104, increments of 0.0009/0.001 while turning with a strafe key above 248.9 u/s, decay after 75
commands, reset 0.2 s after turning stops); perfect-hop cap 380 (`TweakJump`, perf = a jump after a
command that didn't walk-move); crouch-jump bind removal; duck speed restored to 8 on releasing duck;
`CanUnduck` after landing; slope fix.

## SimpleKZ (S9)
Config: as KZTimer except friction 5.2 and velocity component limit 3500 [Ref §20]. 128 tick only.

Ported: the turn-rate prestrafe bonus (up to 26.54321 u over 250, earned at up to 90 degrees per second
of turning with movement keys, 3 grace commands, lost at 0.2824 u per command in the air); perfect hops
within 1 command of landing, or within 3 with jump held recently (`TweakJump`): the takeoff uses the
landing direction at `min(V, (0.2 V + 200) M_landing)` [Ref §20.2], moved onto the ground, and the
prestrafe held at landing is restored; duck speed floors (8 outside a transition, 6.0234375 within);
crouch-jump bind removal; `CanUnduck` after landing; slope fix.

## Not ported (both KZ modes)
- `FixWaterBoost` (no water) and `FixDisplacementStuck` (needs the post-unduck stuck test).
- KZTimer's 0.2 s reset uses `GetEngineTime()` (wall clock); here it is one tick interval per command.
- The slope fix uses the plane categorization grounded the player on; GOKZ traces the ducked hull
  straight down for it. No capture lands on a slope in a KZ mode yet.
- GOKZ core features that don't change movement (timer, jumpstats bookkeeping, HUD).

## Evidence
Real-server captures at 128 tick (`docs/verification.md`): prestrafe while turning (`S20-a/b`) is
WITHIN_EPS in both modes, bit-exact in every horizontal column (one landing `origin_z` tick differs by
2.4e-7, the open landing difference); jumps, perfect bhops and long jumps on the test level and on
kz_longjumps_v4096 are BIT_EXACT or WITHIN_EPS.
