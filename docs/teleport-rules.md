# `trigger_teleport` rules, measured

Measured on 2026-11-13 against CS:GO 1.38.8.1 (dedicated server + legacy client, GOKZ 3.6.4 in SimpleKZ,
128 tick) with the capture rig (`docs/verification.md`). KZ maps chain their stages with teleports
(kz_longjumps_v4096 has 7: an 8-unit slab at each stage's end sends the player to the next stage), so
R7 needs them. Implemented in `crates/course` (`Teleport`, `Course::touch_teleports`,
`CourseRun::after_command`, the BSP importer's `plain_teleport`).

## How it was measured

`compare tpprobe <dir>` writes a flat probe map (`csmove_tpprobe`) with `trigger_teleport`s using
kz_longjumps_v4096's keyvalues (`spawnflags 4097`, an `info_teleport_destination` one unit above the
floor) and 12 scenarios: slow, very slow (0.08 units per command), running, jumping, ducked and
backwards walk-ins on a wall-like volume; walking onto and landing on a 4-unit floor slab; a trigger with
spawnflag 32; and two GOKZ lanes (run from the start button through a teleport to the end button; arrive
next to a start button tapping `+use`). Plugin 1.3.0 also logs the trigger's `OnStartTouch`.
`compare tpprobe-check <captures>` replays them through our sim plus the course's teleports.
Captures: `data/captures/raw/tpprobe1`.

**Check result:** `compare tpprobe-check data/captures/raw/tpprobe1` prints `TELEPORT RULES MATCH`: 11 of
12 scenarios are BIT_EXACT free-running and re-synced (the ducked one WITHIN_ULP), every teleport fires on
the server's command, and GOKZ's timer agrees (TPg up to the refire delay, below). Regression test:
`crates/course/tests/teleport.rs` (the captured TPa_jump teleport and the command after it, bit for bit).

## Results

| Rule | Measured |
|---|---|
| When | Inside the command, after its movement: the teleport fired on the same command as ours in all 11 teleporting scenarios (walls, slabs, both sides, in the air, ducked). |
| Touch test | The hull overlapping the trigger's volume at the end of the command's movement (`Zone::overlaps`); the slow walk-ins switch on the same command. |
| Where | The destination's origin exactly (the player's mins.z is 0); bit-exact in every capture. |
| Velocity | Kept, bit for bit (as Source SDK 2013's `CTriggerTeleport::Touch`, which passes no velocity). |
| Ground | Left (`SetGroundEntity(NULL)`). |
| Next command | **The server re-categorizes the player's position before moving**: arriving 1 unit above the floor, the next command starts on the ground (origin z 0.03125 after it) even when the player arrived rising from a jump (vz 136). This is Source's `m_bGameCodeMovedPlayer` path in `PlayerMove`. In the sim: `PlayerState::game_moved`, set by `touch_teleports`, makes the next `process_movement` run `categorize_position` first and clears it (a `movement` change approved by the user on 2026-11-13; no arithmetic changed). Before it, our replay fell from z 1 and diverged on the command after every teleport. |
| View | The destination's angles (SDK: unless a landmark is used). Not measurable with this rig (the plugin overrides command angles); the env snaps its view to them. |
| Spawnflag 32 | The SDK reads it as "preserve angles", but for triggers it is also "only clients in vehicles", checked first: the TPb trigger **never** teleported the player. The importer treats such teleports as not affecting players. |
| GOKZ | Map teleports leave a running timer alone (time measured across one matched). A start within 4 commands of a teleport is refused (`JustTeleported`, like landing); both TPh lanes reproduce every start, 281 and 280 events. |

TPg's last virtual restart differs by 5 commands: the start button stays in reach behind the player while
`+use` is tapped and its refire delay (not modelled, `docs/use-rules.md`) drops some presses; the end press
lands on the same command (252).

## Importer

A `trigger_teleport` is imported as a `Teleport` when it affects clients (flag 1, not 32), starts enabled,
has no filter, no landmark and no `CheckDestIfClearForPlayer 1`, and its `target` names exactly one entity
with an origin. Anything else goes in the unsupported report. kz_longjumps_v4096 now imports without the
override (7 teleports, nothing unsupported).

## Not covered

- Landmark (relative) teleports, filters, triggers toggled by map logic, `trigger_teleport_relative`.
- Two triggers touched in one command (the first in the course's list wins here).
- The view snap's network quantization.
