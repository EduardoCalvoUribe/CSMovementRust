# CSMovementRust

Rust + Bevy reimplementation of CS:GO (Source 1) player movement, with switchable Vanilla / KZTimer / SimpleKZ
modes, a test level, and a HUD for comparing against the real game.

## Read first
- `game-plan.md`: architecture, milestones, verification strategy. Follow its milestone order and gates.
- `CSGO-Movement-Technical-Reference.md`: the physics spec. Cited in the plan as **[Ref §N]**. Do not duplicate
  its content in code comments; cite the section.

## Architecture rules
- `crates/movement` is pure Rust with **no Bevy dependency** and no I/O, time, or randomness. Bevy code lives only
  in `crates/app`, and stays thin.
- Do not use Avian, Rapier, or other physics/character-controller crates for movement.
- All sim math is `f32`. No `f64`, no `mul_add`/FMA, and do not reorder arithmetic to "simplify" it. Operation order
  is part of the behavior; sub-unit thresholds depend on it. One allowed exception: the trace end point
  (`Ray::finish` in `world/primitive.rs`) keeps the sum in `f64` and rounds once, reproducing the 32-bit
  engine's x87 arithmetic (`docs/divergences.md` D17). Map compilation (`plane_from_points`) and render-only
  geometry (`Brush::polygons`) are not sim math.
- Keep one function per Source routine (`check_jump_button`, `categorize_position`, `try_player_move`, ...) and keep
  Source's call order even where it looks redundant [Ref §2].
- State that crosses commands (stamina, surface friction, duck speed, fall velocity) is a `PlayerState` field,
  never a local.
- Collision goes through the `TraceWorld` trait so primitive brushes and BSP are interchangeable.
- Modes are config plus hooks over one core, not separate controllers.

## Provenance and licence
The project is GPL-3.0-or-later (`LICENSE`). The cstrike15 source cited in the reference is an unofficial leak:
never link it, paste it, or closely transliterate it; implement from the official Source SDK 2013, the reference's
descriptions and measurements. GOKZ and MovementAPI are GPL-3.0 and may be ported, with credit in the code and
`docs/modes-notes.md`. Never commit Valve assets or third-party maps (maps, textures, models, demos).

## Workflow
- Run cargo from **PowerShell**, not Git Bash: Git Bash resolves GNU `link` ahead of MSVC `link.exe` and linking
  fails. MSVC Build Tools must be installed (here: `D:\Programs\VSBuildTools`). Rust lives on D: via the user
  env vars `RUSTUP_HOME=D:\Programs\rust\rustup` and `CARGO_HOME=D:\Programs\rust\cargo`.
- `cargo test` must stay green. Numbers in tests come from the reference and carry a `[Ref §N]` comment.
- Pin the Bevy version exactly; check MSRV against the installed `rustc` before upgrading.
- Validated real-server captures become regression tests under `crates/movement/tests/captures/` (plan §9.6).
- Record each resolved mismatch with the real game in `docs/divergences.md`.

## Status
See `game-plan.md` §3 and §8, and `README.md` for verification status. Current: M0–M9 implemented with gates
green (M5 playtested); §9 capture rig and ghost overlay built and run against CS:GO 1.38.8.1 (see
`docs/verification.md`), including captures on a community KZ map; M10 (triggers) not started. The
repository will be public.

## Reinforcement-learning branch (`reinforcement`)
Work on this branch builds an RL **environment and infrastructure** for training an agent to finish KZ courses
quickly. Read `docs/reinforcement-plan.md` first (milestones R0-R10, gates, decisions, and its §13 handoff
with the exact next steps). Rules specific to this work:
- Do **not** choose or design the RL algorithm or network. The user is surveying SOTA research first and will
  supply it. Build the environment so that choice stays open.
- New crates (`course`, `env`, later `pyenv`) are pure Rust on top of `movement`, with no Bevy. Do not change
  sim arithmetic; the only planned `movement` change is a mode `fork()` for snapshots (plan §9).
- Decided: SimpleKZ at 128 Hz, autobhop off, `Touch` end condition first, `+use` range is a placeholder (64)
  until the user confirms it.
- Observations are pluggable: proprioception, goal-relative vector, and a raycast grid through `TraceWorld`
  (no pixels). Do not hard-code one.
- Do not mark a milestone done until its gate tests have actually run; say plainly when they have not.
