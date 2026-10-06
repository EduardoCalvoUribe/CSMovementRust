//! M5 gate (automated part): headless replay of a recorded command stream equals the live run, bit for
//! bit; and golden scenarios (plan §11).

mod common;
use common::*;
use movement::replay::{state_fields, Recording};
use movement::script::Script;
use movement::*;
use std::path::PathBuf;

fn varied_script() -> Vec<UserCmd> {
    Script::parse(
        "look 0 0
         W 60
         WJ 1
         A 20 yaw 1.3
         D 20 yaw -1.3
         - 10
         C 12
         CJ 1
         - 40
         WH 30
         J 1
         J 30
         - 20",
    )
    .unwrap()
    .build()
}

#[test]
fn replay_matches_live_bit_for_bit() {
    for kind in [ModeKind::Vanilla, ModeKind::KzTimer, ModeKind::SimpleKz] {
        let tickrate = if kind == ModeKind::SimpleKz { 128 } else { 64 };
        let mut live = Sim::flat(kind, tickrate);
        let start = live.state.clone();
        let mut rec = Recording::new(kind, tickrate, start.clone());
        // Autobhop is config, so it must survive the round trip through the file.
        if kind == ModeKind::KzTimer {
            live.cfg.autobhop = true;
            rec.autobhop = true;
        }
        let mut live_states = Vec::new();
        for c in varied_script() {
            live.step(c);
            let mut c = c;
            c.tick = live.tick - 1;
            rec.cmds.push(c);
            live_states.push(live.state.clone());
        }
        // Through the text format, so the export is lossless too.
        let rec = Recording::from_text(&rec.to_text()).unwrap();
        assert_eq!(rec.start, start);
        let mut replayed = Vec::new();
        let world = PrimitiveWorld::flat_floor(0.0);
        let mut det = TechniqueDetector::default();
        rec.replay_with(&world, &mut det, |s| replayed.push(s.clone()));
        assert_eq!(replayed.len(), live_states.len());
        for (i, (a, b)) in replayed.iter().zip(&live_states).enumerate() {
            assert_eq!(a, b, "{kind:?} diverged at command {i}");
            assert_eq!(state_fields(a), state_fields(b));
        }
    }
}

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("golden")
}

/// Each `tests/golden/<name>.script` runs on a flat floor (Vanilla, 64 tick) and must end in the state
/// stored in `<name>.state`. Set `UPDATE_GOLDEN=1` to rewrite the expected files after an intended change.
#[test]
fn golden_scenarios() {
    let update = std::env::var("UPDATE_GOLDEN").is_ok();
    let mut checked = 0;
    for entry in std::fs::read_dir(golden_dir()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("script") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let mut header = text.lines().next().unwrap_or("").trim_start_matches('#').split_whitespace();
        let kind = match header.next() {
            Some("kztimer") => ModeKind::KzTimer,
            Some("simplekz") => ModeKind::SimpleKz,
            _ => ModeKind::Vanilla,
        };
        let tickrate: u32 = header.next().and_then(|t| t.parse().ok()).unwrap_or(64);
        let cmds = movement::script::Script::parse(&text).unwrap().build();
        let mut s = Sim::flat(kind, tickrate);
        for c in cmds {
            s.step(c);
        }
        let got = state_fields(&s.state).join(" ");
        let expected_path = path.with_extension("state");
        if update || !expected_path.exists() {
            std::fs::write(&expected_path, format!("{got}\n")).unwrap();
        } else {
            let want = std::fs::read_to_string(&expected_path).unwrap();
            assert_eq!(got, want.trim(), "golden mismatch for {}", path.display());
        }
        checked += 1;
    }
    assert!(checked >= 4);
}
