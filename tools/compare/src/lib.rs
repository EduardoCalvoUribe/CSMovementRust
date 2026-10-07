//! Capture rig tooling (plan §9): test-level map export, the scenario ladder, the capture format, and
//! the comparator. The `movement` crate stays free of I/O; everything that touches files lives here.

pub mod capture;
pub mod diff;
pub mod kv;
pub mod scenarios;
pub mod vmf;

use std::path::{Path, PathBuf};

/// Tolerances stored next to a promoted capture (plan §9.6 "promotion to regression tests").
#[derive(Clone, Debug)]
pub struct Sidecar {
    /// Worst verdict the regression test accepts.
    pub min: diff::Verdict,
    pub tol: diff::Tolerance,
    /// Verdict at promotion time, for the record.
    pub promoted: String,
}

impl Sidecar {
    pub fn parse(text: &str) -> Result<Sidecar, String> {
        let kv = kv::Kv::parse(text)?;
        Ok(Sidecar {
            min: diff::Verdict::parse_min(kv.req("min_verdict")?)?,
            tol: diff::Tolerance { max_ulp: kv.u32("max_ulp")?, eps: kv.f32("eps")? as f64 },
            promoted: kv.get("promoted_verdict").unwrap_or("").to_string(),
        })
    }

    pub fn to_text(&self) -> String {
        let min = match self.min {
            diff::Verdict::BitExact => "BIT_EXACT",
            diff::Verdict::WithinUlp => "WITHIN_ULP",
            _ => "WITHIN_EPS",
        };
        format!(
            "# Regression tolerance for this capture (plan §9.6).\nmin_verdict = \"{min}\"\nmax_ulp = {}\neps = {}\npromoted_verdict = \"{}\"\n",
            self.tol.max_ulp, self.tol.eps, self.promoted
        )
    }
}

/// The test level's collision world, which every capture runs on.
pub fn world() -> movement::PrimitiveWorld {
    testlevel::build_world(&testlevel::describe())
}

/// Capture folders directly under `root` (folders holding a `states.csv`).
pub fn capture_dirs(root: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(root)
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.join("states.csv").is_file()).collect())
        .unwrap_or_default();
    v.sort();
    v
}
