# Jump stats notes (M8)

How `crates/movement/src/jumpstats.rs` classifies and measures jumps. GOKZ's jump tracking (S15) is the
model the plan names, but this classification is our own simplified version, written from [Ref §16, §20.3]
without reading GOKZ code. Comparisons with GOKZ numbers need the same definitions, so differences are
listed here.

## Takeoff
A takeoff is the first command that ends airborne in walk movement after a grounded or ladder command.

| Type | Rule |
|---|---|
| LAJ (ladder jump) | The previous command was on a ladder |
| Fall | Left the ground without a jump; tracked but not reported |
| BH (bhop) | A jump within one grounded command of landing, after a reported jump |
| MBH (multi bhop) | Same, after a BH or MBH |
| WJ (weird jump) | Same, after a fall |
| LJ (long jump) | Any other jump (on the ground for more than one command, or the first jump) |

A jumpbug counts as both a landing and a takeoff on the same command.

## Landing and distance
- The jump ends on the first command that ends grounded in walk movement.
- **Raw landing**: the origin at the end of that command.
- **Corrected landing** [Ref §16]: categorization grounds the player anywhere within the 2-unit probe, so
  the raw origin can hover above the floor. First trace down from the raw origin to find the floor height,
  then move from the previous command's origin along the velocity the sweep used (start velocity, vertical
  part minus half a gravity step) until the feet reach it. MovementAPI distinguishes more cases.
- **Distance** = horizontal takeoff-to-corrected-landing displacement + 32 (except ladder jumps), the
  reporting convention for hull width [Ref §16]. The HUD also shows the value without the +32 and the raw
  final-command distance.

## Per-jump numbers
- **Pre**: horizontal speed at the start of the takeoff command. **Takeoff**: at its end.
- **Max**: highest horizontal speed in the air. **Height**: highest origin z minus takeoff z.
  **Block**: corrected landing (floor) z minus takeoff z.
- **Airtime**: commands from takeoff to landing.
- **Strafes**: a new strafe starts whenever side input changes sign (zero input doesn't start one).
- **Sync**: percentage of air commands where horizontal speed increased [Ref §16]. Per strafe: gain, loss,
  ticks and sync.
- **Perfect**: the takeoff came within one grounded command.
- **Edgebug / jumpbug / duckbug** flags come from `TechniqueDetector` internal events, not from speeds.
