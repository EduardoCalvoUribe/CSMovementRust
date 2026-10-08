//! `compare`: the capture-rig CLI (plan ?9.6).
//!
//! ```text
//! compare map <out.vmf>                       export the test level as a Hammer map
//! compare scenarios <dir>                     write the scenario ladder for the server plugin
//!   `--bsp <map.bsp> --lj x,y,z,yaw ...` writes long-jump scenarios on that map instead (M9 gate)
//! compare import <results> <captures>         turn plugin output into capture folders
//! compare diff <capture> [--reports <dir>]    replay and diff one capture
//! compare all <captures> [--reports <dir>]    diff every capture, summary table, exit 1 on FAILED
//!   (diff and all take `--bsp <csmove_capture.bsp>` to replay on the compiled map via the BSP backend)
//! compare repro <capture-a> <capture-b>       reproducibility check: are two captures identical?
//! compare promote <capture> <tests-dir>       copy a passing capture into the regression suite
//! compare trace <capture> <from>[..<to>]       our routine-level events for those ticks, re-synced
//! compare bsp <map.bsp>...                     load maps, list spawns, run from each spawn, time traces
//! compare jumps <captures> [--bsp <map.bsp>]  our jump stats for each capture: our replay, and our
//!                                             measurement of the captured trajectory
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
        Some("scenarios") if args.len() >= 2 => cmd_scenarios(Path::new(&args[1]), &args),
        Some("import") if args.len() == 3 => cmd_import(Path::new(&args[1]), Path::new(&args[2])),
        Some("diff") if args.len() >= 2 => {
            replay_world(&args).and_then(|w| cmd_diff(Path::new(&args[1]), &reports_dir(&args), &w, true)).map(|_| ())
        }
        Some("all") if args.len() >= 2 => replay_world(&args).and_then(|w| cmd_all(Path::new(&args[1]), &reports_dir(&args), &w)),
        Some("repro") if args.len() == 3 => cmd_repro(Path::new(&args[1]), Path::new(&args[2])),
        Some("promote") if args.len() == 3 => cmd_promote(Path::new(&args[1]), Path::new(&args[2])),
        Some("trace") if args.len() == 3 => cmd_trace(Path::new(&args[1]), &args[2]),
        Some("jumps") if args.len() >= 2 => replay_world(&args).and_then(|w| cmd_jumps(Path::new(&args[1]), &w)),
        Some("bsp") if args.len() >= 2 => args[1..].iter().try_for_each(|m| cmd_bsp(Path::new(m))),
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

/// The world captures replay on, and the BSP map's name when `--bsp` gave one.
struct ReplayWorld {
    world: movement::world::World,
    bsp: Option<String>,
}

fn replay_world(args: &[String]) -> Result<ReplayWorld, String> {
    let bsp = args.iter().position(|a| a == "--bsp").and_then(|i| args.get(i + 1)).map(Path::new);
    Ok(ReplayWorld {
        world: compare::load_world(bsp)?,
        bsp: bsp.and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()),
    })
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

fn cmd_scenarios(dir: &Path, args: &[String]) -> Result<(), String> {
    let bsp = args.iter().position(|a| a == "--bsp").and_then(|i| args.get(i + 1)).map(Path::new);
    let all = match bsp {
        None => scenarios::all(&compare::world()),
        Some(p) => {
            let world = compare::load_world(Some(p))?;
            let name = p.file_stem().ok_or("map path has no file name")?.to_string_lossy().into_owned();
            let mut starts = Vec::new();
            for (i, a) in args.iter().enumerate() {
                if a == "--lj" {
                    let v: Vec<f32> = args
                        .get(i + 1)
                        .ok_or("--lj needs x,y,z,yaw")?
                        .split(',')
                        .map(|t| t.trim().parse::<f32>().map_err(|e| format!("--lj `{t}`: {e}")))
                        .collect::<Result<_, _>>()?;
                    if v.len() != 4 {
                        return Err("--lj needs x,y,z,yaw".into());
                    }
                    starts.push((movement::Vec3::new(v[0], v[1], v[2]), v[3]));
                }
            }
            if starts.is_empty() {
                return Err("--bsp needs at least one --lj x,y,z,yaw runway start".into());
            }
            scenarios::long_jumps(&world, &name, &starts)
        }
    };
    let mut lists: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for s in &all {
        let name = s.dir_name();
        let d = dir.join(&name);
        write(&d.join("cmds.csv"), &capture::write_cmds(&s.cmds))?;
        let o = s.origin;
        let mut cfg = format!(
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
        if let Some(m) = &s.map {
            cfg.push_str(&format!("map {m}
"));
        }
        write(&d.join("scenario.cfg"), &cfg)?;
        let prefix = s.map.as_ref().map_or(String::new(), |m| format!("{m}_"));
        lists.entry(format!("list_{prefix}{}_{}.txt", capture::mode_name(s.mode), s.tickrate)).or_default().push(name);
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
        // The level hash identifies the test level; captures on other maps are identified by map name.
        let on_test_map = Kv::parse(&read("meta.toml")?)?.get("map").is_none_or(|m| m == "csmove_capture");
        let scen_text = read("scenario.toml")?;
        if on_test_map {
            write(&out.join("scenario.toml"), &format!("{scen_text}level_hash = \"{hash}\"\n"))?;
        } else {
            write(&out.join("scenario.toml"), &scen_text)?;
        }
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

/// Earlier test levels whose captures still replay on the current one: it only adds brushes away from
/// every scenario's path (`bcbe1118972ec86f`: before the pixelsurf lanes; `74f75ae5a752549f`: before
/// the top-down slab lane).
const EARLIER_LEVELS: [&str; 2] = ["bcbe1118972ec86f", "74f75ae5a752549f"];

/// Captures must replay on the level they were made on: the test level (hash-checked), or a BSP map
/// given with `--bsp` whose file name matches the captured map name.
fn check_geometry(cap: &Capture, bsp: Option<&str>) -> Result<(), String> {
    let map = cap.meta.get("map").unwrap_or("csmove_capture");
    if map != "csmove_capture" || bsp.is_some_and(|b| b != "csmove_capture") {
        return match bsp {
            Some(b) if b == map => Ok(()),
            Some(b) => Err(format!("{}: captured on {map}, replaying on {b}", cap.name())),
            None => Err(format!("{}: captured on {map}; pass --bsp with that map", cap.name())),
        };
    }
    let want = capture::level_hash(&testlevel::describe());
    match cap.scenario.get("level_hash") {
        Some(h) if h != want && !EARLIER_LEVELS.contains(&h) => Err(format!("{}: captured on level {h}, current test level is {want}", cap.name())),
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

fn cmd_diff(dir: &Path, reports: &Path, rw: &ReplayWorld, print: bool) -> Result<Outcome, String> {
    let cap = Capture::load(dir)?;
    check_geometry(&cap, rw.bsp.as_deref())?;
    let world = &rw.world;
    let tol = Tolerance::default();
    let free = diff::run(&cap, world, Sync::Free, 0, tol)?;
    let resync = diff::run(&cap, world, Sync::Resync, 0, tol)?;
    // Phase check (plan ?9.7 step 4): does delaying our stream by a tick explain a failure?
    let mut shifted = Vec::new();
    if matches!(free.verdict, Verdict::Failed(_)) {
        shifted.push((1, diff::run(&cap, world, Sync::Free, 1, tol)?.verdict));
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

fn cmd_all(root: &Path, reports: &Path, world: &ReplayWorld) -> Result<(), String> {
    let dirs = compare::capture_dirs(root);
    if dirs.is_empty() {
        return Err(format!("no captures under {}", root.display()));
    }
    let mut rows = Vec::new();
    for d in &dirs {
        match cmd_diff(d, reports, world, false) {
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
    state.duck_speed_anchor = state.origin;
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

/// M9 smoke check on a real map: parse it, then from every spawn run forward and jump for five seconds
/// at 128 tick, reporting where the player ended up and how long the simulation took.
fn cmd_bsp(path: &Path) -> Result<(), String> {
    use movement::cmd::{Buttons, UserCmd};
    use movement::world::BspMap;
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let t0 = std::time::Instant::now();
    let map = BspMap::parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let parse = t0.elapsed();
    let spawns = map.spawn_points();
    println!(
        "{}: parsed in {parse:.1?}: {} faces, {} ladders, {} entities, {} triggers, {} spawns",
        path.display(),
        map.faces.len(),
        map.ladders.len(),
        map.entities.len(),
        map.triggers.len(),
        spawns.len()
    );
    let world: movement::world::World = map.world.into();
    let cfg = movement::MovementConfig::default();
    let dt = movement::tick_interval(128);
    let t1 = std::time::Instant::now();
    let mut cmds = 0;
    let mut grounded = 0;
    for (name, origin, yaw) in spawns.iter().take(8) {
        let mut mode = movement::ModeKind::Vanilla.create();
        let mut s = movement::PlayerState::new(*origin);
        let mut on = 0;
        for k in 0..640u32 {
            let b = if k % 96 == 95 { Buttons::FORWARD | Buttons::JUMP } else { Buttons::FORWARD };
            let c = UserCmd::from_buttons(k, movement::Vec3::new(0.0, *yaw + (k as f32 * 0.2), 0.0), b);
            movement::process_movement(&cfg, mode.as_mut(), &world, &mut s, &c, &mut movement::NullObserver, dt);
            on += s.on_ground() as u32;
            cmds += 1;
        }
        grounded += (on > 320) as u32;
        let o = s.origin;
        println!("  {name:<28} from {:>8.1} {:>8.1} {:>7.1} to {:>8.1} {:>8.1} {:>7.1}, grounded {on}/640", origin.x, origin.y, origin.z, o.x, o.y, o.z);
    }
    let el = t1.elapsed();
    println!("  {cmds} commands in {el:.1?} ({:.1} us each), {grounded} runs mostly grounded", el.as_secs_f64() * 1e6 / cmds.max(1) as f64);
    Ok(())
}

/// Jump stats (M9 gate): for each capture, the jumps our tracker reports on our own replay of the
/// commands, and on the captured (real-game) states, to set against the game's jumpstats output.
fn cmd_jumps(root: &Path, rw: &ReplayWorld) -> Result<(), String> {
    use movement::jumpstats::JumpTracker;
    use movement::{process_movement, tick_interval, TechniqueDetector, TechniqueFlags};
    let dirs = compare::capture_dirs(root);
    for d in &dirs {
        let cap = Capture::load(d)?;
        check_geometry(&cap, rw.bsp.as_deref())?;
        let cfg = cap.config()?;
        let mut mode = cap.mode()?.create();
        let dt = tick_interval(cap.tickrate()?);
        let world: &dyn movement::TraceWorld = &rw.world;
        let mut state = diff::state_from_row(&cap.pre[0], &movement::PlayerState::new(movement::Vec3::ZERO));
        state.duck_speed_anchor = state.origin;
        let (mut ours, mut theirs) = (JumpTracker::new(), JumpTracker::new());
        let mut det = TechniqueDetector::default();
        let mut lines = Vec::new();
        for k in 0..cap.cmds.len() {
            let before = state.clone();
            process_movement(&cfg, mode.as_mut(), world, &mut state, &cap.cmds[k], &mut det, dt);
            if let Some(r) = ours.tick(world, &before, &state, &cap.cmds[k], det.last, cfg.gravity, dt) {
                lines.push(format!("  ours   tick {k:>4}: {} {:.4} (pre {:.2}, max {:.2}, {} strafes, sync {:.1}%)", r.jump_type.short(), r.distance, r.pre_speed, r.max_speed, r.strafes.len(), r.sync));
            }
            let b = diff::state_from_row(&cap.pre[k], &before);
            let a = diff::state_from_row(&cap.post[k], &b);
            // The capture has no routine-level events; a takeoff on a command holding jump is a jump.
            let jumped = b.on_ground() && !a.on_ground() && cap.cmds[k].buttons.contains(movement::cmd::Buttons::JUMP);
            let flags = TechniqueFlags { jumped, ..Default::default() };
            if let Some(r) = theirs.tick(world, &b, &a, &cap.cmds[k], flags, cfg.gravity, dt) {
                lines.push(format!("  theirs tick {k:>4}: {} {:.4} (pre {:.2}, max {:.2})", r.jump_type.short(), r.distance, r.pre_speed, r.max_speed));
            }
        }
        println!("{}", cap.name());
        for l in lines {
            println!("{l}");
        }
    }
    Ok(())
}
