# Game plan: CS:GO movement in Rust + Bevy

Execution plan. The physics, constants, formulas and bug explanations live in
`CSGO-Movement-Technical-Reference.md`; this file points to its sections as **[Ref §N]** and does not repeat them.

## 1. Vision

A from-scratch reimplementation of CS:GO (Source 1) player movement in Rust, with:

- **Switchable modes at runtime:** GOKZ Vanilla, GOKZ KZTimer, GOKZ SimpleKZ [Ref §20].
- **A small test level** with the geometry needed to exercise every major technique.
- **A simple HUD** showing the stats a KZ/movement player reads in the real game, so behavior can be compared
  against the real thing.
- **Bevy** as the host for window, input, rendering and UI. Bevy is not the physics engine.

### Non-goals (for the vision above)

- Networking, prediction, lag compensation, shooting, weapons, hitboxes.
- Water, moving platforms, and conveyors (extend later; [Ref §25] lists them as evidence gaps anyway).
- CS2 behavior [Ref §25].
- Importing real CS:GO maps. BSP loading is a stretch milestone.
- Byte-exact equivalence with every retail build. The target is "matches the documented implementation and
  measured behavior within tight tolerances". See §9.

## 2. Decisions already made

| Decision | Rationale |
|---|---|
| Movement is a pure Rust crate with no Bevy dependency | Testable headless, deterministic, replayable, and unaffected by Bevy API churn |
| Bevy runs the sim from `FixedUpdate` at 64 or 128 Hz | Movement must run at the tickrate, not the render rate [Ref §2] |
| Do not use Avian/Rapier/other physics or character-controller crates for movement | Their depenetration, sliding and snapping fight Source's `TryPlayerMove`/`CategorizePosition`/`StepMove` [Ref §12] |
| Custom swept-AABB trace behind a `TraceWorld` trait | Lets us swap primitive brushes for BSP without touching movement code |
| `f32` everywhere in the sim, with no `f64`, no FMA, and no reordered math | Sub-unit thresholds (jumpbug, edgebug, jump height varying with origin [Ref §9, §14, §15]) depend on float behavior |
| Primitive-brush world first, BSP later | Validate math and state ordering before taking on a file format |
| Modes are data plus hooks over one core, not separate controllers | KZ modes are the vanilla controller plus parameter and hook changes [Ref §20] |

## 3. Current state

Toolchain (M0) is done: Rust 1.99 and MSVC Build Tools are installed, and `cargo test` is green.

`crates/movement` implements the pipeline in §5.3 with one function per Source routine: `check_parameters`,
`reduce_timers`, `duck` (with `finish_duck` / `finish_unduck` / `can_unduck`), `ladder_move`,
`full_walk_move`, `check_jump_button` / `prevent_bunny_jumping`, `friction`, CS `accelerate`, `air_accelerate`,
`walk_move` with the total-speed clamp, `air_move`, `try_player_move`, `step_move`, `stay_on_ground`,
`categorize_position` (with quadrant probes and deadstrafe friction), `set_ground_entity`, `check_falling`.
Also: primitive brush world with a Source-style swept-AABB trace, the observer and technique detector,
three modes, jump stats, a command DSL, and bit-exact record/replay.

`crates/app` is the Bevy 0.19.1 host (§6, §7): fixed-tick sim with input latching and a catch-up cap,
interpolated camera, the §6.3 test level from one description, HUD, debug draw, hotkeys, recording, and a
`--check` headless replay.

Milestone gates (automated parts) pass for M1–M8; see `README.md` for the per-subsystem status.

The §9 rig is built and has been run (V0–V7): `tools/capture` (server plugin, setup and run scripts),
`tools/compare` (map and scenario export, import, diff, reports, promotion), the in-app ghost overlay, and
about 160 vanilla captures at 64 and 128 tick on CS:GO 1.38.8.1, with 42 promoted to regression tests.
Results are in `README.md` and `docs/verification.md`; the mismatches they exposed were fixed and are
recorded in `docs/divergences.md`. Not done: M9 and M10. The GOKZ mode hooks are designs from [Ref §20], not ports (see
`docs/modes-notes.md`). Unverified choices are listed in `docs/divergences.md`.

## 4. Environment setup (do first)

1. Install **Visual Studio Build Tools**, workload "Desktop development with C++". Needed for tests and for Bevy.
2. Run cargo from **PowerShell**, not Git Bash. Git Bash puts GNU `link` ahead of MSVC `link.exe` on `PATH` and
   linking fails with "missing operand".
3. Installed Rust was 1.85.1. Before adding Bevy, check the latest Bevy release's MSRV against `rustc` and run
   `rustup update` if needed. Pin the exact Bevy version in `Cargo.toml` and do not float it.
4. Optional but recommended for dev builds: set `[profile.dev] opt-level = 1` and
   `[profile.dev.package."*"] opt-level = 3` in the workspace manifest. This keeps Bevy usable in debug builds.
   Do not change float settings for the `movement` crate. Check any profile-wide setting that could enable
   fast-math-like behavior. Stable Rust does not do this by default, but verify that nothing is turned on.
5. `cargo test` passes on the existing four tests. This is the gate before anything else.

## 5. Architecture

### 5.1 Workspace layout

```
CSMovementRust/
  Cargo.toml                  workspace
  game-plan.md
  CSGO-Movement-Technical-Reference.md
  crates/
    movement/                 pure Rust, no Bevy; the whole simulation
      src/
        lib.rs
        math.rs               Vec3, angle helpers (angle_vectors), plane math
        config.rs             MovementConfig + mode presets
        cmd.rs                UserCmd, Buttons (bitflags by hand or `bitflags` crate)
        state.rs              PlayerState, DuckState, GroundState
        pipeline.rs           process_movement + FullWalkMove ordering
        physics.rs            friction, accelerate, air_accelerate, clip_velocity
        stamina.rs
        duck.rs               duck / unduck / can_unduck / hull origin shifts
        jump.rs               check_jump_button, bhop rules, prevent_bunny_jumping
        categorize.rs         categorize_position, set_ground_entity, check_falling
        collide.rs            try_player_move, step_move, multi-plane clip
        ladder.rs
        trace.rs              TraceWorld trait, Hull, TraceResult
        world/
          primitive.rs        Brush (convex planes), Aabb helper, ramps, ladder volumes
          bsp.rs              (stretch) BSP loader + brush trace
        modes/
          mod.rs              MovementMode trait, ModeKind enum
          vanilla.rs
          kztimer.rs
          simplekz.rs
        instrument.rs         MoveObserver trait + event types (see 5.5)
        replay.rs             record/replay of UserCmd streams
    app/                      Bevy binary
      src/
        main.rs
        input.rs              keyboard/mouse -> view angles + buttons -> UserCmd
        sim.rs                FixedUpdate system calling movement crate
        level.rs              builds the primitive world AND spawns matching meshes from one description
        camera.rs             first-person camera, render interpolation
        hud.rs                stats overlay
        modes.rs              mode switching UI/hotkeys
        record.rs             CSV/replay export
    tools/                    compare CLI: import real-server captures, replay, diff (see §9.6)
  sourcemod/
    csmove_capture.sp         real-server input injection + logging plugin (see §9.4)
  data/
    levels/                   level descriptions (RON or Rust consts)
    captures/                 real-server captures used as ground truth (see §9.3)
  docs/
    divergences.md            resolved mismatches vs the real game (see §9.7)
```

### 5.2 Core types (movement crate)

- `Vec3` (f32). Keep math explicit and unfused. Add only what's needed.
- `UserCmd { tick: u32, view_angles: Vec3 (pitch,yaw,roll), forward_move: f32, side_move: f32, up_move: f32, buttons: Buttons }`.
  Buttons: `JUMP, DUCK, FORWARD, BACK, LEFT, RIGHT, WALK, USE`. Retain `old_buttons` in state, not in the cmd.
  Movement inputs are +/-450 (Source convention; confirm in S12) and are produced from keys.
- `PlayerState` holds everything in [Ref §1]'s table: origin, velocity, base_velocity, ground_entity
  (`Option<EntityId>`), flags, buttons/old_buttons, max_speed, surface_friction, stamina, duck_amount, duck_speed,
  duck/transition flags, fall_velocity, ladder state (normal, attached, jump-ignore timer), timers, and `tick`.
- `MoveData`: the per-tick working copy (outwish, maxspeed, wish vector) as in Source's `CMoveData`.
- `Hull { mins, maxs }` with `STAND = (-16,-16,0)..(16,16,72)` and `DUCK = ..54` [Ref §1]. Axis-aligned always.
- `MovementConfig`: every constant in [Ref §3] plus the mode-specific knobs in [Ref §20] (weapon-speed scaling flag,
  anti-bhop flag, ladder scale, velocity component limit, ledge helper flag, and so on).
- `PlayerState` and `UserCmd` implement `Clone + PartialEq + Debug`, and serialize through `serde` behind a feature
  flag, so replay and golden tests are cheap.

### 5.3 Tick pipeline

Implement the order in [Ref §2] literally, one function per Source routine so that names map 1:1 to the source and
to the instrumentation points in [Ref §22]. Public entry point:

```rust
pub fn process_movement(
    cfg: &MovementConfig,
    mode: &mut dyn MovementMode,
    world: &impl TraceWorld,
    state: &mut PlayerState,
    cmd: &UserCmd,
    obs: &mut dyn MoveObserver,
    dt: f32,
)
```

Rules for the port:

- Keep Source's routine boundaries and their call order even where it looks redundant. Order is the behavior
  [Ref §29 closing, §7 deadstrafe phase, §8 stamina sampling].
- Stored state that crosses commands (surface friction, fall velocity, duck speed, stamina) is a field on
  `PlayerState`, never a local.
- No hidden globals. The mode hooks and observer are passed in.

### 5.4 Trace layer

```rust
pub struct TraceResult {
    pub fraction: f32,        // 1.0 = no hit
    pub end_pos: Vec3,
    pub plane_normal: Vec3,
    pub start_solid: bool,
    pub all_solid: bool,
    pub hit_entity: Option<EntityId>,
}
pub trait TraceWorld {
    fn trace_hull(&self, start: Vec3, end: Vec3, hull: Hull) -> TraceResult;
    fn point_solid(&self, p: Vec3, hull: Hull) -> bool;      // for stuck checks / can_unduck
    fn ladder_at(&self, p: Vec3, hull: Hull) -> Option<Ladder>;
    fn surface_friction_at(&self, e: EntityId) -> f32;       // default 1.0
}
```

**Primitive world** (`world/primitive.rs`): a `Vec<Brush>`, where a brush is a convex solid given as bounding planes.
Trace by expanding each plane by the hull support distance (Minkowski) and clipping the segment against the plane
set, which is the same algorithm BSP brush tracing uses per brush. Provide constructors for box, wedge/ramp (any
angle), and stairs.

- Implement the Source-style epsilon behavior (`DIST_EPSILON`-style pull-back) on the exit/entry fraction.
  Read the official Source SDK 2013 trace code for this (§10 on provenance).
- A BVH is not needed. A test level has maybe 100 brushes. Linear scan is fine until BSP.

**BSP backend** (stretch, M9): parse lumps (planes, brushes, brushsides, nodes, leafs, leafbrushes, models), trace by
walking the node tree to candidate brushes, then reuse the same per-brush clipping routine from the primitive
world. Displacements are a separate sub-project (needed for realistic surf ramps).

### 5.5 Instrumentation

`MoveObserver` is a trait with default no-op methods, called at each point listed in [Ref §22]:
`on_cmd_start`, `on_jump_button`, `on_duck`, `on_air_accelerate`, `on_walk_move`, `on_try_player_move` (per bump,
with the trace result), `on_categorize`, `on_set_ground`, `on_landing`, `on_cmd_end`. Each event carries the
before/after origin and velocity. Uses:

1. Detecting jumpbug, edgebug, duckbug and distbug-affected landings from internal events, as MovementAPI does
   [Ref §14–16].
2. HUD stats (§7).
3. Trace dump for the comparison tooling (§9).

Observer calls must be zero-cost when unused. Use generics or a `NullObserver`, and don't allocate per tick.

### 5.6 Mode system

```rust
pub trait MovementMode {
    fn kind(&self) -> ModeKind;
    fn config(&self) -> &MovementConfig;
    fn pre_command(&mut self, state: &mut PlayerState, cmd: &UserCmd) {}
    fn post_command(&mut self, state: &mut PlayerState, cmd: &UserCmd) {}
    fn on_jump(&mut self, state: &mut PlayerState, ground_speed: f32) {}   // perf detection / takeoff adjust
    fn on_land(&mut self, state: &mut PlayerState) {}
    fn modify_wish_speed(&self, state: &PlayerState, base: f32) -> f32 { base }
    // add more only when a concrete mode needs it
}
```

- **Vanilla:** all hooks no-op. Config only.
- **KZTimer:** prestrafe modifier, perfect-hop cap, simultaneous jump+duck suppression, duck-state and displacement
  tweaks [Ref §20.1].
- **SimpleKZ:** 128 tick enforced, turning-based prestrafe bonus with growth/decay/grace, modified perf handling,
  landing-speed-dependent takeoff adjustment [Ref §20.2].
- The *exact* algorithms for these hooks are not in the reference; they live in the GOKZ source
  (S7, S8, S9). Task for M7: read those files and write a short `modes-notes.md` per mode before coding. Port the
  logic against the behavior descriptions, not by copying text (§10).
- Mode changes take effect only between commands. Switching resets mode-private state, and optionally the player.

## 6. Bevy application

### 6.1 Timing and input

- `FixedUpdate` at the mode's tickrate: 64 Hz for Vanilla and KZTimer (selectable 64 or 128), 128 Hz forced for
  SimpleKZ [Ref §20.2]. Set `Time::<Fixed>` from the mode. Changing the rate means changing the resource, not
  rebuilding the app.
- `Update` (render rate): read mouse motion and accumulate into `ViewAngles` (yaw, clamped pitch) using a
  sensitivity and `m_yaw` equivalent (0.022 deg/count, to confirm in S12). Read key state into a `HeldInput`
  resource.
- `FixedUpdate`, once per tick: build one `UserCmd` from the *current* view angles and held keys, then run
  `process_movement`. This matches how input timing at a given FPS shapes which command a press lands in
  [Ref §2]. Do not integrate with `Time::delta`.
- Store the `UserCmd` in a ring buffer for replay export.
- **Input-per-tick contract:** `UserCmd` is the only way input reaches the sim, and it is built once per tick.
  `Update` must latch edge events (a press and release that both land between two ticks, as with a scroll-wheel
  jump) and hand them to the next tick, so no press is dropped or doubled. Clear the latches after the tick
  consumes them.
- **Source of truth:** `PlayerState` is authoritative. Bevy `Transform` is a render copy written from the
  interpolated state, and nothing reads position or velocity back from it.
- **Catch-up cap:** after a frame stall, `FixedUpdate` would run many ticks in a row. Cap the ticks per frame
  (for example 4) and discard the excess accumulated time, so the sim never spirals. A discarded-time event
  should be logged because it invalidates any capture comparison for that run.
- Jump on scroll wheel: map wheel events to a jump press and release inside the same frame window. Required for
  vanilla-style bhop testing. Also provide held-jump with an autohop toggle (needed in KZTimer and SimpleKZ runs).

### 6.2 Rendering and camera

- Camera at eye height. Standing view offset 64 and crouched 46 (to confirm against S3/S21).
- **Render interpolation:** keep previous and current `PlayerState` origin, and interpolate by the fixed-timestep
  overstep fraction for the camera, so 64 Hz sim stays smooth at high FPS. Interpolate display only. Never feed
  it back.
- Level rendering: simple unlit/flat-shaded meshes with a grid texture or per-surface debug colors, for distance
  judgment. Walkable (normal.z >= 0.7) and non-walkable (surfable) faces get different colors.
- Optional debug draw: hull wireframe, the last N trace segments with plane normals, and the last collision
  planes. This is the most useful way to debug `TryPlayerMove`.

### 6.3 Level

`level.rs` defines the level once as a list of brush descriptions, from which it builds both the primitive-world
brushes and the Bevy meshes. They can never disagree. Contents:

| Area | Purpose |
|---|---|
| Large flat floor with 64-unit grid markings | Friction, acceleration, counter-strafe, prestrafe |
| Long-jump runway with landing pads at gaps from about 220 to 290 units, in steps | Long jump distance/sync comparison |
| Bhop blocks in a row, spacing and height variants | Bhop, perfs, stamina, anti-bhop cap |
| Ledges at heights 18, 54, 55, 56, 57, 58 | Jump and crouch-jump clearance [Ref §9, §10] |
| Stairs, 18-unit and 16-unit risers | Step logic [Ref §12.3] |
| Free-standing wall and a corridor | Wall strafe, slide clipping [Ref §12, §17] |
| Ramps at 30, 40, 45, 46, 50, 60, 70 degrees with landing space | Surf, slope boost, grounding threshold [Ref §13] |
| Floating platform with a sharp edge | Edgebug (M6) [Ref §14] |
| Drop platforms at set heights (about 8 to 12 units) | Duckbug / jumpbug (M6) [Ref §15] |
| Ladder volume | Ladder movement (M6) [Ref §18] |
| Thin floor trigger and a teleport trigger | Telehop/trigger ordering (stretch) [Ref §21] |

Put a labeled sign (3D text or a minimal overlay marker) at each area.

### 6.4 Controls

Hotkeys: switch mode (F1 Vanilla, F2 KZTimer, F3 SimpleKZ), toggle 64/128 tick (where allowed), reset to spawn (R),
checkpoint save/load, toggle autohop, toggle debug draw, start/stop recording (F5), and teleport-to-area shortcuts.
Pause and single-step the sim (period key) to inspect a tick.

## 7. HUD

Minimal, text only, monospace, top-left and optionally center-bottom for speed. The stats mirror what KZ plugins and
server HUDs report, so they can be compared to the real game.

**Always on:**

- Mode, tickrate, FPS.
- Horizontal speed (to 2 decimals) and vertical velocity. A "pre" speed (last takeoff speed) next to it.
- Position (x, y, z), view angles.
- Grounded flag, ground entity, duck amount, surface-friction factor in use, stamina value.

**Per-jump (shown after landing):**

- Jump type (LJ, bhop, multi-bhop, ladder jump, weird jump, and so on) using GOKZ-style classification, with the
  classification logic documented in `jumpstats-notes.md` (from S15). Start with LJ/bhop/other.
- Distance with the +32 reporting convention [Ref §16], and the same number without it.
- Pre and max speed, takeoff speed, height gained, airtime (ticks), strafe count, **sync %**, per-strafe gain,
  perfect/not-perfect on takeoff, and an edgebug/jumpbug/duckbug flag from internal events.
- Landing-origin correction following the distbug notes [Ref §16]. Show the raw final-command origin too, to see
  the difference.

**Debug (toggle):** wish direction, wish speed, last acceleration budget, last command's trace results, per-tick
speed graph (a simple line over the last ~2 seconds).

Export: F5 writes a CSV, one row per tick (columns in §9.2), plus the `UserCmd` stream.

## 8. Milestones

Each milestone has an acceptance check. Do not start the next until the gate passes. "Gate" means automated tests
unless noted.

### M0: Toolchain
Install MSVC Build Tools, confirm PowerShell workflow, update Rust, `cargo test` passes on existing tests.
Gate: green `cargo test`.

### M1: Core types and grounded movement on a flat floor (no Bevy)
- `UserCmd`, `PlayerState`, `Buttons`, `Hull`, angle-to-vector conversion, wish-velocity construction [Ref §4].
- Full ground acceleration branch including the weapon-scaling/duck/walk logic in [Ref §5.2] (single weapon speed
  of 250 for now) and the ground total-speed clamp [Ref §5.3].
- `TraceWorld` trait and a primitive world with one infinite floor plane.
- Gate: tests for friction/accel numbers from [Ref §5]; accelerate-from-rest reaches 250 at a stable tick count
  (record this as a golden value); counter-strafe stops within the tick count the model predicts.

### M1.5: Capture rig (V0 + V1 in §9.10)
- Confirm a CS:GO build, local server, SourceMod and MovementAPI run (V0). Write `csmove_capture.sp`, the capture
  format importer, and `tools/compare` (V1).
- Gate: scenarios S0 and S1 captured from the real server, imported, and compared end to end, with a
  reproducibility check (same scenario twice gives identical logs). If V0 fails, switch to §9.8 and revise claims.

### M2: Air movement, gravity, jump, stamina
- Split gravity (`start_gravity`/`finish_gravity`), air accelerate with surface-friction state, jump branch
  selection, stamina update ordering, `check_falling` landing penalty, anti-bhop cap [Ref §8, §9, §11].
- Gate: jump apex tests reproducing [Ref §9]'s table (54.653766 / 55.825641 standing; 56.997516 duck/reset), stamina
  cost near 24 after a no-stamina takeoff [Ref §8], a scripted perfect-bhop chain keeps speed, an imperfect hop gets
  friction, deadstrafe budget one quarter in the 0 < vz <= 140 band [Ref §7], and the 3D anti-bhop cap applies at
  takeoff [Ref §11.2].

### M3: Real collision
- Convex-brush hull trace, `try_player_move` (4 bumps, 5 planes), `clip_velocity`, `step_move`, ground
  classification and `categorize_position` with the 2-unit probe and grounded step probe [Ref §12].
- Gate: tests for sliding along a wall, along a 2-plane crease, stepping onto an 18-unit stair but not a 19-unit
  one, standing on a slope below the 0.7 threshold, and sliding off one above it. A "no tunneling" property
  test: random start, velocity and boxes, and the player never ends up inside a brush.

### M4: Duck, unduck, crouch jump
- Duck amount/speed/flags, grounded vs airborne hull origin handling, `can_unduck` traces, duck speed penalty and
  fatigue, jump with the duck/reset branch [Ref §10].
- Gate: crouch-jump heights vs [Ref §10.2] cases, the 9-unit shift test, unduck blocked under a low ceiling.

### M5: Bevy shell, test level, basic HUD (first playable)
- `app` crate: windowing, input to `UserCmd`, `FixedUpdate` sim, camera with render interpolation, level from the
  single description, HUD "always on" stats, mode switching skeleton, reset-to-spawn.
- Gate (manual): you can run, strafe-jump, bhop with scroll, climb stairs, surf a ramp; HUD speed matches an
  offline run of the same recorded `UserCmd` stream through the movement crate (headless replay equals live
  state, bit for bit). Add this replay equality as an automated test in the `movement` crate.

### M6: Techniques that depend on internal ordering
- Observer events, edgebug/duckbug/jumpbug detection, edgebug/jumpbug reproduction in tests using the level's
  drop and ledge platforms [Ref §14, §15], ladders with hysteresis and the jump-ignore interval [Ref §18].
- Gate: scripted command sequences that produce a jumpbug and an edgebug on the test geometry, plus detection
  events firing on exactly those commands.

### M7: Modes
- Per-mode notes from S7-S9, then implement `MovementMode` hooks, then presets: Vanilla -> KZTimer -> SimpleKZ.
- Runtime switching in the app. SimpleKZ locks the tickrate to 128.
- Gate: unit tests per mode against the config table in [Ref §20] and the specific numbers (prestrafe cap of
  276, perf cap of 380, SimpleKZ takeoff formula), plus mode-specific bhop scripts.

### M8: Jump stats and comparison tooling
- Jump classification, distance with correction and +32 convention, sync and per-strafe stats, HUD per-jump
  panel, CSV export, and the in-app ghost overlay (§9.6).
- Gate: S19 long-jump captures meet the §9.9 criteria, and the status table in `README.md` is filled in.

### M9 (stretch): BSP and real maps
- BSP v20 parser, brush trace via the shared brush-clipping routine, entity lumps for ladders/triggers/teleports,
  displacement collision, then run community surf and KZ maps.
- Gate: a known KZ map's long jump block distances match the in-game jumpstats for the same recorded input.

### M10 (stretch): Triggers and telehops
- Thin triggers, ground-probe-vs-overlap behavior, RNGFix-style telehop and slope behaviors as optional toggles
  [Ref §13.3, §21].

## 9. Verifying closeness to the original CS:GO

The reference doc states it ran no retail binary [Ref intro, §22], so every number in it is derived, not
measured. This section is the plan for measuring the real game and comparing it directly with this project.

### 9.1 Principle: same inputs in, same state out

The comparison is only meaningful if both simulations get the identical command stream, start state and
collision geometry. So the rig has four parts:

1. **Input injection on the real server.** A SourceMod plugin overrides the player's commands with a recorded
   stream, so a human never has to reproduce timing.
2. **Ground-truth logging on the real server.** Per tick, plus internal events.
3. **An importer in this project** that reads the log and the command stream.
4. **A comparator** that replays the same commands through the `movement` crate and reports the first
   divergence.

Server-side state is the truth. Client-side rendering, prediction and demo files are secondary sources (§9.8).

### 9.2 Tool inventory

| Tool | Role | Notes |
|---|---|---|
| A runnable CS:GO install (the "csgo_legacy" beta branch in Steam's CS2 properties, or an old install) | The thing being measured | **Verify this exists on your account first (task V0).** If it doesn't, drop to §9.8 and lower the claims |
| CS:GO dedicated server (SteamCMD app 740) or a listen server | Authoritative simulation | A local LAN server with `sv_lan 1` is enough. Pin the build number in every capture |
| MetaMod:Source + SourceMod | Plugin host | Use the versions supported by the server build |
| MovementAPI [S5] | Hooks around movement functions, and ground/duck/jump event forwards | Gives pre/post-movement callbacks and landing-origin helpers, so use it instead of hooking by hand |
| GOKZ [S6-S9] (optional) | Provides the real KZTimer and SimpleKZ modes to capture | Only needed for M7. Vanilla captures need no GOKZ, just MovementAPI. Pin the commit |
| Our own plugin `csmove_capture.sp` (to write) | Input injection, logging, scenario control | Spec in §9.4 |
| `cl_showpos 1`, `net_graph 1`, `sv_showimpacts` | Quick client-side eyeballing only | Rounded, so never use for comparison numbers |
| A CS:GO map with simple geometry on integer coordinates | Shared collision world | Build in Hammer (CS:GO authoring tools) or reuse an existing flat KZ map. Mirror the geometry into the test level by hand, and record the mirror in the scenario file |
| Demo parsers (demoinfocs-golang, awpy) | Secondary source for community runs | Only for the coarse checks in §9.8 |
| Rust: `tools/compare` CLI, plus the in-app ghost overlay | Importer, comparator, reporting | §9.6 and §9.7 |

### 9.3 Capture file format (version 1)

One capture is a folder `data/captures/<name>/` containing:

- `meta.toml`: game build number, map name and BSP hash, server tickrate, mode (vanilla/kztimer/simplekz), MetaMod,
  SourceMod, MovementAPI and GOKZ versions or commits, plugin commit, and a **full dump of movement-relevant
  cvars** (`sv_gravity`, `sv_friction`, `sv_accelerate`, `sv_airaccelerate`, `sv_stopspeed`, `sv_maxspeed`,
  `sv_maxvelocity`, `sv_stepsize`, `sv_enablebunnyhopping`, `sv_staminamax`, `sv_staminajumpcost`,
  `sv_staminalandcost`, `sv_staminarecoveryrate`, `sv_ladder_*`, `sv_timebetweenducks` and so on; list the exact set when
  writing the plugin by dumping every `sv_*` movement cvar), plus the active weapon and its max speed.
- `cmds.csv`: the injected stream, one row per tick: `tick, forward_move, side_move, up_move, buttons,
  pitch, yaw, roll, impulse`.
- `states.csv`: ground truth, one row per tick (columns below).
- `events.csv`: internal events, one row per event: `tick, seq, kind, payload...` with kinds such as
  `jump_button`, `duck`, `categorize`, `set_ground`, `landing`, `bump` (with plane normal and fraction if
  obtainable), `teleport`.
- `scenario.toml`: start origin, start velocity, start view angles, start ground/duck/stamina state, weapon, and
  geometry description used.

**Float handling.** Log every float twice: a decimal with 9 significant digits and the raw 32-bit pattern as hex
(`view_as<int>(f)` in SourcePawn). The Rust importer reads the bits, so there is no text-rounding loss, and
a divergence can be classified as "exact", "1 ULP off", or "real".

`states.csv` columns: `tick, sim_time, origin_xyz, velocity_xyz, base_velocity_xyz, eye_angles, ground_entity,
flags, move_type, duck_amount, duck_speed, stamina, surface_friction, max_speed, fall_velocity, ladder_state`
(use whichever of these the engine exposes through netprops or MovementAPI; mark unavailable columns as empty
rather than faking them, and list them in `meta.toml`).

Log **both** a pre-command and a post-command row per tick, since the first divergence often shows up as a wrong
*start* state for the next tick caused by stored state (stamina, surface friction, duck speed).

### 9.4 The capture plugin (`csmove_capture.sp`)

Responsibilities:

1. **Scenario setup.** On a console command or round start: teleport the client to the scenario origin with the
   given velocity and angles (`TeleportEntity`), strip weapons except the specified one, set stamina and duck state
   as far as the engine allows, and wait a few ticks for the player to settle. Record the settled state as the
   authoritative start and write it to `scenario.toml`. Our sim starts from that recorded state, not from the
   requested one.
2. **Input override.** In `OnPlayerRunCmd`, replace `buttons`, `vel[]` and `angles[]` with the next row of
   `cmds.csv`. Take the command rows from a file that the Rust side wrote, so one input script drives both
   simulations. Use the same tick index on both sides.
3. **Logging.** In `OnPlayerRunCmd` (pre) and a MovementAPI post-move forward, write the rows from §9.3. Buffer in
   memory and flush at the end, because per-tick file writes can hitch the server.
4. **Events.** Use MovementAPI's forwards for jump, duck, landing, and ground change. Where finer routine-level
   hooks are needed (for example around `TryPlayerMove` bumps or `CategorizePosition`), use DHooks detours the way
   MovementAPI and RNGFix do [S5, S13]. This is optional and costs the most effort. Add it only when a
   divergence cannot be explained from pre/post rows.
5. **Control.** Commands: `csmove_run <scenario>`, `csmove_stop`, `csmove_dump`. Keep the plugin stateless between
   runs.

Open item for V1: confirm that a plugin-overridden command goes through the same movement code path as a human
command, with the same `usercmd` fields, and confirm what happens to commands the server drops. [Ref §2] notes
that an input hook doesn't always correspond one-to-one to a simulated command. If a drop is detected (command
number gap in the post-move log), discard the run rather than trying to explain it. Run on a quiet LAN server
with no other load, and reject runs with gaps automatically.

### 9.5 Scenario ladder

Each rung isolates one subsystem so a mismatch points at a small piece of code. Each scenario is a small script
for both simulations. Run each at 64 and 128 tick, and in each relevant mode.

| ID | Scenario | Isolates | Compare | Milestone |
|---|---|---|---|---|
| S0 | Stand still 100 ticks | Settling, gravity at rest, ground snap | Origin z exact | M1 |
| S1 | Accelerate forward from rest on flat ground, then release | Ground acceleration, friction, stop-speed region [Ref §5] | Velocity each tick | M1 |
| S2 | Run, then counter-strafe | Opposite-direction acceleration | Ticks to stop, velocity | M1 |
| S3 | Walk (shift) and crouch-walk variants | Walk/duck multipliers, acceleration scaling | Velocity | M1/M4 |
| S4 | Single standing jump, no input | Jump branch, split gravity, stamina cost [Ref §8, §9] | Z each tick, apex, stamina | M2 |
| S5 | Jump with strafe input, fixed yaw rate | Air acceleration, deadstrafe window [Ref §6, §7] | Velocity, surface_friction | M2 |
| S6 | Jump with *optimal* yaw sequence generated from our own model | Max-gain strafing | Speed gain per tick | M2 |
| S7 | Chain of 10 perfect bhops, flat ground | Perfect landing command, friction skip, stamina, anti-bhop cap [Ref §11] | Takeoff and landing speed, stamina | M2 |
| S8 | Same with a deliberate 1-tick-late jump | Friction/clamp on imperfect landing | Velocity | M2 |
| S9 | Fall from several heights, no input | Landing stamina, `check_falling`, fall velocity | Stamina, velocity z | M2 |
| S10 | Walk into a wall at several angles | Plane clipping [Ref §12.2] | Velocity, origin | M3 |
| S11 | Into a two-wall crease | Multi-plane clip | Velocity | M3 |
| S12 | Stairs at 16, 18 and 19 unit risers | `step_move` | Origin, velocity | M3 |
| S13 | Slope at 30, 40, 44, 46, 50, 60 degrees: walk onto, slide, jump onto | Walkable threshold, ground classification [Ref §12.4, §13] | Ground flag, velocity | M3 |
| S14 | Ledges at 54, 55, 56, 57, 58: jump standing, crouch-jump and begin-duck-jump | Jump heights, branches [Ref §9, §10] | Whether the ledge is reached, z trace | M4 |
| S15 | Duck/unduck on ground and in air, under a low ceiling | Hull shifts, `can_unduck`, duck speed penalty | Origin z, flags, duck_speed | M4 |
| S16 | Free fall past a ledge: edgebug input sequence | Edgebug ordering [Ref §14] | Event list, final state | M6 |
| S17 | Drop-and-unduck sequence | Duckbug and jumpbug [Ref §15] | Event list, velocity z | M6 |
| S18 | Ladder grab, climb at several view pitches, detach, jump off | Ladder math [Ref §18] | Velocity, origin | M6 |
| S19 | Long jump at the same gaps as the test level | End-to-end jump distance [Ref §16] | Landing origin | M8 |
| M-* | S4, S5, S7, S19 repeated under GOKZ KZTimer and SimpleKZ | Mode hooks [Ref §20] | Same | M7 |

Generate input scripts programmatically (from the command DSL in §11) where a fixed pattern suffices, and
record from human play, through the app's recorder, for the realistic cases. Both kinds end up as
`cmds.csv`.

Start every scenario at a **fixed origin that is identical in the real map and the test level.** Include one
repeat at a second origin far from the world origin, because jump height was documented to vary with
distance from the map origin [Ref §9 end].

### 9.6 Importing and comparing in this project

`tools/compare` (Rust, using the `movement` crate):

1. **Load** the capture folder. Validate: columns, tick contiguity, no dropped commands, matching build metadata.
2. **Build** the state from the recorded start state and the mode from `meta.toml`; set `MovementConfig` from the
   recorded cvar dump (so a server running different values still compares fairly).
3. **Replay** `cmds.csv` through `process_movement` with an observer collecting our own events and per-tick rows.
4. **Diff**, tick by tick, column by column, comparing raw bit patterns first:
   - `EXACT`: identical bits.
   - `ULP(n)`: within n units in the last place. Report n.
   - `CLOSE`: within the configured epsilon (default 1e-3 units, or 1e-3 units/s).
   - `DIVERGED`: beyond.
5. **Report**:
   - The first `DIVERGED` tick and the column that diverged first, with both values, plus the previous tick's
     state, the command, and any events either side logged in a window of +/- 3 ticks.
   - Count of EXACT/ULP/CLOSE/DIVERGED per column.
   - Max error per column, and the tick it occurred.
   - A one-line verdict: `BIT_EXACT`, `WITHIN_ULP`, `WITHIN_EPS`, or `FAILED @ tick N`.
6. **Re-sync mode (diagnostic):** optionally overwrite our state with the captured state at each tick start and
   diff only one tick forward. This separates "wrong formula" (fails even when re-synced) from "accumulated
   drift" (passes re-synced but fails free-running). Always run both.
7. Output as text, plus a CSV of residuals, plus an HTML or PNG plot of speed, z, and error over ticks for quick looks.

Exit code is non-zero on `FAILED`, so the compare tool can run in CI over every validated capture.

**In-app ghost overlay.** The Bevy app can load a capture and show it alongside the live sim: a ghost hull
drawn at the captured origin, with the HUD showing our value, the captured value and the delta for speed,
origin and stamina. Use it for visual debugging only, since the CLI is the source of truth.

**Promotion to regression tests.** A capture that reaches `WITHIN_EPS` or better is copied into
`crates/movement/tests/captures/`, with its tolerances in a sidecar file, and replayed by `cargo test`. From then
on it guards every change. Keep captures small (one scenario each, a few hundred ticks).

### 9.7 Diagnosing a divergence

Work in this order, and stop when found:

1. **Reproducibility check.** Capture the same scenario twice on the server. If the two differ, the server isn't
   deterministic for that scenario (dropped commands, timing) and the problem is the rig. Fix the rig first.
2. **Setup check.** Compare tick 0 start states and the cvar dump. A mismatch in max speed, weapon, stamina,
   ground entity, or surface friction at tick 0 explains most early divergences.
3. **Re-sync diff** (§9.6 step 6) to pick formula versus drift.
4. **Phase check.** Shift our command stream by one tick and see if the error collapses. If so, the bug is
   ordering or input phase: jump-before-friction, duck-before-jump, surface-friction sampled a command early
   [Ref §2, §7, §8].
5. **Stored-state check.** Compare stamina, duck_speed, surface_friction, and fall_velocity columns. These carry
   state across commands, and a wrong value shows up as a slowly growing error.
6. **Branch check.** Compare event lists. A missing `landing` or a different jump branch (duck/reset vs standing)
   points to `categorize`, `duck`, or `jump`.
7. **Collision check.** Dump trace calls (start, end, fraction, normal) from both sides with DHooks, if built,
   and diff the first differing trace. Then check geometry mirroring: wrong brush coordinates in the test level
   are the commonest cause, so verify by running the plain-floor scenarios first.
8. **Config check.** Confirm no plugin on the server (including GOKZ in vanilla mode) is altering the scenario.
   Use MovementAPI's logged values, not assumptions.

Record each resolved divergence in `docs/divergences.md`: the capture, the cause, the fix, and whether it
revealed an error in the reference doc. Corrections to the reference are valuable, so note them there too.

### 9.8 Secondary and fallback evidence

Use these when Tier C (live capture) isn't available or to cross-check it. They are weaker, so label results
accordingly.

- **Community numbers:** published jump distances, the known max speeds in each GOKZ mode, perf caps, and the
  per-mode ground speeds. These are sanity checks only [Ref §20].
- **Demo files (`.dem`):** parse with demoinfocs-golang or similar to extract per-tick player origin, velocity,
  angles and buttons from recorded runs. Caveat: demos hold *networked* client-visible state, and the networked
  precision of origin and velocity is limited, so compare at coarse tolerances (about 0.1 to 1 unit) and never use
  them for sub-unit claims such as edgebug or jumpbug thresholds. Missing stored state (stamina, duck speed) must
  be inferred.
- **Differential implementation** (from the Source SDK 2013, for property testing): independent second implementation from the
  Source SDK 2013 for property testing [§10 on provenance].
- **Reference-doc numbers** as unit-test oracles (Tier A). These test the model, not the game.

Rank in a verification report: A (model) < B (differential) < demo/community < C (server capture, bit-level).
Every claim in the README should say which tier supports it.

### 9.9 Acceptance criteria for "close to the original"

Per subsystem, the target and the minimum acceptable:

| Subsystem | Target | Minimum |
|---|---|---|
| Ground accel/friction (S0-S3) | BIT_EXACT | WITHIN_ULP over 500 ticks |
| Air accel, jump, stamina (S4-S9) | BIT_EXACT | WITHIN_ULP over 500 ticks |
| Collision on axis-aligned and simple ramp geometry (S10-S13) | WITHIN_ULP | WITHIN_EPS (1e-3) |
| Duck and ledge cases (S14, S15) | Same reached/not-reached outcome and WITHIN_ULP | Same outcome and WITHIN_EPS |
| Edgebug, jumpbug, duckbug (S16, S17) | Event sequence identical on the same tick | Same outcome on the same input sequence |
| Ladders (S18) | WITHIN_EPS | Same attach/detach ticks, velocity within 1% |
| Per-mode runs (M-*) | WITHIN_EPS | Same takeoff and landing speeds within 0.01 |
| Long jump distance (S19) | Identical landing origin within 1e-3 | Within 0.05 units |

A subsystem is "verified" only when it meets the minimum on captures at both tickrates (where supported) and at
two different origins. Record the status table in `README.md` (verified / partially verified / unverified, with
the tier).

### 9.10 Order of work

The capture rig is infrastructure, so build it early and let it verify each milestone as it lands rather than
verifying everything at the end:

1. **V0: feasibility (before M2).** Confirm a CS:GO build, a local server, and SourceMod and MovementAPI run.
   Confirm plugin input injection works and a reproducibility check (same scenario twice) gives identical logs.
   If this fails, adopt §9.8 and revise the claims.
2. **V1: rig (alongside M1).** Write `csmove_capture.sp` with injection and logging, the capture format, and the
   `tools/compare` importer and diff. Gate: S0 and S1 capture, import, and compare end to end.
3. **V2 to V6:** as each milestone completes, capture its rung(s) from §9.5 and promote passing captures to
   regression tests. A milestone's gate includes its captures reaching at least the minimum in §9.9.
4. **V7: ghost overlay and report** (with M8).

## 10. Provenance and licensing

The historical CS:GO source in the reference [S2, S3, S21] is an unofficial leak, not an authorized release.
Practical policy for this project:

- Use the **official Source SDK 2013** [S1] and the behavior in the reference doc as the implementation source.
- Treat the leaked CS-specific files as a *behavior reference* for checking understanding. Write our own code and
  do not paste or closely transliterate it. Commit nothing derived from verbatim text.
- Do not commit Valve maps, textures, models, or binaries. Test level assets are generated.
- GOKZ/MovementAPI/RNGFix are open source under their own licenses. Check each license before porting any logic
  or distributing, and credit them. The mode notes and comparison tooling should cite them.
- If you plan to publish the repository, decide the policy above before the first public commit.

## 11. Testing strategy

- **Unit tests** per routine, with numbers from the reference in comments pointing at the section.
- **Golden tests:** a recorded `UserCmd` stream plus the expected final `PlayerState` per scenario, stored in
  the repo, replayed on every `cargo test`. Any change to movement ordering shows up here.
- **Property tests** (`proptest`): no tunneling, speed never NaN, velocity component limit respected, replay is
  deterministic (same input twice gives the same output bit for bit), and the primitive trace agrees with a
  brute-force sampled check of the same sweep.
- **Determinism rule:** the `movement` crate may not read time, randomness, or global state.
- Run tests in release mode occasionally (`cargo test --release`) to confirm optimizer-dependent float behavior
  does not change results.
- Keep scenario scripts as data (a small command DSL such as "hold W for 40 ticks, jump, strafe-left with yaw
  delta 0.8 per tick") so techniques can be reproduced in tests and in the app's replay.

## 12. Risks and mitigations

| Risk | Mitigation |
|---|---|
| Faithfulness limited by public sources and the leaked snapshot's mismatch to retail [Ref §25] | Tier C captures; be explicit about what is and isn't verified |
| Float behavior differs from Source's compiled code (x87/SSE, operation order, FMA) | Keep Source's operation order, no `mul_add`, test debug vs release, compare to captures early |
| Duck/unduck and ladders are the least-settled areas [Ref §25] | Do M4 early with heavy tests; instrument; treat ladder as best-effort until captures exist |
| Trace epsilons and plane-clipping details shape surf and edgebug behavior | Port epsilon handling from official Source trace code; add trace-level golden tests; debug draw |
| GOKZ mode details are not in the reference | M7 starts with reading the mode sources and writing notes; do not guess |
| Bevy API churn | Pin the version; keep all logic in the `movement` crate; the Bevy layer stays thin |
| Frame-rate/tick phase gives different results from the real game | Sim only in `FixedUpdate`; sample input at tick time; log which tick each press lands on |
| Licensing of leaked source | Policy in §10 |
| Scope creep into BSP/CS2/weapons | Non-goals in §1; BSP is explicitly stretch |

## 13. Open questions (decide before or during the noted milestone)

1. **Which tickrates must be supported at launch** (64, 128, both)? Default plan: both, and 128 forced in SimpleKZ.
2. **Is a real CS:GO environment available** for Tier C captures (M8)? Verify early, as it changes what "compare to the real thing" means.
3. **Weapon speed model:** M1 uses 250 (knife-like). Decide whether to support weapon speeds and scoped slowdown
   [Ref §5.2] or just a selectable max speed. Default plan: a selectable max-speed value, with the scoped
   branch implemented but not exposed.
4. **Level format:** Rust consts are fine for M5; consider RON when the level grows.
5. **BSP (M9):** is it a real goal or should the project stop at the test level?
6. **Publishing:** public repo or private? Drives §10.

## 14. Immediate next steps

1. M0: install Build Tools, run `cargo test`.
2. In parallel, start V0 from §9.10: check whether a CS:GO build and local server are obtainable. This decides
   how strong the final claims can be.
3. Add `cmd.rs`, `state.rs`, `trace.rs`, and a flat-floor primitive world.
4. Implement wish-velocity construction and the ground path through `process_movement`.
5. Write M1's gate tests, then build the capture rig (M1.5), and only then move to M2.
