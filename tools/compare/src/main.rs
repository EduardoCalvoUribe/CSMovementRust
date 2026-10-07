//! `compare`: the capture-rig CLI (plan ?9.6).
//!
//! ```text
//! compare map <out.vmf>                       export the test level as a Hammer map
//! compare scenarios <dir>                     write the scenario ladder for the server plugin
//! compare import <results> <captures>         turn plugin output into capture folders
//! compare diff <capture> [--reports <dir>]    replay and diff one capture
//! compare all <captures> [--reports <dir>]    diff every capture, summary table, exit 1 on FAILED
//! compare repro <capture-a> <capture-b>       reproducibility check: are two captures identical?
//! compare promote <capture> <tests-dir>       copy a passing capture into the regression suite
//! compare trace <capture> <from>[..<to>]       our routine-level events for those ticks, re-synced
//! ```

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use compare::capture::{self, Capture};
use compare::diff::{self, Sync, Tolerance, Verdict};
use compare::{kv::Kv, scenarios, vmf, Sidecar};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.first().map(String::as_str) {
        Some("map") if args.len() == 2 => cmd_map(Path::new(&args[1])),
        Some("scenarios") if args.len() == 2 => cmd_scenarios(Path::new(&args[1])),
        Some("import") if args.len() == 3 => cmd_import(Path::new(&args[1]), Path::new(&args[2])),
        Some("diff") if args.len() >= 2 => cmd_diff(Path::new(&args[1]), &reports_dir(&args), true).map(|_| ()),
        Some("all") if args.len() >= 2 => cmd_all(Path::new(&args[1]), &reports_dir(&args)),
        Some("repro") if args.len() == 3 => cmd_repro(Path::new(&args[1]), Path::new(&args[2])),
        Some("promote") if args.len() == 3 => cmd_promote(Path::new(&args[1]), Path::new(&args[2])),
        Some("trace") if args.len() == 3 => cmd_trace(Path::new(&args[1]), &args[2]),
        _ => Err("usage: compare map|scenarios|import|diff|all|repro|promote ... (see src/main.rs)".into()),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn reports_dir(args: &[String]) -> PathBuf {
    args.iter()
        .position(|a| a == "--reports")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("reports"))
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

fn cmd_map(out: &Path) -> Result<(), String> {
    write(out, &vmf::generate(&testlevel::describe()))?;
    println!("wrote {}", out.display());
    Ok(())
}

fn cmd_scenarios(dir: &Path) -> Result<(), String> {
    let world = compare::world();
    let all = scenarios::all(&world);
    let mut lists: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for s in &all {
        let name = s.dir_name();
        let d = dir.join(&name);
        write(&d.join("cmds.csv"), &capture::write_cmds(&s.cmds))?;
        let o = s.origin;
        let cfg = format!(
            "name {name}\nid {}\nmode {}\ntickrate {}\norigin {:.6} {:.6} {:.6}\nyaw {:.6}\nsettle {}\ncount {}\nisolates {}\n",
            s.id,
            capture::mode_name(s.mode),
            s.tickrate,
            o.x,
            o.y,
            o.z,
            s.yaw,
            s.settle,
            s.cmds.len(),
            s.isolates
        );
        write(&d.join("scenario.cfg"), &cfg)?;
        lists.entry(format!("list_{}_{}.txt", capture::mode_name(s.mode), s.tickrate)).or_default().push(name);
    }
    for (file, names) in &lists {
        write(&dir.join(file), &(names.join("\n") + "\n"))?;
        println!("{file}: {} scenarios", names.len());
    }
    Ok(())
}

/// Plugin output folders (meta.toml, scenario.toml, states.csv) plus the scenario's cmds.csv become
/// capture folders with the same name.
fn cmd_import(results: &Path, captures: &Path) -> Result<(), String> {
    let hash = capture::level_hash(&testlevel::describe());
    let mut n = 0;
    for dir in compare::capture_dirs(results) {
        let read = |f: &str| std::fs::read_to_string(dir.join(f)).map_err(|e| format!("{}: {e}", dir.join(f).display()));
        let scen = Kv::parse(&read("scenario.toml")?)?;
        let src = results.parent().unwrap_or(results).join("scenarios").join(scen.req("scenario")?);
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let out = captures.join(&name);
        std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        let cmds = std::fs::read_to_string(src.join("cmds.csv")).map_err(|e| format!("{}: {e}", src.display()))?;
        write(&out.join("cmds.csv"), &cmds)?;
        write(&out.join("states.csv"), &read("states.csv")?)?;
        write(&out.join("meta.toml"), &read("meta.toml")?)?;
        write(&out.join("scenario.toml"), &format!("{}level_hash = \"{hash}\"\n", read("scenario.toml")?))?;
        match Capture::load(&out) {
            Ok(_) => n += 1,
            Err(e) => {
                eprintln!("{name}: INVALID ({e}); moved to {name}.invalid");
                let bad = captures.join(format!("{name}.invalid"));
                let _ = std::fs::remove_dir_all(&bad);
                std::fs::rename(&out, &bad).map_err(|e| e.to_string())?;
            }
        }
    }
    println!("imported {n} captures into {}", captures.display());
    Ok(())
}

fn check_geometry(cap: &Capture) -> Result<(), String> {
    let want = capture::level_hash(&testlevel::describe());
    match cap.scenario.get("level_hash") {
        Some(h) if h != want => Err(format!("{}: captured on level {h}, current test level is {want}", cap.name())),
        _ => Ok(()),
    }
}

struct Outcome {
    name: String,
    free: Verdict,
    resync: Verdict,
    events_match: bool,
    first: Option<(u32, &'static str)>,
    ticks: usize,
}

fn cmd_diff(dir: &Path, reports: &Path, print: bool) -> Result<Outcome, String> {
    let cap = Capture::load(dir)?;
    check_geometry(&cap)?;
    let world = compare::world();
    let tol = Tolerance::default();
    let free = diff::run(&cap, &world, Sync::Free, 0, tol)?;
    let resync = diff::run(&cap, &world, Sync::Resync, 0, tol)?;
    // Phase check (plan ?9.7 step 4): does delaying our stream by a tick explain a failure?
    let mut shifted = Vec::new();
    if matches!(free.verdict, Verdict::Failed(_)) {
        shifted.push((1, diff::run(&cap, &world, Sync::Free, 1, tol)?.verdict));
    }
    let text = diff::text_report(&cap, &free, &resync, &shifted);
    if print {
        println!("{text}");
    }
    let base = reports.join(cap.name());
    write(&base.join("report.txt"), &text)?;
    write(&base.join("residuals.csv"), &diff::residuals_csv(&cap, &free))?;
    write(&base.join("residuals_resync.csv"), &diff::residuals_csv(&cap, &resync))?;
    write(&base.join("report.html"), &diff::html_report(&cap, &free, &text))?;
    Ok(Outcome {
        name: cap.name(),
        free: free.verdict,
        resync: resync.verdict,
        events_match: free.events_ours == free.events_theirs,
        first: free.first.map(|(t, c, ..)| (t, c)),
        ticks: cap.cmds.len(),
    })
}

fn cmd_all(root: &Path, reports: &Path) -> Result<(), String> {
    let dirs = compare::capture_dirs(root);
    if dirs.is_empty() {
        return Err(format!("no captures under {}", root.display()));
    }
    let mut rows = Vec::new();
    for d in &dirs {
        match cmd_diff(d, reports, false) {
            Ok(o) => rows.push(o),
            Err(e) => eprintln!("{}: {e}", d.display()),
        }
    }
    let mut md = String::from(
        "| capture | ticks | free-running | re-synced | events | first divergence |\n|---|---|---|---|---|---|\n",
    );
    let mut failed = 0;
    for o in &rows {
        if matches!(o.free, Verdict::Failed(_)) {
            failed += 1;
        }
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            o.name,
            o.ticks,
            o.free.label(),
            o.resync.label(),
            if o.events_match { "same" } else { "DIFFER" },
            o.first.map_or("-".into(), |(t, c)| format!("tick {t} {c}"))
        ));
    }
    write(&reports.join("summary.md"), &md)?;
    println!("{md}\n{} captures, {failed} FAILED. Reports in {}", rows.len(), reports.display());
    if failed > 0 {
        Err(format!("{failed} captures FAILED"))
    } else {
        Ok(())
    }
}

/// Plan ?9.7 step 1: the same scenario captured twice must give identical logs.
fn cmd_repro(a: &Path, b: &Path) -> Result<(), String> {
    let (ca, cb) = (Capture::load(a)?, Capture::load(b)?);
    if ca.cmds != cb.cmds {
        return Err("different command streams".into());
    }
    for k in 0..ca.cmds.len() {
        for (ra, rb, phase) in [(&ca.pre[k], &cb.pre[k], "pre"), (&ca.post[k], &cb.post[k], "post")] {
            let mut x = ra.clone();
            let mut y = rb.clone();
            x.cmdnum = None;
            y.cmdnum = None;
            x.server_tick = None;
            y.server_tick = None;
            if x != y {
                return Err(format!("NOT REPRODUCIBLE: first difference at tick {k} ({phase})\n a: {x:?}\n b: {y:?}"));
            }
        }
    }
    println!("REPRODUCIBLE: {} ticks identical", ca.cmds.len());
    Ok(())
}

fn cmd_promote(dir: &Path, tests: &Path) -> Result<(), String> {
    let cap = Capture::load(dir)?;
    let world = compare::world();
    let tol = Tolerance::default();
    let r = diff::run(&cap, &world, Sync::Free, 0, tol)?;
    if r.verdict.rank() > Verdict::WithinEps.rank() {
        return Err(format!("{}: {} is not good enough to promote (needs WITHIN_EPS or better)", cap.name(), r.verdict.label()));
    }
    let out = tests.join(cap.name());
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    for f in ["meta.toml", "scenario.toml"] {
        std::fs::copy(dir.join(f), out.join(f)).map_err(|e| format!("{f}: {e}"))?;
    }
    // Promoted copies keep only the raw-bit float columns (the loader prefers them anyway): half the size.
    write(&out.join("cmds.csv"), &capture::compact_csv(&std::fs::read_to_string(dir.join("cmds.csv")).map_err(|e| e.to_string())?))?;
    write(&out.join("states.csv"), &capture::compact_csv(&std::fs::read_to_string(dir.join("states.csv")).map_err(|e| e.to_string())?))?;
    let check = Capture::load(&out)?;
    if check.pre != cap.pre || check.post != cap.post || check.cmds != cap.cmds {
        return Err(format!("{}: compact copy does not round-trip", cap.name()));
    }
    let side = Sidecar { min: r.verdict, tol, promoted: r.verdict.label() };
    write(&out.join("tolerance.toml"), &side.to_text())?;
    println!("promoted {} ({}) to {}", cap.name(), r.verdict.label(), out.display());
    Ok(())
}

/// Plan ?9.7 steps 3 and 7: re-sync to the captured start of each tick and print our routine-level
/// events (bumps with fractions and normals, categorization, ducks, jumps) next to the captured result.
fn cmd_trace(dir: &Path, range: &str) -> Result<(), String> {
    use movement::instrument::{EventLog, MoveObserver};
    let cap = Capture::load(dir)?;
    let world = compare::world();
    let (from, to) = match range.split_once("..") {
        Some((a, b)) => (a.parse::<usize>().map_err(|e| e.to_string())?, b.parse::<usize>().map_err(|e| e.to_string())?),
        None => {
            let t = range.parse::<usize>().map_err(|e| e.to_string())?;
            (t, t)
        }
    };
    let cfg = cap.config()?;
    let mut mode = cap.mode()?.create();
    let dt = movement::tick_interval(cap.tickrate()?);
    let mut state = diff::state_from_row(&cap.pre[0], &movement::PlayerState::new(movement::Vec3::ZERO));
    for k in 0..=to.min(cap.cmds.len() - 1) {
        let mut log = EventLog::default();
        if k >= from {
            state = diff::state_from_row(&cap.pre[k], &state);
        }
        let obs: &mut dyn MoveObserver = &mut log;
        movement::process_movement(&cfg, mode.as_mut(), &world, &mut state, &cap.cmds[k], obs, dt);
        if k >= from {
            println!("== tick {k}");
            for l in &log.lines {
                println!("  {l}");
            }
            let r = &cap.post[k];
            println!("  captured: origin {:?} vel {:?} ground {}", r.origin, r.velocity, r.ground);
        }
    }
    Ok(())
}
