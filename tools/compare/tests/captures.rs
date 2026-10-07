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

/// The same captures on the compiled test map loaded through the BSP backend (M9), which must give
/// every capture at least the verdict it was promoted with. The map is built locally by
/// `tools/capture/setup.ps1` (`data/map/csmove_capture.bsp`, not committed); without it this test
/// only says so.
#[test]
fn promoted_captures_match_on_the_compiled_bsp() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let bsp = std::env::var_os("CSMOVE_CAPTURE_BSP")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| manifest.join("../../data/map/csmove_capture.bsp"));
    if !bsp.is_file() {
        eprintln!("skipped: {} not found (build it with tools/capture/setup.ps1)", bsp.display());
        return;
    }
    let world = compare::load_world(Some(&bsp)).unwrap();
    let root = manifest.join("../../crates/movement/tests/captures");
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
    assert!(failures.is_empty(), "{} of {} captures regressed on the BSP:\n{}", failures.len(), dirs.len(), failures.join("\n"));
}
