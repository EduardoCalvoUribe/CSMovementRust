//! `MovementMode::fork` (docs/reinforcement-plan.md §9, R2 gate): continuing a forked mode equals
//! continuing the original, for every mode, and the fork is independent of the original afterwards.

use movement::replay::state_fields;
use movement::trace::DIST_EPSILON;
use movement::world::{Brush, PrimitiveWorld};
use movement::{process_movement, tick_interval, Buttons, ModeKind, MovementMode, NullObserver, PlayerState, UserCmd, Vec3};

const MODES: [ModeKind; 3] = [ModeKind::Vanilla, ModeKind::KzTimer, ModeKind::SimpleKz];

fn world() -> PrimitiveWorld {
    let mut w = PrimitiveWorld::flat_floor(0.0);
    w.add(Brush::cuboid(Vec3::new(150.0, -100.0, 0.0), Vec3::new(250.0, 100.0, 40.0)));
    w
}

/// Strafing, turning, jumping, and ducking commands from a 64-bit LCG, so mode state (prestrafe, perf
/// tracking, turning flags) is live at any cut point.
fn cmds(seed: u64, n: u32) -> Vec<UserCmd> {
    let mut x = seed;
    let mut next = move || {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (x >> 33) as u32
    };
    let mut yaw = 0.0f32;
    (0..n)
        .map(|tick| {
            let r = next();
            let mut b = Buttons::FORWARD;
            b.insert(if (tick / 40) % 2 == 0 { Buttons::RIGHT } else { Buttons::LEFT });
            if r % 37 == 0 {
                b.insert(Buttons::JUMP);
            }
            if r % 53 == 0 {
                b.insert(Buttons::DUCK);
            }
            yaw += if (tick / 40) % 2 == 0 { -1.5 } else { 1.5 } + (next() % 100) as f32 / 100.0 - 0.5;
            UserCmd::from_buttons(tick, Vec3::new(0.0, yaw, 0.0), b)
        })
        .collect()
}

fn tickrate(kind: ModeKind) -> u32 {
    kind.create().required_tickrate().unwrap_or(128)
}

fn run(
    kind: ModeKind,
    mode: &mut dyn MovementMode,
    state: &mut PlayerState,
    cmds: &[UserCmd],
    w: &PrimitiveWorld,
) -> Vec<Vec<String>> {
    let cfg = *mode.config();
    let dt = tick_interval(tickrate(kind));
    cmds.iter()
        .map(|c| {
            process_movement(&cfg, mode, w, state, c, &mut NullObserver, dt);
            state_fields(state)
        })
        .collect()
}

#[test]
fn forked_mode_continues_like_the_original() {
    let w = world();
    let all = cmds(11, 1200);
    for kind in MODES {
        for cut in [1usize, 137, 600, 1199] {
            let spawn = PlayerState::new(Vec3::new(0.0, 0.0, DIST_EPSILON));
            let reference = run(kind, kind.create().as_mut(), &mut spawn.clone(), &all, &w);

            let mut mode = kind.create();
            let mut state = spawn.clone();
            run(kind, mode.as_mut(), &mut state, &all[..cut], &w);
            let mut forked = mode.fork();
            let forked_state = state.clone();
            assert_eq!(forked.kind(), kind);

            // Perturb the original after the fork; the fork must not see it.
            run(kind, mode.as_mut(), &mut state, &cmds(99, 300), &w);

            let rest = run(kind, forked.as_mut(), &mut forked_state.clone(), &all[cut..], &w);
            assert_eq!(rest, reference[cut..], "{kind:?} cut at {cut}");
        }
    }
}

/// The gate above only means something if mode state matters at the cut: resuming the KZ modes with a
/// fresh mode instead of a fork must diverge.
#[test]
fn fresh_mode_at_the_cut_diverges_for_kz_modes() {
    let w = world();
    let all = cmds(11, 1200);
    for kind in [ModeKind::KzTimer, ModeKind::SimpleKz] {
        let spawn = PlayerState::new(Vec3::new(0.0, 0.0, DIST_EPSILON));
        let reference = run(kind, kind.create().as_mut(), &mut spawn.clone(), &all, &w);
        let cut = 600;
        let mut state = spawn.clone();
        run(kind, kind.create().as_mut(), &mut state, &all[..cut], &w);
        let rest = run(kind, kind.create().as_mut(), &mut state, &all[cut..], &w);
        assert_ne!(rest, reference[cut..], "{kind:?}: mode state had no effect, so the fork test is weak");
    }
}
