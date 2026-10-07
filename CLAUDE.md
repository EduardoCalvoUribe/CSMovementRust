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
  is part of the behavior; sub-unit thresholds depend on it.
- Keep one function per Source routine (`check_jump_button`, `categorize_position`, `try_player_move`, ...) and keep
  Source's call order even where it looks redundant [Ref §2].
- State that crosses commands (stamina, surface friction, duck speed, fall velocity) is a `PlayerState` field,
  never a local.
- Collision goes through the `TraceWorld` trait so primitive brushes and BSP are interchangeable.
- Modes are config plus hooks over one core, not separate controllers.

## Provenance policy
The cstrike15 source cited in the reference is an unofficial leak. Implement from the official Source SDK 2013 and
the reference's descriptions. Do not paste or closely transliterate leaked or GOKZ/MovementAPI code, and check
those licenses before porting logic. Never commit Valve assets (maps, textures, models, demos).

## Workflow
- Run cargo from **PowerShell**, not Git Bash: Git Bash resolves GNU `link` ahead of MSVC `link.exe` and linking
  fails. MSVC Build Tools must be installed (here: `D:\Programs\VSBuildTools`). Rust lives on D: via the user
  env vars `RUSTUP_HOME=D:\Programs\rust\rustup` and `CARGO_HOME=D:\Programs\rust\cargo`.
- `cargo test` must stay green. Numbers in tests come from the reference and carry a `[Ref §N]` comment.
- Pin the Bevy version exactly; check MSRV against the installed `rustc` before upgrading.
- Validated real-server captures become regression tests under `crates/movement/tests/captures/` (plan §9.6).
- Record each resolved mismatch with the real game in `docs/divergences.md`.

## Status
See `game-plan.md` §3 and §8, and `README.md` for verification status. Current: M0–M8 implemented with automated
gates green (manual M5 playtest pending); §9 capture rig and ghost overlay built and run against CS:GO
1.38.8.1 (see `docs/verification.md`); M9 (BSP), M10 (triggers) not started.
