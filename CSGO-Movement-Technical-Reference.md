# CS:GO movement: code, mathematics, reverse engineering, and movement techniques

Research reference • 4–5 October 2026

## Scope and evidence

This reference focuses on **CS:GO's Source 1 movement**, including the differences introduced by KZ and surf servers. CS2 appears only where it helps distinguish evidence: its current movement implementation cannot be established by reading CS:GO code.

There is no publicly released, authoritative, complete source tree for the final retail CS:GO movement implementation. There are nevertheless unusually good overlapping sources:

| Evidence | What it establishes | Limitation |
|---|---|---|
| Valve's official Source SDK 2013 | The public Source movement architecture and many inherited algorithms | It is not the retail CS:GO implementation |
| Historical public mirror of CS:GO source | Concrete implementations of CS-specific acceleration, jumping, stamina, ducking, and collision behavior | An unofficial historical source disclosure; not an authenticated final-retail release |
| click4dylan's reconstructed CS:GO movement | Independent reverse-engineered representations of movement routines | The author explicitly labels it outdated since the December 2018 update |
| MovementAPI and GOKZ | Instrumentation of actual CS:GO movement functions; exact plugin behavior and jump classification | Plugin code establishes the plugin's behavior, not automatically stock game behavior |
| RNGFix, MomSurfFix, Movement Unlocker | Research embodied in interventions that repair or replace particular engine behaviors | Their chosen fixes may deliberately change movement rules |
| Researchers' explanations, calculators, and demonstrations | Reproducible observations, hypotheses, and practical setups | Build, tickrate, map geometry, and server configuration are often incompletely specified |

The two principal historical code snapshots inspected are pinned here:

- CS:GO source mirror: commit **f82112a2388b841d72cb62ca48ab1846dfcc11c8**. This is the mirror's commit identity, **not** a claim about the corresponding retail game build.
- Reverse-engineered movement: commit **24d68f47761e5635437aee473860f13e6adbfe6b**.

Throughout this document:

- **Code** means directly established by an inspected implementation.
- **Derived** means mathematics or consequences worked out from that implementation.
- **Community** means an identified researcher's account or tool.
- **Unresolved** means the material inspected does not justify a precise implementation-level claim.

The numerical examples below were calculated independently. They are **not new in-game measurements**. Branch-specific behavior is identified instead of claiming byte-for-byte equivalence to every CS:GO build.

## 1. The player is a swept hull with a state machine

Ordinary player locomotion is not a rigid-body simulation with realistic forces, mass, and traction. It is a custom, deterministic controller that:

1. Interprets a command.
2. Changes velocity with explicit formulas.
3. Sweeps a collision hull through the world.
4. Clips velocity against collision planes.
5. Decides whether the player now counts as grounded.
6. Applies state changes, landing effects, and subsequent trigger processing.

The ordinary CS:GO standing hull is 32 × 32 × 72 units; the crouched hull is 32 × 32 × 54. The horizontal half-width is 16. The hull is axis aligned: turning the camera does not rotate this square footprint. Shooting hitboxes and the animated character model are separate representations.

Important state includes:

| State | Why it matters |
|---|---|
| Origin and velocity | The current simulated position and motion |
| Base velocity / ground entity | Moving platforms, conveyors, pushes, and inherited motion |
| Movement type | Walking/air movement, ladder, noclip, observer, etc. |
| Ground entity and ground flag | Eligibility for friction, jumping, landing, and ground speed restrictions |
| Current and previous buttons | Whether jump or crouch was newly pressed or held |
| Maximum speed and movement inputs | Weapon and movement-state-dependent wish velocity |
| Surface friction | Multiplies acceleration as well as affecting friction-related behavior |
| Stamina | A penalty accumulator affecting movement and jumping |
| Duck amount, duck speed, and duck flags | Related but distinct states; not one Boolean |
| Stored fall velocity | Used later in landing processing |
| Collision planes and remaining movement time | Determine the within-tick path after impacts |

The central distinction is:

> A collision trace hitting a floor, the player being classified as grounded, and landing effects being processed are three separate events.

Many movement techniques arise because only some of these happen during a command. [S1–S4]

## 2. Commands, ticks, and the movement pipeline

A user command contains view angles, forward/side/up movement values, buttons, a command number, and timing-related fields. Input collection, rendering, network delivery, prediction, and server simulation are related but different processes.

For ordinary CS:GO fixed-tick movement, use:

\[
\Delta t = 1/64 = 0.015625
\]

or

\[
\Delta t = 1/128 = 0.0078125.
\]

Do not substitute the render-frame duration into these formulas merely because the client renders at 300 FPS. FPS can affect when input becomes available and which command receives it; it does not simply turn a 64-tick server into a 300-Hz movement simulator.

Client prediction executes movement locally so the player responds immediately; authoritative server results can cause corrections. An accurate reproducer needs the command stream and complete state, not just a demo's rendered camera positions.

A simplified **ordinary dry-land/air** path in the inspected CS:GO source is:

    ProcessMovement / PlayerMove
        CheckParameters: restrict inputs and speed
        ReduceTimers: recover stamina and update timers
        process duck state
        detect/handle ladders
        FullWalkMove
            StartGravity: half a gravity step
            CheckJumpButton, if requested
                possibly change vertical velocity
                FinishGravity inside successful jump
                record jump stamina
            if grounded:
                zero vertical velocity and stored fall velocity
                apply ground friction
            WalkMove or AirMove
                accelerate
                trace hull and resolve collisions
            CategorizePosition
            FinishGravity: another half step
            if grounded: zero vertical velocity
            CheckFalling
        later: process impacts / trigger touches

This is deliberately not a complete transcription: water, special movement types, moving entities, stuck recovery, and optimized initial categorization have additional branches.

Two particularly important details are often absent from simplified explanations:

- **Jump processing precedes ground friction.** A successful jump can leave the ground before that command's friction and ground speed clamp.
- **Duck processing precedes the ordinary jump/landing path.** Changing the hull can create a grounded state before the main movement routine checks jump.

RNGFix also documents a plugin-hook distinction: an input-command hook is not guaranteed to correspond one-for-one to a physically simulated command when the server drops commands under overload. Instrument the movement execution itself when measuring exact behavior. [S2, S3, S5, S12]

## 3. Constants: useful defaults, not universal laws

The inspected CS:GO implementation and GOKZ's vanilla mode give the following useful reference values:

| Parameter | Reference value | Meaning |
|---|---:|---|
| Gravity | 800 units/s² | Ordinary downward acceleration |
| Jump impulse | 301.993377 units/s | Nominal jump parameter |
| Ground acceleration | 5.5 | Coefficient in the ground formula |
| Air acceleration | 12 | Coefficient in the air formula |
| Air directional wish-speed cap | 30 units/s | Limits the velocity projection along the requested direction |
| Ground friction | 5.2 | Ground deceleration coefficient |
| Stop speed | 80 units/s | Minimum control speed for friction |
| Step size | 18 units | Ordinary stair-step height |
| Walkable/standable normal threshold | 0.7 | Minimum upward normal component for ordinary support |
| Ground probe, when airborne | 2 units | Near-ground support detection |
| Velocity component limit | 3500 units/s | Per-component safeguard, not the usual running cap |
| Walk multiplier | 0.52 | Applies in the relevant walking branches |
| Duck multiplier | 0.34 | Fully crouched movement scale |
| Stamina jump cost | 0.080 | Multiplied by the jump routine's measured impulse |
| Stamina landing cost | 0.050 | Multiplied by stored falling speed |
| Stamina recovery | 60/s | Penalty reduction over time |
| Maximum stamina penalty | 80 | Clamped accumulator |
| Stamina normalization range | 100 | Denominator used by movement penalties |
| Ordinary ladder scale | 0.78 | CS-specific ladder speed scale |

“Maximum speed” is overloaded. Weapon running speed, the move-data maximum, the player's maximum-speed field, a server cvar, the air directional cap, and the absolute velocity safeguard are not interchangeable. A knife's familiar 250 units/s is not a universal cap on airborne velocity.

Read defaults together with configuration and code branches. A server can change cvars, and plugins can change behavior without changing those cvars. [S2, S3, S6–S9]

## 4. From keys to wish velocity

Let \(\mathbf f\) and \(\mathbf r\) be the horizontal, normalized forward and right vectors obtained from view orientation. Let \(F\) and \(R\) be command forward and side movement.

\[
\mathbf u = F\mathbf f + R\mathbf r
\]

\[
W_{\rm raw}=\|\mathbf u\|,\qquad
\hat{\mathbf w}=\mathbf u/W_{\rm raw}.
\]

The controller caps or scales the desired speed to obtain \(W\). Input magnitude, weapon speed, walking, ducking, stamina, and other restrictions can enter before acceleration.

This explains several practical facts:

- W+A creates a diagonal **wish direction**, not an automatic \(\sqrt2\) increase beyond the capped wish speed.
- Mouse movement matters because it rotates the world-space direction associated with A, D, W, and S.
- Camera pitch is removed from ordinary horizontal ground/air movement construction; ladder movement is an important exception.
- A and D are convenient for air strafing, but they are not privileged physics inputs. W can accelerate correctly with an appropriate view direction.
- Holding W while performing the usual A/D sideways-strafe technique changes the wish direction and can ruin that technique's alignment. “W disables air strafing” is not the underlying rule. [S2, S3, S12]

## 5. Ground friction, acceleration, and counter-strafing

### 5.1 Ground friction

For ordinary horizontal ground speed \(V\), friction behaves approximately as:

\[
D=\max(V,V_{\rm stop})\,f\,\mu\,\Delta t
\]

\[
V'=\max(0,V-D),\qquad
\mathbf v'=\mathbf v\,V'/V.
\]

Here \(f\) is the friction cvar, \(\mu\) the relevant surface-friction factor, and \(V_{\rm stop}=80\).

Above stop speed, the decay is proportional to current speed. Below it, the minimum control speed makes the remaining speed disappear more quickly than a pure exponential would.

At 250 units/s, \(\mu=1\), and \(f=5.2\), one friction step removes:

- 20.3125 units/s at 64 tick.
- 10.15625 units/s at 128 tick.

Friction is followed by acceleration. Holding the desired direction replenishes speed; releasing it does not.

### 5.2 Ground acceleration

The basic structure is projection limited:

\[
p=\mathbf v\cdot\hat{\mathbf w},\quad
r=W-p,\quad
q=\max(0,\min(r,A_g)),
\]

\[
\mathbf v_{\rm new}=\mathbf v+q\hat{\mathbf w}.
\]

But CS:GO's \(A_g\) is **not always simply**
\(sv\_accelerate \times W\times\Delta t\).
Its override builds a separate acceleration scale.

For the normal branch in the inspected code:

1. Start acceleration scale and goal speed at \(\max(250,W)\).
2. If weapon-speed scaling is enabled, compute
   \[
   k=\min(1,V_{\rm weapon}/250).
   \]
3. Multiply goal speed by \(k\).
4. Multiply acceleration scale by \(k\) when neither walking nor ducking, or when a special slow-scoped-sniper condition applies.
5. Apply duck/walk multipliers, with exceptions for that scoped condition.
6. When walking near the goal speed, taper acceleration over the last 5 units/s.
7. Form the budget from acceleration coefficient, adjusted scale, surface friction, and timestep.

The slow-scoped condition checks weapon zoom state, multiple zoom levels, and whether the weapon speed multiplied by the walking factor is below 110. This is why implementing a single generic acceleration formula can reproduce running reasonably while getting scoped or walking behavior wrong.

The source also contains an experimental exponent-based branch, but its controlling compile-time value is zero in the inspected snapshot. It should not be presented as the active default.

For a simple 250-unit/s running case:

\[
A_g=5.5(250)\Delta t,
\]

giving 21.484375 at 64 tick or 10.7421875 at 128 tick before the remaining-speed clamp.

### 5.3 The ground total-speed clamp

The inspected WalkMove path also limits the **magnitude of horizontal velocity** to the current move maximum. This is a different operation from the directional projection test.

That distinction matters enormously:

- Air acceleration can increase total speed beyond the ordinary run speed.
- A subsequent ground movement command can remove that excess.
- Movement Unlocker and movement-mode plugins can alter this behavior.
- A KZ prestrafe above 250 does not establish that stock CS:GO has the same unrestricted ground acceleration as another Counter-Strike game.

### 5.4 Counter-strafing

Suppose the player moves right. Pressing left requests acceleration opposite the existing velocity. Friction already reduces rightward speed; opposite acceleration removes additional rightward velocity until the player stops and then starts moving left.

It is not a discrete “stop” command. The exact stop time depends on initial velocity, weapon, state, timestep, and input transition.

Weapon accuracy is a separate system that consumes movement-related state. There is no single universal counter-strafe delay that can be derived from movement alone for every gun and firing state.

Tagging is also separate from stamina. The inspected movement code applies a velocity modifier on the ground and recovers it toward one; conflating all slowdown into one “friction” parameter produces incorrect results. [S2, S3, S10]

## 6. Air strafing: exact core mathematics

This is the most important small algorithm.

Let:

- \(\mathbf v\): horizontal velocity.
- \(\hat{\mathbf w}\): unit wish direction.
- \(W\): requested speed after applicable input restrictions.
- \(C=\min(W,30)\): capped directional wish speed.
- \(a\): air-acceleration coefficient.
- \(\mu\): stored surface-friction factor.
- \(A=aW\mu\Delta t\): acceleration budget.
- \(p=\mathbf v\cdot\hat{\mathbf w}\): velocity already along the wish direction.

Then:

\[
q=\max\left(0,\min\left(A,C-p\right)\right)
\]

\[
\boxed{\mathbf v_{\rm new}=\mathbf v+q\hat{\mathbf w}}
\]

Original explanatory pseudocode:

    air_accelerate(velocity, wish_direction, wish_speed, dt):
        directional_limit = min(wish_speed, 30)
        already_along_wish = dot(velocity, wish_direction)
        room = directional_limit - already_along_wish
        if room <= 0:
            return velocity

        budget = air_accel * wish_speed * surface_friction * dt
        amount = min(budget, room)
        return velocity + amount * wish_direction

The famous mismatch is that **C uses capped wish speed while A uses uncapped wish speed**. However, the deeper source of strafe acceleration is the projection-based limit. Fixing that mismatch reduces or changes gains; it does not by itself turn the algorithm into a total-speed cap.

Also, 30 is not an unconditional maximum velocity increment. If \(p\) is negative, \(C-p\) can exceed 30. “The air cap is 30” needs this qualification.

### 6.1 Why sideways input increases speed

If \(p=0\), the added velocity is perpendicular to existing velocity:

\[
V_{\rm new}^2=V^2+q^2.
\]

At \(V=250\) and \(q=30\):

\[
V_{\rm new}=\sqrt{250^2+30^2}\approx251.793566.
\]

You did not gain 30 units/s of scalar speed. You added a 30-unit/s vector and gained about 1.794 units/s in its magnitude.

Afterward the velocity direction has rotated. Continuing to gain efficiently requires rotating the wish direction with it. That is why the player turns the mouse while holding a strafe key.

### 6.2 Deriving the maximum one-step speed gain

For arbitrary projection \(p\):

\[
V_{\rm new}^2=V^2+2qp+q^2.
\]

There are two regions:

- Full budget: \(p\le C-A\), so \(q=A\).
- Directionally limited: \(C-A<p<C\), so \(q=C-p\).

In the second region:

\[
V_{\rm new}^2-V^2=C^2-p^2.
\]

For \(V>0\), \(A>0\), the maximum one-step speed magnitude occurs at:

\[
p_*=\operatorname{clamp}(C-A,0,V).
\]

Thus the optimum angle between velocity and wish direction is:

\[
\theta_*=\arccos(p_*/V).
\]

This optimizes **speed after one unconstrained air step**, not landing position, route time, wall contact, or a complete jump.

For the illustrative state \(V=W=250\), \(a=12\), \(\mu=1\):

| Quantity | 64 tick | 128 tick |
|---|---:|---:|
| Budget \(A\) | 46.875 | 23.4375 |
| Optimum projection \(p_*\) | 0 | 6.5625 |
| Optimum wish angle | 90° | 88.495813° |
| One-step scalar speed gain | 1.793566 | 1.708032 |
| Resulting velocity-direction turn | 6.842773° | 5.340923° |

These are hypothetical isolated states, not an assertion that a real stock jump maintains \(W=250\). Stamina and ducking often reduce it.

### 6.3 Steering versus gaining speed

Very sharp turns can require braking one velocity component to redirect the player. High “sync” or high speed does not guarantee a good trajectory.

For a long jump, the objective is approximately:

\[
\mathbf x_{\rm land}-\mathbf x_{\rm takeoff}
=\sum_i \mathbf v_{h,i}\Delta t_i,
\]

subject to collision and landing constraints. Growing speed while turning too far away from the landing block can reduce useful displacement.

Mouse sensitivity is not a physics constant. It controls the mapping from physical mouse displacement to view-angle changes, which then controls wish direction.

### 6.4 Tickrate

The timestep appears inside the budget, but clamps and discrete collision decisions make the full system nonlinear. Doubling tickrate does not simply double or halve useful acceleration. It changes the number of steering opportunities, which side of the acceleration clamp applies, the timing of ground classification, and collision phase. [S1–S4; derivations above]

## 7. Deadstrafe: “surface friction” while airborne

In the relevant CategorizePosition path, a player without ground support and with upward velocity in the range

\[
0<v_z\le140
\]

can have surface friction set to 0.25 instead of 1.

Air acceleration multiplies by this value. Therefore its available budget becomes one quarter as large.

At the illustrative \(W=250\), \(a=12\):

| State | 64-tick budget | 128-tick budget |
|---|---:|---:|
| Ordinary \(\mu=1\) | 46.875 | 23.4375 |
| Deadstrafe \(\mu=0.25\) | 11.71875 | 5.859375 |

This is not a general reduction in all velocity to one quarter. It reduces acceleration capacity.

The factor is stored during categorization and consumed by movement processing; the exact first and last affected commands depend on call ordering. A reimplementation that selects friction solely from the vertical velocity at the start of its air-acceleration function can be a command out of phase.

This gives a concrete explanation for the weak turning feel during part of a jump. The researcher zer0k independently explains and links the responsible code paths. [S2, S11]

## 8. Stamina: why consecutive jumps differ

CS:GO stamina is best understood as a **penalty accumulator**, \(S\), rather than remaining energy.

In the inspected code:

\[
S_{\rm recover}=\max(0,S-60\Delta t)
\]

\[
S_{\rm after\ jump}
=\operatorname{clamp}(S+0.080I,0,80)
\]

\[
S_{\rm after\ land}
=\operatorname{clamp}(S+0.050F,0,80),
\]

where \(I\) is the impulse quantity passed by the jump routine and \(F\) is stored falling velocity used for landing.

Relevant movement-speed scaling is:

\[
M_{\rm speed}=(1-S/100)^2,
\]

while the jump's vertical velocity is multiplied by:

\[
M_{\rm jump}=\operatorname{clamp}(1-S/100,0,1).
\]

Consequences:

- S = 20 gives a movement factor of 0.64, not 0.8.
- It gives a jump-velocity factor of 0.8; the ideal ballistic height contribution then scales roughly with its square.
- Jump cost is not “8% of the stamina maximum.” It is 0.080 times a velocity/impulse quantity.
- Landing cost depends on falling speed, not just whether a landing happened.
- A perfect bhop can avoid a ground-friction step while still having landing stamina and jump stamina.
- Reduced maximum wish speed also reduces the uncapped \(W\) in the air-acceleration budget.
- A jumpbug can avoid the ordinary landing event but still execute jump cost.

Call order matters: speed checking and stamina recovery do not necessarily sample \(S\) at the same point in the command. Recompute state at the same stages as the engine when reproducing it.

For an unpenalized normal takeoff, the jump-cost impulse in the inspected branch is approximately \(J-g\Delta t/2\); in the crouch-reset branch it is approximately \(J\). Both are near 300, producing about 24 penalty units. This is why disabling stamina on a KZ server makes a large difference even when gravity and nominal jump impulse are unchanged. [S3, S7–S9]

## 9. Jumping and the 57-unit misconception

The familiar calculation is:

\[
J=\sqrt{2gh}
=\sqrt{2(800)(57)}
\approx301.993377.
\]

But **57 is not the maximum sampled height of every ordinary CS:GO jump**.

The inspected jump routine behaves differently by state:

- Ordinary standing jump: add \(J\) to existing vertical velocity.
- Ducking/ducked jump, and some player-support cases: assign the vertical velocity from \(J\).
- Apply stamina scaling.
- Call FinishGravity inside the jump routine.
- The surrounding movement routine also performs its gravity steps.

Let \(h_g=g\Delta t/2\), with no stamina, no base velocity, ordinary jump surface factor, and initial rest on flat ground.

| Stage | Ordinary standing branch | Duck/reset branch |
|---|---:|---:|
| After initial half gravity | \(-h_g\) | \(-h_g\) |
| After adding/assigning jump | \(J-h_g\) | \(J\) |
| Velocity used in first movement sweep | \(J-2h_g\) | \(J-h_g\) |

After the command's final half gravity, subsequent position steps follow their respective discrete trajectories.

Ignoring contact offsets and floating-point effects, the standing branch gives:

\[
z_n-z_0=nJ\Delta t-\frac12g\Delta t^2n(n+1).
\]

Equivalently, its sampled positions lie on:

\[
z(t)-z_0=(J-h_g)t-\frac12gt^2.
\]

The reset branch gives:

\[
z_n-z_0=nJ\Delta t-\frac12g(n\Delta t)^2.
\]

Calculated maximum sampled displacements:

| Branch | 64 tick | 128 tick |
|---|---:|---:|
| Ordinary standing, no stamina | 54.653766 | 55.825641 |
| Duck/reset, before any hull-origin adjustment | 56.997516 | 56.997516 |

These are displacements relative to the simulated starting origin in the stated model, not universal maximum ledge heights.

Ground separation, hull changes, collision tolerances, stamina, and server modifications explain why practical mapping tables contain nearby but different numbers. Valve itself later documented that CS:GO jump height could vary by up to 0.03125 unit with distance from the map origin.

That last observation is strong evidence against presenting a long decimal as a universal empirical threshold. [S2, S3, S20]

## 10. Crouch jumping, minijumps, and crouch fatigue

### 10.1 Hull geometry

Standing height is 72 and crouched height is 54: an 18-unit difference.

When changing hull in the air, the inspected implementation moves the origin by half this difference to keep the hull's center aligned:

- Finish airborne duck: feet/origin move upward by 9.
- Finish airborne unduck: feet/origin move downward by 9.

Grounded transitions use different origin handling to preserve support.

The improvement in ledge clearance therefore is not simply “crouching adds 18 units of jump height.”

### 10.2 Two independent benefits

Crouch jumping can combine:

1. The duck/reset jump branch, which avoids the standing branch's initial vertical deficit.
2. A later airborne hull-origin shift raising the feet by 9 units.

Which benefits occur depends on the exact starting duck state and input sequence.

Examples:

- Jump standing, then crouch airborne: gain the hull clearance, but the jump already used the standing launch branch.
- Begin ducking and jump at the appropriate transition: the ducking flag can select the reset branch before the full airborne hull transition.
- Start fully crouched: the smaller hull is already in use, so there is no new +9 shift merely from taking off.
- Jump fully crouched and then stand airborne: lowering the feet by 9 gives a low-clearance/minijump-style trajectory, roughly the 57-minus-9 geometry before discrete and state effects.

Unducking is conditional: CanUnduck traces the proposed standing hull and origin. The engine does not blindly expand it through a floor or ceiling.

### 10.3 Duck state is not just a key

Duck amount, actual hull state, animation flags, and transition flags can disagree during a transition. The jump routine checks more than one of them.

The CS-specific input crop uses duck fraction \(d\) to form \(M_{duck}=1-0.66d\). It scales movement inputs and the move maximum, including in the air. At full duck, a nominal 250 becomes 85 before other applicable factors. Thus crouching can improve clearance while reducing the air-acceleration budget; existing airborne velocity is not simply multiplied by 0.34 on every command.

The inspected implementation penalizes duck speed on press/release transitions, recovers it over time, and restricts repeated ducking. This is the basis of crouch fatigue/spam restrictions. It is separate from jump stamina.

KZ plugins may remove or alter these restrictions, and some specifically suppress simultaneous grounded jump+duck inputs. Therefore a bind or sequence that reaches a particular height in one mode need not work in another. [S3, S7–S9]

## 11. Bunny hopping

Bhop combines three mechanisms:

1. Leave the ground before a normal ground-movement command removes speed.
2. Use air acceleration while airborne.
3. Repeat with a valid jump-button transition.

Stock jump handling ordinarily requires jump to have been released since the previous press. An airborne jump press can consume the button transition even though no jump occurs. Holding jump continuously is consequently different from auto-bhop provided by settings or plugins.

Scrolling gives multiple opportunities to place a fresh press on the appropriate command. It does not instruct the engine to preserve speed directly.

### 11.1 Why the perfect landing command matters

Once a command ends grounded, a successful jump at the beginning of the next movement command can clear support before ground friction and WalkMove's total-speed cap.

If a command instead runs ordinary ground movement, speed can be reduced by both friction and the move-speed limit.

### 11.2 Stock anti-bhop restriction

The inspected PreventBunnyJumping routine uses:

\[
V_{\rm allowed}=1.1\,(\text{player maximum-speed field})
\]

and rescales velocity when its **3D magnitude** exceeds that value.

This is not automatically \(1.1\times\) the active weapon's 250-unit/s speed, because the player field and the movement-data/weapon speed are different quantities. If the relevant player field is 260, the threshold is 286.

The routine is conditional on the bunnyhopping setting. Unlimited bhop servers often disable this restriction and add other changes.

### 11.3 Important qualifications

- A perfect bhop does not mean there was no landing classification.
- It does not necessarily avoid stamina.
- The relevant cap can be applied at takeoff rather than continuously in the air.
- “Bhop is random” often means the landing-command phase is hard to control. The movement code is not necessarily drawing a random number.
- A universal 50% success ceiling is not a general theorem about skilled, timed bhops; simplified periodic-scroll models have narrower assumptions. [S2, S3, S5, S7–S9]

## 12. Collision resolution and stepping

### 12.1 Sweeps, not just endpoint overlap

The controller traces the player hull along a proposed displacement. It finds a collision fraction, advances to the contact, changes velocity, and can continue for the remaining portion of the command.

This is why “the game only checks the final position each tick” is an inadequate explanation. Thin solid objects and thin trigger volumes also do not necessarily behave alike.

The inspected TryPlayerMove has a small fixed collision-iteration budget—four bumps—and keeps up to five collision planes.

### 12.2 Clipping against a plane

For a stationary plane with unit normal \(\mathbf n\), the ideal non-bouncing projection is:

\[
\mathbf v'=\mathbf v-(\mathbf v\cdot\mathbf n)\mathbf n.
\]

This removes the component into the plane and preserves tangential motion.

The real routine also includes numerical correction when the result still points into the plane. Thus the ideal projection explains behavior but is not the entire floating-point implementation.

With multiple planes, the solver tries a velocity satisfying the accumulated constraints. With two suitable planes, motion can be restricted to their crease:

\[
\mathbf c=\frac{\mathbf n_1\times\mathbf n_2}
{\|\mathbf n_1\times\mathbf n_2\|},
\qquad
\mathbf v'=(\mathbf v\cdot\mathbf c)\mathbf c.
\]

No feasible motion, an all-solid trace, or an oscillation-prevention condition can stop the player.

### 12.3 Stairs

StepMove compares an ordinary sliding route with an alternative that moves up, travels, then moves down onto support. It selects based on horizontal progress while preserving particular velocity components from the alternative computations.

A step is therefore not a hidden jump impulse. Whether the step route is even attempted depends on the surrounding movement state and obstruction.

### 12.4 Ground classification

Ordinary support uses a downward trace and an upward-facing normal threshold near 0.7. Additional checks include upward velocity, moving support, fallback traces, and whether the player was already grounded.

A previously grounded walking player can receive a longer downward probe involving step size. Do not apply the airborne 2-unit probe as a universal description of all ground snapping.

A successful support decision can zero vertical velocity without the movement sweep ever physically colliding with that surface. [S1–S4]

## 13. Surfing and slope boosts

### 13.1 The geometry of a surf ramp

Let the ramp rise at angle \(\alpha\) above horizontal. Its upward normal component is:

\[
n_z=\cos\alpha.
\]

The 0.7 support threshold corresponds to:

\[
\alpha=\arccos(0.7)\approx45.573^\circ.
\]

A steeper ramp is not ordinarily treated as walkable ground. The player remains in air movement while collision clipping keeps motion tangent to the ramp.

Gravity contributes downhill motion after projection. Input into the ramp maintains contact; air strafing changes the tangential trajectory. Contact need not invoke ordinary ground friction.

A shallower ramp can also behave like a slide while the player is excluded from grounding by upward velocity. “Only surfaces steeper than 45.573° can ever be surfed” is too strong.

### 13.2 Where the speed comes from

An ideal stationary-plane projection alone does not increase total kinetic speed:

\[
\|\mathbf v'\|^2
=\|\mathbf v\|^2-(\mathbf v\cdot\mathbf n)^2.
\]

But it can turn downward velocity into horizontal velocity. Gravity supplies energy during descent, and player air acceleration can add speed.

For a ramp rising along \(x\), choose
\(\mathbf n=(-\sin\alpha,0,\cos\alpha)\). Then:

\[
v'_x=v_x\cos^2\alpha+v_z\sin\alpha\cos\alpha.
\]

This directly shows the coupling between vertical and horizontal components. Signs determine whether the projection helps downhill travel or harms uphill travel.

### 13.3 Why slope landings can vary

There are two distinct outcomes near a shallow slope:

- Actual sweep collision: velocity is clipped by the slope.
- End-of-command near-ground classification: support is recognized and vertical velocity is zeroed without the same projection.

The second route can lose a downhill conversion benefit or preserve horizontal velocity that an uphill collision would remove.

RNGFix intervenes in these cases. Its presence on a server is therefore part of the movement specification.

### 13.4 Ramp bugs and surf fixes

A visible smooth ramp need not produce an identical collision-plane sequence across seams, bevels, or adjacent brushes. Numerical trace results and the limited plane/bump solver can generate an unwanted stop or deflection.

MomSurfFix replaces movement collision handling to mitigate such failures. It is useful primary implementation evidence, but “all ramp bugs have one cause” is not supported.

Specify map collision geometry and the fix plugin version when reproducing a surf problem. [S2, S13, S14; geometric derivations above]

## 14. Edgebugs

An edgebug is a within-command event:

1. Fall toward a walkable surface near its edge.
2. The movement sweep hits it and removes the downward velocity component.
3. Horizontal movement continues for the remaining fraction of the command.
4. The hull travels beyond support.
5. Final categorization finds no ground.
6. Ordinary grounded landing processing does not run.

This connects fall-damage avoidance to state ordering. It does not require the ground entity to remain set at the command boundary.

For a simplified horizontal ledge, after a collision at fraction \(f\):

\[
d_{\rm remaining}\approx V_h(1-f)\Delta t.
\]

To edgebug, this remaining displacement must carry the hull out of support. The 16-unit half-width matters: use the hull's support geometry, not just whether the center crossed the visible edge.

After a flat collision removes \(v_z\), final half gravity can leave:

- \(-6.25\) units/s at 64 tick.
- \(-3.125\) units/s at 128 tick.

These are useful diagnostic signatures, not sufficient universal detectors. Other states and collision sequences can produce similar values.

MovementAPI detects the event around internal movement and categorization hooks, which is much stronger evidence than an explanation based only on a final velocity screenshot. [S2, S5]

## 15. Duckbugs and jumpbugs

These exploit hull transition and ground classification **before** the main landing path.

### 15.1 Duckbug

A crouched falling player releases duck. Airborne unducking lowers the feet by 9 units. If the standing hull fits and the ground probe now recognizes support, the player can become grounded during duck processing.

The subsequent ordinary ground path clears stored fall velocity before later falling checks. That ordering can avoid the normal landing consequences.

It is not merely that crouching absorbs damage.

### 15.2 Jumpbug

A jumpbug adds a valid jump request to that temporary support state:

1. Fall crouched.
2. Unduck at the correct height and command.
3. Duck processing establishes support.
4. Jump processing uses it immediately.
5. The main routine proceeds airborne.
6. Normal grounded landing processing is bypassed.

A before/after command logger may show airborne at both ends and miss the intermediate grounded state. Internal hooks are needed to see the cause.

### 15.3 Why the window is narrow

The idealized geometry combines a 9-unit foot shift with an approximately 2-unit ground probe. For a simple unobstructed horizontal floor, this suggests a narrow range around 9–11 units of pre-unduck clearance.

This is a geometric estimate, not an exact inclusive interval across all builds and coordinates. Trace clearance, tolerances, previous state, and collision timing matter.

At falling speed \(F\), crossing a 2-unit vertical band takes roughly:

\[
\Delta t_{\rm band}\approx2/F.
\]

At 1000 units/s, that is about 2 ms—shorter than either ordinary CS:GO tick interval. A discrete command must land in a suitable state; late visual reaction alone is unreliable.

Jumpbugs still execute the jump routine and its restrictions/costs. They should not be described as a free, arbitrary upward impulse. [S2, S3, S5]

## 16. Distbug and misleading jump distances

The distance bug concerns **measurement**, not a new acceleration force.

A player can become grounded while still above the floor, or collide and slide during the remaining part of the landing command. Using the final command origin as the exact physical landing point produces inconsistent jump distances.

MovementAPI's corrected landing-origin calculations distinguish these cases and reconstruct or extrapolate a more consistent point.

GOKZ's jumpstats then apply additional conventions. For most non-ladder jumps, the inspected calculation adds 32 units to horizontal takeoff-to-landing displacement. That is a reporting convention associated with the hull width; it is not an extra 32 units of center travel generated by physics.

Other metrics also need definitions:

- “Sync” can mean the fraction of measured air ticks with increasing horizontal speed.
- Per-strafe gain, overlap, dead air, and efficiency depend on the plugin's classification.
- Block distance and free jump distance are different quantities.
- Different landing heights change airtime and attainable displacement.
- A high sync percentage does not prove maximum acceleration or a good route.

Comparisons require the same timer, mode, jump type, and correction logic. [S5, S6, S15]

## 17. Wall strafing

An airborne player pressing partly into a wall can continue accelerating tangentially while collision resolution removes the into-wall component.

This is not the same as unconstrained free-air strafing, because part of the added vector is discarded.

A useful **derived ideal model**: let the player move along a straight wall with tangential speed \(V\), and let the wish direction have tangential component \(c>0\) plus a component into the wall. Then:

\[
p=Vc,\qquad
q=\min(A,C-p),\qquad
\Delta V_{\parallel}=qc=\frac{qp}{V}.
\]

When the directional limit is the active constraint and \(A\ge C/2\), this is maximized at:

\[
p=C/2,\qquad
\Delta V_{\parallel}=\frac{C^2}{4V}.
\]

For \(C=30,V=250\), that ideal increment is 0.9 units/s per step.

This calculation assumes consistent contact with one stationary vertical wall, no ground, no corners, and no extra state changes. It explains why the angle optimal against a wall differs from the free-air maximum-speed angle.

Do not transfer GoldSrc ground-wallrunning explanations directly to stock CS:GO; its ground speed clamp and acceleration override are relevant differences. [S2, S3; original derivation]

## 18. Ladders: separate movement mathematics

Ladder movement uses view orientation differently from ordinary horizontal movement. Looking up or down can change motion.

For ladder normal \(\mathbf n\) and world up \(\mathbf z\), define:

\[
\mathbf e=
\frac{\mathbf z\times\mathbf n}{\|\mathbf z\times\mathbf n\|},
\qquad
\mathbf u_L=\mathbf n\times\mathbf e.
\]

A desired velocity \(\mathbf d\) is decomposed into its normal component
\(p=\mathbf d\cdot\mathbf n\) and the component along the ladder plane. The controller maps movement into the ladder into motion along its vertical direction, with CS-specific damping and scaling.

Consequently:

- View pitch and yaw affect climbing.
- Combining direction keys and camera angle can climb faster than a simple forward-facing input.
- There is no sound universal “fast ladder speed” without specifying view/input state and mode.
- Walking or ducking can change climb behavior.
- Leaving a ladder and pressing jump while attached are different events.

In the inspected CS-specific code, ladder detection distance is 2 units when entering and 10 units while already attached. This hysteresis is relevant to movement near a ladder's boundary.

A newly entered ladder sets an approximately 0.2-second jump-ignore interval. The ordinary detach-jump branch assigns an outward velocity of \(270\mathbf n\); it does not simply add a standard ground-jump impulse.

The base movement includes a ledge-catch helper with additional geometry and motion requirements. GOKZ modes can turn that helper off.

MovementAPI explicitly distinguishes climbing off a ladder from a jump detachment, explaining why ladder jump and ladder hop categories should not be inferred from the word “jump” alone.

For named techniques such as **Danvari tech, Vesq tech, and ladder glide**, the inspected public material establishes that specialized ladder/transition setups exist. However, I did not retrieve a sufficiently complete primary technical account to attribute every named technique to one exact branch. The underlying attachment hysteresis, jump lockout, checkpoint behavior, and mode modifications are candidates to instrument—not interchangeable proven explanations. [S2, S3, S5, S7–S9]

## 19. Pixel surfing and tiny collision features

“Pixel surf” describes practical setups around very small collision features or boundary conditions. It should not be assumed to mean a conventional steep surf plane one screen pixel wide.

The public Pixurf calculator contains discrete height lists, tickrate labels, and adjusted eye/hull-height constants. It is evidence of systematic community setup research, not an engine implementation.

The accompanying public explanation contains theoretical claims about particular contact and height conditions. I cannot promote every claim there to verified engine behavior without the relevant BSP collision geometry and a trace log.

A precise investigation must identify:

1. The actual colliding brush faces or model triangles.
2. Hull position, not just camera height.
3. Tickrate and vertical phase.
4. Normal vectors returned by each trace.
5. Whether the player is grounded, sliding, or repeatedly hitting an edge.
6. Any plugins modifying collision handling.

Very small floating-point and trace-separation effects can matter. Valve's acknowledgement of coordinate-dependent CS:GO jump-height variation supports the need for coordinate-specific measurement, but it does not prove a particular pixel-surf theory.

Do not hardcode a calculator's adjusted “72.03” or “64.09” as the actual standing hull or universal eye height. Those values can include measurement conventions and clearance offsets. [S16, S17, S20]

## 20. KZ modes are different controllers

The following comparison is from the inspected **GOKZ mode implementations**, not a claim about every server bearing a similar name.

| Setting/behavior | GOKZ Vanilla | GOKZ KZTimer mode | GOKZ SimpleKZ |
|---|---:|---:|---:|
| Ground acceleration | 5.5 | 6.5 | 6.5 |
| Air acceleration | 12 | 100 | 100 |
| Friction | 5.2 | 5.0 | 5.2 |
| Weapon-speed acceleration scaling | Enabled | Disabled | Disabled |
| Jump/landing stamina costs | Enabled | Zero | Zero |
| Stock anti-bhop restriction | Enabled | Disabled | Disabled |
| Ladder speed scale | 0.78 | 1.0 | 1.0 |
| Absolute component velocity limit | 3500 | 2000 | 3500 |
| Ledge helper | Enabled | Disabled | Disabled |
| Added prestrafe system | No KZT/SKZ bonus | Yes | Yes |
| Custom perfect-bhop handling | Vanilla-style | Yes | Yes |

“Vanilla” here means a plugin mode intended to preserve the relevant vanilla movement rules. It is still running in a KZ server environment.

### 20.1 KZTimer mode

The inspected GOKZ KZTimer mode contains:

- A prestrafe velocity modifier with reference maximum 1.104, corresponding to 276 from 250.
- A perfect-hop speed cap of 380.
- Additional duck-state and displacement handling.
- A routine suppressing a fresh simultaneous grounded jump+duck input in the relevant condition.

These are programmed game-mode choices. The numbers 276 and 380 are not universal Source constants.

### 20.2 SimpleKZ

SimpleKZ's inspected implementation explicitly requires 128 tick. It has:

- A custom turning-based prestrafe bonus.
- Bonus growth and decay, with a grace interval.
- Modified recognition and treatment of perfect hops.
- Landing-speed-dependent takeoff adjustment.
- Duck-speed modifications and other corrections.

For example, its high-speed takeoff adjustment includes a rule built around:

\[
\min(V_{\rm landing},(0.2V_{\rm landing}+200)M_{\rm pre}),
\]

in the relevant branch. Replacing that with a generic “380 cap” fails to reproduce the mode.

### 20.3 Named KZ techniques

| Technique/category | Technical interpretation or evidence status |
|---|---|
| Long jump | Optimize takeoff state, airtime, wish-direction sequence, and landing geometry |
| Single/multi bhop | Repeated landing/takeoff handling; exact limits depend on mode |
| Weird jump | A jumpstats category involving a drop into a hop; classification is plugin-defined |
| Ladder jump/hop | Distinct ladder-exit paths and plugin classifications |
| Bhop ups / elevated final blocks | Geometry and landing-height constraints interact with repeated hop state |
| Kurouching | Community crouched-hop setup exploiting the contrast between retained hop speed and a missed hop's crouched ground limit; exact usefulness is mode dependent |
| Count jump | Must be tied to the specific timer/game implementation; do not assume a GoldSrc explanation automatically applies to CS:GO |
| Our Father / WAD / prekeep / related named prestrafes | Need the relevant custom prestrafe implementation and a precise input sequence; names alone do not identify an engine primitive |
| Danvari / Vesq | Specialized ladder/setup techniques; precise primary implementation evidence remains incomplete here |
| Jumpbug / edgebug | Engine state-ordering phenomena, though plugins can change them |

A player-authored technique catalogue was useful for finding names, but several of its broad historical or cross-mode claims should not substitute for code verification. [S6–S9, S18]

## 21. Triggers, telehops, boosts, and player support

Movement and trigger processing are not a single continuous event stream.

A thin teleport trigger can be crossed during movement, followed by a solid collision that changes velocity, before trigger handling teleports the player. A telehop fix may restore velocity representing the no-collision trajectory.

A player can also be classified as standing above a thin floor trigger without their hull touching that trigger. Repeated hopping can then avoid a trigger that a mapper expected to activate. This follows from the difference between ground probing and actual hull overlap.

Base velocity is another important distinction. Some environmental motion is added for movement and removed afterward, rather than permanently merged into the player's own velocity. A fix that combines the two incorrectly can double-count a push.

For player boosts, the jump code has explicit player-support cases. Standing on a falling player can suppress the ordinary upward launch, while other player-support states use different jump handling. Moving-ground/base-velocity transitions also matter.

Therefore:

- A static two-player boost is partly a support/hull geometry problem.
- A runboost requires analysis of moving support and takeoff state.
- It is unsafe to model every runboost as simply adding two independently chosen run speeds.
- Grenade/projectile boosts require the specific projectile, damage, and collision code.

### Rocket jumping

TF2 rocket jumping is not a native CS:GO weapon mechanic. It requires a game-specific explosion knockback implementation in addition to the shared movement controller. Once an impulse is applied, air acceleration, gravity, and collision rules determine the subsequent trajectory.

A generic expression such as \(\Delta\mathbf v = \mathbf J/m\) can describe an impulse conceptually, but it is not evidence that CS:GO or TF2 implements every explosion using that exact physical formula. A CS:GO rocket-jump plugin must be researched as its own implementation. [S2, S3, S13]

## 22. What a faithful movement reproducer needs

A minimal strafing demo can implement the air formula in a few lines. A faithful CS:GO movement simulator needs substantially more:

1. Correct command construction and button history.
2. CS-specific maximum-speed and acceleration rules.
3. Stamina sampled and updated at the correct stages.
4. Surface-friction state across commands.
5. Split gravity and the extra jump-routine gravity application.
6. Distinct normal and duck/reset launch branches.
7. Duck amount, hull transitions, origin shifts, and obstruction checks.
8. Swept axis-aligned hull collision against actual collision geometry.
9. Multi-plane clipping, remaining-time movement, and numerical corrections.
10. Ground classification, step probing, and support entities.
11. Stored fall velocity and landing event ordering.
12. Ladder attachment, conversion, detachment, and cooldown state.
13. Base velocity, moving entities, and subsequent trigger processing.
14. All server cvars and movement-altering plugins.

For build-accurate reverse engineering, log at least:

    command number and simulation time
    origin and velocity before/after each relevant routine
    movement inputs, angles, current/old buttons
    ground entity and flags
    move maximum speed and weapon state
    surface friction and stamina
    duck amount, duck speed, duck/transition flags
    stored fall velocity
    every movement trace:
        start, end, fraction, startsolid/allsolid,
        plane normal, hit entity, remaining time

Useful internal observation points include CheckJumpButton, Duck, AirAccelerate, WalkMove, TryPlayerMove, CategorizePosition, SetGroundEntity, and landing processing. MovementAPI provides a concrete example of this approach.

A measurement made only at the start and end of a tick can miss the event that explains a jumpbug, edgebug, or ladder transition. Rounded HUD values are inadequate for sub-unit thresholds.

This research compared implementations and checked selected formulas numerically. It did not run a retail CS:GO binary, attach a debugger, or validate every technique on a particular map.

## 23. Corrections to common explanations

| Common explanation | More precise statement |
|---|---|
| “Air speed is capped at 30.” | A projection along the wish direction is capped; total horizontal speed is not capped there |
| “Air strafing adds 30 speed every tick.” | It adds a vector whose magnitude is branch-dependent; scalar speed gain is much smaller in ordinary sideways strafing |
| “A/D are special acceleration keys.” | All movement inputs become a world-space wish direction |
| “Crouch adds 18 jump units.” | Airborne hull changes shift the feet by 9; jump branch selection is another effect |
| “The jump impulse means every jump is 57 units.” | Normal launch gravity ordering reduces sampled standing-jump height |
| “A perfect bhop avoids all landing penalties.” | It can avoid ground friction/capping while retaining stamina-related effects |
| “Edgebug means the player never touched ground.” | A collision occurs; final grounded classification does not |
| “Jumpbug is just a perfectly timed normal bhop.” | Duck processing creates an intermediate support state before jump processing |
| “Slope boosts are random.” | Many are deterministic consequences of collision phase versus support probing |
| “KZ movement is stock with higher air acceleration.” | Modes also modify stamina, prestrafe, ducking, hop recognition, ladder settings, and sometimes collision behavior |
| “The SDK is the exact final CS:GO code.” | It establishes the lineage; CS-specific and later retail behavior require other evidence |
| “The same math proves current CS2 behavior.” | Similar concepts are useful, but CS2 needs build-specific implementation and timing evidence |

## 24. Annotated primary-source reading list

The references below link the actual implementations or authors' accounts, except the historical CS:GO source (S2, S3, S12, S21), an unofficial leak that is cited by file but not linked. Repositories should be pinned to a commit before using them as a reproducibility specification.

### Actual code and reconstructed code

**S1. Valve, Source SDK 2013 — movement implementation.** Official public baseline. Useful for architecture, clipping, stepping, friction, and air acceleration. It is not CS:GO's complete controller.  
https://github.com/ValveSoftware/source-sdk-2013/blob/master/src/game/shared/gamemovement.cpp

**S2. Historical CS:GO source (unofficial leak) — shared movement, `game/shared/gamemovement.cpp`.** Read AirAccelerate, AirMove, WalkMove, TryPlayerMove, ClipVelocity, StepMove, CategorizePosition, FullWalkMove, CheckFalling, and LadderMove.  
Unofficial leak of the CS:GO source; not linked.

**S3. Historical CS:GO source (unofficial leak) — CS-specific movement override, `cs_gamemovement.cpp`.** The most important file for avoiding generic-Source mistakes. Read CheckParameters, Accelerate, CheckJumpButton, PreventBunnyJumping, OnJump, OnLand, Duck, FinishDuck, FinishUnDuck, and CanUnduck.  
Unofficial leak of the CS:GO source; not linked.

**S4. click4dylan, CSGO_GameMovement_Reversed.** Independent reverse-engineering artifact; repository description explicitly warns of its age.  
https://github.com/click4dylan/CSGO_GameMovement_Reversed/tree/24d68f47761e5635437aee473860f13e6adbfe6b  
https://github.com/click4dylan/CSGO_GameMovement_Reversed/blob/24d68f47761e5635437aee473860f13e6adbfe6b/IGameMovement.cpp

### Instrumentation and exact game-mode implementations

**S5. DanZay and contributors, MovementAPI.** Particularly useful for internal movement events, duckbug/jumpbug/edgebug detection, and corrected landing origins.  
https://github.com/danzayau/MovementAPI  
https://github.com/danzayau/MovementAPI/blob/master/addons/sourcemod/scripting/movementapi/hooks.sp  
https://github.com/danzayau/MovementAPI/blob/master/addons/sourcemod/scripting/movementapi/stocks.sp

**S6. KZGlobalTeam, GOKZ.** Treat modes and jumpstats as distinct implementation layers.  
https://github.com/KZGlobalTeam/gokz

**S7. GOKZ Vanilla mode.** Reference configuration and mode-specific interventions.  
https://github.com/KZGlobalTeam/gokz/blob/master/addons/sourcemod/scripting/gokz-mode-vanilla.sp

**S8. GOKZ KZTimer mode.** Prestrafe modifier, 380 perf cap, duck and displacement adjustments.  
https://github.com/KZGlobalTeam/gokz/blob/master/addons/sourcemod/scripting/gokz-mode-kztimer.sp

**S9. GOKZ SimpleKZ mode.** 128-tick requirement, custom prestrafe, and adjusted hop behavior.  
https://github.com/KZGlobalTeam/gokz/blob/master/addons/sourcemod/scripting/gokz-mode-simplekz.sp

**S10. Movement Unlocker, original author discussion.** Evidence that unlocking movement entails modifying more than the air-acceleration cvar.  
https://forums.alliedmods.net/showthread.php?t=255298

**S11. zer0k, “CSGO (and CS2) deadstrafe explained.”** Direct researcher explanation tying the effect to CategorizePosition and AirAccelerate.  
https://gist.github.com/zer0k-z/808bc8bfc494e0bbb5a423c2b1ca6685

**S12. Historical command/input definitions (unofficial leak), `usercmd.h` and `in_main.cpp`.** Useful for separating input sampling from movement simulation.  
Unofficial leak of the CS:GO source; not linked.

### Collision research and community tools

**S13. jason-e, RNGFix technical notes and implementation.** Direct technical account of collision/ground-detection differences, edgebugs, trigger behavior, telehops, and fixes.  
https://github.com/jason-e/rngfix/blob/master/tech.md  
https://github.com/jason-e/rngfix/blob/master/plugin/scripting/rngfix.sp

**S14. GAMMACASE, MomSurfFix.** Surf collision replacement/fix implementation.  
https://github.com/GAMMACASE/MomSurfFix  
https://github.com/GAMMACASE/MomSurfFix/blob/master/addons/sourcemod/scripting/momsurffix/gamemovement.sp

**S15. GOKZ jump tracking.** Exact distance and statistics conventions.  
https://github.com/KZGlobalTeam/gokz/blob/master/addons/sourcemod/scripting/gokz-jumpstats/jump_tracking.sp

**S16. HackerPide, Pixurf calculator.** Useful evidence for empirical discrete-height setup research; not a movement-engine source.  
https://github.com/HackerPide/Pixurf  
https://github.com/HackerPide/Pixurf/blob/master/src/pixurf/Pixurf.java

**S17. “Pixel Surf Info.”** Community technical/setup notes. Authoritative attribution and full engine-level validation remain incomplete.  
https://pastebin.com/2FBBgmpH

**S18. Node, player-authored KZ technique catalogue.** Discovery aid for named techniques and examples. Cross-mode claims need independent checking.  
https://www.node-music.com/post/kz_tech

**S19. UdNeedAMiracle, “Mastery of Strafing in CSGO.”** Historical player observations and terminology. Broad practical rules are not substitutes for the acceleration formula.  
https://houseofclimb.com/threads/mastery-of-strafing-in-csgo-a-guide-by-udneedamiracle.871/

**S20. Valve, 28 October 2024 release notes.** Explicit retrospective confirmation of coordinate-dependent CS:GO jump-height variation; the announced fix itself was for CS2.  
https://store.steampowered.com/news/posts/?appids=730&enddate=1730247350&feed=steam_community_announcements

**S21. Historical CS constants and shared movement cvars (unofficial leak), `cs_shareddefs.cpp` and `movevars_shared.cpp`.** Companion definitions for interpreting the movement code.  
Unofficial leak of the CS:GO source; not linked.

## 25. Remaining evidence gaps

The strongest coverage is for acceleration, stamina, jumping, ducking, ordinary collision, surf geometry, and the core KZ bugs. The following remain narrower than a complete final-retail specification:

- Exact mapping from the historical source snapshot to each retail CS:GO build.
- Every later binary change, especially duck and ladder behavior.
- A fully authenticated, current decompilation of final-retail movement.
- Exact primary implementation explanations for every named high-tier KZ technique.
- Coordinate- and BSP-specific pixel-surf and ramp-bug mechanisms.
- A complete player-runboost and projectile-boost model.
- All water, moving-platform, and special-game-mode branches.
- Current CS2 equivalence.

These are explicit boundaries of the evidence, not reasons to discard the well-supported core. The major practical conclusion is that a small set of mathematical operations becomes a large movement system because their **ordering, stored state, collision geometry, and mode-specific modifications** determine when each operation is allowed to act.
