//! Write an episode file for the viewer (`csmove --episode <file>`).
//!
//! ```text
//! cargo run -p env --example episode -- <Stage> <seed> [Touch|Button] [out.episode]
//! ```
//!
//! The episode is the toy course's reference solution run through `Env`, the same path an agent's
//! actions take. Stages: Walk, Gap, GapChain, ShortBlocks, Heights.

use std::sync::Arc;

use course::toy::{generate_with_end, EndKind, Stage};
use env::{Action, CourseSource, Env, EnvConfig, Episode};
use movement::Buttons;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: episode <Stage> <seed> [Touch|Button] [out.episode]";
    let stage = args
        .first()
        .and_then(|s| Stage::ALL.into_iter().find(|x| format!("{x:?}").eq_ignore_ascii_case(s)))
        .unwrap_or_else(|| panic!("{usage}"));
    let seed: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or_else(|| panic!("{usage}"));
    let end = match args.get(2).map(|s| s.to_ascii_lowercase()).as_deref() {
        None | Some("touch") => EndKind::Touch,
        Some("button") => EndKind::Button,
        Some(e) => panic!("unknown end `{e}`; {usage}"),
    };
    let out = args.get(3).cloned().unwrap_or_else(|| format!("{stage:?}_{seed}_{end:?}.episode"));

    let toy = generate_with_end(stage, seed, end);
    let mut env = Env::new(Arc::new(toy.course.clone()), EnvConfig::default()).expect("env");
    env.reset(seed);
    let key = |m: f32| (m > 0.0) as i8 - (m < 0.0) as i8;
    for c in &toy.solution {
        let a = Action {
            forward: key(c.forward_move),
            side: key(c.side_move),
            jump: c.buttons.contains(Buttons::JUMP),
            duck: c.buttons.contains(Buttons::DUCK),
            use_: c.buttons.contains(Buttons::USE),
            ..Action::IDLE
        };
        let s = env.step(a);
        if s.info.terminated || s.info.truncated {
            break;
        }
    }
    let ep = Episode::from_env(&env, CourseSource::Toy { stage, seed, end });
    std::fs::write(&out, ep.to_text()).expect("write episode");
    println!("{out}: {:?} in {} run ticks ({} commands)", ep.status, ep.run_ticks, ep.recording.cmds.len());
}
