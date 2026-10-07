//! Every promoted real-server capture is replayed on each `cargo test` and must stay within the
//! tolerance it was promoted with (plan §9.6).

use std::path::Path;

use compare::capture::Capture;
use compare::diff::{self, Sync};
use compare::Sidecar;

#[test]
fn promoted_captures_still_match() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/movement/tests/captures");
    let world = compare::world();
    let mut failures = Vec::new();
    let dirs = compare::capture_dirs(&root);
    for dir in &dirs {
        let cap = Capture::load(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
        let side = Sidecar::parse(&std::fs::read_to_string(dir.join("tolerance.toml")).unwrap()).unwrap();
        let r = diff::run(&cap, &world, Sync::Free, 0, side.tol).unwrap();
        if r.verdict.rank() > side.min.rank() {
            failures.push(format!("{}: {} (promoted as {})", cap.name(), r.verdict.label(), side.promoted));
        }
    }
    assert!(failures.is_empty(), "{} of {} captures regressed:\n{}", failures.len(), dirs.len(), failures.join("\n"));
}
