# CSMovementRust

A from-scratch Rust reimplementation of CS:GO (Source 1) player movement, with switchable GOKZ-style
Vanilla / KZTimer / SimpleKZ modes, a test level, and a HUD for comparing against the real game.

- `crates/movement`: the simulation. Pure Rust, no Bevy, no I/O, time or randomness. One function per
  Source routine, in Source's call order.
- `crates/app`: a thin Bevy 0.19.1 host (window, input, fixed-tick sim, camera, level, HUD).
- `game-plan.md`: architecture and milestones. `CSGO-Movement-Technical-Reference.md`: the physics spec,
  cited as [Ref §N].
- `docs/`: divergences and unverified choices, mode notes, jump stats notes.

## Build and run

Run cargo from PowerShell with MSVC Build Tools installed (see `CLAUDE.md`).

```powershell
cargo test                       # movement crate + app tests
cargo test -p movement --release # same results with the optimizer on
cargo run -p app --release       # the game
cargo run -p app --release -- --check recordings\<file>.replay   # headless replay vs live run
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

Tiers follow plan §9.8: **A** = reference-doc numbers used as test oracles (tests the model, not the game);
**C** = real-server capture. No tier B, demo, or tier C evidence exists yet: the capture rig (plan §9) has
not been built. Nothing below is verified against the real game.

| Subsystem | Status | Tier | Evidence |
|---|---|---|---|
| Ground friction / acceleration (S0–S3) | Unverified | A | `tests/ground.rs`: [Ref §5] friction and budget numbers, golden accelerate-from-rest tick counts, counter-strafe model |
| Air accel, jump, stamina (S4–S9) | Unverified | A | `tests/air.rs`: [Ref §9] apex table at 64/128 (both branches), stamina cost about 24, deadstrafe band, anti-bhop 3D cap, perfect/late bhop |
| Collision (S10–S13) | Unverified | A | `tests/collision.rs`: wall and crease slides, 18 vs 19 step, 45/46 degree threshold, no-tunneling and trace-sampling property tests |
| Duck and ledges (S14, S15) | Unverified | A | `tests/duck.rs`: four crouch-jump cases from [Ref §10.2], 9-unit shift, low ceiling |
| Edgebug, jumpbug, duckbug (S16, S17) | Unverified | A | `tests/techniques.rs`: reproduced on test geometry, detected on exactly that command, [Ref §14] -6.25 signature, [Ref §15.3] 9–11 window |
| Ladders (S18) | Unverified | A | `tests/techniques.rs`: attach, pitch-dependent climb, hysteresis, jump-ignore, 270 n detach |
| Per-mode runs (M-*) | Unverified, hook algorithms are designs | A | `tests/modes.rs`: [Ref §20] config table, 276 prestrafe cap, 380 perf cap, SimpleKZ takeoff formula; see `docs/modes-notes.md` |
| Long jump distance (S19) | Unverified | — | `tests/jumpstats.rs` checks conventions only |
| Replay determinism | Verified (internal) | — | `tests/replay.rs`: replay equals live bit for bit for all modes; golden scenarios; identical in debug and release |

Known gaps against the plan: the §9 capture and comparison rig (excluded from this pass), the in-app ghost
overlay, BSP loading (M9), triggers and telehops (M10), and the items in `docs/divergences.md`.

## Provenance

Implemented from the official Source SDK 2013 structure and the reference's descriptions. No leaked CS:GO
code, GOKZ, or MovementAPI code was copied. No Valve assets are in the repository; the test level is
generated.
