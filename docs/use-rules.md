# `+use` and KZ timer rules, measured

Measured on 2026-11-13 against CS:GO 1.38.8.1 (dedicated server + legacy client, GOKZ 3.6.4 in SimpleKZ,
128 tick) with the capture rig (`docs/verification.md`). They replace the R6 placeholders
(`docs/reinforcement-plan.md` §10.2) and are implemented in `crates/course/src/run.rs` (`use_target`,
`CourseRun`, `KzRules`).

## How it was measured

`compare useprobe <dir>` writes a flat probe map (`csmove_useprobe`) with `func_button`s using the
keyvalues of kz_longjumps_v4096's `climb_startbutton` (`spawnflags 1025` = don't move + use activates,
`wait 0.0001`, `min_use_angle -1`), and 45 scenarios:
- walk-ins tapping `+use` every other command: slow (0.3 or 0.08 units per command) and fast (250 u/s,
  three offsets, both tap phases), on a 16-unit and a 64-unit cube, ducked, with the button 16-64 units
  above, 48 below, 24-64 to the side, diagonal, and behind a wall;
- yaw sweeps (from facing away to facing it, both sides, 24 and 48 units) and pitch sweeps (full range);
- `+use` held the whole walk-in;
- a button with `min_use_angle 0.8`;
- GOKZ: press `climb_startbutton` and run to `climb_endbutton`; jump in place and keep tapping the start
  button through the landing (both tap phases).

The capture plugin (1.2.0) logs every `OnPressed`/`OnUseLocked` output and GOKZ's
`GOKZ_OnTimerStart_Post`/`GOKZ_OnTimerEnd_Post` with the command index and whether it happened inside that
command, plus the server's eye position before and after each command (`events.csv`).
`compare useprobe-report <captures> <out.csv>` tabulates every fresh press; `compare useprobe-check
<captures>` replays them through the `course` crate's rules and requires agreement.

## Engine `+use` (any `func_button`)

| Rule | Measured |
|---|---|
| Trigger | A fresh press only (`+use` down now, up on the previous command). Holding it while walking into range never presses. |
| Position | The eye position **before** the command's movement. Only that hypothesis is consistent across the fast walk-ins (after-movement gives contradictory brackets). The press happens inside the command. |
| Eye height | 64 standing, 46 ducked above the origin (server eye position). |
| Reach | A **box**: the button is in reach when the largest per-axis gap between the eye and the button's bounding box is under ~82.05. The x-gap cutoff is the same, 81.92-82.08, with the button 64 above, 48 below, 64 to the side or 32+32 diagonal, so neither a 3-D nor a horizontal distance fits; the Chebyshev distance fits with the cutoff in (82.029, 82.078]. Distance to the button's centre does not fit (the 16- and 64-unit cubes switch at the same face distance). Consistent with Source's radius search (80) enumerating by bounding box plus about 2 units of bounds bloat; `USE_RANGE = 82.05`. |
| View direction | With `min_use_angle -1` (KZ maps' value): **none**. Every yaw and pitch presses, including facing directly away. With `0.8`: presses only within ~37° (fired at 36.5°, missed at 37.5°: `dot >= 0.8`). The env assumes -1. |
| Line of sight | Required: a button 36 units away behind a wall never pressed (450 presses). Modelled as a trace from the eye to the box's nearest point that must reach the button. |
| Several in reach | Not measured. The env picks the one whose nearest point is closest to the view line (Source's scoring). |
| Refire delay | After firing, even with `wait 0.0001`, a button ignores presses for a few commands: the next fire came 2, 4, 6 or 8 commands later, never more. **Not modelled**, by decision (the env lets every in-reach press count). |

Check result: 9,174 presses agree with `use_target`, 2,454 misses fall within 8 commands of a fire
(refire delay), 0 unexplained.

## GOKZ timer (SimpleKZ)

From gokz-core's source (GPL-3.0, KZGlobalTeam/gokz: `timer/timer.sp`, `timer/virtual_buttons.sp`,
`map/buttons.sp`, `include/gokz/core.inc`), confirmed by the captures:

- Buttons are `func_button`s named `climb_startbutton` / `climb_endbutton` (and `climb_bonus<N>_...`),
  hooked on `OnPressed`.
- **Start** (`TimerStart`): refused unless on the ground and not just landed: the press command must be
  more than 4 after the command whose movement landed the player (`GOKZ_TIMER_START_GROUND_TICKS`).
  Captured: landed in command 102, presses at 103-106 refused, 107 started. Judged before the press
  command's movement, like the press itself. Also refused: just teleported or noclipped, invalid move type.
- **Virtual start button**: a successful start records the origin the player pressed from. After every
  later command (GOKZ's post-think), if a fresh `+use` (any, not only on the button) happened within the
  last 2 s, the player's origin is within the mode's range of that point (SimpleKZ **32**, KZTimer 70,
  vanilla 0; 3-D, origin to origin, in line of sight) and the start conditions hold, the timer **restarts**.
  Captured: restarts while 31.05 away, none at 33.01. So in practice a run starts when the player leaves
  that 32-unit radius (or the ground), not at the press; standing still after a press, restarts continue for
  2 s. Not after the start command itself (`JustStartedTimer`).
- **End** (`TimerEnd`): any press of the end button while running; no ground check.
- **Time**: started (or restarted) on command `s`, ended by a press on command `e`: `e - s` ticks.
  Captured: last restart after command 59, end press in command 158, GOKZ time 0.7734375 s = 99 ticks.

`CourseRun` reproduces every captured timer start, restart and end time (`compare useprobe-check`).

## Not covered

- The refire delay (above).
- `min_use_angle` other than -1, rotated or non-box buttons (the env uses the bounding box of the button's
  brushes), and which button wins when two are in reach.
- GOKZ start/end *zones* (`climb_startzone`), teleport and noclip rules, the virtual *end* button (only
  matters after a run has ended), and the "TimerButtonZoneType" options (defaults assumed).
- Eye height while the duck is in transition (only fully standing and fully ducked were seen).
