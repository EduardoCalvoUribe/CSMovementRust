# CSMovementRust

A from-scratch Rust reimplementation of CS:GO (Source 1) player movement, with switchable GOKZ-style
Vanilla / KZTimer / SimpleKZ modes, a test level, and a HUD for comparing against the real game.

- `crates/movement`: the simulation. Pure Rust, no Bevy, no I/O, time or randomness. One function per
  Source routine, in Source's call order.
- `crates/app`: a thin Bevy 0.19.1 host (window, input, fixed-tick sim, camera, level, HUD).
- `game-plan.md`: architecture and milestones. `CSGO-Movement-Technical-Reference.md`: the physics spec,
  cited as [Ref §N].
- `crates/testlevel`: the test level, shared by the app, the map exporter and the comparator.
- `tools/compare`: the capture-rig CLI (map and scenario export, import, diff, reports, promotion);
  `tools/capture`: the server plugin and setup/run scripts (plan §9).
- `docs/`: verification record, divergences, mode notes, jump stats notes.

## Build and run

Run cargo from PowerShell with MSVC Build Tools installed (see `CLAUDE.md`).

```powershell
cargo test                       # movement crate + app tests
cargo test -p movement --release # same results with the optimizer on
cargo run -p app --release       # the game
cargo run -p app --release -- --check recordings\<file>.replay   # headless replay vs live run
cargo run -p app --release -- --ghost data\captures\raw\<run>\<capture>   # play a real-server capture as a ghost
```

## Controls

| Key | Action |
|---|---|
| Left mouse / Esc | Grab / release the cursor |
| W A S D, Space, Left Ctrl, Left Shift | Move, jump, duck, walk |
| Mouse wheel | Jump (press and release land on one tick) |
| F1 / F2 / F3 | Vanilla / KZTimer / SimpleKZ (SimpleKZ locks 128 tick) |
| T | Toggle 64 / 128 tick (where the mode allows) |
| R | Reset to spawn |
| F6 / F7 | Save / load checkpoint |
| H | Toggle autohop |
| G | Toggle debug view (hull, traces, plane normals, wish values, speed graph) |
| F5 | Start / stop recording (`recordings/`: `.replay` command stream, `.csv` per-tick state, `.final`) |
| P / . | Pause / single-step one tick |
| 1–9 | Teleport to a test area |

Test areas: flat floor, long-jump runway (gaps 220–290), bhop rows, ledges (18–66), stairs (18, 16, and a
19 that can't be stepped), wall and corridor, ramps (30–70 degrees), a 60 degree surf ridge, an edgebug
platform, jumpbug drop towers, and a ladder.

## Verification status

Measured against the real game (Tier C): CS:GO 1.38.8.1 dedicated server, the test level compiled into a
map, a real client driven by an input-injection plugin, every float logged as raw bits. Rig, commands and
findings: `docs/verification.md`; every mismatch found and fixed: `docs/divergences.md`. Tiers follow plan
§9.8 (**A** = reference-doc numbers as test oracles, **C** = real-server capture).

Results with the current model, as worst verdict per capture against plan §9.9 (64 tick / 128 tick):

| Subsystem | 64 tick | 128 tick | §9.9 minimum | Status |
|---|---|---|---|---|
| Ground friction / acceleration (S0-S3) | 7/7 BIT_EXACT | 7/7 BIT_EXACT | WITHIN_ULP | **Verified** (C), target met |
| Air accel, jump, stamina, bhop, falls (S4-S9) | 10/10 ≤ WITHIN_EPS, most BIT_EXACT/ULP | 10/10 ≤ WITHIN_EPS | WITHIN_ULP | **Partially verified** (C): a few captures are 1-2 ticks off by more than 4 ULP |
| Collision: walls, creases, stairs, ramps 30-60° (S10-S13) | 27/27 ≤ WITHIN_EPS | 27/27 ≤ WITHIN_EPS | WITHIN_EPS | **Verified** (C) |
| Duck and ledges (S14, S15) | all ≤ WITHIN_EPS | all ≤ WITHIN_EPS | same outcome, WITHIN_EPS | **Verified** (C) |
| Edgebug, jumpbug, duckbug (S16, S17) | events identical; S17 duck_speed column open (D14) | all ≤ WITHIN_EPS | same event sequence | **Verified** (C) for events |
| Ladders (S18) | WITHIN_EPS | WITHIN_EPS | attach/detach ticks, velocity within 1% | **Verified** (C) |
| Long jump distance (S19) | WITHIN_EPS | WITHIN_EPS | within 0.05 units | **Verified** (C) |
| Far from the map origin (S1F, S4F, S7F) | as their near-origin twins | same | | **Verified** (C) |
| Per-mode runs (M-*): KZTimer, SimpleKZ jumps, bhops, long jumps | no GOKZ counterpart (GOKZ keeps 64 tick Vanilla) | 14/14 ≤ WITHIN_EPS, most BIT_EXACT | same takeoff/landing speeds within 0.01 | **Verified** (C) at 128 tick |
| KZ prestrafe (S20) | | KZTimer and SimpleKZ FAILED | | **Unverified**: our hook designs differ from GOKZ (D10) |
| Replay determinism | | | | Verified (internal): replay equals live bit for bit |

Reproducibility of the rig itself: 75 scenarios captured in two separate server sessions are
bit-identical. 50 validated captures are replayed by `cargo test` (`crates/movement/tests/captures/`,
checked by `tools/compare/tests/captures.rs`).

## Provenance

Implemented from the official Source SDK 2013 structure and the reference's descriptions. No leaked CS:GO
code, GOKZ, or MovementAPI code was copied. No Valve assets are in the repository; the test level is
generated.
