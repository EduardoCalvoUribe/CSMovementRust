# Reinforcement-learning environment: plan

Branch `reinforcement`. Goal: an environment in which an agent learns to complete KZ courses quickly,
starting from a generated toy course and growing to imported real maps. **This document covers the
environment and infrastructure only.** The learning algorithm and network are deliberately not chosen here;
they come after the literature survey. Everything below is designed so that choice can be made later without
touching the sim.

References: `game-plan.md` (**plan §N**), `CSGO-Movement-Technical-Reference.md` (**Ref §N**).

## 1. Starting point (what the code already gives us)

Read from the tree at `b9b99b1`:

| Piece | Where | Relevance |
|---|---|---|
| `process_movement(cfg, mode, world, state, cmd, obs, dt)` | `movement/src/pipeline.rs` | The whole sim is one function call per tick. No hidden globals. |
| `UserCmd { view_angles, forward_move, side_move, up_move, buttons }`, `Buttons::USE` exists | `cmd.rs` | The action space already exists. |
| `PlayerState` (`Clone`, `PartialEq`) | `state.rs` | Cheap to snapshot. |
| `MovementMode` trait objects with private state; `ModeKind::create() -> Box<dyn .. + Send + Sync>` | `modes/` | **Gap:** mode state (SimpleKZ prestrafe, perf tracking) is not in `PlayerState` and the trait has no clone. Snapshot/restore needs a `fork()`/`box_clone` on the trait. |
| `TraceWorld` (`trace_hull`, `trace_ladder`, `point_solid`), `World` enum (`Primitive` / `Bsp`), both `Clone` | `trace.rs`, `world/` | Observations (rays) and zone tests reuse it. |
| `BspMap::parse`: planes, brushes, nodes, displacements, entities, `triggers: Vec<Trigger>` (kept, **not simulated**), `spawn_points()` | `world/bsp.rs` | The importer exists. Triggers are parsed but inert (M10 not started). `func_button` brushes are already solid world geometry with `EntityId(entity index)`. |
| `Recording` (mode, tickrate, autobhop, start state, `Vec<UserCmd>`), text round-trip, `replay` | `replay.rs` | Any agent run can be saved and replayed bit for bit. |
| `Script` command DSL | `script.rs` | Scripted reference solutions for tests. |
| `testlevel` crate: level described once, used by app + compare | `crates/testlevel` | Pattern to follow for generated courses. |
| `tools/compare`: capture import, diff, scenario/VMF export | `tools/compare` | Path to validate learned runs on the real server (§8). |
| SimpleKZ at 128 Hz (`required_tickrate() == Some(128)`), verified against the real game | `modes/simplekz.rs`, README | The target mode. |

Known limits that matter here:

- `BspMap::parse` rejects **LZMA-compressed lumps** (except lump 40). Many workshop KZ maps may fail to load.
- Triggers (timers, teleports, push, hurt) do nothing. Maps relying on them will be rejected, not silently
  mis-simulated (§5).
- Not yet measured: trace throughput, especially BSP traces (the tree walk allocates a `seen` bitset per trace).

## 2. Architecture

New crates, all pure Rust, none depends on Bevy. `movement` stays unchanged except for the small changes
listed in §9.

```
crates/
  course/    Course description, zones, timer, end-button logic, toy-course generator
  env/       reset/step, action mapping, observation + reward providers, VecEnv
  pyenv/     (optional, late) PyO3 bindings
```

The BSP importer stays in `movement::world::bsp`. A function in `course` turns a `BspMap` into a `Course`.

Dependencies point one way: `movement` <- `course` <- `env` <- `pyenv`. The Bevy app gains a viewer for
`Recording`s of agent runs but is never in the training loop.

## 3. `course`: the common description

```rust
pub struct Course {
    pub world: World,                 // Primitive or Bsp, via the existing enum
    pub spawn: (Vec3, f32),           // origin, yaw
    pub start_zone: Zone,
    pub end: EndSpec,
    pub hazards: Vec<Zone>,           // trigger_hurt equivalents
    pub kill_z: f32,                  // out-of-bounds floor
    pub waypoints: Vec<Vec3>,         // optional, authored by the generator, empty for imported maps
    pub meta: CourseMeta,             // name, source (toy seed / bsp path + hash), unsupported entities
}
```

- **Zone** is a small `PrimitiveWorld` of brushes. Overlap with the player hull is the existing
  `point_solid(origin, hull)`. No new collision code.
- **Timer** (`CourseRun`, per episode, plain data):
  - Starts on the first tick the hull no longer overlaps `start_zone`. Re-entering the start zone resets it.
  - Counts **ticks**. Seconds = ticks / tickrate. Integer ticks make completion exact and reproducible.
  - Stops when the end condition fires; that tick count is the run's time.
- **End button**, two modes behind `EndSpec`:
  - `Touch`: hull overlaps the end volume. Easy to learn, default for the toy course.
  - `Use`: `Buttons::USE` is held and a trace along the view direction, up to use range, hits the button entity
    (`TraceResult::hit_entity`, which already identifies BSP brush entities). Faithful to the real game, and the
    agent has to aim. Use range and the exact conditions come from the real game's behavior; check them against
    the GOKZ/Source behavior before relying on them (open item).
- **Termination:** `Completed`, `Hazard`, `OutOfBounds` (below `kill_z`), or `Truncated` (tick limit, a config
  value). Failure and timeout are different flags so a learner can treat them differently.

The importer (`course::from_bsp`) reads start/end by mapper naming convention and zones from `Trigger` brushes.
The exact entity names used by GOKZ-era maps must be confirmed against real maps before coding. It also builds an
**unsupported-entity report** (teleports, push triggers, moving platforms, water), and refuses to produce a
`Course` when something that changes the outcome of a run is present, unless the caller overrides.

## 4. `env`: step, reset, actions

```rust
pub struct Env { /* Course, mode, cfg, PlayerState, view angles, CourseRun, providers, rng seed */ }
impl Env {
    pub fn reset(&mut self, seed: u64) -> Obs;
    pub fn step(&mut self, a: Action) -> Step;   // Step { obs, reward, terminated, truncated, info }
}
```

- **Action** is what a player controls: forward/back and left/right as {-1,0,1} (the sim takes +-450 from keys,
  as `UserCmd::from_buttons` does), `jump`, `duck`, `use`, and a **yaw delta** per tick (plus pitch if the
  observation needs it). The env integrates view angles, clamps the per-tick turn to a configured maximum, and
  builds the `UserCmd`. The turn limit is a mouse-realism setting (sensitivity x counts), so it is config.
- **Action repeat:** configurable ticks per policy step. 1 is the default so nothing about timing is hidden.
- **Mode/tickrate:** SimpleKZ, 128 Hz, from `ModeKind` + `required_tickrate`. Other modes work through config.
- **Autobhop** (`cfg.autobhop`): open decision (§10). Off means the agent must release and press jump around
  landings, as with scroll-wheel bhop in the real game. On makes bhop chains trivial.
- **Determinism:** same course, seed, and action stream give bit-identical states. A test enforces it.
- **Snapshot/restore** (for curriculum starts and mid-course training): `PlayerState` + mode state + view angles
  + `CourseRun`. Needs the mode fork from §9.
- **Every episode can be exported** as a `Recording` (start state + the `UserCmd`s). That is the single
  artifact for the viewer, regression tests, and real-server validation.

## 5. Observations (pluggable, declared specs)

Providers implement one trait returning named, fixed-shape `f32` blocks plus a spec (names, shapes), so
whichever model comes out of the research can choose or combine them. No provider assumes an architecture.

1. **Proprioception**
   - velocity expressed in the view frame (forward/side/up) and horizontal speed
   - grounded, ducked, duck amount, stamina, ticks since last takeoff, last takeoff speed
   - yaw rate of the last tick, previous action
   - timer-running flag and elapsed ticks
2. **Goal-relative vector:** end position (and the next waypoint when the course has them) in the view frame,
   plus horizontal distance and height difference.
3. **Raycast "screen":** a grid of rays from the eye, configurable horizontal/vertical FOV and resolution.
   - Per ray: normalized distance, and a surface class (walkable floor `normal.z >= 0.7`, steep/surf, wall,
     ladder, button/zone, none).
   - Implemented with `trace_hull` using a zero-size hull, through the same `TraceWorld` as the sim. Check that
     a zero hull is exact in `Ray::new`.
   - It is what a player sees, minus rendering, and it works identically on any map.
   - Cost is rays x traces per step. It is the throughput risk (§7); resolution is a knob.
4. **Raw geometry export:** the course's convex brushes as plane sets, plus bounds. For the geometry-as-set /
   graph approaches the survey may favor. Offered as data, with no encoder built.

Pixels are out of scope: a renderer in the loop costs throughput and the ray grid carries the same information.

## 6. Rewards (pluggable)

Reward is a trait over `(prev, action, next, events)`; `info` always reports the raw quantities (completed, time
in ticks, distance to goal, progress).

- **Base:** a completion bonus and a per-tick time cost. Optimizes "finish" and "finish fast" directly.
- **Progress shaping (optional):** reward for reaching the next waypoint, or reduction in distance to it.
  Waypoints are authored by the toy generator. Real maps have none; the importer would need a waypoint source,
  or run unshaped. Straight-line distance to the end is misleading on KZ routes, so it is not the default.
- Penalties for hazards/out-of-bounds come from the termination reason.

Which shaping works is an experiment, not a design decision; the env just has to make them swappable.

## 7. Throughput and vectorization

- `VecEnv`: N independent `Env`s stepped in parallel with `rayon`, writing observations into one preallocated
  buffer (no per-step allocation in the sim path).
- `Env` must be `Send`; check `Box<dyn MovementMode + Send + Sync>` and `World` satisfy it.
- Benchmark targets are measurements to take first, not promises: sim ticks/s per core on the primitive world
  and on a BSP map, and the extra cost of a ray grid. If BSP traces are too slow, the options are caching the
  `seen` bitset per thread and a coarser ray grid; do not touch trace math (plan §2: operation order is
  behavior).
- Laptop-scale compute is the assumption, so the headless sim is the bottleneck we control.
- **Measured (R1, 2026-11-12):** one `Env` on one core of an i7-7700K, release build, primitive world with
  two brushes, default observation (proprioception + goal): **~2.2M steps/s** (`crates/env/tests/throughput.rs`,
  run with `cargo test --release -p env --test throughput -- --ignored --nocapture`).
- **Measured (R4, same machine):** with the ray grid on a 6-brush toy course, default 8x16 grid (128 rays,
  each a world trace plus a ladder trace on hit and two zone traces): **~46k steps/s**; 4x8 grid: **~162k
  steps/s**. Rays dominate the step cost; resolution, `detect_ladders`, and `see_zones` are the knobs.
- **Measured (R5, same machine, 4 cores / 8 threads, rayon global pool), total env-steps/s:**

  | envs | no rays | default rays (8x16) |
  |---|---|---|
  | 1 | 1.63M | 64k |
  | 4 | 0.54M | 97k |
  | 8 | 0.89M | 187k |
  | 16 | 1.55M | 236k |
  | 64 | 3.28M | 301k |

  Without rays one step is ~0.5 us, so rayon's per-step fork/join costs more than the work until there are
  dozens of envs; use many envs per `VecEnv` (or action repeat). With rays the work dominates and 64 envs
  reach ~4.7x one env. Not yet measured: BSP worlds.

## 8. Toy courses, curriculum, validation

**Generator** (`course::toy`), seeded and deterministic: start platform, a sequence of blocks, end platform
with the end volume or button. Emits `Brush`es into a `PrimitiveWorld` the same way `testlevel` does, so the
same description can be rendered by the app and exported through `tools/compare`.

- Difficulty stages: walk to the end -> one jump over a gap -> jump chains -> long jumps near the mode's
  limit -> bhop chains -> height changes, then crouch-jump and ladders as the sim allows.
- Randomized gaps, offsets, and heights within a stage, so a policy cannot memorize a layout.
- Each stage ships with a **scripted solution** (`Script`) proving it is solvable and giving a reference time.
  Gaps are chosen from the verified jump-distance data (README: LJ numbers), not guessed.

**Rules layer / anti-exploit.** RL will find simulator exploits. Mitigations:

- `OutOfBounds` and a course bounding box.
- Flag runs that skip required waypoints or touch geometry classes a real run cannot use.
- **Real-server validation:** `Recording` -> `cmds.csv` -> the existing capture rig (`tools/compare`,
  plan §9) replays the exact commands in CS:GO and diffs. A learned record only counts as real once it passes.
  This depends on the rig accepting arbitrary command streams on a toy map exported through the VMF path;
  confirm before relying on it.

## 9. Required changes to `movement` (small, additive)

1. **Mode fork:** `fn fork(&self) -> Box<dyn MovementMode + Send + Sync>` (or `box_clone`) so state can be
   snapshotted. Vanilla/KZTimer/SimpleKZ implement it by cloning.
2. Possibly expose the ground normal / hit entity from the last command for observation features (it is
   already in `MoveData` and the observers; verify rather than assume).
3. No change to any sim arithmetic. `cargo test` and all captures must stay green after each change.

## 10. Decisions (open ones marked)

1. **Autobhop: DECIDED off** (faithful to the real game; the agent must release and press jump around landings). Still a config flag;
   revisit only if learning stalls.
2. **Use-range: placeholder 64 units until confirmed by the user against the real game** (a named constant, easy to change). The exact `+use` rules for the end button (verify on
   a real map).
3. **Mapper naming convention** for start/end on the maps you will use (needs real maps to check).
4. **Waypoints for real maps**: none vs. extracted vs. hand-annotated.
5. **Python bindings:** build `pyenv` only when the trainer is chosen.
6. **Where the maps come from.** No KZ map is on the dev laptop for now, so R7 waits on one (toy courses carry R0-R6). The repo docs report a real KZ map was used for the BSP gate; if it is
   in a Steam workshop folder, `crates/app/src/maps.rs` already finds it; otherwise point
   `CSMOVE_MAP_DIRS` at your map folder. Maps are never committed (CLAUDE.md).

## 11. Milestones and gates

Each gate is an automated test unless noted. Do not start the next until the gate passes.

| | Milestone | Gate |
|---|---|---|
| R0 | `course` crate: `Course`, `Zone`, `CourseRun` timer, `Touch` end condition, hazards, kill plane | A `Script` that walks start->end on a flat primitive course yields the exact expected tick count; leaving the start zone starts the timer, re-entering resets it |
| R1 | `env` crate: `Action`->`UserCmd`, `reset`/`step`, termination reasons, proprioception + goal-vector providers, base reward, `Recording` export | Determinism test; replaying the exported `Recording` reproduces the final state bit for bit; episode matches R0's scripted tick count when fed the same actions |
| R2 | Mode `fork`, snapshot/restore | Restore then continue equals running straight through, for all three modes |
| R3 | Toy generator with stages and scripted solutions | Every generated stage is solved by its script; same seed gives the same course |
| R4 | Raycast provider + surface classes | Rays agree with brute-force sampling on the primitive world; observation spec test; cost-per-step benchmark recorded |
| R5 | `VecEnv` (rayon), preallocated buffers | Parallel result equals serial result; steps/s benchmark recorded in this doc |
| R6 | `Use` end condition; waypoints + shaping rewards | Scripted run presses the button; shaping reward telescopes to progress |
| R7 | BSP -> `Course`, naming convention, unsupported-entity report, LZMA lump support if needed | A real KZ map loads as a `Course`; a hand-recorded run completes it with the expected time; maps with teleports/push triggers are rejected with a clear report |
| R8 | Bevy viewer for agent `Recording`s | Recorded run replays visually and bit-exactly (manual + replay test) |
| R9 | Real-server validation of a recording through the capture rig | A scripted toy-course run diffs `WITHIN_EPS` or better against CS:GO |
| R10 (late) | `pyenv` bindings | A Python process runs a rollout |

R0-R2 can start immediately. R7 needs a real map and the LZMA decision. R9 depends on the CS:GO install used
for the earlier captures still being available.

## 12. Risks

| Risk | Mitigation |
|---|---|
| Sim throughput too low on laptop CPU | Measure first (R1/R4/R5); tune ray resolution and action repeat; do not touch sim arithmetic |
| Agent exploits sim divergences from the real game | Bounds/waypoint rules (§8); real-server validation (R9) |
| Sparse reward makes learning impractical | Waypoint shaping on toy courses; curriculum; swappable rewards |
| Real maps use unsimulated triggers | Importer rejects with a report; M10 work later, scoped by what the maps need |
| LZMA lumps block map loading | Decide at R7, using the maps you actually have |
| Snapshot/restore subtly diverges from straight-through | R2 gate for all modes |

## 13. Status and handoff

All gates below were run with `cargo test` (PowerShell, MSVC Build Tools) on 2026-11-12; the whole workspace
suite was green after each.

- **R0 passed.** `crates/course`, 8 tests in `crates/course/tests/run.rs`. On first run one test was wrong, not
  the timer: walking back 400 ticks crosses the start zone (reset) and leaves through its far side (new run),
  so it now checks the reset tick and the restart at tick 1. Pinned golden: holding forward on the flat course
  completes on command index 312 with a run time of **259 ticks** (504 units at 250 u/s is ~258).
- **R1 passed.** `crates/env`: `Action` -> `UserCmd`, clamped per-tick yaw/pitch (`EnvConfig`, placeholder 10
  deg/tick), `reset(seed)` / `step` and allocation-free `reset_into` / `step_into`, action repeat,
  terminated vs truncated, `Proprioception` (21 fields) and `GoalVector` (4 fields) providers behind
  `ObsProvider` with a named `ObsSpec`, `BaseReward` behind `Reward`, and `Recording` export. 12 tests in
  `crates/env/tests/env.rs`: bit-identical episodes, every intermediate state of the exported recording
  replays bit for bit (also through the text format), and holding forward reproduces R0's 312 / 259.
- **R2 passed.** `MovementMode::fork()` (the one planned `movement` change, §9.1; each mode derives `Clone`).
  `Env::snapshot` / `restore` cover sim state, mode state, view, timer, tracked facts, reward state, and the
  recording so far. Gates: `crates/movement/tests/mode_fork.rs` (fork continues like the original, all three
  modes, and resuming the KZ modes with a fresh mode diverges, so the test is sensitive) and
  `crates/env/tests/snapshot.rs` (restore into the same or a fresh env then continue equals straight
  through, all three modes, five cut points).
- Reward magnitudes (`BaseReward::default`: +10 completion, -1/128 per tick) and the turn limit are
  placeholders for whoever picks the learner; nothing depends on them.
- **R3 passed, with a reduced stage list.** `course::toy::generate(stage, seed)`: a row of floating blocks
  along +x on integer coordinates, void below (falling is out of bounds), start zone on the back half of the
  start platform, the whole end platform top as the `Touch` end. `Course` gained `waypoints` (block-top
  centers; unused by run rules). Stages: `Walk`, `Gap`, `GapChain`, `ShortBlocks`, `Heights` (ups 8-32,
  drops 16-96). Each course carries a **reference solution**: a `UserCmd` list produced by a simple
  controller (hold W, jump near the edge on a fresh press, release in the air; on `ShortBlocks` re-jump on
  landing) run at generation time, plus its reference time in ticks. Deviation from §8: gaps are set from
  the controller's **measured reach at 250 u/s** (hull front past the edge at landing height: 217 flat, 186
  at +32, 159 at +48, 260 at -64), not from the README LJ numbers, because no controller here goes faster
  than 250. **Not yet built:** "long jumps near the mode's limit" and speed-gaining "bhop chains"; both need
  a prestrafing, air-strafing reference controller to prove them solvable. `ShortBlocks` rewards jumping
  on the landing (edge-only jumping drifts and overshoots, e.g. seed 15) but does not force speed gain.
  Gates in `crates/course/tests/toy.rs`: 300 seeds x 5 stages replayed from scratch all complete at their
  reference time; same seed, same course; seeds vary the layout; integer geometry. Reference times at
  seeds 0-299: Walk 83, Gap 116-170, GapChain 490-1169, ShortBlocks 422-735, Heights 464-1314 ticks.
  `crates/env/tests/env.rs` also drives toy solutions through `Env` as `Action`s and gets the same times
  and the same commands.

- **R4 passed.** `env::RayGrid` (`crates/env/src/rays.rs`), an `ObsProvider` writing a `[rows, cols,
  channels]` block named `rays`: per ray the distance as a fraction of `max_distance` (1.0 = nothing) and a
  `SurfaceClass` (none, floor `n.z >= 0.7`, steep, wall `|n.z| < 0.01`, ceiling, ladder, goal, hazard), one-hot
  or as an index (`ClassEncoding`). Rays are zero-size hull traces through `TraceWorld` (a zero hull is an
  exact segment trace in `Ray::new`: zero extents and offset; it stops `DIST_EPSILON` short, as Source
  does). Ladders are told from walls by a second `trace_ladder`; the end volume and hazards are seen through
  `Zone::ray_entry` up to the first solid hit. Eye height is 64 standing / 46 ducked, interpolated by
  `duck_amount`, the app camera's values (still marked "to confirm" there). The grid is evenly spaced in
  angle, row 0 top, column 0 left, and follows the view pitch unless `follow_pitch` is off. Defaults (8x16,
  120x60 degrees, 2048 range) are placeholders. Gates in `crates/env/tests/rays.rs`: >2500 rays from random
  eyes and views agree with brute-force ray marching plus bisection on a scene with every class (distance
  within `DIST_EPSILON / cos` and class exact, edges and grazing rays skipped); straight-down distance
  equals the eye height; layout orientation; the written block matches its spec in both encodings.
  Benchmark recorded in §7.

- **R5 passed.** `env::VecEnv` (`crates/env/src/vec.rs`), the first external dependency in `env`: `rayon =
  "=1.11.0"`. N envs (same observation spec, courses may differ) step into one env-major observation buffer
  allocated once, plus a `final_obs` buffer holding the last observation of any episode that just ended.
  Auto-reset (default on) resets a finished env right away with seed `episode_seed(base, env, episode)`, so
  results never depend on threads; `with_auto_reset(false)` + `reset_one` for manual control;
  `with_threads(n)` for a private pool; `serial()` for comparison. Gates in `crates/env/tests/vec.rs`: global
  pool, a 3-thread pool, and serial give bit-identical observations, final observations, infos, and end
  states over 700 steps with 12 envs on mixed toy courses (with rays) and >20 auto-resets; auto-reset
  matches a lone `Env` step for step, final observation and reset seed included. Benchmark in §7.

- **R6 passed, with placeholder `+use` rules.** `EndSpec::Use { entity, target }`: the run completes on a
  **fresh** `+use` press (held on this command, not the previous one; `CourseRun::use_held`) while running,
  when a straight zero-size trace from the eye along the view, `USE_RANGE` = 64 long, hits `entity`
  (`course::use_hits`), checked on the state after the tick's movement. Range, press-vs-hold, the straight
  trace (Source's use search is more forgiving), and the timing are all placeholders to confirm against the
  real game (§10.2). The eye (`course::eye`, 64/46) moved from `env` to `course` so both share it.
  `toy::generate_with_end(stage, seed, EndKind::Button)` adds a 16x32x48 button brush (`BUTTON_ENTITY` = 1)
  spanning eye height near the end platform's far end; the layout is the same as the `Touch` course, and the
  `Touch` courses (and R3 reference times) are unchanged. Waypoints are now `Waypoint { pos, radius }`,
  reached in order while running (`CourseRun::next_waypoint`; within `radius` horizontally, 8 below to 72
  above; reset with the timer) and never decide the outcome; toy waypoints are block-top centers with radius
  half the block length + 16. `GoalVector` grew to 9 fields (adds `has_waypoint` and the next waypoint in the
  view frame). `RayGrid` classes the button brush as `goal`. Rewards (all optional, combine with
  `SumReward`): `ProgressShaping` (potential `-Course::remaining_route`, `gamma` configurable, default scale
  0.01 a placeholder), `WaypointBonus`; `RewardCtx` now carries the runs. `StepInfo::waypoints_reached`.
  Gates in `crates/env/tests/r6.rs`: 5 stages x 100 seeds of button courses are solved by their scripted
  runs, finishing on a fresh press; holding `+use` never presses, out of range or facing away misses;
  shaping sums telescope to `scale * (phi_end - phi_start)` on solutions and on random play; waypoints are
  reached one at a time in order (all of them, except that dropping onto the end platform can complete the
  `Touch` end before the last one, which the test checks is the reason); the goal block reports the next
  waypoint; snapshots fork combined rewards.

### Handoff for the next session

1. **Next is R7** (BSP -> `Course`): needs a real KZ map on this machine, the mapper naming convention for
   start/end (§10.3), and the LZMA decision. Ask the user before starting. R8 (Bevy viewer for agent
   recordings) does not depend on R7 and can go first.
2. Optional, when wanted: a strafing reference controller, which unlocks the LJ and bhop-chain stages of R3.
3. **Still open, ask the user when they come up:** whether to use waypoint shaping during training (it is
   built and optional); whether real maps will be available (R7 needs one, and `BspMap::parse` rejects LZMA
   lumps); the real `+use` rules (range, press vs hold, search); real values for the per-tick turn limit.
4. **Environment notes:** local `main` matches `origin/main` (`b9b99b1`). Local `reinforcement` tracks
   `origin/reinforcement`; push only when the user asks.
