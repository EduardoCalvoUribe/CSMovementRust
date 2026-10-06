# Mode notes (M7)

Plan §5.6 asks for per-mode notes written from the GOKZ mode sources (S7–S9) before coding. During
implementation the GOKZ sources could not be fetched (the download was blocked in the coding session), so
these notes are written from the reference's description of those sources [Ref §20] only. Every hook
algorithm below is therefore **our design**, chosen to hit the numbers the reference quotes. Treat them as
unverified until they are checked against GOKZ (and its license is reviewed
before any closer port) or against M-* captures.

All three modes run on the same core (`process_movement`); a mode is a `MovementConfig` preset plus hooks
(`MovementMode` trait in `crates/movement/src/modes/mod.rs`).

## Vanilla (S7)
Config only, no hooks. Values: [Ref §3] and the Vanilla column of [Ref §20].

## KZTimer (S8)
Config: accelerate 6.5, airaccelerate 100, friction 5.0, no weapon-speed scaling, zero jump/landing
stamina, stock anti-bhop off, ladder scale 1.0, velocity component limit 2000, ledge helper off [Ref §20].

Hooks (`modes/kztimer.rs`):
- **Prestrafe modifier** (reference max 1.104, i.e. 276 from 250 [Ref §20.1]). Design: while grounded,
  the modifier grows by 0.208 per second when the yaw changed since the last command and both forward and
  side input are held; otherwise it decays at the same rate to 1. It multiplies the move-data maximum
  speed on the ground (`modify_wish_speed`), so both the wish speed and the ground total-speed clamp
  [Ref §5.3] rise with it.
- **Perfect-hop cap 380** [Ref §20.1]. A jump within one grounded command of landing (`ground ticks <= 1`)
  scales horizontal velocity down to 380, at takeoff, inside the jump routine.
- **Simultaneous jump+duck** [Ref §20.1]. A grounded command with a fresh jump press and a fresh duck press
  drops the duck press, so the jump uses the standing branch.
- **Not implemented:** "additional duck-state and displacement handling" [Ref §20.1]; the reference gives no
  detail.

## SimpleKZ (S9)
Config: as KZTimer except friction 5.2 and velocity component limit 3500 [Ref §20]. 128 tick is required
(`required_tickrate`), and the app locks the tickrate when SimpleKZ is selected [Ref §20.2].

Hooks (`modes/simplekz.rs`):
- **Turning-based prestrafe bonus with growth, decay and grace** [Ref §20.2]. Design: while grounded and
  turning with side input, the bonus grows by 0.26 per second up to 0.104 (276 from 250), and a 0.1 s grace
  timer is refreshed. When not turning, the grace timer runs down first, then the bonus decays at 0.35 per
  second. `M_pre = 1 + bonus`.
- **Perf recognition**: a jump within 2 grounded commands of landing counts as a perfect hop (design).
- **Landing-speed-dependent takeoff** [Ref §20.2]: on a perfect hop, horizontal speed is capped at
  `min(V_land, (0.2 V_land + 200) * M_pre)`, where `V_land` is the horizontal speed recorded in landing
  processing. This is the reference's formula verbatim; the rest is design.
- **Not implemented:** SimpleKZ duck-speed modifications and "other corrections" [Ref §20.2].
