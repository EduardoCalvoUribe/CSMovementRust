//! R8 replay gate (docs/reinforcement-plan.md §11): an exported episode, through its text form, rebuilds
//! its course and replays to the same outcome and final state bit for bit; tampering is detected.

use std::sync::Arc;

use course::toy::{generate_with_end, EndKind, Stage};
use course::Status;
use env::{Action, CourseSource, Env, EnvConfig, Episode};
use movement::Buttons;

fn key(m: f32) -> i8 {
    (m > 0.0) as i8 - (m < 0.0) as i8
}

/// Run `stage`/`seed`/`end` with the reference solution, or with LCG actions if `random` is set.
fn episode(stage: Stage, seed: u64, end: EndKind, random: bool) -> Episode {
    let toy = generate_with_end(stage, seed, end);
    let mut env = Env::new(Arc::new(toy.course.clone()), EnvConfig { max_episode_ticks: 2000, ..EnvConfig::default() }).unwrap();
    env.reset(seed);
    let mut x = seed.wrapping_add(99);
    let mut next = move || {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (x >> 33) as u32
    };
    for i in 0.. {
        let a = if random {
            let r = next();
            Action {
                forward: if r % 6 == 0 { 0 } else { 1 },
                side: (r / 6 % 3) as i8 - 1,
                jump: r / 18 % 7 == 0,
                duck: r / 126 % 11 == 0,
                use_: r / 1386 % 3 == 0,
                yaw_delta: (next() % 401) as f32 / 100.0 - 2.0,
                pitch_delta: (next() % 101) as f32 / 100.0 - 0.5,
            }
        } else {
            let Some(c) = toy.solution.get(i) else { break };
            Action {
                forward: key(c.forward_move),
                side: key(c.side_move),
                jump: c.buttons.contains(Buttons::JUMP),
                use_: c.buttons.contains(Buttons::USE),
                ..Action::IDLE
            }
        };
        let s = env.step(a);
        if s.info.terminated || s.info.truncated {
            break;
        }
    }
    Episode::from_env(&env, CourseSource::Toy { stage, seed, end })
}

#[test]
fn exported_episodes_replay_bit_for_bit_through_text() {
    let mut outcomes = std::collections::HashSet::new();
    for stage in Stage::ALL {
        for end in [EndKind::Touch, EndKind::Button] {
            for seed in 0..6 {
                for random in [false, true] {
                    let ep = episode(stage, seed, end, random);
                    let back = Episode::from_text(&ep.to_text()).unwrap();
                    assert_eq!(back, ep);
                    back.verify().unwrap_or_else(|e| panic!("{stage:?} {end:?} {seed} random={random}: {e}"));
                    if !random {
                        assert_eq!(ep.status, Status::Completed);
                    }
                    outcomes.insert(format!("{:?}", ep.status));
                }
            }
        }
    }
    // Random play also produced failures, so non-completing episodes are covered too.
    assert!(outcomes.contains("OutOfBounds"), "{outcomes:?}");
}

#[test]
fn tampering_is_detected() {
    let ep = episode(Stage::GapChain, 1, EndKind::Touch, false);
    let mut bad = ep.clone();
    let i = bad.recording.cmds.len() / 2;
    // A one-tick strafe in the air can legitimately change nothing (air wish speed cap), so tamper with
    // something that must matter: 20 ticks of back instead of forward.
    for c in &mut bad.recording.cmds[i..i + 20] {
        c.forward_move = -450.0;
    }
    assert!(bad.verify().is_err());
    let mut bad = ep.clone();
    bad.run_ticks += 1;
    assert!(bad.verify().is_err());
    let mut bad = ep;
    bad.source = CourseSource::Toy { stage: Stage::GapChain, seed: 2, end: EndKind::Touch };
    assert!(bad.verify().is_err());
}

#[test]
fn malformed_files_are_errors() {
    let ep = episode(Stage::Walk, 0, EndKind::Touch, false).to_text();
    assert!(Episode::from_text("csmove-replay 1\n").is_err());
    assert!(Episode::from_text(&ep.replace("course toy Walk", "course toy Nope")).is_err());
    assert!(Episode::from_text(&ep.replace("csmove-episode 1", "csmove-episode 2")).is_err());
    assert!(Episode::from_text(&ep.replacen("outcome", "outcomes", 1)).is_err());
}
